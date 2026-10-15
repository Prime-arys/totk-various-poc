#pragma once

/* What exlaunch asks of the program it is built around (see the file of the
   same name in utils/exlaunch/source/program, which this one replaces). */

#include "common.hpp"

/* The name the module carries, in exlaunch's log lines and in a crash report. */
#define EXL_MODULE_NAME "enemy-hp-se"

/* Failed checks say what failed, in the debug log, before stopping the game. */
#define EXL_DEBUG
/* The few allocations of the C library come out of a buffer of the module:
   the game's allocator is not up yet when the module starts. */
#define EXL_USE_FAKEHEAP

namespace exl::setting {
    /* How large the fake .bss heap will be. */
    constexpr size_t HeapSize = 0x5000;

    /* Room for the trampolines of hooks (two are placed). */
    constexpr size_t JitSize = 0x1000;

    /* Room for the entries of inline hooks (two are placed). */
    constexpr size_t InlinePoolSize = 0x1000;

    /* Longest log line; the buffer is on the stack. */
    constexpr size_t LogBufferSize = 512;

    static_assert(ALIGN_UP(JitSize, PAGE_SIZE) == JitSize, "");
    static_assert(ALIGN_UP(InlinePoolSize, PAGE_SIZE) == InlinePoolSize, "");
}
