//! What a Rust library without `std` needs from the program it is linked
//! into: memory (newlib's malloc, which libnx sets up) and a way to report a
//! panic before stopping.

use core::alloc::{GlobalAlloc, Layout};
use core::ffi::c_void;

extern "C" {
    fn malloc(size: usize) -> *mut c_void;
    fn free(pointer: *mut c_void);
    fn realloc(pointer: *mut c_void, size: usize) -> *mut c_void;
    fn memalign(alignment: usize, size: usize) -> *mut c_void;
    /// Logs the message and stops the program (source/core/host.cpp).
    fn tkm_host_panic(message: *const u8, length: usize) -> !;
}

/// newlib's malloc returns 16-byte aligned blocks on AArch64.
const MALLOC_ALIGNMENT: usize = 16;

struct Newlib;

unsafe impl GlobalAlloc for Newlib {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.align() <= MALLOC_ALIGNMENT {
            malloc(layout.size()) as *mut u8
        } else {
            memalign(layout.align(), layout.size()) as *mut u8
        }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, _: Layout) {
        free(pointer as *mut c_void)
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if layout.align() <= MALLOC_ALIGNMENT {
            return realloc(pointer as *mut c_void, new_size) as *mut u8;
        }
        let new_layout = Layout::from_size_align_unchecked(new_size, layout.align());
        let new_pointer = self.alloc(new_layout);
        if !new_pointer.is_null() {
            core::ptr::copy_nonoverlapping(pointer, new_pointer, layout.size().min(new_size));
            self.dealloc(pointer, layout);
        }
        new_pointer
    }
}

#[global_allocator]
static ALLOCATOR: Newlib = Newlib;

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    use core::fmt::Write;

    // Formatted on the stack: the panic may come from a failed allocation.
    struct Buffer {
        bytes: [u8; 512],
        length: usize,
    }
    impl Write for Buffer {
        fn write_str(&mut self, text: &str) -> core::fmt::Result {
            let room = self.bytes.len() - self.length;
            let count = text.len().min(room);
            self.bytes[self.length..self.length + count].copy_from_slice(&text.as_bytes()[..count]);
            self.length += count;
            Ok(())
        }
    }

    let mut buffer = Buffer {
        bytes: [0; 512],
        length: 0,
    };
    let _ = write!(buffer, "{}", info);
    unsafe { tkm_host_panic(buffer.bytes.as_ptr(), buffer.length) }
}
