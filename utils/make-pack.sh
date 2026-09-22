#!/usr/bin/env bash
# Lays out a ready-to-copy pack from the files meson just built. Two kinds:
#
#   switch     what goes on a console's SD card
#   emulator   what goes into a Ryujinx data folder
#
#   utils/make-pack.sh --kind <switch|emulator> --out <dir> [--title-id <id>] \
#       --subsdk9 <file> --npdm <file> --merger <nro> --manager <nro> \
#       --skyline-config <ini> --merger-config <ini> \
#       --readme <md> --readme-fr <md> [--zip]
#
# Mods are not part of a pack: they are built apart (docs/mods.md) and dropped
# into totk/mods/ afterwards.

set -e
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

KIND=""
OUT=""
SUBSDK9=""
NPDM=""
MERGER=""
MANAGER=""
SKYLINE_CONFIG=""
MERGER_CONFIG=""
README=""
README_FR=""
ZIP=0
while [ $# -gt 0 ]; do
    case "$1" in
        --kind) shift; KIND="$1" ;;
        --title-id) shift; TITLE_ID="$1" ;;
        --out) shift; OUT="$1" ;;
        --subsdk9) shift; SUBSDK9="$1" ;;
        --npdm) shift; NPDM="$1" ;;
        --merger) shift; MERGER="$1" ;;
        --manager) shift; MANAGER="$1" ;;
        --skyline-config) shift; SKYLINE_CONFIG="$1" ;;
        --merger-config) shift; MERGER_CONFIG="$1" ;;
        --readme) shift; README="$1" ;;
        --readme-fr) shift; README_FR="$1" ;;
        --zip) ZIP=1 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
    shift
done
for name in KIND OUT SUBSDK9 NPDM MERGER MANAGER SKYLINE_CONFIG MERGER_CONFIG README README_FR; do
    if [ -z "${!name}" ]; then
        echo "error: --$(echo "$name" | tr 'A-Z_' 'a-z-') is missing" >&2
        exit 2
    fi
done

rm -rf "$OUT"
mkdir -p "$OUT"

# Where the two kinds put the same four things.
case "$KIND" in
    switch)
        SD="$OUT/SD"
        EXEFS="$SD/atmosphere/contents/$TITLE_ID/exefs"
        ;;
    emulator)
        SD="$OUT/Ryujinx/sdcard"
        EXEFS="$OUT/Ryujinx/mods/contents/$(echo "$TITLE_ID" | tr 'A-Z' 'a-z')/skyline-totk/exefs"
        ;;
    *)
        echo "error: --kind must be 'switch' or 'emulator'" >&2
        exit 2
        ;;
esac
PLUGINS="$SD/atmosphere/contents/$TITLE_ID/skyline/plugins"

mkdir -p "$EXEFS" "$PLUGINS" "$SD/skyline/totk" "$SD/switch" "$SD/totk/mods"
cp "$SUBSDK9" "$EXEFS/subsdk9"
cp "$NPDM" "$EXEFS/main.npdm"
cp "$MERGER" "$PLUGINS/totk-mod-merger-plugin.nro"
cp "$MANAGER" "$SD/switch/$(basename "$MANAGER")"
cp "$SKYLINE_CONFIG" "$SD/skyline/totk/config.ini"
cp "$MERGER_CONFIG" "$SD/totk/config.ini"
# The pack carries both notices; English is the one named README.
cp "$README" "$OUT/README.md"
cp "$README_FR" "$OUT/LISEZMOI.md"

cat > "$SD/totk/mods/README.txt" <<'TEXT'
One folder per mod, right here:

  totk/mods/My Mod/romfs/...         a mod as a folder
  totk/mods/My Mod/My Mod.tkcl       a TKMM package
  totk/mods/My Mod/mod.ini           optional: name, version, priority
  totk/mods/My Mod/plugin.nro        optional: the code the mod brings

Mods are not part of the pack: see docs/mods.md in the repository to build one
(the EnemyHp mod, for instance).
TEXT
cat > "$SD/totk/mods/LISEZMOI.txt" <<'TEXT'
Un dossier par mod, ici meme :

  totk/mods/Mon Mod/romfs/...        un mod en dossier
  totk/mods/Mon Mod/Mon Mod.tkcl     un paquet TKMM
  totk/mods/Mon Mod/mod.ini          facultatif : nom, version, priorite
  totk/mods/Mon Mod/plugin.nro       facultatif : le code que le mod fournit

Les mods ne font pas partie du pack : voir docs/mods.fr.md du depot pour en
construire (par exemple le mod EnemyHp).
TEXT

# What is in the pack, for a bug report: sizes and fingerprints.
{
    echo "# $KIND pack, $(date '+%Y-%m-%d %H:%M')"
    echo
    (cd "$OUT" && find . -type f ! -name contenu.txt | sort | while read -r file; do
        printf '%10d  %s  %s\n' "$(stat -c %s "$file")" "$(md5sum "$file" | cut -c1-32)" "${file#./}"
    done)
} > "$OUT/contenu.txt"

say "Pack '$KIND' written to $OUT"
sed -n '3,$p' "$OUT/contenu.txt"

if [ "$ZIP" = 1 ]; then
    ARCHIVE="$OUT.zip"
    rm -f "$ARCHIVE"
    python3 - "$OUT" "$ARCHIVE" <<'PYTHON'
import os, sys, zipfile

source, archive = sys.argv[1], sys.argv[2]
with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as zip_file:
    for directory, _, names in os.walk(source):
        for name in names:
            path = os.path.join(directory, name)
            zip_file.write(path, os.path.relpath(path, source))
print("Zipped into", archive)
PYTHON
fi
