//! Resource databases (`RSDB/*.rstbl.byml`): row tables and the tag table.
//!
//! Ports of RsdbRowChangelogBuilder, RsdbRowMerger, RsdbTagChangelogBuilder,
//! RsdbTagMerger and RsdbTagTable.

use alloc::collections::{BTreeMap, BTreeSet};
use core::cmp::Ordering;

use hashbrown::HashMap;
use totk_formats::byml::{ArrayChange, Byml, ChangeType, Document, Map, NodeRef};

use crate::byml_changelog::{log_changes_inline, TrackingInfo};
use crate::byml_merge::{self, MergeResult, Segment, Tracking};
use crate::prelude::*;
use crate::{debug, info};

/// The field rows of a table are keyed by, or `None` if the file is not a row
/// table TKMM merges.
pub fn row_key(canonical: &str) -> Option<&'static str> {
    Some(match canonical {
        "RSDB/GameSafetySetting.Product.rstbl.byml" => "NameHash",
        "RSDB/RumbleCall.Product.rstbl.byml" | "RSDB/UIScreen.Product.rstbl.byml" => "Name",
        "RSDB/TagDef.Product.rstbl.byml" => "FullTagId",
        "RSDB/ActorInfo.Product.rstbl.byml"
        | "RSDB/AttachmentActorInfo.Product.rstbl.byml"
        | "RSDB/Challenge.Product.rstbl.byml"
        | "RSDB/EnhancementMaterialInfo.Product.rstbl.byml"
        | "RSDB/EventPlayEnvSetting.Product.rstbl.byml"
        | "RSDB/EventSetting.Product.rstbl.byml"
        | "RSDB/GameActorInfo.Product.rstbl.byml"
        | "RSDB/GameAnalyzedEventInfo.Product.rstbl.byml"
        | "RSDB/GameEventBaseSetting.Product.rstbl.byml"
        | "RSDB/GameEventMetadata.Product.rstbl.byml"
        | "RSDB/LoadingTips.Product.rstbl.byml"
        | "RSDB/Location.Product.rstbl.byml"
        | "RSDB/LocatorData.Product.rstbl.byml"
        | "RSDB/PouchActorInfo.Product.rstbl.byml"
        | "RSDB/XLinkPropertyTable.Product.rstbl.byml"
        | "RSDB/XLinkPropertyTableList.Product.rstbl.byml" => "__RowId",
        _ => return None,
    })
}

pub const TAG_TABLE: &str = "RSDB/Tag.Product.rstbl.byml";

/// A row key: a string for most tables, a hash for GameSafetySetting.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum RowKey {
    Name(String),
    Hash(u32),
}

impl RowKey {
    fn of(row: &Byml, key: &str) -> Option<RowKey> {
        match row.as_map()?.get(key)? {
            Byml::String(name) => Some(RowKey::Name(name.clone())),
            Byml::UInt32(hash) => Some(RowKey::Hash(*hash)),
            _ => None,
        }
    }
}

fn utf16_cmp(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// BymlRowComparer.
fn compare_rows(a: &Byml, b: &Byml, key: &str) -> Ordering {
    let value = |row: &Byml| row.as_map().and_then(|m| m.get(key)).cloned();
    match (value(a), value(b)) {
        (Some(Byml::String(x)), Some(Byml::String(y))) => utf16_cmp(&x, &y),
        (Some(Byml::Int64(x)), Some(Byml::Int64(y))) => x.cmp(&y),
        (Some(Byml::UInt64(x)), Some(Byml::UInt64(y))) => x.cmp(&y),
        (Some(Byml::UInt32(x)), Some(Byml::UInt32(y))) => x.cmp(&y),
        (Some(Byml::Int(x)), Some(Byml::Int(y))) => x.cmp(&y),
        _ => Ordering::Equal,
    }
}

// --- rows --------------------------------------------------------------------

/// Changelog of a modded row table: changed and new rows, keyed. Rows are read
/// one at a time (ActorInfo alone has 400 000 nodes).
pub fn build_row_changelog(canonical: &str, key: &str, src: &[u8], vanilla: &[u8]) -> Result<Option<Vec<u8>>, String> {
    let error = |e: totk_formats::Error| e.to_string();
    let src = Document::open(src).map_err(error)?;
    let vanilla = Document::open(vanilla).map_err(error)?;
    let format = src.format();

    let rows_of = |document: &Document| -> Result<Vec<NodeRef>, String> {
        let root = document.root().ok_or("empty RSDB table")?;
        document
            .array_items(root)
            .map_err(|_| "RSDB table is not an array of rows".to_string())
    };
    let (rows, vanilla_rows) = (rows_of(&src)?, rows_of(&vanilla)?);

    let key_of = |document: &Document, row: NodeRef| -> Result<Option<RowKey>, String> {
        let Some(field) = document.map_get(row, key).map_err(error)? else {
            return Ok(None);
        };
        Ok(match document.load(field).map_err(error)? {
            Byml::String(name) => Some(RowKey::Name(name)),
            Byml::UInt32(hash) => Some(RowKey::Hash(hash)),
            _ => None,
        })
    };

    let mut index: HashMap<RowKey, usize> = HashMap::with_capacity(vanilla_rows.len());
    for (i, row) in vanilla_rows.iter().enumerate() {
        if let Some(row_key) = key_of(&vanilla, *row)? {
            index.insert(row_key, i);
        }
    }

    let mut info = TrackingInfo::for_canonical(canonical);
    let mut changelog: BTreeMap<RowKey, Byml> = BTreeMap::new();

    for (i, row_ref) in rows.into_iter().enumerate() {
        let Some(row_key) = key_of(&src, row_ref)? else {
            info!("{}: row {} has no {} field, skipped", canonical, i, key);
            continue;
        };
        let mut row = src.load(row_ref).map_err(error)?;
        if let Some(&vanilla_index) = index.get(&row_key) {
            let vanilla_row = vanilla.load(vanilla_rows[vanilla_index]).map_err(error)?;
            if log_changes_inline(&mut info, &mut row, &vanilla_row) {
                continue;
            }
        }
        changelog.insert(row_key, row);
    }

    if changelog.is_empty() {
        return Ok(None);
    }

    let document = if key == "NameHash" {
        Byml::HashMap32(
            changelog
                .into_iter()
                .filter_map(|(k, v)| match k {
                    RowKey::Hash(hash) => Some((hash, v)),
                    RowKey::Name(_) => None,
                })
                .collect(),
        )
    } else {
        Byml::Map(
            changelog
                .into_iter()
                .filter_map(|(k, v)| match k {
                    RowKey::Name(name) => Some((name, v)),
                    RowKey::Hash(_) => None,
                })
                .collect(),
        )
    };
    Ok(Some(document.write(format)))
}

/// RsdbRowMerger.
pub fn merge_rows(canonical: &str, key: &str, vanilla: Vec<u8>, changelogs: &[Vec<u8>]) -> MergeResult<Vec<u8>> {
    let (mut root, format) = Byml::parse(&vanilla).map_err(|e| e.to_string())?;
    drop(vanilla);
    let mut tracking = Tracking::new(canonical);

    for changelog in changelogs {
        let changelog = Byml::from_binary(changelog).map_err(|e| e.to_string())?;
        let Byml::Array(rows) = &mut root else {
            return Err("RSDB table is not an array of rows".into());
        };
        let mut entries: BTreeMap<RowKey, Byml> = match changelog {
            Byml::Map(map) => map.into_iter().map(|(k, v)| (RowKey::Name(k.to_string()), v)).collect(),
            Byml::HashMap32(map) => map.into_iter().map(|(k, v)| (RowKey::Hash(k), v)).collect(),
            _ => continue,
        };

        let mut i = 0;
        while i < rows.len() {
            let Some(row_key) = RowKey::of(&rows[i], key) else {
                i += 1;
                continue;
            };
            let Some(entry) = entries.remove(&row_key) else {
                i += 1;
                continue;
            };

            let removal_key = format!("{:?}", row_key);
            let tracked = tracking.array_entry(&Vec::new());
            if entry.is_remove() {
                tracked.keyed_removals.insert(removal_key, i);
                i += 1;
                continue;
            }
            if tracked.keyed_removals.remove(&removal_key).is_some() {
                info!(
                    "{}: row {:?} was removed by one mod and modified by another; it has been kept",
                    canonical, row_key
                );
            }

            let (Some(base_row), Byml::Map(changes)) = (rows[i].as_map_mut(), entry) else {
                i += 1;
                continue;
            };
            let mut path = vec![Segment::Index(i)];
            byml_merge::merge_map(base_row, changes, &mut tracking, &mut path, |k| Segment::Key(k.to_string()))?;
            i += 1;
        }

        rows.extend(entries.into_values().filter(|row| !row.is_remove()));
    }

    tracking.apply(&mut root)?;
    if let Byml::Array(rows) = &mut root {
        rows.sort_by(|a, b| compare_rows(a, b, key));
    }
    Ok(root.write(format))
}

// --- tags ----------------------------------------------------------------------

const PATH_LIST: &str = "PathList";
const TAG_LIST: &str = "TagList";
const BIT_TABLE: &str = "BitTable";
const RANK_TABLE: &str = "RankTable";

type EntryKey = (String, String, String);

fn binary(node: Option<&Byml>) -> &[u8] {
    match node {
        Some(Byml::Binary(data)) | Some(Byml::BinaryAligned(data, _)) => data,
        _ => &[],
    }
}

fn strings(node: Option<&Byml>) -> Vec<String> {
    node.and_then(|n| n.as_array())
        .map(|items| items.iter().map(|i| i.as_str().unwrap_or_default().to_string()).collect())
        .unwrap_or_default()
}

/// RsdbTagTable.GetEntryTags: tags whose bit is set for an entry.
fn entry_tags(entry_index: usize, tags: &[String], bits: &[u8]) -> Vec<String> {
    let start = entry_index * tags.len();
    tags.iter()
        .enumerate()
        .filter(|(t, _)| {
            let bit = start + t;
            bits.get(bit / 8).map_or(false, |byte| (byte >> (bit % 8)) & 1 == 1)
        })
        .map(|(_, tag)| tag.clone())
        .collect()
}

fn entry_keys(paths: &[Byml]) -> Vec<Option<EntryKey>> {
    paths
        .chunks(3)
        .map(|chunk| match chunk {
            [Byml::String(a), Byml::String(b), Byml::String(c)] => Some((a.clone(), b.clone(), c.clone())),
            _ => None,
        })
        .collect()
}

/// Changelog of a modded tag table: per entry, tags added and removed.
pub fn build_tag_changelog(src: &[u8], vanilla: &[u8]) -> Result<Option<Vec<u8>>, String> {
    let vanilla = Byml::from_binary(vanilla).map_err(|e| e.to_string())?;
    let vanilla = vanilla.as_map().ok_or("tag table is not a map")?;
    let vanilla_paths = vanilla.get(PATH_LIST).and_then(|n| n.as_array()).ok_or("tag table without paths")?;
    let vanilla_tags = strings(vanilla.get(TAG_LIST));
    let vanilla_bits = binary(vanilla.get(BIT_TABLE));
    let vanilla_entries: HashMap<EntryKey, usize> = entry_keys(vanilla_paths)
        .into_iter()
        .enumerate()
        .filter_map(|(i, key)| key.map(|k| (k, i)))
        .collect();
    let vanilla_tag_set: BTreeSet<&str> = vanilla_tags.iter().map(|s| s.as_str()).collect();

    let (src, format) = Byml::parse(src).map_err(|e| e.to_string())?;
    let src = src.as_map().ok_or("tag table is not a map")?;
    let paths = src.get(PATH_LIST).and_then(|n| n.as_array()).ok_or("tag table without paths")?;
    let tags_node = src.get(TAG_LIST).and_then(|n| n.as_array()).cloned().unwrap_or_default();
    let tags = strings(src.get(TAG_LIST));
    let bits = binary(src.get(BIT_TABLE));

    let mut changelog: Vec<Byml> = Vec::new();
    for (entry_index, chunk) in paths.chunks(3).enumerate() {
        if chunk.len() < 3 {
            break;
        }
        let tags_of_entry = entry_tags(entry_index, &tags, bits);
        let key = match chunk {
            [Byml::String(a), Byml::String(b), Byml::String(c)] => Some((a.clone(), b.clone(), c.clone())),
            _ => None,
        };

        match key.and_then(|k| vanilla_entries.get(&k).copied()) {
            None => {
                changelog.extend(chunk.iter().cloned());
                changelog.push(Byml::Array(tags_of_entry.into_iter().map(Byml::String).collect()));
            }
            Some(vanilla_index) => {
                let mut vanilla_entry_tags = entry_tags(vanilla_index, &vanilla_tags, vanilla_bits);
                vanilla_entry_tags.sort_by(|a, b| utf16_cmp(a, b));
                vanilla_entry_tags.dedup();
                let changes = tag_changes(&vanilla_entry_tags, &tags_of_entry);
                if !changes.is_empty() {
                    changelog.extend(chunk.iter().cloned());
                    changelog.push(Byml::ArrayChangelog(changes));
                }
            }
        }
    }

    let new_tags: Vec<Byml> = tags_node
        .into_iter()
        .filter(|tag| !matches!(tag, Byml::String(s) if vanilla_tag_set.contains(s.as_str())))
        .collect();

    if changelog.is_empty() && new_tags.is_empty() {
        return Ok(None);
    }

    let mut result = Map::new();
    result.insert("Entries".into(), Byml::Array(changelog));
    result.insert("Tags".into(), Byml::Array(new_tags));
    Ok(Some(Byml::Map(result).write(format)))
}

/// RsdbTagChangelogBuilder.CreateChangelog: a sorted-list diff.
fn tag_changes(vanilla: &[String], modded: &[String]) -> Vec<ArrayChange> {
    let change = |kind: ChangeType, tag: &str| ArrayChange {
        index: 0,
        change: kind,
        node: Byml::String(tag.to_string()),
        key_primary: None,
        key_secondary: None,
    };

    let mut changes = Vec::new();
    let mut vi = 0usize;
    let mut mi = 0usize;
    while mi < modded.len() {
        if vi >= vanilla.len() {
            changes.push(change(ChangeType::Add, &modded[mi]));
            mi += 1;
            continue;
        }
        match utf16_cmp(&vanilla[vi], &modded[mi]) {
            Ordering::Equal => {
                vi += 1;
                mi += 1;
            }
            Ordering::Less => {
                changes.push(change(ChangeType::Remove, &vanilla[vi]));
                vi += 1;
            }
            Ordering::Greater => {
                changes.push(change(ChangeType::Add, &modded[mi]));
                mi += 1;
            }
        }
    }
    for tag in &vanilla[vi.min(vanilla.len())..] {
        changes.push(change(ChangeType::Remove, tag));
    }
    changes
}

/// RsdbTagMerger.
pub fn merge_tags(vanilla: &[u8], changelogs: &[Vec<u8>]) -> MergeResult<Vec<u8>> {
    let (root, format) = Byml::parse(vanilla).map_err(|e| e.to_string())?;
    let root = root.as_map().ok_or("tag table is not a map")?;
    let paths = root.get(PATH_LIST).and_then(|n| n.as_array()).ok_or("tag table without paths")?;
    let mut tags = strings(root.get(TAG_LIST));
    let bits = binary(root.get(BIT_TABLE)).to_vec();
    let rank_table = root.get(RANK_TABLE).cloned().unwrap_or(Byml::Binary(Vec::new()));

    let mut entries: HashMap<EntryKey, Vec<String>> = HashMap::new();
    for (entry_index, key) in entry_keys(paths).into_iter().enumerate() {
        if let Some(key) = key {
            entries.insert(key, entry_tags(entry_index, &tags, &bits));
        }
    }

    for changelog in changelogs {
        let changelog = Byml::from_binary(changelog).map_err(|e| e.to_string())?;
        let Some(changelog) = changelog.as_map() else {
            continue;
        };
        tags.extend(strings(changelog.get("Tags")));

        let Some(items) = changelog.get("Entries").and_then(|n| n.as_array()) else {
            continue;
        };
        for chunk in items.chunks(4) {
            let [Byml::String(prefix), Byml::String(name), Byml::String(suffix), change] = chunk else {
                debug!("malformed tag changelog entry");
                continue;
            };
            let entry_tags = entries
                .entry((prefix.clone(), name.clone(), suffix.clone()))
                .or_default();
            match change {
                Byml::Array(added) => entry_tags.extend(added.iter().filter_map(|t| t.as_str().map(str::to_string))),
                Byml::ArrayChangelog(changes) => {
                    for change in changes {
                        let Byml::String(tag) = &change.node else {
                            continue;
                        };
                        if change.change == ChangeType::Remove {
                            if let Some(position) = entry_tags.iter().position(|t| t == tag) {
                                entry_tags.remove(position);
                            }
                        } else {
                            entry_tags.push(tag.clone());
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // RsdbTagTable.Compile
    let mut sorted: Vec<(EntryKey, Vec<String>)> = entries.into_iter().collect();
    sorted.sort_by(|a, b| {
        utf16_cmp(&a.0 .0, &b.0 .0)
            .then_with(|| utf16_cmp(&a.0 .1, &b.0 .1))
            .then_with(|| utf16_cmp(&a.0 .2, &b.0 .2))
    });

    tags.sort_by(|a, b| utf16_cmp(a, b));
    tags.dedup();
    let tag_index: HashMap<&str, usize> = tags.iter().enumerate().map(|(i, t)| (t.as_str(), i)).collect();

    let mut bit_table = vec![0u8; (tags.len() * sorted.len() + 7) / 8];
    let mut path_list = Vec::with_capacity(sorted.len() * 3);
    for (entry_index, ((prefix, name, suffix), entry_tags)) in sorted.iter().enumerate() {
        path_list.push(Byml::String(prefix.clone()));
        path_list.push(Byml::String(name.clone()));
        path_list.push(Byml::String(suffix.clone()));
        for tag in entry_tags {
            let Some(&t) = tag_index.get(tag.as_str()) else {
                return Err(format!("tag '{}' is used by an entry but missing from the tag list", tag));
            };
            let bit = entry_index * tags.len() + t;
            bit_table[bit / 8] |= 1 << (bit % 8);
        }
    }
    // It borrows `tags`, which moves below.
    drop(tag_index);

    let mut result = Map::new();
    result.insert(BIT_TABLE.into(), Byml::Binary(bit_table));
    result.insert(PATH_LIST.into(), Byml::Array(path_list));
    result.insert(RANK_TABLE.into(), rank_table);
    result.insert(TAG_LIST.into(), Byml::Array(tags.into_iter().map(Byml::String).collect()));
    Ok(Byml::Map(result).write(format))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, value: i32) -> Byml {
        Byml::Map(Map::from_iter::<Vec<(totk_formats::byml::Key, Byml)>>(vec![
            ("__RowId".into(), Byml::from(id)),
            ("Value".into(), Byml::Int(value)),
        ]))
    }

    fn table(rows: Vec<Byml>) -> Vec<u8> {
        Byml::Array(rows).to_binary(7)
    }

    #[test]
    fn rows_from_several_mods_merge() {
        let canonical = "RSDB/ActorInfo.Product.rstbl.byml";
        let vanilla = table(vec![row("A", 1), row("B", 2), row("C", 3)]);
        let mod_a = table(vec![row("A", 10), row("B", 2), row("C", 3)]);
        let mod_b = table(vec![row("A", 1), row("B", 2), row("C", 30), row("D", 4)]);

        let changelogs: Vec<Vec<u8>> = [mod_a, mod_b]
            .iter()
            .map(|m| build_row_changelog(canonical, "__RowId", m, &vanilla).unwrap().unwrap())
            .collect();
        let merged = merge_rows(canonical, "__RowId", vanilla.clone(), &changelogs).unwrap();
        assert_eq!(
            Byml::from_binary(&merged).unwrap(),
            Byml::Array(vec![row("A", 10), row("B", 2), row("C", 30), row("D", 4)])
        );
    }

    fn tag_table(entries: &[(&str, &[&str])], tags: &[&str]) -> Vec<u8> {
        let mut paths = Vec::new();
        let mut bits = vec![0u8; (entries.len() * tags.len() + 7) / 8];
        for (e, (name, entry_tags)) in entries.iter().enumerate() {
            paths.extend([Byml::from("Actor/"), Byml::from(*name), Byml::from(".engine__actor__ActorParam.gyml")]);
            for tag in *entry_tags {
                let t = tags.iter().position(|x| x == tag).unwrap();
                let bit = e * tags.len() + t;
                bits[bit / 8] |= 1 << (bit % 8);
            }
        }
        Byml::Map(Map::from_iter::<Vec<(totk_formats::byml::Key, Byml)>>(vec![
            (BIT_TABLE.into(), Byml::Binary(bits)),
            (PATH_LIST.into(), Byml::Array(paths)),
            (RANK_TABLE.into(), Byml::Binary(vec![1, 2, 3])),
            (TAG_LIST.into(), Byml::Array(tags.iter().map(|t| Byml::from(*t)).collect())),
        ]))
        .to_binary(7)
    }

    #[test]
    fn tag_edits_from_several_mods_merge() {
        let vanilla = tag_table(&[("Apple", &["Food"]), ("Sword", &["Weapon"])], &["Food", "Weapon"]);
        // Mod A tags the apple as a weapon; mod B adds a new tag to the sword.
        let mod_a = tag_table(&[("Apple", &["Food", "Weapon"]), ("Sword", &["Weapon"])], &["Food", "Weapon"]);
        let mod_b = tag_table(&[("Apple", &["Food"]), ("Sword", &["Sharp", "Weapon"])], &["Food", "Sharp", "Weapon"]);

        let changelogs: Vec<Vec<u8>> = [mod_a, mod_b]
            .iter()
            .map(|m| build_tag_changelog(m, &vanilla).unwrap().unwrap())
            .collect();
        let merged = merge_tags(&vanilla, &changelogs).unwrap();
        let expected = tag_table(
            &[("Apple", &["Food", "Weapon"]), ("Sword", &["Sharp", "Weapon"])],
            &["Food", "Sharp", "Weapon"],
        );
        assert_eq!(Byml::from_binary(&merged).unwrap(), Byml::from_binary(&expected).unwrap());
    }
}
