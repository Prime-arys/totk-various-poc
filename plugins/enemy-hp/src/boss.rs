//! The big gauge of bosses and mini-bosses (Hinox, Stone Talus, Molduga,
//! Gleeok, the temples' bosses...): the numbers under it, and regeneration.
//!
//! TotK 1.2.1 updates that gauge (its `UIBossLifeScreen`, `0x1ae2178`) once
//! per frame: it fetches the boss's life component and reads its current and
//! maximum life to fill the bar. A hook right after the fetch (`0x1ae22b0`,
//! life component in `x0`, the screen in `x19`) regenerates the boss when
//! `regen_bosses` is on, then writes "current/max" into the text pane
//! `T_BossLife_00` that the mod's layout adds under the bar. Unlike the
//! ordinary enemy gauge, the game has no code of its own for boss numbers.
//!
//! Like the enemy numbers, they only show while the armour worn has the
//! `VisualizeLife` effect (the Champion's Tunic, or any armour, depending on
//! the mod's option).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

use skyline::hooks::InlineCtx;

use crate::game::{self, Site};
use crate::regen::{self, Gauge};
use crate::{debug, debugging};

/// Right after the boss gauge fetched the life component. What 1.2.1 has from
/// 8 bytes before: `bl <life component>`, `cbz x0`, `mov x20, x0` (replaced),
/// `ldr x0, [sp, #16]`.
const BOSS_GAUGE: Site = Site {
    offset: 0x1ae22b0,
    expected: [
        0x41, 0xee, 0xbf, 0x97, 0xe0, 0x09, 0x00, 0xb4, 0xf4, 0x03, 0x00, 0xaa, 0xe0, 0x0b, 0x40, 0xf9,
    ],
};

/// The screen's layout, which the game hands to the same text function when
/// it writes the boss's name (`0x1ae2660`).
const LAYOUT: usize = 40;
/// The three animations that draw the bar, which the game moves together when
/// the gauge fills in (`0x1ae2228`). Each frame the game puts the first where
/// the life says, and lets the others catch up — but only while the "damage"
/// animation is over. Life given back changes the life every frame, which
/// keeps that animation going, so the bar would stay where the last hit left
/// it: the plugin moves the three itself instead.
const BAR_ANIMATIONS: [usize; 3] = [416, 424, 448];
/// Set when the gauge is a long one; the animation then covers the whole bar
/// instead of half of it (`0x1ae2394`).
const LONG_GAUGE: usize = 512;
/// In an animation: the resource it plays (its frame count is a u16 at +8),
/// the frame it is on, and `SetFrame` in its vtable.
const ANIMATION_RESOURCE: usize = 24;
const ANIMATION_FRAME: usize = 32;
const SET_FRAME: usize = 208;
/// The pane the mod's layout adds under the bar.
const PANE: &[u8] = b"T_BossLife_00\0";
/// A text written is written again after this long even if it did not change,
/// in case the layout was reset under it.
const REFRESH_SECONDS: f32 = 1.0;
/// Boss gauges on screen at once.
const SCREENS: usize = 4;

/// What was last written into a screen's pane.
#[derive(Clone, Copy)]
struct Shown {
    screen: usize,
    current: i32,
    max: i32,
    visible: bool,
    time: f32,
}

const NOTHING: Shown = Shown {
    screen: 0,
    current: 0,
    max: 0,
    visible: false,
    time: f32::MIN,
};

/// Only the game's UI thread comes here, but a spin lock costs nothing.
struct Screens {
    busy: AtomicBool,
    shown: core::cell::UnsafeCell<[Shown; SCREENS]>,
}

unsafe impl Sync for Screens {}

static SCREENS_SHOWN: Screens = Screens {
    busy: AtomicBool::new(false),
    shown: core::cell::UnsafeCell::new([NOTHING; SCREENS]),
};

static START: OnceLock<Instant> = OnceLock::new();
static FIRST_CALL: AtomicBool = AtomicBool::new(true);
static NO_PANE_SAID: AtomicBool = AtomicBool::new(false);

fn now() -> f32 {
    START.get_or_init(Instant::now).elapsed().as_secs_f32()
}

/// Hooks the boss gauge. Returns why not, for the log.
pub fn install() -> Result<(), String> {
    START.get_or_init(Instant::now);
    game::hook(&BOSS_GAUGE, on_boss_gauge)
}

unsafe extern "C" fn on_boss_gauge(ctx: &mut InlineCtx) {
    let life = ctx.registers[0].x() as usize;
    let screen = ctx.registers[19].x() as usize;
    if debugging() && FIRST_CALL.swap(false, Ordering::Relaxed) {
        debug(&format!("boss: first gauge update seen (screen {:#x}, life {:#x})", screen, life));
    }
    if life == 0 || screen == 0 {
        return;
    }
    if regen::bosses() {
        if let Some((current, max)) = regen::update(life, Gauge::Boss) {
            move_bar(screen, current as f32 / max as f32);
        }
    }
    show(screen, life);
}

/// Puts the bar where the life is, as the game does when the gauge fills in.
unsafe fn move_bar(screen: usize, ratio: f32) {
    let first = game::read::<usize>(screen + BAR_ANIMATIONS[0]);
    if first == 0 {
        return;
    }
    let resource = game::read::<usize>(first + ANIMATION_RESOURCE);
    if resource == 0 {
        return;
    }
    let frames = game::read::<u16>(resource + 8) as f32;
    let whole = game::read::<u8>(screen + LONG_GAUGE) != 0;
    let frame = ratio.clamp(0.0, 1.0) * frames * if whole { 1.0 } else { 0.5 };
    for offset in BAR_ANIMATIONS {
        let animation = game::read::<usize>(screen + offset);
        if animation == 0 || game::read::<f32>(animation + ANIMATION_FRAME) == frame {
            continue;
        }
        let vtable = game::read::<usize>(animation);
        let set_frame: extern "C" fn(usize, f32) = core::mem::transmute(game::read::<usize>(vtable + SET_FRAME));
        set_frame(animation, frame);
    }
}

/// Writes the boss's life under its gauge, when it changed.
unsafe fn show(screen: usize, life: usize) {
    let layout = game::read::<usize>(screen + LAYOUT);
    if layout == 0 {
        return;
    }
    let visible = game::visualize_life();
    let current = game::current_life(life).map_or(0, |cell| cell.load(Ordering::Acquire)).max(0);
    let max = game::max_life(life).max(0);

    while SCREENS_SHOWN.busy.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
        core::hint::spin_loop();
    }
    let shown = &mut *SCREENS_SHOWN.shown.get();
    let time = now();
    let index = shown
        .iter()
        .position(|entry| entry.screen == screen)
        .unwrap_or_else(|| (0..SCREENS).min_by(|&a, &b| shown[a].time.total_cmp(&shown[b].time)).unwrap_or(0));
    let last = shown[index];
    let same = last.screen == screen && last.visible == visible && (!visible || (last.current, last.max) == (current, max));
    if same && time - last.time < REFRESH_SECONDS {
        SCREENS_SHOWN.busy.store(false, Ordering::Release);
        return;
    }
    shown[index] = Shown {
        screen,
        current,
        max,
        visible,
        time,
    };
    SCREENS_SHOWN.busy.store(false, Ordering::Release);

    let mut buffer = [0u16; 32];
    let text = if visible && max > 0 { life_text(&mut buffer, current, max) } else { &buffer[..1] };
    let panes = game::set_pane_text(layout, PANE, text);

    if panes == 0 && !NO_PANE_SAID.swap(true, Ordering::Relaxed) {
        skyline::println!(
            "[enemy-hp] the boss gauge has no {} pane: numbers need the EnemyHp.tkcl of version 2.3 or later\n",
            core::str::from_utf8(&PANE[..PANE.len() - 1]).unwrap_or("?")
        );
    }
    if debugging() && (last.screen != screen || last.visible != visible) {
        debug(&format!(
            "boss: {:#x}: {}/{}, numbers {} ({} pane{})",
            life,
            current,
            max,
            if visible { "shown" } else { "hidden, no VisualizeLife armour" },
            panes,
            if panes == 1 { "" } else { "s" }
        ));
    }
}

/// "current/max" in UTF-16, with its terminator, in `buffer`.
fn life_text(buffer: &mut [u16; 32], current: i32, max: i32) -> &[u16] {
    let mut length = push_number(buffer, 0, current);
    buffer[length] = b'/' as u16;
    length = push_number(buffer, length + 1, max);
    buffer[length] = 0;
    &buffer[..length + 1]
}

/// Writes `value` (0 or more) at `at`, returns where it ends.
fn push_number(buffer: &mut [u16; 32], at: usize, value: i32) -> usize {
    let mut digits = [0u16; 10];
    let mut count = 0;
    let mut rest = value.max(0) as u32;
    loop {
        digits[count] = b'0' as u16 + (rest % 10) as u16;
        count += 1;
        rest /= 10;
        if rest == 0 {
            break;
        }
    }
    for (offset, digit) in digits[..count].iter().rev().enumerate() {
        buffer[at + offset] = *digit;
    }
    at + count
}
