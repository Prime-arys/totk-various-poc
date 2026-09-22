//! zstd support for TotK.
//!
//! Most romfs files are stored as `<name>.zs`, compressed with one of the
//! dictionaries packed inside `Pack/ZsDic.pack.zs`. Every frame the game ships
//! names the dictionary it needs in its header, so decompression only requires
//! registering all of them once.
//!
//! Compression goes the other way round: rather than carrying a zstd encoder,
//! merged files are written as *raw* zstd frames (stored blocks, no
//! dictionary). Those are valid zstd any decoder accepts, cost no CPU on the
//! console, and only take up more room on the SD card.

use ruzstd::StreamingDecoder;
use ruzstd::decoding::dictionary::Dictionary;
use ruzstd::frame_decoder::FrameDecoder;
use ruzstd::io::Read;

use crate::prelude::*;
use crate::sarc::Sarc;
use crate::{Error, Result};

const ZSTD_MAGIC: [u8; 4] = [0x28, 0xB5, 0x2F, 0xFD];
/// Raw blocks are capped at 128 KiB by the format.
const MAX_RAW_BLOCK: usize = 128 * 1024;

pub struct Zstd {
    decoder: FrameDecoder,
}

impl Default for Zstd {
    fn default() -> Self {
        Self::new()
    }
}

impl Zstd {
    pub fn new() -> Self {
        Zstd {
            decoder: FrameDecoder::new(),
        }
    }

    /// Registers every dictionary found in a decompressed `ZsDic.pack` SARC.
    /// Returns how many were loaded.
    pub fn load_dictionaries(&mut self, zsdic_pack: &[u8]) -> Result<usize> {
        let sarc = Sarc::parse(zsdic_pack)?;
        let mut count = 0;
        for entry in sarc.entries() {
            if !entry.name.ends_with(".zsdic") {
                continue;
            }
            let dict = Dictionary::decode_dict(entry.data)
                .map_err(|e| Error::Zstd(format!("bad dictionary {}: {:?}", entry.name, e)))?;
            self.decoder
                .add_dict(dict)
                .map_err(|e| Error::Zstd(format!("could not add dictionary {}: {:?}", entry.name, e)))?;
            count += 1;
        }
        Ok(count)
    }

    pub fn is_compressed(data: &[u8]) -> bool {
        data.len() >= 4 && data[..4] == ZSTD_MAGIC
    }

    /// Decompresses a zstd frame. Data that is not zstd is returned untouched,
    /// which keeps callers simple: some ".zs" files in mods are not compressed.
    pub fn decompress(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        if !Self::is_compressed(data) {
            return Ok(data.to_vec());
        }

        let mut reader = StreamingDecoder::new_with_decoder(data, &mut self.decoder)
            .map_err(|e| Error::Zstd(format!("{:?}", e)))?;

        let hint = reader.decoder.content_size() as usize;
        let mut out = Vec::with_capacity(if hint > 0 { hint } else { data.len() * 4 });
        let mut chunk = vec![0u8; 128 * 1024];
        loop {
            let read = reader
                .read(&mut chunk)
                .map_err(|e| Error::Zstd(format!("{:?}", e)))?;
            if read == 0 {
                break;
            }
            out.extend_from_slice(&chunk[..read]);
        }
        Ok(out)
    }
}

/// Decompressed size a zstd frame declares in its header, when it does.
pub fn frame_content_size(data: &[u8]) -> Option<u64> {
    if !Zstd::is_compressed(data) {
        return None;
    }
    let descriptor = *data.get(4)?;
    let single_segment = descriptor & 0b0010_0000 != 0;
    let dictionary_size = [0usize, 1, 2, 4][(descriptor & 0b11) as usize];
    let mut offset = 5 + if single_segment { 0 } else { 1 } + dictionary_size;
    let size_flag = descriptor >> 6;
    let field = match size_flag {
        0 if single_segment => 1,
        0 => return None,
        1 => 2,
        2 => 4,
        _ => 8,
    };
    let bytes = data.get(offset..offset + field)?;
    offset += field;
    let _ = offset;
    let mut value = 0u64;
    for (i, byte) in bytes.iter().enumerate() {
        value |= (*byte as u64) << (8 * i);
    }
    Some(if field == 2 { value + 256 } else { value })
}

/// Wraps `data` in a zstd frame made of stored (uncompressed) blocks.
pub fn compress_raw(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 32 + (data.len() / MAX_RAW_BLOCK + 1) * 3);
    out.extend_from_slice(&ZSTD_MAGIC);

    // Frame header descriptor: single segment (the window is the whole content,
    // so no window descriptor byte follows), no checksum, no dictionary, and the
    // content size in the smallest field that holds it — like the reference
    // encoder. TKMM, for one, refuses the 8 byte form.
    let size = data.len() as u64;
    if size < 256 {
        out.push(0b0010_0000);
        out.push(size as u8);
    } else if size < 256 + 65536 {
        out.push(0b0110_0000);
        out.extend_from_slice(&((size - 256) as u16).to_le_bytes());
    } else if size <= u32::MAX as u64 {
        out.push(0b1010_0000);
        out.extend_from_slice(&(size as u32).to_le_bytes());
    } else {
        out.push(0b1110_0000);
        out.extend_from_slice(&size.to_le_bytes());
    }

    if data.is_empty() {
        // A frame still needs one (empty, last) block.
        out.extend_from_slice(&[0x01, 0x00, 0x00]);
        return out;
    }

    let mut offset = 0;
    while offset < data.len() {
        let size = core::cmp::min(MAX_RAW_BLOCK, data.len() - offset);
        let last = offset + size == data.len();
        // block header: size, then block type 0 (raw) and the last-block flag
        let header = ((size as u32) << 3) | last as u32;
        out.extend_from_slice(&header.to_le_bytes()[..3]);
        out.extend_from_slice(&data[offset..offset + size]);
        offset += size;
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_frame_roundtrips() {
        let payload: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        let frame = compress_raw(&payload);
        assert!(Zstd::is_compressed(&frame));

        let mut zstd = Zstd::new();
        assert_eq!(zstd.decompress(&frame).unwrap(), payload);
    }

    #[test]
    fn every_content_size_field_width_roundtrips() {
        let mut zstd = Zstd::new();
        for size in [1usize, 255, 256, 1000, 65791, 65792, 200_000] {
            let payload: Vec<u8> = (0..size).map(|i| (i % 7) as u8).collect();
            assert_eq!(zstd.decompress(&compress_raw(&payload)).unwrap(), payload, "size {}", size);
        }
    }

    #[test]
    fn reads_declared_content_sizes() {
        for size in [0usize, 10, 300, 70_000, 200_000] {
            let frame = compress_raw(&vec![7u8; size]);
            assert_eq!(frame_content_size(&frame), Some(size as u64), "{}", size);
        }
        assert_eq!(frame_content_size(b"not zstd"), None);
    }

    #[test]
    fn empty_raw_frame_roundtrips() {
        let frame = compress_raw(&[]);
        let mut zstd = Zstd::new();
        assert!(zstd.decompress(&frame).unwrap().is_empty());
    }

    #[test]
    fn passes_through_uncompressed_data() {
        let mut zstd = Zstd::new();
        assert_eq!(zstd.decompress(b"SARC not compressed").unwrap(), b"SARC not compressed");
    }
}
