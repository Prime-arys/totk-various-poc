# totk-mod-merger-plugin

*[Version française](README.fr.md)*

Merging mods **directly on the console**, for The Legend of Zelda: Tears of the Kingdom.

This is the on-board equivalent of [TKMM](https://github.com/TKMM-Team/Tkmm): when the game
starts, the plugin works out what each mod changes compared to the game's romfs (TKMM's
*changelogs*), replays those changes over one another, rebuilds the resource size table (RSTB) and
serves the result to the game through `nn::fs`. In the spirit of
[ARCropolis](https://github.com/Raytwo/arcropolis) for Smash.

- "folder" mods (`romfs/`, `exefs/`) **and TKMM `.tkcl` packages**, options included;
- **profiles** (named mod lists, order and options per profile);
- a **C API** (`tkm_*`) so another plugin (an online mode…) can impose its own mod list;
- reuses a **RomFSlite** export from TKMM as a base mod;
- a result **equivalent to TKMM's**, checked file by file (see [Tests](#tests)).

It runs on [skyline-totk](../skyline-totk). The [totk-mod-manager](../totk-mod-manager) homebrew
manages mods and profiles from the console, installs mods from GameBanana and runs the merge
**before** launching the game ("Apply"): it carries the same merging code (this repository's
crates, compiled without `std`), so the plugin finds its merge again and starts without waiting.

## Installation

1. Install skyline-totk (see its README).
2. Copy `totk-mod-merger-plugin.nro` into
   `sd:/atmosphere/contents/0100F2C0115B6000/skyline/plugins/`
   — **not** into `.../romfs/skyline/plugins`: as soon as a `romfs` folder exists for the title,
   Atmosphère builds a "LayeredFS" romfs over TotK's ~300,000 files at every boot (slow, and
   hungry enough to keep the game from starting on recent firmwares). The plugin warns in its log
   when that folder exists.
3. Drop the mods into `sd:/totk/mods/`, **each in its own folder** (or install them with
   totk-mod-manager):

```
sd:/totk/
├── config.ini                    (optional; profile = the active profile)
├── mods/
│   ├── My Mod/                   a "folder" mod
│   │   ├── mod.ini               (optional: name, version, author, description…)
│   │   ├── romfs/...
│   │   ├── exefs/*.pchtxt|*.ips  (optional: code patches)
│   │   └── plugin.nro            (optional: the mod's code, see Mods with code)
│   └── Other Mod/                a TKMM package
│       ├── mod.ini               (optional)
│       └── Other Mod.tkcl
├── profiles/
│   └── Default.ini               mod lists (see Profiles)
├── locale.txt                    (generated: the language the game reads)
├── cache/                        (generated: changelogs of folder mods, conflicts.tsv)
├── merged/                       (generated: the merges kept, see Merge cache)
└── merger.log                    (generated)
```

A mod one folder lower (`My Mod/My Mod v2/romfs`, the way many archives extract) is found too. A
`.tkcl` dropped loose in `mods/` is ignored (the log says so; totk-mod-manager moves it into a
folder when it starts).

The merge only happens on the first boot with a combination of mods never merged before;
afterwards the result is reused as is (an instant boot). `force_merge = 1`, or deleting `merged/`,
starts it again. With totk-mod-manager, it happens in the homebrew and the game starts straight
away.

### Merge cache

The last `merge_cache_size` merges (10 by default) stay on the SD card. Coming back to mods
already merged (switching profile, re-enabling a mod, restoring an option) serves the kept merge
without computing anything. Beyond that, the least recently used one is removed after the next
merge.

```
sd:/totk/merged/
├── store/<xx>/<fingerprint>-<size>   merged files, named after their contents (xxHash64)
├── <id>/                             one merge: index.tsv (game path → served file),
│                                     patches.tsv, plan.txt, locales.txt, profile.txt,
│                                     conflicts.tsv, then stamp.txt last (a complete merge)
└── recent.txt                        the merges, most recent first
```

A file another merge already produced is not written again: merging the same mods writes nothing,
and a variant (one mod fewer) only writes the files that change. Over 4 real mods (1,321 files
served, 141 MiB): 16 files written for the same list minus two mods, and going back to the first
merge takes 0.01 s. Files kept as they are inside the mods' folders are never copied. An
interrupted merge (no `stamp.txt`) is ignored, then removed.

The end of every merge is detailed in the log:
`merged 2708 files (1321 served) in 3.9s: reading mods 0.8s, comparing them 0.0s, merging 3.1s (of which writing packs 0.2s); 1175 file(s) written (141 MiB), 0 already stored`.

## Configuration

`sd:/totk/config.ini` — see [`config.example.ini`](config.example.ini). The useful settings:

| Key | Default | Role |
|---|---|---|
| `profile` | *(empty)* | the active profile (`sd:/totk/profiles/<name>.ini`); empty: every mod, by `priority` |
| `mods_dir` / `profiles_dir` | `sd:/totk/mods` / `sd:/totk/profiles` | the mod and profile folders |
| `merge_at_boot` | `1` | mods changed since the last merge: merge at boot (`1`) or keep the last merge until "Apply" in the manager (`0`, always a fast boot) |
| `merge_cache_size` | `10` | how many merges are kept (see Merge cache) |
| `locales` | `auto` | texts (`Mals`) to merge: `auto` (the language the game reads), `all`, or a list such as `USen,EUfr` |
| `use_romfslite` | `1` | use a TKMM RomFSlite export as a base mod |
| `apply_patches` | `1` | apply the mods' `.ips`/`.pchtxt` patches in memory |
| `mod_plugins` | `1` | load the plugins mods provide (see Mods with code) |
| `shop_param_limit` | `512` | the shop limit TKMM raises at every merge (`0`: off) |
| `control_timeout_ms` | `60000` | the longest wait for a plugin that took control (API) |
| `log_redirects` / `verbose` | `0` | diagnostic logs |

**Languages.** TotK only reads one text archive, the one of its language. In `auto`, the plugin
notes at startup which one it opens (`sd:/totk/locale.txt`) and later merges only handle that one:
over 4 real mods that touch texts, 145 MB written to the SD card instead of 274. Until the
language is known (the first merge), all of them are merged. A merge holding more languages than
needed stays valid; changing the console's language redoes the merge at the next boot.

Per mod, `<mod>/mod.ini` (written by totk-mod-manager on an installation):

```ini
name = Displayed name
version = 1.2
author = Author
description = One line\nor several
url = https://gamebanana.com/mods/123456
thumbnail = thumbnail.jpg
plugins = 1             # 0: keep the mod's files, do not load its code
# Without a profile only:
enabled = 1
priority = 100          # higher = merged later = wins conflicts

[options]
# This mod's default options (a profile can override them):
# group = option(s), separated by ";"
Colour = Red
Weapons = Swords; Bows
```

### Profiles

`sd:/totk/profiles/<name>.ini`, the active one being `profile` in `config.ini`. Mods are listed by
folder name, **the first one wins** over the following ones (like TKMM's list); a mod that is
installed but absent from the profile is not loaded.

```ini
[mod]
folder = Weapons of Legend Redux
enabled = 1
option.Weapon type = Swords; Bows

[mod]
folder = Even More Wonderful Capsules
enabled = 0
```

### Mods with code

A mod can provide its own **Skyline plugin**, which the merger loads into the game once the merge
is served:

```
sd:/totk/mods/My Mod/
├── mod.ini
├── romfs/...                  (optional: a mod can be code and nothing else)
├── plugin.nro                 loaded after the merge
└── plugins/*.nro              (or several, loaded in alphabetical order)
```

This is what lets **several code mods coexist**: a console has only one exefs
(`atmosphere/contents/<title>/exefs`), already taken by Skyline, so two mods each replacing
`subsdk9` cannot be installed together; as plugins, they load one after the other.

- Only the plugins of mods **enabled in the profile** are loaded, in merge order.
- The same `.nro` provided by two mods is only loaded once.
- `plugins = 0` in the `mod.ini` keeps the mod's files and leaves its code out; `mod_plugins = 0`
  in `config.ini` turns them all off (totk-mod-manager exposes both).
- Loading goes through `nn::ro`, like Skyline's: reading, registering the fingerprints, then
  `main`. The log lists what is loaded.

Such a plugin is an ordinary Skyline plugin (hooks, `nn::fs`, memory patches). It runs **after**
the merge: it therefore reads the merged files, but it is too late to pick the mods (for that, a
plugin in `skyline/plugins`, see *API for other plugins*). Two functions tell it where it comes
from:

```rust
let dir  = totk_mod_merger_api::current_mod_dir();   // "sd:/totk/mods/My Mod"
let name = totk_mod_merger_api::current_mod_name();  // "My Mod"
```

> The game's heap only has a few megabytes free while the plugins run: a plugin that reads the
> game's files has to take its large buffers elsewhere, as
> [`plugins/enemy-hp/src/scratch.rs`](../plugins/enemy-hp/src/scratch.rs) does (memory mapped by
> skyline-totk). Without that, the first one-megabyte allocation crashes the game.

A complete example: [`plugins/enemy-hp`](../plugins/enemy-hp) — a mod that is nothing but code
(`mod.ini` + `plugin.nro`). It reads the enemies' parameters **from the merged files** and writes
their hit points into `enemy-hp.txt`, next to the mod:

```
Enemy_Bokoblin_Junior   25
Enemy_Bokoblin_Middle   72
Enemy_Lynel_Senior      4000
```

The same values can be checked on a PC:
`cargo run --release -p totk-merge --example enemy_hp -- <romfs>` (the computation is the plugin's
own file, included as is).

**Showing them above the life bar**, the way the Champion's Tunic does in Breath of the Wild,
needs no code: TotK always does it, it is only missing the data.

- The enemy gauge's code (around `0x12c7b90` in the `main` of 1.2.1) computes the current/max HP
  ratio for the `Gauge` animation, then, if the armour being worn has the **`VisualizeLife`**
  effect, plays the `TextVisible` animation and writes the two numbers, formatted by the
  `LayoutMsg/EnemyInfo_00` messages `0000` and `0001`, into the **`T_CurrentLife_00`** and
  **`T_MaxLife_00`** panes.
- But no TotK armour has that effect, `blyt/PaEnemyLife_00.bflyt` has no text left (just the empty
  `N_TextVisible_00` anchor that animation shows) and `EnemyInfo_00.msbt` is no longer in the
  message archives.

Two examples rebuild all of that:
[`make_enemy_life_ui_mod`](../sharedlibs/totk-merge/examples/make_enemy_life_ui_mod.rs) adds the
two panes under the anchor (the `Normal_00` font, one layout variant per text size) and the two
messages (the "number" tag of one of the game's messages);
[`make_visualize_life_mod`](../sharedlibs/totk-merge/examples/make_visualize_life_mod.rs) gives the
effect to the new Champion's Tunic (`Armor_1106..1110_Upper`) or to every armour, keeping their
original effects. [`plugins/enemy-hp/build-mod.sh`](../plugins/enemy-hp/build-mod.sh) puts it all
together into a 284 KB `.tkcl` with two option groups (size of the numbers, armour), plus the
plugin:

```bash
./build.sh --romfs <romfs> mod                       # at the root of the repository
../plugins/enemy-hp/build-mod.sh <romfs> <folder>    # or on its own, in the container
```

Checked in game: "35/35", "840/840" above the gauges with the tunic worn.

**Bosses and mini-bosses** (Hinox, Constructs, Moldugas, Gleeoks, temple bosses…) have a gauge of
their own, at the top of the screen: `blyt/BossLife_00.bflyt`, updated every frame by `0x1ae2178`
(the `UIBossLifeScreen`), which reads the same HP fields. It never had numbers and the game has no
code to write any, so this time it is the plugin
([`plugins/enemy-hp/src/boss.rs`](../plugins/enemy-hp/src/boss.rs)): a hook at `0x1ae22b0` (the
life component in `x0`, the screen in `x19`) writes "current/max" into the **`T_BossLife_00`** pane
that `make_enemy_life_ui_mod` adds, empty, under the start of the bar (the bar is anchored to the
left and grows with the boss' max HP). It goes through the game's own function for writing the
boss' name, `0xb519b0(layout = [screen + 40], pane name, message, 1, 0)`, with a message
`{UTF-16 text, length, -1}` as `0x1235fc4` builds it; only when the text changes, and only when the
byte the enemy gauges test is raised (`[[0x462ec80] + 2204]`, the `VisualizeLife` effect). The same
hook makes the boss regenerate, unless `regen_bosses = 0` (and `regen_percent_bosses` gives them a
pace of their own: they have thousands of HP).

The bar itself is drawn by three animations (`[screen + 416]`, `+ 424` and `+ 448`): the game puts
the first one where the HP are and lets the others catch up, but only once the damage animation is
over — and HP given back change the life every frame, which restarts it endlessly, so the bar
stayed at the last hit taken while the numbers climbed. When it gives HP back, the plugin sets the
three animations on the matching frame itself (`SetFrame`, vtable + 208), exactly as the game does
when the gauge fills up (`0x1ae2228`). What the plugin knows of the game's code is gathered in
[`plugins/enemy-hp/src/game.rs`](../plugins/enemy-hp/src/game.rs).

**Regeneration**, on the other hand, is code: the mod's plugin
([`plugins/enemy-hp/src/regen.rs`](../plugins/enemy-hp/src/regen.rs)) puts an *inline* hook at
`0x12c7a48`, where the gauge's code has just obtained the "life" component of the enemy being
displayed, right before reading its HP: `[[life + 6192] + 8]` (current), `[[life + 6200] + 8]`
(max), minus `[[life + 6216] + 8]` when that pointer exists — the fields the game's damage
function (`0x64b4b8`) changes and the numbers display. (`[[life + 6208] + 8]`, which the
`0x15ebf38`/`0x16f62a4` functions read, is a second pool that takes hits before the HP do, empty
for an ordinary enemy: those are not its HP.) There it raises the HP by `regen_percent` % of the
max per second (decimals accepted, dot or comma), `regen_delay` seconds after the last hit taken,
without ever going over the max or reviving an enemy at 0 HP; the write is atomic, like the game's,
and gives way to a hit landing at the same moment. The enemies concerned are those whose gauge the
game shows, that is, those around the player. The hook is only installed when
`totk_get_version() == 10201` and the bytes at the site are the expected ones; otherwise the log
says so and the rest of the mod works. Settings: `enemy-hp.ini` in the mod's folder; `debug = 1`
writes what the hook sees into `skyline.log` (the first call, every enemy, hits, HP given back,
enemies skipped and why).

## What merging does

It is a port of `TkSharp.Merging` (TKMM 2.x), merger by merger:

| Files | Treatment |
|---|---|
| `*.bgyml`, `*.byml` | merged key by key; arrays by position, by value or **by key** (`Actors`/`Hash`, `BoneList`/`BoneName`… — TKMM's ~150 tables) |
| `RSDB/*.rstbl.byml` | merged row by row (`__RowId`, `Name`, `NameHash`, `FullTagId`); the tag table per entry |
| `GameData/GameDataList` | merged by hash within each table, then the save metadata is recomputed |
| `Mals/*.sarc` (`.msbt`) | merged by text label, for every language |
| `.pack` | each inner file is merged separately then put back into every pack that holds it |
| `.sarc`, `.blarc`, `.bfarc`, `.bkres`, `.genvb`, `.ta` | inner files merged recursively |
| `*.rsizetable` | rebuilt (TKMM's formulas, including `.ainb`/`.asb`/`.bstar`/`.mc`) |
| `exefs/*.ips`, `*.pchtxt` | patches for the game's version merged and applied in memory at startup |
| others (models, textures, sounds…) | the highest priority mod wins |

### Conflicts

Before merging, the engine compares what the mods change (the `conflicts` module):

- **file**: several mods provide a file that does not merge, with different contents (identical
  copies do not count);
- **values**: within a merged file (`.bgyml`/`.byml`, RSDB and GameData rows, `.msbt`, `.sarc`
  archives…), several mods give the same value different contents, or one replaces/removes a node
  in which another changes values. Additions do not count, and a mod giving the same value as the
  winning mod loses nothing. Texts are compared in the game's language.

Merging carries on in every case (the highest priority mod wins). Conflicts are written to
`merger.log` and to `sd:/totk/cache/conflicts.tsv`, which totk-mod-manager displays; the manager
also presents them before applying, with the option to cancel (`totk_merge::set_conflict_sink`).
The analysis only reads the files at least two mods touch: 0.0 s over the 4 real test mods, which
have one genuine conflict (`ELink2/elink2.Product.belnk` replaced by two mods).

A file a folder mod provides as is is not copied: it is served from its folder. The files produced
are "raw" zstd frames (uncompressed blocks): valid zstd, with no CPU cost on the console, a little
larger on the SD card.

### What is not taken from TKMM

- **Texture changelogs `__Combined.bntx`** in a `.tkcl`: ignored with a warning (complete BNTX
  files work, the highest priority one wins).
- **The `.bfres.mc` material fix** TKMM applies for the 1.4 shaders: useless in 1.2.1, not ported.
- **Code** (`subsdk*`, the `main` of an `exefs`) and **cheats**: ignored. Code mods have to be
  Skyline plugins — which is exactly what lets them coexist.
- TKMM's "file identical to an older version" check (a 13 MB list of checksums) is not carried: a
  folder mod built for an earlier version of the game is compared against the installed version.

## RomFSlite

[RomFSlite](https://tkmm.org/docs/settings) is a feature of the UltraCam optimiser /
[nx-optimizer](https://github.com/MaxLastBreath/nx-optimizer): its injected code serves the
contents of `atmosphere/contents/0100F2C0115B6000/romfslite/` to the game directly, **without
going through Atmosphère's LayeredFS** (which runs out of memory on recent firmwares with TotK's
300,000 files). TKMM can export its merge into that folder.

Reusing *its code* is not possible: it is closed and lives in UltraCam's exefs, which conflicts
with skyline-totk's. But **this plugin applies the same technique** (`nn::fs` redirection from the
SD card, never a `romfs` folder for the title), so it gets the same performance: no LayeredFS
rebuild at boot, and a merge done once then cached.

And the RomFSlite **format** is supported: when the `romfslite` folder exists, its contents are
merged below every other mod. So a TKMM profile exported from a PC can be kept, with mods added on
top on the console. (If UltraCam is installed as well, turn its RomFSlite off so the two do not
both serve files.)

## API for other plugins

A plugin can take over the mod list, an online mode downloading the pack every player shares, for
instance. While it has control, **the mods on the SD card are not loaded** (unless it asks for
them), only the ones it adds are merged, and the result goes into a separate folder (the local
mods' cache stays valid).

- The contract: [`include/totk_mod_merger.h`](include/totk_mod_merger.h)
- Rust bindings: [`sharedlibs/totk-mod-merger-api`](../sharedlibs/totk-mod-merger-api)
- A complete example: [`plugins/online-example`](../plugins/online-example) (downloads
  `manifest.txt` and the `.tkcl` files it lists from an HTTP server, imposes them, and falls back
  on the last packs downloaded or on the local mods when the server does not answer)

```rust
use totk_mod_merger_api::ModMerger;

#[skyline::main(name = "online")]
pub fn main() {
    let Some(merger) = ModMerger::find() else { return };
    let Some(control) = merger.take_control("online", 30_000) else { return };
    let worker = std::thread::spawn(move || {
        let pack = download_pack();                       // your code
        let index = control.add_mod(&pack, Some("Online pack")).unwrap();
        control.select_option(index, "Rules", "Ranked");
        control.set_merged_dir("sd:/totk/online/merged");
        control.commit();                                 // the merge starts
    });
    std::mem::forget(worker); // never drop a JoinHandle on Skyline (see below)
}
```

How it goes: skyline-totk runs every plugin's `main` (in no particular order), then calls the
merger through `skyline_totk_on_plugins_loaded`. The game is still blocked on mounting its romfs
at that point: the merger waits for `tkm_commit`/`tkm_release_control` (or for the timeout),
merges, installs the redirection, then calls the `tkm_on_merged` callbacks.
`skyline_totk_init_sockets` makes it possible to use the network that early.

> A Skyline trap: dropping a thread's `JoinHandle` calls `pthread_detach`, which crashes nnSdk.
> Join it or forget it (`std::mem::forget`).

## Memory and performance

By the time the plugins run, the game's heap only has a few megabytes free. So the plugin has a
heap of its own (`src/alloc.rs`) on memory mapped from the kernel by skyline-totk
(`totk_map_memory`), used only during the merge and then given back (`trim`) — only what is needed
afterwards (the redirection table) stays on the game's heap.

Large documents are read on demand (a GameDataList has 1.4 million nodes), BYML maps are sorted
vectors with shared keys, and BYML writing duplicates neither strings nor nodes: the heaviest test
merge (GameDataList + ActorInfo + texts in 14 languages + packs) peaks at **~120 MiB** instead of
213. Under Ryujinx, that merge takes ~8 s on the first boot, then 0 s (cache).

## Compiling

From the root of the repository, everything is built in the container (see
[docs/build.md](../docs/build.md)):

```bash
./build.sh totk-mod-merger-plugin.nro   # the merger
./build.sh plugins                      # the example plugins, in release/plugins/
```

Or on its own, in a shell inside the container (`./build.sh shell`):

```bash
utils/build-nro.sh totk-mod-merger-plugin
utils/build-nro.sh online-example        # the example plugin (API)
utils/build-nro.sh enemy-hp              # the example plugin provided by a mod
```

`cargo skyline build` is not used directly: 3.5 ships a *target spec* written for a newer rustc
than `skyline-v3`; [`utils/build-nro.sh`](../utils/build-nro.sh) adapts it, then converts the ELF
with `linkle` (`elf2nro` produces an NRO with `bss_size = 0` here, which crashes).

## Tests

```bash
cargo test -p totk-formats -p totk-merge
# against the game's real files (an extracted romfs):
TOTK_VANILLA_DIR=/path/romfs cargo test -p totk-formats -p totk-merge -- --nocapture
```

### Parity with TKMM

[`utils/tkmm-oracle`](../utils/tkmm-oracle) runs TKMM's own code (TkSharp) to package a `.tkcl` or
to merge mods; `compare_merge` merges the same mods with this plugin and compares the results
content by content (BYML trees, archive entries, MSBT labels, RSTB entries, patches).

```bash
dotnet output/dotnet/bin/TkmmOracle/release/tkmm-oracle.dll merge <romfs> out-tkmm modA modB package.tkcl
cargo run --release -p totk-merge --example compare_merge -- <romfs> out-tkmm work modA modB package.tkcl
cargo run --release -p totk-merge --example make_fixtures -- <romfs> fixtures   # realistic test mods
cargo run --release -p totk-merge --example peak_memory -- <romfs> work mods...
```

Results (TotK 1.2.1):

| Set of mods | Files | Result |
|---|---|---|
| 2 generated mods that overlap (the same document in a pack, ActorInfo, GameDataList, texts, tags) | 7 | equivalent |
| 1 folder mod + 1 `.tkcl` packaged by TKMM | 7 | equivalent |
| 5 real mods (including 2 on the same pack, a `.bcett` with keyed arrays, `.pchtxt` patches) | 42 | equivalent, RSTB included |

Under Ryujinx: 4 mods merged at boot (including a `.tkcl` with an option), patches applied, the
game up to the title screen with the merged files; the online example plugin: taking control,
downloading, falling back on the local mods, the closing callback. Still to be confirmed on a
console.

Mods with code, under Ryujinx: the `EnemyHp` mod (`mod.ini` + `plugin.nro`, no file) is loaded
after the merge, finds its folder again, reads 19 enemies in 0.4 s and writes its report. With a
mod setting the blue Bokoblin to 1 HP on top (`make_enemy_hp_mod`), the report says 1 instead of
72: the plugin really does read the merged files. Worth noting: the game's heap refused the first
megabyte the plugin asked for, hence `scratch.rs`.

## Architecture

```
src/                          the Skyline plugin: entry, tkm_* API, nn::fs hooks, patches, heap
../sharedlibs/totk-merge/     orchestration and merging, no_std + alloc (std feature by default)
  engine.rs control.rs mods.rs profile.rs cache.rs   merging, taking control, mods, profiles, cache
  conflicts.rs merge_cache.rs                        conflicts between mods, the merges kept
  tkcl.rs rom.rs canonical.rs config.rs ini.rs       TKMM packages, the game's files, settings
  builder.rs merger.rs                               TkChangelogBuilder / TkMerger
  byml_*.rs rsdb.rs gamedata.rs                      the mergers
../sharedlibs/totk-formats/   BYML, SARC, MSBT, RESTBL, zstd, ZIP; sys.rs: files/clock
../sharedlibs/totk-mod-merger-api/  Rust bindings of the API
src/plugins.rs                loading the plugins mods provide
../plugins/online-example/    the example plugin (takes over the mod list)
../plugins/enemy-hp/          the example mod that is nothing but code
include/totk_mod_merger.h     the C contract of the API
../utils/tkmm-oracle/         TKMM as a reference for comparisons
```

Both crates only depend on `alloc`. Every file access goes through `totk_formats::sys`: `std::fs`
with the `std` feature (the plugin, where Skyline implements it on `nn::fs`, and the tests on a
PC), or `tkm_host_*` functions provided by the host program without it — that is how
totk-mod-manager compiles exactly the same code for `aarch64-unknown-none`. Paths stay written
`sd:/…` everywhere (the homebrew resolves them to `sdmc:/…`), so the index and the merge
fingerprints are identical on both sides.

```bash
# checking that it compiles without std:
cargo +nightly-2024-10-09 build -p totk-merge --no-default-features \
    --target aarch64-unknown-none -Zbuild-std=core,alloc
```

`data/PackFileLookup.pkcache.zs` (the index of the files inside packs) comes from TKMM (MIT
licence, © TKMM-Team).
