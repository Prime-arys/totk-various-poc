//! Merging changelogs into files the game reads. Port of TkMerger, SarcMerger,
//! PackMerger, MsbtMerger, TkPackFileCollector and TkResourceSizeCollector.

use alloc::collections::BTreeMap;

use hashbrown::HashMap;
use totk_formats::msbt::{Duplicates, Msbt};
use totk_formats::rstb::{self, Rstb};
use totk_formats::sarc::{Sarc, SarcBuilder};
use totk_formats::zstd::{compress_raw, frame_content_size};

use crate::builder::{create_changelogs_external, BuildContext, DELETED_MARK};
use crate::canonical::extension;
use crate::prelude::*;
use crate::rom::{TkRom, VersionTable};
use crate::tkcl::{attributes, Changelog, ChangelogEntry, EntryType};
use crate::{byml_merge, debug, gamedata, info, rsdb};

/// Where merged files go.
pub trait Output {
    /// Stores a file for the romfs path `relative`.
    fn write(&mut self, relative: &str, data: &[u8]) -> Result<(), String>;
    /// Serves an existing file for `relative` without copying it.
    fn link(&mut self, relative: &str, path: &str) -> Result<(), String>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergerKind {
    GameData,
    RsdbTag,
    RsdbRow(&'static str),
    Pack,
    Sarc,
    Byml,
    Msbt,
    /// __Combined.bntx: TKMM merges textures; only whole files are handled here.
    Bntx,
}

/// TkMerger.GetMerger. Model codec files (.bfres.mc), which TKMM passes through
/// a material fix-up for 1.4.x, are treated as whole files.
pub fn merger_for(canonical: &str) -> Option<MergerKind> {
    if canonical == gamedata::CANONICAL {
        return Some(MergerKind::GameData);
    }
    if canonical == rsdb::TAG_TABLE {
        return Some(MergerKind::RsdbTag);
    }
    if let Some(key) = rsdb::row_key(canonical) {
        return Some(MergerKind::RsdbRow(key));
    }
    match extension(canonical) {
        ".pack" => Some(MergerKind::Pack),
        ".bfarc" | ".bkres" | ".blarc" | ".genvb" | ".sarc" | ".ta" => Some(MergerKind::Sarc),
        ".bntx" if canonical.ends_with("__Combined.bntx") => Some(MergerKind::Bntx),
        ".byml" | ".bgyml" => Some(MergerKind::Byml),
        ".msbt" => Some(MergerKind::Msbt),
        _ => None,
    }
}

enum Outcome {
    Data(Vec<u8>),
    /// A pack: written once every file that goes in it is known.
    Delayed,
}

pub(crate) const SIZE_OVERRIDE_CANONICAL: &str = "System/Resource/ResourceSizeTable.Product.rsizetable";

/// NSO offsets of the shop parameter limit (TkPatch.CreateWithDefaults).
fn shop_param_patch_address(nso_binary_id: &str) -> Option<u32> {
    Some(match nso_binary_id.to_ascii_lowercase().as_str() {
        "d5ad6ac71ef53e3e52417c1b81dbc9b4142aa3b3" => 0x01ada148,
        "168dd518d925c7a327677286e72feda833314919" => 0x01ad7938,
        "9a10ed9435c06733da597d8094d9000ab5d3ee60" => 0x01ace2b8,
        "6f32c68dd3bc7d77aa714b80e92a096a737cda77" => 0x01ac0128,
        "9b4e43650501a4d4489b4bbfdb740f26af3cf850" => 0x01acb308,
        "6265f94d606242ce730ef721a8037dda8e4bfc63" => 0x0212e60c,
        "965eab9ceb8eb867f747da772022c95065c9b927" => 0x0211e2b8,
        "5cb42b1cf25469fb0635fd046453d843c18bc8ab" => 0x0211de70,
        "277178b7dba1b6d4949a84778d4abc58b31b34f5" => 0x021238fc,
        _ => return None,
    })
}

pub struct MergeOptions {
    /// Message archive locales to merge, e.g. "USen".
    pub locales: Vec<String>,
    /// TKMM always raises the shop parameter limit to this; 0 leaves it alone.
    pub shop_param_limit: u32,
}

#[derive(Debug, Default)]
pub struct MergeReport {
    /// Code patches for this game version: (offset from the main module's
    /// start, instruction), applied by whoever runs the game.
    pub patches: Vec<(u32, u32)>,
    pub files: usize,
    pub warnings: usize,
    pub ignored_code_files: usize,
    /// Time spent putting packs back together and writing them.
    pub pack_seconds: f32,
}

struct PackEntry {
    archive: String,
    attributes: u32,
    canonical: String,
    data: Vec<u8>,
    size_override: u32,
    /// Vanilla content: its resource size stays as it is.
    placeholder: bool,
}

/// A group of changelog entries for one canonical file.
struct Target<'a> {
    key: &'a ChangelogEntry,
    members: Vec<(&'a Changelog, &'a ChangelogEntry)>,
}

pub struct Merger<'a> {
    rom: &'a TkRom,
    output: &'a mut dyn Output,
    options: MergeOptions,
    result_rstb: Option<Rstb>,
    pack_entries: Vec<PackEntry>,
    tracked_packs: HashMap<String, SarcBuilder>,
    size_overrides: HashMap<usize, Rstb>,
    report: MergeReport,
}

impl<'a> Merger<'a> {
    pub fn new(rom: &'a TkRom, output: &'a mut dyn Output, options: MergeOptions) -> Merger<'a> {
        let result_rstb = match rom.get_vanilla(&rom.resource_size_table_path()).0 {
            Some(data) => match Rstb::parse(&data) {
                Ok(table) => Some(table),
                Err(error) => {
                    info!("could not read the resource size table: {}", error);
                    None
                }
            },
            None => {
                info!("the resource size table was not found, sizes will not be updated");
                None
            }
        };

        Merger {
            rom,
            output,
            options,
            result_rstb,
            pack_entries: Vec::new(),
            tracked_packs: HashMap::new(),
            size_overrides: HashMap::new(),
            report: MergeReport::default(),
        }
    }

    fn ctx(&self) -> BuildContext<'a> {
        BuildContext { rom: self.rom }
    }

    /// Merges changelogs, lowest priority first.
    pub fn merge(mut self, changelogs: &[&Changelog]) -> MergeReport {
        self.load_size_overrides(changelogs);
        self.merge_patches(changelogs);

        for changelog in changelogs {
            let code = changelog.subsdk_files.len() + changelog.exe_files.len() + changelog.cheats.len();
            if code > 0 {
                info!(
                    "{}: {} code/cheat file(s) ignored — exefs mods need to be Skyline plugins",
                    changelog.source.describe(),
                    code
                );
                self.report.ignored_code_files += code;
            }
        }

        let targets = group_targets(changelogs);
        // Message archives count as one step of the merge.
        let steps = targets.len() + 1;
        crate::progress(crate::Stage::Merging, 0, steps, "Mals");
        self.merge_mals(changelogs);

        for (index, target) in targets.iter().enumerate() {
            crate::progress(crate::Stage::Merging, index + 1, steps, &target.key.canonical);
            self.merge_target(target);
        }

        crate::progress(crate::Stage::Writing, 0, 1, "");
        let packs = crate::sys::time::Instant::now();
        self.write_packs();
        self.report.pack_seconds = packs.elapsed_secs();
        self.write_resource_size_table();
        crate::progress(crate::Stage::Writing, 1, 1, "");
        self.report
    }

    fn warn(&mut self, message: String) {
        info!("{}", message);
        self.report.warnings += 1;
    }

    fn merge_patches(&mut self, changelogs: &[&Changelog]) {
        let id = self.rom.nso_binary_id.to_ascii_lowercase();
        let mut entries: BTreeMap<u32, u32> = BTreeMap::new();

        if self.options.shop_param_limit != 0 {
            if let Some(address) = shop_param_patch_address(&id) {
                // mov w8, #limit
                let instruction = (0x5280_0008u32 | (self.options.shop_param_limit << 5)).swap_bytes();
                entries.insert(address, instruction);
            }
        }

        for changelog in changelogs {
            for patch in &changelog.patches {
                if patch.nso_binary_id.eq_ignore_ascii_case(&id) {
                    entries.extend(patch.entries.iter().copied());
                } else {
                    debug!("patch for another game version ({}) skipped", patch.nso_binary_id);
                }
            }
        }

        self.report.patches = entries.into_iter().collect();
    }

    fn load_size_overrides(&mut self, changelogs: &[&Changelog]) {
        for changelog in changelogs {
            if !changelog.entries.iter().any(|e| e.canonical == SIZE_OVERRIDE_CANONICAL) {
                continue;
            }
            let Some(data) = changelog.source.read(&format!("romfs/{}", SIZE_OVERRIDE_CANONICAL)) else {
                continue;
            };
            match Rstb::parse(&data) {
                Ok(table) => {
                    self.size_overrides.insert(*changelog as *const Changelog as usize, table);
                }
                Err(error) => info!("{}: bad resource size overrides: {}", changelog.source.describe(), error),
            }
        }
    }

    /// Resource size a package asks for a whole file it ships.
    fn size_override(&self, changelog: &Changelog, entry: &ChangelogEntry) -> u32 {
        if entry.kind != EntryType::Copy {
            return 0;
        }
        self.size_overrides
            .get(&(changelog as *const Changelog as usize))
            .and_then(|table| table.name_size(&entry.canonical))
            .unwrap_or(0)
    }

    fn changelog_file_path(&self, entry: &ChangelogEntry) -> String {
        changelog_file_path(self.rom, entry)
    }

    fn read_member(&self, changelog: &Changelog, entry: &ChangelogEntry) -> Option<Vec<u8>> {
        let path = self.changelog_file_path(entry);
        let data = changelog.source.read(&path);
        if data.is_none() {
            info!("{}: {} is listed but missing", changelog.source.describe(), path);
        }
        data
    }

    fn decompress(&self, data: Vec<u8>) -> Result<Vec<u8>, String> {
        if totk_formats::zstd::Zstd::is_compressed(&data) {
            self.rom.decompress(&data)
        } else {
            Ok(data)
        }
    }

    fn merge_target(&mut self, target: &Target) {
        let key = target.key;
        let archives: Vec<String> = target
            .members
            .iter()
            .flat_map(|(_, entry)| entry.archive_canonicals.iter().cloned())
            .collect();
        let relative = if archives.is_empty() {
            self.rom.canonical_to_relative(&key.canonical, key.attributes)
        } else {
            self.rom.canonical_to_relative(&key.canonical, 0)
        };

        let last = target
            .members
            .iter()
            .rev()
            .find(|(_, entry)| entry.kind != EntryType::Placeholder)
            .copied();
        let size_override = last
            .map(|(changelog, entry)| self.size_override(changelog, entry))
            .unwrap_or(0);

        let mut kind = merger_for(&key.canonical);
        let mut members: Vec<(&Changelog, &ChangelogEntry)> = target
            .members
            .iter()
            .copied()
            .filter(|(_, entry)| entry.kind != EntryType::Placeholder)
            .collect();

        if kind == Some(MergerKind::Bntx) {
            let changelogs = members.iter().filter(|(_, e)| e.kind == EntryType::Changelog).count();
            if changelogs > 0 {
                self.warn(format!(
                    "{}: {} texture changelog(s) cannot be applied on the console and were skipped",
                    key.canonical, changelogs
                ));
                members.retain(|(_, e)| e.kind != EntryType::Changelog);
            }
            kind = None;
        }

        let Some(kind) = kind else {
            // Whole files: the highest priority one wins.
            match members.last() {
                Some((changelog, entry)) => self.copy_to_output(changelog, entry, &relative, key, &archives, size_override),
                None => self.copy_placeholder(&relative, key, &archives),
            }
            return;
        };

        if members.is_empty() {
            self.copy_placeholder(&relative, key, &archives);
            return;
        }

        let (vanilla, found_missing) = self.rom.get_vanilla(&relative);
        let outcome = match vanilla {
            None if members.len() == 1 => {
                if found_missing && key.kind == EntryType::Changelog {
                    self.warn(format!(
                        "the changelog for '{}' could not be merged: the vanilla file is missing",
                        key.canonical
                    ));
                    return;
                }
                let (changelog, entry) = members[0];
                self.copy_to_output(changelog, entry, &relative, key, &archives, size_override);
                return;
            }
            None => {
                if found_missing && members.iter().any(|(_, e)| e.kind == EntryType::Changelog) {
                    self.warn(format!(
                        "the changelog for '{}' could not be merged: the vanilla file is missing",
                        key.canonical
                    ));
                    return;
                }
                self.merge_custom_target(kind, &key.canonical, &members)
            }
            Some(vanilla) => {
                let mut inputs = Vec::with_capacity(members.len());
                for (changelog, entry) in &members {
                    let Some(data) = self.read_member(changelog, entry) else {
                        continue;
                    };
                    match self.decompress(data) {
                        Ok(data) => inputs.push(data),
                        Err(error) => info!("{}: {}", entry.canonical, error),
                    }
                }
                self.merge_data(kind, &key.canonical, vanilla, inputs)
            }
        };

        match outcome {
            Ok(Outcome::Data(data)) => {
                debug!("merged {} ({} bytes)", key.canonical, data.len());
                self.report.files += 1;
                self.copy_merged_to_output(data, &relative, key, &archives, 0);
            }
            Ok(Outcome::Delayed) => {}
            Err(error) => {
                // Better something than nothing: fall back to the highest
                // priority whole file, if a mod shipped one.
                match members.iter().rev().find(|(_, e)| e.kind == EntryType::Copy) {
                    Some((changelog, entry)) => {
                        self.warn(format!(
                            "could not merge '{}' ({}); using the version from {}",
                            key.canonical,
                            error,
                            changelog.source.describe()
                        ));
                        self.copy_to_output(changelog, entry, &relative, key, &archives, size_override);
                    }
                    None => self.warn(format!("could not merge '{}': {}", key.canonical, error)),
                }
            }
        }
    }

    fn copy_placeholder(&mut self, relative: &str, key: &ChangelogEntry, archives: &[String]) {
        if archives.is_empty() {
            // Copying a vanilla file over itself would be pointless.
            return;
        }
        match self.rom.get_vanilla(relative).0 {
            Some(vanilla) => self.collect_pack_entries(key, archives, vanilla, 0, true),
            None => self.warn(format!(
                "could not place '{}' in its packs: the vanilla file could not be found",
                relative
            )),
        }
    }

    fn merge_custom_target(
        &mut self,
        kind: MergerKind,
        canonical: &str,
        members: &[(&Changelog, &ChangelogEntry)],
    ) -> Result<Outcome, String> {
        let mut streams = Vec::with_capacity(members.len());
        for (changelog, entry) in members {
            let data = self
                .read_member(changelog, entry)
                .ok_or_else(|| format!("{} is missing from {}", entry.canonical, changelog.source.describe()))?;
            streams.push((self.decompress(data)?, entry.kind));
        }

        let (base, base_kind) = streams.remove(0);
        let ctx = self.ctx();

        let deltas = if base_kind != EntryType::Copy || streams.iter().all(|(_, k)| *k != EntryType::Changelog) {
            let targets: Vec<Vec<u8>> = streams.into_iter().map(|(data, _)| data).collect();
            create_changelogs_external(&ctx, canonical, &base, &targets)?
        } else {
            let mut deltas = Vec::new();
            let mut copies = Vec::new();
            for (data, kind) in streams {
                if kind == EntryType::Changelog {
                    deltas.push(data);
                } else {
                    copies.push(data);
                }
            }
            if !copies.is_empty() {
                deltas.extend(create_changelogs_external(&ctx, canonical, &base, &copies)?);
            }
            deltas
        };

        if deltas.is_empty() {
            return Ok(Outcome::Data(base));
        }
        self.merge_data(kind, canonical, base, deltas)
    }

    fn merge_data(&mut self, kind: MergerKind, canonical: &str, vanilla: Vec<u8>, inputs: Vec<Vec<u8>>) -> Result<Outcome, String> {
        if inputs.is_empty() {
            return Ok(Outcome::Data(vanilla));
        }
        Ok(Outcome::Data(match kind {
            MergerKind::GameData => gamedata::merge(vanilla, &inputs)?,
            MergerKind::RsdbTag => rsdb::merge_tags(&vanilla, &inputs)?,
            MergerKind::RsdbRow(key) => rsdb::merge_rows(canonical, key, vanilla, &inputs)?,
            MergerKind::Byml => byml_merge::merge_documents(canonical, vanilla, &inputs)?,
            MergerKind::Msbt => merge_msbt(&vanilla, &inputs)?,
            MergerKind::Sarc => self.merge_sarc(canonical, &vanilla, &inputs)?,
            MergerKind::Bntx => inputs.into_iter().last().unwrap(),
            MergerKind::Pack => {
                self.merge_pack(canonical, &vanilla, &inputs)?;
                return Ok(Outcome::Delayed);
            }
        }))
    }

    /// SarcMerger.MergeMany.
    fn merge_sarc(&mut self, parent: &str, vanilla: &[u8], changelogs: &[Vec<u8>]) -> Result<Vec<u8>, String> {
        let vanilla_sarc = Sarc::parse(vanilla).map_err(|e| e.to_string())?;
        let mut merged = SarcBuilder::from_sarc(&vanilla_sarc);

        let mut groups: Vec<(String, Vec<Vec<u8>>)> = Vec::new();
        let mut positions: HashMap<String, usize> = HashMap::new();
        for changelog in changelogs {
            let sarc = Sarc::parse(changelog).map_err(|e| e.to_string())?;
            for entry in sarc.entries() {
                let position = *positions.entry(entry.name.to_string()).or_insert_with(|| {
                    groups.push((entry.name.to_string(), Vec::new()));
                    groups.len() - 1
                });
                groups[position].1.push(entry.data.to_vec());
            }
        }

        for (name, buffers) in groups {
            // Deletion marks from older TKMM versions are ignored, so files a
            // game update added are not removed.
            let mut effective: Vec<Vec<u8>> = buffers.into_iter().filter(|b| b.as_slice() != DELETED_MARK).collect();
            if effective.is_empty() {
                continue;
            }
            let nested = format!("{}/{}", parent, name);
            let kind = merger_for(&name).filter(|k| !matches!(k, MergerKind::Pack | MergerKind::Bntx));

            let data = match (vanilla_sarc.get(&name), kind) {
                (_, None) => effective.pop().unwrap(),
                (None, Some(_)) if effective.len() == 1 => effective.pop().unwrap(),
                (None, Some(kind)) => {
                    let base = effective.remove(0);
                    let deltas = create_changelogs_external(&self.ctx(), &nested, &base, &effective)?;
                    if deltas.is_empty() {
                        base
                    } else {
                        match self.merge_data(kind, &nested, base, deltas)? {
                            Outcome::Data(data) => data,
                            Outcome::Delayed => continue,
                        }
                    }
                }
                (Some(vanilla_entry), Some(kind)) => {
                    match self.merge_data(kind, &nested, vanilla_entry.data.to_vec(), effective)? {
                        Outcome::Data(data) => data,
                        Outcome::Delayed => continue,
                    }
                }
            };
            merged.insert(&name, data);
        }

        Ok(merged.build())
    }

    /// PackMerger: only deletions live in a pack's changelog.
    fn merge_pack(&mut self, canonical: &str, vanilla: &[u8], changelogs: &[Vec<u8>]) -> Result<(), String> {
        let vanilla_sarc = Sarc::parse(vanilla).map_err(|e| e.to_string())?;
        let mut merged = SarcBuilder::from_sarc(&vanilla_sarc);

        let mut last: BTreeMap<String, bool> = BTreeMap::new();
        for changelog in changelogs {
            let sarc = Sarc::parse(changelog).map_err(|e| e.to_string())?;
            for entry in sarc.entries() {
                last.insert(entry.name.to_string(), entry.data == DELETED_MARK);
            }
        }
        for (name, removed) in last {
            if removed {
                merged.remove(&name);
            }
        }

        self.tracked_packs.insert(canonical.to_string(), merged);
        Ok(())
    }

    fn collect_pack_entries(&mut self, key: &ChangelogEntry, archives: &[String], data: Vec<u8>, size_override: u32, placeholder: bool) {
        for archive in archives {
            self.pack_entries.push(PackEntry {
                archive: archive.clone(),
                attributes: key.attributes,
                canonical: key.canonical.clone(),
                data: data.clone(),
                size_override,
                placeholder,
            });
        }
    }

    fn copy_to_output(
        &mut self,
        changelog: &Changelog,
        entry: &ChangelogEntry,
        relative: &str,
        key: &ChangelogEntry,
        archives: &[String],
        size_override: u32,
    ) {
        let source_path = self.changelog_file_path(entry);
        self.report.files += 1;

        if !archives.is_empty() {
            if let Some(data) = self.read_member(changelog, entry) {
                // TKMM leaves the size of files identical to vanilla alone
                // (it checks against its checksum list).
                let is_vanilla = self.rom.get_vanilla(&key.canonical).0.map_or(false, |vanilla| vanilla == data);
                self.collect_pack_entries(key, archives, data, size_override, is_vanilla);
            }
            return;
        }

        if !rstb::requires_data(relative) {
            let head = changelog.source.read_head(&source_path, 32);
            let size = match &head {
                Some(head) => frame_content_size(head),
                None => None,
            };

            if let Some(path) = changelog.source.file_path(&source_path) {
                let size = size.or_else(|| crate::sys::fs::metadata(&path).map(|m| m.len)).unwrap_or(0);
                self.collect_size(size as u32, relative, None, size_override);
                if let Err(error) = self.output.link(relative, &path) {
                    self.warn(format!("could not serve {}: {}", relative, error));
                }
                return;
            }

            let Some(data) = changelog.source.read(&source_path) else {
                self.warn(format!("{} is missing from {}", source_path, changelog.source.describe()));
                return;
            };
            let size = size.unwrap_or(data.len() as u64);
            self.collect_size(size as u32, relative, None, size_override);
            if let Err(error) = self.output.write(relative, &data) {
                self.warn(format!("could not write {}: {}", relative, error));
            }
            return;
        }

        let Some(raw) = changelog.source.read(&source_path) else {
            self.warn(format!("{} is missing from {}", source_path, changelog.source.describe()));
            return;
        };
        match self.decompress(raw.clone()) {
            Ok(data) => self.collect_size(data.len() as u32, relative, Some(&data), size_override),
            Err(error) => info!("{}: {}", relative, error),
        }
        let result = match changelog.source.file_path(&source_path) {
            Some(path) => self.output.link(relative, &path),
            None => self.output.write(relative, &raw),
        };
        if let Err(error) = result {
            self.warn(format!("could not write {}: {}", relative, error));
        }
    }

    fn copy_merged_to_output(&mut self, data: Vec<u8>, relative: &str, key: &ChangelogEntry, archives: &[String], size_override: u32) {
        if !archives.is_empty() {
            self.collect_pack_entries(key, archives, data, size_override, false);
            return;
        }
        self.write_simple(data, relative, key.attributes, size_override);
    }

    /// TkMerger.CopyMergedToSimpleOutput. Output is stored in raw zstd frames:
    /// valid for the game, and no compression time spent on the console.
    fn write_simple(&mut self, data: Vec<u8>, relative: &str, attributes: u32, size_override: u32) {
        self.collect_size(data.len() as u32, relative, Some(&data), size_override);
        let bytes = if attributes & self::attributes::HAS_ZS_EXTENSION != 0 {
            compress_raw(&data)
        } else {
            data
        };
        if let Err(error) = self.output.write(relative, &bytes) {
            self.warn(format!("could not write {}: {}", relative, error));
        }
    }

    /// TkResourceSizeCollector.Collect.
    fn collect_size(&mut self, file_size: u32, path: &str, data: Option<&[u8]>, size_override: u32) {
        let Some(table) = self.result_rstb.as_mut() else {
            return;
        };
        let name = rstb::resource_name(path);
        let ext = rstb::resource_extension(path);
        if name == "Pack/ZsDic.pack" || matches!(ext, ".rsizetable" | ".bwav" | ".webm") {
            return;
        }

        let size = if size_override != 0 {
            size_override
        } else {
            rstb::resource_size_with_extension(file_size, &name, ext, data.unwrap_or(&[]))
        };

        if table.name_size(&name).is_some() {
            table.set_name_size(&name, size);
            return;
        }
        let hash = totk_formats::crc32::compute_str(&name);
        if table.try_add_hash(hash, size) {
            return;
        }
        if !table.has_original_hash(hash) {
            // Two modded resources share a hash: keep them apart.
            table.set_name_size(&name, size);
            return;
        }
        table.set_hash_size(hash, size);
    }

    /// TkPackFileCollector.Write.
    fn write_packs(&mut self) {
        let entries = core::mem::take(&mut self.pack_entries);
        let mut order: Vec<String> = Vec::new();
        let mut groups: HashMap<String, Vec<PackEntry>> = HashMap::new();
        for entry in entries {
            if !groups.contains_key(&entry.archive) {
                order.push(entry.archive.clone());
            }
            groups.entry(entry.archive.clone()).or_default().push(entry);
        }

        for archive in order {
            let entries = groups.remove(&archive).unwrap();
            let attributes = entries[0].attributes;
            let relative = self.rom.canonical_to_relative(&archive, attributes);

            let mut sarc = match self.tracked_packs.remove(&archive) {
                Some(tracked) => tracked,
                None => match self.rom.get_vanilla(&relative).0 {
                    Some(vanilla) => match Sarc::parse(&vanilla) {
                        Ok(sarc) => SarcBuilder::from_sarc(&sarc),
                        Err(error) => {
                            self.warn(format!("{}: {}", relative, error));
                            continue;
                        }
                    },
                    None => SarcBuilder::new(),
                },
            };

            for entry in entries {
                if sarc.get(&entry.canonical) == Some(DELETED_MARK.as_slice()) {
                    sarc.remove(&entry.canonical);
                    continue;
                }
                if !entry.placeholder {
                    self.collect_size(entry.data.len() as u32, &entry.canonical, Some(&entry.data), entry.size_override);
                }
                sarc.insert(&entry.canonical, entry.data);
            }

            self.report.files += 1;
            self.write_simple(sarc.build(), &relative, attributes, 0);
        }
    }

    fn write_resource_size_table(&mut self) {
        let Some(table) = self.result_rstb.take() else {
            return;
        };
        let relative = self.rom.resource_size_table_path();
        let bytes = compress_raw(&table.write());
        if let Err(error) = self.output.write(&relative, &bytes) {
            self.warn(format!("could not write {}: {}", relative, error));
        }
    }

    fn merge_mals(&mut self, changelogs: &[&Changelog]) {
        let with_mals: Vec<&&Changelog> = changelogs.iter().filter(|c| !c.mals_files.is_empty()).collect();
        if with_mals.is_empty() {
            return;
        }

        for locale in self.options.locales.clone() {
            let mut inputs = Vec::new();
            for changelog in &with_mals {
                let file = best_mals(&changelog.mals_files, &locale);
                match changelog.source.read(&format!("romfs/{}", file)).map(|data| self.decompress(data)) {
                    Some(Ok(data)) => inputs.push(data),
                    Some(Err(error)) => info!("{}: {}", file, error),
                    None => info!("{}: romfs/{} is missing", changelog.source.describe(), file),
                }
            }
            if inputs.is_empty() {
                continue;
            }

            let canonical = format!("Mals/{}.Product.sarc", locale);
            let attributes = attributes::HAS_ZS_EXTENSION | attributes::IS_PRODUCT_FILE;
            let relative = self.rom.canonical_to_relative(&canonical, attributes);
            let Some(vanilla) = self.rom.get_vanilla(&relative).0 else {
                self.warn(format!("{} could not be merged: the vanilla file was not found", canonical));
                continue;
            };

            match self.merge_sarc(&canonical, &vanilla, &inputs) {
                Ok(merged) => {
                    debug!("merged {}", canonical);
                    self.report.files += 1;
                    self.write_simple(merged, &relative, attributes, 0);
                }
                Err(error) => self.warn(format!("could not merge {}: {}", canonical, error)),
            }
        }
    }
}

/// TkMerger.GetRelativeRomFsPath: where an entry's file is inside its
/// changelog, picking the version closest to the running game.
pub fn changelog_file_path(rom: &TkRom, entry: &ChangelogEntry) -> String {
    if entry.versions.is_empty() {
        return format!("romfs/{}", entry.canonical);
    }

    let canonical = entry.canonical.as_str();
    let file_stem = {
        let name = canonical.rsplit('/').next().unwrap_or(canonical);
        match name.rfind('.') {
            Some(i) => &name[..i],
            None => name,
        }
    };
    let last3 = |value: &str| -> Option<i32> { value.get(value.len().saturating_sub(3)..)?.parse().ok() };

    let table_version = if canonical.len() > 15 && canonical.starts_with("Event/EventFlow") {
        Some(rom.event_flow_version(file_stem).and_then(|v| v.parse().ok()))
    } else if canonical.len() > 8 && canonical.starts_with("Sequence") {
        Some(rom.versioned_name(VersionTable::Sequence, file_stem).and_then(last3))
    } else if canonical.len() > 6 && canonical.starts_with("Effect") {
        Some(rom.versioned_name(VersionTable::Effect, file_stem).and_then(last3))
    } else if canonical.len() > 5 && canonical.starts_with("Logic") {
        Some(rom.versioned_name(VersionTable::Logic, file_stem).and_then(last3))
    } else if canonical.len() > 2 && canonical.starts_with("AI") {
        Some(rom.versioned_name(VersionTable::Ai, file_stem).and_then(last3))
    } else {
        None
    };

    let version = match table_version {
        Some(Some(target)) => best_version(target, &entry.versions),
        Some(None) => entry.versions[0],
        None => best_version(rom.game_version, &entry.versions),
    };
    format!("romfs/{}{}", entry.canonical, version)
}

/// TkMerger.GetBestVersion.
fn best_version(target: i32, provided: &[i32]) -> i32 {
    provided.iter().rev().copied().find(|&v| target >= v).unwrap_or(provided[0])
}

/// MalsMerger.GetBestMals: the message archive closest to `locale`.
pub fn best_mals<'b>(targets: &'b [String], locale: &str) -> &'b str {
    if targets.len() == 1 || locale.len() != 4 {
        return &targets[0];
    }
    let (region, language) = locale.split_at(2);
    let mut best: Option<&str> = None;
    let mut level = 1;
    for target in targets {
        let Some(code) = target.get(5..9) else {
            continue;
        };
        let (target_region, target_language) = code.split_at(2);
        if target_language == language && target_region == region {
            return target;
        }
        if target_language == language {
            best = Some(target);
            level = 2;
            continue;
        }
        if level < 2 && target_language == "en" {
            best = Some(target);
        }
    }
    best.unwrap_or(&targets[0])
}

/// MsbtMerger: later entries replace earlier ones.
fn merge_msbt(vanilla: &[u8], changelogs: &[Vec<u8>]) -> Result<Vec<u8>, String> {
    let mut merged = Msbt::parse(vanilla).map_err(|e| e.to_string())?;
    for changelog in changelogs {
        let changelog = Msbt::parse_with(changelog, Duplicates::KeepLast).map_err(|e| e.to_string())?;
        for (label, entry) in changelog.entries() {
            merged.insert(label.to_string(), entry.clone());
        }
    }
    Ok(merged.write())
}

/// TkMerger.GetTargets: every changelog entry grouped by canonical name, in
/// the order they first appear.
fn group_targets<'b>(changelogs: &[&'b Changelog]) -> Vec<Target<'b>> {
    let mut targets: Vec<Target<'b>> = Vec::new();
    let mut positions: HashMap<&'b str, usize> = HashMap::new();
    for changelog in changelogs {
        for entry in &changelog.entries {
            if entry.canonical == SIZE_OVERRIDE_CANONICAL {
                continue;
            }
            let position = *positions.entry(entry.canonical.as_str()).or_insert_with(|| {
                targets.push(Target {
                    key: entry,
                    members: Vec::new(),
                });
                targets.len() - 1
            });
            targets[position].members.push((changelog, entry));
        }
    }
    targets
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_closest_message_archive() {
        let files = vec!["Mals/EUen.Product.sarc".to_string(), "Mals/USfr.Product.sarc".to_string()];
        assert_eq!(best_mals(&files, "USen"), "Mals/EUen.Product.sarc");
        assert_eq!(best_mals(&files, "EUfr"), "Mals/USfr.Product.sarc");
        assert_eq!(best_mals(&files, "JPja"), "Mals/EUen.Product.sarc");
        let files = vec!["Mals/JPja.Product.sarc".to_string(), "Mals/USfr.Product.sarc".to_string()];
        assert_eq!(best_mals(&files, "KRko"), "Mals/JPja.Product.sarc");
    }

    #[test]
    fn picks_versions_like_tkmm() {
        assert_eq!(best_version(121, &[100, 110, 120]), 120);
        assert_eq!(best_version(100, &[110, 120]), 110);
    }

    #[test]
    fn merger_kinds_follow_tkmm() {
        assert_eq!(merger_for("RSDB/Tag.Product.rstbl.byml"), Some(MergerKind::RsdbTag));
        assert_eq!(merger_for("RSDB/Other.Product.rstbl.byml"), Some(MergerKind::Byml));
        assert_eq!(merger_for("Pack/Actor/Foo.pack"), Some(MergerKind::Pack));
        assert_eq!(merger_for("Model/Foo.bfres"), None);
    }
}
