//! Minimal ZIP reader: central directory, stored and deflated entries.
//!
//! Enough for the content archive TKMM appends to `.tkcl` files (written by
//! .NET's ZipArchive). The archive does not have to start at offset 0: its base
//! is recovered from the end of central directory record, so a `.tkcl` can be
//! handed over whole. Reads go through [`RandomAccess`], so a large archive on
//! the SD card is never loaded into memory in one piece.

use hashbrown::HashMap;

use crate::prelude::*;
use crate::{Error, Result};

const LOCAL_HEADER: u32 = 0x0403_4B50;
const CENTRAL_HEADER: u32 = 0x0201_4B50;
const END_OF_CENTRAL_DIRECTORY: u32 = 0x0605_4B50;

/// Positional reads over some storage.
pub trait RandomAccess {
    fn len(&self) -> u64;
    fn read_at(&self, offset: u64, buffer: &mut [u8]) -> Result<()>;

    fn read_vec(&self, offset: u64, size: usize) -> Result<Vec<u8>> {
        let mut buffer = vec![0u8; size];
        self.read_at(offset, &mut buffer)?;
        Ok(buffer)
    }
}

impl RandomAccess for [u8] {
    fn len(&self) -> u64 {
        <[u8]>::len(self) as u64
    }

    fn read_at(&self, offset: u64, buffer: &mut [u8]) -> Result<()> {
        let start = offset as usize;
        let source = self
            .get(start..start + buffer.len())
            .ok_or(Error::Truncated { what: "zip data" })?;
        buffer.copy_from_slice(source);
        Ok(())
    }
}

impl RandomAccess for Vec<u8> {
    fn len(&self) -> u64 {
        self.as_slice().len() as u64
    }

    fn read_at(&self, offset: u64, buffer: &mut [u8]) -> Result<()> {
        self.as_slice().read_at(offset, buffer)
    }
}

/// A file read at any offset, safe to share between threads.
pub struct FileAccess {
    file: crate::sys::fs::File,
}

impl FileAccess {
    pub fn open(path: &str) -> crate::sys::fs::Result<FileAccess> {
        Ok(FileAccess {
            file: crate::sys::fs::File::open(path)?,
        })
    }
}

impl RandomAccess for FileAccess {
    fn len(&self) -> u64 {
        self.file.len()
    }

    fn read_at(&self, offset: u64, buffer: &mut [u8]) -> Result<()> {
        match self.file.read_at(offset, buffer) {
            Ok(read) if read == buffer.len() => Ok(()),
            _ => Err(Error::Truncated { what: "zip file" }),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ZipEntry {
    pub name: String,
    pub method: u16,
    pub compressed_size: u64,
    pub size: u64,
    /// Absolute offset of the local file header.
    local_header: u64,
}

#[derive(Debug, Default)]
pub struct ZipIndex {
    entries: Vec<ZipEntry>,
    by_name: HashMap<String, usize>,
    /// Where the archive starts; everything before it is someone else's data
    /// (the mod metadata, for a .tkcl).
    base: u64,
}

fn le16(bytes: &[u8], offset: usize) -> Result<u16> {
    crate::u16_at(bytes, offset, "zip structure")
}

fn le32(bytes: &[u8], offset: usize) -> Result<u32> {
    crate::u32_at(bytes, offset, "zip structure")
}

impl ZipIndex {
    /// Reads the central directory of the archive at the end of `source`.
    pub fn parse<S: RandomAccess + ?Sized>(source: &S) -> Result<ZipIndex> {
        let total = source.len();
        let tail_size = total.min(22 + 0xFFFF) as usize;
        let tail_start = total - tail_size as u64;
        let tail = source.read_vec(tail_start, tail_size)?;

        let eocd_in_tail = find_end_of_central_directory(&tail)?;
        let eocd = tail_start + eocd_in_tail as u64;
        let entry_count = le16(&tail, eocd_in_tail + 10)? as usize;
        let directory_size = le32(&tail, eocd_in_tail + 12)? as u64;
        let directory_offset = le32(&tail, eocd_in_tail + 16)? as u64;

        let base = eocd
            .checked_sub(directory_size + directory_offset)
            .ok_or(Error::Invalid("zip central directory lies outside the file"))?;

        let directory = source.read_vec(base + directory_offset, directory_size as usize)?;

        let mut index = ZipIndex {
            base,
            ..ZipIndex::default()
        };
        let mut cursor = 0usize;
        for _ in 0..entry_count {
            if le32(&directory, cursor)? != CENTRAL_HEADER {
                return Err(Error::Invalid("bad zip central directory entry"));
            }
            let method = le16(&directory, cursor + 10)?;
            let compressed_size = le32(&directory, cursor + 20)? as u64;
            let size = le32(&directory, cursor + 24)? as u64;
            let name_length = le16(&directory, cursor + 28)? as usize;
            let extra_length = le16(&directory, cursor + 30)? as usize;
            let comment_length = le16(&directory, cursor + 32)? as usize;
            let local_header = le32(&directory, cursor + 42)? as u64;

            let name_bytes = directory
                .get(cursor + 46..cursor + 46 + name_length)
                .ok_or(Error::Truncated { what: "zip entry name" })?;
            let name = String::from_utf8_lossy(name_bytes).replace('\\', "/");

            index.by_name.insert(name.to_ascii_lowercase(), index.entries.len());
            index.entries.push(ZipEntry {
                name,
                method,
                compressed_size,
                size,
                local_header: base + local_header,
            });

            cursor += 46 + name_length + extra_length + comment_length;
        }

        Ok(index)
    }

    pub fn entries(&self) -> &[ZipEntry] {
        &self.entries
    }

    /// Offset the archive starts at within its source.
    pub fn base(&self) -> u64 {
        self.base
    }

    /// Case-insensitive lookup, like the Windows file systems TKMM runs on.
    pub fn find(&self, name: &str) -> Option<&ZipEntry> {
        self.by_name
            .get(&name.replace('\\', "/").to_ascii_lowercase())
            .map(|&index| &self.entries[index])
    }

    /// Extracts an entry from the source that was indexed.
    pub fn read<S: RandomAccess + ?Sized>(&self, source: &S, entry: &ZipEntry) -> Result<Vec<u8>> {
        let header = source.read_vec(entry.local_header, 30)?;
        if le32(&header, 0)? != LOCAL_HEADER {
            return Err(Error::Invalid("bad zip local header"));
        }
        let name_length = le16(&header, 26)? as u64;
        let extra_length = le16(&header, 28)? as u64;
        let start = entry.local_header + 30 + name_length + extra_length;
        let compressed = source.read_vec(start, entry.compressed_size as usize)?;

        match entry.method {
            0 => Ok(compressed),
            8 => miniz_oxide::inflate::decompress_to_vec_with_limit(&compressed, entry.size as usize)
                .map_err(|_| Error::Invalid("corrupt deflate stream in zip entry")),
            _ => Err(Error::Invalid("unsupported zip compression method")),
        }
    }
}

fn find_end_of_central_directory(tail: &[u8]) -> Result<usize> {
    let mut position = tail.len().checked_sub(22).ok_or(Error::Truncated { what: "zip archive" })?;
    loop {
        if le32(tail, position)? == END_OF_CENTRAL_DIRECTORY {
            return Ok(position);
        }
        if position == 0 {
            return Err(Error::Invalid("no zip end of central directory record"));
        }
        position -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny archive holding "a.txt" (stored, "hello") and "dir/b.txt"
    /// (deflated, "hello hello hello"), with 4 bytes of junk in front of it the
    /// way a .tkcl prepends its metadata.
    fn sample() -> Vec<u8> {
        fn local(name: &str, method: u16, data: &[u8], size: u32) -> Vec<u8> {
            let mut out = Vec::new();
            out.extend_from_slice(&LOCAL_HEADER.to_le_bytes());
            out.extend_from_slice(&[20, 0, 0, 0]);
            out.extend_from_slice(&method.to_le_bytes());
            out.extend_from_slice(&[0; 8]); // time, date, crc (unchecked)
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&size.to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(data);
            out
        }
        fn central(name: &str, method: u16, compressed: u32, size: u32, offset: u32) -> Vec<u8> {
            let mut out = Vec::new();
            out.extend_from_slice(&CENTRAL_HEADER.to_le_bytes());
            out.extend_from_slice(&[20, 0, 20, 0, 0, 0]);
            out.extend_from_slice(&method.to_le_bytes());
            out.extend_from_slice(&[0; 8]);
            out.extend_from_slice(&compressed.to_le_bytes());
            out.extend_from_slice(&size.to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&[0; 12]); // extra, comment, disk, internal and external attrs
            out.extend_from_slice(&offset.to_le_bytes());
            out.extend_from_slice(name.as_bytes());
            out
        }

        let deflated = miniz_oxide::deflate::compress_to_vec(b"hello hello hello", 6);

        let mut zip = Vec::new();
        let first = local("a.txt", 0, b"hello", 5);
        let second_offset = first.len() as u32;
        let second = local("dir/b.txt", 8, &deflated, 17);
        zip.extend_from_slice(&first);
        zip.extend_from_slice(&second);

        let directory_offset = zip.len() as u32;
        let mut directory = central("a.txt", 0, 5, 5, 0);
        directory.extend(central("dir/b.txt", 8, deflated.len() as u32, 17, second_offset));
        zip.extend_from_slice(&directory);

        zip.extend_from_slice(&END_OF_CENTRAL_DIRECTORY.to_le_bytes());
        zip.extend_from_slice(&[0; 4]);
        zip.extend_from_slice(&2u16.to_le_bytes());
        zip.extend_from_slice(&2u16.to_le_bytes());
        zip.extend_from_slice(&(directory.len() as u32).to_le_bytes());
        zip.extend_from_slice(&directory_offset.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());

        let mut with_prefix = b"TKMP".to_vec();
        with_prefix.extend(zip);
        with_prefix
    }

    #[test]
    fn reads_stored_and_deflated_entries_after_a_prefix() {
        let data = sample();
        let index = ZipIndex::parse(data.as_slice()).unwrap();
        assert_eq!(index.entries().len(), 2);
        assert_eq!(index.base(), 4);
        assert_eq!(index.read(data.as_slice(), index.find("a.txt").unwrap()).unwrap(), b"hello");
        assert_eq!(
            index.read(data.as_slice(), index.find("DIR\\B.TXT").unwrap()).unwrap(),
            b"hello hello hello"
        );
    }
}
