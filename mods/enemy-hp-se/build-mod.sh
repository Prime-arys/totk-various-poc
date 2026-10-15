#!/usr/bin/env bash
# Builds the SE edition of the Enemy HP mod: one file, EnemyHpSe.tkcl, which
# holds everything the way TKMM packages a mod —
#
#   romfs          the data (the numbers over the gauges), as in the first
#                  edition
#   exefs/subsdk9  the code, a module built with exlaunch (source/): the
#                  regeneration of the enemies around the player, and the
#                  numbers under the boss gauge
#   options        what the first edition asks in enemy-hp.ini is chosen here:
#                  - Number size: Small, Medium, Large (default), Very large
#                  - Armour: Champion's Tunic (default), or every armour
#                  - Regeneration: off, or 0.5 to 5 % of max HP per second
#                  - Regeneration delay: 3 to 20 seconds without being hit
#                  - Boss regeneration: off, 0.1 to 1 %, or as the enemies
#
# The same file goes in sd:/totk/mods/EnemyHpSe/ for totk-mod-merger, or is
# imported in TKMM: it needs neither Skyline nor a plugin.
#
# Usage: ./build-mod.sh <extracted romfs> <out folder>
#
# Needs the tkmm-oracle tool built to package the .tkcl, and devkitA64 for the
# module. `./build.sh --romfs <romfs> mod-se` does the whole thing, tool
# included, inside the container.

set -e
set -o pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$DIR/../.." && pwd)"
ROMFS="${1:?usage: build-mod.sh <romfs> <out folder>}"
OUT="${2:?usage: build-mod.sh <romfs> <out folder>}"
# Where the packager is: utils/build-mod.sh builds it and says so through
# TKMM_ORACLE. Alone, look for it under output/dotnet.
ORACLE="${TKMM_ORACLE:-$(find "$ROOT/output/dotnet" -name tkmm-oracle.dll 2>/dev/null | head -1 || true)}"
if [ -z "$ORACLE" ]; then
    echo "error: tkmm-oracle.dll not found; build it with utils/build-mod.sh" >&2
    exit 1
fi

# Somewhere to assemble the mod before packaging it. TMPDIR points inside
# output/ when the build calls this script.
PROJECT="$(mktemp -d)"
trap 'rm -rf "$PROJECT"' EXIT
cd "$ROOT"

# --- the data: the same as the first edition's, named in English -----------

# The numbers: text panes in the enemy and boss gauge layouts (one option per
# size) and the messages the enemy ones are filled from.
ENEMY_HP_LABELS=en \
    cargo run --release -q -p totk-merge --example make_enemy_life_ui_mod -- "$ROMFS" "$PROJECT"

# What turns them on: the game's VisualizeLife armour effect, on the
# Champion's Tunic only (as in Breath of the Wild) or on every armour.
GROUP="$PROJECT/options/2 Armour"
cargo run --release -q -p totk-merge --example make_visualize_life_mod -- \
    "$ROMFS" "$GROUP/1 Tunic" tunic
cargo run --release -q -p totk-merge --example make_visualize_life_mod -- \
    "$ROMFS" "$GROUP/2 All"
rm -f "$GROUP/1 Tunic/mod.ini" "$GROUP/2 All/mod.ini"

# --- the options -----------------------------------------------------------

# TKMM's option group types, as its project files number them.
SINGLE_REQUIRED=3

# group <folder> <name> <description> <priority>
group() {
    mkdir -p "$PROJECT/options/$1"
    cat > "$PROJECT/options/$1/info.json" <<JSON
{
  "Name": "$2",
  "Description": "$3",
  "Type": $SINGLE_REQUIRED,
  "Priority": $4
}
JSON
}

# option <group folder> <folder> <name> <description> <default: true|false>
option() {
    mkdir -p "$PROJECT/options/$1/$2"
    cat > "$PROJECT/options/$1/$2/info.json" <<JSON
{
  "Name": "$3",
  "Description": "$4",
  "Priority": 0,
  "IsDefaultSelected": $5
}
JSON
}

# setting <group folder> <folder> <name> <description> <default> <file> <line>
#
# An option that sets the module. An option can only bring files, and the
# module can only read the game's: so each group ships one small text file in
# romfs (EnemyHp/<file>), which the merged game serves like any other and the
# module reads the first time a gauge is drawn (source/options.cpp).
setting() {
    option "$1" "$2" "$3" "$4" "$5"
    mkdir -p "$PROJECT/options/$1/$2/romfs/EnemyHp"
    printf '%s\n' "$7" > "$PROJECT/options/$1/$2/romfs/EnemyHp/$6"
}

group "2 Armour" "Armour" \
    "Which armour shows the enemies' HP while it is worn." 1
option "2 Armour" "1 Tunic" "Champion's Tunic" \
    "As in Breath of the Wild: only while wearing it (chest, every upgrade level)." true
option "2 Armour" "2 All" "Every armour" \
    "The HP show whatever is worn." false

REGEN="3 Regeneration"
group "$REGEN" "Regeneration" \
    "Share of their maximum HP the enemies around you recover each second." 2
setting "$REGEN" "1 Off"  "Off"   "Enemies do not recover HP."                    false Regen.txt "regen_percent = 0"
setting "$REGEN" "2 0.5"  "0.5 %" "Slow: a full recovery takes over three minutes." false Regen.txt "regen_percent = 0.5"
setting "$REGEN" "3 1"    "1 %"   "A full recovery in 100 seconds."               false Regen.txt "regen_percent = 1"
setting "$REGEN" "4 1.5"  "1.5 %" "A full recovery in about a minute."            true  Regen.txt "regen_percent = 1.5"
setting "$REGEN" "5 2"    "2 %"   "A full recovery in 50 seconds."                false Regen.txt "regen_percent = 2"
setting "$REGEN" "6 3"    "3 %"   "A full recovery in about half a minute."       false Regen.txt "regen_percent = 3"
setting "$REGEN" "7 5"    "5 %"   "Fast: a full recovery in 20 seconds."          false Regen.txt "regen_percent = 5"

DELAY="4 Regeneration delay"
group "$DELAY" "Regeneration delay" \
    "How long an enemy must go without being hit before it starts recovering." 3
setting "$DELAY" "1 3"  "3 seconds"  "Barely a pause between two hits." false RegenDelay.txt "regen_delay = 3"
setting "$DELAY" "2 5"  "5 seconds"  "A short break in the fight."      false RegenDelay.txt "regen_delay = 5"
setting "$DELAY" "3 8"  "8 seconds"  "Enough to change weapons."        true  RegenDelay.txt "regen_delay = 8"
setting "$DELAY" "4 12" "12 seconds" "Enough to step back and eat."     false RegenDelay.txt "regen_delay = 12"
setting "$DELAY" "5 20" "20 seconds" "Only when the fight is left."     false RegenDelay.txt "regen_delay = 20"

BOSS="5 Boss regeneration"
group "$BOSS" "Boss regeneration" \
    "The same share for bosses and mini-bosses (the ones with a long gauge: Hinox, Talus, Molduga, Gleeok...), which have thousands of HP." 4
setting "$BOSS" "1 Off"  "Off"             "Bosses and mini-bosses do not recover HP." false RegenBoss.txt "regen_percent_bosses = 0"
setting "$BOSS" "2 0.1"  "0.1 %"           "Very slow."                                false RegenBoss.txt "regen_percent_bosses = 0.1"
setting "$BOSS" "3 0.2"  "0.2 %"           "Slow: a long fight is not undone."         true  RegenBoss.txt "regen_percent_bosses = 0.2"
setting "$BOSS" "4 0.5"  "0.5 %"           "A full recovery in over three minutes."    false RegenBoss.txt "regen_percent_bosses = 0.5"
setting "$BOSS" "5 1"    "1 %"             "A full recovery in 100 seconds."           false RegenBoss.txt "regen_percent_bosses = 1"
setting "$BOSS" "6 Same" "Same as enemies" "Whatever Regeneration says."               false RegenBoss.txt "regen_percent_bosses = same"

# --- the code --------------------------------------------------------------

# The module, and the permissions it is loaded under (skyline-totk's, so
# installing the mod next to Skyline changes nothing there).
bash utils/build-exlaunch.sh enemy-hp-se --source "$DIR/source" --out "$PROJECT/exefs"

# --- the package -----------------------------------------------------------

# What TKMM (and the merger's log) calls the package.
cat > "$PROJECT/.tkproj" <<'JSON'
{
  "Mod": {
    "Name": "Enemy HP SE",
    "Version": "1.0",
    "Author": "totk-mod-merger",
    "Description": "Enemy HP as numbers above their gauge (and under the long gauge of bosses and mini-bosses), as with the Champion's Tunic of Breath of the Wild, and the enemies around the player recovering their HP. Everything is in the package: the code is an exefs module (for Tears of the Kingdom 1.2.1), and its settings are the options."
  }
}
JSON

mkdir -p "$OUT"
dotnet "$ORACLE" package "$ROMFS" "$PROJECT" "$OUT/EnemyHpSe.tkcl" | tail -1

echo "Built $OUT/EnemyHpSe.tkcl ($(wc -c < "$OUT/EnemyHpSe.tkcl") bytes)"
