# skyline-totk

*[Version française](README.fr.md)*

[Skyline](https://github.com/skyline-dev/skyline) adapted to **The Legend of Zelda: Tears of the
Kingdom** (`0100F2C0115B6000`).

Skyline is a code loading environment: it injects itself into the game as a `subsdk9`, installs
hooks, and loads plugins (`.nro`) from the SD card — typically plugins written in Rust with
[`cargo-skyline`](https://crates.io/crates/cargo-skyline), or in C++.

## What changed from the original Skyline

| Topic | Skyline (SSBU) | skyline-totk |
|---|---|---|
| NPDM | `cross.npdm` frozen for Smash | generated from TotK's `main.npdm` (`npdm/totk.json`), wider permissions |
| Init | everything at module load (logger, sockets, a 6 MB network pool) | nothing allocates before the romfs is mounted; only the hooks are installed at startup |
| Logger | kernel + TCP, hard-coded | configurable (`kernel`, `sd`, `tcp`) through `config.ini`, SD by default |
| Threads | core 3, hard-coded | the process' default core (TotK only allows 0-2 — the old code failed) |
| Build | `CROSSVER`, Smash IPS patches | a single build, output ready to copy to an SD card |
| Plugin ABI | — | adds `totk_get_version`, `totk_get_version_string`, `totk_get_rom_mount` |

## Requirements

- **Console**: Atmosphère with *sigpatches* (the custom `main.npdm` is only accepted if ACID
  signature checking is disabled in the loader).
- **Build**: devkitPro (`devkitA64`, `libnx`, `npdmtool`) and Python 3 — all of it is in the
  repository's image, which builds this directory with `./build.sh skyline-totk` at the root
  (see [docs/build.md](../docs/build.md)).

## Compiling

From the root of the repository, in the container:

```bash
./build.sh skyline-totk
```

On its own, in a shell that has devkitPro (`./build.sh shell` at the root), with the `build.sh`
of this directory:

```bash
cd skyline-totk
./build.sh package OUT_ROOT=/work/output/skyline
```

`build.sh` sets `DEVKITPRO` and calls `make`. Without `OUT_ROOT`, everything is written next to
the sources (`build/`, `out/`); with it, nothing is. The result lands in `$OUT_ROOT/out/`:

```
out/atmosphere/contents/0100F2C0115B6000/
├── exefs/
│   ├── main.npdm     <- the game's NPDM + Skyline's permissions
│   └── subsdk9       <- Skyline
└── skyline/plugins/   <- drop your .nro plugins here
```

Plugins are looked for in `sd:/atmosphere/contents/0100F2C0115B6000/skyline/plugins/`, then (for
compatibility) in `romfs:/skyline/plugins/`. Prefer the first: as soon as a `romfs` folder exists
for the title, Atmosphère builds a LayeredFS romfs over TotK's ~300,000 files at every boot, which
is slow and can keep the game from starting (fs.mitm memory) on recent firmwares. The log points
out plugins still loaded from the romfs.

Copy the `out/atmosphere` folder to the root of the SD card (it merges with what is there).

Other targets:

```bash
./build.sh clean
./build.sh install SD=/d            # copies straight onto a mounted SD card
./build.sh dump-npdm GAME_NPDM=/path/to/exefs/main.npdm   # regenerates npdm/totk.json
```

## Configuration (optional)

The file `sd:/skyline/totk/config.ini`:

```ini
# Log outputs: none | kernel | sd | tcp (combinable: "sd,tcp"), or "all"
log = sd
log_path = sd:/skyline/totk/skyline.log
tcp_port = 6969
# Load the plugins
plugins = 1
# The plugin folder on the SD card (the romfs is looked at afterwards)
plugins_dir = sd:/atmosphere/contents/0100F2C0115B6000/skyline/plugins
```

Without the file, the values above apply as they are. The SD log is rewritten at every boot. With
`tcp`, Skyline takes over the game's network stack (`nn::socket`): only turn that on for
debugging, with `cargo skyline listen <ip>` on the other end.

## How it starts

1. Atmosphère's loader loads `subsdk9` alongside the game's NSOs, with the `main.npdm` provided.
2. `rtld` resolves our `nn::*` imports against the game's modules (`main`, `sdk`), then calls our
   `DT_INIT` (`__custom_init` → `skyline_init`).
3. At that point the game's allocator is not guaranteed to be ready: we limit ourselves to
   getting our process handle, initialising the hook engine (which goes through the JIT svcs, not
   the heap) and installing two hooks — `nn::fs::MountRom` and `nn::ro::Initialize`.
4. When the game mounts its romfs, the hook starts a worker thread that mounts the SD card, reads
   the configuration, starts the logger, detects the game's version, then loads the plugins
   through `nn::ro`. The calling thread waits for it, so the plugins are ready before the game
   reads a single file.
5. Once every `main` has run, the callbacks registered with `skyline_totk_on_plugins_loaded` are
   called — always before the game resumes.

## Writing a plugin

Any standard Skyline plugin works:

```bash
cargo skyline new my-plugin
# then drop the .nro in atmosphere/contents/0100F2C0115B6000/skyline/plugins/
```

On top of the usual Skyline ABI (`A64HookFunction`, `A64InlineHook`, `sky_memcpy`,
`getRegionAddress`, `get_program_id`, `skyline_tcp_send_raw`), this fork exports:

```c
uint32_t    totk_get_version();         // 10201 for 1.2.1
const char* totk_get_version_string();  // "1.2.1"
const char* totk_get_rom_mount();       // "content:/" on TotK

// Scratch memory taken from the kernel (svcMapPhysicalMemory), not from the game's heap
void* totk_map_memory(uint64_t size);
bool  totk_unmap_memory(void* address, uint64_t size);

// Calls callback(user) once every plugin has run its main (immediately if that is already the
// case, and then returns false). It lets a plugin wait until the others have had a chance to use
// it — that is how totk-mod-merger lets another plugin pick the mods.
bool skyline_totk_on_plugins_loaded(void (*callback)(void*), void* user);

// Brings up the network stack (nn::socket) for plugins that need it right at startup, to
// download a mod pack for instance. Idempotent.
bool skyline_totk_init_sockets();
```

**Why `totk_map_memory`**: when the plugins start, TotK's allocator only has a few megabytes
available (measured on 1.2.1: buffers of 1, 2 and 4 MiB go through, 8 MiB does not). A plugin
handling large files has to take its memory elsewhere — these two functions map it from the
kernel, into the process' *alias* region.

Two compatibility symbols are exported as well, `__nnmusl_ErrnoLocation` and `__pthread_join`:
`libc-nnsdk` (used by Rust plugins) asks for them, but TotK's SDK 15.3.1 no longer exports them
under those names. Without them, a Rust plugin that touches `errno` or threads does not load.

See `totk-mod-merger-plugin/` for a complete example, and its `plugins/online-example` for a
plugin that uses the network and another plugin's API.

Known traps for Rust plugins on this SDK:

- never drop a thread's `JoinHandle`: that calls `pthread_detach`, which crashes nnSdk (join the
  thread, or `std::mem::forget` the handle);
- no `std::sync::Mutex` in a hook called by several of the game's threads: on contention, nnSdk
  aborts the process (`ArbitrateLock` → invalid handle). A spinlock is fine;
- thread stacks are taken from the game's heap: 4 MiB can be refused, 2 MiB goes through.

## State

Checked on **TotK 1.2.1** (under Ryujinx, which applies exefs mods the way a console does):
`subsdk9` and the `main.npdm` are accepted, the hooks install, the SD card is mounted, the game's
version is detected (`1.2.1`), the plugin is loaded through `nn::ro` and run, then the game starts
normally. The matching lines in `sd:/skyline/totk/skyline.log`:

```
[SdLogger] Logger initialized.
[skyline-totk] Tears of the Kingdom 1.2.1 (code 10201)
[skyline-totk] romfs mounted at 'content:/'
[PluginManager] Loaded 'sd:/atmosphere/contents/0100F2C0115B6000/skyline/plugins/totk-mod-merger-plugin.nro'
[PluginManager] Running plugins-loaded callback 0
```

Testing on real hardware is still to be done (Atmosphère + sigpatches).

## Checking a new version of the game

The `nn::*` symbols are resolved dynamically: nothing is hard-coded, so a game update does not
break Skyline as long as the symbols exist. To check after an update:

```bash
# 1. extract the update's exefs
nstool -k prod.keys --tik <ticket> -x /0 exefs <program.nca>
# 2. regenerate the NPDM
./build.sh dump-npdm GAME_NPDM=exefs/main.npdm && ./build.sh
# 3. compare the symbols subsdk9 imports against those the game exports
python3 scripts/check_symbols.py exefs skyline-totk.elf
```

## Credits

- [Skyline](https://github.com/skyline-dev/skyline) — the base of the project
- [exlaunch](https://github.com/shadowninja108/exlaunch) — a reference for modern injection
- [cucholix/TotK-graphic-plugin](https://github.com/cucholix/TotK-graphic-plugin) — the first
  adaptation of Skyline to TotK
