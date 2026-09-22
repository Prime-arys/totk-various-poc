#!/usr/bin/env bash
# Builds totk-mod-manager.nro.
#
#   1. the Rust core (core/) as a static library for aarch64-unknown-none,
#      with the merger's crates built without std;
#   2. the borealis application for the Switch (deko3d), linked against it.
#
#   ./build.sh [--ryujinx] [--build-dir <dir>] [--out <dir>]
#
#   --ryujinx     also writes totk-mod-manager-ryujinx.nro, patched for Ryujinx
#                 (its JIT refuses an ARMv9 register read in libgcc's unwinder
#                 that real Switch hardware never reaches)
#   --build-dir   where cmake and cargo work (default: build/ next to this
#                 script; the repository points it at output/manager)
#   --out         where the .nro files are written (default: the build dir)
#
# It runs inside the repository's container, which has devkitPro (switch-dev,
# switch-glm, switch-curl, switch-libarchive), cmake and the nightly below.
# See docs/build.md.

set -e

RUST_TOOLCHAIN="${RUST_TOOLCHAIN:-nightly-2024-10-09}"

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$DIR"

RYUJINX=0
BUILD_ROOT="$DIR/build"
OUT=""
while [ $# -gt 0 ]; do
    case "$1" in
        --ryujinx) RYUJINX=1 ;;
        --build-dir) shift; BUILD_ROOT="$1" ;;
        --out) shift; OUT="$1" ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
    shift
done
mkdir -p "$BUILD_ROOT"
BUILD_ROOT="$(cd "$BUILD_ROOT" && pwd)"
[ -n "$OUT" ] || OUT="$BUILD_ROOT"
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"

export DEVKITPRO="${DEVKITPRO:-/opt/devkitpro}"
export PATH="$DEVKITPRO/tools/bin:$DEVKITPRO/devkitA64/bin:$PATH"

echo "== Rust core"
(
    cd core
    RUSTFLAGS="-C relocation-model=pic" cargo "+$RUST_TOOLCHAIN" build --release \
        --target aarch64-unknown-none -Zbuild-std=core,alloc \
        --target-dir "$BUILD_ROOT/rust"
)

# How many compilers at once. Not one per core: borealis and yoga are heavy
# C++, each job can take the better part of a gigabyte, and a container with
# many cores but ordinary memory dies with "Cannot allocate memory" in the
# middle of a header. One job per 1.5 GB, never more than the cores.
job_count() {
    local cores memory_kb by_memory
    cores="$(nproc)"
    memory_kb="$(awk '/^MemTotal:/ {print $2}' /proc/meminfo 2>/dev/null)"
    if [ -n "$memory_kb" ]; then
        by_memory=$((memory_kb / 1500000))
        [ "$by_memory" -lt 1 ] && by_memory=1
        [ "$by_memory" -lt "$cores" ] && cores="$by_memory"
    fi
    printf '%s' "$cores"
}
JOBS="${JOBS:-$(job_count)}"

echo "== Switch application ($JOBS jobs)"
cmake -B "$BUILD_ROOT/switch" -G "Unix Makefiles" \
    -DPLATFORM_SWITCH=ON -DUSE_DEKO3D=ON -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_DEPENDS_USE_COMPILER=FALSE \
    -DCORE_LIBRARY="$BUILD_ROOT/rust/aarch64-unknown-none/release/libtotk_manager_core.a"
make -C "$BUILD_ROOT/switch" -j"$JOBS" totk-mod-manager.nro

cp "$BUILD_ROOT/switch/totk-mod-manager.nro" "$OUT/totk-mod-manager.nro"
echo "Built $OUT/totk-mod-manager.nro"

if [ "$RYUJINX" = 1 ]; then
    # The same program with two instructions replaced; see the comment above.
    python3 - "$OUT/totk-mod-manager.nro" "$OUT/totk-mod-manager-ryujinx.nro" <<'PYTHON'
import sys
data = bytearray(open(sys.argv[1], "rb").read())
mrs_gcspr = bytes.fromhex("21253bd5")   # mrs x1, gcspr_el0
mov_zero = bytes.fromhex("010080d2")    # mov x1, #0
count, start = 0, 0
while (index := data.find(mrs_gcspr, start)) != -1:
    if index % 4 == 0:
        data[index:index + 4] = mov_zero
        count += 1
    start = index + 1
open(sys.argv[2], "wb").write(data)
print(f"Ryujinx build: {count} instruction(s) patched")
PYTHON
fi
