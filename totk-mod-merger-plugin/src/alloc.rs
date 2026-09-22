//! Global allocator: a heap of the plugin's own, on memory mapped from the
//! kernel.
//!
//! The game's allocator has very little to spare while plugins run (a probe on
//! TotK 1.2.1 got 1, 2 and 4 MiB buffers but was refused an 8 MiB one), and a
//! merge makes millions of small allocations: parsing a GameDataList alone
//! creates over a million nodes. Taking those from the game would exhaust its
//! heap and hang the boot.
//!
//! So the plugin allocates from memory skyline-totk maps with
//! svcMapPhysicalMemory (`totk_map_memory`), which comes from the application's
//! memory pool rather than the game's heap:
//!
//! - blocks of up to 16 MiB come from power-of-two size classes carved out of
//!   64 MiB chunks, with a free list per class (so even the large arrays of a
//!   GameDataList do not each use one of skyline-totk's 64 mappings);
//! - bigger blocks get a mapping of their own;
//! - [`trim`] hands chunks nothing uses any more back to the kernel once the
//!   merge is over.
//!
//! When no scratch memory is available (another Skyline, or the kernel refuses)
//! everything falls back to the game's allocator.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

const PAGE: usize = 0x1000;
const CHUNK: usize = 64 * 1024 * 1024;
/// The first chunk is small: whatever a merge leaks for good (thread-local
/// data of the merge thread, say) is allocated first and ends up there, and it
/// is the one chunk [`trim`] cannot give back.
const FIRST_CHUNK: usize = 2 * 1024 * 1024;
/// skyline-totk tracks 64 mappings; leave room for the largest blocks.
const MAX_CHUNKS: usize = 48;
const MIN_CLASS_SHIFT: u32 = 4;
const MAX_CLASS_SHIFT: u32 = 24;
const CLASSES: usize = (MAX_CLASS_SHIFT - MIN_CLASS_SHIFT + 1) as usize;

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
    let result = unsafe { skyline::nn::ro::LookupSymbol(&mut address as *mut usize, name.as_ptr()) };
    let resolved = if result == 0 && address != 0 { address } else { 1 };
    cache.store(resolved, Ordering::Relaxed);
    (resolved != 1).then_some(resolved)
}

fn scratch_map() -> Option<MapFn> {
    lookup(b"totk_map_memory\0", &MAP).map(|address| unsafe { core::mem::transmute(address) })
}

fn scratch_unmap() -> Option<UnmapFn> {
    lookup(b"totk_unmap_memory\0", &UNMAP).map(|address| unsafe { core::mem::transmute(address) })
}

#[derive(Clone, Copy)]
struct Chunk {
    base: usize,
    size: usize,
    /// Blocks handed out and not freed yet.
    live: usize,
}

struct Heap {
    free: [usize; CLASSES],
    chunks: [Chunk; MAX_CHUNKS],
    chunk_count: usize,
    /// Bump region inside the newest chunk.
    cursor: usize,
    end: usize,
}

struct Locked {
    busy: AtomicBool,
    heap: UnsafeCell<Heap>,
}

unsafe impl Sync for Locked {}

static HEAP: Locked = Locked {
    busy: AtomicBool::new(false),
    heap: UnsafeCell::new(Heap {
        free: [0; CLASSES],
        chunks: [Chunk { base: 0, size: 0, live: 0 }; MAX_CHUNKS],
        chunk_count: 0,
        cursor: 0,
        end: 0,
    }),
};

/// Set once mapping a chunk fails, so the allocator stops retrying on every
/// allocation.
static EXHAUSTED: AtomicBool = AtomicBool::new(false);

/// Whether small blocks come from the scratch heap. Only while merging: what
/// outlives the merge is copied to the game's heap, so every chunk can be
/// handed back afterwards.
static SCRATCH_ON: AtomicBool = AtomicBool::new(false);

pub fn use_scratch(enabled: bool) {
    SCRATCH_ON.store(enabled, Ordering::Release);
}

struct Guard;

fn lock() -> Guard {
    while HEAP
        .busy
        .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
    Guard
}

impl Drop for Guard {
    fn drop(&mut self) {
        HEAP.busy.store(false, Ordering::Release);
    }
}

/// Size class of a layout, or `None` for blocks served by their own mapping.
fn class_of(layout: Layout) -> Option<(usize, usize)> {
    let size = layout.size().max(layout.align()).max(1 << MIN_CLASS_SHIFT).next_power_of_two();
    let shift = size.trailing_zeros();
    (shift <= MAX_CLASS_SHIFT && layout.align() <= PAGE).then(|| ((shift - MIN_CLASS_SHIFT) as usize, size))
}

impl Heap {
    fn chunk_index(&self, address: usize) -> Option<usize> {
        self.chunks[..self.chunk_count]
            .iter()
            .position(|chunk| chunk.base != 0 && address >= chunk.base && address < chunk.base + chunk.size)
    }

    unsafe fn allocate(&mut self, class: usize, size: usize) -> *mut u8 {
        let head = self.free[class];
        if head != 0 {
            self.free[class] = *(head as *const usize);
            if let Some(index) = self.chunk_index(head) {
                self.chunks[index].live += 1;
            }
            return head as *mut u8;
        }

        let alignment = size.min(PAGE);
        let mut start = (self.cursor + alignment - 1) & !(alignment - 1);
        if self.cursor == 0 || start + size > self.end {
            if !self.grow(size) {
                return core::ptr::null_mut();
            }
            start = self.cursor;
        }
        self.cursor = start + size;
        if let Some(index) = self.chunk_index(start) {
            self.chunks[index].live += 1;
        }
        start as *mut u8
    }

    unsafe fn grow(&mut self, block: usize) -> bool {
        if EXHAUSTED.load(Ordering::Relaxed) {
            return false;
        }
        let Some(slot) = (0..MAX_CHUNKS).find(|&i| i >= self.chunk_count || self.chunks[i].base == 0) else {
            EXHAUSTED.store(true, Ordering::Relaxed);
            return false;
        };
        let Some(map) = scratch_map() else {
            EXHAUSTED.store(true, Ordering::Relaxed);
            return false;
        };
        let wanted = if self.chunks.iter().all(|c| c.base == 0) { FIRST_CHUNK } else { CHUNK };
        let size = wanted.max((block + PAGE - 1) & !(PAGE - 1));
        let base = map(size as u64) as usize;
        if base == 0 {
            EXHAUSTED.store(true, Ordering::Relaxed);
            return false;
        }
        self.chunks[slot] = Chunk { base, size, live: 0 };
        self.chunk_count = self.chunk_count.max(slot + 1);
        self.cursor = base;
        self.end = base + size;
        true
    }

    unsafe fn release(&mut self, pointer: *mut u8, class: usize, index: usize) {
        *(pointer as *mut usize) = self.free[class];
        self.free[class] = pointer as usize;
        self.chunks[index].live -= 1;
    }
}

pub struct TotkAllocator;

unsafe impl GlobalAlloc for TotkAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        match class_of(layout) {
            Some(_) if !SCRATCH_ON.load(Ordering::Acquire) => {}
            Some((class, size)) => {
                let pointer = {
                    let _guard = lock();
                    (*HEAP.heap.get()).allocate(class, size)
                };
                if !pointer.is_null() {
                    return pointer;
                }
            }
            None if layout.align() <= PAGE => {
                if let Some(map) = scratch_map() {
                    let pointer = map(layout.size() as u64);
                    if !pointer.is_null() {
                        return pointer;
                    }
                }
            }
            None => {}
        }
        System.alloc(layout)
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        match class_of(layout) {
            Some((class, _)) => {
                let _guard = lock();
                let heap = &mut *HEAP.heap.get();
                if let Some(index) = heap.chunk_index(pointer as usize) {
                    heap.release(pointer, class, index);
                    return;
                }
            }
            None => {
                if pointer as usize % PAGE == 0 {
                    if let Some(unmap) = scratch_unmap() {
                        // Refused for pointers that did not come from a mapping.
                        if unmap(pointer, layout.size() as u64) {
                            return;
                        }
                    }
                }
            }
        }
        System.dealloc(pointer, layout)
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_layout = Layout::from_size_align_unchecked(new_size, layout.align());
        if !SCRATCH_ON.load(Ordering::Acquire) && class_of(layout).is_some() && class_of(new_layout).is_some() {
            let in_heap = {
                let _guard = lock();
                (*HEAP.heap.get()).chunk_index(pointer as usize).is_some()
            };
            if !in_heap {
                return System.realloc(pointer, layout, new_size);
            }
        }
        if let (Some((old, _)), Some((new, _))) = (class_of(layout), class_of(new_layout)) {
            if old == new {
                let _guard = lock();
                if (*HEAP.heap.get()).chunk_index(pointer as usize).is_some() {
                    return pointer;
                }
            }
        }

        let new_pointer = self.alloc(new_layout);
        if !new_pointer.is_null() {
            core::ptr::copy_nonoverlapping(pointer, new_pointer, layout.size().min(new_size));
            self.dealloc(pointer, layout);
        }
        new_pointer
    }
}

/// Returns chunks no live block uses to the kernel. Returns (chunks released,
/// chunks still mapped).
pub fn trim() -> (usize, usize) {
    let Some(unmap) = scratch_unmap() else {
        return (0, 0);
    };
    let _guard = lock();
    let heap = unsafe { &mut *HEAP.heap.get() };

    // No allocation in here: the heap lock is held and is not reentrant.
    let mut empty = [false; MAX_CHUNKS];
    let mut any = false;
    for i in 0..heap.chunk_count {
        if heap.chunks[i].base != 0 && heap.chunks[i].live == 0 {
            empty[i] = true;
            any = true;
        }
    }
    if !any {
        let mapped = heap.chunks[..heap.chunk_count].iter().filter(|c| c.base != 0).count();
        return (0, mapped);
    }

    let in_empty_chunk = |heap: &Heap, address: usize| {
        (0..heap.chunk_count)
            .any(|i| empty[i] && address >= heap.chunks[i].base && address < heap.chunks[i].base + heap.chunks[i].size)
    };

    // Unlink every free block that lives in a chunk about to go away.
    for class in 0..CLASSES {
        let mut previous: usize = 0;
        let mut current = heap.free[class];
        while current != 0 {
            let next = unsafe { *(current as *const usize) };
            if in_empty_chunk(heap, current) {
                if previous == 0 {
                    heap.free[class] = next;
                } else {
                    unsafe { *(previous as *mut usize) = next };
                }
            } else {
                previous = current;
            }
            current = next;
        }
    }

    let mut released = 0;
    for i in (0..heap.chunk_count).filter(|&i| empty[i]) {
        let (base, size) = (heap.chunks[i].base, heap.chunks[i].size);
        if heap.cursor >= base && heap.cursor <= base + size {
            heap.cursor = 0;
            heap.end = 0;
        }
        if unmap(base as *mut u8, size as u64) {
            heap.chunks[i].base = 0;
            released += 1;
        }
    }
    EXHAUSTED.store(false, Ordering::Relaxed);
    let mapped = heap.chunks[..heap.chunk_count].iter().filter(|c| c.base != 0).count();
    (released, mapped)
}
