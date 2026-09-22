//! Builds the romfs half of the `EnemyHp` mod: every piece of armour gets the
//! game's own `VisualizeLife` effect, which is what showed an enemy's health as
//! a number over its gauge in Breath of the Wild (the Champion's Tunic).
//!
//!     cargo run --release -p totk-merge --example make_visualize_life_mod -- \
//!         <romfs> <out dir> [tunic | <actor,actor...>] [effect]
//!
//! `VisualizeLife` is in Tears of the Kingdom's list of armour effects but no
//! armour uses it, so it takes a mod to turn it back on:
//!
//! - `Component/ArmorParam/Default.game__component__ArmorParam.bgyml`
//!   (in `Pack/ResidentCommon.pack.zs`) covers every piece that has no effect
//!   of its own, since those inherit from it;
//! - each armour that *does* list effects (resistances, masks...) gets
//!   `VisualizeLife` added to its list, so no outfit loses the display.
//!
//! With `tunic`, only the Champion's Tunic (`Armor_1106..1110_Upper`) gets it,
//! as in Breath of the Wild. With a list of actors, only those; with an effect
//! name, that effect instead (a known-good one such as `ResistHot` tells a bad
//! edit from an effect the game does not implement).

use std::path::{Path, PathBuf};

use totk_formats::byml::{Byml, Map};
use totk_formats::sarc::{Sarc, SarcBuilder};
use totk_formats::zstd::compress_raw;
use totk_merge::rom::TkRom;

const DEFAULT_EFFECT: &str = "VisualizeLife";
const DEFAULT_PACK: &str = "Pack/ResidentCommon.pack.zs";
const DEFAULT_ENTRY: &str = "Component/ArmorParam/Default.game__component__ArmorParam.bgyml";
/// The five ranks of the Champion's Tunic.
const TUNIC: [&str; 5] = [
    "Armor_1106_Upper",
    "Armor_1107_Upper",
    "Armor_1108_Upper",
    "Armor_1109_Upper",
    "Armor_1110_Upper",
];

/// The list of effects an `ArmorParam` ends up with, following `$parent` when
/// it sets none of its own (an upgraded armour inherits from the rank below).
fn inherited(sarc: &Sarc<'_>, entry: &str) -> Vec<Byml> {
    let mut current = entry.to_string();
    for _ in 0..8 {
        let Some(file) = sarc.get(&current) else { break };
        let Ok((node, _)) = Byml::parse(file.data) else { break };
        let Some(map) = node.as_map() else { break };
        if let Some(list) = map.get("ArmorEffect").and_then(Byml::as_array) {
            return list.clone();
        }
        let Some(parent) = map.get("$parent").and_then(Byml::as_str) else { break };
        let path = parent.strip_prefix("Work/").unwrap_or(parent);
        current = match path.strip_suffix(".gyml") {
            Some(stem) => format!("{}.bgyml", stem),
            None => path.to_string(),
        };
    }
    Vec::new()
}

/// Adds the effect to an `ArmorParam` document. False when it is already there.
fn add_effect(parameters: &mut Byml, effect: &str, inherited: &[Byml]) -> bool {
    let Some(map) = parameters.as_map_mut() else {
        return false;
    };
    let mut added = Map::new();
    // The set-bonus table spells the level `ArmorEffectLevel`; effects that
    // have no level (EnableUseSwordBeam...) leave it out.
    if let Some((name, level)) = effect.split_once('@') {
        added.insert("ArmorEffectLevel".into(), Byml::Int(level.parse().unwrap_or(1)));
        added.insert("ArmorEffectType".into(), Byml::String(name.into()));
    } else {
        added.insert("ArmorEffectType".into(), Byml::String(effect.into()));
    }

    match map.get_mut("ArmorEffect").and_then(Byml::as_array_mut) {
        Some(list) => {
            let already = list.iter().any(|entry| {
                entry.as_map().and_then(|e| e.get("ArmorEffectType")).and_then(Byml::as_str)
                    == Some(effect.split('@').next().unwrap_or(effect))
            });
            if already {
                return false;
            }
            list.push(Byml::Map(added));
        }
        None => {
            // A list of its own replaces its parent's rather than adding to
            // it, so what it inherited has to be carried over (the upgraded
            // Champion's Tunic would otherwise lose its sword beam).
            let mut list = inherited.to_vec();
            list.push(Byml::Map(added));
            map.insert("ArmorEffect".into(), Byml::Array(list));
        }
    }
    true
}

/// Rewrites the given files inside a pack and writes the pack into the mod.
fn patch_pack(rom: &TkRom, out: &Path, pack: &str, entries: &[String], effect: &str) -> bool {
    let Some(data) = rom.get_vanilla(pack).0 else {
        return false;
    };
    let Ok(sarc) = Sarc::parse(&data) else { return false };

    let mut builder = SarcBuilder::from_sarc(&sarc);
    let mut changed = false;
    for entry in entries {
        let Some(file) = sarc.get(entry) else { continue };
        let Ok((mut parameters, format)) = Byml::parse(file.data) else {
            continue;
        };
        let inherited = inherited(&sarc, entry);
        if add_effect(&mut parameters, effect, &inherited) {
            builder.insert(entry, parameters.write(format));
            changed = true;
        }
    }
    if !changed {
        return false;
    }

    let file = out.join("romfs").join(pack);
    std::fs::create_dir_all(file.parent().expect("parent")).expect("create the mod folder");
    std::fs::write(&file, compress_raw(&builder.build())).expect("write the pack");
    true
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("usage: make_visualize_life_mod <romfs> <out dir> [tunic | <actor,actor...>] [effect]");
        std::process::exit(2);
    }
    let root = args[0].trim_end_matches(['/', '\\']).to_string();
    let out = PathBuf::from(&args[1]);
    let selection = args.get(2).cloned().unwrap_or_default();
    let tunic_only = selection == "tunic";
    let chosen: Vec<String> = match selection.is_empty() || tunic_only {
        true => Vec::new(),
        false => selection.split(',').map(str::to_string).collect(),
    };
    let effect = args.get(3).cloned().unwrap_or_else(|| DEFAULT_EFFECT.to_string());
    let everything = !tunic_only && chosen.is_empty();
    let rom = TkRom::open(&format!("{}/", root)).expect("romfs");

    let mut patched = 0;
    if everything {
        // Everything that has no effect of its own inherits from this one.
        if patch_pack(&rom, &out, DEFAULT_PACK, &[DEFAULT_ENTRY.to_string()], &effect) {
            println!("{}: {} added to every armour without an effect", DEFAULT_PACK, effect);
            patched += 1;
        }
    }

    let mut packs: Vec<String> = std::fs::read_dir(format!("{}/Pack/Actor", root))
        .expect("Pack/Actor")
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.starts_with("Armor_") && name.ends_with(".pack.zs"))
        .collect();
    packs.sort();

    for pack in &packs {
        let actor = pack.trim_end_matches(".pack.zs").to_string();
        let wanted = match (tunic_only, chosen.is_empty()) {
            (true, _) => TUNIC.contains(&actor.as_str()),
            (false, false) => chosen.contains(&actor),
            (false, true) => true,
        };
        if !wanted {
            continue;
        }
        let relative = format!("Pack/Actor/{}", pack);
        let Some(data) = rom.get_vanilla(&relative).0 else { continue };
        let Ok(sarc) = Sarc::parse(&data) else { continue };

        // Armour that lists effects of its own: the game takes that list as it
        // is, so the display has to be added to it. The others inherit it from
        // the default above.
        let entries: Vec<String> = sarc
            .entries()
            .filter(|entry| entry.name.contains("/ArmorParam/"))
            // Armour with a list of its own (even an empty one, which hides
            // the default's) or one inherited from a lower rank has to be
            // patched; the rest inherits the effect from the default above.
            .filter(|entry| {
                !everything
                    || !inherited(&sarc, entry.name).is_empty()
                    || Byml::parse(entry.data)
                        .ok()
                        .and_then(|(node, _)| node.as_map().map(|map| map.contains_key("ArmorEffect")))
                        .unwrap_or(false)
            })
            .map(|entry| entry.name.to_string())
            .collect();
        if entries.is_empty() {
            continue;
        }
        if patch_pack(&rom, &out, &relative, &entries, &effect) {
            patched += 1;
        }
    }

    std::fs::write(out.join("mod.ini"), "name = Enemy HP\nversion = 1.0\npriority = 10\n").ok();

    let what = match (tunic_only, chosen.is_empty()) {
        (true, _) => "the Champion's Tunic".to_string(),
        (false, false) => selection.clone(),
        (false, true) => "every armour".to_string(),
    };
    println!("{} file(s) written to {} ({}, {})", patched, out.display(), what, effect);
}
