// Compatibility shims for plugins built against older nnSdk versions.
//
// Rust plugins built with cargo-skyline link against `libc-nnsdk`, which was
// written for the SDK Smash Ultimate shipped with. A few of its symbols were
// renamed in later SDKs, and Tears of the Kingdom (SDK 15.3.1) no longer
// exports them. A plugin importing one would fail to bind, or jump to nothing
// the first time it was called, so the subsdk provides them instead.

#include "types.h"

extern "C" {

// The game exports the modern spellings.
int* __errno_location();
int pthread_join(unsigned long thread, void** value);

// libc-nnsdk declares this as returning a pointer to a 64 bit errno. Only reads
// go through it (Rust's std does `*errno_loc() as i32`), and the extra four
// bytes are ignored on a little endian target, so handing back the real 32 bit
// slot is safe.
long* __nnmusl_ErrnoLocation() { return reinterpret_cast<long*>(__errno_location()); }

// libc-nnsdk asks for the musl-internal alias of pthread_join, which newer SDKs
// no longer export. Plugins that spawn a thread and join it need this.
int __pthread_join(unsigned long thread, void** value) { return pthread_join(thread, value); }
}
