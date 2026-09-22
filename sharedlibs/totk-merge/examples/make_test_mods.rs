//! Generates two mods that change different files inside the same pack.
//!
//! Handy for checking the merger on a console (or an emulator): a loader that
//! just picks one file over the other loses one of the two changes, a merger
//! keeps both.
//!
//!     cargo run -p totk-merge --example make_test_mods -- <romfs> <out dir> [pack]
//!
//! `<romfs>` is an extracted game romfs, `<out dir>` is where the mod folders
//! are written (e.g. the `totk/mods` folder on the SD card).

use std::path::{Path, PathBuf};

use totk_formats::sarc::{Sarc, SarcBuilder};
use totk_formats::zstd::{compress_raw, Zstd};

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(romfs), Some(out)) = (args.next(), args.next()) else {
        eprintln!("usage: make_test_mods <romfs> <out dir> [pack relative to romfs]");
        std::process::exit(2);
    };
    let pack = args
        .next()
        .unwrap_or_else(|| "Pack/Actor/Npc_Gerudo_Queen_Sage.pack.zs".to_string());

    let romfs = PathBuf::from(romfs);
    let out = PathBuf::from(out);

    let mut zstd = Zstd::new();
    let dictionaries = zstd
        .decompress(&std::fs::read(romfs.join("Pack/ZsDic.pack.zs")).expect("Pack/ZsDic.pack.zs"))
        .expect("ZsDic is stored without a dictionary");
    let count = zstd.load_dictionaries(&dictionaries).expect("dictionaries");
    println!("loaded {} zstd dictionaries", count);

    let vanilla = zstd
        .decompress(&std::fs::read(romfs.join(&pack)).unwrap_or_else(|e| panic!("{}: {}", pack, e)))
        .expect("decompress pack");
    let sarc = Sarc::parse(&vanilla).expect("parse pack");

    let names: Vec<String> = sarc.entries().map(|entry| entry.name.to_string()).collect();
    if names.len() < 2 {
        eprintln!("{} only holds {} file(s), pick another pack", pack, names.len());
        std::process::exit(1);
    }
    println!("{} holds {} files", pack, names.len());

    // New files rather than edits of real ones: the game never loads them, so
    // a merge that works changes nothing about how the game behaves, and a
    // merge that drops one is still easy to spot in the output.
    write_mod(&out, "test-mod-a", 10, &pack, &sarc, "TestModA.txt", b"TEST-MOD-A");
    write_mod(&out, "test-mod-b", 20, &pack, &sarc, "TestModB.txt", b"TEST-MOD-B");

    println!();
    println!("Merged correctly, {} ends up with both TestModA.txt and", pack);
    println!("TestModB.txt added, and its {} original files untouched.", names.len());
}

fn write_mod(out: &Path, folder: &str, priority: i32, pack: &str, sarc: &Sarc<'_>, target: &str, payload: &[u8]) {
    let mut builder = SarcBuilder::from_sarc(sarc);
    builder.insert(target, payload.to_vec());

    let file = out.join(folder).join("romfs").join(pack);
    std::fs::create_dir_all(file.parent().unwrap()).expect("create mod folder");
    std::fs::write(&file, compress_raw(&builder.build())).expect("write pack");

    std::fs::write(
        out.join(folder).join("mod.ini"),
        format!("name = {}\npriority = {}\nenabled = 1\n", folder, priority),
    )
    .expect("write mod.ini");

    println!("wrote {} (changes {})", file.display(), target);
}
