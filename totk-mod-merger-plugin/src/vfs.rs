//! Serving merged files to the game.
//!
//! TotK goes through `nn::fs` for everything it reads, so redirecting a file is
//! a matter of handing `OpenFile` a different path. The handle that comes back
//! is an ordinary one, which means reads, seeks and sizes all keep working
//! without any further hooks.

use std::collections::HashMap;
use std::ffi::CString;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::OnceLock;

use skyline::nn;

use crate::log;
use totk_merge::config;
use totk_merge::engine::Redirects;

/// romfs-relative path -> NUL terminated SD path, ready to hand to nn::fs.
static REDIRECTS: OnceLock<HashMap<String, CString>> = OnceLock::new();

/// Longest romfs-relative path we will consider; keeps the hook's stack use
/// bounded no matter what the game asks for.
const MAX_PATH: usize = 512;

/// How many redirected reads to log when `log_redirects` is on. Enough to show
/// mods being picked up without filling the SD card during a play session.
const MAX_LOGGED_HITS: usize = 64;

static LOG_HITS: AtomicUsize = AtomicUsize::new(0);
static LOGGING: AtomicBool = AtomicBool::new(false);

pub fn set_logging(enabled: bool) {
    LOGGING.store(enabled, Ordering::Relaxed);
}

fn note_hit(path: &str) {
    if !LOGGING.load(Ordering::Relaxed) {
        return;
    }
    let count = LOG_HITS.fetch_add(1, Ordering::Relaxed);
    if count < MAX_LOGGED_HITS {
        log::write(&format!("serving {}", path));
    } else if count == MAX_LOGGED_HITS {
        log::write("(further redirected reads not logged)");
    }
}

pub fn install(redirects: Redirects) {
    let mut table = HashMap::with_capacity(redirects.len());
    for (romfs, sd) in redirects {
        match CString::new(sd) {
            Ok(path) => {
                table.insert(romfs, path);
            }
            Err(_) => log::write(&format!("skipping {}: path contains a NUL byte", romfs)),
        }
    }

    if REDIRECTS.set(table).is_err() {
        log::write("redirect table was already installed");
        return;
    }

    skyline::install_hooks!(open_file_hook, get_entry_type_hook);
    log::write(&format!(
        "file redirection active ({} files)",
        REDIRECTS.get().unwrap().len()
    ));
}

/// The path nn::fs was handed, if it is short, NUL terminated UTF-8.
fn path_text(path: *const u8) -> Option<&'static str> {
    if path.is_null() {
        return None;
    }
    let mut length = 0;
    // SAFETY: nn::fs paths are NUL terminated; the bound keeps a corrupt or
    // unterminated pointer from running away.
    while length < MAX_PATH && unsafe { *path.add(length) } != 0 {
        length += 1;
    }
    if length == MAX_PATH {
        return None;
    }
    let bytes = unsafe { core::slice::from_raw_parts(path, length) };
    core::str::from_utf8(bytes).ok()
}

/// Maps "<mount>:/Pack/Foo.pack.zs" to the SD file that replaces it.
///
/// The mount name is not checked: the table only ever holds romfs-shaped paths,
/// and the game's other mounts (save, temp) use names of their own.
fn lookup(path: *const u8) -> Option<(&'static str, &'static CString)> {
    let table = REDIRECTS.get()?;
    let relative = path_text(path)?.split_once(":/")?.1;
    table.get_key_value(relative).map(|(key, value)| (key.as_str(), value))
}

static LOCALE_SEEN: AtomicBool = AtomicBool::new(false);

/// The game reads the message archive of one language: remember which, so the
/// next merges (here or in the mod manager) skip the others.
fn note_locale(path: *const u8) {
    if LOCALE_SEEN.load(Ordering::Relaxed) {
        return;
    }
    let Some(locale) = path_text(path).and_then(config::locale_of_message_archive) else {
        return;
    };
    if LOCALE_SEEN.swap(true, Ordering::Relaxed) {
        return;
    }
    if config::detected_locale().as_deref() == Some(locale) {
        return;
    }
    match totk_merge::sys::fs::write(config::LOCALE_PATH, locale.as_bytes()) {
        Ok(()) => log::write(&format!(
            "the game reads the {} texts: recorded in {} for the next merges",
            locale,
            config::LOCALE_PATH
        )),
        Err(error) => log::write(&format!("could not record the game's language: {}", error)),
    }
}

#[skyline::hook(replace = nn::fs::OpenFile)]
unsafe fn open_file_hook(handle: *mut nn::fs::FileHandle, path: *const u8, mode: i32) -> u32 {
    note_locale(path);
    match lookup(path) {
        Some((romfs, redirect)) => {
            note_hit(romfs);
            original!()(handle, redirect.as_ptr() as *const u8, mode)
        }
        None => original!()(handle, path, mode),
    }
}

#[skyline::hook(replace = nn::fs::GetEntryType)]
unsafe fn get_entry_type_hook(entry_type: *mut u32, path: *const u8) -> u32 {
    match lookup(path) {
        Some((_, redirect)) => original!()(entry_type, redirect.as_ptr() as *const u8),
        None => original!()(entry_type, path),
    }
}
