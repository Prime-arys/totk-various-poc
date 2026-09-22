//! What the merger needs from the platform it runs on: files, a clock and a
//! lock.
//!
//! With the `std` feature (the Skyline plugin, PC tools and tests) this is a
//! thin layer over `std`, which Skyline implements on top of `nn::fs`. Without
//! it (the mod manager homebrew, where Rust has no standard library) the host
//! program provides the `tkm_host_*` C functions declared in `host`.
//!
//! Paths are plain strings with forward slashes. A path prefix can be aliased
//! (`sd:/` → `sdmc:/` in the homebrew, → an emulator's SD folder on a PC), so
//! the merge sees the same paths wherever it runs and writes the same index
//! the plugin reads back.

pub mod path {
    use crate::prelude::*;

    /// `base` + "/" + `name`, without doubling the separator.
    pub fn join(base: &str, name: &str) -> String {
        let name = name.trim_start_matches('/');
        if base.is_empty() {
            return name.to_string();
        }
        if base.ends_with('/') || base.ends_with('\\') {
            format!("{}{}", base, name)
        } else {
            format!("{}/{}", base, name)
        }
    }

    /// Everything before the last separator, if there is one.
    pub fn parent(path: &str) -> Option<&str> {
        let trimmed = path.trim_end_matches(['/', '\\']);
        let index = trimmed.rfind(['/', '\\'])?;
        let parent = &trimmed[..index];
        // Keep "sd:/" rather than "sd:", which is not a directory.
        if parent.ends_with(':') {
            Some(&trimmed[..index + 1])
        } else {
            Some(parent)
        }
    }

    /// The last component.
    pub fn file_name(path: &str) -> &str {
        let trimmed = path.trim_end_matches(['/', '\\']);
        match trimmed.rfind(['/', '\\']) {
            Some(index) => &trimmed[index + 1..],
            None => trimmed,
        }
    }

    /// `path` relative to `root`, with forward slashes.
    pub fn strip_root<'a>(path: &'a str, root: &str) -> Option<&'a str> {
        let root = root.trim_end_matches(['/', '\\']);
        let rest = path.strip_prefix(root)?;
        if rest.is_empty() {
            return Some(rest);
        }
        rest.strip_prefix(['/', '\\'])
    }

    /// Forward slashes, for paths that end up in index files.
    pub fn normalize(path: &str) -> String {
        path.replace('\\', "/")
    }
}

pub mod fs {
    use core::fmt;

    use crate::prelude::*;

    #[derive(Debug, Clone)]
    pub struct Error(pub String);

    impl fmt::Display for Error {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(&self.0)
        }
    }

    pub type Result<T> = core::result::Result<T, Error>;

    #[derive(Debug, Clone, Copy)]
    pub struct Metadata {
        pub is_dir: bool,
        pub len: u64,
    }

    #[derive(Debug, Clone)]
    pub struct Entry {
        pub name: String,
        pub is_dir: bool,
        /// Size of a file (0 for a directory).
        pub len: u64,
    }

    // --- path aliases -------------------------------------------------------

    static ALIASES: super::sync::Mutex<Vec<(String, String)>> = super::sync::Mutex::new(Vec::new());

    /// Makes paths starting with `from` open `to` + the rest instead.
    pub fn set_alias(from: &str, to: &str) {
        let mut aliases = ALIASES.lock();
        aliases.retain(|(existing, _)| existing != from);
        aliases.push((from.to_string(), to.to_string()));
    }

    /// The path the platform should open for `path`.
    pub fn resolve(path: &str) -> String {
        let aliases = ALIASES.lock();
        for (from, to) in aliases.iter() {
            if let Some(rest) = path.strip_prefix(from.as_str()) {
                return format!("{}{}", to, rest);
            }
        }
        path.to_string()
    }

    pub fn is_dir(path: &str) -> bool {
        metadata(path).map_or(false, |m| m.is_dir)
    }

    pub fn is_file(path: &str) -> bool {
        metadata(path).map_or(false, |m| !m.is_dir)
    }

    pub fn exists(path: &str) -> bool {
        metadata(path).is_some()
    }

    pub fn read_to_string(path: &str) -> Result<String> {
        let data = read(path)?;
        String::from_utf8(data).map_err(|_| Error(format!("{}: not UTF-8 text", path)))
    }

    /// Creates `path` and its missing parents.
    pub fn create_dir_all(path: &str) -> Result<()> {
        let trimmed = path.trim_end_matches(['/', '\\']);
        if trimmed.is_empty() || trimmed.ends_with(':') || is_dir(trimmed) {
            return Ok(());
        }
        if let Some(parent) = super::path::parent(trimmed) {
            create_dir_all(parent)?;
        }
        match create_dir(trimmed) {
            Ok(()) => Ok(()),
            Err(_) if is_dir(trimmed) => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Deletes a directory and everything in it.
    pub fn remove_dir_all(path: &str) -> Result<()> {
        if !is_dir(path) {
            return Err(Error(format!("{}: not a directory", path)));
        }
        remove_tree(path)
    }

    // --- std ------------------------------------------------------------------

    #[cfg(feature = "std")]
    mod platform {
        use std::io::{Read, Seek, SeekFrom, Write};

        use super::{resolve, Entry, Error, Metadata, Result};
        use crate::prelude::*;

        fn error(path: &str, error: std::io::Error) -> Error {
            Error(format!("{}: {}", path, error))
        }

        pub fn read(path: &str) -> Result<Vec<u8>> {
            std::fs::read(resolve(path)).map_err(|e| error(path, e))
        }

        pub fn write(path: &str, data: &[u8]) -> Result<()> {
            std::fs::write(resolve(path), data).map_err(|e| error(path, e))
        }

        pub fn metadata(path: &str) -> Option<Metadata> {
            std::fs::metadata(resolve(path)).ok().map(|m| Metadata {
                is_dir: m.is_dir(),
                len: if m.is_dir() { 0 } else { m.len() },
            })
        }

        pub fn read_dir(path: &str) -> Result<Vec<Entry>> {
            let mut entries = Vec::new();
            for entry in std::fs::read_dir(resolve(path)).map_err(|e| error(path, e))?.flatten() {
                let Ok(file_type) = entry.file_type() else {
                    continue;
                };
                let is_dir = file_type.is_dir();
                let len = if is_dir { 0 } else { entry.metadata().map(|m| m.len()).unwrap_or(0) };
                entries.push(Entry {
                    name: entry.file_name().to_string_lossy().to_string(),
                    is_dir,
                    len,
                });
            }
            Ok(entries)
        }

        pub fn create_dir(path: &str) -> Result<()> {
            std::fs::create_dir(resolve(path)).map_err(|e| error(path, e))
        }

        pub fn remove_tree(path: &str) -> Result<()> {
            std::fs::remove_dir_all(resolve(path)).map_err(|e| error(path, e))
        }

        pub fn remove_file(path: &str) -> Result<()> {
            std::fs::remove_file(resolve(path)).map_err(|e| error(path, e))
        }

        pub fn rename(from: &str, to: &str) -> Result<()> {
            std::fs::rename(resolve(from), resolve(to)).map_err(|e| error(from, e))
        }

        pub struct File {
            file: std::sync::Mutex<std::fs::File>,
            len: u64,
        }

        impl File {
            pub fn open(path: &str) -> Result<File> {
                let file = std::fs::File::open(resolve(path)).map_err(|e| error(path, e))?;
                let len = file.metadata().map_err(|e| error(path, e))?.len();
                Ok(File {
                    file: std::sync::Mutex::new(file),
                    len,
                })
            }

            pub fn len(&self) -> u64 {
                self.len
            }

            /// Reads as much of `buffer` as the file holds from `offset`.
            pub fn read_at(&self, offset: u64, buffer: &mut [u8]) -> Result<usize> {
                let mut file = self.file.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                file.seek(SeekFrom::Start(offset)).map_err(|e| Error(e.to_string()))?;
                let mut done = 0;
                while done < buffer.len() {
                    match file.read(&mut buffer[done..]) {
                        Ok(0) => break,
                        Ok(read) => done += read,
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                        Err(e) => return Err(Error(e.to_string())),
                    }
                }
                Ok(done)
            }
        }

        pub struct Writer {
            file: std::io::BufWriter<std::fs::File>,
            path: String,
        }

        impl Writer {
            pub fn create(path: &str) -> Result<Writer> {
                let file = std::fs::File::create(resolve(path)).map_err(|e| error(path, e))?;
                Ok(Writer {
                    file: std::io::BufWriter::with_capacity(1 << 20, file),
                    path: path.to_string(),
                })
            }

            pub fn write_all(&mut self, data: &[u8]) -> Result<()> {
                self.file.write_all(data).map_err(|e| error(&self.path, e))
            }

            pub fn finish(mut self) -> Result<()> {
                self.file.flush().map_err(|e| error(&self.path, e))
            }
        }
    }

    // --- host functions -------------------------------------------------------

    #[cfg(not(feature = "std"))]
    mod platform {
        use core::ffi::c_void;

        use super::{resolve, Entry, Error, Metadata, Result};
        use crate::prelude::*;
        use crate::sys::host;

        /// NUL terminated copy of the resolved path.
        fn c_path(path: &str) -> Vec<u8> {
            let mut bytes = resolve(path).into_bytes();
            bytes.push(0);
            bytes
        }

        fn error(path: &str, code: i64) -> Error {
            Error(format!("{}: error {}", path, code))
        }

        pub fn metadata(path: &str) -> Option<Metadata> {
            let c = c_path(path);
            let (mut is_dir, mut len) = (0i32, 0u64);
            let result = unsafe { host::tkm_host_stat(c.as_ptr(), &mut is_dir, &mut len) };
            (result == 0).then_some(Metadata {
                is_dir: is_dir != 0,
                len: if is_dir != 0 { 0 } else { len },
            })
        }

        pub fn read(path: &str) -> Result<Vec<u8>> {
            let file = File::open(path)?;
            let mut data = vec![0u8; file.len() as usize];
            let read = file.read_at(0, &mut data)?;
            data.truncate(read);
            Ok(data)
        }

        pub fn write(path: &str, data: &[u8]) -> Result<()> {
            let mut writer = Writer::create(path)?;
            writer.write_all(data)?;
            writer.finish()
        }

        pub fn read_dir(path: &str) -> Result<Vec<Entry>> {
            let c = c_path(path);
            let handle = unsafe { host::tkm_host_dir_open(c.as_ptr()) };
            if handle.is_null() {
                return Err(Error(format!("{}: cannot open directory", path)));
            }
            let mut entries = Vec::new();
            let mut name = [0u8; 1024];
            loop {
                let (mut is_dir, mut len) = (0i32, 0u64);
                let result =
                    unsafe { host::tkm_host_dir_next(handle, name.as_mut_ptr(), name.len(), &mut is_dir, &mut len) };
                if result <= 0 {
                    break;
                }
                let length = name.iter().position(|&b| b == 0).unwrap_or(name.len());
                let text = String::from_utf8_lossy(&name[..length]).into_owned();
                if text == "." || text == ".." {
                    continue;
                }
                entries.push(Entry {
                    name: text,
                    is_dir: is_dir != 0,
                    len: if is_dir != 0 { 0 } else { len },
                });
            }
            unsafe { host::tkm_host_dir_close(handle) };
            Ok(entries)
        }

        pub fn create_dir(path: &str) -> Result<()> {
            let c = c_path(path);
            match unsafe { host::tkm_host_mkdir(c.as_ptr()) } {
                0 => Ok(()),
                code => Err(error(path, code as i64)),
            }
        }

        pub fn remove_file(path: &str) -> Result<()> {
            let c = c_path(path);
            match unsafe { host::tkm_host_remove_file(c.as_ptr()) } {
                0 => Ok(()),
                code => Err(error(path, code as i64)),
            }
        }

        pub fn remove_tree(path: &str) -> Result<()> {
            for entry in read_dir(path)? {
                let child = crate::sys::path::join(path, &entry.name);
                if entry.is_dir {
                    remove_tree(&child)?;
                } else {
                    remove_file(&child)?;
                }
            }
            let c = c_path(path);
            match unsafe { host::tkm_host_remove_dir(c.as_ptr()) } {
                0 => Ok(()),
                code => Err(error(path, code as i64)),
            }
        }

        pub fn rename(from: &str, to: &str) -> Result<()> {
            let (a, b) = (c_path(from), c_path(to));
            match unsafe { host::tkm_host_rename(a.as_ptr(), b.as_ptr()) } {
                0 => Ok(()),
                code => Err(error(from, code as i64)),
            }
        }

        pub struct File {
            handle: *mut c_void,
            len: u64,
        }

        // The host's file handles are only used under its own lock.
        unsafe impl Send for File {}
        unsafe impl Sync for File {}

        impl File {
            pub fn open(path: &str) -> Result<File> {
                let c = c_path(path);
                let mut len = 0u64;
                let handle = unsafe { host::tkm_host_open_read(c.as_ptr(), &mut len) };
                if handle.is_null() {
                    return Err(Error(format!("{}: cannot open", path)));
                }
                Ok(File { handle, len })
            }

            pub fn len(&self) -> u64 {
                self.len
            }

            pub fn read_at(&self, offset: u64, buffer: &mut [u8]) -> Result<usize> {
                let read = unsafe { host::tkm_host_read_at(self.handle, offset, buffer.as_mut_ptr(), buffer.len()) };
                if read < 0 {
                    return Err(Error(format!("read error {}", read)));
                }
                Ok(read as usize)
            }
        }

        impl Drop for File {
            fn drop(&mut self) {
                unsafe { host::tkm_host_close(self.handle) };
            }
        }

        pub struct Writer {
            handle: *mut c_void,
            path: String,
        }

        impl Writer {
            pub fn create(path: &str) -> Result<Writer> {
                let c = c_path(path);
                let handle = unsafe { host::tkm_host_open_write(c.as_ptr()) };
                if handle.is_null() {
                    return Err(Error(format!("{}: cannot create", path)));
                }
                Ok(Writer {
                    handle,
                    path: path.to_string(),
                })
            }

            pub fn write_all(&mut self, data: &[u8]) -> Result<()> {
                let written = unsafe { host::tkm_host_write(self.handle, data.as_ptr(), data.len()) };
                if written as usize != data.len() {
                    return Err(Error(format!("{}: write error (disk full?)", self.path)));
                }
                Ok(())
            }

            pub fn finish(mut self) -> Result<()> {
                let handle = core::mem::replace(&mut self.handle, core::ptr::null_mut());
                match unsafe { host::tkm_host_close(handle) } {
                    0 => Ok(()),
                    code => Err(error(&self.path, code as i64)),
                }
            }
        }

        impl Drop for Writer {
            fn drop(&mut self) {
                if !self.handle.is_null() {
                    unsafe { host::tkm_host_close(self.handle) };
                }
            }
        }
    }

    pub use platform::{metadata, read, read_dir, remove_file, rename, write, File, Writer};
    use platform::{create_dir, remove_tree};
}

/// The functions a program without `std` provides.
#[cfg(not(feature = "std"))]
pub mod host {
    use core::ffi::c_void;

    extern "C" {
        /// 0 when `path` exists; fills whether it is a directory and its size.
        pub fn tkm_host_stat(path: *const u8, is_dir: *mut i32, len: *mut u64) -> i32;
        /// A handle for reads at any offset, or null. Fills the file size.
        pub fn tkm_host_open_read(path: *const u8, len: *mut u64) -> *mut c_void;
        /// Bytes read (short only at the end of the file), or a negative error.
        pub fn tkm_host_read_at(handle: *mut c_void, offset: u64, buffer: *mut u8, len: usize) -> i64;
        /// A handle writing a new (truncated) file, or null.
        pub fn tkm_host_open_write(path: *const u8) -> *mut c_void;
        /// Bytes written.
        pub fn tkm_host_write(handle: *mut c_void, data: *const u8, len: usize) -> i64;
        /// Closes a read or write handle; 0 when everything was flushed.
        pub fn tkm_host_close(handle: *mut c_void) -> i32;
        pub fn tkm_host_dir_open(path: *const u8) -> *mut c_void;
        /// 1 and a NUL terminated name for the next entry, 0 at the end.
        pub fn tkm_host_dir_next(handle: *mut c_void, name: *mut u8, capacity: usize, is_dir: *mut i32, len: *mut u64) -> i32;
        pub fn tkm_host_dir_close(handle: *mut c_void);
        pub fn tkm_host_mkdir(path: *const u8) -> i32;
        pub fn tkm_host_remove_file(path: *const u8) -> i32;
        /// Removes an empty directory.
        pub fn tkm_host_remove_dir(path: *const u8) -> i32;
        pub fn tkm_host_rename(from: *const u8, to: *const u8) -> i32;
        /// A monotonic clock, in microseconds.
        pub fn tkm_host_ticks_us() -> u64;
    }
}

pub mod time {
    /// A point in time, for "took 3.2 s" logs.
    #[derive(Debug, Clone, Copy)]
    pub struct Instant(u64);

    #[cfg(feature = "std")]
    fn micros() -> u64 {
        static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        START.get_or_init(std::time::Instant::now).elapsed().as_micros() as u64
    }

    #[cfg(not(feature = "std"))]
    fn micros() -> u64 {
        unsafe { super::host::tkm_host_ticks_us() }
    }

    impl Instant {
        pub fn now() -> Instant {
            Instant(micros())
        }

        pub fn elapsed_ms(&self) -> u64 {
            micros().saturating_sub(self.0) / 1000
        }

        pub fn elapsed_secs(&self) -> f32 {
            micros().saturating_sub(self.0) as f32 / 1_000_000.0
        }
    }
}

pub mod sync {
    use core::cell::UnsafeCell;
    use core::ops::{Deref, DerefMut};
    use core::sync::atomic::{AtomicBool, Ordering};

    /// A spinning lock: held for a few instructions at a time, and safe to use
    /// where the platform's mutex is not (Skyline's nn::fs hooks, or a Rust
    /// library with no operating system underneath).
    pub struct Mutex<T> {
        locked: AtomicBool,
        value: UnsafeCell<T>,
    }

    unsafe impl<T: Send> Send for Mutex<T> {}
    unsafe impl<T: Send> Sync for Mutex<T> {}

    impl<T> Mutex<T> {
        pub const fn new(value: T) -> Mutex<T> {
            Mutex {
                locked: AtomicBool::new(false),
                value: UnsafeCell::new(value),
            }
        }

        pub fn lock(&self) -> MutexGuard<'_, T> {
            while self
                .locked
                .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_err()
            {
                core::hint::spin_loop();
            }
            MutexGuard { mutex: self }
        }
    }

    pub struct MutexGuard<'a, T> {
        mutex: &'a Mutex<T>,
    }

    impl<T> Deref for MutexGuard<'_, T> {
        type Target = T;
        fn deref(&self) -> &T {
            unsafe { &*self.mutex.value.get() }
        }
    }

    impl<T> DerefMut for MutexGuard<'_, T> {
        fn deref_mut(&mut self) -> &mut T {
            unsafe { &mut *self.mutex.value.get() }
        }
    }

    impl<T> Drop for MutexGuard<'_, T> {
        fn drop(&mut self) {
            self.mutex.locked.store(false, Ordering::Release);
        }
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use crate::prelude::*;

    #[test]
    fn joins_and_splits_paths() {
        assert_eq!(path::join("sd:/totk", "mods"), "sd:/totk/mods");
        assert_eq!(path::join("sd:/", "totk"), "sd:/totk");
        assert_eq!(path::parent("sd:/totk/mods"), Some("sd:/totk"));
        assert_eq!(path::parent("sd:/totk"), Some("sd:/"));
        assert_eq!(path::file_name("sd:/totk/mods/"), "mods");
        assert_eq!(path::strip_root("sd:/totk/mods/a/b", "sd:/totk/mods/"), Some("a/b"));
    }

    #[test]
    fn aliases_and_tree_operations() {
        let root = std::env::temp_dir().join(format!("totk-sys-test-{}", std::process::id()));
        // An alias target has to exist, like `sdmc:/` does.
        std::fs::create_dir_all(&root).unwrap();
        let root = root.to_string_lossy().replace('\\', "/");
        fs::set_alias("test-alias:/", &format!("{}/", root));
        fs::create_dir_all("test-alias:/a/b").unwrap();
        fs::write("test-alias:/a/b/file.txt", b"hello").unwrap();
        assert!(fs::is_dir("test-alias:/a"));
        assert_eq!(fs::read("test-alias:/a/b/file.txt").unwrap(), b"hello");
        let entries = fs::read_dir("test-alias:/a/b").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].len, 5);
        let file = fs::File::open("test-alias:/a/b/file.txt").unwrap();
        let mut buffer = [0u8; 3];
        assert_eq!(file.read_at(2, &mut buffer).unwrap(), 3);
        assert_eq!(&buffer, b"llo");
        fs::remove_dir_all("test-alias:/a").unwrap();
        assert!(!fs::exists("test-alias:/a"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
