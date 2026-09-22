//! Mods that change the same things.
//!
//! The merge goes ahead regardless (the mod with the highest priority wins),
//! but whoever runs it can warn about them first. Two kinds are found:
//!
//! - files that cannot be merged (models, textures, AI...) shipped by several
//!   mods: only the winner's is used;
//! - values of merged files (parameters, game data rows, texts...) that
//!   several mods set differently: only the winner's value is used. Additions
//!   (new rows, array entries) do not collide.

use alloc::collections::{BTreeMap, BTreeSet};

use totk_formats::byml::{Byml, ChangeType};
use totk_formats::msbt::{Duplicates, Msbt};
use totk_formats::sarc::Sarc;
use totk_formats::zstd::Zstd;

use crate::builder::DELETED_MARK;
use crate::merger::{best_mals, changelog_file_path, merger_for, MergerKind, SIZE_OVERRIDE_CANONICAL};
use crate::mods::Fingerprint;
use crate::prelude::*;
use crate::rom::TkRom;
use crate::sys::{fs, path};
use crate::tkcl::{Changelog, ChangelogEntry, EntryType};
use crate::{debug, info, progress, Stage};

/// The conflicts found by the last merge, in the cache folder.
pub const FILE_NAME: &str = "conflicts.tsv";
/// Values named per file, at most.
const SAMPLES: usize = 3;
/// Files larger than this are not taken apart value by value.
const MAX_VALUES_FILE: usize = 8 * 1024 * 1024;
/// Leaf value of a removed key or row.
const REMOVED: u64 = u64::MAX;

/// One mod's changelogs (a package brings one per selected option).
pub struct ModChanges<'a> {
    pub folder: String,
    pub name: String,
    pub changelogs: Vec<&'a Changelog>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictKind {
    /// A file that cannot be merged, shipped by several mods.
    File,
    /// Values of a merged file that several mods set differently.
    Values,
}

impl ConflictKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ConflictKind::File => "file",
            ConflictKind::Values => "values",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModRef {
    pub folder: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub kind: ConflictKind,
    /// The game file, e.g. "Component/.../Foo.game__component__X.bgyml", or
    /// "Mals/EUfr/EventFlowMsg/Npc.msbt" for texts.
    pub file: String,
    /// The mods involved, the one whose version is used first.
    pub mods: Vec<ModRef>,
    /// Values set differently (1 for a whole file).
    pub count: usize,
    /// A few of those values, e.g. "Life" or "[Weapon_Sword_001]/Attack".
    pub samples: Vec<String>,
}

/// Finds conflicts between `mods`, given lowest priority first. Texts are
/// compared in `locale` only (mods change every language the same way).
pub fn find(rom: &TkRom, mods: &[ModChanges], locale: Option<&str>) -> Vec<Conflict> {
    let mut targets: BTreeMap<&str, Vec<(usize, &Changelog, &ChangelogEntry)>> = BTreeMap::new();
    for (index, changes) in mods.iter().enumerate() {
        for changelog in &changes.changelogs {
            for entry in &changelog.entries {
                if entry.kind != EntryType::Placeholder && entry.canonical != SIZE_OVERRIDE_CANONICAL {
                    targets
                        .entry(entry.canonical.as_str())
                        .or_default()
                        .push((index, *changelog, entry));
                }
            }
        }
    }
    targets.retain(|_, members| {
        let first = members[0].0;
        members.iter().any(|(index, _, _)| *index != first)
    });

    // Message archives count as one step.
    let steps = targets.len() + 1;
    let mut conflicts = Vec::new();
    for (step, (canonical, members)) in targets.iter().enumerate() {
        progress(Stage::Conflicts, step, steps, canonical);
        let found = match merger_for(canonical) {
            None | Some(MergerKind::Bntx) => file_conflict(rom, mods, canonical, members),
            // Tag tables and packs only ever add up.
            Some(MergerKind::RsdbTag | MergerKind::Pack) => None,
            Some(kind) => value_conflict(rom, mods, canonical, kind, members),
        };
        conflicts.extend(found);
    }
    progress(Stage::Conflicts, steps - 1, steps, "Mals");
    if let Some(locale) = locale {
        conflicts.extend(text_conflicts(rom, mods, locale));
    }
    progress(Stage::Conflicts, steps, steps, "");
    conflicts
}

fn mod_refs(mods: &[ModChanges], involved: &BTreeSet<usize>) -> Vec<ModRef> {
    involved
        .iter()
        .rev()
        .map(|&index| ModRef {
            folder: mods[index].folder.clone(),
            name: mods[index].name.clone(),
        })
        .collect()
}

fn decompress(rom: &TkRom, data: Vec<u8>) -> Option<Vec<u8>> {
    if Zstd::is_compressed(&data) {
        rom.decompress(&data).ok()
    } else {
        Some(data)
    }
}

/// Each mod's last entry for a file: the one its merge uses.
fn last_per_mod<'b>(
    members: &[(usize, &'b Changelog, &'b ChangelogEntry)],
) -> Vec<(usize, &'b Changelog, &'b ChangelogEntry)> {
    let mut last: BTreeMap<usize, (&Changelog, &ChangelogEntry)> = BTreeMap::new();
    for (index, changelog, entry) in members {
        last.insert(*index, (*changelog, *entry));
    }
    last.into_iter().map(|(index, (changelog, entry))| (index, changelog, entry)).collect()
}

fn file_conflict(
    rom: &TkRom,
    mods: &[ModChanges],
    canonical: &str,
    members: &[(usize, &Changelog, &ChangelogEntry)],
) -> Option<Conflict> {
    let versions = last_per_mod(members);
    // The same file in several mods (a shared dependency) is no conflict.
    if same_contents(rom, &versions) {
        debug!("{}: the same file in {} mods", canonical, versions.len());
        return None;
    }
    let involved: BTreeSet<usize> = versions.iter().map(|(index, _, _)| *index).collect();
    Some(Conflict {
        kind: ConflictKind::File,
        file: canonical.to_string(),
        mods: mod_refs(mods, &involved),
        count: 1,
        samples: Vec::new(),
    })
}

fn same_contents(rom: &TkRom, versions: &[(usize, &Changelog, &ChangelogEntry)]) -> bool {
    // Most files differ from their first bytes: only read whole files when
    // those match.
    const HEAD: usize = 4096;
    let paths: Vec<String> = versions.iter().map(|(_, _, entry)| changelog_file_path(rom, entry)).collect();
    let heads: Vec<Option<Vec<u8>>> = versions
        .iter()
        .zip(&paths)
        .map(|((_, changelog, _), path)| changelog.source.read_head(path, HEAD))
        .collect();
    if heads.iter().any(|head| head.is_none() || *head != heads[0]) {
        return false;
    }
    let first = versions[0].1.source.read(&paths[0]);
    first.is_some()
        && versions[1..]
            .iter()
            .zip(&paths[1..])
            .all(|((_, changelog, _), path)| changelog.source.read(path) == first)
}

fn value_conflict(
    rom: &TkRom,
    mods: &[ModChanges],
    canonical: &str,
    kind: MergerKind,
    members: &[(usize, &Changelog, &ChangelogEntry)],
) -> Option<Conflict> {
    let mut sets: Vec<(usize, Leaves)> = Vec::new();
    for (index, changelog, entry) in members {
        let data = changelog.source.read(&changelog_file_path(rom, entry))?;
        let data = decompress(rom, data)?;
        if data.len() > MAX_VALUES_FILE {
            debug!("{}: too large to compare value by value", canonical);
            return None;
        }
        let leaves = file_leaves(kind, &data)?;
        // Options of one package apply one after the other.
        match sets.last_mut() {
            Some((last, merged)) if last == index => merged.extend(leaves),
            _ => sets.push((*index, leaves)),
        }
    }
    let (involved, count, samples) = collisions(&sets)?;
    Some(Conflict {
        kind: ConflictKind::Values,
        file: canonical.to_string(),
        mods: mod_refs(mods, &involved),
        count,
        samples,
    })
}

fn text_conflicts(rom: &TkRom, mods: &[ModChanges], locale: &str) -> Vec<Conflict> {
    let mut per_file: BTreeMap<String, Vec<(usize, Leaves)>> = BTreeMap::new();
    for (index, changes) in mods.iter().enumerate() {
        let mut files: BTreeMap<String, Leaves> = BTreeMap::new();
        for changelog in &changes.changelogs {
            if changelog.mals_files.is_empty() {
                continue;
            }
            let archive = best_mals(&changelog.mals_files, locale);
            let Some(data) = changelog
                .source
                .read(&format!("romfs/{}", archive))
                .and_then(|data| decompress(rom, data))
            else {
                continue;
            };
            let Ok(sarc) = Sarc::parse(&data) else {
                continue;
            };
            for entry in sarc.entries() {
                if entry.data == DELETED_MARK {
                    continue;
                }
                if let Some(leaves) = msbt_leaves(entry.data) {
                    files.entry(entry.name.to_string()).or_default().extend(leaves);
                }
            }
        }
        for (name, leaves) in files {
            per_file.entry(name).or_default().push((index, leaves));
        }
    }

    let mut conflicts = Vec::new();
    for (name, sets) in per_file {
        if sets.len() < 2 {
            continue;
        }
        if let Some((involved, count, samples)) = collisions(&sets) {
            conflicts.push(Conflict {
                kind: ConflictKind::Values,
                file: format!("Mals/{}/{}", locale, name),
                mods: mod_refs(mods, &involved),
                count,
                samples,
            });
        }
    }
    conflicts
}

/// Value path ("Actor/[3]/Life") -> hash of what a mod sets there.
type Leaves = BTreeMap<String, u64>;

fn file_leaves(kind: MergerKind, data: &[u8]) -> Option<Leaves> {
    match kind {
        MergerKind::Byml | MergerKind::GameData | MergerKind::RsdbRow(_) => byml_leaves(data),
        MergerKind::Msbt => msbt_leaves(data),
        MergerKind::Sarc => sarc_leaves(data),
        _ => None,
    }
}

fn byml_leaves(data: &[u8]) -> Option<Leaves> {
    let root = Byml::from_binary(data).ok()?;
    let mut leaves = Leaves::new();
    collect(&root, &mut String::new(), &mut leaves);
    Some(leaves)
}

fn msbt_leaves(data: &[u8]) -> Option<Leaves> {
    let msbt = Msbt::parse_with(data, Duplicates::KeepLast).ok()?;
    let mut leaves = Leaves::new();
    for (label, entry) in msbt.entries() {
        let mut hash = Fingerprint::default();
        hash.mix(&entry.text);
        hash.mix(entry.attribute.as_deref().unwrap_or(&[]));
        leaves.insert(label.to_string(), hash.value());
    }
    Some(leaves)
}

/// Archives merged file by file: each file is a value, or holds values when
/// it can be merged itself.
fn sarc_leaves(data: &[u8]) -> Option<Leaves> {
    let sarc = Sarc::parse(data).ok()?;
    let mut leaves = Leaves::new();
    for entry in sarc.entries() {
        if entry.data == DELETED_MARK {
            continue;
        }
        let inner = match merger_for(entry.name) {
            Some(kind @ (MergerKind::Byml | MergerKind::Msbt)) => file_leaves(kind, entry.data),
            _ => None,
        };
        match inner {
            Some(inner) => leaves.extend(inner.into_iter().map(|(path, hash)| (format!("{}/{}", entry.name, path), hash))),
            None => {
                let mut hash = Fingerprint::default();
                hash.mix(entry.data);
                leaves.insert(entry.name.to_string(), hash.value());
            }
        }
    }
    Some(leaves)
}

/// Walks a changelog: maps and edited array entries hold values; added
/// entries collide with nothing.
fn collect(node: &Byml, path: &mut String, leaves: &mut Leaves) {
    match node {
        Byml::Map(map) => {
            for (key, value) in map.iter() {
                child(path, key.as_str(), value, leaves);
            }
        }
        Byml::HashMap32(map) => {
            for (key, value) in map.iter() {
                child(path, &format!("{:08x}", key), value, leaves);
            }
        }
        Byml::HashMap64(map) => {
            for (key, value) in map.iter() {
                child(path, &format!("{:016x}", key), value, leaves);
            }
        }
        Byml::ArrayChangelog(changes) => {
            for change in changes {
                let segment = match (&change.key_primary, &change.key_secondary) {
                    (Some(primary), Some(secondary)) => format!("[{}|{}]", key_text(primary), key_text(secondary)),
                    (Some(primary), None) => format!("[{}]", key_text(primary)),
                    _ => format!("[{}]", change.index),
                };
                match change.change {
                    ChangeType::Add => {}
                    ChangeType::Remove => {
                        leaves.insert(joined(path, &segment), REMOVED);
                    }
                    ChangeType::Edit => child(path, &segment, &change.node, leaves),
                }
            }
        }
        _ => {
            leaves.insert(path.clone(), node_hash(node));
        }
    }
}

fn child(path: &mut String, segment: &str, value: &Byml, leaves: &mut Leaves) {
    let length = path.len();
    if !path.is_empty() {
        path.push('/');
    }
    path.push_str(segment);
    if value.is_remove() {
        leaves.insert(path.clone(), REMOVED);
    } else {
        collect(value, path, leaves);
    }
    path.truncate(length);
}

fn joined(path: &str, segment: &str) -> String {
    if path.is_empty() {
        segment.to_string()
    } else {
        format!("{}/{}", path, segment)
    }
}

fn key_text(key: &Byml) -> String {
    match key {
        Byml::String(text) => text.clone(),
        Byml::Int(value) => format!("{}", value),
        Byml::UInt32(value) => format!("{:#x}", value),
        Byml::Int64(value) => format!("{}", value),
        Byml::UInt64(value) => format!("{:#x}", value),
        other => format!("{:016x}", node_hash(other)),
    }
}

fn node_hash(node: &Byml) -> u64 {
    let mut hash = Fingerprint::default();
    mix_node(node, &mut hash);
    hash.value()
}

fn mix_node(node: &Byml, hash: &mut Fingerprint) {
    hash.mix(&[node.node_type()]);
    match node {
        Byml::Null => {}
        Byml::String(text) => hash.mix_str(text),
        Byml::Binary(data) | Byml::BinaryAligned(data, _) => hash.mix(data),
        Byml::Array(items) => items.iter().for_each(|item| mix_node(item, hash)),
        Byml::Map(map) => map.iter().for_each(|(key, value)| {
            hash.mix_str(key.as_str());
            mix_node(value, hash);
        }),
        Byml::HashMap32(map) => map.iter().for_each(|(key, value)| {
            hash.mix(&key.to_le_bytes());
            mix_node(value, hash);
        }),
        Byml::HashMap64(map) => map.iter().for_each(|(key, value)| {
            hash.mix(&key.to_le_bytes());
            mix_node(value, hash);
        }),
        Byml::ArrayChangelog(changes) => changes.iter().for_each(|change| {
            hash.mix(&change.index.to_le_bytes());
            hash.mix(&[change.change as u8]);
            mix_node(&change.node, hash);
        }),
        Byml::Bool(value) => hash.mix(&[*value as u8]),
        Byml::Int(value) => hash.mix(&value.to_le_bytes()),
        Byml::Float(value) => hash.mix(&value.to_bits().to_le_bytes()),
        Byml::UInt32(value) => hash.mix(&value.to_le_bytes()),
        Byml::Int64(value) => hash.mix(&value.to_le_bytes()),
        Byml::UInt64(value) => hash.mix(&value.to_le_bytes()),
        Byml::Double(value) => hash.mix(&value.to_bits().to_le_bytes()),
        Byml::Changelog(change) => hash.mix(&[*change as u8]),
    }
}

/// The values mods set differently: which mods, how many values, a few of
/// them. A mod replacing or removing a whole node collides with the values
/// other mods set inside it.
fn collisions(sets: &[(usize, Leaves)]) -> Option<(BTreeSet<usize>, usize, Vec<String>)> {
    let mut values: BTreeMap<&str, Vec<(usize, u64)>> = BTreeMap::new();
    for (index, leaves) in sets {
        for (path, hash) in leaves {
            values.entry(path.as_str()).or_default().push((*index, *hash));
        }
    }

    let mut involved = BTreeSet::new();
    let mut count = 0;
    let mut samples = Vec::new();
    for (path, set) in &values {
        let mut colliding: BTreeSet<usize> = BTreeSet::new();
        // The mod with the highest priority sets the value; those setting the
        // same one lose nothing.
        let (winner, winning) = *set.iter().max_by_key(|(index, _)| *index).unwrap();
        for (index, hash) in set {
            if *hash != winning {
                colliding.insert(*index);
                colliding.insert(winner);
            }
        }
        // Paths under this one sort right after it.
        for (inner, inner_set) in values.range::<&str, _>((core::ops::Bound::Excluded(path), core::ops::Bound::Unbounded)) {
            if !inner.starts_with(path) {
                break;
            }
            let rest = &inner.as_bytes()[path.len()..];
            if rest.first() != Some(&b'/') {
                continue;
            }
            for (index, _) in inner_set {
                if set.iter().any(|(owner, _)| owner != index) {
                    colliding.insert(*index);
                    colliding.extend(set.iter().map(|(owner, _)| *owner));
                }
            }
        }
        if colliding.len() >= 2 {
            count += 1;
            if samples.len() < SAMPLES {
                samples.push(path.to_string());
            }
            involved.extend(colliding);
        }
    }
    (count > 0).then_some((involved, count, samples))
}

/// One line per conflict in the plugin's log.
pub fn log(conflicts: &[Conflict]) {
    const SHOWN: usize = 40;
    if conflicts.is_empty() {
        return;
    }
    info!(
        "{} conflict(s) between mods (the mod named first wins, the merge goes ahead):",
        conflicts.len()
    );
    for conflict in conflicts.iter().take(SHOWN) {
        let names: Vec<&str> = conflict.mods.iter().map(|m| m.name.as_str()).collect();
        match conflict.kind {
            ConflictKind::File => info!("  {}: replaced by {}", conflict.file, names.join(" > ")),
            ConflictKind::Values => info!(
                "  {}: {} value(s) set by {} ({}{})",
                conflict.file,
                conflict.count,
                names.join(" > "),
                conflict.samples.join(", "),
                if conflict.count > conflict.samples.len() { ", ..." } else { "" }
            ),
        }
    }
    if conflicts.len() > SHOWN {
        info!("  ... and {} more", conflicts.len() - SHOWN);
    }
}

fn field(text: &str) -> String {
    text.chars().map(|c| if matches!(c, '\t' | '\n' | '\r' | '|') { ' ' } else { c }).collect()
}

/// `kind, file, count, folders, names, samples`, tab separated, lists
/// separated by `|`.
pub fn to_tsv(conflicts: &[Conflict]) -> String {
    let mut text = String::new();
    for conflict in conflicts {
        let folders: Vec<String> = conflict.mods.iter().map(|m| field(&m.folder)).collect();
        let names: Vec<String> = conflict.mods.iter().map(|m| field(&m.name)).collect();
        let samples: Vec<String> = conflict.samples.iter().map(|s| field(s)).collect();
        text.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\n",
            conflict.kind.as_str(),
            field(&conflict.file),
            conflict.count,
            folders.join("|"),
            names.join("|"),
            samples.join("|")
        ));
    }
    text
}

pub fn parse_tsv(text: &str) -> Vec<Conflict> {
    let list = |value: &str| -> Vec<String> {
        value.split('|').filter(|s| !s.is_empty()).map(str::to_string).collect()
    };
    let mut conflicts = Vec::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() < 6 {
            continue;
        }
        let kind = match fields[0] {
            "file" => ConflictKind::File,
            "values" => ConflictKind::Values,
            _ => continue,
        };
        let folders = list(fields[3]);
        let names = list(fields[4]);
        conflicts.push(Conflict {
            kind,
            file: fields[1].to_string(),
            count: fields[2].parse().unwrap_or(1),
            mods: folders
                .iter()
                .enumerate()
                .map(|(i, folder)| ModRef {
                    folder: folder.clone(),
                    name: names.get(i).cloned().unwrap_or_else(|| folder.clone()),
                })
                .collect(),
            samples: list(fields[5]),
        });
    }
    conflicts
}

pub fn save(cache_dir: &str, conflicts: &[Conflict]) -> fs::Result<()> {
    fs::create_dir_all(cache_dir)?;
    fs::write(&path::join(cache_dir, FILE_NAME), to_tsv(conflicts).as_bytes())
}

/// The conflicts the last merge found.
pub fn load(cache_dir: &str) -> Vec<Conflict> {
    fs::read_to_string(&path::join(cache_dir, FILE_NAME))
        .map(|text| parse_tsv(&text))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use totk_formats::byml::ArrayChange;

    fn map(entries: &[(&str, Byml)]) -> Byml {
        Byml::Map(entries.iter().map(|(k, v)| (k.to_string(), v.clone())).collect())
    }

    fn leaves(node: &Byml) -> Leaves {
        let mut leaves = Leaves::new();
        collect(node, &mut String::new(), &mut leaves);
        leaves
    }

    #[test]
    fn different_values_collide_equal_ones_do_not() {
        let a = leaves(&map(&[("Life", Byml::Int(10)), ("Speed", Byml::Float(1.0))]));
        let b = leaves(&map(&[("Life", Byml::Int(20)), ("Speed", Byml::Float(1.0))]));
        let (involved, count, samples) = collisions(&[(0, a), (1, b)]).unwrap();
        assert_eq!(involved.into_iter().collect::<Vec<_>>(), vec![0, 1]);
        assert_eq!(count, 1);
        assert_eq!(samples, vec!["Life".to_string()]);

        let a = leaves(&map(&[("Life", Byml::Int(10))]));
        let b = leaves(&map(&[("Speed", Byml::Int(3))]));
        assert!(collisions(&[(0, a.clone()), (1, b)]).is_none());
        assert!(collisions(&[(0, a.clone()), (1, a)]).is_none());
    }

    #[test]
    fn mods_agreeing_with_the_winner_lose_nothing() {
        let one = leaves(&map(&[("Life", Byml::Int(1))]));
        let two = leaves(&map(&[("Life", Byml::Int(2))]));
        // 0 and 2 set the same value, 2 wins: only 1 loses its value.
        let (involved, _, _) = collisions(&[(0, one.clone()), (1, two), (2, one)]).unwrap();
        assert_eq!(involved.into_iter().collect::<Vec<_>>(), vec![1, 2]);
    }

    #[test]
    fn additions_do_not_collide_edits_do() {
        let change = |kind: ChangeType, index: i32, node: Byml| ArrayChange {
            index,
            change: kind,
            node,
            key_primary: None,
            key_secondary: None,
        };
        let a = leaves(&map(&[(
            "Items",
            Byml::ArrayChangelog(vec![change(ChangeType::Add, 3, Byml::Int(1)), change(ChangeType::Edit, 0, Byml::Int(5))]),
        )]));
        let b = leaves(&map(&[(
            "Items",
            Byml::ArrayChangelog(vec![change(ChangeType::Add, 3, Byml::Int(2)), change(ChangeType::Edit, 1, Byml::Int(6))]),
        )]));
        assert!(collisions(&[(0, a.clone()), (1, b)]).is_none());

        let c = leaves(&map(&[("Items", Byml::ArrayChangelog(vec![change(ChangeType::Remove, 0, Byml::Null)]))]));
        let (_, count, samples) = collisions(&[(0, a), (1, c)]).unwrap();
        assert_eq!((count, samples), (1, vec!["Items/[0]".to_string()]));
    }

    #[test]
    fn replacing_a_node_collides_with_values_inside_it() {
        let a = leaves(&map(&[("Param", map(&[("Attack", Byml::Int(5))]))]));
        let b = leaves(&map(&[("Param", Byml::Changelog(ChangeType::Remove)), ("Param2", Byml::Int(1))]));
        let (involved, count, samples) = collisions(&[(0, a), (1, b)]).unwrap();
        assert_eq!(involved.len(), 2);
        assert_eq!((count, samples), (1, vec!["Param".to_string()]));

        // "Param2" starts with "Param" but is not inside it.
        let a = leaves(&map(&[("Param", Byml::Int(5))]));
        let b = leaves(&map(&[("Param2", map(&[("X", Byml::Int(1))]))]));
        assert!(collisions(&[(0, a), (1, b)]).is_none());
    }

    #[test]
    fn tsv_round_trip() {
        let conflicts = vec![
            Conflict {
                kind: ConflictKind::File,
                file: "Model/Foo.bfres".into(),
                mods: vec![
                    ModRef { folder: "B".into(), name: "Mod | B".into() },
                    ModRef { folder: "A".into(), name: "Mod A".into() },
                ],
                count: 1,
                samples: vec![],
            },
            Conflict {
                kind: ConflictKind::Values,
                file: "Mals/EUfr/Npc.msbt".into(),
                mods: vec![ModRef { folder: "C".into(), name: "C".into() }, ModRef { folder: "A".into(), name: "A".into() }],
                count: 4,
                samples: vec!["Talk_01".into(), "Talk_02".into()],
            },
        ];
        let parsed = parse_tsv(&to_tsv(&conflicts));
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].mods[0].name, "Mod   B");
        assert_eq!(parsed[1], conflicts[1]);
    }
}
