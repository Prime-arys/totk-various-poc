//! Merges mods with this crate and compares the result with a TKMM merge of
//! the same mods (made with utils/tkmm-oracle).
//!
//!     cargo run --release -p totk-merge --example compare_merge -- \
//!         <romfs> <tkmm output> <work dir> <mod>...
//!
//! Mods are folders or `.tkcl` files, lowest priority first. Files are
//! compared by content: BYML documents as trees, archives entry by entry,
//! message files label by label, the resource size table entry by entry.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use totk_formats::byml::Byml;
use totk_formats::msbt::Msbt;
use totk_formats::rstb::Rstb;
use totk_formats::sarc::Sarc;
use totk_merge::config::Config;
use totk_merge::engine::Engine;
use totk_merge::mods::{classify, ModSpec, Plan};
use totk_merge::rom::TkRom;

fn list(root: &Path, dir: &Path, out: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            list(root, &path, out);
        } else {
            out.push(path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"));
        }
    }
}

fn describe_difference(name: &str, ours: &[u8], theirs: &[u8], depth: usize, report: &mut Vec<String>) -> bool {
    if ours == theirs {
        return true;
    }
    let pad = "  ".repeat(depth);

    if ours.starts_with(b"SARC") && theirs.starts_with(b"SARC") {
        let (a, b) = (Sarc::parse(ours).unwrap(), Sarc::parse(theirs).unwrap());
        let mut same = true;
        let names: std::collections::BTreeSet<&str> = a.entries().map(|e| e.name).chain(b.entries().map(|e| e.name)).collect();
        for entry_name in names {
            match (a.get(entry_name), b.get(entry_name)) {
                (Some(x), Some(y)) => {
                    if !describe_difference(entry_name, x.data, y.data, depth + 1, report) {
                        same = false;
                    }
                }
                (Some(_), None) => {
                    report.push(format!("{}{}: only in ours", pad, entry_name));
                    same = false;
                }
                (None, Some(_)) => {
                    report.push(format!("{}{}: only in TKMM's", pad, entry_name));
                    same = false;
                }
                (None, None) => {}
            }
        }
        if !same {
            report.push(format!("{}^ in {}", pad, name));
        }
        return same;
    }

    if ours.starts_with(b"MsgStdBn") && theirs.starts_with(b"MsgStdBn") {
        let (a, b) = (Msbt::parse(ours).unwrap(), Msbt::parse(theirs).unwrap());
        let mut same = a.len() == b.len();
        for (label, entry) in a.entries() {
            if b.get(label) != Some(entry) {
                same = false;
                report.push(format!("{}{}: label {} differs", pad, name, label));
            }
        }
        if a.len() != b.len() {
            report.push(format!("{}{}: {} labels vs {}", pad, name, a.len(), b.len()));
        }
        return same;
    }

    if ours.starts_with(b"RESTBL") && theirs.starts_with(b"RESTBL") {
        // Compared by the caller, entry by entry.
        return false;
    }

    let parsed = (Byml::parse(ours), Byml::parse(theirs));
    if let (Ok((a, _)), Ok((b, _))) = parsed {
        if a.value_eq(&b) {
            if ours.len() != theirs.len() {
                report.push(format!("{}{}: same document, {} bytes vs {}", pad, name, ours.len(), theirs.len()));
                println!("  note: {} is the same document written in {} bytes (TKMM: {})", name, ours.len(), theirs.len());
            }
            return true;
        }
        let mut differences = Vec::new();
        diff_byml(&a, &b, &mut String::new(), &mut differences);
        report.push(format!("{}{}: BYML differs ({} place(s))", pad, name, differences.len()));
        for difference in differences.iter().take(8) {
            report.push(format!("{}  {}", pad, difference));
        }
        return false;
    }

    report.push(format!("{}{}: bytes differ ({} vs {} bytes)", pad, name, ours.len(), theirs.len()));
    false
}

fn diff_byml(a: &Byml, b: &Byml, path: &mut String, out: &mut Vec<String>) {
    if a.value_eq(b) || out.len() > 50 {
        return;
    }
    match (a, b) {
        (Byml::Map(x), Byml::Map(y)) => {
            let keys: std::collections::BTreeSet<&str> = x.keys().chain(y.keys()).map(|k| k.as_str()).collect();
            for key in keys {
                let len = path.len();
                path.push('/');
                path.push_str(key);
                match (x.get(key), y.get(key)) {
                    (Some(p), Some(q)) => diff_byml(p, q, path, out),
                    (p, q) => out.push(format!("{}: {:?} vs {:?}", path, p.map(short), q.map(short))),
                }
                path.truncate(len);
            }
        }
        (Byml::HashMap32(x), Byml::HashMap32(y)) => {
            let keys: std::collections::BTreeSet<&u32> = x.keys().chain(y.keys()).collect();
            for key in keys {
                let len = path.len();
                path.push_str(&format!("/{:08x}", key));
                match (x.get(key), y.get(key)) {
                    (Some(p), Some(q)) => diff_byml(p, q, path, out),
                    (p, q) => out.push(format!("{}: {:?} vs {:?}", path, p.map(short), q.map(short))),
                }
                path.truncate(len);
            }
        }
        (Byml::Array(x), Byml::Array(y)) => {
            if x.len() != y.len() {
                out.push(format!("{}: {} entries vs {}", path, x.len(), y.len()));
            }
            for (i, (p, q)) in x.iter().zip(y).enumerate() {
                let len = path.len();
                path.push_str(&format!("[{}]", i));
                diff_byml(p, q, path, out);
                path.truncate(len);
            }
        }
        _ => out.push(format!("{}: {} vs {}", path, short(a), short(b))),
    }
}

fn short(node: &Byml) -> String {
    let text = format!("{:?}", node);
    if text.len() > 80 {
        format!("{}…", &text[..80])
    } else {
        text
    }
}

fn main() {
    totk_merge::set_log_sink(|line| println!("  | {}", line));
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 4 {
        eprintln!("usage: compare_merge <romfs> <tkmm output> <work dir> <mod>...");
        std::process::exit(2);
    }
    let romfs = format!("{}/", args[0].trim_end_matches(['/', '\\']));
    let tkmm = PathBuf::from(&args[1]);
    let work = PathBuf::from(&args[2]);

    let mut config = Config::default();
    config.cache_dir = work.join("cache").to_string_lossy().to_string();
    config.locales = std::env::var("LOCALES").unwrap_or_else(|_| "USen".into());
    config.force_merge = true;
    config.use_romfslite = false;

    let mods: Vec<ModSpec> = args[3..]
        .iter()
        .enumerate()
        .map(|(i, path)| {
            let path = path.replace('\\', "/");
            let (kind, content) = classify(&path).expect("not a mod");
            ModSpec {
                name: totk_merge::sys::path::file_name(&path).to_string(),
                kind,
                path: content,
                priority: i as i32,
                options: BTreeMap::new(),
                plugins: Vec::new(),
            }
        })
        .collect();
    let plan = Plan {
        mods,
        merged_dir: work.join("merged").to_string_lossy().to_string(),
        profile: "compare".into(),
    };

    let started = std::time::Instant::now();
    let outcome = Engine::new(&config, &romfs).run(&plan);
    println!("our merge: {} files in {:.1}s", outcome.redirects.len(), started.elapsed().as_secs_f32());

    let rom = TkRom::open(&romfs).unwrap();
    let mut theirs = Vec::new();
    list(&tkmm.join("romfs"), &tkmm.join("romfs"), &mut theirs);
    theirs.sort();

    let (mut identical, mut equivalent, mut different, mut missing) = (0, 0, 0, 0);
    for relative in &theirs {
        let their_raw = std::fs::read(tkmm.join("romfs").join(relative)).unwrap();
        let Some(our_path) = outcome.redirects.get(relative) else {
            println!("MISSING  {}", relative);
            missing += 1;
            continue;
        };
        let our_raw = std::fs::read(our_path).unwrap();
        if our_raw == their_raw {
            println!("SAME     {}", relative);
            identical += 1;
            continue;
        }
        let ours = rom.decompress(&our_raw).unwrap();
        let theirs_data = rom.decompress(&their_raw).unwrap();

        if relative.contains(".rsizetable") {
            let (a, b) = (Rstb::parse(&ours).unwrap(), Rstb::parse(&theirs_data).unwrap());
            // TKMM trims the name block to the longest name; the entries are
            // what matters.
            if a.hash_table() == b.hash_table() && a.name_table() == b.name_table() {
                println!("EQUAL    {} (resource size table)", relative);
                equivalent += 1;
            } else {
                println!(
                    "DIFFERS  {} ({} / {} hash entries, {} / {} names)",
                    relative,
                    a.hash_entries(),
                    b.hash_entries(),
                    a.name_entries(),
                    b.name_entries()
                );
                different += 1;
                // Resource names for the hashes, from everything served.
                let mut names: BTreeMap<u32, String> = BTreeMap::new();
                for (romfs_path, sd_path) in &outcome.redirects {
                    let name = totk_formats::rstb::resource_name(romfs_path);
                    names.insert(totk_formats::crc32::compute_str(&name), name);
                    if let Ok(data) = rom.decompress(&std::fs::read(sd_path).unwrap()) {
                        if let Ok(sarc) = Sarc::parse(&data) {
                            for entry in sarc.entries() {
                                names.insert(
                                    totk_formats::crc32::compute_str(entry.name),
                                    format!("{} ({} bytes, in {})", entry.name, entry.data.len(), romfs_path),
                                );
                            }
                        }
                    }
                }
                let (ha, hb) = (a.hash_table(), b.hash_table());
                let hb: BTreeMap<u32, u32> = hb.into_iter().collect();
                for (hash, size) in ha.iter().filter(|(h, s)| hb.get(h) != Some(s)).take(20) {
                    println!("  hash {:08x}: ours {} TKMM {:?} {}", hash, size, hb.get(hash), names.get(hash).map(String::as_str).unwrap_or("?"));
                }
                for (name, size) in a.name_table() {
                    if b.name_table().get(name) != Some(size) {
                        println!("  name {}: ours {} TKMM {:?}", name, size, b.name_table().get(name));
                    }
                }
            }
            continue;
        }

        let mut report = Vec::new();
        if describe_difference(relative, &ours, &theirs_data, 1, &mut report) {
            println!("EQUAL    {}", relative);
            equivalent += 1;
        } else {
            println!("DIFFERS  {}", relative);
            for line in report {
                println!("{}", line);
            }
            different += 1;
        }
    }

    let extra: Vec<&String> = outcome.redirects.keys().filter(|k| !theirs.contains(k)).collect();
    for relative in &extra {
        println!("EXTRA    {}", relative);
    }

    if let Ok(entries) = std::fs::read_dir(tkmm.join("exefs")) {
        for entry in entries.flatten() {
            let data = std::fs::read(entry.path()).unwrap();
            let theirs_patch = totk_merge::builder::parse_ips(&data, "").map(|p| p.entries).unwrap_or_default();
            let same = theirs_patch == outcome.patches;
            println!("{} exefs patches ({} ours, {} TKMM's)", if same { "SAME    " } else { "DIFFERS " }, outcome.patches.len(), theirs_patch.len());
        }
    }

    println!(
        "\n{} identical, {} equivalent, {} different, {} missing, {} extra",
        identical,
        equivalent,
        different,
        missing,
        extra.len()
    );
}
