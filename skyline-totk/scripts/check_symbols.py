#!/usr/bin/env python3
"""Check that every symbol a module imports is exported by something else.

The subsdk resolves all of its nn::* calls through rtld against the game's own
modules; plugins resolve theirs against the game *and* the subsdk. If a game
update drops or renames one of them, the module still builds but fails at load
time. Run this after any game update:

    # the subsdk against the game
    python3 scripts/check_symbols.py <exefs dir> skyline-totk.elf

    # a plugin against the game plus the subsdk
    python3 scripts/check_symbols.py <exefs dir> skyline-totk.elf -- plugin.so

Providers may be directories (every NSO in them is read), NSO files or ELF
files. The module to check is the last argument, or after a "--" separator.
"""

import os
import struct
import sys


# --- LZ4 block decompression (NSO segments are LZ4-block compressed) ---------

def lz4_decompress(src, expected_size):
    dst = bytearray()
    i = 0
    n = len(src)
    while i < n:
        token = src[i]
        i += 1

        literals = token >> 4
        if literals == 15:
            while True:
                b = src[i]
                i += 1
                literals += b
                if b != 255:
                    break
        dst += src[i:i + literals]
        i += literals

        if i >= n:
            break

        offset = src[i] | (src[i + 1] << 8)
        i += 2
        match = token & 0xF
        if match == 15:
            while True:
                b = src[i]
                i += 1
                match += b
                if b != 255:
                    break
        match += 4

        start = len(dst) - offset
        for j in range(match):
            dst.append(dst[start + j])

    if expected_size and len(dst) != expected_size:
        raise ValueError("LZ4 output %d bytes, expected %d" % (len(dst), expected_size))
    return bytes(dst)


# --- NSO ---------------------------------------------------------------------

def read_nso_symbols(path):
    """Returns (exported, imported) symbol name sets for an NSO file."""
    with open(path, "rb") as f:
        data = f.read()
    if data[:4] != b"NSO0":
        raise ValueError("%s is not an NSO" % path)

    flags = struct.unpack_from("<I", data, 0xC)[0]
    ro_file_off, ro_mem_off, ro_size = struct.unpack_from("<III", data, 0x20)
    ro_compressed_size = struct.unpack_from("<I", data, 0x64)[0]

    blob = data[ro_file_off:ro_file_off + ro_compressed_size]
    ro = lz4_decompress(blob, ro_size) if flags & 2 else blob

    # Offsets are relative to the start of .ro's memory mapping.
    dynstr_off, dynstr_size = struct.unpack_from("<II", data, 0x90)
    dynsym_off, dynsym_size = struct.unpack_from("<II", data, 0x98)

    dynstr = ro[dynstr_off:dynstr_off + dynstr_size]
    dynsym = ro[dynsym_off:dynsym_off + dynsym_size]

    exported, imported = set(), set()
    for off in range(0, len(dynsym) - 23, 24):
        name_off, info, _other, shndx, _value, _size = struct.unpack_from("<IBBHQQ", dynsym, off)
        if name_off == 0 or name_off >= len(dynstr):
            continue
        end = dynstr.find(b"\0", name_off)
        name = dynstr[name_off:end].decode("utf-8", "replace")
        if not name:
            continue
        weak = (info >> 4) == 2
        (imported if shndx == 0 else exported).add((name, weak))
    return exported, imported


# --- ELF ---------------------------------------------------------------------

def read_elf_symbols(path):
    """Returns (exported, imported) symbol sets for an ELF file."""
    with open(path, "rb") as f:
        data = f.read()
    if data[:4] != b"\x7fELF":
        raise ValueError("%s is not an ELF" % path)

    e_shoff, = struct.unpack_from("<Q", data, 0x28)
    e_shentsize, e_shnum, e_shstrndx = struct.unpack_from("<HHH", data, 0x3A)

    sections = []
    for i in range(e_shnum):
        off = e_shoff + i * e_shentsize
        name, sh_type, _flags, _addr, sh_off, sh_size, sh_link, _info, _align, sh_entsize = \
            struct.unpack_from("<IIQQQQIIQQ", data, off)
        sections.append((name, sh_type, sh_off, sh_size, sh_link, sh_entsize))

    shstr_off, shstr_size = sections[e_shstrndx][2], sections[e_shstrndx][3]
    shstr = data[shstr_off:shstr_off + shstr_size]

    def section_name(name_off):
        end = shstr.find(b"\0", name_off)
        return shstr[name_off:end].decode()

    exported, imported = set(), set()
    for name, _sh_type, sh_off, sh_size, sh_link, sh_entsize in sections:
        if section_name(name) != ".dynsym":
            continue
        str_off, str_size = sections[sh_link][2], sections[sh_link][3]
        dynstr = data[str_off:str_off + str_size]
        for off in range(sh_off, sh_off + sh_size, sh_entsize or 24):
            name_off, info, _other, shndx, _value, _size = struct.unpack_from("<IBBHQQ", data, off)
            if name_off == 0:
                continue
            end = dynstr.find(b"\0", name_off)
            sym = dynstr[name_off:end].decode("utf-8", "replace")
            if not sym:
                continue
            weak = (info >> 4) == 2
            (imported if shndx == 0 else exported).add((sym, weak))
    return exported, imported


def read_symbols(path):
    """Reads an NSO or an ELF, whichever it turns out to be."""
    with open(path, "rb") as f:
        magic = f.read(4)
    if magic == b"NSO0":
        return read_nso_symbols(path)
    if magic == b"\x7fELF":
        return read_elf_symbols(path)
    raise ValueError("%s is neither an NSO nor an ELF" % path)


def imports_with_relocations(path):
    """Undefined symbols that something actually references.

    Linkers leave a few undefined names behind with no relocation pointing at
    them (Skyline's `deadbeef` entrypoint marker, `__nro_header_start`); those
    are never looked up at runtime, so they are not failures.
    """
    with open(path, "rb") as f:
        data = f.read()
    if data[:4] != b"\x7fELF":
        return None  # only ELFs carry relocation sections we can read cheaply

    e_shoff, = struct.unpack_from("<Q", data, 0x28)
    e_shentsize, e_shnum, _ = struct.unpack_from("<HHH", data, 0x3A)

    sections = []
    for i in range(e_shnum):
        off = e_shoff + i * e_shentsize
        sections.append(struct.unpack_from("<IIQQQQIIQQ", data, off))

    referenced = set()
    for _name, sh_type, _flags, _addr, sh_off, sh_size, sh_link, _info, _align, sh_entsize in sections:
        if sh_type != 4:  # SHT_RELA
            continue
        dynsym = sections[sh_link]
        str_off, str_size = sections[dynsym[6]][4], sections[dynsym[6]][5]
        dynstr = data[str_off:str_off + str_size]
        for off in range(sh_off, sh_off + sh_size, sh_entsize or 24):
            _offset, info, _addend = struct.unpack_from("<QQq", data, off)
            sym_index = info >> 32
            if sym_index == 0:
                continue
            sym_off = dynsym[4] + sym_index * (dynsym[9] or 24)
            name_off, _info, _other, _shndx, _value, _size = struct.unpack_from("<IBBHQQ", data, sym_off)
            if name_off == 0:
                continue
            end = dynstr.find(b"\0", name_off)
            referenced.add(dynstr[name_off:end].decode("utf-8", "replace"))
    return referenced


def main(argv):
    args = argv[1:]
    if "--" in args:
        separator = args.index("--")
        providers, targets = args[:separator], args[separator + 1:]
    else:
        providers, targets = args[:-1], args[-1:]

    if not providers or not targets:
        raise SystemExit(__doc__)

    exports = set()
    for provider in providers:
        if not os.path.exists(provider):
            raise SystemExit("no such file or directory: %s" % provider)
        paths = []
        if os.path.isdir(provider):
            paths = [os.path.join(provider, name) for name in sorted(os.listdir(provider))]
        else:
            paths = [provider]
        for path in paths:
            if not os.path.isfile(path):
                continue
            try:
                exported, _ = read_symbols(path)
            except ValueError:
                continue  # main.npdm and friends
            exports |= {sym for sym, _weak in exported}
            print("%-24s %6d exported symbols" % (os.path.basename(path), len(exported)))

    if not exports:
        raise SystemExit("no NSO or ELF found in %s" % ", ".join(providers))

    status = 0
    for target in targets:
        _exported, imported = read_symbols(target)
        referenced = imports_with_relocations(target)

        missing = []
        for sym, weak in sorted(imported):
            if weak or sym in exports:
                continue
            if referenced is not None and sym not in referenced:
                continue  # nothing looks it up at runtime
            missing.append(sym)

        print()
        if missing:
            print("%s: %d unresolved import(s), it will not load:" % (os.path.basename(target), len(missing)))
            for sym in missing:
                print("  %s" % sym)
            status = 1
        else:
            print("%s: every referenced import resolves." % os.path.basename(target))

    return status


if __name__ == "__main__":
    sys.exit(main(sys.argv))
