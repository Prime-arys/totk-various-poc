//! Lists every armour effect the game's own files use, and who has it.
//!
//!     cargo run --release -p totk-merge --example armor_effects -- <romfs>

use std::collections::BTreeMap;

use totk_formats::byml::Byml;
use totk_formats::sarc::Sarc;
use totk_merge::rom::TkRom;

fn main() {
    let root = std::env::args().nth(1).expect("path to a romfs");
    let root = root.trim_end_matches(['/', '\\']).to_string();
    let rom = TkRom::open(&format!("{}/", root)).unwrap();

    let mut packs: Vec<String> = std::fs::read_dir(format!("{}/Pack/Actor", root))
        .expect("Pack/Actor")
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.starts_with("Armor_") && name.ends_with(".pack.zs"))
        .collect();
    packs.sort();

    let mut effects: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for pack in &packs {
        let Some(data) = rom.get_vanilla(&format!("Pack/Actor/{}", pack)).0 else {
            continue;
        };
        let Ok(sarc) = Sarc::parse(&data) else { continue };
        for entry in sarc.entries().filter(|e| e.name.contains("/ArmorParam/")) {
            let Ok((node, _)) = Byml::parse(entry.data) else { continue };
            let Some(list) = node.as_map().and_then(|map| map.get("ArmorEffect")).and_then(Byml::as_array) else {
                continue;
            };
            for effect in list {
                let Some(map) = effect.as_map() else { continue };
                let name = map.get("ArmorEffectType").and_then(Byml::as_str).unwrap_or("?");
                let level = map.get("EffectLevel").map(|l| format!("{:?}", l)).unwrap_or_default();
                effects
                    .entry(name.to_string())
                    .or_default()
                    .push(format!("{} {}", pack.trim_end_matches(".pack.zs"), level));
            }
        }
    }

    for (effect, owners) in &effects {
        println!("{:30} {:3} armour(s), e.g. {}", effect, owners.len(), owners[0]);
    }
    println!("\n{} armour pack(s) scanned", packs.len());
}
