# Console pack — skyline-totk, totk-mod-merger, totk-mod-manager

*Une version française de cette notice se trouve à côté : `LISEZMOI.md`.*

This pack holds what is needed to run *Tears of the Kingdom* mods on a Switch, **with no mod at
all**: mods are built separately and then dropped into `totk/mods/`.

| File | What it is |
|---|---|
| `atmosphere/contents/0100F2C0115B6000/exefs/subsdk9` + `main.npdm` | skyline-totk: Skyline adapted to the game |
| `atmosphere/contents/0100F2C0115B6000/skyline/plugins/totk-mod-merger-plugin.nro` | the mod merger |
| `switch/totk-mod-manager.nro` | the homebrew that manages the mods and runs the merge |
| `skyline/totk/config.ini` | skyline-totk's settings (log, plugin loading) |
| `totk/config.ini` | the merger's settings (profiles, cache, log) |

`contenu.txt`, next to this file, lists everything with sizes and MD5 fingerprints.

## Requirements

- **Tears of the Kingdom 1.2.1**.
- **Atmosphère with sigpatches**: skyline-totk replaces the game's `main.npdm`.
- No other exefs mod for TotK: rename `sd:/atmosphere/contents/0100F2C0115B6000/` if it already
  exists, and leave no `romfs/` folder there.

## Installation

Copy the **contents** of the `SD` folder to the root of the SD card.

To update a pack that is already installed, replacing `switch/totk-mod-manager.nro` and
`atmosphere/contents/0100F2C0115B6000/skyline/plugins/totk-mod-merger-plugin.nro` is enough.

## Use

1. On the HOME menu, **hold R while launching Tears of the Kingdom**: the homebrew menu opens.
2. Start **TotK Mod Manager**, enable and order the mods there, then **Apply** (**X**).
3. Launch the game from the closing message, or normally.

Mods go one per folder in `totk/mods/` (see `totk/mods/README.txt`). When something goes wrong,
`sd:/totk/merger.log` and `sd:/skyline/totk/skyline.log` tell what happened.
