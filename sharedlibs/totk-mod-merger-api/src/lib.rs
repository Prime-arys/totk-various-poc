//! Safe bindings to totk-mod-merger-plugin's `tkm_*` API.
//!
//! ```ignore
//! use totk_mod_merger_api::ModMerger;
//!
//! #[skyline::main(name = "online")]
//! pub fn main() {
//!     let Some(merger) = ModMerger::find() else { return };
//!     let Some(control) = merger.take_control("online", 30_000) else { return };
//!     std::thread::spawn(move || {
//!         let pack = download_pack(); // your code
//!         control.add_mod(&pack, Some("Online pack"));
//!         control.commit();
//!     });
//! }
//! ```
//!
//! The functions are looked up with `nn::ro::LookupSymbol`, so the merger can be
//! loaded before or after the plugin using them, or not at all.

use core::ffi::{c_char, c_void};
use std::ffi::CString;

/// API version these bindings were written for.
pub const API_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Waiting,
    Merging,
    Done,
    Failed,
}

impl State {
    fn from_raw(value: i32) -> State {
        match value {
            0 => State::Waiting,
            1 => State::Merging,
            2 => State::Done,
            _ => State::Failed,
        }
    }
}

pub type MergedCallback = extern "C" fn(state: i32, served_files: u32, user: *mut c_void);

struct Functions {
    api_version: extern "C" fn() -> u32,
    take_control: extern "C" fn(*const c_char, u32) -> u64,
    release_control: extern "C" fn(u64) -> bool,
    set_local_mods_enabled: extern "C" fn(u64, bool) -> bool,
    clear_mods: extern "C" fn(u64) -> bool,
    add_mod: extern "C" fn(u64, *const c_char, *const c_char) -> i32,
    select_option: extern "C" fn(u64, i32, *const c_char, *const c_char) -> bool,
    set_merged_dir: extern "C" fn(u64, *const c_char) -> bool,
    commit: extern "C" fn(u64) -> bool,
    get_state: extern "C" fn() -> i32,
    get_served_files: extern "C" fn() -> u32,
    on_merged: extern "C" fn(Option<MergedCallback>, *mut c_void) -> bool,
}

fn lookup(name: &str) -> Option<usize> {
    let name = CString::new(name).ok()?;
    let mut address: usize = 0;
    let result = unsafe { skyline::nn::ro::LookupSymbol(&mut address as *mut usize, name.as_ptr() as *const u8) };
    (result == 0 && address != 0).then_some(address)
}

macro_rules! resolve {
    ($name:literal) => {
        unsafe { core::mem::transmute(lookup($name)?) }
    };
}

/// While a mod's plugin runs its `main`: the folder the plugin came from
/// (`sd:/totk/mods/<mod>`). `None` at any other time, and for plugins loaded
/// from `skyline/plugins`.
pub fn current_mod_dir() -> Option<String> {
    read_string("tkm_current_mod_dir")
}

/// The name of that mod, as the mod list shows it.
pub fn current_mod_name() -> Option<String> {
    read_string("tkm_current_mod_name")
}

fn read_string(symbol: &str) -> Option<String> {
    let getter: extern "C" fn() -> *const c_char = unsafe { core::mem::transmute(lookup(symbol)?) };
    let pointer = getter();
    (!pointer.is_null()).then(|| unsafe { core::ffi::CStr::from_ptr(pointer) }.to_string_lossy().into_owned())
}

/// The loaded merger.
pub struct ModMerger {
    functions: Functions,
}

impl ModMerger {
    /// Finds the merger, if it is loaded and speaks a compatible API.
    pub fn find() -> Option<ModMerger> {
        let functions = Functions {
            api_version: resolve!("tkm_api_version"),
            take_control: resolve!("tkm_take_control"),
            release_control: resolve!("tkm_release_control"),
            set_local_mods_enabled: resolve!("tkm_set_local_mods_enabled"),
            clear_mods: resolve!("tkm_clear_mods"),
            add_mod: resolve!("tkm_add_mod"),
            select_option: resolve!("tkm_select_option"),
            set_merged_dir: resolve!("tkm_set_merged_dir"),
            commit: resolve!("tkm_commit"),
            get_state: resolve!("tkm_get_state"),
            get_served_files: resolve!("tkm_get_served_files"),
            on_merged: resolve!("tkm_on_merged"),
        };
        ((functions.api_version)() == API_VERSION).then_some(ModMerger { functions })
    }

    /// Takes control of the mod list; see `tkm_take_control`.
    pub fn take_control(self, owner: &str, timeout_ms: u32) -> Option<Control> {
        let owner = CString::new(owner).ok()?;
        let token = (self.functions.take_control)(owner.as_ptr(), timeout_ms);
        (token != 0).then_some(Control { merger: self, token })
    }

    pub fn state(&self) -> State {
        State::from_raw((self.functions.get_state)())
    }

    pub fn served_files(&self) -> u32 {
        (self.functions.get_served_files)()
    }

    /// Runs `callback` once the merge is over.
    pub fn on_merged(&self, callback: MergedCallback, user: *mut c_void) -> bool {
        (self.functions.on_merged)(Some(callback), user)
    }
}

/// Control of the mod list, until committed or released.
pub struct Control {
    merger: ModMerger,
    token: u64,
}

// The token is plain data and the API is thread safe.
unsafe impl Send for Control {}

impl Control {
    pub fn merger(&self) -> &ModMerger {
        &self.merger
    }

    pub fn set_local_mods_enabled(&self, enabled: bool) -> bool {
        (self.merger.functions.set_local_mods_enabled)(self.token, enabled)
    }

    pub fn clear_mods(&self) -> bool {
        (self.merger.functions.clear_mods)(self.token)
    }

    /// Adds a mod; returns its index for [`Control::select_option`].
    pub fn add_mod(&self, path: &str, name: Option<&str>) -> Option<i32> {
        let path = CString::new(path).ok()?;
        let name = name.and_then(|n| CString::new(n).ok());
        let index = (self.merger.functions.add_mod)(
            self.token,
            path.as_ptr(),
            name.as_ref().map_or(core::ptr::null(), |n| n.as_ptr()),
        );
        (index >= 0).then_some(index)
    }

    pub fn select_option(&self, mod_index: i32, group: &str, option: &str) -> bool {
        let (Ok(group), Ok(option)) = (CString::new(group), CString::new(option)) else {
            return false;
        };
        (self.merger.functions.select_option)(self.token, mod_index, group.as_ptr(), option.as_ptr())
    }

    pub fn set_merged_dir(&self, dir: &str) -> bool {
        match CString::new(dir) {
            Ok(dir) => (self.merger.functions.set_merged_dir)(self.token, dir.as_ptr()),
            Err(_) => false,
        }
    }

    /// The list is final: the merge starts.
    pub fn commit(self) -> ModMerger {
        (self.merger.functions.commit)(self.token);
        self.merger
    }

    /// Gives up control: the SD card's mods are merged instead.
    pub fn release(self) -> ModMerger {
        (self.merger.functions.release_control)(self.token);
        self.merger
    }
}
