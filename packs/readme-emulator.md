# Emulator pack (Ryujinx) — skyline-totk, totk-mod-merger, totk-mod-manager

*Une version française de cette notice se trouve à côté : `LISEZMOI.md`.*

The same contents as the console pack, laid out where Ryujinx expects them, and **with no mod at
all**: mods are built separately and then dropped into `sdcard/totk/mods/`.

| File | Where it goes |
|---|---|
| `Ryujinx/mods/contents/0100f2c0115b6000/skyline-totk/exefs/{subsdk9,main.npdm}` | the game's exefs mod |
| `Ryujinx/sdcard/atmosphere/contents/0100F2C0115B6000/skyline/plugins/totk-mod-merger-plugin.nro` | the merger, on the emulated SD card |
| `Ryujinx/sdcard/switch/totk-mod-manager-ryujinx.nro` | the homebrew (emulator build) |
| `Ryujinx/sdcard/skyline/totk/config.ini`, `Ryujinx/sdcard/totk/config.ini` | the settings |

`contenu.txt`, next to this file, lists everything with sizes and MD5 fingerprints.

## Requirements

- Ryujinx with **Tears of the Kingdom 1.2.1** (the base game plus its update).
- The exefs mods of other tools (UltraCam…) disabled for this game.

## Installation

Copy the contents of the `Ryujinx` folder into Ryujinx' data folder (`%AppData%\Ryujinx` on
Windows, `~/.config/Ryujinx` on Linux): the `mods` and `sdcard` folders are added there without
overwriting anything else.

The exefs mod should then appear in *Right click on the game → Manage mods*.

## Use

The homebrew is started with *File → Load a homebrew*, choosing
`sdcard/switch/totk-mod-manager-ryujinx.nro` — that is the build with the instruction Ryujinx'
JIT refuses taken out; the console build crashes at startup here.

Then as on a console: enable the mods, **Apply**, then launch the game. The logs are in
`sdcard/totk/merger.log` and `sdcard/skyline/totk/skyline.log`.
