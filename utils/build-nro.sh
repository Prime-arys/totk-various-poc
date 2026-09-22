#!/usr/bin/env bash
# Builds one Skyline plugin of the workspace as an NRO: the merger itself, or
# any plugin under plugins/.
#
#   utils/build-nro.sh <package> [--out <file.nro>] [--debug]
#
# `cargo skyline build` would normally do this, but cargo-skyline 3.5 ships a
# target spec written for a newer rustc than the skyline-v3 toolchain it
# installs. So we write a spec that follows the toolchain actually present,
# run the same cargo invocation ourselves, then convert the ELF with linkle
# (the same converter cargo-skyline uses internally).

set -e
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

PACKAGE=""
OUT=""
PROFILE="release"
PROFILE_FLAG="--release"
while [ $# -gt 0 ]; do
    case "$1" in
        --debug) PROFILE="debug"; PROFILE_FLAG="" ;;
        --out) shift; OUT="$1" ;;
        -*) echo "unknown argument: $1" >&2; exit 2 ;;
        *) PACKAGE="$1" ;;
    esac
    shift
done
if [ -z "$PACKAGE" ]; then
    echo "usage: utils/build-nro.sh <package> [--out <file.nro>] [--debug]" >&2
    exit 2
fi
LIBRARY="lib$(echo "$PACKAGE" | tr '-' '_')"
[ -n "$OUT" ] || OUT="$OUTPUT/nro/$PACKAGE.nro"
# cargo runs from the root below, so the destination has to be absolute by
# then (meson hands one relative to its build directory).
mkdir -p "$(dirname "$OUT")"
OUT="$(cd "$(dirname "$OUT")" && pwd)/$(basename "$OUT")"

SKYLINE_DIR="${CARGO_HOME:-$HOME/.cargo}/skyline"
LINKER_SCRIPT="$SKYLINE_DIR/link.T"
TARGET="$OUTPUT/aarch64-skyline-switch.json"

if [ ! -f "$LINKER_SCRIPT" ]; then
    echo "error: $LINKER_SCRIPT not found, so the Skyline toolchain is not in" >&2
    echo "       this image. Rebuild it: ./build.sh --rebuild-image" >&2
    exit 1
fi

# Two things the toolchain decides, and they change from one skyline-v3 to the
# next (cargo skyline builds it from whatever nightly is current when the image
# is made):
#
#   - the data layout rustc expects for aarch64, which follows its LLVM. Asking
#     it is the only way to be right: a spec that says anything else is refused.
#   - whether a `.json` target needs -Zjson-target-spec, which recent cargo
#     require and older ones do not know.
TOOLCHAIN_SPEC="$(rustup run skyline-v3 rustc --print target-spec-json -Z unstable-options \
    --target aarch64-unknown-none 2>/dev/null)"

# A field of that spec, exactly as this rustc writes it — quotes included. It
# spells them differently from one version to the next (a pointer width is a
# string for 1.83 and a number for 1.95), and refuses the other spelling.
raw_field() {
    printf '%s\n' "$TOOLCHAIN_SPEC" \
        | sed -n "s/^[[:space:]]*\"$1\":[[:space:]]*\(.*\)$/\1/p" \
        | head -1 | sed 's/,[[:space:]]*$//'
}

DATA_LAYOUT="$(raw_field data-layout)"
POINTER_WIDTH="$(raw_field target-pointer-width)"
[ -n "$DATA_LAYOUT" ] || DATA_LAYOUT='"e-m:e-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128-Fn32"'
[ -n "$POINTER_WIDTH" ] || POINTER_WIDTH='"64"'

JSON_SPEC_OPTION=()
if rustup run skyline-v3 cargo -Z json-target-spec locate-project >/dev/null 2>&1; then
    JSON_SPEC_OPTION=(-Z json-target-spec)
fi

mkdir -p "$(dirname "$TARGET")"
cat > "$TARGET" <<JSON
{
    "arch": "aarch64",
    "crt-static-default": false,
    "crt-static-respected": false,
    "data-layout": $DATA_LAYOUT,
    "disable-redzone": true,
    "dynamic-linking": true,
    "env": "",
    "executables": true,
    "features": "+v8a,+neon,+crypto,+crc",
    "has-rpath": false,
    "linker": "rust-lld",
    "linker-flavor": "ld.lld",
    "llvm-target": "aarch64-unknown-none",
    "max-atomic-width": 128,
    "os": "switch",
    "panic-strategy": "abort",
    "position-independent-executables": true,
    "post-link-args": {
        "ld.lld": [
            "--no-gc-sections",
            "--eh-frame-hdr"
        ]
    },
    "pre-link-args": {
        "ld.lld": [
            "-T$LINKER_SCRIPT",
            "-init=__custom_init",
            "-fini=__custom_fini",
            "--export-dynamic"
        ]
    },
    "relro-level": "off",
    "target-endian": "little",
    "target-family": null,
    "target-pointer-width": $POINTER_WIDTH,
    "vendor": "jam1garner"
}
JSON

export SKYLINE_ADD_NRO_HEADER=1
export RUSTFLAGS="--cfg skyline_std_v3"
export PATH="$SKYLINE_DIR/toolchain/skyline/bin:$PATH"

cd "$ROOT"
rustup run skyline-v3 cargo build $PROFILE_FLAG -p "$PACKAGE" \
    --target "$TARGET" \
    "${JSON_SPEC_OPTION[@]}" \
    -Z build-std=core,alloc,std,panic_abort

ELF="$CARGO_TARGET_DIR/aarch64-skyline-switch/$PROFILE/$LIBRARY.so"

# devkitPro's elf2nro writes a zero bss size here, which crashes the plugin,
# so it is not used as a fallback.
linkle nro "$ELF" "$OUT"

say "Built $OUT"
