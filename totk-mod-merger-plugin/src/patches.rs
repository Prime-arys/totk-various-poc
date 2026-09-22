//! Code patches from mods (`.ips`, `.pchtxt`), applied in memory.
//!
//! TKMM writes them to an IPS file for Atmosphère to apply at the next boot;
//! here the game is already loaded, so they are written straight into the main
//! module, which is just as early: the game has not run past its romfs mount.

use skyline::hooks::{getRegionAddress, Region};
use skyline::patching::Patch;

use crate::log;

/// `patches`: (offset from the start of the main module, instruction bytes as
/// a big endian value).
pub fn apply(patches: &[(u32, u32)]) {
    if patches.is_empty() {
        return;
    }

    let (text, bss) = unsafe {
        (
            getRegionAddress(Region::Text) as usize,
            getRegionAddress(Region::Bss) as usize,
        )
    };
    // Code and data both live between the start of .text and .bss.
    let module_size = bss.saturating_sub(text);

    let mut applied = 0;
    for &(offset, value) in patches {
        let offset = offset as usize;
        if offset + 4 > module_size {
            log::write(&format!("patch at {:#x} is outside the game's code, skipped", offset));
            continue;
        }
        match Patch::in_text(offset).bytes(value.to_be_bytes()) {
            Ok(()) => applied += 1,
            Err(error) => log::write(&format!("could not patch {:#x}: {:?}", offset, error)),
        }
    }
    log::write(&format!("applied {} code patch(es)", applied));
}
