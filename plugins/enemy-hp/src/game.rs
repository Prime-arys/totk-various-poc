//! What the plugin knows of Tears of the Kingdom 1.2.1's code, in one place.
//!
//! Every address here is an offset from the start of the game's main module,
//! found by reading 1.2.1's code. A hook is only placed after checking that
//! the game says it is 1.2.1 and that the bytes around the spot are the ones
//! read, so another build of the game is left untouched.

use std::sync::atomic::AtomicI32;

use skyline::hooks::{getRegionAddress, InlineCtx, Region};

/// TotK 1.2.1, as `totk_get_version` gives it.
const SUPPORTED_VERSION: u32 = 10201;

/// A place in the game's code to hook, and what 1.2.1 has from 8 bytes
/// before it: the instruction replaced must be one that can be moved.
pub struct Site {
    pub offset: usize,
    pub expected: [u8; 16],
}

pub type Callback = unsafe extern "C" fn(&mut InlineCtx);

fn version() -> Option<u32> {
    let mut address: usize = 0;
    let result = unsafe { skyline::nn::ro::LookupSymbol(&mut address, b"totk_get_version\0".as_ptr()) };
    if result != 0 || address == 0 {
        return None;
    }
    let getter: extern "C" fn() -> u32 = unsafe { core::mem::transmute(address) };
    Some(getter())
}

/// Where the game's main module starts.
pub fn base() -> usize {
    unsafe { getRegionAddress(Region::Text) as usize }
}

/// Hooks `site`, after checking it is where 1.2.1 has it. Returns why not.
pub fn hook(site: &Site, callback: Callback) -> Result<(), String> {
    match version() {
        Some(SUPPORTED_VERSION) => {}
        Some(other) => return Err(format!("game version {} is not 1.2.1", other)),
        None => return Err("this Skyline does not say which version of the game runs".into()),
    }
    let address = base() + site.offset;
    let found = unsafe { core::slice::from_raw_parts((address - 8) as *const u8, site.expected.len()) };
    if found != site.expected {
        return Err(format!("the game's code at {:#x} is not the expected 1.2.1 build", site.offset));
    }
    unsafe {
        skyline::hooks::A64InlineHook(
            address as *const skyline::libc::c_void,
            callback as *const skyline::libc::c_void,
        )
    };
    Ok(())
}

pub unsafe fn read<T: Copy>(address: usize) -> T {
    core::ptr::read_volatile(address as *const T)
}

// --- an actor's life ------------------------------------------------------

/// The life component, as the game's damage code (`0x64b4b8`) and its gauges
/// read it. Each field points at something holding an i32 at +8:
///
/// ```text
/// [[life + 6192] + 8]  current life, written with ldxr/stxr
/// [[life + 6200] + 8]  maximum life
/// [[life + 6216] + 8]  life taken off the maximum (when the pointer is set)
/// [[life + 6208] + 8]  a second pool that soaks damage before the life, empty
///                      for ordinary enemies — not the life, left alone
/// ```
const CURRENT_LIFE: usize = 6192;
const MAX_LIFE: usize = 6200;
const MAX_LIFE_LOSS: usize = 6216;

/// The i32 at +8 of what `[life + field]` points at, if it points anywhere.
unsafe fn value(life: usize, field: usize) -> Option<i32> {
    let holder = read::<usize>(life + field);
    (holder != 0).then(|| read::<i32>(holder + 8))
}

/// The current life, to read and change the way the game does: with
/// exclusive loads and stores.
pub unsafe fn current_life(life: usize) -> Option<&'static AtomicI32> {
    let holder = read::<usize>(life + CURRENT_LIFE);
    (holder != 0).then(|| &*((holder + 8) as *const AtomicI32))
}

/// The most life the actor can have right now.
pub unsafe fn max_life(life: usize) -> i32 {
    value(life, MAX_LIFE).unwrap_or(0) - value(life, MAX_LIFE_LOSS).unwrap_or(0)
}

// --- the numbers -----------------------------------------------------------

/// The byte the enemy gauges test (`0x12c78cc`) before they show their
/// numbers: set while the armour worn has the `VisualizeLife` effect. It sits
/// in an object the game keeps a pointer to at this address.
const PLAYER_STATE: usize = 0x462ec80;
const VISUALIZE_LIFE: usize = 2204;

pub unsafe fn visualize_life() -> bool {
    let global = read::<usize>(base() + PLAYER_STATE);
    if global == 0 {
        return false;
    }
    let state = read::<usize>(global);
    state != 0 && read::<u8>(state + VISUALIZE_LIFE) != 0
}

/// What the game's text functions take (built by `0x1235fc4` from a string):
/// the text, its length in UTF-16 units, and a message attribute (-1: none).
#[repr(C)]
struct Message {
    text: *const u16,
    length: u32,
    attribute: i64,
}

/// `0xb519b0(layout, pane name, message, refresh, 0)`: finds the text pane of
/// that name in a layout and gives it the text. The boss gauge writes its
/// name with it. Returns how many panes took the text.
const SET_PANE_TEXT: usize = 0xb519b0;

pub unsafe fn set_pane_text(layout: usize, pane: &[u8], text: &[u16]) -> u32 {
    debug_assert!(pane.ends_with(&[0]) && text.ends_with(&[0]));
    let message = Message {
        text: text.as_ptr(),
        length: (text.len() - 1) as u32,
        attribute: -1,
    };
    let function: extern "C" fn(usize, *const u8, *const Message, u32, usize) -> u32 =
        core::mem::transmute(base() + SET_PANE_TEXT);
    function(layout, pane.as_ptr(), &message, 1, 0)
}
