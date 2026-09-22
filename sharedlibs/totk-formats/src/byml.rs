//! BYML (binary YAML), versions 2 to 7, either byte order.
//!
//! Besides Nintendo's node types this understands the two TKMM adds for
//! changelogs: `ArrayChangelog` (0xFD, a list of add/edit/remove operations on
//! an array) and `Changelog` (0xFE, a "remove this key" marker). The layout of
//! both, and the order the writer lays nodes out in, follow Tkmm.BymlLibrary so
//! files round-trip with TKMM.

use alloc::rc::Rc;

use hashbrown::HashMap;

use crate::prelude::*;
use crate::{Error, Result};

pub use crate::vecmap::VecMap;

const MAGIC_LE: [u8; 2] = *b"YB";
const MAGIC_BE: [u8; 2] = *b"BY";

/// How a document was stored, so it can be written back the same way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Format {
    pub version: u16,
    pub big_endian: bool,
}

impl Default for Format {
    fn default() -> Self {
        Format {
            version: 7,
            big_endian: false,
        }
    }
}
const HEADER_SIZE: usize = 0x10;

pub mod node_type {
    pub const NONE: u8 = 0x00;
    pub const HASH_MAP32: u8 = 0x20;
    pub const HASH_MAP64: u8 = 0x21;
    pub const STRING: u8 = 0xA0;
    pub const BINARY: u8 = 0xA1;
    pub const BINARY_ALIGNED: u8 = 0xA2;
    pub const ARRAY: u8 = 0xC0;
    pub const MAP: u8 = 0xC1;
    pub const STRING_TABLE: u8 = 0xC2;
    pub const BOOL: u8 = 0xD0;
    pub const INT: u8 = 0xD1;
    pub const FLOAT: u8 = 0xD2;
    pub const UINT32: u8 = 0xD3;
    pub const INT64: u8 = 0xD4;
    pub const UINT64: u8 = 0xD5;
    pub const DOUBLE: u8 = 0xD6;
    pub const ARRAY_CHANGELOG: u8 = 0xFD;
    pub const CHANGELOG: u8 = 0xFE;
    pub const NULL: u8 = 0xFF;
}

use node_type as nt;

/// TKMM change kinds. The numeric values are part of the file format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChangeType {
    Add = 0,
    Edit = 1,
    Remove = 2,
}

impl ChangeType {
    fn from_raw(value: u32) -> Result<ChangeType> {
        match value {
            0 => Ok(ChangeType::Add),
            1 => Ok(ChangeType::Edit),
            2 => Ok(ChangeType::Remove),
            _ => Err(Error::Invalid("unknown BYML change type")),
        }
    }
}

/// A map key. Shared between every map of a document that uses the same key
/// (a GameDataList has 1.2 million entries but a few hundred distinct keys),
/// so reading a document does not allocate a string per entry.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Key(Rc<str>);

impl Key {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::ops::Deref for Key {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

impl core::borrow::Borrow<str> for Key {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Debug for Key {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Debug::fmt(&*self.0, f)
    }
}

impl core::fmt::Display for Key {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for Key {
    fn from(value: &str) -> Self {
        Key(Rc::from(value))
    }
}

impl From<String> for Key {
    fn from(value: String) -> Self {
        Key(Rc::from(value))
    }
}

impl From<&String> for Key {
    fn from(value: &String) -> Self {
        Key(Rc::from(value.as_str()))
    }
}

impl PartialEq<str> for Key {
    fn eq(&self, other: &str) -> bool {
        &*self.0 == other
    }
}

impl PartialEq<&str> for Key {
    fn eq(&self, other: &&str) -> bool {
        &*self.0 == *other
    }
}

pub type Map = VecMap<Key, Byml>;

impl<V> FromIterator<(String, V)> for VecMap<Key, V> {
    fn from_iter<I: IntoIterator<Item = (String, V)>>(iter: I) -> Self {
        iter.into_iter().map(|(k, v)| (Key::from(k), v)).collect()
    }
}

impl<V> From<alloc::collections::BTreeMap<String, V>> for VecMap<Key, V> {
    fn from(map: alloc::collections::BTreeMap<String, V>) -> Self {
        map.into_iter().collect()
    }
}

impl<K: Ord, V> From<alloc::collections::BTreeMap<K, V>> for VecMap<K, V> {
    fn from(map: alloc::collections::BTreeMap<K, V>) -> Self {
        map.into_iter().collect()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ArrayChange {
    pub index: i32,
    pub change: ChangeType,
    pub node: Byml,
    pub key_primary: Option<Byml>,
    pub key_secondary: Option<Byml>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Byml {
    Null,
    String(String),
    Binary(Vec<u8>),
    BinaryAligned(Vec<u8>, i32),
    Array(Vec<Byml>),
    Map(Map),
    HashMap32(VecMap<u32, Byml>),
    HashMap64(VecMap<u64, Byml>),
    ArrayChangelog(Vec<ArrayChange>),
    Bool(bool),
    Int(i32),
    Float(f32),
    UInt32(u32),
    Int64(i64),
    UInt64(u64),
    Double(f64),
    /// TKMM's key removal marker.
    Changelog(ChangeType),
}

impl Default for Byml {
    fn default() -> Self {
        Byml::Null
    }
}

impl Byml {
    pub fn node_type(&self) -> u8 {
        match self {
            Byml::Null => nt::NULL,
            Byml::String(_) => nt::STRING,
            Byml::Binary(_) => nt::BINARY,
            Byml::BinaryAligned(_, _) => nt::BINARY_ALIGNED,
            Byml::Array(_) => nt::ARRAY,
            Byml::Map(_) => nt::MAP,
            Byml::HashMap32(_) => nt::HASH_MAP32,
            Byml::HashMap64(_) => nt::HASH_MAP64,
            Byml::ArrayChangelog(_) => nt::ARRAY_CHANGELOG,
            Byml::Bool(_) => nt::BOOL,
            Byml::Int(_) => nt::INT,
            Byml::Float(_) => nt::FLOAT,
            Byml::UInt32(_) => nt::UINT32,
            Byml::Int64(_) => nt::INT64,
            Byml::UInt64(_) => nt::UINT64,
            Byml::Double(_) => nt::DOUBLE,
            Byml::Changelog(_) => nt::CHANGELOG,
        }
    }

    /// Containers: nodes other nodes can live in.
    pub fn is_container(&self) -> bool {
        matches!(
            self,
            Byml::Array(_) | Byml::Map(_) | Byml::HashMap32(_) | Byml::HashMap64(_) | Byml::ArrayChangelog(_)
        )
    }

    /// Nodes whose value fits inline in the parent's 4 byte slot.
    fn is_inline(&self) -> bool {
        matches!(
            self,
            Byml::String(_)
                | Byml::Bool(_)
                | Byml::Int(_)
                | Byml::Float(_)
                | Byml::UInt32(_)
                | Byml::Changelog(_)
                | Byml::Null
        )
    }

    pub fn is_remove(&self) -> bool {
        matches!(self, Byml::Changelog(ChangeType::Remove))
    }

    pub fn as_map(&self) -> Option<&Map> {
        match self {
            Byml::Map(map) => Some(map),
            _ => None,
        }
    }

    pub fn as_map_mut(&mut self) -> Option<&mut Map> {
        match self {
            Byml::Map(map) => Some(map),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&Vec<Byml>> {
        match self {
            Byml::Array(array) => Some(array),
            _ => None,
        }
    }

    pub fn as_array_mut(&mut self) -> Option<&mut Vec<Byml>> {
        match self {
            Byml::Array(array) => Some(array),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Byml::String(value) => Some(value),
            _ => None,
        }
    }

    /// Value comparison the way TKMM does it: floating point values within
    /// 0.0001 of each other are considered equal.
    pub fn value_eq(&self, other: &Byml) -> bool {
        match (self, other) {
            (Byml::Float(a), Byml::Float(b)) => {
                let difference = a - b;
                difference < 0.0001 && difference > -0.0001
            }
            (Byml::Double(a), Byml::Double(b)) => {
                // |a - b| < 0.0001, spelled out: `abs` is not in `core`.
                let difference = a - b;
                difference < 0.0001 && difference > -0.0001
            }
            (Byml::Array(a), Byml::Array(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.value_eq(y))
            }
            (Byml::Map(a), Byml::Map(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|((ka, va), (kb, vb))| ka == kb && va.value_eq(vb))
            }
            (Byml::HashMap32(a), Byml::HashMap32(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|((ka, va), (kb, vb))| ka == kb && va.value_eq(vb))
            }
            (Byml::HashMap64(a), Byml::HashMap64(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|((ka, va), (kb, vb))| ka == kb && va.value_eq(vb))
            }
            // Like Tkmm.BymlLibrary: only the operations and their nodes count.
            (Byml::ArrayChangelog(a), Byml::ArrayChangelog(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.change == y.change && x.node.value_eq(&y.node))
            }
            _ => self == other,
        }
    }

    pub fn from_binary(data: &[u8]) -> Result<Byml> {
        Self::parse(data).map(|(byml, _)| byml)
    }

    /// Parses a document, also returning its version so it can be written back
    /// the same way.
    pub fn from_binary_with_version(data: &[u8]) -> Result<(Byml, u16)> {
        Self::parse(data).map(|(byml, format)| (byml, format.version))
    }

    /// Parses a document, also returning how it was stored.
    pub fn parse(data: &[u8]) -> Result<(Byml, Format)> {
        let magic = data.get(0..2).ok_or(Error::Truncated { what: "BYML header" })?;
        let big_endian = if magic == MAGIC_LE {
            false
        } else if magic == MAGIC_BE {
            true
        } else {
            return Err(Error::BadMagic {
                expected: "YB",
                got: [magic[0], magic[1], 0, 0],
            });
        };

        let endian = Endian { big: big_endian };
        let version = endian.u16(data, 2)?;
        if !(2..=7).contains(&version) {
            return Err(Error::Invalid("unsupported BYML version"));
        }
        let format = Format { version, big_endian };

        let key_table_offset = endian.u32(data, 4)? as usize;
        let string_table_offset = endian.u32(data, 8)? as usize;
        let root_offset = endian.u32(data, 12)? as usize;

        let reader = Reader {
            data,
            endian,
            keys: read_string_table(data, endian, key_table_offset)?
                .into_iter()
                .map(Key::from)
                .collect(),
            strings: read_string_table(data, endian, string_table_offset)?,
        };

        if root_offset == 0 {
            return Ok((Byml::Null, format));
        }

        let root_type = *data.get(root_offset).ok_or(Error::Truncated { what: "BYML root" })?;
        let root = reader.read_node(root_type, root_offset as u32, 0)?;
        Ok((root, format))
    }

    /// Writes a little endian document.
    pub fn to_binary(&self, version: u16) -> Vec<u8> {
        self.write(Format {
            version,
            big_endian: false,
        })
    }

    pub fn write(&self, format: Format) -> Vec<u8> {
        Writer::new(self, format).write()
    }
}

/// A node of a [`Document`], not read yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeRef {
    pub node_type: u8,
    value: u32,
}

/// A document read on demand. For large files (a GameDataList has 1.4 million
/// nodes) where only a few rows at a time matter, this avoids building the
/// whole tree.
pub struct Document<'a> {
    reader: Reader<'a>,
    root: Option<NodeRef>,
    format: Format,
}

impl<'a> Document<'a> {
    pub fn open(data: &'a [u8]) -> Result<Document<'a>> {
        let magic = data.get(0..2).ok_or(Error::Truncated { what: "BYML header" })?;
        let big_endian = if magic == MAGIC_LE {
            false
        } else if magic == MAGIC_BE {
            true
        } else {
            return Err(Error::BadMagic {
                expected: "YB",
                got: [magic[0], magic[1], 0, 0],
            });
        };

        let endian = Endian { big: big_endian };
        let version = endian.u16(data, 2)?;
        if !(2..=7).contains(&version) {
            return Err(Error::Invalid("unsupported BYML version"));
        }

        let key_table_offset = endian.u32(data, 4)? as usize;
        let string_table_offset = endian.u32(data, 8)? as usize;
        let root_offset = endian.u32(data, 12)? as usize;

        let reader = Reader {
            data,
            endian,
            keys: read_string_table(data, endian, key_table_offset)?
                .into_iter()
                .map(Key::from)
                .collect(),
            strings: read_string_table(data, endian, string_table_offset)?,
        };

        let root = if root_offset == 0 {
            None
        } else {
            let node_type = *data.get(root_offset).ok_or(Error::Truncated { what: "BYML root" })?;
            Some(NodeRef {
                node_type,
                value: root_offset as u32,
            })
        };

        Ok(Document {
            reader,
            root,
            format: Format { version, big_endian },
        })
    }

    pub fn format(&self) -> Format {
        self.format
    }

    pub fn root(&self) -> Option<NodeRef> {
        self.root
    }

    /// Reads a node and everything under it.
    pub fn load(&self, node: NodeRef) -> Result<Byml> {
        self.reader.read_node(node.node_type, node.value, 0)
    }

    /// The entries of a string-keyed map.
    pub fn map_entries(&self, map: NodeRef) -> Result<Vec<(Key, NodeRef)>> {
        if map.node_type != nt::MAP {
            return Err(Error::Invalid("BYML node is not a map"));
        }
        let offset = map.value as usize;
        let count = self.reader.container_count(offset, nt::MAP)?;
        let mut entries = Vec::with_capacity(count);
        for index in 0..count {
            let entry = offset + 4 + index * 8;
            let (key_index, node_type) = self.reader.endian.key_and_type(self.reader.data, entry)?;
            let value = self.reader.endian.u32(self.reader.data, entry + 4)?;
            let key = self
                .reader
                .keys
                .get(key_index)
                .ok_or(Error::Invalid("BYML key index out of range"))?
                .clone();
            entries.push((key, NodeRef { node_type, value }));
        }
        Ok(entries)
    }

    /// One entry of a string-keyed map, without reading the others.
    pub fn map_get(&self, map: NodeRef, key: &str) -> Result<Option<NodeRef>> {
        if map.node_type != nt::MAP {
            return Ok(None);
        }
        let offset = map.value as usize;
        let count = self.reader.container_count(offset, nt::MAP)?;
        for index in 0..count {
            let entry = offset + 4 + index * 8;
            let (key_index, node_type) = self.reader.endian.key_and_type(self.reader.data, entry)?;
            if self.reader.keys.get(key_index).map_or(false, |k| k.as_str() == key) {
                let value = self.reader.endian.u32(self.reader.data, entry + 4)?;
                return Ok(Some(NodeRef { node_type, value }));
            }
        }
        Ok(None)
    }

    /// The items of an array.
    pub fn array_items(&self, array: NodeRef) -> Result<Vec<NodeRef>> {
        if array.node_type != nt::ARRAY {
            return Err(Error::Invalid("BYML node is not an array"));
        }
        let offset = array.value as usize;
        let count = self.reader.container_count(offset, nt::ARRAY)?;
        let types = self
            .reader
            .data
            .get(offset + 4..offset + 4 + count)
            .ok_or(Error::Truncated { what: "BYML array" })?;
        let values_start = offset + 4 + align4(count);
        let mut items = Vec::with_capacity(count);
        for (index, &node_type) in types.iter().enumerate() {
            let value = self.reader.endian.u32(self.reader.data, values_start + index * 4)?;
            items.push(NodeRef { node_type, value });
        }
        Ok(items)
    }
}

/// [`Byml::value_eq`] for optional nodes: two missing nodes are equal.
pub fn option_value_eq(a: &Option<Byml>, b: &Option<Byml>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => a.value_eq(b),
        _ => false,
    }
}

impl From<&str> for Byml {
    fn from(value: &str) -> Self {
        Byml::String(value.to_string())
    }
}

impl From<String> for Byml {
    fn from(value: String) -> Self {
        Byml::String(value)
    }
}

impl From<i32> for Byml {
    fn from(value: i32) -> Self {
        Byml::Int(value)
    }
}

impl From<u32> for Byml {
    fn from(value: u32) -> Self {
        Byml::UInt32(value)
    }
}

// --- reading -----------------------------------------------------------------

#[derive(Clone, Copy)]
struct Endian {
    big: bool,
}

impl Endian {
    fn u16(self, data: &[u8], offset: usize) -> Result<u16> {
        let b = data.get(offset..offset + 2).ok_or(Error::Truncated { what: "BYML data" })?;
        let b = [b[0], b[1]];
        Ok(if self.big { u16::from_be_bytes(b) } else { u16::from_le_bytes(b) })
    }

    fn u32(self, data: &[u8], offset: usize) -> Result<u32> {
        let b = data.get(offset..offset + 4).ok_or(Error::Truncated { what: "BYML data" })?;
        let b = [b[0], b[1], b[2], b[3]];
        Ok(if self.big { u32::from_be_bytes(b) } else { u32::from_le_bytes(b) })
    }

    fn u64(self, data: &[u8], offset: usize) -> Result<u64> {
        let b: [u8; 8] = data
            .get(offset..offset + 8)
            .ok_or(Error::Truncated { what: "BYML data" })?
            .try_into()
            .unwrap();
        Ok(if self.big { u64::from_be_bytes(b) } else { u64::from_le_bytes(b) })
    }

    /// A container header: node type in the first byte, 24 bit count after it.
    fn container(self, data: &[u8], offset: usize) -> Result<(u8, usize)> {
        let value = self.u32(data, offset)?;
        Ok(if self.big {
            ((value >> 24) as u8, (value & 0x00FF_FFFF) as usize)
        } else {
            ((value & 0xFF) as u8, (value >> 8) as usize)
        })
    }

    /// A map entry's first field: 24 bit key index, then the node type.
    fn key_and_type(self, data: &[u8], offset: usize) -> Result<(usize, u8)> {
        let value = self.u32(data, offset)?;
        Ok(if self.big {
            ((value >> 8) as usize, (value & 0xFF) as u8)
        } else {
            ((value & 0x00FF_FFFF) as usize, (value >> 24) as u8)
        })
    }
}

fn read_string_table(data: &[u8], endian: Endian, offset: usize) -> Result<Vec<String>> {
    if offset == 0 {
        return Ok(Vec::new());
    }

    let (node_type, count) = endian.container(data, offset)?;
    if node_type != nt::STRING_TABLE {
        return Err(Error::Invalid("BYML string table has the wrong node type"));
    }

    let mut strings = Vec::with_capacity(count);
    for index in 0..count {
        let start = offset + endian.u32(data, offset + 4 + index * 4)? as usize;
        let end = offset + endian.u32(data, offset + 4 + (index + 1) * 4)? as usize;
        let raw = data.get(start..end).ok_or(Error::Truncated { what: "BYML string" })?;
        let raw = raw.strip_suffix(&[0]).unwrap_or(raw);
        strings.push(String::from_utf8_lossy(raw).into_owned());
    }
    Ok(strings)
}

struct Reader<'a> {
    data: &'a [u8],
    endian: Endian,
    keys: Vec<Key>,
    strings: Vec<String>,
}

/// Deeper than any real document; stops a malformed file with a cycle from
/// recursing forever.
const MAX_DEPTH: u32 = 128;

impl<'a> Reader<'a> {
    fn read_node(&self, node_type: u8, value: u32, depth: u32) -> Result<Byml> {
        if depth > MAX_DEPTH {
            return Err(Error::Invalid("BYML nesting too deep"));
        }
        let offset = value as usize;

        Ok(match node_type {
            nt::NULL | nt::NONE => Byml::Null,
            nt::STRING => Byml::String(
                self.strings
                    .get(value as usize)
                    .cloned()
                    .ok_or(Error::Invalid("BYML string index out of range"))?,
            ),
            nt::BOOL => Byml::Bool(value != 0),
            nt::INT => Byml::Int(value as i32),
            nt::FLOAT => Byml::Float(f32::from_bits(value)),
            nt::UINT32 => Byml::UInt32(value),
            nt::CHANGELOG => Byml::Changelog(ChangeType::from_raw(value)?),
            nt::INT64 => Byml::Int64(self.endian.u64(self.data, offset)? as i64),
            nt::UINT64 => Byml::UInt64(self.endian.u64(self.data, offset)?),
            nt::DOUBLE => Byml::Double(f64::from_bits(self.endian.u64(self.data, offset)?)),
            nt::BINARY => {
                let size = self.endian.u32(self.data, offset)? as usize;
                let bytes = self
                    .data
                    .get(offset + 4..offset + 4 + size)
                    .ok_or(Error::Truncated { what: "BYML binary" })?;
                Byml::Binary(bytes.to_vec())
            }
            nt::BINARY_ALIGNED => {
                let size = self.endian.u32(self.data, offset)? as usize;
                let alignment = self.endian.u32(self.data, offset + 4)? as i32;
                let bytes = self
                    .data
                    .get(offset + 8..offset + 8 + size)
                    .ok_or(Error::Truncated { what: "BYML binary" })?;
                Byml::BinaryAligned(bytes.to_vec(), alignment)
            }
            nt::ARRAY => self.read_array(offset, depth)?,
            nt::MAP => self.read_map(offset, depth)?,
            nt::HASH_MAP32 => self.read_hash_map32(offset, depth)?,
            nt::HASH_MAP64 => self.read_hash_map64(offset, depth)?,
            nt::ARRAY_CHANGELOG => self.read_array_changelog(offset, depth)?,
            _ => return Err(Error::Invalid("unsupported BYML node type")),
        })
    }

    fn container_count(&self, offset: usize, expected: u8) -> Result<usize> {
        let (node_type, count) = self.endian.container(self.data, offset)?;
        if node_type != expected {
            return Err(Error::Invalid("BYML container has an unexpected node type"));
        }
        Ok(count)
    }

    fn read_array(&self, offset: usize, depth: u32) -> Result<Byml> {
        let count = self.container_count(offset, nt::ARRAY)?;
        let types = self
            .data
            .get(offset + 4..offset + 4 + count)
            .ok_or(Error::Truncated { what: "BYML array" })?;
        let values_start = offset + 4 + align4(count);

        let mut array = Vec::with_capacity(count);
        for (index, &node_type) in types.iter().enumerate() {
            let value = self.endian.u32(self.data, values_start + index * 4)?;
            array.push(self.read_node(node_type, value, depth + 1)?);
        }
        Ok(Byml::Array(array))
    }

    fn read_map(&self, offset: usize, depth: u32) -> Result<Byml> {
        let count = self.container_count(offset, nt::MAP)?;
        let mut map = VecMap::with_capacity(count);
        for index in 0..count {
            let entry = offset + 4 + index * 8;
            let (key_index, node_type) = self.endian.key_and_type(self.data, entry)?;
            let value = self.endian.u32(self.data, entry + 4)?;
            let key = self
                .keys
                .get(key_index)
                .ok_or(Error::Invalid("BYML key index out of range"))?
                .clone();
            map.insert(key, self.read_node(node_type, value, depth + 1)?);
        }
        Ok(Byml::Map(map))
    }

    fn read_hash_map32(&self, offset: usize, depth: u32) -> Result<Byml> {
        let count = self.container_count(offset, nt::HASH_MAP32)?;
        let types_start = offset + 4 + count * 8;
        let mut map = VecMap::with_capacity(count);
        for index in 0..count {
            let entry = offset + 4 + index * 8;
            let hash = self.endian.u32(self.data, entry)?;
            let value = self.endian.u32(self.data, entry + 4)?;
            let node_type = *self
                .data
                .get(types_start + index)
                .ok_or(Error::Truncated { what: "BYML hash map" })?;
            map.insert(hash, self.read_node(node_type, value, depth + 1)?);
        }
        Ok(Byml::HashMap32(map))
    }

    fn read_hash_map64(&self, offset: usize, depth: u32) -> Result<Byml> {
        let count = self.container_count(offset, nt::HASH_MAP64)?;
        let types_start = offset + 4 + count * 12;
        let mut map = VecMap::with_capacity(count);
        for index in 0..count {
            let entry = offset + 4 + index * 12;
            let hash = self.endian.u64(self.data, entry)?;
            let value = self.endian.u32(self.data, entry + 8)?;
            let node_type = *self
                .data
                .get(types_start + index)
                .ok_or(Error::Truncated { what: "BYML hash map" })?;
            map.insert(hash, self.read_node(node_type, value, depth + 1)?);
        }
        Ok(Byml::HashMap64(map))
    }

    fn read_array_changelog(&self, offset: usize, depth: u32) -> Result<Byml> {
        let count = self.container_count(offset, nt::ARRAY_CHANGELOG)?;
        let types_start = offset + 4 + count * 20;
        let mut changes = Vec::with_capacity(count);
        for index in 0..count {
            let entry = offset + 4 + index * 20;
            let types = types_start + index * 4;
            let type_bytes = self
                .data
                .get(types..types + 3)
                .ok_or(Error::Truncated { what: "BYML array changelog" })?;

            let key_primary = match type_bytes[1] {
                nt::NONE => None,
                t => Some(self.read_node(t, self.endian.u32(self.data, entry + 12)?, depth + 1)?),
            };
            let key_secondary = match type_bytes[2] {
                nt::NONE => None,
                t => Some(self.read_node(t, self.endian.u32(self.data, entry + 16)?, depth + 1)?),
            };

            changes.push(ArrayChange {
                index: self.endian.u32(self.data, entry)? as i32,
                change: ChangeType::from_raw(self.endian.u32(self.data, entry + 4)?)?,
                node: self.read_node(type_bytes[0], self.endian.u32(self.data, entry + 8)?, depth + 1)?,
                key_primary,
                key_secondary,
            });
        }
        Ok(Byml::ArrayChangelog(changes))
    }
}

fn align4(value: usize) -> usize {
    (value + 3) & !3
}

// --- writing -----------------------------------------------------------------

/// UTF-16 ordinal comparison, which is how the key and string tables are sorted
/// by the tools the game (and TKMM) were built with.
fn utf16_ordinal(a: &str, b: &str) -> core::cmp::Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// The strings sorted as the tables need them, and each one's index. Both
/// borrow from the document: copying a GameDataList's strings would cost
/// megabytes for nothing.
fn sorted_table<'a>(set: HashMap<&'a str, ()>) -> (Vec<&'a str>, HashMap<&'a str, u32>) {
    let mut strings: Vec<&str> = set.into_keys().collect();
    strings.sort_by(|a, b| utf16_ordinal(a, b));
    let indices = strings.iter().enumerate().map(|(index, s)| (*s, index as u32)).collect();
    (strings, indices)
}

struct Writer<'a> {
    root: &'a Byml,
    format: Format,
    out: Vec<u8>,
    key_indices: HashMap<&'a str, u32>,
    string_indices: HashMap<&'a str, u32>,
    /// Structural hash -> most recent node written with that hash, as an index
    /// into `written_nodes`, whose entries chain to older nodes with the same
    /// hash. No allocation per node: a GameDataList stages hundreds of
    /// thousands of them.
    written: HashMap<u64, u32>,
    /// (node, offset it was written at, previous entry with the same hash + 1).
    written_nodes: Vec<(&'a Byml, u32, u32)>,
}

impl<'a> Writer<'a> {
    fn new(root: &'a Byml, format: Format) -> Writer<'a> {
        Writer {
            root,
            format,
            out: Vec::new(),
            key_indices: HashMap::new(),
            string_indices: HashMap::new(),
            written: HashMap::new(),
            written_nodes: Vec::new(),
        }
    }

    fn write(mut self) -> Vec<u8> {
        let mut keys = HashMap::new();
        let mut strings = HashMap::new();
        collect_strings(self.root, &mut keys, &mut strings);

        let (key_list, key_indices) = sorted_table(keys);
        let (string_list, string_indices) = sorted_table(strings);
        self.key_indices = key_indices;
        self.string_indices = string_indices;

        self.out.resize(HEADER_SIZE, 0);
        let key_table_offset = self.write_string_table(&key_list);
        let string_table_offset = self.write_string_table(&string_list);
        let root_offset = self.out.len() as u32;

        let root = self.root;
        if root.is_container() {
            self.write_container(root);
        } else if !root.is_inline() {
            self.write_special(root);
        }

        let big = self.format.big_endian;
        self.out[0..2].copy_from_slice(if big { &MAGIC_BE } else { &MAGIC_LE });
        let version = self.format.version;
        self.out[2..4].copy_from_slice(&if big { version.to_be_bytes() } else { version.to_le_bytes() });
        let offsets = [key_table_offset, string_table_offset, root_offset];
        for (i, value) in offsets.into_iter().enumerate() {
            self.put_u32(4 + i * 4, value);
        }
        self.out
    }

    fn u32_bytes(&self, value: u32) -> [u8; 4] {
        if self.format.big_endian {
            value.to_be_bytes()
        } else {
            value.to_le_bytes()
        }
    }

    fn push_u32(&mut self, value: u32) {
        let bytes = self.u32_bytes(value);
        self.out.extend_from_slice(&bytes);
    }

    fn push_u64(&mut self, value: u64) {
        let bytes = if self.format.big_endian {
            value.to_be_bytes()
        } else {
            value.to_le_bytes()
        };
        self.out.extend_from_slice(&bytes);
    }

    fn put_u32(&mut self, position: usize, value: u32) {
        let bytes = self.u32_bytes(value);
        self.out[position..position + 4].copy_from_slice(&bytes);
    }

    fn push_container_header(&mut self, node_type: u8, count: usize) {
        let value = if self.format.big_endian {
            ((node_type as u32) << 24) | count as u32
        } else {
            ((count as u32) << 8) | node_type as u32
        };
        self.push_u32(value);
    }

    fn push_key_and_type(&mut self, key_index: u32, node_type: u8) {
        let value = if self.format.big_endian {
            (key_index << 8) | node_type as u32
        } else {
            key_index | ((node_type as u32) << 24)
        };
        self.push_u32(value);
    }

    fn align(&mut self, alignment: usize) {
        while self.out.len() % alignment != 0 {
            self.out.push(0);
        }
    }

    fn write_string_table(&mut self, strings: &[&str]) -> u32 {
        if strings.is_empty() {
            return 0;
        }
        let table_offset = self.out.len() as u32;
        self.push_container_header(nt::STRING_TABLE, strings.len());

        let mut offset = 4 + (strings.len() as u32 + 1) * 4;
        self.push_u32(offset);
        for string in strings {
            offset += string.len() as u32 + 1;
            self.push_u32(offset);
        }
        for string in strings {
            self.out.extend_from_slice(string.as_bytes());
            self.out.push(0);
        }
        self.align(4);
        table_offset
    }

    /// Value stored in a parent's slot for an inline node.
    fn inline_value(&self, node: &Byml) -> u32 {
        match node {
            Byml::String(value) => self.string_indices[value.as_str()],
            Byml::Bool(value) => *value as u32,
            Byml::Int(value) => *value as u32,
            Byml::Float(value) => value.to_bits(),
            Byml::UInt32(value) => *value,
            Byml::Changelog(change) => *change as u32,
            _ => 0,
        }
    }

    /// Writes a container's body; non-inline children are laid out right after
    /// it, in order, and their offsets patched into the slots left for them.
    fn write_container(&mut self, container: &'a Byml) {
        let mut staged: Vec<(usize, &'a Byml)> = Vec::new();

        match container {
            Byml::Array(items) => {
                self.push_container_header(nt::ARRAY, items.len());
                for item in items {
                    self.out.push(item.node_type());
                }
                self.align(4);
                for item in items {
                    self.slot(item, &mut staged);
                }
            }
            Byml::Map(map) => {
                self.push_container_header(nt::MAP, map.len());
                let mut entries: Vec<(&Key, &Byml)> = map.iter().collect();
                entries.sort_by(|a, b| utf16_ordinal(a.0, b.0));
                for (key, value) in entries {
                    let index = self.key_indices[key.as_str()];
                    self.push_key_and_type(index, value.node_type());
                    self.slot(value, &mut staged);
                }
            }
            Byml::HashMap32(map) => {
                self.push_container_header(nt::HASH_MAP32, map.len());
                for (hash, value) in map {
                    self.push_u32(*hash);
                    self.slot(value, &mut staged);
                }
                for value in map.values() {
                    self.out.push(value.node_type());
                }
                self.align(4);
            }
            Byml::HashMap64(map) => {
                self.push_container_header(nt::HASH_MAP64, map.len());
                for (hash, value) in map {
                    self.push_u64(*hash);
                    self.slot(value, &mut staged);
                }
                for value in map.values() {
                    self.out.push(value.node_type());
                }
                self.align(4);
            }
            Byml::ArrayChangelog(changes) => {
                self.push_container_header(nt::ARRAY_CHANGELOG, changes.len());
                for change in changes {
                    self.push_u32(change.index as u32);
                    self.push_u32(change.change as u32);
                    self.slot(&change.node, &mut staged);
                    match &change.key_primary {
                        Some(key) => self.slot(key, &mut staged),
                        None => self.push_u32(0),
                    }
                    match &change.key_secondary {
                        Some(key) => self.slot(key, &mut staged),
                        None => self.push_u32(0),
                    }
                }
                for change in changes {
                    self.out.push(change.node.node_type());
                    self.out.push(change.key_primary.as_ref().map_or(nt::NONE, Byml::node_type));
                    self.out.push(change.key_secondary.as_ref().map_or(nt::NONE, Byml::node_type));
                    self.out.push(nt::NONE);
                }
                self.align(4);
            }
            _ => unreachable!("write_container called on a non-container"),
        }

        for (slot, node) in staged {
            let hash = structural_hash(node);
            let offset = match self.lookup_written(node, hash) {
                Some(offset) => offset,
                None => {
                    let mut position = self.out.len();
                    if let Byml::BinaryAligned(_, alignment) = node {
                        let alignment = (*alignment).max(1) as usize;
                        // The data after the 8 byte header lands on the boundary.
                        let data_start = position + 8;
                        let padding = (alignment - data_start % alignment) % alignment;
                        self.out.resize(position + padding, 0);
                        position += padding;
                    }
                    let offset = position as u32;
                    if node.is_container() {
                        self.write_container(node);
                    } else {
                        self.write_special(node);
                    }
                    self.remember_written(node, offset, hash);
                    offset
                }
            };
            self.put_u32(slot, offset);
        }
    }

    /// Writes an inline value, or leaves a placeholder for a node written later.
    fn slot(&mut self, node: &'a Byml, staged: &mut Vec<(usize, &'a Byml)>) {
        if node.is_inline() {
            let value = self.inline_value(node);
            self.push_u32(value);
        } else {
            staged.push((self.out.len(), node));
            self.push_u32(0);
        }
    }

    fn write_special(&mut self, node: &Byml) {
        match node {
            Byml::Binary(bytes) => {
                self.push_u32(bytes.len() as u32);
                self.out.extend_from_slice(bytes);
                self.align(4);
            }
            Byml::BinaryAligned(bytes, alignment) => {
                self.push_u32(bytes.len() as u32);
                self.push_u32(*alignment as u32);
                self.out.extend_from_slice(bytes);
            }
            Byml::Int64(value) => self.push_u64(*value as u64),
            Byml::UInt64(value) => self.push_u64(*value),
            Byml::Double(value) => self.push_u64(value.to_bits()),
            _ => {}
        }
    }

    fn lookup_written(&self, node: &Byml, hash: u64) -> Option<u32> {
        let mut link = self.written.get(&hash).map(|&index| index + 1)?;
        while link != 0 {
            let (existing, offset, previous) = self.written_nodes[(link - 1) as usize];
            if structurally_equal(existing, node) {
                return Some(offset);
            }
            link = previous;
        }
        None
    }

    fn remember_written(&mut self, node: &'a Byml, offset: u32, hash: u64) {
        let index = self.written_nodes.len() as u32;
        let previous = self.written.insert(hash, index).map_or(0, |i| i + 1);
        self.written_nodes.push((node, offset, previous));
    }
}

fn collect_strings<'a>(node: &'a Byml, keys: &mut HashMap<&'a str, ()>, strings: &mut HashMap<&'a str, ()>) {
    match node {
        Byml::String(value) => {
            strings.insert(value, ());
        }
        Byml::Array(items) => items.iter().for_each(|item| collect_strings(item, keys, strings)),
        Byml::Map(map) => {
            for (key, value) in map {
                keys.insert(key.as_str(), ());
                collect_strings(value, keys, strings);
            }
        }
        Byml::HashMap32(map) => map.values().for_each(|value| collect_strings(value, keys, strings)),
        Byml::HashMap64(map) => map.values().for_each(|value| collect_strings(value, keys, strings)),
        Byml::ArrayChangelog(changes) => {
            for change in changes {
                collect_strings(&change.node, keys, strings);
                if let Some(key) = &change.key_primary {
                    collect_strings(key, keys, strings);
                }
                if let Some(key) = &change.key_secondary {
                    collect_strings(key, keys, strings);
                }
            }
        }
        _ => {}
    }
}

/// Exact (bitwise) equality, used to share identical nodes in the output.
fn structurally_equal(a: &Byml, b: &Byml) -> bool {
    match (a, b) {
        (Byml::Float(x), Byml::Float(y)) => x.to_bits() == y.to_bits(),
        (Byml::Double(x), Byml::Double(y)) => x.to_bits() == y.to_bits(),
        (Byml::Array(x), Byml::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| structurally_equal(p, q))
        }
        (Byml::Map(x), Byml::Map(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|((kp, vp), (kq, vq))| kp == kq && structurally_equal(vp, vq))
        }
        (Byml::HashMap32(x), Byml::HashMap32(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|((kp, vp), (kq, vq))| kp == kq && structurally_equal(vp, vq))
        }
        (Byml::HashMap64(x), Byml::HashMap64(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|((kp, vp), (kq, vq))| kp == kq && structurally_equal(vp, vq))
        }
        (Byml::ArrayChangelog(x), Byml::ArrayChangelog(y)) => {
            x.len() == y.len()
                && x.iter().zip(y).all(|(p, q)| {
                    p.index == q.index
                        && p.change == q.change
                        && structurally_equal(&p.node, &q.node)
                        && match (&p.key_primary, &q.key_primary) {
                            (None, None) => true,
                            (Some(a), Some(b)) => structurally_equal(a, b),
                            _ => false,
                        }
                        && match (&p.key_secondary, &q.key_secondary) {
                            (None, None) => true,
                            (Some(a), Some(b)) => structurally_equal(a, b),
                            _ => false,
                        }
                })
        }
        _ => a == b,
    }
}

fn structural_hash(node: &Byml) -> u64 {
    use core::hash::{BuildHasher, Hasher};
    let mut hasher = foldhash::quality::FixedState::with_seed(0).build_hasher();
    hash_node(node, &mut hasher);
    hasher.finish()
}

fn hash_node<H: core::hash::Hasher>(node: &Byml, state: &mut H) {
    use core::hash::Hash;
    node.node_type().hash(state);
    match node {
        Byml::Null => {}
        Byml::String(value) => value.hash(state),
        Byml::Binary(bytes) => bytes.hash(state),
        Byml::BinaryAligned(bytes, alignment) => {
            bytes.hash(state);
            alignment.hash(state);
        }
        Byml::Array(items) => {
            items.len().hash(state);
            items.iter().for_each(|item| hash_node(item, state));
        }
        Byml::Map(map) => {
            map.len().hash(state);
            for (key, value) in map {
                key.hash(state);
                hash_node(value, state);
            }
        }
        Byml::HashMap32(map) => {
            map.len().hash(state);
            for (key, value) in map {
                key.hash(state);
                hash_node(value, state);
            }
        }
        Byml::HashMap64(map) => {
            map.len().hash(state);
            for (key, value) in map {
                key.hash(state);
                hash_node(value, state);
            }
        }
        Byml::ArrayChangelog(changes) => {
            changes.len().hash(state);
            for change in changes {
                change.index.hash(state);
                (change.change as u8).hash(state);
                hash_node(&change.node, state);
                if let Some(key) = &change.key_primary {
                    hash_node(key, state);
                }
                if let Some(key) = &change.key_secondary {
                    hash_node(key, state);
                }
            }
        }
        Byml::Bool(value) => value.hash(state),
        Byml::Int(value) => value.hash(state),
        Byml::Float(value) => value.to_bits().hash(state),
        Byml::UInt32(value) => value.hash(state),
        Byml::Int64(value) => value.hash(state),
        Byml::UInt64(value) => value.hash(state),
        Byml::Double(value) => value.to_bits().hash(state),
        Byml::Changelog(change) => (*change as u8).hash(state),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Byml {
        let mut inner = Map::new();
        inner.insert("Name".into(), Byml::from("Link"));
        inner.insert("Hp".into(), Byml::Int(12));
        inner.insert("Speed".into(), Byml::Float(1.5));
        inner.insert("Id".into(), Byml::UInt64(0x1122_3344_5566_7788));
        inner.insert("Blob".into(), Byml::Binary(vec![1, 2, 3]));
        inner.insert("Aligned".into(), Byml::BinaryAligned(vec![9; 5], 0x20));

        let mut hashes = VecMap::new();
        hashes.insert(0xDEAD_BEEF, Byml::Bool(true));
        hashes.insert(0x1234, Byml::Double(2.25));

        let mut root = Map::new();
        root.insert("Player".into(), Byml::Map(inner.clone()));
        root.insert("Copy".into(), Byml::Map(inner));
        root.insert(
            "List".into(),
            Byml::Array(vec![Byml::from("a"), Byml::Int(-3), Byml::Null, Byml::HashMap32(hashes)]),
        );
        root.insert(
            "Changes".into(),
            Byml::ArrayChangelog(vec![ArrayChange {
                index: 2,
                change: ChangeType::Edit,
                node: Byml::from("edited"),
                key_primary: Some(Byml::from("key")),
                key_secondary: None,
            }]),
        );
        root.insert("Gone".into(), Byml::Changelog(ChangeType::Remove));
        Byml::Map(root)
    }

    #[test]
    fn round_trips() {
        let original = sample();
        let binary = original.to_binary(7);
        let (parsed, version) = Byml::from_binary_with_version(&binary).unwrap();
        assert_eq!(version, 7);
        assert_eq!(parsed, original);
    }

    #[test]
    fn shares_identical_containers() {
        let binary = sample().to_binary(7);
        // "Player" and "Copy" hold the same map: it must be written once.
        let copy_size = Byml::Map(Map::from_iter(vec![(Key::from("Player"), sample().as_map().unwrap().get("Player").unwrap().clone())]))
            .to_binary(7)
            .len();
        assert!(binary.len() < copy_size * 2, "identical maps were written twice");
    }

    #[test]
    fn aligned_binary_lands_on_its_boundary() {
        let byml = Byml::Array(vec![Byml::Int(1), Byml::BinaryAligned(vec![7; 3], 0x40)]);
        let binary = byml.to_binary(7);
        let parsed = Byml::from_binary(&binary).unwrap();
        assert_eq!(parsed, byml);
        let array_offset = u32::from_le_bytes(binary[12..16].try_into().unwrap()) as usize;
        let slot = array_offset + 4 + 4 + 4; // header, 2 types padded to 4, first value
        let node = u32::from_le_bytes(binary[slot..slot + 4].try_into().unwrap()) as usize;
        assert_eq!((node + 8) % 0x40, 0);
    }

    #[test]
    fn round_trips_big_endian() {
        let original = sample();
        let format = Format {
            version: 7,
            big_endian: true,
        };
        let binary = original.write(format);
        assert_eq!(&binary[0..2], b"BY");
        let (parsed, read_format) = Byml::parse(&binary).unwrap();
        assert_eq!(read_format, format);
        assert_eq!(parsed, original);
    }

    #[test]
    fn float_comparison_is_tolerant() {
        assert!(Byml::Float(1.0).value_eq(&Byml::Float(1.00001)));
        assert!(!Byml::Float(1.0).value_eq(&Byml::Float(1.1)));
    }
}
