//! Lists what is inside an archive of the game (SARC, compressed or not).
//!
//!     cargo run --release -p totk-merge --example sarc_list -- <romfs> <relative path> [name]
//!
//! With a name, the entry is written to the current folder instead; with
//! `--grep <text>`, the entries that contain that text are listed.

use totk_formats::sarc::Sarc;
use totk_merge::rom::TkRom;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("usage: sarc_list <romfs> <relative path> [entry to extract]");
        std::process::exit(2);
    }
    let rom = TkRom::open(&format!("{}/", args[0].trim_end_matches(['/', '\\']))).unwrap();
    // A romfs-relative path, or an archive anywhere (a mod's, a merged one).
    let data = match std::fs::read(&args[1]) {
        Ok(raw) => rom.decompress(&raw).unwrap_or(raw),
        Err(_) => rom.get_vanilla(&args[1]).0.expect("file not found in the romfs"),
    };
    let sarc = Sarc::parse(&data).expect("not a SARC");

    match args.get(2) {
        // Which entries contain a string (a font, a pane name...).
        Some(flag) if flag == "--grep" => {
            let needle = args.get(3).expect("text to look for").as_bytes();
            for entry in sarc.entries() {
                if entry.data.windows(needle.len()).any(|window| window == needle) {
                    println!("{}", entry.name);
                }
            }
        }
        None => {
            for entry in sarc.entries() {
                println!("{:>10}  {}", entry.data.len(), entry.name);
            }
        }
        Some(name) => {
            let entry = sarc.get(name).expect("no such entry");
            let out = name.rsplit('/').next().unwrap();
            std::fs::write(out, entry.data).unwrap();
            println!("wrote {} ({} bytes)", out, entry.data.len());
        }
    }
}
