#!/usr/bin/env bash
# Builds the Enemy HP mod, ready to copy to sd:/totk/mods/EnemyHp/:
#
#   EnemyHp.tkcl   the data, as a TKMM package with two option groups:
#                  - Taille des chiffres: Petite, Moyenne, Grande (default),
#                    Très grande (enemy gauge numbers, and the boss gauge's
#                    pane)
#                  - Armures: Nouvelle tunique de Prodige (default), or every
#                    armour
#   plugin.nro     regeneration of the enemies around the player (bosses
#                  included), the numbers under the boss gauge, and the
#                  enemy-hp.txt report
#   enemy-hp.ini   the plugin's settings
#   mod.ini
#
# Usage: ./build-mod.sh <extracted romfs> <out folder>
#
# Needs the tkmm-oracle tool built to package the .tkcl, and the Skyline
# toolchain for the plugin. `./build.sh --romfs <romfs> mod` does the whole
# thing, tool included, inside the container.

set -e
set -o pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$DIR/../.." && pwd)"
ROMFS="${1:?usage: build-mod.sh <romfs> <out folder>}"
OUT="${2:?usage: build-mod.sh <romfs> <out folder>}"
# Where the packager is: utils/build-enemy-hp-mod.sh builds it and says so
# through TKMM_ORACLE. Alone, look for it under output/dotnet.
ORACLE="${TKMM_ORACLE:-$(find "$ROOT/output/dotnet" -name tkmm-oracle.dll 2>/dev/null | head -1 || true)}"
if [ -z "$ORACLE" ]; then
    echo "error: tkmm-oracle.dll not found; build it with utils/build-enemy-hp-mod.sh" >&2
    exit 1
fi

# Somewhere to assemble the mod before packaging it. TMPDIR points inside
# output/ when the build calls this script.
PROJECT="$(mktemp -d)"
trap 'rm -rf "$PROJECT"' EXIT
cd "$ROOT"

# The numbers: text panes in the enemy and boss gauge layouts (one option per
# size) and the messages the enemy ones are filled from.
cargo run --release -q -p totk-merge --example make_enemy_life_ui_mod -- "$ROMFS" "$PROJECT"

# What turns them on: the game's VisualizeLife armour effect, on the
# Champion's Tunic only (as in Breath of the Wild) or on every armour.
GROUP="$PROJECT/options/2 Armures"
cargo run --release -q -p totk-merge --example make_visualize_life_mod -- \
    "$ROMFS" "$GROUP/1 Tunique" tunic
cargo run --release -q -p totk-merge --example make_visualize_life_mod -- \
    "$ROMFS" "$GROUP/2 Toutes"
rm -f "$GROUP/1 Tunique/mod.ini" "$GROUP/2 Toutes/mod.ini"
cat > "$GROUP/info.json" <<'JSON'
{
  "Name": "Armures",
  "Description": "Quelle armure affiche les PV des ennemis quand on la porte.",
  "Type": 3,
  "Priority": 1
}
JSON
cat > "$GROUP/1 Tunique/info.json" <<'JSON'
{
  "Name": "Nouvelle tunique de Prodige",
  "Description": "Comme dans Breath of the Wild : seulement en la portant (torse, tous ses niveaux).",
  "Priority": 0,
  "IsDefaultSelected": true
}
JSON
cat > "$GROUP/2 Toutes/info.json" <<'JSON'
{
  "Name": "Toutes les armures",
  "Description": "Les PV s'affichent quelle que soit la tenue.",
  "Priority": 0,
  "IsDefaultSelected": false
}
JSON

mkdir -p "$OUT"
dotnet "$ORACLE" package "$ROMFS" "$PROJECT" "$OUT/EnemyHp.tkcl" | tail -1

# The plugin: regeneration, the boss numbers, and the report.
bash utils/build-nro.sh enemy-hp --out "$OUT/plugin.nro" >/dev/null
cat > "$OUT/mod.ini" <<'INI'
name = Enemy HP
version = 2.3
author = totk-mod-merger
description = Enemy HP as numbers above their gauge (and under the long gauge of bosses and mini-bosses), as with the Champion's Tunic of Breath of the Wild, and the enemies around the player recovering their HP. Options: size of the numbers, armour that displays them. Regeneration settings: enemy-hp.ini.
priority = 10
INI
cat > "$OUT/enemy-hp.ini" <<'INI'
# Settings of the Enemy HP plugin (re-read every time the game starts).

# Regeneration of the enemies around the player: the ones whose gauge is shown.
regen = 1
# Share of max HP recovered per second, in %. Decimals are accepted
# (0.5 as well as 0,5).
regen_percent = 1.5
# Seconds without being hit before regeneration starts.
regen_delay = 8
# Bosses and mini-bosses (the ones with a long gauge: Hinox, Construct,
# Molduga...) regenerate too.
regen_bosses = 1
# Their own share, if they should recover their HP faster or slower than the
# others (they have thousands of it). Without this line: regen_percent.
#regen_percent_bosses = 0.5
regen_percent_bosses = 0.2

# enemy-hp.txt: the enemy HP read from the merged files.
report = 0

# Detailed regeneration log in skyline.log (for a bug report).
debug = 0
INI

echo "Built $OUT: EnemyHp.tkcl ($(wc -c < "$OUT/EnemyHp.tkcl") bytes), plugin.nro, mod.ini"
