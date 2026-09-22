# totk-various-poc

*[Version française](README.fr.md)*

This repository holds a fully **vibe-coded experimentation** for modding *The Legend of Zelda:
Tears of the Kingdom* (**v1.2.1**). It was made for a few reasons:

1. Switching mods easily on a real console, without needing a 16 GB copy of the game on the SD card.
2. Letting several exefs mods run together, for when IPS patches and cheats are not enough.
3. Building the mod I wanted to play with: **EnemyHp**, which shows enemy HP as numbers (as in
   BotW) and has them regenerate over time.

To get there, the repository holds a **Skyline** adapted to TotK, a **homebrew** that manages mods
and launches the game, a **merger plugin** that merges the mods and serves them to the game, and a
few example mods and plugins.

All of it is written for version **1.2.1** of the game, and it is an experimentation: 
full compatibility with existing mods and with every TKMM feature is not guaranteed.

## What is in the repository


| Directory                                            | What it is                                                                                                                                                                                                                              |
| ------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [`skyline-totk/`](skyline-totk/)                     | Skyline adapted to TotK 1.2.1: the`exefs` (`subsdk9` + `main.npdm`) that loads the plugins, with its log on the SD card, its mapped memory and its hooks. C++ / devkitPro.                                                              |
| [`totk-mod-merger-plugin/`](totk-mod-merger-plugin/) | The merger: a Skyline plugin that reads the mods in`sd:/totk/mods`, merges them (a port of TKMM) and serves the files to the game. It also loads the `plugin.nro` a mod may provide. Rust.                                              |
| [`totk-mod-manager/`](totk-mod-manager/)             | The homebrew (borealis) that manages mods, profiles, options and conflicts, runs the merge and launches the game. C++ for the interface, Rust (`core/`) for the shared logic.                                                           |
| [`sharedlibs/`](sharedlibs/)                         | The common crates:`totk-formats` (SARC, BYML, RSTB, zstd), `totk-merge` (finding and merging mods), `totk-mod-merger-api` (the `tkm_*` API for other plugins). They compile with or without `std`, for the console as well as for a PC. |
| [`plugins/`](plugins/)                               | Two example plugins:`enemy-hp` (the code of the EnemyHp mod: enemy HP, regeneration) and `online-example` (hands the merger a pack downloaded from a PC).                                                                               |
| [`packs/`](packs/)                                   | The description of the two packs and the notices they carry.                                                                                                                                                                            |
| [`utils/`](utils/)                                   | The scripts Meson calls inside the container,`tkmm-oracle` (TKMM's packager) and the `TkSharp` submodule.                                                                                                                               |
| [`docs/`](docs/)                                     | Building, and mods.                                                                                                                                                                                                                     |

What is borrowed, ported or used as a reference is listed in **[CREDITS.md](CREDITS.md)** — Skyline,
borealis, TKMM/TkSharp and the others.

Third-party code comes in as **two git submodules**, which `build.sh` fetches by itself when they
are missing:


| Submodule                                         | Where                               | Pinned version          | What for                                                                                  |
| --------------------------------------------------- | ------------------------------------- | ------------------------- | ------------------------------------------------------------------------------------------- |
| [borealis](https://github.com/xfangfang/borealis) | `totk-mod-manager/library/borealis` | `5f08b286` (2026-04-25) | the homebrew's interface (its CMake expects it there)                                     |
| [TkSharp](https://github.com/TKMM-Team/TkSharp)   | `utils/TkSharp`                     | `be46b9ad` (2026-09-15) | TKMM's code, which`utils/tkmm-oracle` runs to package `.tkcl` files and to compare merges |

Each submodule is frozen at the commit everything was built and checked with: a clone gets
exactly those versions, without following the upstream branch.

## Building

**Nothing to install but Docker.** Every toolchain (devkitPro, Rust for Skyline, Meson, .NET)
lives in an image; the repository is mounted inside it and that is where everything is built —
identically on Windows, Linux and macOS.

```bash
git clone <this repository> && cd totk-various-poc
./build.sh                  # the two packs, in release/
```

```powershell
.\build.ps1                 # the same thing from PowerShell
```

### What `build.sh` can do

```bash
./build.sh                          # pack-switch and pack-emulator  → release/
./build.sh pack-switch              # just one of them
./build.sh plugins                  # enemy-hp.nro, online-example.nro → release/plugins/
./build.sh --romfs <folder> mod     # the EnemyHp mod → release/mods/EnemyHp/
./build.sh test                     # the tests of the crates that run on a PC
./build.sh doctor                   # what the image holds
./build.sh shell                    # a shell inside the container
./build.sh clean [--all]            # empties output/ (and release/)
./build.sh --rebuild-image          # builds the image again
```

The same options exist in PowerShell (`-Romfs`, `-Zip`, `-RebuildImage`, `-All`…). Everything is
detailed in **[docs/build.md](docs/build.md)**; building mods, in **[docs/mods.md](docs/mods.md)**.

## Installation and usage

> What an SD card looks like. The build writes a pack into `release/`, whose contents are copied
> to the card.

```text
sd:/
├───atmosphere
│   └───contents
│       └───0100F2C0115B6000
│           ├───exefs                                  # skyline-totk's exefs
│           │       main.npdm
│           │       subsdk9
│           │
│           └───skyline
│               └───plugins
│                       totk-mod-merger-plugin.nro     # the merger plugin
│
├───skyline
│   └───totk
│           config.ini                                 # various options for skyline-totk
│
├───switch
│       totk-mod-manager.nro                           # the homebrew that manages mods and launches the game
│
└───totk
    │   config.ini                                     # options for the merger, shared with the manager
    │
    └───mods                                           # the mods, each in its own folder
        ├───EnemyHp
        │       EnemyHp.tkcl
        │       plugin.nro
        │       mod.ini
        │       enemy-hp.ini
        └───OtherExample
                romfs/
                thumbnail.jpg
                mod.ini
```

### Usage

To use the manager, open your homebrew launcher through the game: start TotK while holding **R**
on the controller. That way the manager can read the game's own files, merge the mods and launch
the game.

See **[the manager's README](totk-mod-manager/README.md)** for the homebrew's options and use, and
**[the merger's README](totk-mod-merger-plugin/README.md)** for the merger plugin's.
See **[the console pack's notice](packs/readme-switch.md)** for installing and using that pack, and
**[the emulator pack's notice](packs/readme-emulator.md)** for the other one.

## Note

See **[CREDITS.md](CREDITS.md)** for everything borrowed, ported or used as a reference, and for
the licences of third-party code.

None of the game's files are in this repository.
