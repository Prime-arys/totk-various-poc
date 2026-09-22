#!/usr/bin/env bash
# What the build needs, and what this container has. Nothing is built and
# nothing is changed; the exit code is 1 when something a pack needs is
# missing — which, in the image, means the image is broken or out of date:
#
#   ./build.sh doctor          (or, inside: utils/doctor.sh)
#   ./build.sh --rebuild-image

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

missing=0
missing_names=""

report() {
    local name="$1" found="$2" advice="$3"
    if [ -n "$found" ]; then
        printf '  \033[32mok\033[0m    %-24s %s\n' "$name" "$found"
    else
        printf '  \033[31mmiss\033[0m  %-24s %s\n' "$name" "$advice"
        missing=$((missing + 1))
        missing_names="${missing_names:+$missing_names, }$name"
    fi
}

where() {
    command -v "$1" 2>/dev/null
}

say "== where things go"
printf '  repository   %s\n' "$ROOT"
printf '  output       %s\n' "$OUTPUT"
printf '  release      %s\n' "$RELEASE"
if [ ! -f /.dockerenv ]; then
    echo "  note: this is not the container. The build is only supported inside"
    echo "        it (docs/build.md); what follows is about this machine."
fi

say "== skyline-totk and totk-mod-manager (devkitPro)"
DEVKITPRO="${DEVKITPRO:-/opt/devkitpro}"
report "DEVKITPRO" "$([ -d "$DEVKITPRO" ] && echo "$DEVKITPRO")" "the image has no devkitPro"
for tool in aarch64-none-elf-gcc npdmtool elf2nso uam make cmake python3; do
    path="$(where "$tool")"
    for candidate in "$DEVKITPRO/devkitA64/bin/$tool" "$DEVKITPRO/tools/bin/$tool"; do
        [ -n "$path" ] && break
        [ -x "$candidate" ] && path="$candidate"
    done
    report "$tool" "$path" "missing from the image"
done
pacman_program=pacman
command -v dkp-pacman >/dev/null 2>&1 && pacman_program=dkp-pacman
for package in switch-glm switch-curl switch-libarchive; do
    report "$package" "$($pacman_program -Q "$package" 2>/dev/null | head -1)" \
        "missing from the image"
done

say "== the plugins (Rust for Skyline)"
report "cargo" "$(where cargo)" "missing from the image"
report "linkle" "$(where linkle)" "missing from the image"
linker_script="${CARGO_HOME:-$HOME/.cargo}/skyline/link.T"
report "skyline linker script" "$([ -f "$linker_script" ] && echo "$linker_script")" \
    "cargo skyline update-std did not finish when the image was built"
if command -v rustup >/dev/null 2>&1; then
    toolchains="$(rustup toolchain list 2>/dev/null)"
    report "skyline-v3 toolchain" "$(printf '%s\n' "$toolchains" | grep -m1 skyline-v3)" \
        "missing from the image"
    report "nightly for the core" \
        "$(printf '%s\n' "$toolchains" | grep -m1 "${RUST_TOOLCHAIN:-nightly-2024-10-09}")" \
        "missing from the image"
fi

say "== the submodules (these live in the repository, not in the image)"
report "borealis" \
    "$([ -f "$ROOT/totk-mod-manager/library/borealis/library/CMakeLists.txt" ] && echo "totk-mod-manager/library/borealis")" \
    "git submodule update --init"
report "TkSharp" \
    "$([ -f "$ROOT/utils/TkSharp/TkSharp/TkSharp.csproj" ] && echo "utils/TkSharp")" \
    "git submodule update --init"

say "== building mods"
report "dotnet" "$(where dotnet)" "image built with --no-dotnet: ./build.sh --rebuild-image"

echo
if [ "$missing" = 0 ]; then
    say "Everything is here."
else
    say "Missing: $missing_names."
    echo "  An image that is out of date explains most of these:"
    echo "  ./build.sh --rebuild-image"
    exit 1
fi
