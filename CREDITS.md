# Credits and references

*[Version française](CREDITS.fr.md)*

This repository only exists thanks to other people's work: a code loader, an interface library, a
mod merger, toolchains and a lot of published reverse engineering. This page says what comes from
elsewhere, under which licence, and what served as a reference.

The rest — the merger in Rust, the homebrew, the Skyline adaptations, the example plugins — was
written here.

## Code taken or adapted

| Project | Licence | What it brings |
|---|---|---|
| [Skyline](https://github.com/skyline-dev/skyline) (The Skyline Project) | MIT | The loader: [`skyline-totk/`](skyline-totk/) is an adaptation of it to TotK (the game's NPDM, a log on the SD card, deferred initialisation, the `totk_*` ABI). The differences are listed in its [README](skyline-totk/README.md). |
| [libeiffel](https://github.com/skyline-dev/libeiffel) (skyline-dev) | MIT | nnSdk utilities, in [`skyline-totk/libs/libeiffel`](skyline-totk/libs/libeiffel). |
| [And64InlineHook](https://github.com/Rprop/And64InlineHook) (Rprop) | MIT | The AArch64 inline hooks every plugin depends on, in `skyline-totk/source/skyline/inlinehook/`. |
| [borealis](https://github.com/xfangfang/borealis) (xfangfang, after natinusala) | Apache-2.0 | The homebrew's interface, as a submodule in `totk-mod-manager/library/borealis` (pinned at `5f08b286`). It carries fmt (MIT), tinyxml2 (zlib), tweeny (MIT), nanovg (zlib), yoga (MIT), libromfs, glad/glfw and SDL for the PC builds, as well as the *Material Icons* fonts (Apache-2.0, Google) and the system's own. |

## Ports and ideas

| Project | Licence | What it is owed |
|---|---|---|
| [TKMM / TkSharp](https://github.com/TKMM-Team/Tkmm) (TKMM-Team) | MIT | The heart of the matter: [`sharedlibs/totk-merge`](sharedlibs/totk-merge) is a Rust port of `TkSharp.Merging` (merging BYML, SARC, RSTB, MSBT, GameDataList, code patches), and the `.tkcl` package format is theirs. [`utils/tkmm-oracle`](utils/tkmm-oracle) runs the real TkSharp — as a submodule in `utils/TkSharp`, pinned at `be46b9ad` — to check that both give the same result, file by file. [TKMM's documentation](https://tkmm.org/docs/settings) served as the reference for the settings and the mod options. |
| [ArcRopolis](https://github.com/Raytwo/arcropolis) (Raytwo) | — | The idea that a mod can bring its own plugin, which the manager loads after the merge. |
| [exlaunch](https://github.com/shadowninja108/exlaunch) (shadowninja108) | — | A reference on code injection and on the NPDM format on the Switch. |
| [TotK-graphic-plugin](https://github.com/cucholix/TotK-graphic-plugin) (cucholix) | — | An example of a Skyline plugin for TotK, useful to check ABI compatibility. |
| [nx-optimizer](https://github.com/MaxLastBreath/nx-optimizer) (MaxLastBreath) | — | A reference on the code patches (`.pchtxt`) applied to the game. |
| [SimpleModDownloader](https://github.com/PoloNX/SimpleModDownloader) (PoloNX) | GPL-3.0 | The homebrew that browses and installs GameBanana mods from the console, on borealis as well: the reference behind this manager's GameBanana tab (searching, sorting, pages, a mod's files, the MD5 check, extracting the archive). |
| [sys-clk](https://github.com/retronx-team/sys-clk) (RetroNX) | GPL-3.0 | Its IPC service, which the homebrew asks to raise the clocks during a merge and then to restore the overlay's settings. The protocol is re-implemented here (`totk-mod-manager/source/core/sysclk.cpp`): no sys-clk code is reused. |

## Toolchains

| Tool | Role |
|---|---|
| [devkitPro](https://devkitpro.org/) — devkitA64, libnx, `npdmtool`, portlibs | All the C/C++ code for the console. Its [`devkitpro/devkita64`](https://hub.docker.com/r/devkitpro/devkita64) image is the base of the one in the [`Dockerfile`](Dockerfile). |
| [cargo-skyline](https://crates.io/crates/cargo-skyline), [skyline-rs](https://github.com/ultimate-research/skyline-rs), [nnsdk-rs](https://github.com/ultimate-research/nnsdk-rs) (jam1garner, ultimate-research) | Rust for Skyline plugins: the `skyline-v3` toolchain, an adapted `std`, the nnSdk headers. |
| [linkle](https://github.com/MegatonHammer/linkle) (Megaton Hammer) | ELF → NRO conversion (devkitPro's own produces a zero `bss_size` here). |
| [Meson](https://mesonbuild.com/) and [ninja](https://ninja-build.org/) | This repository's orchestration, inside the container. |
| [.NET](https://dotnet.microsoft.com/) | The tool that compares against TKMM. |
| [Atmosphère](https://github.com/Atmosphere-NX/Atmosphere), [hbmenu](https://github.com/switchbrew/nx-hbmenu) | What runs all of this on a console. |
| [GameBanana](https://gamebanana.com/) | The mod catalogue totk-mod-manager browses and installs from, through its public API (apiv12). |
| [Ryujinx](https://ryujinx.org/) | The test bench on a PC. |
| [Docker](https://www.docker.com/) | The machine's only prerequisite: every toolchain above fits in one image. |

## Libraries

**Rust** (through cargo, see [`Cargo.lock`](Cargo.lock)): `ruzstd` and `miniz_oxide`/`adler` for
zstd and deflate without `std`, `hashbrown` and `foldhash` for the tables, `skyline`, `nnsdk` and
`libc-nnsdk` for reaching the console's system.

**C** (devkitPro portlibs, linked into the homebrew): libcurl, libarchive, zlib, bzip2, xz, lz4,
zstd, expat.

## Reverse engineering and formats

The game's file formats (SARC, BYML, MSBT, RSTB/RESTBL, BFLYT/BFLAN, dictionary ZSTD) have been
described publicly since *Breath of the Wild*; this repository builds on that documentation and,
for what is not in it, on reading the game's code, done here:

- [ZeldaMods](https://zeldamods.org/) and [oead](https://github.com/zeldamods/oead) (leoetlino):
  the reference on SARC, BYML, RSTB and the series' zstd compression;
- [Switch Toolbox](https://github.com/KillzXGaming/Switch-Toolbox) (KillzXGaming): the structure
  of the `bflyt`/`bflan` interface layouts;
- the TotK modding community's conventions for packages and code patches (`.pchtxt`).

The addresses hooked in the game's code (life gauges, the "life" component, interface text
functions) were found here, in the `main` of **TotK 1.2.1**; they are documented next to the code
that uses them ([`plugins/enemy-hp/src/game.rs`](plugins/enemy-hp/src/game.rs),
[`boss.rs`](plugins/enemy-hp/src/boss.rs), [`regen.rs`](plugins/enemy-hp/src/regen.rs)) and
checked at startup before anything is patched.

## The game

*The Legend of Zelda: Tears of the Kingdom*, the Nintendo Switch and Ryujinx belong to their
respective owners. This repository contains **no game file**: no assets, no keys, no `romfs`.
Everything built here reads the files of the copy the user owns, on their own console or in their
own dump.

## This repository's licence

The code written here follows the MIT licence of Skyline, which it derives from
([`skyline-totk/LICENSE`](skyline-totk/LICENSE)). What was taken keeps its own: MIT for Skyline,
libeiffel, And64InlineHook and TkSharp, Apache-2.0 for borealis, and their respective licences for
what borealis carries (fmt, tinyxml2, tweeny, nanovg, yoga, Material Icons…).

If a credit is missing or misattributed, it is a mistake: pointing it out is enough to fix it.
