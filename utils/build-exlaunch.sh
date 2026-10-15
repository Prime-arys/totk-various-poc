#!/usr/bin/env bash
# Builds an exefs module with exlaunch (utils/exlaunch, a submodule): the kind
# of code a TKMM package ships as exefs/subsdk9, loaded by the console itself
# when the game starts.
#
#   utils/build-exlaunch.sh <name> --source <dir> --out <dir>
#
#   <name>          what the module is called in output/ and in the logs
#   --source <dir>  the program: its .cpp/.hpp/.s files, among them main.cpp
#                   (exl_main) and setting.hpp, which replace exlaunch's own
#   --out <dir>     where subsdk9 and main.npdm are left
#
# exlaunch builds where it is: its Makefile compiles every source under its
# own source/ folder and writes build/ and deploy/ next to it. So a copy of it
# is laid out in output/exlaunch/<name>, the program is dropped into its
# source/program, and make runs there: the submodule is never written to, and
# everything transient stays in output/.

set -e
set -o pipefail
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

NAME="${1:?usage: build-exlaunch.sh <name> --source <dir> --out <dir>}"
shift
SOURCE=""
OUT=""
while [ $# -gt 0 ]; do
    case "$1" in
        --source) shift; SOURCE="$1" ;;
        --out) shift; OUT="$1" ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
    shift
done
if [ -z "$SOURCE" ] || [ -z "$OUT" ] || [ ! -f "$SOURCE/main.cpp" ]; then
    echo "usage: utils/build-exlaunch.sh <name> --source <dir with main.cpp> --out <dir>" >&2
    exit 2
fi

EXLAUNCH="$ROOT/utils/exlaunch"
if [ ! -f "$EXLAUNCH/misc/mk/common.mk" ]; then
    echo "error: utils/exlaunch is empty: git submodule update --init" >&2
    exit 1
fi
SOURCE="$(cd "$SOURCE" && pwd)"
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"

say "== $NAME (exlaunch)"
STAGE="$OUTPUT/exlaunch/$NAME"
# Laid out afresh each time: the whole thing compiles in ten seconds, which
# is not worth a file left behind by a previous build being compiled again.
rm -rf "$STAGE"
mkdir -p "$STAGE"
cp -r "$EXLAUNCH/Makefile" "$EXLAUNCH/misc" "$EXLAUNCH/source" "$STAGE/"
cp -r "$SOURCE/." "$STAGE/source/program/"

# A submodule checked out on Windows comes with CRLF line endings (this
# repository's .gitattributes stops at its own files), and exlaunch runs
# shell scripts as part of its build: "set -e\r" is an error. The copy is
# what gets fixed.
{ grep -rlI $'\r' "$STAGE" || true; } | while IFS= read -r file; do
    sed -i 's/\r$//' "$file"
done

# The permissions of the title, which the module is loaded under. The game's
# own, opened up the way skyline-totk needs them: a mod has to be able to map
# and patch the game's code (see skyline-totk/npdm/totk.json).
cp "$ROOT/skyline-totk/npdm/totk.json" "$STAGE/config.json"

# exlaunch's settings (its config.mk, which the copy does not take).
#
# -Wno-error: exlaunch compiles itself with -Werror, and each new GCC finds
# something new to warn about in it (devkitA64's current one stops on a
# typedef in its hooking code). A warning in someone else's framework is no
# reason for a mod not to build; the ones in the program are shown below.
cat > "$STAGE/config.mk" <<MK
# Written by utils/build-exlaunch.sh.
LOAD_KIND := Module
PROGRAM_ID := $(printf '%s' "$TITLE_ID" | tr 'A-Z' 'a-z')
ELF_EXTRACT :=
PYTHON := python3
NPDM_JSON := application.json
C_FLAGS := -Wno-error
CXX_FLAGS :=
MK

export DEVKITPRO=/opt/devkitpro
export DEVKITA64=$DEVKITPRO/devkitA64
export PATH="$DEVKITA64/bin:$DEVKITPRO/tools/bin:$PATH"

# One compiler per 1.5 GB of memory: exlaunch's headers are heavy on
# templates, and a starved compiler fails with errors that say nothing of it.
jobs="${JOBS:-$(awk '/MemAvailable/ { n = int($2 / 1572864); print (n < 1 ? 1 : n) }' /proc/meminfo)}"
cores="$(nproc)"
[ "$jobs" -gt "$cores" ] && jobs="$cores"

build_log="$OUTPUT/tmp/exlaunch-$NAME.log"
if ! make -C "$STAGE" -j"$jobs" > "$build_log" 2>&1; then
    cat "$build_log" >&2
    echo "error: $NAME did not build (log above)." >&2
    exit 1
fi
# The program's own warnings; exlaunch's are its business.
grep -E '/source/program/.*(warning|note):' "$build_log" || true

cp "$STAGE/deploy/subsdk9" "$STAGE/deploy/main.npdm" "$OUT/"
say "Built $OUT/subsdk9 ($(stat -c %s "$OUT/subsdk9") bytes) and $OUT/main.npdm"
