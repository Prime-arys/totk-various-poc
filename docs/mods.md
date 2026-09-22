# Mods

*[Version française](mods.fr.md)*

A pack holds what is needed to run mods, not mods: they are built separately and dropped into
`sd:/totk/mods/`. The repository builds one end to end, **EnemyHp**, which serves as a complete
example of a mod with both data *and* code.

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

## The regeneration settings

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

## Making your own mod

A mod is a folder in `sd:/totk/mods/`:

```text
totk/mods/My Mod/romfs/...        a mod as a folder (the files as they go into the game)
totk/mods/My Mod/My Mod.tkcl      or a TKMM package
totk/mods/My Mod/mod.ini          optional: name, version, priority
totk/mods/My Mod/plugin.nro       optional: the code the mod brings
```

The merger reads the enabled mods, merges their files (BYML, SARC, RSTB, MSBT, code patches) and
serves the result to the game; then it loads the `plugin.nro` of every mod that provides one.
What a plugin can ask of the merger is described in
[the merger's README](../totk-mod-merger-plugin/README.md); the `tkm_*` API lives in
[`sharedlibs/totk-mod-merger-api`](../sharedlibs/totk-mod-merger-api).

For a plugin written here, the easiest starting point is `plugins/online-example` (short) or
`plugins/enemy-hp` (complete: hooks, reading the "life" component, interface). Adding them to the
root `Cargo.toml` is enough for them to be built:

```bash
./build.sh plugins          # release/plugins/*.nro
```

## Comparing your merge against TKMM's

`utils/tkmm-oracle` runs the real TkSharp. That is what serves to check, file by file, that the
Rust merger gives the same result as TKMM:

```bash
./build.sh shell
cargo run --release -p totk-merge --example compare_merge -- /romfs <mods...>
```

The other examples in [`sharedlibs/totk-merge/examples/`](../sharedlibs/totk-merge/examples)
replay the reading of the game's formats on a PC: `enemy_hp`, `armor_effects`, `byml_print`,
`sarc_list`, `msbt_dump`…
