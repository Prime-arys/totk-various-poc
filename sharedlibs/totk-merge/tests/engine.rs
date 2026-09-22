//! The whole pipeline against real game files: two folder mods whose edits
//! overlap, merged by the engine, checked in the files it serves.
//!
//!     TOTK_VANILLA_DIR=/path/to/romfs cargo test -p totk-merge --test engine -- --nocapture
//!
//! (Byte-for-byte parity with TKMM itself is checked by the `compare_merge`
//! example against utils/tkmm-oracle.)

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use totk_formats::byml::Byml;
use totk_formats::msbt::Msbt;
use totk_formats::sarc::{Sarc, SarcBuilder};
use totk_formats::zstd::compress_raw;
use totk_merge::config::Config;
use totk_merge::conflicts::ConflictKind;
use totk_merge::engine::Engine;
use totk_merge::mods::{ModKind, ModSpec, Plan};
use totk_merge::rom::TkRom;

fn romfs() -> Option<String> {
    let dir = std::env::var("TOTK_VANILLA_DIR").ok()?;
    Some(format!("{}/", dir.trim_end_matches(['/', '\\'])))
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("totk-merge-engine-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(root: &Path, relative: &str, data: &[u8]) {
    let path = root.join("romfs").join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, if relative.ends_with(".zs") { compress_raw(data) } else { data.to_vec() }).unwrap();
}

/// Paths of the Int/Float fields directly inside a map.
fn numeric_keys(node: &Byml) -> Vec<String> {
    node.as_map()
        .map(|map| {
            map.iter()
                .filter(|(_, v)| matches!(v, Byml::Int(_) | Byml::Float(_)))
                .map(|(k, _)| k.to_string())
                .collect()
        })
        .unwrap_or_default()
}

fn bumped(node: &Byml, key: &str) -> Byml {
    let mut node = node.clone();
    let value = node.as_map_mut().unwrap().get_mut(key).unwrap();
    let next = match &*value {
        Byml::Int(v) => Byml::Int(*v + 11),
        Byml::Float(v) => Byml::Float(*v + 2.5),
        other => other.clone(),
    };
    *value = next;
    node
}

/// A pack holding a document with at least two numeric fields at its root:
/// (pack path, pack, document name, numeric keys).
fn find_document(rom: &TkRom, prefix: &str) -> (String, Vec<u8>, String, Vec<String>) {
    let mut packs: Vec<String> = std::fs::read_dir(format!("{}Pack/Actor", prefix))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".pack.zs"))
        .collect();
    packs.sort();
    packs
        .iter()
        .take(400)
        .find_map(|name| {
            let relative = format!("Pack/Actor/{}", name);
            let data = rom.get_vanilla(&relative).0?;
            let sarc = Sarc::parse(&data).ok()?;
            let found = sarc.entries().find_map(|entry| {
                if !entry.name.ends_with(".bgyml") {
                    return None;
                }
                let keys = numeric_keys(&Byml::from_binary(entry.data).ok()?);
                (keys.len() >= 2).then(|| (entry.name.to_string(), keys))
            })?;
            Some((relative, data.clone(), found.0, found.1))
        })
        .expect("a pack with a suitable document")
}

#[test]
fn overlapping_folder_mods_merge() {
    let Some(prefix) = romfs() else {
        eprintln!("skipped: TOTK_VANILLA_DIR unset");
        return;
    };
    totk_merge::set_log_sink(|line| println!("  | {}", line));
    let rom = TkRom::open(&prefix).unwrap();
    let (pack_path, pack_data, document_name, keys) = find_document(&rom, &prefix);
    let pack = Sarc::parse(&pack_data).unwrap();
    let (document, format) = Byml::parse(pack.get(&document_name).unwrap().data).unwrap();

    let gdl_path = rom.canonical_to_relative("GameData/GameDataList.Product.byml", 5);
    let gdl = Byml::from_binary(&rom.get_vanilla(&gdl_path).0.unwrap()).unwrap();

    let work = scratch("overlap");
    let mut mods = Vec::new();
    for (i, key) in keys.iter().take(2).enumerate() {
        let root = work.join(format!("mod-{}", i));

        let mut builder = SarcBuilder::from_sarc(&pack);
        builder.insert(&document_name, bumped(&document, key).write(format));
        write(&root, &pack_path, &builder.build());

        // Each mod adds its own flag.
        let mut edited = gdl.clone();
        let rows = edited
            .as_map_mut()
            .unwrap()
            .get_mut("Data")
            .unwrap()
            .as_map_mut()
            .unwrap()
            .get_mut("Bool")
            .unwrap()
            .as_array_mut()
            .unwrap();
        let mut flag = rows[0].clone();
        flag.as_map_mut().unwrap().insert("Hash".into(), Byml::UInt32(0xFEED_0000 + i as u32));
        rows.push(flag);
        write(&root, &gdl_path, &edited.to_binary(7));

        mods.push(ModSpec {
            name: format!("mod-{}", i),
            kind: ModKind::Folder,
            path: root.to_string_lossy().replace('\\', "/"),
            priority: i as i32,
            options: BTreeMap::new(),
                plugins: Vec::new(),
        });
    }

    let mut config = Config::default();
    config.cache_dir = work.join("cache").to_string_lossy().to_string();
    config.use_romfslite = false;
    config.locales = "USen".into();
    let plan = Plan {
        mods,
        merged_dir: work.join("merged").to_string_lossy().to_string(),
        profile: "test".into(),
    };

    let outcome = Engine::new(&config, &prefix).run(&plan);
    assert!(!outcome.failed);
    // Different values of one document, and different flags: no conflict.
    assert_eq!(outcome.conflicts, 0);

    // Both edits to the document survive.
    let served = std::fs::read(&outcome.redirects[&pack_path]).unwrap();
    let merged_pack = rom.decompress(&served).unwrap();
    let merged = Byml::from_binary(Sarc::parse(&merged_pack).unwrap().get(&document_name).unwrap().data).unwrap();
    for key in keys.iter().take(2) {
        assert!(merged.as_map().unwrap()[key.as_str()].value_eq(&bumped(&document, key).as_map().unwrap()[key.as_str()]), "{}", key);
    }

    // Both flags are in the GameDataList.
    let served = std::fs::read(&outcome.redirects[&gdl_path]).unwrap();
    let merged_gdl = Byml::from_binary(&rom.decompress(&served).unwrap()).unwrap();
    let hashes: Vec<u32> = merged_gdl.as_map().unwrap()["Data"].as_map().unwrap()["Bool"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|row| match row.as_map()?.get("Hash")? {
            Byml::UInt32(h) => Some(*h),
            _ => None,
        })
        .collect();
    assert!(hashes.contains(&0xFEED_0000) && hashes.contains(&0xFEED_0001));

    // The resource size table is served too, and the second run is cached.
    assert!(outcome.redirects.keys().any(|k| k.contains(".rsizetable")));
    let again = Engine::new(&config, &prefix).run(&plan);
    assert_eq!(again.redirects, outcome.redirects);

    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn conflicting_folder_mods_are_reported() {
    let Some(prefix) = romfs() else {
        eprintln!("skipped: TOTK_VANILLA_DIR unset");
        return;
    };
    let rom = TkRom::open(&prefix).unwrap();
    let (pack_path, pack_data, document_name, keys) = find_document(&rom, &prefix);
    let pack = Sarc::parse(&pack_data).unwrap();
    let (document, format) = Byml::parse(pack.get(&document_name).unwrap().data).unwrap();

    // A message archive and one of its texts.
    let mals_path = "Mals/USen.Product.121.sarc.zs";
    let mals = rom.decompress(&rom.get_vanilla(mals_path).0.unwrap()).unwrap();
    let mals_sarc = Sarc::parse(&mals).unwrap();
    let (msbt_name, label) = mals_sarc
        .entries()
        .find_map(|entry| {
            let msbt = Msbt::parse(entry.data).ok()?;
            let label = msbt.entries().next()?.0.to_string();
            Some((entry.name.to_string(), label))
        })
        .unwrap();
    let msbt = Msbt::parse(mals_sarc.get(&msbt_name).unwrap().data).unwrap();

    let work = scratch("conflicts");
    let mut mods = Vec::new();
    for i in 0..2 {
        let root = work.join("mods").join(format!("mod-{}", i));
        // Both change the same field, to different values.
        let mut edited = bumped(&document, &keys[0]);
        if i == 1 {
            edited = bumped(&edited, &keys[0]);
        }
        let mut builder = SarcBuilder::from_sarc(&pack);
        builder.insert(&document_name, edited.write(format));
        write(&root, &pack_path, &builder.build());
        // A file nothing merges: the same in both, then different.
        write(&root, "Test/Same.bin", b"identical");
        write(&root, "Test/Different.bin", format!("mod {}", i).as_bytes());
        // The same text, reworded differently.
        let mut texts = msbt.clone();
        let mut entry = texts.get(&label).unwrap().clone();
        entry.text = format!("Mod {}", i).encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        texts.insert(label.clone(), entry);
        let mut archive = SarcBuilder::from_sarc(&mals_sarc);
        archive.insert(&msbt_name, texts.write());
        write(&root, mals_path, &archive.build());

        mods.push(ModSpec {
            name: format!("Mod {}", i),
            kind: ModKind::Folder,
            path: root.to_string_lossy().replace('\\', "/"),
            priority: i as i32,
            options: BTreeMap::new(),
                plugins: Vec::new(),
        });
    }

    let mut config = Config::default();
    config.mods_dir = work.join("mods").to_string_lossy().replace('\\', "/");
    config.cache_dir = work.join("cache").to_string_lossy().to_string();
    config.use_romfslite = false;
    config.locales = "USen".into();
    let plan = Plan {
        mods,
        merged_dir: work.join("merged").to_string_lossy().to_string(),
        profile: "test".into(),
    };

    // Declined: nothing is written.
    totk_merge::set_conflict_sink(Some(|conflicts| {
        assert_eq!(conflicts.len(), 3);
        false
    }));
    let declined = Engine::new(&config, &prefix).run(&plan);
    totk_merge::set_conflict_sink(None);
    assert!(declined.cancelled && declined.redirects.is_empty());
    assert!(!work.join("merged").exists());

    let outcome = Engine::new(&config, &prefix).run(&plan);
    assert!(!outcome.failed && !outcome.cancelled);
    assert_eq!(outcome.conflicts, 3);

    let saved = totk_merge::conflicts::load(&config.cache_dir);
    let values = saved
        .iter()
        .find(|c| c.kind == ConflictKind::Values && !c.file.starts_with("Mals/"))
        .expect("a value conflict");
    assert_eq!(values.file, document_name);
    assert_eq!(values.count, 1);
    assert_eq!(values.samples, vec![keys[0].clone()]);
    // The winner (highest priority) first, known by folder and name.
    assert_eq!(values.mods[0].folder, "mod-1");
    assert_eq!(values.mods[0].name, "Mod 1");
    let file = saved.iter().find(|c| c.kind == ConflictKind::File).expect("a file conflict");
    assert_eq!(file.file, "Test/Different.bin");
    assert_eq!(file.mods.len(), 2);
    let text = saved.iter().find(|c| c.file.starts_with("Mals/")).expect("a text conflict");
    assert_eq!(text.file, format!("Mals/USen/{}", msbt_name));
    assert_eq!(text.samples, vec![label.clone()]);

    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn earlier_merges_come_back_from_the_cache() {
    let Some(prefix) = romfs() else {
        eprintln!("skipped: TOTK_VANILLA_DIR unset");
        return;
    };
    let rom = TkRom::open(&prefix).unwrap();
    let (pack_path, pack_data, document_name, keys) = find_document(&rom, &prefix);
    let pack = Sarc::parse(&pack_data).unwrap();
    let (document, format) = Byml::parse(pack.get(&document_name).unwrap().data).unwrap();

    let work = scratch("cache");
    let spec = |i: usize| {
        let root = work.join(format!("mod-{}", i));
        let mut builder = SarcBuilder::from_sarc(&pack);
        builder.insert(&document_name, bumped(&document, &keys[i]).write(format));
        write(&root, &pack_path, &builder.build());
        // A file served from the mod folder.
        write(&root, "Test/Shared.bin", b"the same in every mod");
        ModSpec {
            name: format!("mod-{}", i),
            kind: ModKind::Folder,
            path: root.to_string_lossy().replace('\\', "/"),
            priority: 0,
            options: BTreeMap::new(),
                plugins: Vec::new(),
        }
    };
    let plan = |i: usize| Plan {
        mods: vec![spec(i)],
        merged_dir: work.join("merged").to_string_lossy().replace('\\', "/"),
        profile: format!("profile {}", i),
    };
    let (a, b) = (plan(0), plan(1));

    let mut config = Config::default();
    config.cache_dir = work.join("cache").to_string_lossy().to_string();
    config.use_romfslite = false;
    config.locales = "USen".into();

    let first = Engine::new(&config, &prefix).run(&a);
    assert!(!first.reused && !first.failed);
    let other = Engine::new(&config, &prefix).run(&b);
    assert!(!other.reused);
    assert!(totk_merge::engine::is_plan_merged(&config, &a) && totk_merge::engine::is_plan_merged(&config, &b));

    // Back to the first mods: served from the cache, as they were.
    let again = Engine::new(&config, &prefix).run(&a);
    assert!(again.reused && again.from_cache);
    assert_eq!(again.redirects, first.redirects);
    // Then it is the most recent one.
    let once_more = Engine::new(&config, &prefix).run(&a);
    assert!(once_more.reused && !once_more.from_cache);
    // Merged files come from the store; whole files from the mod folders.
    assert!(first.redirects[&pack_path].contains("/store/"));
    assert!(first.redirects["Test/Shared.bin"].contains("/mod-0/"));

    // With room for one merge, switching merges again, and b's files go.
    config.merge_cache_size = 1;
    let only_b = Engine::new(&config, &prefix).run(&b);
    assert!(only_b.reused);
    let c = Engine::new(&config, &prefix).run(&a);
    assert!(c.reused, "a was still the most recent and kept");
    let merged_dir = work.join("merged");
    let entries = |dir: &Path| std::fs::read_dir(dir).unwrap().flatten().filter(|e| e.file_name() != "store" && e.path().is_dir()).count();
    assert_eq!(entries(&merged_dir), 2);
    let b_again = Engine::new(&config, &prefix).run(&b);
    assert!(b_again.reused);
    std::fs::remove_dir_all(merged_dir.join("store")).unwrap();
    config.force_merge = true;
    let rebuilt = Engine::new(&config, &prefix).run(&a);
    assert!(!rebuilt.reused);
    assert_eq!(entries(&merged_dir), 1);
    assert!(!totk_merge::engine::is_plan_merged(&config, &b));
    for file in rebuilt.redirects.values().filter(|f| f.contains("/store/")) {
        assert!(Path::new(file).exists(), "{}", file);
    }

    let _ = std::fs::remove_dir_all(&work);
}
