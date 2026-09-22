//! Generates a mod that changes how much life an enemy has.
//!
//!     cargo run --release -p totk-merge --example make_enemy_hp_mod -- \
//!         <romfs> <out dir> [actor] [life]
//!
//! Defaults to a blue Bokoblin (`Enemy_Bokoblin_Middle`) with 1 HP, which is
//! plain enough in game. Written for the `enemy-hp` example plugin: with both
//! installed, the plugin's report shows the modded number, which is the proof
//! that a mod's plugin reads the *merged* game and not the cartridge.
//!
//! `<out dir>` is a mods folder, e.g. the `totk/mods` of an SD card.

use std::path::PathBuf;

use totk_formats::byml::Byml;
use totk_formats::sarc::{Sarc, SarcBuilder};
use totk_formats::zstd::compress_raw;
use totk_merge::rom::TkRom;

#[path = "../../../plugins/enemy-hp/src/hp.rs"]
#[allow(dead_code)] // the plugin uses more of it than these examples do
mod hp;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("usage: make_enemy_hp_mod <romfs> <out dir> [actor] [life]");
        std::process::exit(2);
    }
    let root = args[0].trim_end_matches(['/', '\\']).to_string();
    let out = PathBuf::from(&args[1]);
    let actor = args.get(2).cloned().unwrap_or_else(|| "Enemy_Bokoblin_Middle".to_string());
    let life: i32 = args.get(3).map_or(1, |value| value.parse().expect("life has to be a number"));

    let rom = TkRom::open(&format!("{}/", root)).expect("romfs");
    let mut files = |relative: &str| -> Option<Vec<u8>> { rom.get_vanilla(relative).0 };
    let found = hp::life(&mut files, &actor).unwrap_or_else(|| panic!("{} has no life parameter", actor));
    println!("{}: {} HP, set in {} of {}", actor, found.max, found.entry, found.pack);

    // The pack as the game ships it, with that one parameter changed.
    let data = rom.get_vanilla(&found.pack).0.expect("pack");
    let sarc = Sarc::parse(&data).expect("pack is an archive");
    let (mut parameters, format) = Byml::parse(sarc.get(&found.entry).expect("entry").data).expect("parameters");
    parameters
        .as_map_mut()
        .expect("parameters are a map")
        .insert("MaxLife".into(), Byml::Int(life));

    let mut builder = SarcBuilder::from_sarc(&sarc);
    builder.insert(&found.entry, parameters.write(format));

    let folder = out.join(format!("{} {} HP", actor, life));
    let file = folder.join("romfs").join(&found.pack);
    std::fs::create_dir_all(file.parent().expect("parent")).expect("create the mod folder");
    std::fs::write(&file, compress_raw(&builder.build())).expect("write the pack");
    std::fs::write(
        folder.join("mod.ini"),
        format!(
            "name = {} ({} HP)\nversion = 1.0\ndescription = Test mod: {} has {} HP instead of {}.\npriority = 60\n",
            actor, life, actor, life, found.max
        ),
    )
    .expect("write mod.ini");

    println!("wrote {}", file.display());
    println!("Install it next to the enemy-hp mod: its report should then read {}.", life);
}
