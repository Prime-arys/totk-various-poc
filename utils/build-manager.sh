#!/usr/bin/env bash
# Builds totk-mod-manager: its Rust core, then the borealis homebrew, in both
# the console and the emulator flavour.
#
#   utils/build-manager.sh [--out <dir>]
#
# cmake, borealis and the core compile into output/manager. The first build
# takes a while (borealis); later ones are incremental as long as that folder
# is kept.

set -e
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

OUT="$OUTPUT/manager/nro"
while [ $# -gt 0 ]; do
    case "$1" in
        --out) shift; OUT="$1" ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
    shift
done

# meson hands over a path relative to its build directory, and the script
# below works from its own: make it absolute before passing it on.
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"

say "== totk-mod-manager (console and emulator)"
bash "$ROOT/totk-mod-manager/build.sh" \
    --ryujinx --build-dir "$OUTPUT/manager" --out "$OUT"
say "Built $OUT/totk-mod-manager.nro and $OUT/totk-mod-manager-ryujinx.nro"
