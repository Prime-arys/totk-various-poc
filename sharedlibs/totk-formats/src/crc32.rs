//! CRC32 (IEEE), used to key the resource size table.

const POLY: u32 = 0xEDB8_8320;

fn table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = i as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ POLY } else { crc >> 1 };
            bit += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
}

pub fn compute(data: &[u8]) -> u32 {
    let table = table();
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc = table[((crc ^ byte as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    !crc
}

pub fn compute_str(text: &str) -> u32 {
    compute(text.as_bytes())
}

#[cfg(test)]
mod tests {
    #[test]
    fn known_values() {
        assert_eq!(super::compute(b""), 0);
        assert_eq!(super::compute(b"123456789"), 0xCBF4_3926);
        assert_eq!(super::compute_str("The quick brown fox jumps over the lazy dog"), 0x414F_A339);
    }
}
