//! Prints a BYML document as indented text.
//!
//!     cargo run -p totk-formats --example byml_dump -- <romfs> <file> [inner file]
//!
//! `<file>` is relative to `<romfs>` and may be zstd compressed; when it is an
//! archive, `[inner file]` selects a document inside it (without it, the
//! archive's file list is printed).

use std::path::PathBuf;

use totk_formats::byml::Byml;
use totk_formats::sarc::Sarc;
use totk_formats::zstd::Zstd;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("usage: byml_dump <romfs> <file> [inner file]");
        std::process::exit(2);
    }

    let romfs = PathBuf::from(&args[0]);
    let mut zstd = Zstd::new();
    let dictionaries = zstd
        .decompress(&std::fs::read(romfs.join("Pack/ZsDic.pack.zs")).expect("Pack/ZsDic.pack.zs"))
        .unwrap();
    zstd.load_dictionaries(&dictionaries).unwrap();

    let raw = std::fs::read(romfs.join(&args[1])).unwrap_or_else(|e| panic!("{}: {}", args[1], e));
    let data = zstd.decompress(&raw).unwrap();

    let document = if data.starts_with(b"SARC") {
        let sarc = Sarc::parse(&data).unwrap();
        match args.get(2) {
            Some(inner) => sarc
                .get(inner)
                .unwrap_or_else(|| panic!("{} is not in {}", inner, args[1]))
                .data
                .to_vec(),
            None => {
                for entry in sarc.entries() {
                    println!("{:>8}  {}", entry.data.len(), entry.name);
                }
                return;
            }
        }
    } else {
        data
    };

    let (byml, version) = Byml::from_binary_with_version(&document).unwrap();
    println!("# BYML v{}, {} bytes", version, document.len());
    let limit: usize = std::env::var("DUMP_LIMIT").ok().and_then(|v| v.parse().ok()).unwrap_or(usize::MAX);
    let mut printed = 0;
    print(&byml, 0, &mut printed, limit);
}

fn print(node: &Byml, indent: usize, printed: &mut usize, limit: usize) {
    if *printed >= limit {
        return;
    }
    let pad = "  ".repeat(indent);
    match node {
        Byml::Map(map) => {
            for (key, value) in map {
                *printed += 1;
                if *printed >= limit {
                    println!("{}...", pad);
                    return;
                }
                if value.is_container() {
                    println!("{}{}:", pad, key);
                    print(value, indent + 1, printed, limit);
                } else {
                    println!("{}{}: {}", pad, key, scalar(value));
                }
            }
        }
        Byml::HashMap32(map) => {
            for (key, value) in map {
                *printed += 1;
                if value.is_container() {
                    println!("{}0x{:08X}:", pad, key);
                    print(value, indent + 1, printed, limit);
                } else {
                    println!("{}0x{:08X}: {}", pad, key, scalar(value));
                }
            }
        }
        Byml::HashMap64(map) => {
            for (key, value) in map {
                *printed += 1;
                if value.is_container() {
                    println!("{}0x{:016X}:", pad, key);
                    print(value, indent + 1, printed, limit);
                } else {
                    println!("{}0x{:016X}: {}", pad, key, scalar(value));
                }
            }
        }
        Byml::Array(items) => {
            for (index, value) in items.iter().enumerate() {
                *printed += 1;
                if *printed >= limit {
                    println!("{}... ({} items)", pad, items.len());
                    return;
                }
                if value.is_container() {
                    println!("{}- [{}]", pad, index);
                    print(value, indent + 1, printed, limit);
                } else {
                    println!("{}- {}", pad, scalar(value));
                }
            }
        }
        Byml::ArrayChangelog(changes) => {
            for change in changes {
                println!(
                    "{}- {:?} @{} key={:?}/{:?}",
                    pad,
                    change.change,
                    change.index,
                    change.key_primary.as_ref().map(scalar),
                    change.key_secondary.as_ref().map(scalar)
                );
                print(&change.node, indent + 1, printed, limit);
            }
        }
        other => println!("{}{}", pad, scalar(other)),
    }
}

fn scalar(node: &Byml) -> String {
    match node {
        Byml::Null => "null".into(),
        Byml::String(s) => format!("{:?}", s),
        Byml::Binary(b) => format!("<binary {} bytes>", b.len()),
        Byml::BinaryAligned(b, a) => format!("<binary {} bytes, align {}>", b.len(), a),
        Byml::Bool(v) => v.to_string(),
        Byml::Int(v) => format!("{}i", v),
        Byml::Float(v) => format!("{}f", v),
        Byml::UInt32(v) => format!("0x{:X}u", v),
        Byml::Int64(v) => format!("{}l", v),
        Byml::UInt64(v) => format!("0x{:X}ul", v),
        Byml::Double(v) => format!("{}d", v),
        Byml::Changelog(c) => format!("<{:?}>", c),
        container => format!("<{} node>", container.node_type()),
    }
}
