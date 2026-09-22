#!/usr/bin/env bash
# The one thing to run. Everything is built inside a container, so the machine
# needs nothing but Docker — and the result is the same on Windows, Linux and
# macOS. From PowerShell, build.ps1 does exactly the same.
#
#   ./build.sh                          the two packs, in release/
#   ./build.sh pack-switch              one of them (or pack-emulator)
#   ./build.sh plugins                  the example plugins, in release/plugins/
#   ./build.sh --romfs <dossier> mod    the EnemyHp mod, in release/mods/
#   ./build.sh test                     the tests of the crates that run on a PC
#   ./build.sh doctor                   what the image has
#   ./build.sh shell                    a shell inside the container
#   ./build.sh clean [--all]            empties output/ (--all: release/ too)
#
#   --romfs <dossier>   an extracted TotK romfs, mounted read-only (mods)
#   --zip               a .zip next to each pack as well
#   --title-id <id>     another title id than TotK's
#   --rebuild-image     build the image again (after changing the Dockerfile)
#   --no-dotnet         with --rebuild-image: no .NET SDK, so no .tkcl mods
#   -- <commande...>    run that in the container instead
#
# What is built goes to output/, what is finished goes to release/. Nothing
# else in the repository is written to.

set -e
set -o pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
IMAGE="${TOTK_IMAGE:-totk-various-poc:latest}"
BUILD_DIR="output/build"

ROMFS=""
ZIP=0
TITLE_ID=""
REBUILD=0
DOTNET=1
VERBATIM=0
CLEAN_ALL=0
TARGETS=()
while [ $# -gt 0 ]; do
    case "$1" in
        --romfs) shift; ROMFS="$1" ;;
        --zip) ZIP=1 ;;
        --title-id) shift; TITLE_ID="$1" ;;
        --rebuild-image) REBUILD=1 ;;
        --no-dotnet) DOTNET=0 ;;
        --all) CLEAN_ALL=1 ;;
        --) shift; VERBATIM=1; break ;;
        -h | --help) sed -n '2,23p' "${BASH_SOURCE[0]}" | cut -c2-; exit 0 ;;
        -*) echo "unknown option: $1 (--help)" >&2; exit 2 ;;
        *) TARGETS+=("$1") ;;
    esac
    shift
done

# Docker is a Windows program under Git Bash: it wants C:/... for volumes, and
# must be told not to rewrite the paths meant for the container.
mount_path() {
    if command -v cygpath >/dev/null 2>&1; then cygpath -m "$1"; else printf '%s' "$1"; fi
}
export MSYS_NO_PATHCONV=1
export MSYS2_ARG_CONV_EXCL='*'

# Emptying a directory needs no container.
if [ "${TARGETS[0]:-}" = clean ]; then
    rm -rf "$ROOT/output"
    echo "output/ emptied"
    if [ "$CLEAN_ALL" = 1 ]; then
        rm -rf "$ROOT/release"
        echo "release/ emptied"
    fi
    exit 0
fi

if ! command -v docker >/dev/null 2>&1; then
    echo "error: docker not found. Install Docker Desktop (Windows, macOS) or" >&2
    echo "       the docker engine (Linux), then run this again." >&2
    exit 1
fi

# Forgetting `git submodule update --init` fails much further on, inside CMake,
# with a message about borealis that says nothing about submodules.
if [ ! -f "$ROOT/totk-mod-manager/library/borealis/library/CMakeLists.txt" ] \
   && [ -d "$ROOT/.git" ]; then
    echo "== the submodules are empty: git submodule update --init"
    git -C "$ROOT" submodule update --init
fi

if [ "$REBUILD" = 1 ] || ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
    echo "== building the image $IMAGE (once; a few minutes)"
    docker build --build-arg "WITH_DOTNET=$DOTNET" -t "$IMAGE" \
        -f "$(mount_path "$ROOT/Dockerfile")" "$(mount_path "$ROOT")"
fi

# On Linux and macOS the files the build writes belong to whoever ran this,
# not to root. Docker Desktop on Windows handles ownership itself.
user_arguments=()
case "$(uname -s)" in
    Linux | Darwin) user_arguments=(--user "$(id -u):$(id -g)") ;;
esac

# The romfs stays where it is and is mounted read-only: it is tens of
# gigabytes of game files, and the build only reads a handful of them.
mounts=(-v "$(mount_path "$ROOT")":/work)
setup_options=()
if [ -n "$ROMFS" ]; then
    if [ ! -d "$ROMFS" ]; then
        echo "error: --romfs $ROMFS is not a directory." >&2
        exit 1
    fi
    mounts+=(-v "$(mount_path "$(cd "$ROMFS" && pwd)")":/romfs:ro)
    setup_options+=(-Dtotk_romfs=/romfs)
fi
[ "$ZIP" = 1 ] && setup_options+=(-Dpack_zip=true)
[ -n "$TITLE_ID" ] && setup_options+=("-Dtitle_id=$TITLE_ID")

# `meson setup` on a directory that is already configured exits 0 and ignores
# the options it was given, so past the first run the options go to
# `meson configure` instead.
configure="if [ -e $BUILD_DIR/meson-info/meson-info.json ]; \
    then meson configure ${setup_options[*]} $BUILD_DIR >/dev/null; \
    else meson setup ${setup_options[*]} $BUILD_DIR >/dev/null; fi"

if [ "$VERBATIM" = 1 ]; then
    command=("$@")
    [ $# -gt 0 ] || command=(bash -l)
elif [ "${TARGETS[0]:-}" = shell ]; then
    command=(bash -l)
elif [ "${TARGETS[0]:-}" = doctor ]; then
    # Straight to the script: it is about the image, and it has to work even
    # when meson does not.
    command=(bash -lc 'bash utils/doctor.sh')
elif [ "${TARGETS[0]:-}" = test ]; then
    command=(bash -lc "$configure; meson test -C $BUILD_DIR --print-errorlogs")
else
    [ ${#TARGETS[@]} -gt 0 ] || TARGETS=(packs)
    command=(bash -lc "$configure; meson compile -C $BUILD_DIR ${TARGETS[*]}")
fi

# A terminal when there is one (a shell needs it), plain output otherwise, so
# this works the same from a script or a CI job.
terminal_arguments=(-i)
if [ -t 0 ] && [ -t 1 ]; then
    terminal_arguments=(-it)
fi

exec docker run --rm "${terminal_arguments[@]}" "${user_arguments[@]}" \
    "${mounts[@]}" -w /work "$IMAGE" "${command[@]}"
