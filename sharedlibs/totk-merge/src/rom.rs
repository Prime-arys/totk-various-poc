//! The game's own files: version information, path tables and vanilla lookups.
//!
//! Port of TKMM's ExtractedTkRom, reading straight from the mounted romfs on
//! the console (or an extracted copy on a PC).

use core::cell::RefCell;

use hashbrown::HashMap;
use totk_formats::byml::Byml;
use totk_formats::sarc::Sarc;
use totk_formats::xxhash::xxh32_utf16;
use totk_formats::zstd::Zstd;

use crate::info;
use crate::prelude::*;
use crate::sys::fs;
use crate::tkcl::attributes;

/// TKMM's pack file lookup: which pack a file only found inside packs lives
/// in. MIT licensed, from TkSharp.Data.Embedded.
static PACK_FILE_LOOKUP: &[u8] = include_bytes!("../data/PackFileLookup.pkcache.zs");

pub struct TkRom {
    prefix: String,
    pub game_version: i32,
    pub nso_binary_id: String,
    zstd: RefCell<Zstd>,
    address_table: HashMap<String, String>,
    event_flow_versions: HashMap<String, String>,
    effect_versions: HashMap<String, String>,
    ai_versions: HashMap<String, String>,
    logic_versions: HashMap<String, String>,
    sequence_versions: HashMap<String, String>,
    pack_lookup: RefCell<Option<PackLookup>>,
    recent_packs: RefCell<Vec<(String, alloc::rc::Rc<Vec<u8>>)>>,
}

fn string_map(node: &Byml) -> HashMap<String, String> {
    node.as_map()
        .map(|map| {
            map.iter()
                .filter_map(|(key, value)| value.as_str().map(|v| (key.to_string(), v.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

impl TkRom {
    /// `prefix` is prepended to romfs paths: "content:/" on the console, an
    /// extracted romfs directory ending in a slash on a PC.
    pub fn open(prefix: &str) -> Result<TkRom, String> {
        let prefix = prefix.to_string();
        let read = |relative: &str| -> Result<Vec<u8>, String> {
            fs::read(&format!("{}{}", prefix, relative)).map_err(|e| e.to_string())
        };

        let mask = read("System/RegionLangMask.txt")?;
        let (game_version, nso_binary_id) =
            parse_region_lang_mask(&mask).ok_or("could not read the game version from RegionLangMask.txt")?;

        let mut zstd = Zstd::new();
        let dictionaries = zstd
            .decompress(&read("Pack/ZsDic.pack.zs")?)
            .map_err(|e| format!("ZsDic.pack.zs: {}", e))?;
        zstd.load_dictionaries(&dictionaries)
            .map_err(|e| format!("ZsDic.pack.zs: {}", e))?;

        let mut load_byml = |relative: &str| -> Result<Byml, String> {
            let data = zstd.decompress(&read(relative)?).map_err(|e| format!("{}: {}", relative, e))?;
            Byml::from_binary(&data).map_err(|e| format!("{}: {}", relative, e))
        };

        let address_table = string_map(&load_byml(&format!(
            "System/AddressTable/Product.{}.Nin_NX_NVN.atbl.byml.zs",
            game_version
        ))?);
        let table_path = |canonical: &str| -> Result<String, String> {
            address_table
                .get(canonical)
                .map(|path| format!("{}.zs", path))
                .ok_or_else(|| format!("{} is missing from the address table", canonical))
        };

        let event_flow = load_byml(&table_path("Event/EventFlow/EventFlowFileEntry.Product.byml")?)?;
        let event_flow_versions = event_flow.as_map().and_then(|m| m.get("Versions")).map(string_map).unwrap_or_default();

        let effect = load_byml(&table_path("Effect/EffectFileInfo.Product.Nin_NX_NVN.byml")?)?;
        let mut effect_versions: HashMap<String, String> = effect
            .as_map()
            .and_then(|m| m.get("BinaryDict"))
            .map(string_map)
            .unwrap_or_default()
            .into_iter()
            .filter(|(_, value)| value.len() > 11 && &value[value.len() - 11..value.len() - 4] == "Product")
            .collect();
        if let Some(name) = effect
            .as_map()
            .and_then(|m| m.get("StaticEsetb"))
            .and_then(|n| n.as_array())
            .and_then(|a| a.first())
            .and_then(|n| n.as_str())
        {
            effect_versions.insert("static".into(), name.to_string());
        }

        let ai_versions = string_map(&load_byml(&table_path("AI/FileEntry/FileEntry.Product.byml")?)?);
        let logic_versions = string_map(&load_byml(&table_path("Logic/FileEntry/FileEntry.Product.byml")?)?);
        let sequence_versions = string_map(&load_byml(&table_path("Sequence/FileEntry/FileEntry.Product.byml")?)?);

        Ok(TkRom {
            prefix,
            game_version,
            nso_binary_id,
            zstd: RefCell::new(zstd),
            address_table,
            event_flow_versions,
            effect_versions,
            ai_versions,
            logic_versions,
            sequence_versions,
            pack_lookup: RefCell::new(None),
            recent_packs: RefCell::new(Vec::new()),
        })
    }

    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    pub fn decompress(&self, data: &[u8]) -> Result<Vec<u8>, String> {
        self.zstd.borrow_mut().decompress(data).map_err(|e| e.to_string())
    }

    pub fn event_flow_version(&self, name: &str) -> Option<&str> {
        self.event_flow_versions.get(name).map(|s| s.as_str())
    }

    pub fn versioned_name(&self, table: VersionTable, name: &str) -> Option<&str> {
        let map = match table {
            VersionTable::Sequence => &self.sequence_versions,
            VersionTable::Effect => &self.effect_versions,
            VersionTable::Logic => &self.logic_versions,
            VersionTable::Ai => &self.ai_versions,
        };
        map.get(name).map(|s| s.as_str())
    }

    /// ITkRom.CanonicalToRelativePath.
    pub fn canonical_to_relative(&self, canonical: &str, attributes: u32) -> String {
        let mut result = self
            .address_table
            .get(canonical)
            .cloned()
            .unwrap_or_else(|| canonical.to_string());

        let canon = result.clone();
        let len = canon.len();

        let resolved = if len > 3 && canon.starts_with("AI/") {
            self.ai_versions.get(&canon).cloned()
        } else {
            None
        }
        .or_else(|| {
            if len > 26 && canon.starts_with("Event/EventFlow") && canon.is_char_boundary(16) && canon.is_char_boundary(len - 7) {
                let name = &canon[16..len - 7];
                self.event_flow_versions
                    .get(name)
                    .map(|version| format!("Event/EventFlow/{}.{}{}", name, version, path_extension(&canon)))
            } else {
                None
            }
        })
        .or_else(|| {
            if attributes & self::attributes::IS_PRODUCT_FILE != 0
                && len > 37
                && canon.starts_with("Effect")
                && canon.is_char_boundary(len - 30)
            {
                let name = &canon[7..len - 30];
                self.effect_versions
                    .get(name)
                    .map(|file| format!("Effect/{}.Product.Nin_NX_NVN.esetb.byml", file))
            } else {
                None
            }
        })
        .or_else(|| {
            if len > 9 && canon.starts_with("Sequence/") {
                self.sequence_versions.get(&canon).cloned()
            } else {
                None
            }
        })
        .or_else(|| {
            if len > 6 && canon.starts_with("Logic/") {
                self.logic_versions.get(&canon).cloned()
            } else {
                None
            }
        });

        if let Some(resolved) = resolved {
            result = resolved;
        }
        if attributes & self::attributes::HAS_ZS_EXTENSION != 0 {
            result.push_str(".zs");
        }
        if attributes & self::attributes::HAS_MC_EXTENSION != 0 {
            result.push_str(".mc");
        }
        result
    }

    /// Reads a vanilla file by romfs-relative path, decompressed. Files that
    /// only exist inside packs are found through the pack lookup.
    ///
    /// The second value is TKMM's "isFoundMissing": the file is known to exist
    /// in some pack, but could not be read from it.
    pub fn get_vanilla(&self, relative: &str) -> (Option<Vec<u8>>, bool) {
        match fs::read(&format!("{}{}", self.prefix, relative)) {
            Ok(raw) => match self.decompress(&raw) {
                Ok(data) => (Some(data), false),
                Err(error) => {
                    info!("{}: {}", relative, error);
                    (None, false)
                }
            },
            Err(_) => self.get_nested(relative),
        }
    }

    pub fn get_vanilla_canonical(&self, canonical: &str, attributes: u32) -> Option<Vec<u8>> {
        self.get_vanilla(&self.canonical_to_relative(canonical, attributes)).0
    }

    fn get_nested(&self, canonical: &str) -> (Option<Vec<u8>>, bool) {
        let pack = {
            let mut lookup = self.pack_lookup.borrow_mut();
            if lookup.is_none() {
                *lookup = Some(PackLookup::load(&mut self.zstd.borrow_mut()));
            }
            lookup.as_ref().unwrap().get(canonical)
        };

        let Some((pack_canonical, pack_attributes)) = pack else {
            return (None, false);
        };

        let relative = self.canonical_to_relative(&pack_canonical, pack_attributes);
        let Some(data) = self.read_pack(&relative) else {
            return (None, true);
        };
        match Sarc::parse(&data).ok().and_then(|sarc| sarc.get(canonical).map(|e| e.data.to_vec())) {
            Some(file) => (Some(file), false),
            None => (None, true),
        }
    }

    /// Decompressed vanilla pack, remembering the last few: files nested in
    /// the same pack tend to be asked for one after the other.
    fn read_pack(&self, relative: &str) -> Option<alloc::rc::Rc<Vec<u8>>> {
        const KEEP: usize = 4;

        if let Some((_, data)) = self.recent_packs.borrow().iter().find(|(name, _)| name == relative) {
            return Some(data.clone());
        }
        let raw = fs::read(&format!("{}{}", self.prefix, relative)).ok()?;
        let data = alloc::rc::Rc::new(self.decompress(&raw).ok()?);

        let mut recent = self.recent_packs.borrow_mut();
        if recent.len() == KEEP {
            recent.remove(0);
        }
        recent.push((relative.to_string(), data.clone()));
        Some(data)
    }

    /// Where the game keeps its resource size table.
    pub fn resource_size_table_path(&self) -> String {
        if self.game_version >= 140 {
            format!(
                "System/Resource/ResourceSizeTable.Product.{}.Nin_NX_NVN.rsizetable.zs",
                self.game_version
            )
        } else {
            format!("System/Resource/ResourceSizeTable.Product.{}.rsizetable.zs", self.game_version)
        }
    }

    /// Locales the game ships message archives for, e.g. "USen".
    pub fn locales(&self) -> Vec<String> {
        let mut locales: Vec<String> = fs::read_dir(&format!("{}Mals", self.prefix))
            .map(|entries| {
                entries
                    .into_iter()
                    .filter_map(|entry| {
                        let name = entry.name;
                        (name.len() > 4 && name.ends_with(".sarc.zs")).then(|| name[..4].to_string())
                    })
                    .collect()
            })
            .unwrap_or_default();
        locales.sort();
        locales.dedup();
        locales
    }
}

#[derive(Debug, Clone, Copy)]
pub enum VersionTable {
    Sequence,
    Effect,
    Logic,
    Ai,
}

fn path_extension(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rfind('.') {
        Some(i) => &name[i..],
        None => "",
    }
}

/// "…\r\nNormal\r\n121\r\n<nso id>" → (121, nso id).
pub fn parse_region_lang_mask(data: &[u8]) -> Option<(i32, String)> {
    let newline = data.iter().rposition(|&b| b == b'\n')?;
    if newline < 4 {
        return None;
    }
    let nso_binary_id = String::from_utf8_lossy(&data[newline + 1..]).trim().to_string();
    let version = core::str::from_utf8(&data[newline - 4..newline - 1]).ok()?.trim().parse().ok()?;
    Some((version, nso_binary_id))
}

/// TkPackFileLookup: (first char, last char, xxHash32 of the UTF-16 name) →
/// pack canonical and attributes.
struct PackLookup {
    keys: Vec<(u64, u16)>,
    values: Vec<(String, u32)>,
}

impl PackLookup {
    fn load(zstd: &mut Zstd) -> PackLookup {
        let empty = PackLookup {
            keys: Vec::new(),
            values: Vec::new(),
        };
        let Ok(data) = zstd.decompress(PACK_FILE_LOOKUP) else {
            info!("could not decompress the pack file lookup");
            return empty;
        };
        let read_u32 = |offset: usize| -> Option<u32> {
            data.get(offset..offset + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        };
        if read_u32(0) != Some(0x4843_4B50) {
            info!("invalid pack file lookup");
            return empty;
        }
        let count = read_u32(4).unwrap_or(0) as usize;
        let string_table = read_u32(8).unwrap_or(0) as usize;
        let string_count = read_u32(12).unwrap_or(0) as usize;

        let mut values = Vec::with_capacity(string_count);
        let mut position = string_table;
        for _ in 0..string_count {
            let Some(length) = data.get(position..).and_then(|rest| rest.iter().position(|&b| b == 0)) else {
                break;
            };
            let name = String::from_utf8_lossy(&data[position..position + length]).into_owned();
            let attributes = data.get(position + length + 1).copied().unwrap_or(0) as u32;
            values.push((name, attributes));
            position += length + 2;
        }

        let mut keys = Vec::with_capacity(count);
        for i in 0..count {
            let offset = 0x10 + i * 8;
            let Some(entry) = data.get(offset..offset + 8) else {
                break;
            };
            let section = u16::from_le_bytes([entry[0], entry[1]]);
            let hash = u32::from_le_bytes([entry[2], entry[3], entry[4], entry[5]]);
            let index = u16::from_le_bytes([entry[6], entry[7]]);
            keys.push((((hash as u64) << 32) | section as u64, index));
        }
        keys.sort_unstable();
        PackLookup { keys, values }
    }

    fn get(&self, canonical: &str) -> Option<(String, u32)> {
        let units: Vec<u16> = canonical.encode_utf16().collect();
        let (&first, &last) = (units.first()?, units.last()?);
        let section = ((first as u8 as u16) << 8) | last as u8 as u16;
        let key = ((xxh32_utf16(canonical) as u64) << 32) | section as u64;
        let index = self.keys.binary_search_by(|probe| probe.0.cmp(&key)).ok()?;
        self.values.get(self.keys[index].1 as usize).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_region_lang_mask() {
        let data = b"USen EUfr\r\nNormal\r\n121\r\n9b4e43650501a4d4489b4bbfdb740f26af3cf850";
        let (version, id) = parse_region_lang_mask(data).unwrap();
        assert_eq!(version, 121);
        assert_eq!(id, "9b4e43650501a4d4489b4bbfdb740f26af3cf850");
    }
}
