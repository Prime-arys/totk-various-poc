#!/usr/bin/env bash
# Builds skyline-totk (the Skyline fork) and copies what a pack needs out of
# it: the exefs pair subsdk9 + main.npdm.
#
#   utils/build-skyline.sh [--out <dir>] [--title-id <id>]
#
# Its Makefile compiles into output/skyline: objects, the .nso, and the
# packaged exefs, none of it inside the source folder.

set -e
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

OUT="$OUTPUT/skyline/exefs"
while [ $# -gt 0 ]; do
    case "$1" in
        --out) shift; OUT="$1" ;;
        --title-id) shift; TITLE_ID="$1" ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
    shift
done

# meson hands over a path relative to its build directory; make is run from
# elsewhere, so make it absolute first.
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"

say "== skyline-totk"
mkdir -p "$OUTPUT/skyline"
bash "$ROOT/skyline-totk/build.sh" package \
    "OUT_ROOT=$OUTPUT/skyline" "TITLE_ID=$TITLE_ID"

EXEFS="$OUTPUT/skyline/out/atmosphere/contents/$TITLE_ID/exefs"
mkdir -p "$OUT"
cp "$EXEFS/subsdk9" "$EXEFS/main.npdm" "$OUT/"
say "Built $OUT/subsdk9 and $OUT/main.npdm"
