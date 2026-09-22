//! Merge orchestration: loads every mod's changelog (reading `.tkcl` packages,
//! working out folder mods), merges them, and produces the redirect table the
//! nn::fs hooks serve.

use alloc::collections::BTreeMap;

use crate::builder::build_folder;
use crate::cache;
use crate::config::{self, Config, Locales};
use crate::conflicts::{self, ModChanges};
use crate::merge_cache::{locales_cover, merge_id, CachedMerge, MergeCache, Served, StoreWriter};
use crate::merger::{MergeOptions, Merger, Output};
use crate::mods::{content_fingerprint, Fingerprint, ModKind, ModSpec, Plan};
use crate::prelude::*;
use crate::rom::TkRom;
use crate::sys::{fs, path, time::Instant};
use crate::tkcl::{read_tkcl_file, ulid_to_string, Changelog, TkMod};
use crate::{info, progress, Stage};

pub use crate::merge_cache::Redirects;

#[derive(Debug, Default, Clone)]
pub struct Outcome {
    pub redirects: Redirects,
    /// (offset from the start of the main module, instruction bytes as a
    /// big endian value).
    pub patches: Vec<(u32, u32)>,
    pub failed: bool,
    /// Files merged this time (0 when a previous merge was reused).
    pub merged_files: usize,
    pub warnings: usize,
    /// A merge from the cache was served as it was.
    pub reused: bool,
    /// ... and it was not the last one made or used: it came back from the
    /// cache.
    pub from_cache: bool,
    /// Conflicts found between the mods (see [`conflicts`]).
    pub conflicts: usize,
    /// Cancelled by the conflict sink: the previous merge is left as it was.
    pub cancelled: bool,
    /// Why nothing is served, for whoever shows it to a user.
    pub error: Option<String>,
}

impl Outcome {
    fn failure(error: String) -> Outcome {
        info!("{}", error);
        Outcome {
            failed: true,
            error: Some(error),
            ..Outcome::default()
        }
    }
}

/// Writes merged files into the merge cache's store.
struct SdOutput {
    store: StoreWriter,
    redirects: Redirects,
}

impl Output for SdOutput {
    fn write(&mut self, relative: &str, data: &[u8]) -> Result<(), String> {
        let file = self.store.put(data).map_err(|e| e.to_string())?;
        self.redirects.insert(relative.to_string(), file);
        Ok(())
    }

    fn link(&mut self, relative: &str, target: &str) -> Result<(), String> {
        self.redirects.insert(relative.to_string(), path::normalize(target));
        Ok(())
    }
}

/// A loaded mod: packages keep their option data, folder mods have one
/// changelog.
enum Loaded {
    Package(TkMod, BTreeMap<String, Vec<String>>),
    Folder(Changelog),
}

/// Whether the merge cache holds a merge of these mods and settings, which the
/// game would be served without merging, checked without the game's files.
pub fn is_plan_merged(config: &Config, plan: &Plan) -> bool {
    if plan.mods.is_empty() {
        return true;
    }
    let stamp = plan_fingerprint(config, plan);
    let wanted = config.locales();
    MergeCache::new(&plan.merged_dir)
        .entries()
        .iter()
        .any(|merge| merge.plan == stamp && locales_cover(&merge.locales, &wanted))
}

/// The part of a merge's fingerprint that comes from the mods and settings.
/// Languages are not part of it: see [`locales_cover`].
pub fn plan_fingerprint(config: &Config, plan: &Plan) -> String {
    let mut fingerprint = Fingerprint::default();
    fingerprint.mix_str(&format!("builder {}", cache::BUILDER_VERSION));
    fingerprint.mix(&config.shop_param_limit.to_le_bytes());
    for spec in &plan.mods {
        fingerprint.mix_str(&spec.name);
        fingerprint.mix(&spec.priority.to_le_bytes());
        fingerprint.mix_str(&format!("{:?} {:?}", spec.kind, spec.options));
        fingerprint.mix_str(&content_fingerprint(spec));
    }
    format!("{}-{}", fingerprint.finish(), plan.mods.len())
}

pub struct Engine<'a> {
    config: &'a Config,
    rom_prefix: String,
}

impl<'a> Engine<'a> {
    pub fn new(config: &'a Config, rom_prefix: &str) -> Engine<'a> {
        Engine {
            config,
            rom_prefix: rom_prefix.to_string(),
        }
    }

    pub fn run(&self, plan: &Plan) -> Outcome {
        if plan.mods.is_empty() {
            info!("no mods to merge ({})", plan.profile);
            return Outcome::default();
        }

        let rom = match TkRom::open(&self.rom_prefix) {
            Ok(rom) => rom,
            Err(error) => return Outcome::failure(format!("could not read the game's files: {}", error)),
        };
        info!("game version {} ({})", rom.game_version, rom.nso_binary_id);

        let cache = MergeCache::new(&plan.merged_dir);
        cache.remove_old_layout();
        let wanted_locales = self.config.locales();
        let available_locales = rom.locales();
        let locales = self.config.locale_list(&available_locales);
        let plan_stamp = plan_fingerprint(self.config, plan);
        let fingerprint = format!("{}-{}-{}", plan_stamp, rom.game_version, rom.nso_binary_id);
        // What goes in locales.txt: "all" only when that is what was asked.
        let locales_record = if wanted_locales == Locales::All || locales.len() == available_locales.len() {
            "all".to_string()
        } else {
            locales.join(",")
        };

        if !self.config.force_merge {
            let merges = cache.entries();
            let needed = Locales::Only(locales.clone());
            if let Some((position, merge)) = merges
                .iter()
                .enumerate()
                .find(|(_, merge)| merge.stamp == fingerprint && locales_cover(&merge.locales, &needed))
            {
                if let Some(outcome) = self.serve(&cache, merge) {
                    if position == 0 {
                        info!("using the previous merge ({} files)", outcome.redirects.len());
                    } else {
                        info!(
                            "using a merge from the cache ({} files, made for '{}')",
                            outcome.redirects.len(),
                            merge.profile
                        );
                    }
                    return Outcome {
                        from_cache: position > 0,
                        ..outcome
                    };
                }
            }
            if merges.iter().any(|merge| merge.stamp == fingerprint) {
                info!("the cached merge lacks the texts of {}, merging again", locales.join(","));
            }
            if !self.config.merge_at_boot {
                // The last merge for this version of the game.
                let game = format!("-{}-{}", rom.game_version, rom.nso_binary_id);
                if let Some(merge) = merges.iter().find(|merge| merge.stamp.ends_with(&game)) {
                    if let Some(outcome) = self.serve(&cache, merge) {
                        info!(
                            "the mods changed since the last merge, but merge_at_boot is off: serving the last merge \
                             ({} files) until the changes are applied",
                            outcome.redirects.len()
                        );
                        return outcome;
                    }
                }
            }
        }

        info!(
            "merging {} mod(s) for '{}', this only happens when mods change",
            plan.mods.len(),
            plan.profile
        );
        info!(
            "texts merged for {}{}",
            if locales_record == "all" { "every language" } else { &locales_record },
            if self.config.locales.trim().eq_ignore_ascii_case("auto") && wanted_locales != Locales::All {
                " (locales = auto, from sd:/totk/locale.txt)"
            } else {
                ""
            }
        );
        let started = Instant::now();

        let mut loaded = Vec::with_capacity(plan.mods.len());
        for (index, spec) in plan.mods.iter().enumerate() {
            progress(Stage::Reading, index, plan.mods.len(), &spec.name);
            match self.load(&rom, spec) {
                Ok(mod_data) => loaded.push((spec, mod_data)),
                Err(error) => info!("skipping '{}': {}", spec.name, error),
            }
        }
        progress(Stage::Reading, plan.mods.len(), plan.mods.len(), "");
        let read_seconds = started.elapsed_secs();

        let mut changes: Vec<ModChanges> = Vec::with_capacity(loaded.len());
        for (spec, mod_data) in &loaded {
            let changelogs = match mod_data {
                Loaded::Folder(changelog) => vec![changelog],
                Loaded::Package(package, selection) => {
                    let selected = package.selected_changelogs(selection);
                    for group in &package.option_groups {
                        for option in &group.options {
                            if selected.iter().any(|c| core::ptr::eq(*c, &option.changelog)) {
                                info!("  {} / {}: {}", package.name, group.name, option.name);
                            }
                        }
                    }
                    selected
                }
            };
            changes.push(ModChanges {
                folder: self.folder_of(spec),
                name: spec.name.clone(),
                changelogs,
            });
        }

        // Texts are compared in the language the game reads.
        let text_locale = config::detected_locale()
            .filter(|locale| locales.contains(locale))
            .or_else(|| locales.first().cloned());
        let compared = Instant::now();
        let found = conflicts::find(&rom, &changes, text_locale.as_deref());
        let compare_seconds = compared.elapsed_secs();
        conflicts::log(&found);
        if let Err(error) = conflicts::save(&self.config.cache_dir, &found) {
            info!("could not save the conflicts: {}", error);
        }
        if !crate::go_ahead_despite(&found) {
            info!("merge cancelled because of the conflicts");
            return Outcome {
                conflicts: found.len(),
                cancelled: true,
                error: Some("cancelled".into()),
                ..Outcome::default()
            };
        }
        let changelogs: Vec<&Changelog> = changes.iter().flat_map(|c| c.changelogs.iter().copied()).collect();

        let id = merge_id(&fingerprint, &locales_record);
        if let Err(error) = cache.begin(&id) {
            return Outcome::failure(format!("could not create the merge folder: {}", error));
        }
        let mut output = SdOutput {
            store: StoreWriter::new(&cache.store_dir()),
            redirects: Redirects::new(),
        };
        let options = MergeOptions {
            locales,
            shop_param_limit: self.config.shop_param_limit,
        };
        let merging = Instant::now();
        let report = Merger::new(&rom, &mut output, options).merge(&changelogs);

        let served = Served {
            redirects: output.redirects,
            patches: report.patches,
        };
        let merge = CachedMerge {
            id,
            stamp: fingerprint,
            plan: plan_stamp,
            locales: locales_record,
            profile: plan.profile.clone(),
        };
        if let Err(error) = cache.save(&merge, &served, &conflicts::to_tsv(&found)) {
            info!("could not save the merge index: {}", error);
        }
        info!(
            "merged {} files ({} served, {} warning(s)) in {:.1}s: reading mods {:.1}s, comparing them {:.1}s, \
             merging {:.1}s (of which writing packs {:.1}s); {} file(s) written ({} MiB), {} already stored",
            report.files,
            served.redirects.len(),
            report.warnings,
            read_seconds + compare_seconds + merging.elapsed_secs(),
            read_seconds,
            compare_seconds,
            merging.elapsed_secs(),
            report.pack_seconds,
            output.store.written,
            output.store.bytes_written >> 20,
            output.store.reused
        );
        cache.evict(self.config.merge_cache_size);

        Outcome {
            redirects: served.redirects,
            patches: served.patches,
            failed: false,
            merged_files: report.files,
            warnings: report.warnings,
            reused: false,
            from_cache: false,
            conflicts: found.len(),
            cancelled: false,
            error: None,
        }
    }

    /// A cached merge as an outcome; its conflicts become the current ones.
    fn serve(&self, cache: &MergeCache, merge: &CachedMerge) -> Option<Outcome> {
        let served = cache.load(&merge.id)?;
        cache.touch(&merge.id);
        if let Some(text) = cache.conflicts(&merge.id) {
            let _ = fs::create_dir_all(&self.config.cache_dir);
            let _ = fs::write(&path::join(&self.config.cache_dir, conflicts::FILE_NAME), text.as_bytes());
        }
        Some(Outcome {
            redirects: served.redirects,
            patches: served.patches,
            reused: true,
            ..Outcome::default()
        })
    }

    /// The folder a mod lives in under `mods_dir`, or its name for mods from
    /// elsewhere (RomFSlite, a PC).
    fn folder_of(&self, spec: &ModSpec) -> String {
        let root = format!("{}/", self.config.mods_dir.trim_end_matches('/'));
        match spec.path.strip_prefix(root.as_str()) {
            Some(rest) => rest.split('/').next().unwrap_or(rest).to_string(),
            None => spec.name.clone(),
        }
    }

    fn load(&self, rom: &TkRom, spec: &ModSpec) -> Result<Loaded, String> {
        match spec.kind {
            ModKind::Package => {
                let mut package = read_tkcl_file(&spec.path)?;
                if package.name.trim().is_empty() {
                    package.name = spec.name.trim_end_matches(".tkcl").to_string();
                }
                info!(
                    "'{}' {}{} ({}, {} option group(s))",
                    package.name,
                    package.version,
                    if package.author.is_empty() { String::new() } else { format!(" by {}", package.author) },
                    ulid_to_string(&package.id),
                    package.option_groups.len()
                );
                Ok(Loaded::Package(package, spec.options.clone()))
            }
            ModKind::Folder | ModKind::Romfs => Ok(Loaded::Folder(self.load_folder(rom, spec))),
        }
    }

    /// A folder mod's changelog, from the cache when its files are unchanged.
    fn load_folder(&self, rom: &TkRom, spec: &ModSpec) -> Changelog {
        let root = path::join(&self.config.cache_dir, "changelogs");
        let mut fingerprint = Fingerprint::default();
        fingerprint.mix_str(&content_fingerprint(spec));
        fingerprint.mix_str(&format!("{} {}", rom.game_version, cache::BUILDER_VERSION));
        let entry = cache::entry_name(&spec.name, &fingerprint.finish());
        let dir = path::join(&root, &entry);

        if !self.config.force_merge {
            if let Some(changelog) = cache::load(&dir, &spec.name) {
                info!("'{}': {} changed file(s) (cached)", spec.name, changelog.entries.len());
                return changelog;
            }
        }

        let started = Instant::now();
        let built = build_folder(rom, &spec.path, spec.kind == ModKind::Romfs);
        info!(
            "'{}': {} changed file(s), worked out in {:.1}s",
            spec.name,
            built.entries.len() + built.mals_files.len(),
            started.elapsed_secs()
        );
        if built.skipped_cheats > 0 {
            info!("'{}': {} cheat file(s) ignored", spec.name, built.skipped_cheats);
        }

        match cache::store(&dir, &built, rom.game_version) {
            Ok(()) => cache::prune(&root, &spec.name, &entry),
            Err(error) => info!("could not cache the changelog of '{}': {}", spec.name, error),
        }
        built.into_changelog(&spec.name, rom.game_version)
    }
}

/// True when `prefix` looks like a mounted TotK romfs.
pub fn has_romfs(prefix: &str) -> bool {
    fs::exists(&format!("{}Pack/ZsDic.pack.zs", prefix))
}

/// Finds the prefix the game mounted its romfs under.
pub fn detect_rom_prefix() -> String {
    for candidate in ["content:/", "rom:/", "romfs:/"] {
        if has_romfs(candidate) {
            return candidate.to_string();
        }
    }
    info!("could not find the romfs mount, assuming content:/");
    "content:/".to_string()
}
