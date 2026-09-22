//! enemy-hp: the code half of the Enemy HP mod.
//!
//! The mod is an ordinary folder in `sd:/totk/mods`, with a `plugin.nro` next
//! to its `mod.ini`:
//!
//! ```text
//! sd:/totk/mods/EnemyHp/mod.ini
//! sd:/totk/mods/EnemyHp/EnemyHp.tkcl   <- the data (numbers over the gauge)
//! sd:/totk/mods/EnemyHp/plugin.nro     <- this
//! sd:/totk/mods/EnemyHp/enemy-hp.ini   <- settings, optional
//! sd:/totk/mods/EnemyHp/enemies.txt    <- optional, one actor name per line
//! ```
//!
//! totk-mod-merger loads it once the mods are merged. It does three things:
//!
//! - **regeneration** (`regen.rs`): enemies whose gauge is shown — the ones
//!   around the player — get their life back over time, after a few seconds
//!   without being hit; bosses and mini-bosses too, unless `regen_bosses = 0`;
//! - **the numbers on the boss gauge** (`boss.rs`), which the game has no code
//!   for;
//! - **a report** (`hp.rs`): the life of the game's enemies, read in the
//!   *merged* files (mods included), written to `enemy-hp.txt` at every boot.
//!
//! The numbers over an ordinary enemy's health bar are not this plugin's
//! doing: the game still writes them, and the mod's `.tkcl` supplies what it
//! lacks (the `VisualizeLife` armour effect, the text panes, the messages).
//! See `build-mod.sh` next to this crate. What the plugin knows of the game's
//! code is in `game.rs`.

mod boss;
mod game;
#[allow(dead_code)] // where a value is set only matters to the PC examples
mod hp;
mod regen;
mod scratch;

use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Instant;

use totk_formats::zstd::Zstd;

/// Where the mod lives when the merger is too old to say (this plugin still
/// works when it is dropped in `skyline/plugins` by hand).
const FALLBACK_DIR: &str = "sd:/totk/mods/EnemyHp";

/// Game files are megabytes: they cannot come from the game's own heap.
#[global_allocator]
static ALLOCATOR: scratch::Scratch = scratch::Scratch;

fn log(message: &str) {
    skyline::println!("[enemy-hp] {}\n", message);
}

/// `debug = 1` in `enemy-hp.ini`: the hooks say what they see and do.
static DEBUG: AtomicBool = AtomicBool::new(false);
static DEBUG_LINES: AtomicU32 = AtomicU32::new(0);
/// Most lines the diagnostics write, so a long session does not fill the log.
const MAX_DEBUG_LINES: u32 = 300;

pub(crate) fn debugging() -> bool {
    DEBUG.load(Ordering::Relaxed)
}

/// A line in `skyline.log` when `debug = 1`, up to [`MAX_DEBUG_LINES`].
pub(crate) fn debug(message: &str) {
    if debugging() && DEBUG_LINES.fetch_add(1, Ordering::Relaxed) < MAX_DEBUG_LINES {
        log(message);
    }
}

/// Where the game mounted its romfs.
fn rom_prefix() -> Option<String> {
    let mut address: usize = 0;
    let result = unsafe { skyline::nn::ro::LookupSymbol(&mut address, b"totk_get_rom_mount\0".as_ptr()) };
    if result == 0 && address != 0 {
        let getter: extern "C" fn() -> *const u8 = unsafe { core::mem::transmute(address) };
        let pointer = getter();
        if !pointer.is_null() {
            let prefix = unsafe { skyline::from_c_str(pointer) };
            if !prefix.is_empty() {
                return Some(prefix);
            }
        }
    }
    ["content:/", "rom:/", "romfs:/"]
        .into_iter()
        .find(|prefix| std::fs::metadata(format!("{}Pack/ZsDic.pack.zs", prefix)).is_ok())
        .map(str::to_string)
}

/// The enemies to report on: the mod's own list, or the default one.
fn enemies(dir: &str) -> Vec<String> {
    match std::fs::read_to_string(format!("{}/enemies.txt", dir)) {
        Ok(text) => {
            let list: Vec<String> = text
                .lines()
                .map(|line| line.split('#').next().unwrap_or("").trim().to_string())
                .filter(|line| !line.is_empty())
                .collect();
            if list.is_empty() {
                hp::DEFAULT_ENEMIES.iter().map(|a| a.to_string()).collect()
            } else {
                log(&format!("{} enemies listed in enemies.txt", list.len()));
                list
            }
        }
        Err(_) => hp::DEFAULT_ENEMIES.iter().map(|a| a.to_string()).collect(),
    }
}

/// `enemy-hp.ini` in the mod folder; every line is optional.
struct Settings {
    regen: Option<regen::Settings>,
    report: bool,
    debug: bool,
}

fn load_settings(dir: &str) -> Settings {
    let mut enabled = true;
    let mut regen = regen::Settings {
        percent_per_second: 2.0,
        boss_percent_per_second: f32::NAN, // = the same as the enemies, unless set
        delay_seconds: 3.0,
        bosses: true,
    };
    let mut report = true;
    let mut debug = false;
    if let Ok(text) = std::fs::read_to_string(format!("{}/enemy-hp.ini", dir)) {
        for line in text.lines() {
            let line = line.split(['#', ';']).next().unwrap_or("");
            let Some((key, value)) = line.split_once('=') else { continue };
            let (key, value) = (key.trim(), value.trim());
            let flag = matches!(value, "1" | "true" | "yes" | "on");
            // Fractions of a percent are worth having (a boss has thousands of
            // life points), written with a dot or a comma.
            let number = value.replace(',', ".").parse::<f32>();
            match key {
                "regen" => enabled = flag,
                "regen_percent" => regen.percent_per_second = number.unwrap_or(regen.percent_per_second),
                "regen_percent_bosses" => regen.boss_percent_per_second = number.unwrap_or(f32::NAN),
                "regen_delay" => regen.delay_seconds = number.unwrap_or(regen.delay_seconds),
                "regen_bosses" => regen.bosses = flag,
                "report" => report = flag,
                "debug" => debug = flag,
                _ => {}
            }
        }
    }
    regen.percent_per_second = regen.percent_per_second.clamp(0.0, 100.0);
    if regen.boss_percent_per_second.is_nan() {
        regen.boss_percent_per_second = regen.percent_per_second;
    }
    regen.boss_percent_per_second = regen.boss_percent_per_second.clamp(0.0, 100.0);
    regen.delay_seconds = regen.delay_seconds.max(0.0);
    let regenerates = regen.percent_per_second > 0.0 || (regen.bosses && regen.boss_percent_per_second > 0.0);
    Settings {
        regen: (enabled && regenerates).then_some(regen),
        report,
        debug,
    }
}

#[skyline::main(name = "enemy-hp")]
pub fn main() {
    let dir = totk_mod_merger_api::current_mod_dir().unwrap_or_else(|| FALLBACK_DIR.to_string());
    let mod_name = totk_mod_merger_api::current_mod_name().unwrap_or_else(|| "EnemyHp".to_string());
    log(&format!("'{}' loaded from {}", mod_name, dir));
    let settings = load_settings(&dir);
    DEBUG.store(settings.debug, Ordering::Relaxed);

    let mut bosses_regenerate = false;
    match settings.regen {
        Some(regen) => match regen::install(regen) {
            Ok(()) => {
                bosses_regenerate = regen.bosses;
                log(&format!(
                    "regeneration on: {}% of the maximum life per second, {}s after the last hit, {}{}",
                    regen.percent_per_second,
                    regen.delay_seconds,
                    match (regen.bosses, regen.boss_percent_per_second == regen.percent_per_second) {
                        (false, _) => "bosses left out".to_string(),
                        (true, true) => "bosses included".to_string(),
                        (true, false) => format!("bosses included at {}%", regen.boss_percent_per_second),
                    },
                    if settings.debug { " (debug log on)" } else { "" }
                ))
            }
            Err(why) => log(&format!("regeneration off: {}", why)),
        },
        None => log("regeneration off (enemy-hp.ini)"),
    }
    match boss::install() {
        Ok(()) => log(&format!(
            "boss gauges: life numbers with a VisualizeLife armour, regeneration {}",
            if bosses_regenerate { "on" } else { "off" }
        )),
        Err(why) => log(&format!("boss gauges left alone: {}", why)),
    }

    if settings.report {
        write_report(&dir);
    }
}

/// `enemy-hp.txt`: the life of the enemies, as the merged files give it.
fn write_report(dir: &str) {
    let Some(prefix) = rom_prefix() else {
        log("could not find the game's files, nothing to report");
        return;
    };
    let mut zstd = Zstd::new();
    match std::fs::read(format!("{}Pack/ZsDic.pack.zs", prefix))
        .map_err(|e| e.to_string())
        .and_then(|packed| zstd.decompress(&packed).map_err(|e| e.to_string()))
        .and_then(|dictionaries| zstd.load_dictionaries(&dictionaries).map_err(|e| e.to_string()))
    {
        Ok(_) => {}
        Err(error) => {
            log(&format!("could not read the compression dictionaries: {}", error));
            return;
        }
    }

    // Straight through nn::fs, so the merger's redirects apply: this reads the
    // merged packs, not the ones on the cartridge.
    let mut files = |relative: &str| -> Option<Vec<u8>> {
        let data = std::fs::read(format!("{}{}", prefix, relative)).ok()?;
        match Zstd::is_compressed(&data) {
            true => zstd.decompress(&data).ok(),
            false => Some(data),
        }
    };

    let started = Instant::now();
    let mut report = String::from("# Life of the game's enemies, as the merged files give it.\n");
    report.push_str("# Written by the enemy-hp plugin every time the game boots.\n");
    let mut found = 0;
    let actors = enemies(dir);
    for actor in &actors {
        match hp::max_life(&mut files, actor) {
            Some(life) => {
                report.push_str(&format!("{}\t{}\n", actor, life));
                found += 1;
            }
            None => report.push_str(&format!("{}\t-\n", actor)),
        }
    }

    let path = format!("{}/enemy-hp.txt", dir);
    match std::fs::File::create(&path).and_then(|mut file| file.write_all(report.as_bytes())) {
        Ok(()) => log(&format!(
            "{} of {} enemies read in {:.1}s, written to {}",
            found,
            actors.len(),
            started.elapsed().as_secs_f32(),
            path
        )),
        Err(error) => log(&format!("could not write {}: {}", path, error)),
    }

    // A line or two in the log, so a console with no PC attached still shows
    // the plugin ran and what it found.
    for line in report.lines().filter(|line| !line.starts_with('#')).take(3) {
        log(&line.replace('\t', ": "));
    }
}
