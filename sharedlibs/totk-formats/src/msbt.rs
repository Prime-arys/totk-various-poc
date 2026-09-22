//! MSBT message files (the `.msbt` inside `Mals/*.sarc`), little endian.
//!
//! Texts are kept as raw code units rather than decoded into markup: merging
//! only ever swaps whole entries, so there is nothing to gain from parsing the
//! control tags, and raw storage round-trips exactly. The writer lays files out
//! the way EPD's MessageStudio (used by TKMM) does: one label group, an ATR1
//! section only when some entry has an attribute, sections aligned to 16 bytes.

use hashbrown::HashMap;

use crate::prelude::*;
use crate::{Error, Result};

const MAGIC: &[u8; 8] = b"MsgStdBn";
const HEADER_SIZE: usize = 0x20;
const SECTION_HEADER_SIZE: usize = 0x10;

const LBL1: &[u8; 4] = b"LBL1";
const ATR1: &[u8; 4] = b"ATR1";
const TXT2: &[u8; 4] = b"TXT2";

pub const ENCODING_UTF8: u8 = 0;
pub const ENCODING_UTF16: u8 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MsbtEntry {
    /// Attribute string, without its terminator. `None` when absent or empty.
    pub attribute: Option<Vec<u8>>,
    /// Text in the file's encoding, without its terminator. Control tags are
    /// kept as they are.
    pub text: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Msbt {
    encoding: u8,
    entries: Vec<(String, MsbtEntry)>,
    index: HashMap<String, usize>,
}

/// What to do when a label appears twice in a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Duplicates {
    KeepFirst,
    KeepLast,
}

fn u16_at(data: &[u8], offset: usize) -> Result<u16> {
    crate::u16_at(data, offset, "MSBT")
}

fn u32_at(data: &[u8], offset: usize) -> Result<u32> {
    crate::u32_at(data, offset, "MSBT")
}

impl Msbt {
    pub fn new(encoding: u8) -> Msbt {
        Msbt {
            encoding,
            entries: Vec::new(),
            index: HashMap::new(),
        }
    }

    pub fn parse(data: &[u8]) -> Result<Msbt> {
        Self::parse_with(data, Duplicates::KeepFirst)
    }

    pub fn parse_with(data: &[u8], duplicates: Duplicates) -> Result<Msbt> {
        if data.get(0..8) != Some(MAGIC.as_slice()) {
            return Err(Error::Invalid("not an MSBT file"));
        }
        if data.get(8..10) != Some([0xFF, 0xFE].as_slice()) {
            return Err(Error::Invalid("big endian MSBT files are not supported"));
        }
        let encoding = *data.get(0x0C).ok_or(Error::Truncated { what: "MSBT header" })?;
        if encoding != ENCODING_UTF8 && encoding != ENCODING_UTF16 {
            return Err(Error::Invalid("unsupported MSBT text encoding"));
        }
        let section_count = u16_at(data, 0x0E)? as usize;

        let mut labels: Option<&[u8]> = None;
        let mut attributes: Option<&[u8]> = None;
        let mut texts: Option<&[u8]> = None;

        let mut position = HEADER_SIZE;
        for _ in 0..section_count {
            let magic = data.get(position..position + 4).ok_or(Error::Truncated { what: "MSBT section" })?;
            let size = u32_at(data, position + 4)? as usize;
            let start = position + SECTION_HEADER_SIZE;
            let body = data.get(start..start + size).ok_or(Error::Truncated { what: "MSBT section" })?;
            match magic {
                m if m == LBL1 => labels = Some(body),
                m if m == ATR1 => attributes = Some(body),
                m if m == TXT2 => texts = Some(body),
                _ => return Err(Error::Invalid("unsupported MSBT section")),
            }
            position = align_up(start + size, 0x10);
        }

        let labels = labels.ok_or(Error::Invalid("MSBT without labels"))?;
        let texts = texts.ok_or(Error::Invalid("MSBT without texts"))?;
        let text_table = OffsetTable::parse(texts, 4)?;
        let attribute_table = match attributes {
            Some(body) => {
                let count = u32_at(body, 0)?;
                let size = u32_at(body, 4)?;
                match size {
                    0 => None,
                    4 if count > 0 => Some(OffsetTable::parse(body, 8)?),
                    4 => None,
                    _ => return Err(Error::Invalid("unsupported MSBT attribute size")),
                }
            }
            None => None,
        };

        let unit = if encoding == ENCODING_UTF16 { 2 } else { 1 };
        let mut msbt = Msbt::new(encoding);

        let group_count = u32_at(labels, 0)? as usize;
        for group in 0..group_count {
            let label_count = u32_at(labels, 4 + group * 8)? as usize;
            let mut offset = u32_at(labels, 8 + group * 8)? as usize;
            for _ in 0..label_count {
                let length = *labels.get(offset).ok_or(Error::Truncated { what: "MSBT label" })? as usize;
                let name = labels
                    .get(offset + 1..offset + 1 + length)
                    .ok_or(Error::Truncated { what: "MSBT label" })?;
                let text_index = u32_at(labels, offset + 1 + length)? as usize;
                offset += 1 + length + 4;

                let text = strip_nulls(text_table.get(texts, text_index)?, unit);
                let attribute = match &attribute_table {
                    Some(table) if text_index < table.count => {
                        let raw = table.get(attributes.unwrap(), text_index)?;
                        let value = terminated(raw, unit);
                        if value.is_empty() {
                            None
                        } else {
                            Some(value.to_vec())
                        }
                    }
                    _ => None,
                };

                let name = String::from_utf8_lossy(name).into_owned();
                let entry = MsbtEntry { attribute, text };
                match (msbt.index.get(&name), duplicates) {
                    (Some(_), Duplicates::KeepFirst) => {}
                    _ => msbt.insert(name, entry),
                }
            }
        }

        Ok(msbt)
    }

    pub fn encoding(&self) -> u8 {
        self.encoding
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, label: &str) -> Option<&MsbtEntry> {
        self.index.get(label).map(|&i| &self.entries[i].1)
    }

    /// Adds or replaces an entry; a new label goes last.
    pub fn insert(&mut self, label: String, entry: MsbtEntry) {
        match self.index.get(&label) {
            Some(&i) => self.entries[i].1 = entry,
            None => {
                self.index.insert(label.clone(), self.entries.len());
                self.entries.push((label, entry));
            }
        }
    }

    pub fn entries(&self) -> impl Iterator<Item = (&str, &MsbtEntry)> {
        self.entries.iter().map(|(label, entry)| (label.as_str(), entry))
    }

    pub fn write(&self) -> Vec<u8> {
        let unit = if self.encoding == ENCODING_UTF16 { 2 } else { 1 };
        let uses_attributes = self.entries.iter().any(|(_, entry)| entry.attribute.is_some());

        // Entries with an attribute first, sorted by it (MessageStudio's order).
        let mut order: Vec<usize> = (0..self.entries.len()).collect();
        if uses_attributes {
            order.sort_by(|&a, &b| {
                let (a, b) = (&self.entries[a].1.attribute, &self.entries[b].1.attribute);
                a.is_none().cmp(&b.is_none()).then_with(|| a.cmp(b))
            });
        }

        let mut out = vec![0u8; HEADER_SIZE];
        let mut sections = 0u16;

        write_section(&mut out, &mut sections, LBL1, |body| {
            body.extend_from_slice(&1u32.to_le_bytes());
            body.extend_from_slice(&(order.len() as u32).to_le_bytes());
            body.extend_from_slice(&12u32.to_le_bytes());
            for (text_index, &i) in order.iter().enumerate() {
                let label = self.entries[i].0.as_bytes();
                body.push(label.len() as u8);
                body.extend_from_slice(label);
                body.extend_from_slice(&(text_index as u32).to_le_bytes());
            }
        });

        if uses_attributes {
            write_section(&mut out, &mut sections, ATR1, |body| {
                body.extend_from_slice(&(order.len() as u32).to_le_bytes());
                body.extend_from_slice(&4u32.to_le_bytes());
                let mut offset = 8 + order.len() * 4;
                for &i in &order {
                    body.extend_from_slice(&(offset as u32).to_le_bytes());
                    offset += unit + self.entries[i].1.attribute.as_ref().map_or(0, |a| a.len());
                }
                for &i in &order {
                    if let Some(attribute) = &self.entries[i].1.attribute {
                        body.extend_from_slice(attribute);
                    }
                    body.extend(core::iter::repeat(0).take(unit));
                }
            });
        }

        write_section(&mut out, &mut sections, TXT2, |body| {
            body.extend_from_slice(&(order.len() as u32).to_le_bytes());
            let mut offset = 4 + order.len() * 4;
            for &i in &order {
                body.extend_from_slice(&(offset as u32).to_le_bytes());
                offset += self.entries[i].1.text.len() + unit;
            }
            for &i in &order {
                body.extend_from_slice(&self.entries[i].1.text);
                body.extend(core::iter::repeat(0).take(unit));
            }
        });

        let size = out.len() as u32;
        out[0..8].copy_from_slice(MAGIC);
        out[8..10].copy_from_slice(&[0xFF, 0xFE]);
        out[0x0C] = self.encoding;
        out[0x0D] = 3;
        out[0x0E..0x10].copy_from_slice(&sections.to_le_bytes());
        out[0x12..0x16].copy_from_slice(&size.to_le_bytes());
        out
    }
}

struct OffsetTable {
    count: usize,
    offsets: Vec<usize>,
}

impl OffsetTable {
    /// Offsets follow a u32 count, starting `first` bytes into the section.
    fn parse(body: &[u8], first: usize) -> Result<OffsetTable> {
        let count = u32_at(body, 0)? as usize;
        let mut offsets = Vec::with_capacity(count);
        for i in 0..count {
            offsets.push(u32_at(body, first + i * 4)? as usize);
        }
        Ok(OffsetTable { count, offsets })
    }

    fn get<'a>(&self, body: &'a [u8], index: usize) -> Result<&'a [u8]> {
        let start = *self.offsets.get(index).ok_or(Error::Invalid("MSBT label points past the texts"))?;
        let end = self.offsets.get(index + 1).copied().unwrap_or(body.len());
        body.get(start..end.max(start)).ok_or(Error::Truncated { what: "MSBT text" })
    }
}

/// A string up to its first terminator.
fn terminated(raw: &[u8], unit: usize) -> &[u8] {
    let mut end = 0;
    while end + unit <= raw.len() && raw[end..end + unit].iter().any(|&b| b != 0) {
        end += unit;
    }
    &raw[..end]
}

/// Drops NUL code units outside control tags, which is what reading a text
/// into a string and writing it back does in MessageStudio: the terminator
/// goes away and the text compares equal to its decoded form.
fn strip_nulls(raw: &[u8], unit: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len());
    let read = |i: usize| -> u16 {
        if unit == 2 {
            u16::from_le_bytes([raw[i], raw.get(i + 1).copied().unwrap_or(0)])
        } else {
            raw[i] as u16
        }
    };

    let mut i = 0;
    while i + unit <= raw.len() {
        let value = read(i);
        match value {
            0x0E if i + 4 * unit <= raw.len() => {
                // group, type, parameter size in bytes, parameters
                let size = if unit == 2 {
                    read(i + 3 * unit) as usize
                } else {
                    u16::from_le_bytes([raw[i + 5], raw.get(i + 6).copied().unwrap_or(0)]) as usize
                };
                let header = if unit == 2 { 4 * unit } else { 7 };
                let end = (i + header + size).min(raw.len());
                out.extend_from_slice(&raw[i..end]);
                i = end;
            }
            0x0F if i + 3 * unit <= raw.len() => {
                let end = if unit == 2 { i + 3 * unit } else { i + 5 };
                let end = end.min(raw.len());
                out.extend_from_slice(&raw[i..end]);
                i = end;
            }
            0 => i += unit,
            _ => {
                out.extend_from_slice(&raw[i..i + unit]);
                i += unit;
            }
        }
    }
    out
}

fn write_section(out: &mut Vec<u8>, count: &mut u16, magic: &[u8; 4], fill: impl FnOnce(&mut Vec<u8>)) {
    let mut body = Vec::new();
    fill(&mut body);
    out.extend_from_slice(magic);
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&[0; 8]);
    out.extend_from_slice(&body);
    let aligned = align_up(out.len(), 0x10);
    out.resize(aligned, 0);
    *count += 1;
}

fn align_up(value: usize, alignment: usize) -> usize {
    let remainder = value % alignment;
    if remainder == 0 {
        value
    } else {
        value + alignment - remainder
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(|unit| unit.to_le_bytes()).collect()
    }

    #[test]
    fn round_trips_entries() {
        let mut msbt = Msbt::new(ENCODING_UTF16);
        msbt.insert(
            "Hello".into(),
            MsbtEntry {
                attribute: None,
                text: utf16("Hello world"),
            },
        );
        // A control tag with a NUL in its parameters must survive.
        let mut tagged = utf16("a");
        tagged.extend_from_slice(&[0x0E, 0, 1, 0, 2, 0, 2, 0, 0, 0]);
        tagged.extend(utf16("b"));
        msbt.insert(
            "Tagged".into(),
            MsbtEntry {
                attribute: None,
                text: tagged.clone(),
            },
        );

        let read = Msbt::parse(&msbt.write()).unwrap();
        assert_eq!(read.len(), 2);
        assert_eq!(read.get("Hello").unwrap().text, utf16("Hello world"));
        assert_eq!(read.get("Tagged").unwrap().text, tagged);
        assert_eq!(read.write(), msbt.write());
    }

    #[test]
    fn attributes_are_written_only_when_used() {
        let mut msbt = Msbt::new(ENCODING_UTF16);
        msbt.insert(
            "B".into(),
            MsbtEntry {
                attribute: None,
                text: utf16("no attribute"),
            },
        );
        msbt.insert(
            "A".into(),
            MsbtEntry {
                attribute: Some(utf16("attr")),
                text: utf16("with attribute"),
            },
        );
        let bytes = msbt.write();
        assert_eq!(&bytes[0x20..0x24], LBL1);
        let read = Msbt::parse(&bytes).unwrap();
        assert_eq!(read.get("A").unwrap().attribute, Some(utf16("attr")));
        assert_eq!(read.get("B").unwrap().attribute, None);
        // Entries with attributes come first.
        assert_eq!(read.entries().next().unwrap().0, "A");
    }
}
