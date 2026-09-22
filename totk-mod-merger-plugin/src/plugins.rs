//! Loading the Skyline plugins that mods ship.
//!
//! A mod that needs code of its own puts a `plugin.nro` in its folder (or
//! several in a `plugins` folder). Once the mods are merged and served, the
//! ones the profile turned on get loaded here, in merge order, the same way
//! skyline-totk loads the plugins in its own folder: read, register through
//! `nn::ro`, then run `main`.
//!
//! This is what lets code mods coexist. A console has room for one exefs
//! (Atmosphère's `atmosphere/contents/<title>/exefs`), and Skyline already
//! uses it, so two mods that each replace `subsdk9` cannot both be installed;
//! as plugins they simply load one after the other.
//!
//! Plugins are loaded after the merge, which means they cannot take part in it
//! (a plugin that chooses which mods are merged through the `tkm_*` API still
//! belongs in `skyline/plugins`, where it is loaded before the merge). What
//! they can do is everything a Skyline plugin does: hook the game, read the
//! merged files through nn::fs, patch code in memory.

use std::sync::atomic::{AtomicUsize, Ordering};

use skyline::nn;

use totk_merge::config::Config;
use totk_merge::mods::{Plan, PLUGINS_DIR};
use totk_merge::sys::path;

use crate::log;

extern "C" {
    /// The game's own allocator: `nn::ro` maps the NRO out of this buffer, so
    /// it has to be page aligned and stay put for good.
    fn memalign(alignment: usize, size: usize) -> *mut u8;
}

const PAGE: usize = 0x1000;
/// "NRR0"
const NRR_MAGIC: u32 = 0x3052_524E;
/// nn::ro::NrrHeader::Type, "ForSelf".
const NRR_TYPE_FOR_SELF: u8 = 0;
/// Resolve every symbol at load time, so a plugin that is missing one fails
/// here rather than in the middle of the game.
const BIND_NOW: i32 = 1;

/// The mod whose `main` is running, for [`crate::api::tkm_current_mod_dir`].
/// A `Vec` that is never freed backs the strings, so the pointers stay valid.
static CURRENT_DIR: AtomicUsize = AtomicUsize::new(0);
static CURRENT_NAME: AtomicUsize = AtomicUsize::new(0);

/// A NUL terminated copy of `text`, leaked: it is handed to plugins as a
/// `const char*`.
fn leak_c_string(text: &str) -> usize {
    let mut bytes = text.as_bytes().to_vec();
    bytes.push(0);
    let pointer = bytes.as_ptr() as usize;
    core::mem::forget(bytes);
    pointer
}

/// The folder of the mod whose plugin is running, or 0.
pub fn current_mod_dir() -> usize {
    CURRENT_DIR.load(Ordering::Acquire)
}

pub fn current_mod_name() -> usize {
    CURRENT_NAME.load(Ordering::Acquire)
}

struct Found {
    /// Name of the mod that ships it.
    mod_name: String,
    /// The mod's folder, which is what the plugin gets told about itself.
    mod_dir: String,
    path: String,
}

/// The plugins of every mod in the plan, in merge order (the mod that wins
/// conflicts last). A mod that turned its own plugins off through `mod.ini` is
/// not in the plan's plugin lists to begin with.
fn wanted(plan: &Plan) -> Vec<Found> {
    let mut found = Vec::new();
    for spec in &plan.mods {
        for file in &spec.plugins {
            let dir = path::parent(file).unwrap_or("");
            let mod_dir = match path::file_name(dir).eq_ignore_ascii_case(PLUGINS_DIR) {
                true => path::parent(dir).unwrap_or(dir),
                false => dir,
            };
            found.push(Found {
                mod_name: spec.name.clone(),
                mod_dir: mod_dir.to_string(),
                path: file.clone(),
            });
        }
    }
    found
}

/// A plugin read into memory, ready to be registered and loaded.
struct Loaded {
    found: Found,
    data: *mut u8,
    bss: *mut u8,
    bss_size: usize,
    hash: [u8; 32],
}

/// Reads an NRO into page aligned memory of its own and hashes it, the way
/// `nn::ro` wants it.
fn read(found: Found) -> Option<Loaded> {
    let file = match std::fs::read(&found.path) {
        Ok(data) => data,
        Err(error) => {
            log::write(&format!("could not read {}: {}", found.path, error));
            return None;
        }
    };
    // An NRO starts with a branch, then "NRO0" at 0x10; nn::ro checks the rest.
    if file.len() < 0x80 || &file[0x10..0x14] != b"NRO0" {
        log::write(&format!("{} is not an NRO, skipped", found.path));
        return None;
    }

    let data = unsafe { memalign(PAGE, (file.len() + PAGE - 1) & !(PAGE - 1)) };
    if data.is_null() {
        log::write(&format!("not enough memory to load {}", found.path));
        return None;
    }
    unsafe { core::ptr::copy_nonoverlapping(file.as_ptr(), data, file.len()) };

    let mut needed: u64 = 0;
    let result = unsafe { nn::ro::GetBufferSize(&mut needed, data) };
    if result != 0 {
        log::write(&format!("nn::ro refused {} ({:#x}), skipped", found.path, result));
        return None;
    }
    let bss_size = needed as usize;
    let bss = unsafe { memalign(PAGE, bss_size.max(PAGE)) };
    if bss.is_null() {
        log::write(&format!("not enough memory for the data of {}", found.path));
        return None;
    }

    // Over the NRO as its header describes it, which is what the NRR has to
    // hold for ro to accept the module.
    let described = u32::from_le_bytes([
        unsafe { *data.add(0x18) },
        unsafe { *data.add(0x19) },
        unsafe { *data.add(0x1a) },
        unsafe { *data.add(0x1b) },
    ]) as usize;
    let hashed = described.min(file.len());
    let mut hash = [0u8; 32];
    unsafe {
        nn::crypto::GenerateSha256Hash(hash.as_mut_ptr(), hash.len() as u64, data, hashed as u64)
    };

    Some(Loaded {
        found,
        data,
        bss,
        bss_size,
        hash,
    })
}

/// Registers the hashes of every plugin with `ldr:ro`, which only maps modules
/// a program said beforehand it would load.
fn register(plugins: &[Loaded]) -> bool {
    let size = (core::mem::size_of::<nn::ro::NrrHeader>() + plugins.len() * 32 + PAGE - 1) & !(PAGE - 1);
    let buffer = unsafe { memalign(PAGE, size) };
    if buffer.is_null() {
        log::write("not enough memory to register the plugins");
        return false;
    }
    unsafe { core::ptr::write_bytes(buffer, 0, size) };

    let offset = core::mem::size_of::<nn::ro::NrrHeader>() as u32;
    unsafe {
        let header = buffer as *mut nn::ro::NrrHeader;
        (*header).magic = NRR_MAGIC;
        (*header).program_id = nn::ro::ProgramId {
            value: skyline::info::get_program_id(),
        };
        (*header).size = size as u32;
        (*header).type_ = NRR_TYPE_FOR_SELF;
        (*header).hashes_offset = offset;
        (*header).num_hashes = plugins.len() as u32;
    }

    // ro wants them sorted, and checks each module against the list.
    let mut hashes: Vec<[u8; 32]> = plugins.iter().map(|plugin| plugin.hash).collect();
    hashes.sort();
    for (index, hash) in hashes.iter().enumerate() {
        unsafe { core::ptr::copy_nonoverlapping(hash.as_ptr(), buffer.add(offset as usize + index * 32), 32) };
    }

    // The registration outlives the modules, so it is never given back.
    let registration = Box::leak(Box::new(nn::ro::RegistrationInfo {
        state: 0,
        nrrPtr: core::ptr::null_mut(),
        _x10: 0,
        _x18: 0,
    }));
    let result = unsafe { nn::ro::RegisterModuleInfo(registration, buffer) };
    if result != 0 {
        log::write(&format!("could not register the plugins with nn::ro ({:#x})", result));
        return false;
    }
    true
}

/// Loads and runs the plugins the mods of `plan` ship. Returns how many ran.
pub fn load(config: &Config, plan: &Plan) -> usize {
    let found = wanted(plan);
    if found.is_empty() {
        return 0;
    }
    if !config.mod_plugins {
        log::write(&format!(
            "{} mod plugin(s) left out: mod_plugins = 0 in {}",
            found.len(),
            totk_merge::config::CONFIG_PATH
        ));
        return 0;
    }

    let mut plugins: Vec<Loaded> = Vec::new();
    for entry in found {
        let Some(plugin) = read(entry) else {
            continue;
        };
        // The same plugin in two mods (a shared library, a mod installed
        // twice) would be refused by ro the second time.
        if plugins.iter().any(|other| other.hash == plugin.hash) {
            log::write(&format!(
                "'{}' ships {}, which is already loaded",
                plugin.found.mod_name,
                path::file_name(&plugin.found.path)
            ));
            continue;
        }
        plugins.push(plugin);
    }
    if plugins.is_empty() || !register(&plugins) {
        return 0;
    }

    let mut ran = 0;
    for plugin in &plugins {
        let module = Box::leak(Box::new(unsafe { core::mem::zeroed::<nn::ro::Module>() }));
        let result = unsafe {
            nn::ro::LoadModule(module, plugin.data, plugin.bss, plugin.bss_size as u64, BIND_NOW)
        };
        if result != 0 {
            log::write(&format!(
                "could not load {} of '{}' ({:#x})",
                path::file_name(&plugin.found.path),
                plugin.found.mod_name,
                result
            ));
            continue;
        }

        let mut entry: usize = 0;
        let looked_up = unsafe { nn::ro::LookupModuleSymbol(&mut entry, module, b"main\0".as_ptr()) };
        if looked_up != 0 || entry == 0 {
            log::write(&format!(
                "{} of '{}' has no main, loaded all the same",
                path::file_name(&plugin.found.path),
                plugin.found.mod_name
            ));
            continue;
        }

        log::write(&format!(
            "running {} of '{}'",
            path::file_name(&plugin.found.path),
            plugin.found.mod_name
        ));
        CURRENT_DIR.store(leak_c_string(&plugin.found.mod_dir), Ordering::Release);
        CURRENT_NAME.store(leak_c_string(&plugin.found.mod_name), Ordering::Release);
        let main: extern "C" fn() = unsafe { core::mem::transmute(entry) };
        main();
        CURRENT_DIR.store(0, Ordering::Release);
        CURRENT_NAME.store(0, Ordering::Release);
        ran += 1;
    }

    log::write(&format!("{} mod plugin(s) loaded", ran));
    ran
}
