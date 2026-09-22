// Scratch memory for plugins.
//
// Tears of the Kingdom's own allocator only has a few megabytes to spare when
// plugins run (measured on 1.2.1: buffers of 1, 2 and 4 MiB succeed, an 8 MiB
// one does not). A plugin that has to decompress or rebuild a large game file
// would either fail or starve the game.
//
// This hands out memory straight from the kernel instead, mapped into the
// process' address space with svcMapPhysicalMemory, the same mechanism the
// hooking engine uses for its trampolines. It comes out of the application's
// memory pool rather than the game's heap.

#include "nn/os.hpp"
#include "skyline/logger/Logger.hpp"
#include "types.h"

#ifdef __cplusplus
extern "C" {
#endif

#include "skyline/nx/kernel/svc.h"
#include "skyline/nx/kernel/virtmem.h"

#ifdef __cplusplus
}
#endif

#define PAGE_SIZE 0x1000

namespace {

    // Every live mapping, so a stray pointer can never be handed to
    // svcUnmapPhysicalMemory. A plugin merge holds a handful at a time.
    constexpr int MaxMappings = 64;

    struct Mapping {
        void* address;
        u64 size;
    };

    Mapping g_Mappings[MaxMappings];
    nn::os::MutexType g_Mutex;
    bool g_MutexReady = false;

    void lock() {
        if (!g_MutexReady) {
            nn::os::InitializeMutex(&g_Mutex, true, 0);
            g_MutexReady = true;
        }
        nn::os::LockMutex(&g_Mutex);
    }

    void unlock() { nn::os::UnlockMutex(&g_Mutex); }

    // svcMapPhysicalMemory only accepts addresses inside the process' alias
    // region, so the generic virtmem reservation (which scans the whole ASLR
    // space) is not usable here.
    u64 findAliasHole(u64 size) {
        u64 base = 0, region_size = 0;
        if (R_FAILED(svcGetInfo(&base, InfoType_AliasRegionAddress, CUR_PROCESS_HANDLE, 0)) ||
            R_FAILED(svcGetInfo(&region_size, InfoType_AliasRegionSize, CUR_PROCESS_HANDLE, 0)))
            return 0;

        u64 address = base;
        const u64 end = base + region_size;

        while (address + size <= end) {
            MemoryInfo info;
            u32 pageInfo;
            if (R_FAILED(svcQueryMemory(&info, &pageInfo, address))) return 0;

            u64 block_end = info.addr + info.size;
            if (block_end <= address) return 0;  // no progress, give up

            if (info.type == MemType_Unmapped) {
                u64 start = ALIGN_UP(address, PAGE_SIZE);
                if (start + size <= block_end) return start;
            }

            address = block_end;
        }

        return 0;
    }

};  // namespace

extern "C" {

// Maps `size` bytes (rounded up to a page) and returns the address, or null.
void* totk_map_memory(u64 size) {
    if (size == 0) return nullptr;
    size = ALIGN_UP(size, PAGE_SIZE);

    lock();

    int slot = -1;
    for (int i = 0; i < MaxMappings; i++) {
        if (g_Mappings[i].address == nullptr) {
            slot = i;
            break;
        }
    }
    if (slot < 0) {
        unlock();
        return nullptr;
    }

    u64 address = findAliasHole(size);
    if (address == 0) {
        unlock();
        skyline::logger::s_Instance->LogFormat("[scratch] no room in the alias region for 0x%lx bytes", size);
        return nullptr;
    }

    Result rc = svcMapPhysicalMemory(reinterpret_cast<void*>(address), size);
    if (R_FAILED(rc)) {
        unlock();
        skyline::logger::s_Instance->LogFormat("[scratch] svcMapPhysicalMemory(0x%lx, 0x%lx) failed: 0x%x", address,
                                               size, rc);
        return nullptr;
    }

    g_Mappings[slot] = {reinterpret_cast<void*>(address), size};
    unlock();
    return reinterpret_cast<void*>(address);
}

// Releases memory from totk_map_memory. Returns false (and does nothing) for a
// pointer that did not come from it.
bool totk_unmap_memory(void* address, u64 size) {
    if (address == nullptr || size == 0) return false;
    size = ALIGN_UP(size, PAGE_SIZE);

    lock();

    int slot = -1;
    for (int i = 0; i < MaxMappings; i++) {
        if (g_Mappings[i].address == address && g_Mappings[i].size == size) {
            slot = i;
            break;
        }
    }
    if (slot < 0) {
        unlock();
        return false;
    }

    Result rc = svcUnmapPhysicalMemory(address, size);
    if (R_SUCCEEDED(rc)) g_Mappings[slot] = {nullptr, 0};

    unlock();
    return R_SUCCEEDED(rc);
}
}
