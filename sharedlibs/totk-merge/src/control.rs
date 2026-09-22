//! Handing the choice of mods over to another plugin.
//!
//! A plugin such as an online mode can take control of the mod list before
//! the merge starts: the mods on the SD card are then left out (unless it asks
//! for them), and only what it adds is merged. The merge waits until the
//! owner commits its list, releases control, or runs out of time.
//!
//! This module holds the state; the plugin exposes it through a C ABI
//! (`tkm_*` functions) so plugins in any language can use it.

use alloc::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::info;
use crate::mods::{self, ModKind, ModSpec, Plan};
use crate::prelude::*;

/// Version of the `tkm_*` C API. Bumped on incompatible changes.
pub const API_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Phase {
    /// Mods can still be chosen.
    Waiting = 0,
    Merging = 1,
    /// Merged files are being served.
    Done = 2,
    /// Nothing is served (merge failed, or disabled).
    Failed = 3,
}

pub type MergedCallback = extern "C" fn(phase: i32, served_files: u32, user: *mut core::ffi::c_void);

struct Owner {
    name: String,
    token: u64,
    timeout: Duration,
    taken_at: Instant,
}

struct ApiMod {
    path: String,
    name: Option<String>,
    options: BTreeMap<String, Vec<String>>,
}

struct State {
    owner: Option<Owner>,
    committed: bool,
    local_mods: bool,
    mods: Vec<ApiMod>,
    merged_dir: Option<String>,
    phase: Phase,
    served: u32,
    callbacks: Vec<(MergedCallback, usize)>,
    counter: u64,
}

static STATE: Mutex<State> = Mutex::new(State {
    owner: None,
    committed: false,
    local_mods: true,
    mods: Vec::new(),
    merged_dir: None,
    phase: Phase::Waiting,
    served: 0,
    callbacks: Vec::new(),
    counter: 0,
});

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Runs `f` if `token` is the current owner's and mods can still be changed.
fn with_owner<T>(token: u64, f: impl FnOnce(&mut State) -> T) -> Option<T> {
    let mut state = state();
    let owns = state.phase == Phase::Waiting && state.owner.as_ref().map_or(false, |o| o.token == token && token != 0);
    owns.then(|| f(&mut state))
}

/// Takes control of the mod list. Returns a token for the other calls, or 0
/// when another plugin already has control or the merge has started.
pub fn take_control(owner: &str, timeout_ms: u32) -> u64 {
    let mut state = state();
    if state.phase != Phase::Waiting {
        return 0;
    }
    if let Some(current) = &state.owner {
        info!("'{}' asked for control of the mod list, but '{}' has it", owner, current.name);
        return 0;
    }

    state.counter += 1;
    let seed = Instant::now().elapsed().as_nanos() as u64 ^ (&state.counter as *const u64 as u64);
    let token = (seed.rotate_left(17) ^ 0x9E37_79B9_7F4A_7C15u64.wrapping_mul(state.counter)) | 1;

    info!("'{}' took control of the mod list", owner);
    state.owner = Some(Owner {
        name: owner.to_string(),
        token,
        timeout: Duration::from_millis(timeout_ms as u64),
        taken_at: Instant::now(),
    });
    state.committed = false;
    state.local_mods = false;
    state.mods.clear();
    token
}

/// Gives control back: the mods on the SD card are merged as usual.
pub fn release_control(token: u64) -> bool {
    with_owner(token, |state| {
        if let Some(owner) = state.owner.take() {
            info!("'{}' released control of the mod list", owner.name);
        }
        state.committed = false;
        state.local_mods = true;
        state.mods.clear();
        state.merged_dir = None;
    })
    .is_some()
}

pub fn set_local_mods_enabled(token: u64, enabled: bool) -> bool {
    with_owner(token, |state| state.local_mods = enabled).is_some()
}

pub fn clear_mods(token: u64) -> bool {
    with_owner(token, |state| state.mods.clear()).is_some()
}

/// Adds a mod (a `.tkcl`, a folder with `romfs`, or a romfs root). Mods merge
/// in the order they are added, above the SD card's mods. Returns its index,
/// or -1.
pub fn add_mod(token: u64, path: &str, name: Option<&str>) -> i32 {
    with_owner(token, |state| {
        state.mods.push(ApiMod {
            path: path.to_string(),
            name: name.map(str::to_string),
            options: BTreeMap::new(),
        });
        state.mods.len() as i32 - 1
    })
    .unwrap_or(-1)
}

/// Selects an option of a package added with [`add_mod`]. Groups not
/// mentioned keep the package's defaults.
pub fn select_option(token: u64, index: i32, group: &str, option: &str) -> bool {
    with_owner(token, |state| match state.mods.get_mut(index as usize) {
        Some(entry) if index >= 0 => {
            entry.options.entry(group.to_string()).or_default().push(option.to_string());
            true
        }
        _ => false,
    })
    .unwrap_or(false)
}

/// Where this owner's merge goes, so it does not overwrite (and invalidate)
/// the merge of the SD card's mods.
pub fn set_merged_dir(token: u64, dir: &str) -> bool {
    with_owner(token, |state| state.merged_dir = Some(dir.to_string())).is_some()
}

/// The mod list is final: the merge may start.
pub fn commit(token: u64) -> bool {
    with_owner(token, |state| state.committed = true).is_some()
}

pub fn phase() -> Phase {
    state().phase
}

pub fn served_files() -> u32 {
    state().served
}

/// Calls `callback` once the merge is over (right away if it already is).
pub fn on_merged(callback: MergedCallback, user: *mut core::ffi::c_void) {
    let mut state = state();
    match state.phase {
        Phase::Done | Phase::Failed => {
            let (phase, served) = (state.phase as i32, state.served);
            drop(state);
            callback(phase, served, user);
        }
        _ => state.callbacks.push((callback, user as usize)),
    }
}

/// Blocks until the mod list is settled, then returns it.
pub fn wait_for_plan(config: &Config) -> Plan {
    let mut logged = false;
    loop {
        {
            let state = state();
            let Some(owner) = &state.owner else {
                break;
            };
            if state.committed {
                break;
            }
            let timeout = owner.timeout.min(Duration::from_millis(config.control_timeout_ms as u64));
            if owner.taken_at.elapsed() >= timeout {
                info!(
                    "'{}' did not commit its mod list within {} ms, merging what it added so far",
                    owner.name,
                    timeout.as_millis()
                );
                break;
            }
            if !logged {
                info!("waiting for '{}' to choose the mods", owner.name);
                logged = true;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    let mut state = state();
    state.phase = Phase::Merging;

    // The SD card's mods are only looked at when they are merged.
    let (local_profile, mut specs) = if state.local_mods {
        let local = mods::local_plan(config);
        (local.profile, local.mods)
    } else {
        ("local".to_string(), Vec::new())
    };
    let profile = state.owner.as_ref().map(|o| o.name.clone()).unwrap_or(local_profile);

    for (index, api_mod) in state.mods.iter().enumerate() {
        let Some((kind, content)) = mods::classify(&api_mod.path) else {
            info!("'{}' added {}, which is not a mod", profile, api_mod.path);
            continue;
        };
        let name = api_mod
            .name
            .clone()
            .unwrap_or_else(|| crate::sys::path::file_name(&api_mod.path).to_string());
        // A mod handed over by a plugin brings its own plugins along, from the
        // folder it lives in.
        let root = match kind {
            ModKind::Package => crate::sys::path::parent(&content).map(str::to_string).unwrap_or_else(|| content.clone()),
            _ => content.clone(),
        };
        specs.push(ModSpec {
            name,
            kind,
            path: content,
            priority: i32::MAX / 2 + index as i32,
            options: api_mod.options.clone(),
            plugins: mods::plugin_files(&root),
        });
    }

    let merged_dir = match (&state.merged_dir, &state.owner) {
        (Some(dir), _) => dir.clone(),
        (None, Some(owner)) => {
            let safe: String = owner
                .name
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                .collect();
            format!("{}-{}", config.merged_dir.trim_end_matches('/'), safe)
        }
        (None, None) => config.merged_dir.clone(),
    };

    Plan {
        mods: specs,
        merged_dir,
        profile,
    }
}

/// Records the end of the merge and notifies whoever asked.
pub fn finish(phase: Phase, served: u32) {
    let callbacks = {
        let mut state = state();
        state.phase = phase;
        state.served = served;
        std::mem::take(&mut state.callbacks)
    };
    for (callback, user) in callbacks {
        callback(phase as i32, served, user as *mut core::ffi::c_void);
    }
}

#[cfg(test)]
pub(crate) fn reset_for_tests() {
    let mut state = state();
    state.owner = None;
    state.committed = false;
    state.local_mods = true;
    state.mods.clear();
    state.merged_dir = None;
    state.phase = Phase::Waiting;
    state.served = 0;
    state.callbacks.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_owner_at_a_time() {
        reset_for_tests();
        let token = take_control("online", 1000);
        assert_ne!(token, 0);
        assert_eq!(take_control("other", 1000), 0);
        assert!(!commit(token ^ 2));
        assert_eq!(add_mod(token, "sd:/totk/online/pack.tkcl", Some("Pack")), 0);
        assert!(select_option(token, 0, "Group", "Option"));
        assert!(!select_option(token, 3, "Group", "Option"));
        assert!(commit(token));

        let mut config = Config::default();
        config.mods_dir = "/nonexistent".into();
        config.use_romfslite = false;
        let plan = wait_for_plan(&config);
        assert_eq!(plan.profile, "online");
        assert_eq!(plan.merged_dir, "sd:/totk/merged-online");
        // The path does not exist, so the mod is reported and left out.
        assert!(plan.mods.is_empty());
        // Too late to change anything now.
        assert_eq!(add_mod(token, "x", None), -1);

        finish(Phase::Done, 3);
        assert_eq!(phase(), Phase::Done);
        reset_for_tests();
    }
}
