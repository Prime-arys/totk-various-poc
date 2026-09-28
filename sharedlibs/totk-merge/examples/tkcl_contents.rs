//! What a TKMM package holds, for the mod itself and for each of its options:
//! the game files it changes, its code patches (which build they target, and
//! what they write), its cheats, and any exefs code it ships — `subsdk*` and
//! `main.npdm`, which cannot run next to skyline-totk.
//!
//!   cargo run --release -p totk-merge --example tkcl_contents -- <file.tkcl>

use totk_merge::tkcl::{read_tkcl_file, Changelog};

fn print_patches(label: &str, changelog: &Changelog) {
    println!("{}: {} file(s), {} patch(es), {} cheat(s), exefs files {:?}, subsdk {:?}",
        label,
        changelog.entries.len(),
        changelog.patches.len(),
        changelog.cheats.len(),
        changelog.exe_files,
        changelog.subsdk_files);
    for entry in &changelog.entries {
        println!("    file {}", entry.canonical);
    }
    for cheat in &changelog.cheats {
        println!("    cheat {} ({} build(s))", cheat.name, cheat.entries.len());
    }
    for patch in &changelog.patches {
        println!("  build {} — {} word(s)", patch.nso_binary_id, patch.entries.len());
        for (offset, value) in &patch.entries {
            println!("    {:#010x}  {:08x}", offset, value);
        }
    }
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: tkcl_contents <file.tkcl>");
    let package = read_tkcl_file(&path).unwrap_or_else(|error| panic!("{}", error));
    println!("{} {} (game version {})", package.name, package.version, package.changelog.game_version);
    print_patches("the mod itself", &package.changelog);
    for group in &package.option_groups {
        for option in &group.options {
            print_patches(&format!("option {} / {}", group.name, option.name), &option.changelog);
        }
    }
}
