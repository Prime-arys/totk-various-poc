//! totk-mod-merger-plugin
//!
//! Merges Tears of the Kingdom mods on the console itself, the way TKMM does on
//! a PC: folder mods and TKMM packages (`.tkcl`) are turned into changelogs,
//! merged over the game's own files, the resource size table is rebuilt, and
//! the result is served to the game through nn::fs.
//!
//! Runs on skyline-totk, which loads it once romfs is mounted and before the
//! game has read a single resource. Other plugins can take control of which
//! mods are merged through the `tkm_*` API (see `api.rs`), and mods can ship
//! plugins of their own, which are loaded once the merge is served (see
//! `plugins.rs`). The merging itself lives in the `totk-merge` crate so it can
//! be tested on a PC.

mod alloc;
mod api;
mod log;
mod patches;
mod plugins;
mod vfs;

#[global_allocator]
static ALLOCATOR: alloc::TotkAllocator = alloc::TotkAllocator;

use core::ffi::c_void;
use std::sync::OnceLock;

use totk_merge::config::{self, Config};
use totk_merge::control::{self, Phase};
use totk_merge::mods::Plan;
use totk_merge::engine::{self, Engine};

static CONFIG: OnceLock<Config> = OnceLock::new();

#[skyline::main(name = "totk-mod-merger")]
pub fn main() {
    let config = CONFIG.get_or_init(Config::load);

    if !config.log_path.is_empty() {
        log::open_file(&config.log_path);
    }
    totk_merge::set_log_sink(log::write);
    totk_merge::set_verbose(config.verbose);

    log::write(&format!(
        "totk-mod-merger {} starting (API v{})",
        env!("CARGO_PKG_VERSION"),
        control::API_VERSION
    ));

    if !config.enabled {
        log::write(&format!("disabled in {}", config::CONFIG_PATH));
        control::finish(Phase::Failed, 0);
        return;
    }

    let layered_romfs = format!("sd:/atmosphere/contents/{}/romfs", config::TITLE_ID);
    if std::fs::metadata(&layered_romfs).map(|m| m.is_dir()).unwrap_or(false) {
        log::write(&format!(
            "note: {} exists, so Atmosphère builds a layered romfs at every boot (slow, and \
             memory-hungry on firmware 20+). Mods belong in {}, plugins in \
             atmosphere/contents/{}/skyline/plugins.",
            layered_romfs,
            config.mods_dir,
            config::TITLE_ID
        ));
    }

    // Other plugins get the chance to take control of the mod list from their
    // own `main`, so the merge waits until every plugin has been started.
    if run_after_every_plugin() {
        log::write("merge deferred until every plugin is loaded");
    } else {
        log::write("this Skyline cannot defer the merge; plugins loaded later cannot choose the mods");
        merge_and_serve();
    }
}

extern "C" fn after_plugins(_: *mut c_void) {
    merge_and_serve();
}

/// Registers `after_plugins` with skyline-totk. False when it is unavailable.
fn run_after_every_plugin() -> bool {
    let Some(address) = lookup_symbol(b"skyline_totk_on_plugins_loaded\0") else {
        return false;
    };
    let register: extern "C" fn(extern "C" fn(*mut c_void), *mut c_void) -> bool =
        unsafe { core::mem::transmute(address) };
    // `false` also means "already loaded, ran it right away", which is fine
    // either way: the callback has run or will run.
    register(after_plugins, core::ptr::null_mut());
    true
}

fn merge_and_serve() {
    let config = CONFIG.get_or_init(Config::load);
    let plan = std::sync::Arc::new(control::wait_for_plan(config));

    vfs::set_logging(config.log_redirects);
    alloc::use_scratch(true);
    let merged = merge_on_a_big_stack(plan.clone());
    alloc::use_scratch(false);
    // What outlives the merge moves to the game's heap, so the scratch memory
    // can all be given back.
    let outcome = merged.clone();
    drop(merged);

    if config.apply_patches {
        patches::apply(&outcome.patches);
    }

    let (released, mapped) = alloc::trim();
    log::write(&format!(
        "merge memory: {} chunk(s) given back, {} still in use",
        released, mapped
    ));

    if outcome.redirects.is_empty() {
        if !outcome.failed {
            log::write(&format!(
                "nothing to serve: put each mod in its own folder in {} (holding romfs/exefs or a .tkcl)",
                config.mods_dir
            ));
        }
        control::finish(if outcome.failed { Phase::Failed } else { Phase::Done }, 0);
        // A mod can be nothing but code, and a failed merge is no reason to
        // leave out plugins the mods that did load ship.
        plugins::load(config, &plan);
        return;
    }

    let served = outcome.redirects.len() as u32;
    vfs::install(outcome.redirects);
    control::finish(Phase::Done, served);
    plugins::load(config, &plan);
}

/// Runs the merge on a thread of our own.
///
/// Plugins run on Skyline's worker thread, whose stack is sized for plugin
/// setup, not for decoders and document trees. The caller blocks until the
/// merge is over, so redirects are in place before the game reads a resource.
fn merge_on_a_big_stack(plan: std::sync::Arc<Plan>) -> engine::Outcome {
    // Thread stacks come out of the game's heap, which has little to spare at
    // this point: 8 MiB is refused, 4 MiB has always worked.
    const STACK_SIZES: [usize; 2] = [4 * 1024 * 1024, 2 * 1024 * 1024];

    let rom_prefix = rom_mount().unwrap_or_else(engine::detect_rom_prefix);

    for stack_size in STACK_SIZES {
        let (plan, rom_prefix) = (plan.clone(), rom_prefix.clone());
        let worker = std::thread::Builder::new()
            .name("totk-merger".to_string())
            .stack_size(stack_size)
            .spawn(move || {
                let config = CONFIG.get_or_init(Config::load);
                log::write(&format!("romfs mounted at {}", rom_prefix));
                Engine::new(config, &rom_prefix).run(&plan)
            });

        match worker {
            Ok(handle) => {
                return handle.join().unwrap_or_else(|_| {
                    log::write("the merge thread panicked, serving nothing");
                    engine::Outcome {
                        failed: true,
                        ..engine::Outcome::default()
                    }
                })
            }
            Err(error) => log::write(&format!(
                "could not start the merge thread with a {} MiB stack: {}",
                stack_size >> 20,
                error
            )),
        }
    }

    engine::Outcome {
        failed: true,
        ..engine::Outcome::default()
    }
}

fn lookup_symbol(name: &[u8]) -> Option<usize> {
    let mut address: usize = 0;
    let result = unsafe { skyline::nn::ro::LookupSymbol(&mut address as *mut usize, name.as_ptr()) };
    (result == 0 && address != 0).then_some(address)
}

/// Asks skyline-totk where the game mounted its romfs.
fn rom_mount() -> Option<String> {
    let address = lookup_symbol(b"totk_get_rom_mount\0")?;
    let getter: extern "C" fn() -> *const u8 = unsafe { core::mem::transmute(address) };
    let pointer = getter();
    if pointer.is_null() {
        return None;
    }
    Some(unsafe { skyline::from_c_str(pointer) })
}
