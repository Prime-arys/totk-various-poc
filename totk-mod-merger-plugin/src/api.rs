//! The `tkm_*` C API other plugins use to choose the mods that get merged.
//!
//! Plugins find these functions with `nn::ro::LookupSymbol` (or declare them
//! weak and link against this NRO). See `include/totk_mod_merger.h` for the
//! contract, and `crates/totk-mod-merger-api` for a Rust wrapper.

use core::ffi::{c_char, c_void, CStr};

use totk_merge::control::{self, MergedCallback};

/// A borrowed C string, or `None` for null / invalid UTF-8.
unsafe fn text<'a>(pointer: *const c_char) -> Option<&'a str> {
    if pointer.is_null() {
        return None;
    }
    CStr::from_ptr(pointer).to_str().ok()
}

#[no_mangle]
pub extern "C" fn tkm_api_version() -> u32 {
    control::API_VERSION
}

/// While a mod's plugin runs its `main`: the folder that plugin came from
/// (`sd:/totk/mods/<mod>`), so it can find its own files. Null at any other
/// time, and for plugins loaded from `skyline/plugins`. Copy it to keep it.
#[no_mangle]
pub extern "C" fn tkm_current_mod_dir() -> *const c_char {
    crate::plugins::current_mod_dir() as *const c_char
}

/// The name of that mod, as the mod list shows it. Null outside a mod
/// plugin's `main`.
#[no_mangle]
pub extern "C" fn tkm_current_mod_name() -> *const c_char {
    crate::plugins::current_mod_name() as *const c_char
}

/// Takes control of the mod list; returns a token, or 0.
#[no_mangle]
pub unsafe extern "C" fn tkm_take_control(owner: *const c_char, timeout_ms: u32) -> u64 {
    let owner = text(owner).unwrap_or("unnamed plugin");
    control::take_control(owner, timeout_ms)
}

#[no_mangle]
pub extern "C" fn tkm_release_control(token: u64) -> bool {
    control::release_control(token)
}

#[no_mangle]
pub extern "C" fn tkm_set_local_mods_enabled(token: u64, enabled: bool) -> bool {
    control::set_local_mods_enabled(token, enabled)
}

#[no_mangle]
pub extern "C" fn tkm_clear_mods(token: u64) -> bool {
    control::clear_mods(token)
}

/// Adds a mod (`.tkcl`, folder with `romfs`, or romfs root); returns its index
/// or -1.
#[no_mangle]
pub unsafe extern "C" fn tkm_add_mod(token: u64, path: *const c_char, name: *const c_char) -> i32 {
    match text(path) {
        Some(path) => control::add_mod(token, path, text(name)),
        None => -1,
    }
}

#[no_mangle]
pub unsafe extern "C" fn tkm_select_option(token: u64, index: i32, group: *const c_char, option: *const c_char) -> bool {
    match (text(group), text(option)) {
        (Some(group), Some(option)) => control::select_option(token, index, group, option),
        _ => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn tkm_set_merged_dir(token: u64, dir: *const c_char) -> bool {
    match text(dir) {
        Some(dir) => control::set_merged_dir(token, dir),
        None => false,
    }
}

#[no_mangle]
pub extern "C" fn tkm_commit(token: u64) -> bool {
    control::commit(token)
}

/// 0 waiting, 1 merging, 2 done, 3 failed.
#[no_mangle]
pub extern "C" fn tkm_get_state() -> i32 {
    control::phase() as i32
}

#[no_mangle]
pub extern "C" fn tkm_get_served_files() -> u32 {
    control::served_files()
}

#[no_mangle]
pub extern "C" fn tkm_on_merged(callback: Option<MergedCallback>, user: *mut c_void) -> bool {
    match callback {
        Some(callback) => {
            control::on_merged(callback, user);
            true
        }
        None => false,
    }
}
