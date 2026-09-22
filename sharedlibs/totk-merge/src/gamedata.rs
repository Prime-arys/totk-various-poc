//! GameDataList (`GameData/GameDataList.Product.*.byml`): the save data
//! schema. Ports of GameDataChangelogBuilder, GameDataMerger,
//! GameDataMergeTracking and SaveDataWriter.

use alloc::collections::BTreeMap;

use hashbrown::{HashMap, HashSet};
use totk_formats::byml::{node_type, Byml, ChangeType, Document, Map, NodeRef};

use crate::byml_changelog::{default_provider, log_changes_inline, log_changes_with, TrackingInfo};
use crate::byml_keys::{ArrayBuilder, KeyName};
use crate::byml_merge::{self, MergeResult, Segment, Tracking};
use crate::prelude::*;

pub const CANONICAL: &str = "GameData/GameDataList.Product.byml";

fn gdl_provider(_: &TrackingInfo, _: &str) -> ArrayBuilder {
    ArrayBuilder::DirectIndex
}

fn struct_provider(_: &TrackingInfo, name: &str) -> ArrayBuilder {
    match name {
        "DefaultValue" => ArrayBuilder::Keyed(KeyName {
            primary: "Hash",
            secondary: None,
        }),
        _ => ArrayBuilder::Default,
    }
}

fn hash32(row: &Byml) -> Option<u32> {
    match row.as_map()?.get("Hash")? {
        Byml::UInt32(hash) => Some(*hash),
        _ => None,
    }
}

fn hash64(row: &Byml) -> Option<u64> {
    match row.as_map()?.get("Hash")? {
        Byml::UInt64(hash) => Some(*hash),
        _ => None,
    }
}

/// Changelog of a modded GameDataList: per table, changed/new rows by hash and
/// removal markers.
///
/// Both documents are read row by row rather than as trees: a GameDataList
/// parsed whole takes tens of megabytes, twice over here, which is more than
/// the console can spare while the game boots.
pub fn build_changelog(src: &[u8], vanilla: &[u8]) -> Result<Option<Vec<u8>>, String> {
    let error = |e: totk_formats::Error| e.to_string();
    let src = Document::open(src).map_err(error)?;
    let vanilla = Document::open(vanilla).map_err(error)?;
    let format = vanilla.format();

    let data_of = |document: &Document| -> Result<NodeRef, String> {
        let root = document.root().ok_or("empty GameDataList")?;
        document
            .map_get(root, "Data")
            .map_err(error)?
            .ok_or_else(|| "GameDataList without a Data map".to_string())
    };
    let (src_tables, vanilla_tables) = (data_of(&src)?, data_of(&vanilla)?);

    let mut info = TrackingInfo::for_canonical(CANONICAL);
    let mut changelog = Map::new();

    for (table_name, table) in src.map_entries(src_tables).map_err(error)? {
        if table.node_type != node_type::ARRAY {
            continue;
        }
        let Some(vanilla_table) = vanilla.map_get(vanilla_tables, &table_name).map_err(error)? else {
            return Err(format!("vanilla GameDataList has no '{}' table", table_name));
        };

        let wide = table_name.as_str() == "Bool64bitKey";
        let provider: fn(&TrackingInfo, &str) -> ArrayBuilder = if wide {
            default_provider
        } else if table_name.as_str() == "Struct" {
            struct_provider
        } else {
            gdl_provider
        };

        if let Some(logged) = log_rows(&mut info, &src, table, &vanilla, vanilla_table, wide, provider)? {
            changelog.insert(table_name, logged);
        }
    }

    if changelog.is_empty() {
        return Ok(None);
    }
    Ok(Some(Byml::Map(changelog).write(format)))
}

/// A row's hash: UInt32, or UInt64 in the 64-bit table.
fn row_hash(document: &Document, row: NodeRef, wide: bool) -> Result<Option<u64>, String> {
    let Some(hash) = document.map_get(row, "Hash").map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    Ok(match (document.load(hash).map_err(|e| e.to_string())?, wide) {
        (Byml::UInt32(hash), false) => Some(hash as u64),
        (Byml::UInt64(hash), true) => Some(hash),
        _ => None,
    })
}

/// LogEntries / LogUInt64Entries.
fn log_rows(
    info: &mut TrackingInfo,
    src: &Document,
    table: NodeRef,
    vanilla: &Document,
    vanilla_table: NodeRef,
    wide: bool,
    provider: fn(&TrackingInfo, &str) -> ArrayBuilder,
) -> Result<Option<Byml>, String> {
    let error = |e: totk_formats::Error| e.to_string();
    let vanilla_rows = vanilla.array_items(vanilla_table).map_err(error)?;
    let mut index: HashMap<u64, usize> = HashMap::with_capacity(vanilla_rows.len());
    let mut vanilla_hashes = Vec::with_capacity(vanilla_rows.len());
    for (i, row) in vanilla_rows.iter().enumerate() {
        if let Some(hash) = row_hash(vanilla, *row, wide)? {
            index.insert(hash, i);
            vanilla_hashes.push(hash);
        }
    }

    let mut changelog: BTreeMap<u64, Byml> = BTreeMap::new();
    let mut expected = HashSet::new();
    let mut kept = 0usize;

    for row_ref in src.array_items(table).map_err(error)? {
        let Some(hash) = row_hash(src, row_ref, wide)? else {
            kept += 1;
            continue;
        };
        let mut row = src.load(row_ref).map_err(error)?;
        let Some(&vanilla_index) = index.get(&hash) else {
            changelog.insert(hash, row);
            continue;
        };
        kept += 1;
        expected.insert(hash);
        let vanilla_row = vanilla.load(vanilla_rows[vanilla_index]).map_err(error)?;
        if !log_changes_with(info, &mut row, &vanilla_row, provider) {
            changelog.insert(hash, row);
        }
    }

    if kept != vanilla_rows.len() {
        for hash in vanilla_hashes {
            if !expected.remove(&hash) {
                changelog.insert(hash, Byml::Changelog(ChangeType::Remove));
            }
        }
    }

    if changelog.is_empty() {
        return Ok(None);
    }
    Ok(Some(if wide {
        Byml::HashMap64(changelog.into_iter().collect())
    } else {
        Byml::HashMap32(changelog.into_iter().map(|(hash, row)| (hash as u32, row)).collect())
    }))
}

/// A row a changelog added; later changelogs that edit it are merged into it
/// once everything has been read.
struct AddedRow {
    table: String,
    hash: u64,
    is_struct: bool,
    changes: Vec<Byml>,
}

/// GameDataMerger.
pub fn merge(vanilla: Vec<u8>, changelogs: &[Vec<u8>]) -> MergeResult<Vec<u8>> {
    let (mut root, format) = Byml::parse(&vanilla).map_err(|e| e.to_string())?;
    drop(vanilla);
    let mut tracking = Tracking::new(CANONICAL);
    let mut added: Vec<AddedRow> = Vec::new();
    // TKMM sorts the tables a changelog touches, and only those.
    let mut touched: alloc::collections::BTreeSet<String> = alloc::collections::BTreeSet::new();
    let mut dropped: BTreeMap<(String, u64), ()> = BTreeMap::new();

    for changelog in changelogs {
        let changelog = Byml::from_binary(changelog).map_err(|e| e.to_string())?;
        let Byml::Map(changelog) = changelog else {
            return Err("GameDataList changelog is not a map".into());
        };
        let tables = root
            .as_map_mut()
            .and_then(|m| m.get_mut("Data"))
            .and_then(|d| d.as_map_mut())
            .ok_or("GameDataList without a Data map")?;

        for (table_name, entry) in changelog {
            let table_name = table_name.to_string();
            touched.insert(table_name.clone());
            let Some(Byml::Array(rows)) = tables.get_mut(table_name.as_str()) else {
                return Err(format!("GameDataList has no '{}' table", table_name));
            };
            let is_struct = table_name == "Struct";
            let wide = matches!(entry, Byml::HashMap64(_));
            let mut entries: BTreeMap<u64, Byml> = match entry {
                Byml::HashMap32(map) => map.into_iter().map(|(k, v)| (k as u64, v)).collect(),
                Byml::HashMap64(map) => map.into_iter().collect(),
                _ => return Err(format!("invalid GameDataList changelog for '{}'", table_name)),
            };
            let is_struct = is_struct && !wide;

            for i in 0..rows.len() {
                let hash = if wide { hash64(&rows[i]) } else { hash32(&rows[i]).map(|h| h as u64) };
                let Some(hash) = hash else {
                    continue;
                };
                let Some(change) = entries.remove(&hash) else {
                    continue;
                };

                let key = (table_name.clone(), hash);
                if change.is_remove() {
                    dropped.insert(key, ());
                    continue;
                }
                dropped.remove(&key);

                if let Some(row) = added.iter_mut().find(|r| r.table == table_name && r.hash == hash) {
                    row.changes.push(change);
                    continue;
                }

                let mut path = vec![Segment::Key("Data".into()), Segment::Key(table_name.clone()), Segment::Index(i)];
                byml_merge::merge(&mut rows[i], change, &mut tracking, &mut path)?;
            }

            for (hash, row) in entries {
                if row.is_remove() {
                    continue;
                }
                rows.push(row);
                added.push(AddedRow {
                    table: table_name.clone(),
                    hash,
                    is_struct,
                    changes: Vec::new(),
                });
            }
        }
    }

    tracking.apply(&mut root)?;

    let tables = root
        .as_map_mut()
        .and_then(|m| m.get_mut("Data"))
        .and_then(|d| d.as_map_mut())
        .ok_or("GameDataList without a Data map")?;

    // GameDataMergeTracking.Apply: rows several mods added.
    for row in added {
        if row.changes.is_empty() {
            continue;
        }
        let Some(Byml::Array(rows)) = tables.get_mut(row.table.as_str()) else {
            continue;
        };
        let wide = row.table == "Bool64bitKey";
        let Some(base) = rows
            .iter_mut()
            .find(|r| if wide { hash64(r) == Some(row.hash) } else { hash32(r).map(|h| h as u64) == Some(row.hash) })
        else {
            continue;
        };

        let mut info = TrackingInfo::default();
        let mut changes = row.changes;
        for change in changes.iter_mut() {
            log_changes_inline(&mut info, change, base);
        }
        let mut row_tracking = Tracking::new(CANONICAL);
        if row.is_struct {
            row_tracking.type_override = Some("Struct".into());
        }
        for change in changes {
            byml_merge::merge(base, change, &mut row_tracking, &mut Vec::new())?;
        }
        row_tracking.apply(base)?;
    }

    for (table, hash) in dropped.into_keys() {
        if let Some(Byml::Array(rows)) = tables.get_mut(table.as_str()) {
            let wide = table == "Bool64bitKey";
            if let Some(position) = rows
                .iter()
                .position(|r| if wide { hash64(r) == Some(hash) } else { hash32(r).map(|h| h as u64) == Some(hash) })
            {
                rows.remove(position);
            }
        }
    }

    for (name, rows) in tables.iter_mut() {
        if let (true, Byml::Array(rows)) = (touched.contains(name.as_str()), rows) {
            rows.sort_by(|a, b| compare_hashes(a, b));
        }
    }

    // Taken out and put back rather than cloning the tables to satisfy the
    // borrow checker: they are tens of megabytes.
    if let Some(root_map) = root.as_map_mut() {
        if let Some(Byml::Map(mut metadata)) = root_map.remove("MetaData") {
            let result = match root_map.get("Data") {
                Some(Byml::Map(tables)) => calculate_metadata(&mut metadata, tables),
                _ => Ok(()),
            };
            root_map.insert("MetaData".into(), Byml::Map(metadata));
            result?;
        }
    }

    Ok(root.write(format))
}

fn compare_hashes(a: &Byml, b: &Byml) -> core::cmp::Ordering {
    let value = |row: &Byml| row.as_map().and_then(|m| m.get("Hash")).cloned();
    match (value(a), value(b)) {
        (Some(Byml::UInt32(x)), Some(Byml::UInt32(y))) => x.cmp(&y),
        (Some(Byml::UInt64(x)), Some(Byml::UInt64(y))) => x.cmp(&y),
        (Some(Byml::Int(x)), Some(Byml::Int(y))) => x.cmp(&y),
        (Some(Byml::Int64(x)), Some(Byml::Int64(y))) => x.cmp(&y),
        (Some(Byml::String(x)), Some(Byml::String(y))) => x.encode_utf16().cmp(y.encode_utf16()),
        _ => core::cmp::Ordering::Equal,
    }
}

const DEFAULT_SIZE: i32 = 0x130;

const VALID_TABLES: &[&str] = &[
    "Binary", "BinaryArray", "Bool", "Bool64bitKey", "BoolArray", "Enum", "EnumArray", "Float", "FloatArray", "Int",
    "Int64", "Int64Array", "IntArray", "String16", "String16Array", "String32", "String32Array", "String64",
    "String64Array", "UInt", "UInt64", "UInt64Array", "UIntArray", "Vector2", "Vector2Array", "Vector3",
    "Vector3Array", "WString16", "WString16Array", "WString32", "WString32Array", "WString64", "WString64Array",
];

/// SaveDataWriter.CalculateMetadata: save file sizes for the merged schema.
fn calculate_metadata(metadata: &mut Map, tables: &Map) -> MergeResult {
    let mut offsets = [0i32; 7];
    let mut sizes = [0i32; 7];
    let mut size_has_key = [false; 7];
    let mut all_offset = DEFAULT_SIZE;
    let mut all_size = DEFAULT_SIZE;

    let directories: Vec<Byml> = metadata
        .get("SaveDirectory")
        .and_then(|d| d.as_array())
        .cloned()
        .unwrap_or_default();

    for &table_name in VALID_TABLES {
        let Some(Byml::Array(rows)) = tables.get(table_name) else {
            continue;
        };
        let is_bool64 = table_name == "Bool64bitKey";
        let is_array = table_name.len() > 5 && table_name.ends_with("Array");

        for row in rows {
            let Some(entry) = row.as_map() else {
                continue;
            };
            let Some(Byml::Int(save_file_index)) = entry.get("SaveFileIndex") else {
                continue;
            };
            let save_file_index = *save_file_index;
            let no_directory = save_file_index == -1
                || matches!(directories.get(save_file_index as usize), Some(Byml::String(s)) if s.is_empty());

            if no_directory {
                if is_bool64 {
                    continue;
                }
                all_offset += 8;
                all_size += entry_size(table_name, entry, is_array)?;
                continue;
            }

            let slot = save_file_index as usize;
            if slot >= 7 {
                return Err(format!("save file index {} out of range", save_file_index));
            }

            if !is_bool64 {
                offsets[slot] += 8;
                all_offset += 8;
            } else if !size_has_key[slot] {
                sizes[slot] += 8;
                all_size += 8;
                size_has_key[slot] = true;
            }

            let size = entry_size(table_name, entry, is_array)?;
            sizes[slot] += size;
            all_size += size;
        }
    }

    for value in offsets.iter_mut().chain(sizes.iter_mut()) {
        if *value > 0 {
            *value += DEFAULT_SIZE;
        }
    }

    metadata.insert("AllDataSaveOffset".into(), Byml::Int(all_offset));
    metadata.insert("AllDataSaveSize".into(), Byml::Int(all_size));
    metadata.insert("SaveDataOffsetPos".into(), Byml::Array(offsets.iter().map(|&v| Byml::Int(v)).collect()));
    metadata.insert("SaveDataSize".into(), Byml::Array(sizes.iter().map(|&v| Byml::Int(v)).collect()));
    Ok(())
}

fn entry_size(table_name: &str, entry: &Map, is_array: bool) -> MergeResult<i32> {
    let (count, size, type_name) = if is_array {
        (array_count(table_name, entry)?, 0xC, &table_name[..table_name.len() - 5])
    } else {
        (1, 0x8, table_name)
    };

    Ok(match table_name {
        "BoolArray" => {
            // ceil(count / 8) bytes, at least 4, rounded up to a multiple of 4.
            let bytes = (count + 7) / 8;
            let value = if bytes > 3 { bytes } else { 4 };
            size + ((value + 3) / 4) * 4
        }
        "IntArray" | "FloatArray" | "UIntArray" | "EnumArray" => size + count * 4,
        "Binary" | "BinaryArray" => match entry.get("DefaultValue") {
            Some(Byml::UInt32(default)) => size + count * 4 + count * *default as i32,
            _ => size + count * 4,
        },
        _ => {
            size + count
                * match type_name {
                    "UInt64" | "Int64" | "Vector2" => 8,
                    "Vector3" => 12,
                    "String16" => 16,
                    "String32" | "WString16" => 32,
                    "String64" | "WString32" => 64,
                    "WString64" => 128,
                    _ => 0,
                }
        }
    })
}

fn array_count(table_name: &str, entry: &Map) -> MergeResult<i32> {
    for field in ["ArraySize", "Size"] {
        if let Some(value) = entry.get(field) {
            return match value {
                Byml::UInt32(count) => Ok(*count as i32),
                _ => Err(format!("'{}' in '{}' is not a UInt32", field, table_name)),
            };
        }
    }
    match entry.get("DefaultValue") {
        Some(Byml::Array(values)) => Ok(values.len() as i32),
        _ => Err(format!(
            "the length of an array entry in '{}' could not be determined",
            table_name
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(hash: u32, default: i32) -> Byml {
        Byml::Map(Map::from_iter::<Vec<(totk_formats::byml::Key, Byml)>>(vec![
            ("Hash".into(), Byml::UInt32(hash)),
            ("DefaultValue".into(), Byml::Int(default)),
            ("SaveFileIndex".into(), Byml::Int(0)),
        ]))
    }

    fn gdl(rows: Vec<Byml>) -> Vec<u8> {
        let metadata = Byml::Map(Map::from_iter::<Vec<(totk_formats::byml::Key, Byml)>>(vec![(
            "SaveDirectory".into(),
            Byml::Array(vec![Byml::from("a"), Byml::from("")]),
        )]));
        Byml::Map(Map::from_iter::<Vec<(totk_formats::byml::Key, Byml)>>(vec![
            ("Data".into(), Byml::Map(Map::from_iter::<Vec<(totk_formats::byml::Key, Byml)>>(vec![("Int".into(), Byml::Array(rows))]))),
            ("MetaData".into(), metadata),
        ]))
        .to_binary(7)
    }

    #[test]
    fn flags_from_several_mods_merge_and_sizes_follow() {
        let vanilla = gdl(vec![row(1, 0), row(3, 0)]);
        let mod_a = gdl(vec![row(1, 5), row(3, 0), row(2, 0)]);
        let mod_b = gdl(vec![row(1, 0), row(3, 0), row(4, 1)]);

        let changelogs: Vec<Vec<u8>> = [mod_a, mod_b]
            .iter()
            .map(|m| build_changelog(m, &vanilla).unwrap().unwrap())
            .collect();
        let merged = Byml::from_binary(&merge(vanilla.clone(), &changelogs).unwrap()).unwrap();
        let root = merged.as_map().unwrap();
        let rows = root["Data"].as_map().unwrap()["Int"].as_array().unwrap();
        assert_eq!(rows, &vec![row(1, 5), row(2, 0), row(3, 0), row(4, 1)]);

        let metadata = root["MetaData"].as_map().unwrap();
        // 4 entries of 8 bytes each in save file 0, plus the header size.
        assert_eq!(metadata["SaveDataSize"].as_array().unwrap()[0], Byml::Int(4 * 8 + DEFAULT_SIZE));
        assert_eq!(metadata["AllDataSaveOffset"], Byml::Int(DEFAULT_SIZE + 4 * 8));
    }
}
