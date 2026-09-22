//! Resource size table (`.rsizetable`).
//!
//! The game allocates a buffer per resource using this table. A mod that makes
//! a file bigger without updating its entry gets a crash or a silently skipped
//! resource, which is why every merge has to end with an RSTB pass.
//!
//! Layout: a "RESTBL" header, a table keyed by the CRC32 of the resource name,
//! then an overflow table keyed by the name itself (used for hash collisions).
//!
//! The hash table is kept as one sorted vector rather than a map: TotK's table
//! holds ~380 000 entries, and a B-tree that size costs several megabytes spread
//! over tens of thousands of small allocations — enough to exhaust the game's
//! allocator while the plugin is running inside it.

use alloc::collections::BTreeMap;

use crate::prelude::*;
use crate::{u32_at, Error, Result};

const MAGIC: &[u8; 6] = b"RESTBL";
const HEADER_SIZE: usize = 0x16;

pub struct Rstb {
    pub version: u32,
    pub string_block_size: usize,
    /// (CRC32 of the resource name, size), sorted by hash.
    hash_table: Vec<(u32, u32)>,
    /// Resource name -> size, for names whose hash collides.
    name_table: BTreeMap<String, u32>,
    /// Hashes added since parsing. Kept aside so adding thousands of entries
    /// does not shift the big sorted table each time; merged when writing.
    added: BTreeMap<u32, u32>,
}

impl Rstb {
    pub fn parse(data: &[u8]) -> Result<Rstb> {
        let magic = data.get(..6).ok_or(Error::Truncated { what: "RESTBL header" })?;
        if magic != MAGIC {
            return Err(Error::BadMagic {
                expected: "RESTBL",
                got: [magic[0], magic[1], magic[2], magic[3]],
            });
        }

        let version = u32_at(data, 6, "RESTBL header")?;
        let string_block_size = u32_at(data, 10, "RESTBL header")? as usize;
        let hash_count = u32_at(data, 14, "RESTBL header")? as usize;
        let name_count = u32_at(data, 18, "RESTBL header")? as usize;

        if string_block_size == 0 || string_block_size > 1024 {
            return Err(Error::Invalid("implausible RESTBL string block size"));
        }

        let names_start = HEADER_SIZE + hash_count * 8;
        if data.len() < names_start + name_count * (string_block_size + 4) {
            return Err(Error::Truncated { what: "RESTBL tables" });
        }

        let mut hash_table = Vec::with_capacity(hash_count);
        for index in 0..hash_count {
            let offset = HEADER_SIZE + index * 8;
            hash_table.push((
                u32_at(data, offset, "RESTBL hash table")?,
                u32_at(data, offset + 4, "RESTBL hash table")?,
            ));
        }
        // The game binary searches this, so the file is already sorted; sorting
        // again only guards against a hand-built table that is not.
        if !hash_table.windows(2).all(|pair| pair[0].0 <= pair[1].0) {
            hash_table.sort_unstable_by_key(|entry| entry.0);
        }

        let entry_size = string_block_size + 4;
        let mut name_table = BTreeMap::new();
        for index in 0..name_count {
            let offset = names_start + index * entry_size;
            let raw = &data[offset..offset + string_block_size];
            let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
            let name = core::str::from_utf8(&raw[..end])
                .map_err(|_| Error::Invalid("RESTBL name is not utf-8"))?
                .to_string();
            name_table.insert(name, u32_at(data, offset + string_block_size, "RESTBL name table")?);
        }

        Ok(Rstb {
            version,
            string_block_size,
            hash_table,
            name_table,
            added: BTreeMap::new(),
        })
    }

    pub fn hash_entries(&self) -> usize {
        self.hash_table.len() + self.added.len()
    }

    pub fn name_entries(&self) -> usize {
        self.name_table.len()
    }

    pub fn get_size(&self, name: &str) -> Option<u32> {
        if let Some(size) = self.name_table.get(name) {
            return Some(*size);
        }
        let hash = crate::crc32::compute_str(name);
        self.hash_table
            .binary_search_by_key(&hash, |entry| entry.0)
            .ok()
            .map(|index| self.hash_table[index].1)
            .or_else(|| self.added.get(&hash).copied())
    }

    /// Sets the size of a resource, mirroring how the game looks it up.
    ///
    /// A name already in the overflow table stays there. A name whose hash the
    /// table knows updates that entry. Anything else is new to the table (or
    /// collides with an entry the game owns) and goes to the overflow table,
    /// where it cannot damage another resource.
    pub fn set_size(&mut self, name: &str, size: u32) {
        if let Some(entry) = self.name_table.get_mut(name) {
            *entry = size;
            return;
        }

        let hash = crate::crc32::compute_str(name);
        match self.hash_table.binary_search_by_key(&hash, |entry| entry.0) {
            Ok(index) => self.hash_table[index].1 = size,
            Err(_) => match self.added.get_mut(&hash) {
                Some(entry) => *entry = size,
                None => {
                    self.name_table.insert(name.to_string(), size);
                }
            },
        }
    }

    /// An empty table, as TKMM writes for resource size overrides.
    pub fn new() -> Rstb {
        Rstb {
            version: 1,
            string_block_size: 160,
            hash_table: Vec::new(),
            name_table: BTreeMap::new(),
            added: BTreeMap::new(),
        }
    }

    /// Size recorded under the name itself (the overflow table).
    pub fn name_size(&self, name: &str) -> Option<u32> {
        self.name_table.get(name).copied()
    }

    pub fn set_name_size(&mut self, name: &str, size: u32) {
        self.name_table.insert(name.to_string(), size);
    }

    /// Every (hash, size) entry, sorted by hash.
    pub fn hash_table(&self) -> Vec<(u32, u32)> {
        let mut all: Vec<(u32, u32)> = self.hash_table.clone();
        all.extend(self.added.iter().map(|(h, s)| (*h, *s)));
        all.sort_unstable();
        all
    }

    pub fn name_table(&self) -> &BTreeMap<String, u32> {
        &self.name_table
    }

    /// True when the hash was in the table as parsed.
    pub fn has_original_hash(&self, hash: u32) -> bool {
        self.hash_table.binary_search_by_key(&hash, |entry| entry.0).is_ok()
    }

    pub fn has_hash(&self, hash: u32) -> bool {
        self.hash_table.binary_search_by_key(&hash, |entry| entry.0).is_ok() || self.added.contains_key(&hash)
    }

    /// Inserts a hash entry. Returns false (and changes nothing) when the hash
    /// is already present.
    pub fn try_add_hash(&mut self, hash: u32, size: u32) -> bool {
        if self.has_hash(hash) {
            return false;
        }
        self.added.insert(hash, size);
        true
    }

    pub fn set_hash_size(&mut self, hash: u32, size: u32) {
        match self.hash_table.binary_search_by_key(&hash, |entry| entry.0) {
            Ok(index) => self.hash_table[index].1 = size,
            Err(_) => {
                self.added.insert(hash, size);
            }
        }
    }

    pub fn write(&self) -> Vec<u8> {
        let entry_size = self.string_block_size + 4;
        let hash_count = self.hash_entries();
        let mut out = Vec::with_capacity(HEADER_SIZE + hash_count * 8 + self.name_table.len() * entry_size);

        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&self.version.to_le_bytes());
        out.extend_from_slice(&(self.string_block_size as u32).to_le_bytes());
        out.extend_from_slice(&(hash_count as u32).to_le_bytes());
        out.extend_from_slice(&(self.name_table.len() as u32).to_le_bytes());

        // Both tables come out sorted, which is what the game binary searches.
        let mut existing = self.hash_table.iter().peekable();
        let mut added = self.added.iter().peekable();
        loop {
            let take_existing = match (existing.peek(), added.peek()) {
                (Some(a), Some(b)) => a.0 <= *b.0,
                (Some(_), None) => true,
                (None, Some(_)) => false,
                (None, None) => break,
            };
            let (hash, size) = if take_existing {
                *existing.next().unwrap()
            } else {
                let (hash, size) = added.next().unwrap();
                (*hash, *size)
            };
            out.extend_from_slice(&hash.to_le_bytes());
            out.extend_from_slice(&size.to_le_bytes());
        }

        for (name, size) in &self.name_table {
            let mut padded = vec![0u8; self.string_block_size];
            let bytes = name.as_bytes();
            let len = bytes.len().min(self.string_block_size - 1);
            padded[..len].copy_from_slice(&bytes[..len]);
            out.extend_from_slice(&padded);
            out.extend_from_slice(&size.to_le_bytes());
        }

        out
    }
}

/// Size the game should reserve for `size` bytes of `name`'s decompressed data.
///
/// Ported from TKMM's TkResourceSizeCollector, which is the reference every
/// TotK mod is built against. Verified against the vanilla table: for packs it
/// reproduces Nintendo's own numbers exactly.
///
/// `name` is the resource name (see [`resource_name`]) of the original path, so
/// model codec files still end in ".mc" only through `extension`. `data` is the
/// decompressed file; the few formats whose size depends on their contents fall
/// back to the generic rule when it is not provided.
pub fn resource_size(size: u32, name: &str, data: &[u8]) -> u32 {
    resource_size_with_extension(size, name, resource_extension(name), data)
}

/// Like [`resource_size`], for callers that already know the extension (the
/// one of the romfs path, which differs from the name's for ".mc" files).
pub fn resource_size_with_extension(size: u32, name: &str, extension: &str, data: &[u8]) -> u32 {
    let size = size + align_padding(size, 0x20);

    let calculated = match extension {
        ".ainb" => ainb_size(data).map(|extra| size + extra),
        ".asb" => asb_size(data).map(|extra| size + extra),
        ".bstar" => bstar_size(data).map(|extra| size + extra),
        ".mc" => model_codec_size(data),
        _ => None,
    };
    if let Some(value) = calculated {
        return value;
    }

    match name {
        "Event/EventFlow/Dm_ED_0004.bfevfl" => return size + 0x1E0,
        "Effect/static.Nin_NX_NVN.esetb.byml" => return size + 0x1000,
        "Effect/static.Product.110.Nin_NX_NVN.esetb.byml" => return size + 0x1000,
        "Lib/agl/agl_resource.Nin_NX_NVN.release.sarc" => return size + 0x1000,
        "Lib/gsys/gsys_resource.Nin_NX_NVN.release.sarc" => return size + 0x1000,
        "Lib/Terrain/tera_resource.Nin_NX_NVN.release.sarc" => return size + 0x1000,
        "Shader/ApplicationPackage.Nin_NX_NVN.release.sarc" => return size + 0x1000,
        _ => {}
    }

    if extension == ".casset.byml" {
        return size + 0x1C0;
    }

    match extension {
        ".bgyml" => (size + 2000) * 8,
        ".baatarc" | ".bagst" | ".bcul" | ".beco" | ".belnk" | ".bfarc" | ".bfsha" | ".bhtmp"
        | ".blal" | ".blarc" | ".blwp" | ".bnsh" | ".bntx" | ".bphcl" | ".bphhb" | ".bslnk"
        | ".byml" | ".cai" | ".chunk" | ".crbin" | ".cutinfo" | ".dpi" | ".jpg" | ".png"
        | ".quad" | ".tscb" | ".txtg" | ".txt" | ".vsts" | ".wbr" | ".zs" => size + 0x100,
        ".baev" | ".bfevfl" | ".bphnm" => size + 0x120,
        ".bars" => size + 0x240,
        ".bphsh" => size + 0x170,
        ".genvb" => size + 0x180,
        ".pack" | ".sarc" => size + 0x180,
        // These would need the file parsed to be exact. Over-reserving is safe
        // (the table is an allocation hint), under-reserving is not.
        _ => (size + 1500) * 4,
    }
}

fn le_u32(data: &[u8], offset: usize) -> Option<u32> {
    data.get(offset..offset + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// True for the extensions whose size needs the file contents.
pub fn requires_data(path: &str) -> bool {
    matches!(resource_extension(path), ".ainb" | ".asb" | ".bstar" | ".mc")
}

fn exb_signature_size(data: &[u8], exb_offset: usize) -> Option<u32> {
    let count_offset = le_u32(data, exb_offset + 0x20)? as usize;
    let signatures = le_u32(data, exb_offset + count_offset)?;
    Some(16 + (signatures + 1) / 2 * 8)
}

fn ainb_size(data: &[u8]) -> Option<u32> {
    let exb_offset = le_u32(data, 0x44)? as usize;
    let signatures = if exb_offset != 0 {
        let count_offset = le_u32(data, exb_offset + 0x20)? as usize;
        le_u32(data, exb_offset + count_offset)?
    } else {
        0
    };
    Some(392 + 16 + (signatures + 1) / 2 * 8)
}

fn asb_size(data: &[u8]) -> Option<u32> {
    let node_count = le_u32(data, 0x10)?;
    let exb_offset = le_u32(data, 0x60)? as usize;
    let mut size = 552 + 40 * node_count;
    if exb_offset != 0 {
        size += exb_signature_size(data, exb_offset)?;
    }
    Some(size)
}

fn bstar_size(data: &[u8]) -> Option<u32> {
    Some(0x120 + le_u32(data, 0x08)? * 8)
}

fn model_codec_size(data: &[u8]) -> Option<u32> {
    let flags = le_u32(data, 0x08)? as i32;
    let size = (flags >> 5) << (flags & 0xF);
    Some((size as f64 * 2.55) as u32)
}

/// Padding needed to bring `value` up to a multiple of `alignment`.
fn align_padding(value: u32, alignment: u32) -> u32 {
    let remainder = value % alignment;
    if remainder == 0 {
        0
    } else {
        alignment - remainder
    }
}

/// Extension used for size lookups: ".zs" is stripped, except where the game
/// keeps a compound extension.
pub fn resource_extension(path: &str) -> &str {
    let path = if path.ends_with(".casset.byml.zs") {
        return ".casset.byml";
    } else if path.ends_with(".ta.zs") {
        path
    } else if let Some(stripped) = path.strip_suffix(".zs") {
        stripped
    } else {
        path
    };

    match path.rfind('.') {
        Some(index) => &path[index..],
        None => "",
    }
}

/// Resource name the table is keyed by: the romfs path without a ".zs"/".mc"
/// suffix, with forward slashes.
pub fn resource_name(path: &str) -> String {
    let trimmed = if path.ends_with(".zs") || path.ends_with(".mc") {
        &path[..path.len() - 3]
    } else {
        path
    };
    trimmed.replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_ignore_the_zs_suffix() {
        assert_eq!(resource_extension("Pack/Actor/Foo.pack.zs"), ".pack");
        assert_eq!(resource_extension("Foo.bgyml"), ".bgyml");
        assert_eq!(resource_extension("Foo.casset.byml.zs"), ".casset.byml");
        assert_eq!(resource_extension("Foo"), "");
    }

    #[test]
    fn names_drop_the_compression_suffix() {
        assert_eq!(resource_name("Pack/Actor/Foo.pack.zs"), "Pack/Actor/Foo.pack");
        assert_eq!(resource_name("Model/Foo.bfres.mc"), "Model/Foo.bfres");
        assert_eq!(
            resource_name("RSDB/Tag.Product.121.rstbl.byml"),
            "RSDB/Tag.Product.121.rstbl.byml"
        );
    }

    #[test]
    fn sizes_follow_the_reference_rules() {
        // .pack: align up to 0x20 then + 0x180
        assert_eq!(resource_size(0x100, "Pack/Foo.pack", &[]), 0x100 + 0x180);
        // .bgyml: (size + 2000) * 8
        assert_eq!(resource_size(0x20, "Foo.bgyml", &[]), (0x20 + 2000) * 8);
    }
}
