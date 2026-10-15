#!/usr/bin/env bash
# Builds one of the repository's mods: its data, read out of the game's own
# files and packaged as a .tkcl, plus the code that goes with it.
#
#   utils/build-mod.sh <name> --script <its build-mod.sh> --romfs <extracted romfs> [--out <dir>]
#
#   EnemyHp     plugins/enemy-hp/build-mod.sh    a .tkcl and a Skyline plugin
#   EnemyHpSe   mods/enemy-hp-se/build-mod.sh    a .tkcl holding an exefs module
#
# What every mod has in common is done here: the .tkcl is packaged by TKMM's
# own code (utils/tkmm-oracle, built against the utils/TkSharp submodule),
# which has to be built first. Mods are not part of a pack: drop the result in
# sd:/totk/mods/<name>/.

set -e
set -o pipefail
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

usage="usage: utils/build-mod.sh <name> --script <build-mod.sh> --romfs <romfs> [--out <dir>]"
NAME="${1:?$usage}"
shift
SCRIPT=""
ROMFS=""
OUT=""
while [ $# -gt 0 ]; do
    case "$1" in
        --script) shift; SCRIPT="$1" ;;
        --romfs) shift; ROMFS="$1" ;;
        --out) shift; OUT="$1" ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
    shift
done
[ -n "$OUT" ] || OUT="$RELEASE/mods/$NAME"
if [ ! -f "$SCRIPT" ]; then
    echo "$usage" >&2
    exit 2
fi
if [ -z "$ROMFS" ] || [ ! -d "$ROMFS" ]; then
    echo "error: --romfs must point at an extracted TotK romfs." >&2
    echo "       ./build.sh --romfs /path/to/romfs mod" >&2
    exit 2
fi

ORACLE_DIR="$ROOT/utils/tkmm-oracle"
ARTIFACTS="$OUTPUT/dotnet"
if [ ! -f "$ROOT/utils/TkSharp/TkSharp/TkSharp.csproj" ]; then
    echo "error: utils/TkSharp is empty: git submodule update --init" >&2
    exit 1
fi

# .NET writes bin/ and obj/ next to each project it touches — the oracle and
# the five TkSharp projects it references. ArtifactsPath moves all of them
# into output/dotnet, one folder per project, which is the only reason it is
# used here rather than BaseOutputPath (that one would pile every project's
# obj/ into the same directory). Where exactly it lands is a .NET detail, so
# the .dll is looked up rather than spelled out.
# Returning nothing is an answer, not a failure: find exits 1 on a directory
# that is not there yet, and `set -e` would kill the script without a word.
find_oracle() {
    [ -d "$ARTIFACTS" ] || return 0
    find "$ARTIFACTS" -name tkmm-oracle.dll 2>/dev/null | head -1
}

# What the packager was built from: every file of the TkSharp submodule and
# of the tool, by name, size and date. Kept next to the build, so that
# updating TkSharp builds the packager again instead of packaging mods with
# the previous TKMM. The files are looked at rather than the submodule's
# commit: that needs neither git in the image nor git to trust a repository
# mounted from elsewhere, and it sees a change that is not committed.
oracle_stamp() {
    (
        cd "$ROOT/utils"
        find TkSharp tkmm-oracle \( -name bin -o -name obj -o -name .git \) -prune \
            -o -type f -printf '%p %s %T@\n' | sort
    ) | sha1sum | cut -d' ' -f1
}
STAMP_FILE="$ARTIFACTS/tkmm-oracle.stamp"
STAMP="$(oracle_stamp)"

ORACLE="$(find_oracle)"
if [ -z "$ORACLE" ] || [ "$(cat "$STAMP_FILE" 2>/dev/null || true)" != "$STAMP" ]; then
    say "== tkmm-oracle (TKMM's own packager)"
    # Everything here goes through the environment rather than -p:Name=value:
    # the .NET 10 CLI hands that form to MSBuild with its prefix stripped, and
    # MSBuild reads the value as a second project ("Only one project can be
    # specified"). MSBuild takes properties from the environment just as well,
    # and they reach the referenced TkSharp projects the same way.
    #
    #   ArtifactsPath            keeps bin/ and obj/ out of the source folders
    #                            (one subfolder per project, under output/)
    #   MSBUILDDISABLENODEREUSE  MSBuild would otherwise leave four worker
    #   UseSharedCompilation     nodes and a Roslyn compiler server running
    #                            after the build. They inherit the pipe ninja
    #                            reads this script through, so ninja would wait
    #                            for an end-of-file that never comes: the build
    #                            finishes, prints everything, and `build.sh
    #                            mod` hangs until those daemons time out.
    #
    # The output goes to a file rather than through a pipe: should any tool
    # leave a process behind again, it holds that file, never the build's pipe.
    # It also means a failure shows the whole log instead of its last lines.
    build_log="$OUTPUT/tmp/tkmm-oracle-build.log"
    if ! ArtifactsPath="$ARTIFACTS" \
         MSBUILDDISABLENODEREUSE=1 \
         UseSharedCompilation=false \
            dotnet build "$ORACLE_DIR/TkmmOracle.csproj" -c Release \
            > "$build_log" 2>&1; then
        cat "$build_log" >&2
        echo "error: the packager did not build (log above)." >&2
        exit 1
    fi
    tail -3 "$build_log"
    ORACLE="$(find_oracle)"
    if [ -z "$ORACLE" ]; then
        echo "error: the build left no tkmm-oracle.dll under $ARTIFACTS" >&2
        exit 1
    fi
    printf '%s\n' "$STAMP" > "$STAMP_FILE"
fi

mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"

say "== $NAME"
export TKMM_ORACLE="$ORACLE"
bash "$SCRIPT" "$ROMFS" "$OUT"
say "Copy $OUT to sd:/totk/mods/$NAME/"
