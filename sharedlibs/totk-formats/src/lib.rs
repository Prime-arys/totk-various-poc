//! File formats used by The Legend of Zelda: Tears of the Kingdom.
//!
//! This crate is deliberately free of any Switch/skyline dependency so it can
//! be unit tested on a PC against real game files, then compiled as-is into the
//! on-console plugin. It only needs `alloc`: without the `std` feature it builds
//! for bare metal, which is how the mod manager homebrew links it (see `sys`).

#![no_std]

extern crate alloc;
#[cfg(any(feature = "std", test))]
extern crate std;

/// The `alloc` names the rest of the code uses as if they were `std`'s prelude.
pub mod prelude {
    pub use alloc::borrow::ToOwned;
    pub use alloc::boxed::Box;
    pub use alloc::format;
    pub use alloc::string::{String, ToString};
    pub use alloc::vec;
    pub use alloc::vec::Vec;
}

use prelude::*;

pub mod byml;
pub mod crc32;
pub mod msbt;
pub mod rstb;
pub mod sarc;
pub mod sys;
pub mod vecmap;
pub mod xxhash;
pub mod zip;
pub mod zstd;

#[derive(Debug)]
pub enum Error {
    /// A file did not start with the magic it claimed to have.
    BadMagic { expected: &'static str, got: [u8; 4] },
    /// The file ended before a structure it declared was complete.
    Truncated { what: &'static str },
    /// Something in the file is self-inconsistent (bad offset, bad count, ...).
    Invalid(&'static str),
    /// zstd decoding failed.
    Zstd(String),
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::BadMagic { expected, got } => {
                write!(f, "expected {} magic, got {:02X?}", expected, got)
            }
            Error::Truncated { what } => write!(f, "truncated {}", what),
            Error::Invalid(msg) => write!(f, "invalid data: {}", msg),
            Error::Zstd(msg) => write!(f, "zstd: {}", msg),
        }
    }
}

pub type Result<T> = core::result::Result<T, Error>;

// --- little endian readers, bounds checked ----------------------------------

pub(crate) fn u16_at(data: &[u8], off: usize, what: &'static str) -> Result<u16> {
    data.get(off..off + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or(Error::Truncated { what })
}

pub(crate) fn u32_at(data: &[u8], off: usize, what: &'static str) -> Result<u32> {
    data.get(off..off + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or(Error::Truncated { what })
}

pub(crate) fn magic_at(data: &[u8], off: usize, expected: &'static str) -> Result<()> {
    let got = data.get(off..off + 4).ok_or(Error::Truncated { what: expected })?;
    if got == expected.as_bytes() {
        Ok(())
    } else {
        Err(Error::BadMagic {
            expected,
            got: [got[0], got[1], got[2], got[3]],
        })
    }
}
