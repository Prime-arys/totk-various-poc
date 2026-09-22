//! Where a mod plugin gets memory for big buffers.
//!
//! Tears of the Kingdom's own allocator has very little to spare while plugins
//! run: this plugin asking for the 1.7 MiB of a decompressed actor pack got
//! nothing back and brought the game down with it. skyline-totk maps memory
//! from the kernel for exactly this (`totk_map_memory`, out of the
//! application's memory pool rather than the game's heap), which is also what
//! the merger's own allocator is built on.
//!
//! So: blocks of [`BIG`] bytes and over come from the kernel, everything else
//! (strings, document nodes) from the game as usual. Any plugin that reads
//! game files needs something of this shape.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

const PAGE: usize = 0x1000;

/// Blocks this big are mapped rather than taken from the game's heap. Low
/// enough that a game file never goes to the game, high enough that ordinary
/// allocations do not each cost a mapping (skyline-totk tracks 64 of them).
const BIG: usize = 128 * 1024;

type MapFn = extern "C" fn(u64) -> *mut u8;
type UnmapFn = extern "C" fn(*mut u8, u64) -> bool;

/// 0 = not looked up yet, 1 = unavailable, otherwise the address.
static MAP: AtomicUsize = AtomicUsize::new(0);
static UNMAP: AtomicUsize = AtomicUsize::new(0);

fn lookup(name: &[u8], cache: &AtomicUsize) -> Option<usize> {
    match cache.load(Ordering::Relaxed) {
        0 => {}
        1 => return None,
        address => return Some(address),
    }
    let mut address: usize = 0;
    let result = unsafe { skyline::nn::ro::LookupSymbol(&mut address, name.as_ptr()) };
    let resolved = if result == 0 && address != 0 { address } else { 1 };
    cache.store(resolved, Ordering::Relaxed);
    (resolved != 1).then_some(resolved)
}

fn map() -> Option<MapFn> {
    lookup(b"totk_map_memory\0", &MAP).map(|address| unsafe { core::mem::transmute(address) })
}

fn unmap() -> Option<UnmapFn> {
    lookup(b"totk_unmap_memory\0", &UNMAP).map(|address| unsafe { core::mem::transmute(address) })
}

pub struct Scratch;

unsafe impl GlobalAlloc for Scratch {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.size() >= BIG && layout.align() <= PAGE {
            if let Some(map) = map() {
                let pointer = map(layout.size() as u64);
                if !pointer.is_null() {
                    return pointer;
                }
            }
        }
        System.alloc(layout)
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        if layout.size() >= BIG && pointer as usize % PAGE == 0 {
            if let Some(unmap) = unmap() {
                // Refused for a pointer that came from the game's heap.
                if unmap(pointer, layout.size() as u64) {
                    return;
                }
            }
        }
        System.dealloc(pointer, layout)
    }
}
