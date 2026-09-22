//! SARC archives (`.pack`, `.sarc`, `.bfarc`, ...).
//!
//! Layout: a header, an SFAT node table sorted by name hash (the game binary
//! searches it), an SFNT name table and the file data.

use alloc::collections::BTreeMap;

use hashbrown::HashMap;

use crate::prelude::*;
use crate::{magic_at, u16_at, u32_at, Error, Result};

const HEADER_SIZE: usize = 0x14;
const SFAT_HEADER_SIZE: usize = 0x0C;
const SFNT_HEADER_SIZE: usize = 0x08;
const DEFAULT_HASH_KEY: u32 = 0x65;

pub fn name_hash(name: &str, key: u32) -> u32 {
    let mut hash = 0u32;
    for byte in name.as_bytes() {
        hash = hash.wrapping_mul(key).wrapping_add(*byte as u32);
    }
    hash
}

#[derive(Debug, Clone, Copy)]
pub struct SarcEntry<'a> {
    pub name: &'a str,
    pub data: &'a [u8],
}

pub struct Sarc<'a> {
    entries: Vec<SarcEntry<'a>>,
    index: HashMap<&'a str, usize>,
    hash_key: u32,
    /// SarcLibrary's "minimum alignment": 4, reduced to the GCD of every
    /// file's offset.
    min_alignment: u32,
}

impl<'a> Sarc<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Sarc<'a>> {
        magic_at(data, 0, "SARC")?;

        let bom = u16_at(data, 6, "SARC BOM")?;
        if bom != 0xFEFF {
            return Err(Error::Invalid("big endian SARC archives are not supported"));
        }

        let data_offset = u32_at(data, 0x0C, "SARC data offset")? as usize;

        magic_at(data, HEADER_SIZE, "SFAT")?;
        let node_count = u16_at(data, HEADER_SIZE + 6, "SFAT node count")? as usize;
        let hash_key = u32_at(data, HEADER_SIZE + 8, "SFAT hash key")?;

        let sfat_nodes = HEADER_SIZE + SFAT_HEADER_SIZE;
        let sfnt = sfat_nodes + node_count * 0x10;
        magic_at(data, sfnt, "SFNT")?;
        let names_start = sfnt + SFNT_HEADER_SIZE;

        let mut entries = Vec::with_capacity(node_count);
        let mut index = HashMap::with_capacity(node_count);
        let mut min_alignment = MIN_ALIGNMENT;

        for node_index in 0..node_count {
            let node = sfat_nodes + node_index * 0x10;
            let attrs = u32_at(data, node + 4, "SFAT node")?;
            let start = u32_at(data, node + 8, "SFAT node")? as usize;
            let end = u32_at(data, node + 12, "SFAT node")? as usize;

            if attrs & 0x0100_0000 == 0 {
                return Err(Error::Invalid("SARC entry without a name is not supported"));
            }

            let name_offset = names_start + (attrs & 0x00FF_FFFF) as usize * 4;
            let name = read_cstr(data, name_offset)?;

            let file_start = data_offset + start;
            let file_end = data_offset + end;
            let file = data
                .get(file_start..file_end)
                .ok_or(Error::Truncated { what: "SARC file data" })?;

            min_alignment = gcd(min_alignment, file_start as u32);
            index.insert(name, entries.len());
            entries.push(SarcEntry { name, data: file });
        }

        Ok(Sarc {
            entries,
            index,
            hash_key,
            min_alignment,
        })
    }

    pub fn entries(&self) -> impl Iterator<Item = &SarcEntry<'a>> {
        self.entries.iter()
    }

    pub fn get(&self, name: &str) -> Option<&SarcEntry<'a>> {
        self.index.get(name).map(|&i| &self.entries[i])
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn hash_key(&self) -> u32 {
        self.hash_key
    }

    pub fn min_alignment(&self) -> u32 {
        self.min_alignment
    }
}

const MIN_ALIGNMENT: u32 = 4;

fn gcd(a: u32, b: u32) -> u32 {
    if a == 0 || b == 0 {
        return a | b;
    }
    let (mut a, mut b) = (a, b);
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

fn lcm(a: u32, b: u32) -> u32 {
    a / gcd(a, b) * b
}

fn read_cstr(data: &[u8], offset: usize) -> Result<&str> {
    let rest = data.get(offset..).ok_or(Error::Truncated { what: "SARC name" })?;
    let end = rest
        .iter()
        .position(|&b| b == 0)
        .ok_or(Error::Truncated { what: "SARC name" })?;
    core::str::from_utf8(&rest[..end]).map_err(|_| Error::Invalid("SARC name is not utf-8"))
}

/// Alignment of a file, as SarcLibrary (and so TKMM) estimates it when writing.
pub fn estimate_alignment(name: &str, data: &[u8], min_alignment: u32) -> u32 {
    let file_name = name.rsplit('/').next().unwrap_or(name);
    let ext = file_name.rfind('.').map_or("", |i| &file_name[i + 1..]);
    let mut result = min_alignment;
    result = match ext {
        "bffnt" => lcm(result, 0x1000),
        "aglatex" | "aglblm" | "aglccr" | "aglclwd" | "aglcube" | "agldof" | "aglenv" | "aglenvset" | "aglfila"
        | "agllmap" | "agllref" | "aglshpp" | "baglatex" | "baglblm" | "baglccr" | "baglclwd" | "baglcube"
        | "bagldof" | "baglenv" | "baglenvset" | "baglfila" | "bagllmap" | "bagllref" | "baglshpp" | "bglght"
        | "bglpbd" | "bglpbm" | "bgsdw" | "bksky" | "bpref" | "glght" | "glpbd" | "glpbm" | "gsdw" | "ksky"
        | "pref" => lcm(result, 8),
        "byml" | "baglmf" => lcm(result, 0x80),
        "bfres" | "sharc" | "sharcb" => lcm(result, 0x1000),
        "bofx" | "fmd" | "ftx" | "genvres" | "gtx" | "ofx" => lcm(result, 0x2000),
        _ => result,
    };

    const NO_BINARY_CHECK: &[&str] = &[
        "sarc", "bfres", "bcamanim", "batpl", "bnfprl", "bplacement", "hks or lua", "bactcapt", "bitemico", "jpg",
        "bmaptex", "bstftex", "bgdata", "bgsvdata", "hknm2", "bmscdef", "bars", "bxml", "bgparamlist",
        "bmodellist", "baslist", "baiprog", "bphysics", "bchemical", "bas", "batcllist", "batcl", "baischedule",
        "bdmgparam", "brgconfiglist", "brgconfig", "brgbw", "bawareness", "bdrop", "bshop", "brecipe", "blod",
        "bbonectrl", "blifecondition", "bumii", "baniminfo", "byaml", "byml", "bassetting", "hkrb", "hkrg",
        "bphyssb", "hkcl", "hksc", "hktmrb", "brgcon", "esetlist", "bdemo", "bfevfl", "bfevtm",
    ];
    if !NO_BINARY_CHECK.contains(&ext) {
        result = lcm(result, binary_file_alignment(data));
    }
    result
}

/// Nintendo binary files (BNTX, BFRES, ...) declare their alignment in their
/// header.
fn binary_file_alignment(data: &[u8]) -> u32 {
    if data.len() <= 0x20 {
        return 1;
    }
    let big_endian = data[0x0C] == 0xFE && data[0x0D] == 0xFF;
    let size_bytes = [data[0x1C], data[0x1D], data[0x1E], data[0x1F]];
    let size = if big_endian {
        i32::from_be_bytes(size_bytes)
    } else {
        i32::from_le_bytes(size_bytes)
    };
    if size as i64 != data.len() as i64 || data[0x0E] > 31 {
        return 1;
    }
    1 << data[0x0E]
}

/// Rebuilds a SARC archive.
pub struct SarcBuilder {
    files: BTreeMap<String, Vec<u8>>,
    hash_key: u32,
    min_alignment: u32,
}

impl Default for SarcBuilder {
    fn default() -> Self {
        SarcBuilder {
            files: BTreeMap::new(),
            hash_key: DEFAULT_HASH_KEY,
            min_alignment: MIN_ALIGNMENT,
        }
    }
}

impl SarcBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Seeds the builder with every file of an existing archive, keeping its
    /// hash key and minimum alignment.
    pub fn from_sarc(sarc: &Sarc<'_>) -> Self {
        let mut builder = SarcBuilder {
            files: BTreeMap::new(),
            hash_key: sarc.hash_key(),
            min_alignment: sarc.min_alignment(),
        };
        for entry in sarc.entries() {
            builder.files.insert(entry.name.to_string(), entry.data.to_vec());
        }
        builder
    }

    pub fn insert(&mut self, name: &str, data: Vec<u8>) {
        self.files.insert(name.to_string(), data);
    }

    pub fn remove(&mut self, name: &str) {
        self.files.remove(name);
    }

    pub fn contains(&self, name: &str) -> bool {
        self.files.contains_key(name)
    }

    pub fn get(&self, name: &str) -> Option<&[u8]> {
        self.files.get(name).map(|data| data.as_slice())
    }

    pub fn names(&self) -> impl Iterator<Item = &String> {
        self.files.keys()
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn build(&self) -> Vec<u8> {
        // Nodes have to come out sorted by hash: the game binary searches them.
        let mut nodes: Vec<(u32, &String, &Vec<u8>, u32)> = self
            .files
            .iter()
            .map(|(name, data)| {
                let alignment = estimate_alignment(name, data, self.min_alignment);
                (name_hash(name, self.hash_key), name, data, alignment)
            })
            .collect();
        nodes.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(b.1)));

        // Name table, each name NUL terminated and padded to 4 bytes.
        let mut name_table: Vec<u8> = Vec::new();
        let mut name_offsets: Vec<u32> = Vec::with_capacity(nodes.len());
        for (_, name, _, _) in &nodes {
            name_offsets.push(name_table.len() as u32 / 4);
            name_table.extend_from_slice(name.as_bytes());
            name_table.push(0);
            while name_table.len() % 4 != 0 {
                name_table.push(0);
            }
        }

        let sfat_size = SFAT_HEADER_SIZE + nodes.len() * 0x10;
        let sfnt_size = SFNT_HEADER_SIZE + name_table.len();
        let headers_size = HEADER_SIZE + sfat_size + sfnt_size;

        let archive_alignment = nodes.iter().fold(1, |acc, (_, _, _, a)| lcm(acc, *a));
        let data_offset = align_up(headers_size, archive_alignment as usize);

        // Lay the data out first so the node table can reference it.
        let mut blob: Vec<u8> = Vec::new();
        let mut ranges: Vec<(u32, u32)> = Vec::with_capacity(nodes.len());
        for (_, _, data, alignment) in &nodes {
            let relative = align_up(blob.len(), *alignment as usize);
            blob.resize(relative, 0);
            let start = blob.len() as u32;
            blob.extend_from_slice(data);
            ranges.push((start, blob.len() as u32));
        }

        let file_size = data_offset + blob.len();
        let mut out = Vec::with_capacity(file_size);

        out.extend_from_slice(b"SARC");
        out.extend_from_slice(&(HEADER_SIZE as u16).to_le_bytes());
        out.extend_from_slice(&0xFEFFu16.to_le_bytes());
        out.extend_from_slice(&(file_size as u32).to_le_bytes());
        out.extend_from_slice(&(data_offset as u32).to_le_bytes());
        out.extend_from_slice(&0x0100u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());

        out.extend_from_slice(b"SFAT");
        out.extend_from_slice(&(SFAT_HEADER_SIZE as u16).to_le_bytes());
        out.extend_from_slice(&(nodes.len() as u16).to_le_bytes());
        out.extend_from_slice(&self.hash_key.to_le_bytes());
        for (index, (hash, _, _, _)) in nodes.iter().enumerate() {
            out.extend_from_slice(&hash.to_le_bytes());
            out.extend_from_slice(&(0x0100_0000 | name_offsets[index]).to_le_bytes());
            out.extend_from_slice(&ranges[index].0.to_le_bytes());
            out.extend_from_slice(&ranges[index].1.to_le_bytes());
        }

        out.extend_from_slice(b"SFNT");
        out.extend_from_slice(&(SFNT_HEADER_SIZE as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&name_table);

        out.resize(data_offset, 0);
        out.extend_from_slice(&blob);
        out
    }
}

fn align_up(value: usize, alignment: usize) -> usize {
    // Written the long way round: `div_ceil` and `is_multiple_of` are not
    // available on the rustc version the skyline toolchain pins.
    let alignment = alignment.max(1);
    (value + alignment - 1) / alignment * alignment
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_like_the_game() {
        // Known SARC hashes (key 0x65).
        assert_eq!(name_hash("", 0x65), 0);
        assert_eq!(name_hash("A", 0x65), 0x41);
        assert_eq!(name_hash("AB", 0x65), 0x41 * 0x65 + 0x42);
    }

    #[test]
    fn builds_a_readable_archive() {
        let mut builder = SarcBuilder::new();
        builder.insert("Foo/Bar.byml", vec![1, 2, 3, 4]);
        builder.insert("Baz.bgyml", vec![9; 100]);

        let built = builder.build();
        let sarc = Sarc::parse(&built).unwrap();
        assert_eq!(sarc.len(), 2);
        assert_eq!(sarc.get("Foo/Bar.byml").unwrap().data, &[1, 2, 3, 4]);
        assert_eq!(sarc.get("Baz.bgyml").unwrap().data, &[9; 100]);
    }

    #[test]
    fn nodes_are_sorted_by_hash() {
        let mut builder = SarcBuilder::new();
        for name in ["zzz.byml", "aaa.byml", "mmm.byml", "Actor/Foo.bgyml"] {
            builder.insert(name, vec![0; 8]);
        }
        let built = builder.build();
        let node_count = u16::from_le_bytes([built[0x1A], built[0x1B]]) as usize;
        let mut previous = 0;
        for index in 0..node_count {
            let offset = 0x20 + index * 0x10;
            let hash = u32::from_le_bytes(built[offset..offset + 4].try_into().unwrap());
            assert!(hash >= previous, "hashes out of order");
            previous = hash;
        }
    }
}
