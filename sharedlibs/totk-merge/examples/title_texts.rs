//! Prints the title screen texts of a merged message archive.
//!
//!     cargo run --release -p totk-merge --example title_texts -- <Mals/xxXX.Product.121.sarc.zs>

fn main() {
    let path = std::env::args().nth(1).expect("path to a message archive");
    let mut zstd = totk_formats::zstd::Zstd::new();
    let data = zstd.decompress(&std::fs::read(&path).unwrap()).unwrap();
    let sarc = totk_formats::sarc::Sarc::parse(&data).unwrap();
    let msbt = totk_formats::msbt::Msbt::parse(sarc.get("LayoutMsg/Title_00.msbt").unwrap().data).unwrap();
    for (label, entry) in msbt.entries() {
        let units: Vec<u16> = entry.text.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        println!("{} {}", label, String::from_utf16_lossy(&units));
    }
}
