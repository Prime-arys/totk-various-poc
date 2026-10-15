# Mods

*[Version française](mods.fr.md)*

A pack holds what is needed to run mods, not mods: they are built separately and dropped into
`sd:/totk/mods/`. The repository builds one end to end, **EnemyHp**, in two editions that serve
as complete examples of a mod with both data *and* code — one for each way a mod can bring code:

| Edition | Its code | Its settings | Needs |
|---|---|---|---|
| **EnemyHp** | a Skyline plugin (`plugin.nro`), in Rust | a file, `enemy-hp.ini` | skyline-totk and the merger |
| **EnemyHp SE** | an exefs module (`subsdk9`) inside the `.tkcl`, built with exlaunch | the options of the package | nothing: the same file imports into TKMM |

Both show the enemies' HP as numbers and make the enemies around the player recover them. Use one
or the other: they hook the same two instructions of the game, and whichever comes second finds
them taken and stays out.

## Building EnemyHp

```bash
./build.sh --romfs /path/to/romfs mod
```

```powershell
.\build.ps1 -Romfs D:\romfs mod
```

The romfs — an extracted dump of **TotK 1.2.1** — is not in the repository and never enters it:
it is mounted read-only in the container for the length of the build. The mod's data is computed
from the game's own files, so it is required.

The result lands in `release/mods/EnemyHp/`:

| File | What it is |
|---|---|
| `EnemyHp.tkcl` | the data, packaged by TKMM's code (`utils/tkmm-oracle`, which builds against the `utils/TkSharp` submodule): the numbers above the gauges, the text pane under the boss gauge, and the armour effect that turns them on |
| `plugin.nro` | the code: regeneration of the enemies around the player, the boss numbers, the `enemy-hp.txt` report |
| `enemy-hp.ini` | the plugin's settings, re-read every time the game starts |
| `mod.ini` | name, version, description, priority |

The folder is copied as is into `sd:/totk/mods/EnemyHp/`, then enabled from the homebrew.

The mod offers two option groups, chosen in the manager: the **size of the numbers** (four sizes)
and the **armour** that displays them (the new Champion's Tunic alone, as in *Breath of the
Wild*, or every armour).

### The regeneration settings

`enemy-hp.ini`, next to the `plugin.nro`:

```ini
regen = 1                  # enemies whose gauge is shown recover their HP
regen_percent = 1.5        # share of max HP per second, in % (decimals work)
regen_delay = 8            # seconds without being hit before it starts
regen_bosses = 1           # bosses and mini-bosses too (the ones with a long gauge)
regen_percent_bosses = 0.2 # their own share, if they should be slower
report = 0                 # enemy-hp.txt: the HP read from the merged files
debug = 0                  # detailed log in skyline.log
```

## Building EnemyHp SE

```bash
./build.sh --romfs /path/to/romfs mod-se      # or "mods" for both editions
```

```powershell
.\build.ps1 -Romfs D:\romfs mod-se
```

The result is **one file**, `release/mods/EnemyHpSe/EnemyHpSe.tkcl`, which holds everything the
way TKMM packages a mod:

| In the package | What it is |
|---|---|
| `romfs` | the same data as the first edition |
| `exefs/subsdk9` | the code: a module built with [exlaunch](https://github.com/shadowninja108/exlaunch) (the `utils/exlaunch` submodule) from [`mods/enemy-hp-se/source`](../mods/enemy-hp-se/source) — regeneration and the boss numbers; no report |
| `exefs/main.npdm` | the permissions the module is loaded under: skyline-totk's, byte for byte |
| options | the two groups of the first edition, plus one per setting (below) |

It goes in `sd:/totk/mods/EnemyHpSe/` for the merger — or straight into TKMM on a PC, where it is
an ordinary mod with code and needs neither Skyline nor the merger.

### Its options

What the first edition reads in `enemy-hp.ini` is chosen here, in the mod manager or in TKMM:

| Group | Options (default in bold) |
|---|---|
| Number size | Small, Medium, **Large**, Very large |
| Armour | **Champion's Tunic**, Every armour |
| Regeneration | Off, 0.5 %, 1 %, **1.5 %**, 2 %, 3 %, 5 % of max HP per second |
| Regeneration delay | 3, 5, **8**, 12, 20 seconds without being hit |
| Boss regeneration | Off, 0.1 %, **0.2 %**, 0.5 %, 1 %, Same as enemies |

An option of a package can only bring files, and a module can only read the game's. So each of
the three regeneration groups ships one small text file in romfs — `EnemyHp/Regen.txt`,
`EnemyHp/RegenDelay.txt`, `EnemyHp/RegenBoss.txt`, one line each, with the keys of
`enemy-hp.ini` — which the merged game serves like any other file and the module reads the first
time a gauge is drawn ([`options.cpp`](../mods/enemy-hp-se/source/options.cpp)).

### How its code gets loaded

A module is not loaded by Skyline or by the merger: the console (or Ryujinx) maps it with the game
when the game **starts**. What the merger does is put it where it is looked for —
`atmosphere/contents/<title>/exefs/subsdk1`, next to Skyline's `subsdk9` — when the mods are
merged, and take it away when the mod is turned off (see
[the merger's README](../totk-mod-merger-plugin/README.md#mods-with-code)). Two consequences:

- **apply the mods from the mod manager before launching the game.** A change the merger makes
  while the game boots only shows the next time it starts — its log says so;
- the module is for **TotK 1.2.1**. On any other build it finds the game's code different and
  does nothing, as does the plugin of the first edition.

The module says what it did in the debug log (`svcOutputDebugString`, which Ryujinx shows in its
own log): `[enemy-hp-se] regeneration on (enemy gauge hooked), boss gauge hooked`, then the
options it read the first time an enemy's gauge shows.

## Making your own mod

A mod is a folder in `sd:/totk/mods/`:

```text
totk/mods/My Mod/romfs/...        a mod as a folder (the files as they go into the game)
totk/mods/My Mod/My Mod.tkcl      or a TKMM package
totk/mods/My Mod/mod.ini          optional: name, version, priority
totk/mods/My Mod/plugin.nro       optional: code, as a Skyline plugin
totk/mods/My Mod/exefs/subsdk9    optional: code, as an exefs module (in a folder or in the .tkcl)
totk/mods/My Mod/atmosphere/exefs_patches/<name>/<build id>.ips
                                  or code patches packaged for Atmosphère, read as they are
```

The merger reads the enabled mods, merges their files (BYML, SARC, RSTB, MSBT, code patches) and
serves the result to the game; then it loads the `plugin.nro` of every mod that provides one, and
leaves the modules of the others in the title's exefs folder for the next start. What a plugin
can ask of the merger is described in [the merger's README](../totk-mod-merger-plugin/README.md);
the `tkm_*` API lives in [`sharedlibs/totk-mod-merger-api`](../sharedlibs/totk-mod-merger-api).

For a plugin written here, the easiest starting point is `plugins/online-example` (short) or
`plugins/enemy-hp` (complete: hooks, reading the "life" component, interface). Adding them to the
root `Cargo.toml` is enough for them to be built:

```bash
./build.sh plugins          # release/plugins/*.nro
```

For a module, start from [`mods/enemy-hp-se`](../mods/enemy-hp-se): its `source/` folder is an
exlaunch *program* (`main.cpp` with `exl_main`, `setting.hpp`), and
[`utils/build-exlaunch.sh`](../utils/build-exlaunch.sh) builds any such folder into a `subsdk9`
and a `main.npdm`:

```bash
./build.sh -- bash utils/build-exlaunch.sh my-mod --source mods/my-mod/source --out output/my-mod/exefs
```

Which to choose: a plugin is loaded at once (no restart), can use what Skyline and the merger
export, and is written in Rust here; a module is what TKMM users can install, and what existing
exlaunch mods are.

## Comparing your merge against TKMM's

`utils/tkmm-oracle` runs the real TkSharp. That is what serves to check, file by file, that the
Rust merger gives the same result as TKMM — modules included (`exefs/subsdk1`…):

```bash
./build.sh --romfs /path/to/romfs shell
dotnet "$(find output/dotnet -name tkmm-oracle.dll)" merge /romfs output/tmp/tkmm <mods...>
cargo run --release -p totk-merge --example compare_merge -- /romfs output/tmp/tkmm output/tmp/ours <mods...>
```

The other examples in [`sharedlibs/totk-merge/examples/`](../sharedlibs/totk-merge/examples)
replay the reading of the game's formats on a PC: `enemy_hp`, `armor_effects`, `byml_print`,
`sarc_list`, `msbt_dump`…

When a package does not do what it should, `tkcl_contents` lists what it really holds — the files
it changes, its code patches and the build they target, and its exefs code (`subsdk9`,
`main.npdm`), for the mod itself and for each of its options:

```bash
cargo run --release -p totk-merge --example tkcl_contents -- "/mods/My Mod.tkcl"
```
