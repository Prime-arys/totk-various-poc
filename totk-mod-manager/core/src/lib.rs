//! totk-mod-manager's core: the merger's own code (mod discovery, profiles,
//! `.tkcl` reading, merging) behind a C API for the borealis interface.
//!
//! Everything returned as text is JSON (or NULL for "no error") and is freed
//! with `tkmc_free`. Paths use the plugin's `sd:/` spelling everywhere, so the
//! merge written here is byte for byte the one the plugin would make at boot,
//! and the plugin picks it up without merging again. See
//! `include/totk_manager_core.h` for the declarations.

#![no_std]

extern crate alloc;
#[cfg(any(feature = "std", test))]
extern crate std;

mod install;
mod json;
#[cfg(not(feature = "std"))]
mod nx;

use alloc::collections::BTreeMap;
use alloc::ffi::CString;
use core::ffi::{c_char, c_void, CStr};
use core::sync::atomic::{AtomicUsize, Ordering};

use totk_merge::config::{self, Config};
use totk_merge::conflicts::{self, Conflict};
use totk_merge::engine::{self, Engine};
use totk_merge::ini;
use totk_merge::mods::{self, ModInfo, ModKind};
use totk_merge::prelude::*;
use totk_merge::profile::{self, Profile, ProfileMod};
use totk_merge::sys::sync::Mutex;
use totk_merge::sys::time::Instant;
use totk_merge::sys::{fs, path};
use totk_merge::tkcl::{self, OptionGroupType};

use crate::install::InstallRequest;
use crate::json::Json;

pub type LogFn = extern "C" fn(line: *const c_char, user: *mut c_void);
pub type ProgressFn = extern "C" fn(stage: i32, done: u32, total: u32, item: *const c_char, user: *mut c_void);
/// Gets the conflicts as `{"conflicts": [...]}`; returns whether to merge anyway.
pub type ConflictsFn = extern "C" fn(conflicts: *const c_char, user: *mut c_void) -> bool;

static LOG: AtomicUsize = AtomicUsize::new(0);
static LOG_USER: AtomicUsize = AtomicUsize::new(0);
static PROGRESS: AtomicUsize = AtomicUsize::new(0);
static PROGRESS_USER: AtomicUsize = AtomicUsize::new(0);
static CONFLICTS: AtomicUsize = AtomicUsize::new(0);

/// The manager's own preferences, apart from the plugin's config.ini.
const MANAGER_INI: &str = "sd:/totk/manager.ini";

fn c_string(text: String) -> *mut c_char {
    let bytes: Vec<u8> = text.into_bytes().into_iter().filter(|&b| b != 0).collect();
    CString::new(bytes).map(CString::into_raw).unwrap_or(core::ptr::null_mut())
}

/// A borrowed C string, or "" for NULL.
unsafe fn text<'a>(pointer: *const c_char) -> &'a str {
    if pointer.is_null() {
        return "";
    }
    CStr::from_ptr(pointer).to_str().unwrap_or("")
}

fn error_or_null(result: Result<(), String>) -> *mut c_char {
    match result {
        Ok(()) => core::ptr::null_mut(),
        Err(error) => c_string(error),
    }
}

fn forward_log(line: &str) {
    let address = LOG.load(Ordering::Relaxed);
    if address == 0 {
        return;
    }
    let callback: LogFn = unsafe { core::mem::transmute(address) };
    let line = c_string(line.to_string());
    callback(line, LOG_USER.load(Ordering::Relaxed) as *mut c_void);
    unsafe { drop(CString::from_raw(line)) };
}

fn forward_progress(stage: totk_merge::Stage, done: usize, total: usize, item: &str) {
    let address = PROGRESS.load(Ordering::Relaxed);
    if address == 0 {
        return;
    }
    let callback: ProgressFn = unsafe { core::mem::transmute(address) };
    let item = c_string(item.to_string());
    callback(
        stage as i32,
        done as u32,
        total as u32,
        item,
        PROGRESS_USER.load(Ordering::Relaxed) as *mut c_void,
    );
    unsafe { drop(CString::from_raw(item)) };
}

/// `{"conflicts": [{"kind", "file", "count", "mods": [{"folder", "name"}], "samples"}]}`,
/// mods winner first.
fn conflicts_json(conflicts: &[Conflict]) -> String {
    let mut json = Json::new();
    json.begin_object().array_field("conflicts");
    for conflict in conflicts {
        json.begin_object()
            .field_str("kind", conflict.kind.as_str())
            .field_str("file", &conflict.file)
            .field_num("count", conflict.count as i64);
        json.array_field("mods");
        for reference in &conflict.mods {
            json.begin_object()
                .field_str("folder", &reference.folder)
                .field_str("name", &reference.name)
                .end_object();
        }
        json.end_array().array_field("samples");
        for sample in &conflict.samples {
            json.string(sample);
        }
        json.end_array().end_object();
    }
    json.end_array().end_object();
    json.finish()
}

fn ask_about_conflicts(conflicts: &[Conflict]) -> bool {
    let address = CONFLICTS.load(Ordering::Relaxed);
    if address == 0 {
        return true;
    }
    let callback: ConflictsFn = unsafe { core::mem::transmute(address) };
    let text = c_string(conflicts_json(conflicts));
    let go_ahead = callback(text, PROGRESS_USER.load(Ordering::Relaxed) as *mut c_void);
    unsafe { drop(CString::from_raw(text)) };
    go_ahead
}

/// `sd_root` is what `sd:/` means here ("sdmc:/" on the console).
#[no_mangle]
pub unsafe extern "C" fn tkmc_init(sd_root: *const c_char, log: Option<LogFn>, user: *mut c_void) {
    fs::set_alias("sd:/", text(sd_root));
    LOG.store(log.map_or(0, |f| f as usize), Ordering::Relaxed);
    LOG_USER.store(user as usize, Ordering::Relaxed);
    totk_merge::set_log_sink(forward_log);
    totk_merge::set_progress_sink(forward_progress);
}

#[no_mangle]
pub unsafe extern "C" fn tkmc_free(text: *mut c_char) {
    if !text.is_null() {
        drop(CString::from_raw(text));
    }
}

#[no_mangle]
pub unsafe extern "C" fn tkmc_free_bytes(data: *mut u8, size: usize) {
    if !data.is_null() {
        drop(Vec::from_raw_parts(data, size, size));
    }
}

fn write_options(json: &mut Json, key: &str, options: &BTreeMap<String, Vec<String>>) {
    json.object_field(key);
    for (group, selected) in options {
        json.array_field(group);
        for option in selected {
            json.string(option);
        }
        json.end_array();
    }
    json.end_object();
}

/// Everything the mod list screens show, in one go:
///
/// ```json
/// {"config": {...}, "profiles": [...], "profile": {"name", "exists", "mods": [...]},
///  "mods": [{"folder", "name", "kind", ...}], "problems": [...]}
/// ```
#[no_mangle]
pub extern "C" fn tkmc_state() -> *mut c_char {
    let config = Config::load();
    let (installed, problems) = mods::scan(&config);
    let active = if config.profile.is_empty() {
        None
    } else {
        Profile::load(&config, &config.profile)
    };

    let mut json = Json::new();
    json.begin_object();

    json.object_field("config")
        .field_str("profile", &config.profile)
        .field_bool("enabled", config.enabled)
        .field_bool("merge_at_boot", config.merge_at_boot)
        .field_str("locales", &config.locales)
        .field_str("mods_dir", &config.mods_dir)
        .field_str("merged_dir", &config.merged_dir)
        .field_num("merge_cache_size", config.merge_cache_size as i64)
        .field_bool("apply_patches", config.apply_patches)
        .field_bool("mod_plugins", config.mod_plugins)
        .end_object();

    json.array_field("profiles");
    for name in profile::list(&config) {
        json.string(&name);
    }
    json.end_array();

    json.object_field("profile")
        .field_str("name", &config.profile)
        .field_bool("exists", active.is_some());
    json.array_field("mods");
    if let Some(active) = &active {
        for entry in &active.mods {
            json.begin_object()
                .field_str("folder", &entry.folder)
                .field_bool("enabled", entry.enabled);
            write_options(&mut json, "options", &entry.options);
            json.end_object();
        }
    }
    json.end_array().end_object();

    json.array_field("mods");
    for installed in &installed {
        let thumbnail = if installed.info.thumbnail.is_empty() {
            String::new()
        } else {
            path::join(&installed.path, &installed.info.thumbnail)
        };
        json.begin_object()
            .field_str("folder", &installed.folder)
            .field_str("name", installed.name())
            .field_str("kind", installed.kind.as_str())
            .field_str("path", &installed.path)
            .field_str("content", &installed.content)
            .field_str("version", &installed.info.version)
            .field_str("author", &installed.info.author)
            .field_str("description", &installed.info.description)
            .field_str("url", &installed.info.url)
            .field_str("thumbnail", &thumbnail)
            .field_bool("enabled", installed.info.enabled)
            .field_num("priority", installed.info.priority as i64)
            // Skyline plugins the mod ships, and whether they are loaded.
            .field_num("plugins", installed.plugins.len() as i64)
            .field_bool("load_plugins", installed.info.load_plugins)
            // A mod that is only code: nothing of it is merged.
            .field_bool("code_only", !installed.plugins.is_empty() && !has_files(installed));
        json.end_object();
    }
    json.end_array();

    json.array_field("problems");
    for problem in &problems {
        json.string(problem);
    }
    json.end_array();

    json.end_object();
    c_string(json.finish())
}

/// Whether the merge on the SD card is the one of the active profile as it
/// is now. Lists the files of every folder mod: not for every refresh.
#[no_mangle]
pub extern "C" fn tkmc_is_applied() -> bool {
    let config = Config::load();
    let plan = mods::local_plan(&config);
    engine::is_plan_merged(&config, &plan)
}

/// Whether a mod holds anything the merge uses, as opposed to code only.
fn has_files(installed: &mods::InstalledMod) -> bool {
    match installed.kind {
        ModKind::Folder => {
            fs::is_dir(&path::join(&installed.content, "romfs")) || fs::is_dir(&path::join(&installed.content, "exefs"))
        }
        _ => true,
    }
}

fn group_type(kind: OptionGroupType) -> &'static str {
    match kind {
        OptionGroupType::Multi => "multi",
        OptionGroupType::MultiRequired => "multi_required",
        OptionGroupType::Single => "single",
        OptionGroupType::SingleRequired => "single_required",
    }
}

/// A mod's details, with a package's option groups:
/// `{"name", "version", "author", "description", "kind", "option_groups": [{"name",
/// "description", "type", "options": [{"name", "description"}], "defaults": [0]}]}`.
#[no_mangle]
pub unsafe extern "C" fn tkmc_mod_details(folder: *const c_char) -> *mut c_char {
    let config = Config::load();
    let folder = text(folder);
    let dir = config.mod_path(folder);
    let mut json = Json::new();
    json.begin_object().field_str("folder", folder);

    let Some((kind, content)) = mods::inspect_folder(&dir) else {
        json.field_str("error", "not a mod folder").end_object();
        return c_string(json.finish());
    };
    let info = ModInfo::load(&path::join(&dir, mods::MOD_INI));
    json.field_str("kind", kind.as_str()).field_str("content", &content);

    let mut name = info.name.clone().unwrap_or_else(|| folder.to_string());
    let mut version = info.version.clone();
    let mut author = info.author.clone();
    let mut description = info.description.clone();

    let package = (kind == ModKind::Package).then(|| tkcl::read_tkcl_file(&content));
    if let Some(Ok(package)) = &package {
        // mod.ini (from GameBanana's page) is usually more telling: the
        // package only fills in what it lacks.
        if info.name.is_none() && !package.name.trim().is_empty() {
            name = package.name.clone();
        }
        if version.is_empty() {
            version = package.version.clone();
        }
        if author.is_empty() {
            author = package.author.clone();
        }
        if description.is_empty() {
            description = package.description.clone();
        }
    }
    if let Some(Err(error)) = &package {
        json.field_str("error", error);
    }

    json.array_field("option_groups");
    if let Some(Ok(package)) = &package {
        for group in &package.option_groups {
            json.begin_object()
                .field_str("name", &group.name)
                .field_str("description", &group.description)
                .field_str("type", group_type(group.kind));
            json.array_field("options");
            for option in &group.options {
                json.begin_object()
                    .field_str("name", &option.name)
                    .field_str("description", &option.description)
                    .end_object();
            }
            json.end_array();
            json.array_field("defaults");
            for index in &group.default_selected {
                json.number(*index as i64);
            }
            json.end_array();
            json.end_object();
        }
    }
    json.end_array();
    write_options(&mut json, "ini_options", &info.options);

    // The plugins the mod ships, as the merger would load them in the game.
    let mut plugins = mods::plugin_files(&dir);
    if content != dir && kind != ModKind::Package {
        plugins.extend(mods::plugin_files(&content));
    }
    json.array_field("plugins");
    for plugin in &plugins {
        json.string(path::file_name(plugin));
    }
    json.end_array();
    json.field_bool("load_plugins", info.load_plugins);

    json.field_str("name", &name)
        .field_str("version", &version)
        .field_str("author", &author)
        .field_str("description", &description)
        .field_str("url", &info.url)
        .end_object();
    c_string(json.finish())
}

/// The image of a mod: its thumbnail file, or the one inside its package.
/// Returns false when it has none. Free with `tkmc_free_bytes`.
#[no_mangle]
pub unsafe extern "C" fn tkmc_mod_thumbnail(folder: *const c_char, data: *mut *mut u8, size: *mut usize) -> bool {
    let config = Config::load();
    let dir = config.mod_path(text(folder));
    let info = ModInfo::load(&path::join(&dir, mods::MOD_INI));
    let mut bytes = if info.thumbnail.is_empty() {
        None
    } else {
        fs::read(&path::join(&dir, &info.thumbnail)).ok()
    };
    if bytes.is_none() {
        if let Some((ModKind::Package, content)) = mods::inspect_folder(&dir) {
            bytes = tkcl::read_tkcl_thumbnail(&content);
        }
    }
    match bytes {
        Some(bytes) if !bytes.is_empty() => {
            let mut boxed = bytes.into_boxed_slice();
            *size = boxed.len();
            *data = boxed.as_mut_ptr();
            core::mem::forget(boxed);
            true
        }
        _ => false,
    }
}

// --- profiles ------------------------------------------------------------------

static PENDING: Mutex<Option<Profile>> = Mutex::new(None);

#[no_mangle]
pub unsafe extern "C" fn tkmc_profile_name_valid(name: *const c_char) -> bool {
    profile::is_valid_name(text(name))
}

/// Starts writing a profile: add mods (winner first), then commit.
#[no_mangle]
pub unsafe extern "C" fn tkmc_profile_begin(name: *const c_char) {
    *PENDING.lock() = Some(Profile::new(text(name)));
}

#[no_mangle]
pub unsafe extern "C" fn tkmc_profile_add_mod(folder: *const c_char, enabled: bool) {
    if let Some(pending) = PENDING.lock().as_mut() {
        pending.mods.push(ProfileMod {
            folder: text(folder).to_string(),
            enabled,
            options: BTreeMap::new(),
        });
    }
}

/// Selects an option for the mod added last.
#[no_mangle]
pub unsafe extern "C" fn tkmc_profile_add_option(group: *const c_char, option: *const c_char) {
    if let Some(entry) = PENDING.lock().as_mut().and_then(|p| p.mods.last_mut()) {
        let selected = entry.options.entry(text(group).to_string()).or_default();
        let option = text(option);
        // An empty option records "nothing selected" for the group.
        if !option.is_empty() {
            selected.push(option.to_string());
        }
    }
}

#[no_mangle]
pub extern "C" fn tkmc_profile_commit() -> *mut c_char {
    let Some(pending) = PENDING.lock().take() else {
        return c_string("no profile being written".into());
    };
    if !profile::is_valid_name(&pending.name) {
        return c_string(format!("'{}' cannot be a profile name", pending.name));
    }
    error_or_null(pending.save(&Config::load()).map_err(|e| e.to_string()))
}

#[no_mangle]
pub unsafe extern "C" fn tkmc_profile_activate(name: *const c_char) -> *mut c_char {
    error_or_null(config::set_value(config::CONFIG_PATH, "profile", text(name)).map_err(|e| e.to_string()))
}

#[no_mangle]
pub unsafe extern "C" fn tkmc_profile_delete(name: *const c_char) -> *mut c_char {
    let config = Config::load();
    error_or_null(profile::delete(&config, text(name)).map_err(|e| e.to_string()))
}

#[no_mangle]
pub unsafe extern "C" fn tkmc_profile_rename(from: *const c_char, to: *const c_char) -> *mut c_char {
    let config = Config::load();
    let (from, to) = (text(from), text(to));
    if !profile::is_valid_name(to) {
        return c_string(format!("'{}' cannot be a profile name", to));
    }
    if !from.eq_ignore_ascii_case(to) && fs::exists(&profile::file_path(&config, to)) {
        return c_string(format!("a profile named '{}' already exists", to));
    }
    error_or_null(profile::rename(&config, from, to).map_err(|e| e.to_string()))
}

#[no_mangle]
pub unsafe extern "C" fn tkmc_config_set(key: *const c_char, value: *const c_char) -> *mut c_char {
    error_or_null(config::set_value(config::CONFIG_PATH, text(key), text(value)).map_err(|e| e.to_string()))
}

/// Turns the plugins a mod ships on or off, through `plugins` in its
/// `mod.ini`. Its files are merged either way.
#[no_mangle]
pub unsafe extern "C" fn tkmc_mod_set_plugins(folder: *const c_char, enabled: bool) -> *mut c_char {
    let config = Config::load();
    let file = path::join(&config.mod_path(text(folder)), mods::MOD_INI);
    let mut info = ModInfo::load(&file);
    info.load_plugins = enabled;
    error_or_null(fs::write(&file, info.to_ini().as_bytes()).map_err(|e| e.to_string()))
}

/// A preference of the manager itself (plain text), or NULL when unset.
#[no_mangle]
pub unsafe extern "C" fn tkmc_manager_get(key: *const c_char) -> *mut c_char {
    let key = text(key);
    let contents = fs::read_to_string(MANAGER_INI).unwrap_or_default();
    let sections = ini::parse(&contents);
    match sections[0].entries.iter().rev().find(|(name, _)| name.eq_ignore_ascii_case(key)) {
        Some((_, value)) => c_string(value.clone()),
        None => core::ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn tkmc_manager_set(key: *const c_char, value: *const c_char) -> *mut c_char {
    error_or_null(config::set_value(MANAGER_INI, text(key), text(value)).map_err(|e| e.to_string()))
}

/// The merges kept on the SD card: `{"merges", "bytes"}` (bytes of the files
/// they store). Lists the store: not instant.
#[no_mangle]
pub extern "C" fn tkmc_merge_cache_usage() -> *mut c_char {
    let config = Config::load();
    let (merges, bytes) = totk_merge::merge_cache::MergeCache::new(&config.merged_dir).usage();
    let mut json = Json::new();
    json.begin_object()
        .field_num("merges", merges as i64)
        .field_num("bytes", bytes as i64)
        .end_object();
    c_string(json.finish())
}

/// The conflicts the last merge found (by the manager or at boot), in the
/// format of `tkmc_apply`'s callback.
#[no_mangle]
pub extern "C" fn tkmc_conflicts() -> *mut c_char {
    let config = Config::load();
    c_string(conflicts_json(&conflicts::load(&config.cache_dir)))
}

// --- installing ------------------------------------------------------------------

/// What an extracted archive holds:
/// `{"candidates": [{"kind", "path", "label", "has_code", "size"}]}`.
#[no_mangle]
pub unsafe extern "C" fn tkmc_find_mods(dir: *const c_char) -> *mut c_char {
    let mut json = Json::new();
    json.begin_object().array_field("candidates");
    for candidate in install::find_candidates(text(dir)) {
        json.begin_object()
            .field_str("kind", candidate.kind.as_str())
            .field_str("path", &candidate.path)
            .field_str("label", &candidate.label)
            .field_bool("has_code", candidate.has_code)
            .field_num("size", candidate.size as i64)
            .end_object();
    }
    json.end_array().end_object();
    c_string(json.finish())
}

#[no_mangle]
pub unsafe extern "C" fn tkmc_folder_name(name: *const c_char) -> *mut c_char {
    c_string(install::folder_name(text(name)))
}

/// Installs a candidate found by `tkmc_find_mods` as `<mods>/<folder>`.
/// Returns `{"ok", "path", "error"}`.
#[no_mangle]
pub unsafe extern "C" fn tkmc_install(
    source: *const c_char,
    kind: *const c_char,
    folder: *const c_char,
    name: *const c_char,
    version: *const c_char,
    author: *const c_char,
    description: *const c_char,
    url: *const c_char,
    thumbnail: *const c_char,
) -> *mut c_char {
    let config = Config::load();
    let kind = match text(kind) {
        "package" => ModKind::Package,
        "romfs" => ModKind::Romfs,
        _ => ModKind::Folder,
    };
    let mut info = ModInfo::default();
    info.name = Some(text(name).to_string()).filter(|n| !n.is_empty());
    info.version = text(version).to_string();
    info.author = text(author).to_string();
    info.description = text(description).to_string();
    info.url = text(url).to_string();
    let thumbnail = text(thumbnail);

    let result = install::install(
        &config,
        InstallRequest {
            source: text(source),
            kind,
            folder: text(folder),
            info,
            thumbnail: (!thumbnail.is_empty()).then_some(thumbnail),
        },
    );
    let mut json = Json::new();
    json.begin_object();
    match result {
        Ok(target) => json.field_bool("ok", true).field_str("path", &target),
        Err(error) => json.field_bool("ok", false).field_str("error", &error),
    };
    json.end_object();
    c_string(json.finish())
}

#[no_mangle]
pub unsafe extern "C" fn tkmc_uninstall(folder: *const c_char) -> *mut c_char {
    error_or_null(install::uninstall(&Config::load(), text(folder)))
}

/// Moves mods of the old layout into folders; returns `{"moved": [...]}`.
#[no_mangle]
pub extern "C" fn tkmc_migrate() -> *mut c_char {
    let moved = install::migrate_loose_packages(&Config::load());
    let mut json = Json::new();
    json.begin_object().array_field("moved");
    for folder in &moved {
        json.string(folder);
    }
    json.end_array().end_object();
    c_string(json.finish())
}

/// Deletes a folder and everything in it (downloads, temporary files).
#[no_mangle]
pub unsafe extern "C" fn tkmc_remove_tree(dir: *const c_char) -> bool {
    let dir = text(dir);
    !fs::is_dir(dir) || fs::remove_dir_all(dir).is_ok()
}

// --- merging ---------------------------------------------------------------------

/// The game version the romfs at `rom_prefix` belongs to:
/// `{"ok", "version", "nso", "error"}`.
#[no_mangle]
pub unsafe extern "C" fn tkmc_rom_info(rom_prefix: *const c_char) -> *mut c_char {
    let prefix = text(rom_prefix);
    let mut json = Json::new();
    json.begin_object();
    match fs::read(&format!("{}System/RegionLangMask.txt", prefix))
        .ok()
        .and_then(|data| totk_merge::rom::parse_region_lang_mask(&data))
    {
        Some((version, nso)) => {
            json.field_bool("ok", true)
                .field_num("version", version as i64)
                .field_str("nso", &nso);
        }
        None => {
            json.field_bool("ok", false)
                .field_str("error", &format!("no TotK romfs at {}", prefix));
        }
    }
    json.end_object();
    c_string(json.finish())
}

/// Merges the active profile the way the plugin would at boot (reusing a
/// merge that is still current). Blocks; `progress` is called along the way,
/// and `conflicts` (when not NULL) before merging mods that conflict.
/// Returns `{"ok", "reused", "cancelled", "conflicts", "files", "served",
/// "warnings", "patches", "seconds", "error"}`.
#[no_mangle]
pub unsafe extern "C" fn tkmc_apply(
    rom_prefix: *const c_char,
    progress: Option<ProgressFn>,
    conflicts: Option<ConflictsFn>,
    user: *mut c_void,
) -> *mut c_char {
    PROGRESS.store(progress.map_or(0, |f| f as usize), Ordering::Relaxed);
    PROGRESS_USER.store(user as usize, Ordering::Relaxed);
    CONFLICTS.store(conflicts.map_or(0, |f| f as usize), Ordering::Relaxed);
    totk_merge::set_conflict_sink(Some(ask_about_conflicts));

    let mut config = Config::load();
    // Applying is exactly what that setting waits for.
    config.merge_at_boot = true;
    let started = Instant::now();
    let plan = mods::local_plan(&config);
    let outcome = Engine::new(&config, text(rom_prefix)).run(&plan);

    totk_merge::set_conflict_sink(None);
    CONFLICTS.store(0, Ordering::Relaxed);
    PROGRESS.store(0, Ordering::Relaxed);
    let mut json = Json::new();
    json.begin_object()
        .field_bool("ok", !outcome.failed && !outcome.cancelled)
        .field_bool("reused", outcome.reused)
        .field_bool("from_cache", outcome.from_cache)
        .field_bool("cancelled", outcome.cancelled)
        .field_num("conflicts", outcome.conflicts as i64)
        .field_num("mods", plan.mods.len() as i64)
        .field_num("files", outcome.merged_files as i64)
        .field_num("served", outcome.redirects.len() as i64)
        .field_num("warnings", outcome.warnings as i64)
        .field_num("patches", outcome.patches.len() as i64)
        .field_tenths("seconds", started.elapsed_secs())
        .field_opt_str("error", outcome.error.as_deref())
        .end_object();
    c_string(json.finish())
}
