//! Enemies around the player get their life back over time.
//!
//! "Around the player" is what the game itself decides: the enemies whose
//! health gauge it shows. Two gauges call in here, once per frame each:
//!
//! - the gauge over an ordinary enemy (those fighting Link or close to him —
//!   further away with `VisualizeLife`). At `0x12c7a48` the game has just
//!   fetched the enemy's life component (in `x0`), right before reading its
//!   life to draw the bar and the numbers; the hook is placed there;
//! - the big gauge of a boss or mini-boss, which `boss.rs` hooks and hands
//!   over when `regen_bosses` is on.
//!
//! Either way the life is topped up before the gauge reads it, so the bar and
//! the numbers show the regained life on the same frame. A dead enemy (0 life)
//! is never brought back, life never goes past the maximum, and regeneration
//! waits a few seconds after the enemy last lost life, so a fight is not
//! undone as it happens.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

use skyline::hooks::InlineCtx;

use crate::game::{self, Site};
use crate::{debug, debugging};

/// The ordinary enemy gauge, just after it fetched the life component. What
/// 1.2.1 has from 8 bytes before: `bl <life component>`, `cbz x0`,
/// `mov x21, x0` (replaced), `str w23, [sp, #8]`.
const ENEMY_GAUGE: Site = Site {
    offset: 0x12c7a48,
    expected: [
        0x5b, 0x58, 0xe0, 0x97, 0xc0, 0x06, 0x00, 0xb4, 0xf5, 0x03, 0x00, 0xaa, 0xf7, 0x0b, 0x00, 0xb9,
    ],
};

/// A gauge update after a pause (a menu, the map) must not heal for the whole
/// pause: time is counted by update, at most this much each.
const MAX_STEP: f32 = 0.25;
/// Enemies followed at once; more gauges than this are never on screen.
const TRACKED: usize = 64;
/// Lines at most saying why an enemy was skipped.
const MAX_SKIP_LINES: u32 = 20;

#[derive(Clone, Copy)]
pub struct Settings {
    /// Share of the maximum life regained per second, in percent.
    pub percent_per_second: f32,
    /// The same for a boss or mini-boss, which has far more life.
    pub boss_percent_per_second: f32,
    /// Seconds without losing life before regeneration starts.
    pub delay_seconds: f32,
    /// Bosses and mini-bosses too (the ones with the big gauge).
    pub bosses: bool,
}

/// Which gauge an update comes from, for the log.
#[derive(Clone, Copy)]
pub enum Gauge {
    Enemy,
    Boss,
}

impl Gauge {
    fn name(self) -> &'static str {
        match self {
            Gauge::Enemy => "enemy",
            Gauge::Boss => "boss",
        }
    }

    fn percent_per_second(self, settings: &Settings) -> f32 {
        match self {
            Gauge::Enemy => settings.percent_per_second,
            Gauge::Boss => settings.boss_percent_per_second,
        }
    }
}

#[derive(Clone, Copy)]
struct Enemy {
    life: usize,
    last_seen: i32,
    last_damage: f32,
    last_update: f32,
    /// Life earned but not handed out yet (less than one point).
    fraction: f32,
    last_log: f32,
}

const EMPTY: Enemy = Enemy {
    life: 0,
    last_seen: 0,
    last_damage: 0.0,
    last_update: 0.0,
    fraction: 0.0,
    last_log: 0.0,
};

/// The gauge code runs on the game's own thread, which must never block on a
/// std mutex (a contended one aborts nnSdk): a spin lock it is.
struct Table {
    busy: AtomicBool,
    enemies: core::cell::UnsafeCell<[Enemy; TRACKED]>,
}

unsafe impl Sync for Table {}

static TABLE: Table = Table {
    busy: AtomicBool::new(false),
    enemies: core::cell::UnsafeCell::new([EMPTY; TRACKED]),
};

static SETTINGS: OnceLock<Settings> = OnceLock::new();
static START: OnceLock<Instant> = OnceLock::new();
static SKIP_LINES: AtomicU32 = AtomicU32::new(0);
static FIRST_ENEMY_CALL: AtomicBool = AtomicBool::new(true);
static FIRST_BOSS_CALL: AtomicBool = AtomicBool::new(true);

fn now() -> f32 {
    START.get_or_init(Instant::now).elapsed().as_secs_f32()
}

fn debug_skip(gauge: Gauge, life: usize, why: &str) {
    if SKIP_LINES.fetch_add(1, Ordering::Relaxed) < MAX_SKIP_LINES {
        debug(&format!("regen: {} {:#x}: skipped, {}", gauge.name(), life, why));
    }
}

/// Turns regeneration on and hooks the ordinary enemy gauge. The boss gauge
/// is `boss.rs`'s. Returns why the hook was not placed, for the log.
pub fn install(settings: Settings) -> Result<(), String> {
    let _ = SETTINGS.set(settings);
    START.get_or_init(Instant::now);
    game::hook(&ENEMY_GAUGE, on_enemy_gauge)
}

/// Whether bosses regenerate (regeneration on, and `regen_bosses`).
pub fn bosses() -> bool {
    SETTINGS.get().is_some_and(|settings| settings.bosses)
}

/// Called for each shown enemy gauge, every frame, with the enemy's life
/// component in `x0`.
unsafe extern "C" fn on_enemy_gauge(ctx: &mut InlineCtx) {
    update(ctx.registers[0].x() as usize, Gauge::Enemy);
}

/// Tops up the life of the actor whose life component is `life`, if it is
/// its time. Called once per frame per shown gauge; gives back the life and
/// the maximum when it did raise it, so the caller can follow.
pub unsafe fn update(life: usize, gauge: Gauge) -> Option<(i32, i32)> {
    let Some(settings) = SETTINGS.get() else { return None };
    let first = match gauge {
        Gauge::Enemy => &FIRST_ENEMY_CALL,
        Gauge::Boss => &FIRST_BOSS_CALL,
    };
    if debugging() && first.swap(false, Ordering::Relaxed) {
        debug(&format!("regen: first {} gauge update seen", gauge.name()));
    }
    if life == 0 {
        return None;
    }
    let Some(cell) = game::current_life(life) else {
        if debugging() {
            debug_skip(gauge, life, "no current life");
        }
        return None;
    };
    let current = cell.load(Ordering::Acquire);
    let max = game::max_life(life);
    if current <= 0 || max <= 0 {
        if debugging() && max <= 0 {
            debug_skip(gauge, life, &format!("life {}/{}", current, max));
        }
        return None;
    }

    while TABLE.busy.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
        core::hint::spin_loop();
    }
    let enemies = &mut *TABLE.enemies.get();
    let time = now();

    // This enemy's entry, or the one updated longest ago.
    let index = match enemies.iter().position(|enemy| enemy.life == life) {
        Some(index) => index,
        None => {
            let oldest = (0..TRACKED)
                .min_by(|&a, &b| enemies[a].last_update.total_cmp(&enemies[b].last_update))
                .unwrap_or(0);
            enemies[oldest] = Enemy {
                life,
                last_seen: current,
                last_damage: time,
                last_update: time,
                fraction: 0.0,
                last_log: time,
            };
            if debugging() {
                debug(&format!("regen: {:#x}: new {}, {}/{}", life, gauge.name(), current, max));
            }
            oldest
        }
    };
    let enemy = &mut enemies[index];

    if current < enemy.last_seen {
        if debugging() {
            debug(&format!("regen: {:#x}: hit, {} -> {}/{}", life, enemy.last_seen, current, max));
        }
        enemy.last_damage = time;
        enemy.fraction = 0.0;
    }
    let step = (time - enemy.last_update).clamp(0.0, MAX_STEP);
    enemy.last_update = time;

    let mut life_now = current;
    if current < max && time - enemy.last_damage >= settings.delay_seconds {
        enemy.fraction += max as f32 * gauge.percent_per_second(settings) / 100.0 * step;
        let gained = enemy.fraction.floor();
        if gained >= 1.0 {
            let target = (current + gained as i32).min(max);
            // Lost to a hit landing right now: that hit wins, and the next
            // update sees it.
            if cell.compare_exchange(current, target, Ordering::AcqRel, Ordering::Acquire).is_ok() {
                enemy.fraction -= gained;
                life_now = target;
                if debugging() && time - enemy.last_log >= 1.0 {
                    enemy.last_log = time;
                    debug(&format!("regen: {:#x}: +{} -> {}/{}", life, target - current, target, max));
                }
            }
        }
    } else if current >= max {
        enemy.fraction = 0.0;
    }
    enemy.last_seen = life_now;

    TABLE.busy.store(false, Ordering::Release);
    (life_now > current).then_some((life_now, max))
}
