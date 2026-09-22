#!/usr/bin/env python3
"""Convert a game's main.npdm into an npdmtool-compatible JSON descriptor.

Skyline needs an NPDM with wider kernel/service/filesystem permissions than the
game ships with (it maps process code memory to install hooks, talks to
fsp-srv / ldr:ro / bsd:u, ...). Rather than hand-writing one, decode the NPDM
found in the game's exefs and re-emit it as JSON, optionally widening the
permissions.

Usage:
    python3 npdm2json.py <main.npdm> <out.json> [--full-perms]
    python3 npdm2json.py <main.npdm> --dump          # human readable dump
"""

import json
import struct
import sys

SVC_NAMES = {
    0x01: "svcSetHeapSize", 0x02: "svcSetMemoryPermission", 0x03: "svcSetMemoryAttribute",
    0x04: "svcMapMemory", 0x05: "svcUnmapMemory", 0x06: "svcQueryMemory", 0x07: "svcExitProcess",
    0x08: "svcCreateThread", 0x09: "svcStartThread", 0x0a: "svcExitThread", 0x0b: "svcSleepThread",
    0x0c: "svcGetThreadPriority", 0x0d: "svcSetThreadPriority", 0x0e: "svcGetThreadCoreMask",
    0x0f: "svcSetThreadCoreMask", 0x10: "svcGetCurrentProcessorNumber", 0x11: "svcSignalEvent",
    0x12: "svcClearEvent", 0x13: "svcMapSharedMemory", 0x14: "svcUnmapSharedMemory",
    0x15: "svcCreateTransferMemory", 0x16: "svcCloseHandle", 0x17: "svcResetSignal",
    0x18: "svcWaitSynchronization", 0x19: "svcCancelSynchronization", 0x1a: "svcArbitrateLock",
    0x1b: "svcArbitrateUnlock", 0x1c: "svcWaitProcessWideKeyAtomic", 0x1d: "svcSignalProcessWideKey",
    0x1e: "svcGetSystemTick", 0x1f: "svcConnectToNamedPort", 0x20: "svcSendSyncRequestLight",
    0x21: "svcSendSyncRequest", 0x22: "svcSendSyncRequestWithUserBuffer",
    0x23: "svcSendAsyncRequestWithUserBuffer", 0x24: "svcGetProcessId", 0x25: "svcGetThreadId",
    0x26: "svcBreak", 0x27: "svcOutputDebugString", 0x28: "svcReturnFromException",
    0x29: "svcGetInfo", 0x2a: "svcFlushEntireDataCache", 0x2b: "svcFlushDataCache",
    0x2c: "svcMapPhysicalMemory", 0x2d: "svcUnmapPhysicalMemory",
    0x2e: "svcGetDebugFutureThreadInfo", 0x2f: "svcGetLastThreadInfo",
    0x30: "svcGetResourceLimitLimitValue", 0x31: "svcGetResourceLimitCurrentValue",
    0x32: "svcSetThreadActivity", 0x33: "svcGetThreadContext3", 0x34: "svcWaitForAddress",
    0x35: "svcSignalToAddress", 0x36: "svcSynchronizePreemptionState",
    0x37: "svcGetResourceLimitPeakValue", 0x39: "svcCreateIoPool", 0x3a: "svcCreateIoRegion",
    0x3c: "svcDumpInfo", 0x3d: "svcKernelDebug", 0x3e: "svcChangeKernelTraceState",
    0x40: "svcCreateSession", 0x41: "svcAcceptSession", 0x42: "svcReplyAndReceiveLight",
    0x43: "svcReplyAndReceive", 0x44: "svcReplyAndReceiveWithUserBuffer", 0x45: "svcCreateEvent",
    0x46: "svcMapIoRegion", 0x47: "svcUnmapIoRegion", 0x48: "svcMapPhysicalMemoryUnsafe",
    0x49: "svcUnmapPhysicalMemoryUnsafe", 0x4a: "svcSetUnsafeLimit", 0x4b: "svcCreateCodeMemory",
    0x4c: "svcControlCodeMemory", 0x4d: "svcSleepSystem", 0x4e: "svcReadWriteRegister",
    0x4f: "svcSetProcessActivity", 0x50: "svcCreateSharedMemory",
    0x51: "svcMapTransferMemory", 0x52: "svcUnmapTransferMemory",
    0x53: "svcCreateInterruptEvent", 0x54: "svcQueryPhysicalAddress",
    0x55: "svcQueryIoMapping", 0x56: "svcCreateDeviceAddressSpace",
    0x57: "svcAttachDeviceAddressSpace", 0x58: "svcDetachDeviceAddressSpace",
    0x59: "svcMapDeviceAddressSpaceByForce", 0x5a: "svcMapDeviceAddressSpaceAligned",
    0x5b: "svcMapDeviceAddressSpace", 0x5c: "svcUnmapDeviceAddressSpace",
    0x5d: "svcInvalidateProcessDataCache", 0x5e: "svcStoreProcessDataCache",
    0x5f: "svcFlushProcessDataCache", 0x60: "svcDebugActiveProcess",
    0x61: "svcBreakDebugProcess", 0x62: "svcTerminateDebugProcess", 0x63: "svcGetDebugEvent",
    0x64: "svcContinueDebugEvent", 0x65: "svcGetProcessList", 0x66: "svcGetThreadList",
    0x67: "svcGetDebugThreadContext", 0x68: "svcSetDebugThreadContext",
    0x69: "svcQueryDebugProcessMemory", 0x6a: "svcReadDebugProcessMemory",
    0x6b: "svcWriteDebugProcessMemory", 0x6c: "svcSetHardwareBreakPoint",
    0x6d: "svcGetDebugThreadParam", 0x6f: "svcGetSystemInfo", 0x70: "svcCreatePort",
    0x71: "svcManageNamedPort", 0x72: "svcConnectToPort", 0x73: "svcSetProcessMemoryPermission",
    0x74: "svcMapProcessMemory", 0x75: "svcUnmapProcessMemory", 0x76: "svcQueryProcessMemory",
    0x77: "svcMapProcessCodeMemory", 0x78: "svcUnmapProcessCodeMemory", 0x79: "svcCreateProcess",
    0x7a: "svcStartProcess", 0x7b: "svcTerminateProcess", 0x7c: "svcGetProcessInfo",
    0x7d: "svcCreateResourceLimit", 0x7e: "svcSetResourceLimitLimitValue",
    0x7f: "svcCallSecureMonitor", 0x90: "svcMapInsecureMemory", 0x91: "svcUnmapInsecureMemory",
}


def parse_kac(data):
    """Decode a kernel access control blob into npdmtool JSON capabilities."""
    caps = []
    syscalls = {}
    for (desc,) in struct.iter_unpack("<I", data):
        if desc == 0xFFFFFFFF:
            continue
        # The capability type is encoded as the number of trailing set bits.
        bits = 0
        while desc & (1 << bits):
            bits += 1
        value = desc >> (bits + 1)
        if bits == 3:  # kernel_flags
            caps.append({
                "type": "kernel_flags",
                "value": {
                    "lowest_thread_priority": value & 0x3F,
                    "highest_thread_priority": (value >> 6) & 0x3F,
                    "lowest_cpu_id": (value >> 12) & 0xFF,
                    "highest_cpu_id": (value >> 20) & 0xFF,
                },
            })
        elif bits == 4:  # syscalls
            index = (value >> 24) & 0x7
            mask = value & 0xFFFFFF
            for bit in range(24):
                if mask & (1 << bit):
                    num = index * 0x18 + bit
                    syscalls[SVC_NAMES.get(num, "svc_%02x" % num)] = "0x%02x" % num
        elif bits == 6:  # map normal/io, always emitted as a pair
            caps.append({"type": "_raw_map", "value": "0x%08x" % desc})
        elif bits == 7:  # map_page
            caps.append({"type": "map_page", "value": "0x%x" % ((value & 0xFFFFFF) << 12)})
        elif bits == 10:  # map_region
            regions = []
            for i in range(3):
                field = (value >> (7 * i)) & 0x7F
                regions.append({"region_type": field & 0x3F, "is_ro": bool(field >> 6)})
            caps.append({"type": "map_region", "value": regions})
        elif bits == 11:  # irq_pair
            irqs = []
            for i in range(2):
                irq = (value >> (10 * i)) & 0x3FF
                irqs.append(None if irq == 0x3FF else irq)
            caps.append({"type": "irq_pair", "value": irqs})
        elif bits == 13:  # application_type
            caps.append({"type": "application_type", "value": value & 7})
        elif bits == 14:  # min_kernel_version
            caps.append({"type": "min_kernel_version", "value": "0x%x" % (value & 0xFFFF)})
        elif bits == 15:  # handle_table_size
            caps.append({"type": "handle_table_size", "value": value & 0x3FF})
        elif bits == 16:  # debug_flags
            caps.append({
                "type": "debug_flags",
                "value": {
                    "allow_debug": bool(value & 1),
                    "force_debug_prod": bool(value & 2),
                    "force_debug": bool(value & 4),
                },
            })
        else:
            caps.append({"type": "_unknown", "value": "0x%08x" % desc})
    if syscalls:
        caps.insert(0, {"type": "syscalls", "value": syscalls})
    return caps


def parse_fah(blob):
    _version, perms, coi_off, coi_size, sdoi_off, sdoi_size = struct.unpack_from("<IQIIII", blob, 0)
    out = {"permissions": "0x%016x" % perms}
    if sdoi_size >= 4:
        count = struct.unpack_from("<I", blob, sdoi_off)[0]
        acc_off = sdoi_off + 4
        id_off = acc_off + ((count + 3) & ~3)
        ids = []
        for i in range(count):
            ids.append({
                "accessibility": blob[acc_off + i],
                "id": "0x%016x" % struct.unpack_from("<Q", blob, id_off + 8 * i)[0],
            })
        if ids:
            out["save_data_owner_ids"] = ids
    if coi_size >= 4:
        count = struct.unpack_from("<I", blob, coi_off)[0]
        ids = ["0x%016x" % struct.unpack_from("<Q", blob, coi_off + 4 + 8 * i)[0]
               for i in range(count)]
        if ids:
            out["content_owner_ids"] = ids
    return out


def parse_sac(blob):
    """Decode a service access control blob into (name, is_host) pairs."""
    services = []
    pos = 0
    while pos < len(blob):
        control = blob[pos]
        pos += 1
        length = (control & 0x7) + 1
        name = blob[pos:pos + length].decode("utf-8", "replace")
        pos += length
        services.append((name, bool(control & 0x80)))
    return services


def parse_npdm(path):
    with open(path, "rb") as f:
        data = f.read()
    if data[:4] != b"META":
        raise SystemExit("%s is not an NPDM (bad magic)" % path)

    (_magic, sig_key_gen, _8, mmu_flags, _d, main_prio, default_cpu, _10,
     sys_res_size, version, stack_size) = struct.unpack_from("<IIIBBBBIIII", data, 0)
    name = data[0x20:0x30].split(b"\0")[0].decode()
    aci0_off, aci0_size, acid_off, acid_size = struct.unpack_from("<IIII", data, 0x70)

    acid = data[acid_off:acid_off + acid_size]
    (_acid_magic, _size, _208, flags, pid_min, pid_max,
     _fac_off, _fac_size, _sac_off, _sac_size,
     _kac_off, _kac_size) = struct.unpack_from("<IIIIQQIIIIII", acid, 0x200)

    aci0 = data[aci0_off:aci0_off + aci0_size]
    program_id = struct.unpack_from("<Q", aci0, 0x10)[0]
    (fah_off, fah_size, sac_off, sac_size,
     kac_off, kac_size) = struct.unpack_from("<IIIIII", aci0, 0x20)

    sac = parse_sac(aci0[sac_off:sac_off + sac_size])
    return {
        "name": name,
        "title_id": "0x%016x" % program_id,
        "title_id_range_min": "0x%016x" % pid_min,
        "title_id_range_max": "0x%016x" % pid_max,
        "main_thread_stack_size": "0x%08x" % stack_size,
        "main_thread_priority": main_prio,
        "default_cpu_id": default_cpu,
        "version": "0x%08x" % version,
        "system_resource_size": "0x%08x" % sys_res_size,
        "is_retail": bool(flags & 1),
        "pool_partition": (flags >> 2) & 0xF,
        "is_64_bit": bool(mmu_flags & 1),
        "address_space_type": (mmu_flags >> 1) & 0x7,
        "optimize_memory_allocation": bool(mmu_flags & 0x10),
        "disable_device_address_space_merge": bool(mmu_flags & 0x20),
        "enable_alias_region_extra_size": bool(mmu_flags & 0x40),
        "signature_key_generation": sig_key_gen,
        "filesystem_access": parse_fah(aci0[fah_off:fah_off + fah_size]),
        "service_access": [n for n, host in sac if not host],
        "service_host": [n for n, host in sac if host],
        "kernel_capabilities": parse_kac(aci0[kac_off:kac_off + kac_size]),
    }


def widen(desc):
    """Grant everything Skyline needs on top of the game's own permissions."""
    desc["filesystem_access"]["permissions"] = "0xffffffffffffffff"
    # "*" is the wildcard sm matches any service name against.
    desc["service_access"] = ["*"]
    desc["service_host"] = []
    for cap in desc["kernel_capabilities"]:
        if cap["type"] == "syscalls":
            allowed = dict(cap["value"])
            for num, svc_name in SVC_NAMES.items():
                allowed[svc_name] = "0x%02x" % num
            cap["value"] = dict(sorted(allowed.items(), key=lambda kv: int(kv[1], 16)))
        elif cap["type"] == "debug_flags":
            cap["value"]["allow_debug"] = True
        elif cap["type"] == "handle_table_size":
            # Skyline and its plugins create extra threads, sessions and events.
            cap["value"] = max(int(cap["value"]), 0x200)
    return desc


def main(argv):
    if len(argv) < 2:
        raise SystemExit(__doc__)
    desc = parse_npdm(argv[1])

    if "--dump" in argv:
        print(json.dumps(desc, indent=4))
        return

    if len(argv) < 3:
        raise SystemExit(__doc__)
    if "--full-perms" in argv:
        desc = widen(desc)

    unknown = [c for c in desc["kernel_capabilities"] if c["type"].startswith("_")]
    if unknown:
        print("warning: %d kernel capabilities could not be re-encoded: %s" % (len(unknown), unknown),
              file=sys.stderr)
        desc["kernel_capabilities"] = [c for c in desc["kernel_capabilities"]
                                       if not c["type"].startswith("_")]

    with open(argv[2], "w") as f:
        json.dump(desc, f, indent=4)
        f.write("\n")
    print("wrote %s" % argv[2])


if __name__ == "__main__":
    main(sys.argv)
