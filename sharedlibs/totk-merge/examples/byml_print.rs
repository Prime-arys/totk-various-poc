//! Prints a BYML of the game as text.
//!
//!     cargo run --release -p totk-merge --example byml_print -- <romfs> <relative path> [entry]
//!
//! With an entry, the BYML is read from inside that archive:
//!
//!     ... -- <romfs> Pack/Actor/Enemy_Bokoblin_Middle.pack.zs \
//!            Component/LifeParam/Enemy_Bokoblin_Middle.game__component__LifeParam.bgyml

use totk_formats::byml::Byml;
use totk_formats::sarc::Sarc;
use totk_merge::rom::TkRom;

fn print(node: &Byml, indent: usize) {
    let pad = " ".repeat(indent);
    match node {
        Byml::Map(map) => {
            for (key, value) in map.iter() {
                match value {
                    Byml::Map(_) | Byml::Array(_) => {
                        println!("{}{}:", pad, key.as_str());
                        print(value, indent + 2);
                    }
                    _ => println!("{}{}: {}", pad, key.as_str(), scalar(value)),
                }
            }
        }
        Byml::Array(items) => {
            for (index, value) in items.iter().enumerate() {
                match value {
                    Byml::Map(_) | Byml::Array(_) => {
                        println!("{}[{}]:", pad, index);
                        print(value, indent + 2);
                    }
                    _ => println!("{}[{}]: {}", pad, index, scalar(value)),
                }
            }
        }
        other => println!("{}{}", pad, scalar(other)),
    }
}

fn scalar(node: &Byml) -> String {
    match node {
        Byml::Null => "null".into(),
        Byml::String(text) => format!("\"{}\"", text),
        Byml::Bool(value) => value.to_string(),
        Byml::Int(value) => value.to_string(),
        Byml::UInt32(value) => value.to_string(),
        Byml::Int64(value) => value.to_string(),
        Byml::UInt64(value) => value.to_string(),
        Byml::Float(value) => value.to_string(),
        Byml::Double(value) => value.to_string(),
        Byml::Binary(data) => format!("<{} bytes>", data.len()),
        other => format!("<{:?}>", other.node_type()),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("usage: byml_print <romfs> <relative path> [entry inside the archive]");
        std::process::exit(2);
    }
    let rom = TkRom::open(&format!("{}/", args[0].trim_end_matches(['/', '\\']))).unwrap();
    // A romfs-relative path, or a file anywhere (a mod's, or a merged one).
    let data = match std::fs::read(&args[1]) {
        Ok(raw) => rom.decompress(&raw).unwrap_or(raw),
        Err(_) => rom.get_vanilla(&args[1]).0.expect("file not found in the romfs"),
    };

    let inner;
    let bytes = match args.get(2) {
        None => &data[..],
        Some(name) => {
            let sarc = Sarc::parse(&data).expect("not an archive");
            inner = sarc.get(name).expect("no such entry").data.to_vec();
            &inner[..]
        }
    };
    let (node, _) = Byml::parse(bytes).expect("not a BYML");
    print(&node, 0);
}
