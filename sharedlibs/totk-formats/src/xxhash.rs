//! xxHash32, which TKMM keys its pack file lookup with, and xxHash64.

use crate::prelude::*;

const PRIME1: u32 = 0x9E37_79B1;
const PRIME2: u32 = 0x85EB_CA77;
const PRIME3: u32 = 0xC2B2_AE3D;
const PRIME4: u32 = 0x27D4_EB2F;
const PRIME5: u32 = 0x1656_67B1;

fn round(accumulator: u32, lane: u32) -> u32 {
    accumulator
        .wrapping_add(lane.wrapping_mul(PRIME2))
        .rotate_left(13)
        .wrapping_mul(PRIME1)
}

fn read_u32(data: &[u8]) -> u32 {
    u32::from_le_bytes([data[0], data[1], data[2], data[3]])
}

pub fn xxh32(data: &[u8], seed: u32) -> u32 {
    let mut rest = data;
    let mut hash = if data.len() >= 16 {
        let mut v1 = seed.wrapping_add(PRIME1).wrapping_add(PRIME2);
        let mut v2 = seed.wrapping_add(PRIME2);
        let mut v3 = seed;
        let mut v4 = seed.wrapping_sub(PRIME1);
        while rest.len() >= 16 {
            v1 = round(v1, read_u32(&rest[0..]));
            v2 = round(v2, read_u32(&rest[4..]));
            v3 = round(v3, read_u32(&rest[8..]));
            v4 = round(v4, read_u32(&rest[12..]));
            rest = &rest[16..];
        }
        v1.rotate_left(1)
            .wrapping_add(v2.rotate_left(7))
            .wrapping_add(v3.rotate_left(12))
            .wrapping_add(v4.rotate_left(18))
    } else {
        seed.wrapping_add(PRIME5)
    };

    hash = hash.wrapping_add(data.len() as u32);

    while rest.len() >= 4 {
        hash = hash
            .wrapping_add(read_u32(rest).wrapping_mul(PRIME3))
            .rotate_left(17)
            .wrapping_mul(PRIME4);
        rest = &rest[4..];
    }
    for &byte in rest {
        hash = hash
            .wrapping_add((byte as u32).wrapping_mul(PRIME5))
            .rotate_left(11)
            .wrapping_mul(PRIME1);
    }

    hash ^= hash >> 15;
    hash = hash.wrapping_mul(PRIME2);
    hash ^= hash >> 13;
    hash = hash.wrapping_mul(PRIME3);
    hash ^= hash >> 16;
    hash
}

const PRIME64_1: u64 = 0x9E37_79B1_85EB_CA87;
const PRIME64_2: u64 = 0xC2B2_AE3D_27D4_EB4F;
const PRIME64_3: u64 = 0x1656_67B1_9E37_79F9;
const PRIME64_4: u64 = 0x85EB_CA77_C2B2_AE63;
const PRIME64_5: u64 = 0x27D4_EB2F_1656_67C5;

fn round64(accumulator: u64, lane: u64) -> u64 {
    accumulator
        .wrapping_add(lane.wrapping_mul(PRIME64_2))
        .rotate_left(31)
        .wrapping_mul(PRIME64_1)
}

fn merge_round64(hash: u64, value: u64) -> u64 {
    (hash ^ round64(0, value)).wrapping_mul(PRIME64_1).wrapping_add(PRIME64_4)
}

fn read_u64(data: &[u8]) -> u64 {
    u64::from_le_bytes([data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7]])
}

/// xxHash64, for naming files by their content.
pub fn xxh64(data: &[u8], seed: u64) -> u64 {
    let mut rest = data;
    let mut hash = if data.len() >= 32 {
        let mut v1 = seed.wrapping_add(PRIME64_1).wrapping_add(PRIME64_2);
        let mut v2 = seed.wrapping_add(PRIME64_2);
        let mut v3 = seed;
        let mut v4 = seed.wrapping_sub(PRIME64_1);
        while rest.len() >= 32 {
            v1 = round64(v1, read_u64(&rest[0..]));
            v2 = round64(v2, read_u64(&rest[8..]));
            v3 = round64(v3, read_u64(&rest[16..]));
            v4 = round64(v4, read_u64(&rest[24..]));
            rest = &rest[32..];
        }
        let mut hash = v1
            .rotate_left(1)
            .wrapping_add(v2.rotate_left(7))
            .wrapping_add(v3.rotate_left(12))
            .wrapping_add(v4.rotate_left(18));
        hash = merge_round64(hash, v1);
        hash = merge_round64(hash, v2);
        hash = merge_round64(hash, v3);
        merge_round64(hash, v4)
    } else {
        seed.wrapping_add(PRIME64_5)
    };

    hash = hash.wrapping_add(data.len() as u64);

    while rest.len() >= 8 {
        hash = (hash ^ round64(0, read_u64(rest)))
            .rotate_left(27)
            .wrapping_mul(PRIME64_1)
            .wrapping_add(PRIME64_4);
        rest = &rest[8..];
    }
    if rest.len() >= 4 {
        hash = (hash ^ (read_u32(rest) as u64).wrapping_mul(PRIME64_1))
            .rotate_left(23)
            .wrapping_mul(PRIME64_2)
            .wrapping_add(PRIME64_3);
        rest = &rest[4..];
    }
    for &byte in rest {
        hash = (hash ^ (byte as u64).wrapping_mul(PRIME64_5))
            .rotate_left(11)
            .wrapping_mul(PRIME64_1);
    }

    hash ^= hash >> 33;
    hash = hash.wrapping_mul(PRIME64_2);
    hash ^= hash >> 29;
    hash = hash.wrapping_mul(PRIME64_3);
    hash ^= hash >> 32;
    hash
}

/// Hash of a string's UTF-16 code units, the way .NET hashes a `char` span.
pub fn xxh32_utf16(text: &str) -> u32 {
    let bytes: Vec<u8> = text.encode_utf16().flat_map(|unit| unit.to_le_bytes()).collect();
    xxh32(&bytes, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_reference_vectors() {
        assert_eq!(xxh32(b"", 0), 0x02CC_5D05);
        assert_eq!(xxh32(b"a", 0), 0x550D_7456);
        assert_eq!(xxh32(b"abc", 0), 0x32D1_53FF);
        assert_eq!(xxh32(b"Nobody inspects the spammish repetition", 0), 0xE229_3B2F);

        assert_eq!(xxh64(b"", 0), 0xEF46_DB37_51D8_E999);
        assert_eq!(xxh64(b"a", 0), 0xD24E_C4F1_A98C_6E5B);
        assert_eq!(xxh64(b"abc", 0), 0x44BC_2CF5_AD77_0999);
        assert_eq!(xxh64(b"Nobody inspects the spammish repetition", 0), 0xFBCE_A83C_8A37_8BF1);
    }
}
