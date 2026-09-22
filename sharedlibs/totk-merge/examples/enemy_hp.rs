//! Prints how much life enemies have, from a romfs on a PC.
//!
//!     cargo run --release -p totk-merge --example enemy_hp -- <romfs> [actor...]
//!
//! Without actors, the ones the `enemy-hp` example plugin reports on; with
//! `all`, every `Enemy_*` actor of the game that has any life at all.
//!
//! The lookup itself is the plugin's, included from its source, so what this
//! prints on a PC is what the plugin writes on the console.

use std::collections::HashMap;

use totk_merge::rom::TkRom;

#[path = "../../../plugins/enemy-hp/src/hp.rs"]
#[allow(dead_code)] // the plugin uses more of it than these examples do
mod hp;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: enemy_hp <romfs> [actor... | all]");
        std::process::exit(2);
    }
    let root = args[0].trim_end_matches(['/', '\\']).to_string();
    let rom = TkRom::open(&format!("{}/", root)).unwrap();

    let mut cache: HashMap<String, Option<Vec<u8>>> = HashMap::new();
    let mut files = |relative: &str| -> Option<Vec<u8>> {
        cache
            .entry(relative.to_string())
            .or_insert_with(|| rom.get_vanilla(relative).0)
            .clone()
    };

    let actors: Vec<String> = match args.get(1).map(String::as_str) {
        None => hp::DEFAULT_ENEMIES.iter().map(|a| a.to_string()).collect(),
        Some("all") => {
            let mut found: Vec<String> = std::fs::read_dir(format!("{}/Pack/Actor", root))
                .expect("no Pack/Actor folder")
                .filter_map(|entry| entry.ok())
                .filter_map(|entry| entry.file_name().into_string().ok())
                .filter_map(|name| name.strip_suffix(".pack.zs").map(str::to_string))
                .filter(|name| name.starts_with("Enemy_"))
                .collect();
            found.sort();
            found
        }
        Some(_) => args[1..].to_vec(),
    };

    let mut found = 0;
    for actor in &actors {
        match hp::max_life(&mut files, actor) {
            Some(life) => {
                println!("{}\t{}", actor, life);
                found += 1;
            }
            None => println!("{}\t-", actor),
        }
    }
    eprintln!("{} of {} actor(s) have a life parameter", found, actors.len());
}
