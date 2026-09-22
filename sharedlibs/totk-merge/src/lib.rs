//! Mod discovery and merging, with no dependency on the console it runs on.
//!
//! File access goes through `totk_formats::sys`: `std::fs` in the Skyline
//! plugin (which implements it on top of `nn::fs`) and on a PC, where the whole
//! pipeline can be exercised against an extracted romfs; functions of the host
//! program in the mod manager homebrew, which builds this crate without `std`.

#![no_std]

extern crate alloc;
#[cfg(any(feature = "std", test))]
extern crate std;

pub use totk_formats::prelude;
pub use totk_formats::sys;

pub mod builder;
pub mod byml_changelog;
pub mod byml_keys;
pub mod byml_merge;
pub mod cache;
pub mod canonical;
pub mod config;
pub mod conflicts;
#[cfg(feature = "std")]
pub mod control;
pub mod engine;
pub mod gamedata;
pub mod merge_cache;
pub mod ini;
pub mod merger;
pub mod mods;
pub mod profile;
pub mod rom;
pub mod rsdb;
pub mod tkcl;

use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[doc(hidden)]
pub use alloc::format as __format;

/// Where log lines go (a `fn(&str)` stored as an address). The plugin points
/// this at Skyline's logger, the homebrew at its log file, tools at stdout.
static LOG_SINK: AtomicUsize = AtomicUsize::new(0);

pub fn set_log_sink(sink: fn(&str)) {
    LOG_SINK.store(sink as usize, Ordering::Relaxed);
}

#[doc(hidden)]
pub fn log_line(line: &str) {
    let address = LOG_SINK.load(Ordering::Relaxed);
    if address != 0 {
        let sink: fn(&str) = unsafe { core::mem::transmute(address) };
        sink(line);
    }
}

static VERBOSE: AtomicBool = AtomicBool::new(false);

/// Also log the details `debug!` reports.
pub fn set_verbose(verbose: bool) {
    VERBOSE.store(verbose, Ordering::Relaxed);
}

#[doc(hidden)]
pub fn verbose() -> bool {
    VERBOSE.load(Ordering::Relaxed)
}

#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {
        if $crate::verbose() {
            $crate::log_line(&$crate::__format!($($arg)*))
        }
    };
}

#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => { $crate::log_line(&$crate::__format!($($arg)*)) };
}

/// What a merge is busy with, for progress bars.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Stage {
    /// Reading mods and working out folder mods' changes (`done` of `total`
    /// mods).
    Reading = 0,
    /// Merging game files (`done` of `total` files).
    Merging = 1,
    /// Writing packs and the resource size table.
    Writing = 2,
    /// Comparing what the mods change (`done` of `total` files several mods
    /// touch), between reading and merging.
    Conflicts = 3,
}

/// `fn(Stage, done, total, item)`, stored as an address.
static PROGRESS_SINK: AtomicUsize = AtomicUsize::new(0);

pub fn set_progress_sink(sink: fn(Stage, usize, usize, &str)) {
    PROGRESS_SINK.store(sink as usize, Ordering::Relaxed);
}

pub(crate) fn progress(stage: Stage, done: usize, total: usize, item: &str) {
    let address = PROGRESS_SINK.load(Ordering::Relaxed);
    if address != 0 {
        let sink: fn(Stage, usize, usize, &str) = unsafe { core::mem::transmute(address) };
        sink(stage, done, total, item);
    }
}

/// `fn(&[Conflict]) -> bool`, stored as an address.
static CONFLICT_SINK: AtomicUsize = AtomicUsize::new(0);

/// Asked before merging mods that conflict: `false` cancels the merge. Without
/// one (the plugin at boot), merges always go ahead.
pub fn set_conflict_sink(sink: Option<fn(&[conflicts::Conflict]) -> bool>) {
    CONFLICT_SINK.store(sink.map_or(0, |f| f as usize), Ordering::Relaxed);
}

pub(crate) fn go_ahead_despite(conflicts: &[conflicts::Conflict]) -> bool {
    let address = CONFLICT_SINK.load(Ordering::Relaxed);
    if address == 0 || conflicts.is_empty() {
        return true;
    }
    let sink: fn(&[conflicts::Conflict]) -> bool = unsafe { core::mem::transmute(address) };
    sink(conflicts)
}
