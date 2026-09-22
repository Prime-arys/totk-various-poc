//! Prints every text of one message file.
//!
//!     cargo run --release -p totk-merge --example msbt_dump -- <romfs> <locale> <msbt>
//!     ... -- <romfs> EUfr ActorMsg/SheikahCameraTarget.msbt

use totk_formats::msbt::Msbt;
use totk_formats::sarc::Sarc;
use totk_merge::rom::TkRom;
use totk_merge::tkcl::attributes;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!("usage: msbt_dump <romfs> <locale> <message file>");
        std::process::exit(2);
    }
    let rom = TkRom::open(&format!("{}/", args[0].trim_end_matches(['/', '\\']))).unwrap();
    let relative = rom.canonical_to_relative(
        &format!("Mals/{}.Product.sarc", args[1]),
        attributes::HAS_ZS_EXTENSION | attributes::IS_PRODUCT_FILE,
    );
    let data = rom.get_vanilla(&relative).0.expect("message archive");
    let sarc = Sarc::parse(&data).unwrap();
    let entry = sarc.get(&args[2]).expect("no such message file");
    let msbt = Msbt::parse(entry.data).unwrap();

    for (label, value) in msbt.entries() {
        let units: Vec<u16> = value.text.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        println!("{}\t{}", label, String::from_utf16_lossy(&units).replace('\n', "\\n"));
    }
}
