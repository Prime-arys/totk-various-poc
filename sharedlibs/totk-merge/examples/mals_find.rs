//! Finds game texts: prints the message file and label of every text of a
//! locale that contains a string.
//!
//!     cargo run --release -p totk-merge --example mals_find -- <romfs> <locale> <text>

use totk_formats::msbt::Msbt;
use totk_formats::sarc::Sarc;
use totk_merge::rom::TkRom;
use totk_merge::tkcl::attributes;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!("usage: mals_find <romfs> <locale> <text>");
        std::process::exit(2);
    }
    let rom = TkRom::open(&format!("{}/", args[0].trim_end_matches(['/', '\\']))).unwrap();
    let relative = rom.canonical_to_relative(
        &format!("Mals/{}.Product.sarc", args[1]),
        attributes::HAS_ZS_EXTENSION | attributes::IS_PRODUCT_FILE,
    );
    let data = rom.get_vanilla(&relative).0.expect("message archive");
    let sarc = Sarc::parse(&data).unwrap();
    let needle = args[2].to_lowercase();

    for entry in sarc.entries().filter(|e| e.name.ends_with(".msbt")) {
        let msbt = Msbt::parse(entry.data).unwrap();
        for (label, value) in msbt.entries() {
            let units: Vec<u16> = value.text.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            let text = String::from_utf16_lossy(&units);
            if text.to_lowercase().contains(&needle) && text.chars().count() < 80 {
                println!("{} | {} | {:?}", entry.name, label, text);
            }
        }
    }
}
