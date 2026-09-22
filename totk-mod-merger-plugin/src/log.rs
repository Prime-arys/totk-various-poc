//! Logging: everything goes to Skyline's logger, and optionally to a file on
//! the SD card so a user can hand over a log without a PC attached.
//!
//! The file is guarded by a spin lock rather than `std::sync::Mutex`: lines
//! are also written from the nn::fs hooks, on the game's own loading threads,
//! and a contended std mutex there ends in an nnSdk abort.

use std::cell::UnsafeCell;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

struct LogFile {
    busy: AtomicBool,
    file: UnsafeCell<Option<File>>,
}

unsafe impl Sync for LogFile {}

static LOG_FILE: LogFile = LogFile {
    busy: AtomicBool::new(false),
    file: UnsafeCell::new(None),
};

fn with_file(f: impl FnOnce(&mut Option<File>)) {
    while LOG_FILE
        .busy
        .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
    f(unsafe { &mut *LOG_FILE.file.get() });
    LOG_FILE.busy.store(false, Ordering::Release);
}

pub fn open_file(path: &str) {
    if let Some(parent) = std::path::Path::new(path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match OpenOptions::new().write(true).create(true).truncate(true).open(path) {
        Ok(opened) => with_file(|file| *file = Some(opened)),
        Err(error) => skyline::println!("[totk-merger] could not open {}: {}\n", path, error),
    }
}

pub fn write(line: &str) {
    skyline::println!("[totk-merger] {}\n", line);
    with_file(|file| {
        if let Some(file) = file.as_mut() {
            let _ = writeln!(file, "{}", line);
            let _ = file.flush();
        }
    });
}
