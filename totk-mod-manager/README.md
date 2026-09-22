# totk-mod-manager

*[Version française](README.fr.md)*

A mod manager **on the console** for Tears of the Kingdom, companion to
[totk-mod-merger-plugin](../totk-mod-merger-plugin): the homebrew equivalent of TKMM.

- enable / disable mods, manage their **priority** (the highest wins), pick the **options** of
  TKMM packages;
- **conflicts**: mods that replace the same file or change the same values are reported before
  applying (you can apply anyway), and listed on each mod's page;
- **mods with code**: a mod can provide its own Skyline plugin, loaded into the game (see below);
  the list says so, and its page lets you leave it out;
- **profiles** (named mod lists);
- **install from GameBanana** (search, sorting, pages, MD5 checking, zip/7z/rar archives or
  `.tkcl`);
- **Apply**: run the merge in the homebrew, without extracting the game onto the SD card, in
  **boost mode** (with sys-clk: CPU and memory at their maximum). The game then starts without
  merging again, and the last 10 merges stay **in cache** so you can come back to one of them
  without waiting;
- interface in **French** and in **English** (the console's language, or your choice).

The interface is [borealis](https://github.com/xfangfang/borealis) (deko3d). The merge is the
plugin's own code, compiled for the homebrew: the same result, and the plugin recognises the merge
done here (`using the previous merge` in its log).


|                                                 |                                                          |
| ------------------------------------------------- | ---------------------------------------------------------- |
| ![Mods](docs/mods.jpg)                          | ![A package's options](docs/options.jpg)                 |
| ![GameBanana](docs/gamebanana.jpg)              | ![A mod's page](docs/page-mod.jpg)                       |
| ![Profiles](docs/profils.jpg)                   | ![Apply](docs/appliquer.jpg)                             |
| ![Conflicts before applying](docs/conflits.jpg) | ![A mod's priority and conflicts](docs/conflits-mod.jpg) |
| ![Moving a mod](docs/deplacer.jpg)              | ![Settings](docs/parametres.jpg)                         |

*(screenshots under Ryujinx, whose fallback font spaces accented letters out)*

## Installation

The repository's packs already hold it, next to skyline-totk and the merger: copy
`release/pack-switch/SD/` to the root of the SD card, and the manager is in `switch/`. See
[docs/build.md](../docs/build.md).

By hand, or to update just the manager:

1. skyline-totk and totk-mod-merger-plugin installed (see their READMEs).
2. Copy `totk-mod-manager.nro` into `sd:/switch/`.

Under an emulator, take `totk-mod-manager-ryujinx.nro` instead (`release/pack-emulator/`): it is
the same program with the two ARMv9 instructions Ryujinx' JIT refuses replaced. The console build
crashes at startup there.

## Use


| Button    | Action                                                                            |
| ----------- | ----------------------------------------------------------------------------------- |
| **A**     | enable / disable the mod, open an item                                            |
| **Y**     | the mod's page: position, options, conflicts, description, removal                |
| **−**    | pick the mod up to move it:**↑ / ↓** move it up or down, **A** or **B** drop it |
| **L / R** | move the mod up / down one step                                                   |
| **X**     | **Apply** (everywhere)                                                            |
| **+**     | quit                                                                              |

A mod's page (**Y**) also lets you pick its position in the list directly.

Changes (enabling, order, options, profile) are saved to the SD card immediately. Until they are
applied, the Mods tab shows "Changes to apply"; if they never are, the plugin will merge when the
game starts, as before (or keep the previous merge if *Merge when the game starts if needed* is
turned off in the settings).

### Applying without a dump of the game: starting the manager "through the game"

Merging needs the game's original files. Rather than a 16 GB dump on the SD card, the manager
reads **the installed game's romfs**, which the console only allows to the program that *is* the
game:

1. on the HOME menu, **hold R while launching Tears of the Kingdom**;
2. the homebrew menu opens instead of the game (Atmosphère's "title override");
3. start TotK Mod Manager, then **X** to apply, then "Launch the game" (which starts TotK
   normally, with skyline-totk and the mods).

In that mode, Atmosphère runs the homebrew *under TotK's identity*: the filesystem then opens the
game's data to it (base game + update) as it would to the game itself
(`romfsMountFromCurrentProcess`), and it gets an application's full memory. Settings → "Launch
mode" says "Through the game (R)" when that is the case.

Other sources, tried in order when the manager was not started through the game:

- the game **paused in the background** (started, then HOME, the manager opened from the album):
  its romfs is read through it (`romfsMountDataStorageFromProgram`);
- a romfs extracted into `sd:/totk/romfs/` (useful under an emulator).

Otherwise, "Apply" explains what to do, and the merge is still done by the plugin when the game
starts.

### Boost mode

Merging depends a lot on the CPU and above all on memory (on a console, 3 mods and ~4,700 files:
1 min 15 s normally, 59 s in boost mode, 42 s with memory at 1600 MHz on top). Settings → *Boost
mode*, during the merge only:


| Mode                  | Effect                                                                                                                                                                                                                               |
| ----------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Off                   | nothing                                                                                                                                                                                                                              |
| Standard              | the system's own boost mode, the one used for game loading: CPU at 1785 MHz instead of 1020, GPU slowed down                                                                                                                         |
| **Maximum** (default) | Standard, plus, if[sys-clk](https://github.com/retronx-team/sys-clk) is running, its temporary overrides (the ones from its overlay): CPU and memory at the chosen frequencies, 1785 and 1600 MHz by default (the original maximums) |

With sys-clk, the frequencies offered are the ones it says it can set: higher values only appear
with an overclocking build. Overrides that existed before the merge (set in the overlay) are
restored at the end, and when the manager exits. If the manager is force-closed during a merge
(HOME → Close the software), the overrides stay until they are changed in the overlay. The manager
only looks for sys-clk when it is installed (`atmosphere/contents/00FF0000636C6BFF`). When the
Mods tab says "Applied", boost is not started.

### Mods with code

A mod can contain a `plugin.nro` (or several in a `plugins/` folder) next to its `romfs`, or even
be nothing else. The game's plugin loads them once the merge is served, in mod order: that is what
lets several code mods coexist, where a console has only one `exefs`, already taken by Skyline.

- The Mods tab shows "plugin" on those mods, and "Code (plugin)" for the ones with no file to
  merge.
- The mod's page (**Y**) lists the `.nro` it provides and lets you not load that code
  (`plugins = 0` in its `mod.ini`) while keeping its files.
- Settings → *Load the code mods provide* turns loading off for every mod (`mod_plugins` in
  `config.ini`) — to be used when a mod crashes the game.

These settings do not change the merge: they are read by the plugin when the game starts, so
there is nothing to apply. Writing such a plugin: see the merger's README (section *Mods with
code*) and its `plugins/enemy-hp` example.

### Conflicts between mods

After reading the mods and before merging, the engine compares what they change:

- **replaced file**: several mods provide a file that does not merge (a model, a texture, an AI…)
  and whose contents differ. Only the highest mod's version is used;
- **values**: several mods change the same value of a merged file (a parameter in a `.bgyml`, a
  row of RSDB or GameData, a text in a `.msbt`…) with different values. The highest mod's value is
  used. Additions (new rows, new list items) and changes to different values merge without a
  conflict.

When there are any, "Apply" summarises them (which mod wins over which, how many files and values)
and asks for **Cancel** or **Apply anyway**. Cancelling leaves the existing merge alone. The
warning can be turned off in the settings.

The list stays available: the Mods tab shows each mod's number of conflicts, and its page (**Y**)
details them, each marked "Winning" or "Overridden" according to the profile's current order, with
**A** for the full path, the order of the mods involved and the values at stake. Changing the
order updates those marks immediately; the list itself is the one from the last analysis (at every
merge, by the manager or by the plugin when the game starts, which also writes them to
`merger.log`). Texts are compared in the game's language.

### GameBanana

The same API as TKMM (apiv12, game 7617), 20 mods per page, with rated content filtered out. The
list is sorted by **most downloaded**, most liked, most viewed, newest or recently updated, and
the search starts at three characters. A mod's page shows its screenshots, its figures and its
files; the recommended file is labelled as such, and the ones GameBanana archived are listed after
it, labelled **Archived**.

Installing a file: download into `sd:/totk/downloads/`, MD5 check, extraction (libarchive), then a
search like TKMM's: a `.tkcl` first, otherwise the folders containing `romfs/`/`exefs/` (or a bare
romfs). When there are several (variants), the manager asks which one to install. The mod lands in
`sd:/totk/mods/<name>/` with a `mod.ini` (name, version, author, description, link) and its
thumbnail, and goes **to the top of the active profile**. Reinstalling the same mod updates it
while keeping its settings. The mod's `plugin.nro` files are installed with it. A warning points
out exefs code (`subsdk`, `main.npdm`…), which the merger ignores: such code has to be provided as
a plugin to be loaded.

### Settings

The tab, in order:

| Group | What is in it |
|---|---|
| **Merge** | *Merger enabled* · *Merge when the game starts if needed* · *Apply the mods' code patches* (`.ips`/`.pchtxt`) · *Load the code mods ship* · *Warn about conflicts before applying* · *Text languages* · *Merges kept in the cache* (1 to 50) and what that cache uses on the SD card |
| **Interface** | *Language / Langue*: the console's, or forced. It takes effect at the next start, which the manager offers to do right away |
| **Installation** | what is in place: the game and its version, skyline-totk's `exefs`, the merger plugin — each *Installed* or *Missing*. A warning appears when an `atmosphere/contents/0100F2C0115B6000/romfs` folder exists, because it slows every boot. Then *Started as*: the game (R), application, or applet (album) |
| **Maintenance** | *Clear the merge cache*: the kept merges (`sd:/totk/merged`), the changelogs (`cache`) and the downloads. Mods and profiles are kept |
| **About** | the version, and where the log is |
| **Boost mode** | *While merging* (off, standard, maximum), and, with sys-clk, the CPU and memory frequencies |

*Text languages* is worth a word: on *Auto*, only the language the game reads is merged — the
plugin records it at launch — which cuts the merge time of mods that change texts. *All* merges
every language.

These settings are written in two places: the ones the plugin also reads go into
`sd:/totk/config.ini` (merger enabled, merging at boot, code patches, mod plugins, text languages,
cache size), the ones that belong to the manager into `sd:/totk/manager.ini` (boost and its
frequencies, the conflict warning, the language).

### Files on the SD card

```
sd:/switch/totk-mod-manager.nro
sd:/totk/
├── config.ini          active profile and settings (shared with the plugin)
├── mods/<mod>/         one folder per mod: romfs/, exefs/ or .tkcl, mod.ini, thumbnail.jpg
│                      (and its plugin.nro, when it provides one)
├── profiles/<name>.ini one file per profile
├── merged/             the merges kept, shared by the plugin and the manager
├── cache/              changelogs of folder mods, conflicts.tsv (conflicts of the served merge)
├── locale.txt          the game's language, written down by the plugin
├── downloads/          downloads in progress (emptied after installation)
├── manager.ini         the manager's preferences (boost, sys-clk frequencies, warning, language)
└── manager.log         the manager's log
```

**Merge cache.** The last 10 merges stay on the SD card (Settings → *Merges kept in cache*, from 1
to 50): switching profile or re-enabling a mod serves a kept merge again immediately ("Merge found
in the cache"), and a new merge only writes the files no other has produced yet. Settings → *Merge
cache* shows the number of merges and the space taken. Details in the plugin's README.

The interface's language follows the console's (French for French and Canadian French, English
otherwise); Settings → "Langue / Language" forces it, from the next start.

On its first run, the manager creates the "Default" profile from the mods present (ordered by
their `priority`), moves `.tkcl` files dropped loose in `mods/` into folders (the old layout), and
adds the mods copied by hand since to the top of the active profile.

## Compiling

From the root of the repository, everything is in the container (see
[docs/build.md](../docs/build.md)):

```bash
./build.sh totk-mod-manager    # both .nro files, in output/manager/nro/
```

On its own, in a shell that has devkitPro, cmake and the nightly (`./build.sh shell`):

```bash
./build.sh                                   # totk-mod-manager.nro, in build/
./build.sh --ryujinx --build-dir /work/output/manager --out /tmp/nro
```

Requirements, should it ever be built outside the container: devkitPro with `switch-dev`, `cmake`,
`switch-glm`, `switch-curl`, `switch-libarchive`, the `nightly-2024-10-09` toolchain with
`rust-src`, and the borealis submodule in `library/borealis`.

`build.sh` first compiles the Rust core (`core/`) for `aarch64-unknown-none` (`-Zbuild-std`), then
the application with CMake/devkitA64. The Ryujinx flavour replaces a read of the ARMv9 register
`GCSPR_EL0` that libgcc's exception unwinder contains (devkitA64 16) and that Ryujinx' JIT
refuses; a real console never runs that path.

## Architecture

```
core/                     Rust core: a no_std static library, C API (include/totk_manager_core.h)
  src/lib.rs              state (JSON), profiles, installation, merging (totk-merge's engine)
  src/install.rs          analysing extracted archives, installing, migrating
  src/json.rs             the JSON the interface is handed, written without a library
  src/nx.rs               allocator (newlib's malloc) and panics
source/
  core/host.cpp           tkm_host_* functions: the core's file access (newlib, libnx fs)
  core/core.cpp           the C++ side of the core (structures)
  core/game.cpp           the game's romfs (title override, paused game, dump), version, launching
  net/                    libcurl (the system's TLS), the GameBanana API
  util/                   archives (libarchive), MD5, images in the background, threads, logging
  app/                    borealis screens
resources/                translations (fr, en-US), icon
library/borealis/         borealis (xfangfang)
```

The core is `totk-merge` built **without `std`**: its file access goes through the `tkm_host_*`
functions in `source/core/host.cpp`, and the plugin's `sd:/` paths are resolved to `sdmc:/`.
Profiles, `mod.ini`, `.tkcl` and merging therefore have a single implementation for both the
plugin and the homebrew.

### Libraries


|                        |                                                               |
| ------------------------ | --------------------------------------------------------------- |
| borealis (xfangfang)   | the interface, with its nlohmann::json                        |
| CMake                  | required by borealis;`build.sh` calls it                      |
| libcurl (devkitPro)    | built on the console's TLS service: the system's certificates |
| libarchive (devkitPro) | zip, 7z, rar                                                  |

## Tests

Under Ryujinx 1.3.3 (a keyboard standing in for a controller): migrating the old layout and
creating the profile, enabling, reordering, options (single/multiple choice), creating /
activating / deleting profiles, a GameBanana search and installing a real mod (a zip holding a
`.tkcl` with options), settings, **Apply** from `sd:/totk/romfs`, then starting the game:
`using the previous merge (79 files)` and the mods visible on the title screen.

Then, with two variants generated to contradict each other (a value of the rocket, a text of the
title screen): the conflicts window (4 conflicts), **Cancel** then **Apply anyway**, boost mode on
and off, the number of conflicts in the Mods tab, a mod's page (Winning / Overridden, the details
of one conflict), moving with **−** and **↑ / ↓**, picking the position from the mod's page,
settings, switching to English then back to automatic, quitting with **+**.

On the engine side, `cargo test -p totk-merge` covers detection (values, additions, replaced
nodes, mods agreeing with the winner, the file's format) and the `engine` integration test checks
it against real game files (a parameter of a `.bgyml`, a whole file, a text of a `.msbt`,
cancelling).

Merge cache: unit tests (eviction, shared files, an interrupted merge, a truncated file) and
integration tests (two sets of mods alternating, coming back from the cache, size 1). Under
Ryujinx, starting the game with the old `merged/` layout (removed, the merge redone into the
cache, files served from `store/`), then in the manager: Settings (boost mode without sys-clk,
cache size and usage), a mod disabled then applied (2 files written, 1 already there), re-enabled
then applied ("Merge found in the cache").

Mods with code: under Ryujinx, a mod that is nothing but `mod.ini` + `plugin.nro` is listed as
"Code (plugin)", loaded when the game starts after the merge, finds its folder again and reads the
**merged** files (with a mod setting the blue Bokoblin to 1 HP on top, its report says 1 instead of
72). On the engine side, `cargo test` covers finding the `.nro` files (the mod's folder,
`plugins/`, a mod one level lower, `plugins = 0`) and installing them from an archive.

Not tested (impossible under an emulator): sys-clk, launching through the game (title override)
and reading the console's romfs, launching the game from the manager, and memory in applet mode.
Checked on a console by the user: leaving for the homebrew menu and restarting after a language
change.

## Credits

What this homebrew owes to others — the repository's full list is in
[CREDITS.md](../CREDITS.md):

- [borealis](https://github.com/xfangfang/borealis) (xfangfang, after natinusala), Apache-2.0 —
  the whole interface, deko3d on the Switch, as a submodule in `library/borealis`. It carries
  nlohmann::json, which `net/gamebanana.cpp` uses, and the image decoding the thumbnails go
  through (nanovg/stb_image), as well as the *Material Icons* fonts (Apache-2.0, Google).
- [TKMM / TkSharp](https://github.com/TKMM-Team/Tkmm) (TKMM-Team), MIT — merging, the `.tkcl`
  package format and the settings this interface exposes. The Rust core is a port of
  `TkSharp.Merging`, shared with the merger plugin.
- [sys-clk](https://github.com/retronx-team/sys-clk) (RetroNX), GPL-3.0 — the IPC service the
  boost mode asks to raise the CPU and memory clocks during a merge. Its protocol is
  re-implemented in `source/core/sysclk.cpp`: no sys-clk code is reused here.
- [devkitPro](https://devkitpro.org/) — devkitA64, libnx and the portlibs the homebrew links
  against: libcurl (over the console's own TLS service), libarchive, zlib, bzip2, xz, lz4, zstd,
  expat.
- [SimpleModDownloader](https://github.com/PoloNX/SimpleModDownloader) (PoloNX), GPL-3.0 — the
  homebrew that browses and installs GameBanana mods from the console, on borealis as well: the
  reference behind this manager's GameBanana tab (searching, sorting, pages, a mod's files, the
  MD5 check, extracting the archive).
- [GameBanana](https://gamebanana.com/) — the mod catalogue the Browse tab reads through its
  public API (apiv12), and the authors of the mods installed from it.
- [Atmosphère](https://github.com/Atmosphere-NX/Atmosphere) — the title override (holding R) that
  lets the manager read the game's files without a dump, and
  [hbmenu](https://github.com/switchbrew/nx-hbmenu), which starts it.
