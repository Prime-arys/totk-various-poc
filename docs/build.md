# Building

*[Version française](build.fr.md)*

Everything is built in a container. The machine needs nothing but **Docker** (Docker Desktop on
Windows and macOS, the docker engine on Linux) and **git** for the clone; nothing else is
installed, and nothing else is used — the machine's own toolchains, if it has any, play no part.

```bash
git clone <this repository> && cd totk-various-poc
./build.sh                  # or, from PowerShell: .\build.ps1
```

The first run does three things: fetch the submodules, build the image (a few minutes, once),
then build the two packs.

## The two directories

| Directory | What it holds |
|---|---|
| `output/` | everything transient. `output/build` is the Meson directory, `output/cargo` cargo's output, `output/skyline` the object files and the `.nso` of skyline-totk, `output/manager` cmake + borealis + the Rust core, `output/dotnet` the .NET output of the packager and of TkSharp, `output/tmp` the scratch files. |
| `release/` | what is finished: `release/pack-switch`, `release/pack-emulator`, `release/plugins/*.nro`, `release/mods/<mod>/`. |

No other directory of the repository is written to. That is what makes cleaning obvious:

```bash
./build.sh clean            # empties output/
./build.sh clean --all      # empties output/ and release/
```

Nothing else is removed: the sources, the submodules and the Docker image stay. For the image:
`docker image rm totk-various-poc`.

## The targets

```bash
./build.sh                          # packs: both of them, in release/
./build.sh pack-switch              # release/pack-switch/
./build.sh pack-emulator            # release/pack-emulator/
./build.sh plugins                  # release/plugins/{enemy-hp,online-example}.nro
./build.sh --romfs <folder> mod     # release/mods/EnemyHp/
./build.sh test                     # the tests of the crates, on the PC
./build.sh doctor                   # what the image holds
./build.sh shell                    # a shell inside the container
```

Each name is a Meson target: whatever follows `build.sh` is passed on as is, so
`./build.sh skyline-totk` or `./build.sh totk-mod-manager` build a single piece. The full list is
printed at the end of `meson setup`, in the summary.

The options:

| Option (bash) | Option (PowerShell) | Effect |
|---|---|---|
| `--romfs <folder>` | `-Romfs <folder>` | an extracted TotK romfs, mounted read-only, needed to build a mod |
| `--zip` | `-Zip` | also writes a `.zip` next to each pack |
| `--title-id <id>` | `-TitleId <id>` | another title id than TotK's |
| `--rebuild-image` | `-RebuildImage` | builds the image again (after a change to the `Dockerfile`) |
| `--no-dotnet` | `-NoDotnet` | with the previous one: an image without the .NET SDK, ~1 GiB less, but no more `.tkcl` |
| `-- <command>` | `-Run '<command>'` | runs something else in the container |

The options passed on to Meson (`--romfs`, `--zip`, `--title-id`) are remembered in
`output/build`: once given, they stay until they are changed or until a clean.

## What each target does

| Target | What runs | Where it works |
|---|---|---|
| `skyline-totk` | skyline-totk's `Makefile`, devkitA64, `npdmtool`, `elf2nso` | `output/skyline` |
| `totk-mod-merger-plugin.nro` | cargo with the `skyline-v3` toolchain, then `linkle` | `output/cargo` |
| `totk-mod-manager` | cargo (the `no_std` core), then cmake + make (borealis, deko3d) | `output/manager` |
| `enemy-hp.nro`, `online-example.nro` | as for the merger | `output/cargo` |
| `mod` | `totk-merge`'s examples read the romfs, `tkmm-oracle` packages the `.tkcl` | `output/dotnet`, `output/tmp` |
| `pack-switch`, `pack-emulator` | laying out the files already built | writes into `release/` |

Meson compiles nothing itself: it knows the targets, their order and the layout of a pack, and
calls the scripts in [`utils/`](../utils). Those assume the container (devkitPro in
`/opt/devkitpro`, Rust in `/opt/rust`) — which is why they no longer have anything to look for a
shell, convert a path or find a writable temporary directory.

## The packs

```text
pack-switch/SD/                                   pack-emulator/Ryujinx/
├── atmosphere/contents/<game>/                   ├── mods/contents/<game>/skyline-totk/
│   ├── exefs/{subsdk9,main.npdm}                 │   └── exefs/{subsdk9,main.npdm}
│   └── skyline/plugins/                          └── sdcard/
│       └── totk-mod-merger-plugin.nro                ├── atmosphere/…/skyline/plugins/…nro
├── skyline/totk/config.ini                           ├── skyline/totk/config.ini
├── switch/totk-mod-manager.nro                       ├── switch/totk-mod-manager-ryujinx.nro
└── totk/{config.ini, mods/}                          └── totk/{config.ini, mods/}
```

Each pack is rebuilt from scratch and carries a notice (`README.md`, and `LISEZMOI.md` in French)
and a `contenu.txt` listing its files with their size and MD5 fingerprint — handy to check what
was installed when a run goes wrong.

The emulator pack carries the Ryujinx flavour of the homebrew: the same program, with the two
ARMv9 instructions its JIT refuses replaced (see `totk-mod-manager/build.sh`).

**No mod is in a pack**: they are built separately ([mods.md](mods.md)) and dropped into
`totk/mods/`.

## The image

| Tool | What for |
|---|---|
| devkitPro (`devkitpro/devkita64`) + `switch-dev`, `uam` | `skyline-totk` and the homebrew |
| `switch-glm`, `switch-curl`, `switch-libarchive` | the homebrew's libraries |
| cmake, ninja, make, meson, python3, git | the build itself |
| rustup + the nightly of the manager's core | `totk-mod-manager/core` |
| `cargo-skyline` (the `skyline-v3` toolchain) and `linkle` | the Skyline plugins |
| .NET 10 SDK (optional) | `utils/tkmm-oracle`, the `.tkcl` packager |

The [`Dockerfile`](../Dockerfile) copies nothing in: the repository is mounted at `/work` when the
image runs. So it only changes when a toolchain does, and `./build.sh --rebuild-image` is enough
to apply that. `./build.sh doctor` says what the image holds, line by line.

The Skyline toolchain is rebuilt by `cargo skyline update-std` while the image is being built: it
is a Rust nightly plus `skyline-rs`' standard library. That step asks github, which rate-limits by
address; the `Dockerfile` retries every download with a growing wait, and if it still fails,
trying again later is enough.

On Linux and macOS, `build.sh` passes `--user $(id -u):$(id -g)` so the files produced belong to
whoever ran the command. On Windows, Docker Desktop takes care of it.

## Troubleshooting

| Symptom | Usual cause |
|---|---|
| `docker not found` | install Docker Desktop (Windows, macOS) or the docker engine (Linux) |
| `No space left on device` while `df` looks roomy | it is the Docker virtual machine's disk: `docker builder prune` and `docker image prune` free it |
| `retrying (n): cargo skyline update-std` while building the image, then a failure | github is rate-limiting the address; start again later |
| CMake cannot find borealis | the submodules are empty: `git submodule update --init` (`build.sh` does it by itself when it sees the empty directory) |
| `--romfs must point at an extracted TotK romfs` | `--romfs` missing, or the directory given does not exist |
| `error: the build left no ...tkmm-oracle.dll` | image built with `--no-dotnet`: `./build.sh --rebuild-image` |
| `cc1plus: ... Cannot allocate memory` while compiling borealis | too many compilers at once for the Docker virtual machine's memory. The script starts one per 1.5 GiB; to force it: `./build.sh -- bash -lc 'JOBS=4 meson compile -C output/build totk-mod-manager'`, or give Docker Desktop more memory |
| a `meson setup` that ignores an option | `meson setup` on an already configured directory exits without an error and ignores the options; `build.sh` knows this and goes through `meson configure` — but if you run Meson by hand, that is the trap to know |
| the command never returns although everything is built | a tool left a daemon behind that inherited the pipe ninja reads its command through, so ninja waits for an end-of-file that never comes. `dotnet build` did exactly that (four MSBuild nodes and the Roslyn server): `utils/build-enemy-hp-mod.sh` turns both off and sends the build's output to a file rather than a pipe |
| the homebrew crashes at startup under Ryujinx | the console build is being used: take `totk-mod-manager-ryujinx.nro` |

## Without `build.sh`

The launcher is only a shortcut; these two commands do the same thing:

```bash
docker build -t totk-various-poc .
docker run --rm -v "$PWD:/work" -w /work totk-various-poc \
    bash -lc 'meson setup output/build && meson compile -C output/build packs'
```

And in a shell inside the container (`./build.sh shell`), the scripts in `utils/` work on their
own:

```bash
utils/doctor.sh
utils/build-skyline.sh --out /tmp/exefs
utils/build-nro.sh totk-mod-merger-plugin --out /tmp/merger.nro
utils/build-manager.sh --out /tmp/manager
utils/build-enemy-hp-mod.sh --romfs /romfs --out /tmp/EnemyHp
```
