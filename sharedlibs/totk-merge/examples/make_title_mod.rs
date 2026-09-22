//! Writes a mod that appends a marker to title screen texts, in every
//! language. Several such mods touching different labels of the same message
//! file show at a glance, on the title screen, whether they were merged.
//!
//!     cargo run --release -p totk-merge --example make_title_mod -- <romfs> <mod folder> <label>=<suffix>...
//!
//! Labels of `LayoutMsg/Title_00.msbt`: 0000 New Game, 0001 Continue,
//! 0002 Options, 0003 amiibo. Only the edited message file is put in each
//! language's archive, which is all a changelog needs.

use std::path::PathBuf;

use totk_formats::msbt::{Msbt, MsbtEntry};
use totk_formats::sarc::{Sarc, SarcBuilder};
use totk_formats::zstd::compress_raw;
use totk_merge::rom::TkRom;
use totk_merge::tkcl::attributes;

const MESSAGE_FILE: &str = "LayoutMsg/Title_00.msbt";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!("usage: make_title_mod <romfs> <mod folder> <label>=<suffix>...");
        std::process::exit(2);
    }
    let rom = TkRom::open(&format!("{}/", args[0].trim_end_matches(['/', '\\']))).unwrap();
    let out = PathBuf::from(&args[1]);
    let edits: Vec<(String, String)> = args[2..]
        .iter()
        .map(|edit| {
            let (label, suffix) = edit.split_once('=').expect("<label>=<suffix>");
            (label.to_string(), suffix.to_string())
        })
        .collect();

    for locale in rom.locales() {
        let relative = rom.canonical_to_relative(
            &format!("Mals/{}.Product.sarc", locale),
            attributes::HAS_ZS_EXTENSION | attributes::IS_PRODUCT_FILE,
        );
        let archive = rom.get_vanilla(&relative).0.expect("message archive");
        let sarc = Sarc::parse(&archive).unwrap();
        let mut msbt = Msbt::parse(sarc.get(MESSAGE_FILE).expect("Title_00.msbt").data).unwrap();

        for (label, suffix) in &edits {
            let mut entry: MsbtEntry = msbt.get(label).unwrap_or_else(|| panic!("no label {}", label)).clone();
            entry.text.extend(suffix.encode_utf16().flat_map(|unit| unit.to_le_bytes()));
            msbt.insert(label.clone(), entry);
        }

        let mut builder = SarcBuilder::new();
        builder.insert(MESSAGE_FILE, msbt.write());
        let path = out.join("romfs").join(&relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, compress_raw(&builder.build())).unwrap();
    }
    println!("{}: {} language(s), {:?}", out.display(), rom.locales().len(), edits);
}
