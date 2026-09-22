//! Tests against real game files.
//!
//! Point `TOTK_VANILLA_DIR` at an extracted romfs (or any subset of it holding
//! `Pack/ZsDic.pack.zs`, a few `Pack/**.pack.zs` and
//! `System/Resource/ResourceSizeTable.Product.*.rsizetable.zs`) to run them:
//!
//!     TOTK_VANILLA_DIR=/path/to/romfs cargo test -p totk-formats -- --nocapture
//!
//! Without it the tests skip, so CI and a plain `cargo test` stay green.

use std::path::{Path, PathBuf};

use totk_formats::rstb::{self, Rstb};
use totk_formats::sarc::{Sarc, SarcBuilder};
use totk_formats::zstd::{compress_raw, Zstd};

fn vanilla_dir() -> Option<PathBuf> {
    std::env::var_os("TOTK_VANILLA_DIR").map(PathBuf::from)
}

fn zstd_with_dicts(root: &Path) -> Zstd {
    let mut zstd = Zstd::new();
    let packed = std::fs::read(root.join("Pack/ZsDic.pack.zs")).expect("ZsDic.pack.zs");
    let unpacked = zstd.decompress(&packed).expect("ZsDic is not dictionary compressed");
    let loaded = zstd.load_dictionaries(&unpacked).expect("load dictionaries");
    assert!(loaded >= 3, "expected at least 3 dictionaries, got {}", loaded);
    zstd
}

fn rstb_path(root: &Path) -> PathBuf {
    let dir = root.join("System/Resource");
    let mut candidates: Vec<_> = std::fs::read_dir(&dir)
        .expect("System/Resource")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.to_string_lossy().contains(".rsizetable"))
        .collect();
    candidates.sort();
    candidates.pop().expect("a rsizetable in System/Resource")
}

#[test]
fn reads_dictionaries_and_packs() {
    let Some(root) = vanilla_dir() else {
        eprintln!("skipped: TOTK_VANILLA_DIR unset");
        return;
    };
    let mut zstd = zstd_with_dicts(&root);

    let packs: Vec<PathBuf> = std::fs::read_dir(root.join("Pack/Actor"))
        .expect("Pack/Actor")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.to_string_lossy().ends_with(".pack.zs"))
        .take(50)
        .collect();
    assert!(!packs.is_empty());

    for path in packs {
        let raw = std::fs::read(&path).unwrap();
        let decompressed = zstd
            .decompress(&raw)
            .unwrap_or_else(|e| panic!("{}: {}", path.display(), e));
        let sarc = Sarc::parse(&decompressed).unwrap_or_else(|e| panic!("{}: {}", path.display(), e));
        assert!(!sarc.is_empty(), "{} has no entries", path.display());
    }
}

/// Rebuilding an untouched archive has to produce something the game can still
/// read, with the same contents.
#[test]
fn rebuilds_packs_losslessly() {
    let Some(root) = vanilla_dir() else {
        eprintln!("skipped: TOTK_VANILLA_DIR unset");
        return;
    };
    let mut zstd = zstd_with_dicts(&root);

    let mut checked = 0;
    for entry in std::fs::read_dir(root.join("Pack/Actor")).unwrap().flatten().take(25) {
        let path = entry.path();
        if !path.to_string_lossy().ends_with(".pack.zs") {
            continue;
        }
        let original = zstd.decompress(&std::fs::read(&path).unwrap()).unwrap();
        let sarc = Sarc::parse(&original).unwrap();

        let rebuilt = SarcBuilder::from_sarc(&sarc).build();
        let reparsed = Sarc::parse(&rebuilt).unwrap();

        assert_eq!(reparsed.len(), sarc.len(), "{}", path.display());
        for original_entry in sarc.entries() {
            let rebuilt_entry = reparsed
                .get(original_entry.name)
                .unwrap_or_else(|| panic!("{} lost {}", path.display(), original_entry.name));
            assert_eq!(rebuilt_entry.data, original_entry.data, "{}", original_entry.name);
        }

        // And it survives a round trip through our raw zstd frames.
        let framed = compress_raw(&rebuilt);
        assert_eq!(zstd.decompress(&framed).unwrap(), rebuilt);
        checked += 1;
    }
    assert!(checked > 0);
}

#[test]
fn parses_the_resource_size_table() {
    let Some(root) = vanilla_dir() else {
        eprintln!("skipped: TOTK_VANILLA_DIR unset");
        return;
    };
    let mut zstd = zstd_with_dicts(&root);

    let path = rstb_path(&root);
    let raw = zstd.decompress(&std::fs::read(&path).unwrap()).unwrap();
    let table = Rstb::parse(&raw).expect("parse rstb");

    println!(
        "rstb: version {} string_block {} hashes {} names {}",
        table.version,
        table.string_block_size,
        table.hash_entries(),
        table.name_entries()
    );
    assert_eq!(table.version, 1);
    assert!(table.hash_entries() > 10_000);

    // Writing it back must be byte identical, otherwise our writer is lossy.
    assert_eq!(table.write(), raw, "rstb round trip differs");
}

/// Works out which resource size formula the vanilla table agrees with.
#[test]
fn resource_sizes_match_the_vanilla_table() {
    let Some(root) = vanilla_dir() else {
        eprintln!("skipped: TOTK_VANILLA_DIR unset");
        return;
    };
    let mut zstd = zstd_with_dicts(&root);
    let table = Rstb::parse(&zstd.decompress(&std::fs::read(rstb_path(&root)).unwrap()).unwrap()).unwrap();

    let mut compared = 0;
    let mut fits = 0;
    for entry in std::fs::read_dir(root.join("Pack/Actor")).unwrap().flatten().take(200) {
        let path = entry.path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if !name.ends_with(".pack.zs") {
            continue;
        }
        let decompressed = zstd.decompress(&std::fs::read(&path).unwrap()).unwrap();
        let canonical = rstb::resource_name(&format!("Pack/Actor/{}", name));
        let Some(recorded) = table.get_size(&canonical) else {
            continue;
        };
        let computed = rstb::resource_size(decompressed.len() as u32, &canonical, &decompressed);
        compared += 1;
        if computed == recorded {
            fits += 1;
        } else {
            println!(
                "{}: computed {} != table {} (raw {})",
                canonical,
                computed,
                recorded,
                decompressed.len()
            );
        }
    }

    println!("compared {} entries, {} reproduced exactly", compared, fits);
    assert!(compared > 0);
    // For packs our formula has to reproduce Nintendo's own numbers exactly;
    // anything smaller would under-allocate a mod of the same size.
    assert_eq!(fits, compared);
}

/// Every BYML document inside a sample of actor packs must survive a parse,
/// write, parse cycle unchanged.
#[test]
fn round_trips_byml_documents() {
    use totk_formats::byml::Byml;

    let Some(root) = vanilla_dir() else {
        eprintln!("skipped: TOTK_VANILLA_DIR unset");
        return;
    };
    let mut zstd = zstd_with_dicts(&root);

    let mut documents = 0;
    let mut bytes = 0usize;
    let mut rewritten = 0usize;
    for entry in std::fs::read_dir(root.join("Pack/Actor")).unwrap().flatten().take(400) {
        let path = entry.path();
        if !path.to_string_lossy().ends_with(".pack.zs") {
            continue;
        }
        let pack = zstd.decompress(&std::fs::read(&path).unwrap()).unwrap();
        let sarc = Sarc::parse(&pack).unwrap();
        for file in sarc.entries() {
            if !(file.name.ends_with(".bgyml") || file.name.ends_with(".byml")) {
                continue;
            }
            let (parsed, version) = Byml::from_binary_with_version(file.data)
                .unwrap_or_else(|e| panic!("{} in {}: {}", file.name, path.display(), e));
            let written = parsed.to_binary(version);
            let reparsed = Byml::from_binary(&written)
                .unwrap_or_else(|e| panic!("rewritten {} does not parse: {}", file.name, e));
            assert_eq!(reparsed, parsed, "{} changed across a round trip", file.name);
            documents += 1;
            bytes += file.data.len();
            rewritten += written.len();
        }
    }

    println!(
        "{} documents round-tripped, {} bytes in, {} bytes out",
        documents, bytes, rewritten
    );
    assert!(documents > 100);
}

#[test]
fn round_trips_message_files() {
    use totk_formats::msbt::Msbt;

    let Some(root) = vanilla_dir() else {
        eprintln!("skipped: TOTK_VANILLA_DIR unset");
        return;
    };
    let mut zstd = zstd_with_dicts(&root);

    let mals: Vec<PathBuf> = std::fs::read_dir(root.join("Mals"))
        .expect("Mals")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .collect();
    assert!(!mals.is_empty());

    let (mut files, mut entries, mut attributes, mut identical) = (0, 0, 0, 0);
    for path in mals.iter().take(2) {
        let sarc_data = zstd.decompress(&std::fs::read(path).unwrap()).unwrap();
        let sarc = Sarc::parse(&sarc_data).unwrap();
        for entry in sarc.entries().filter(|e| e.name.ends_with(".msbt")) {
            let msbt = Msbt::parse(entry.data).unwrap_or_else(|e| panic!("{}: {}", entry.name, e));
            let written = msbt.write();
            let again = Msbt::parse(&written).unwrap();
            assert_eq!(again.len(), msbt.len(), "{}", entry.name);
            for (label, value) in msbt.entries() {
                assert_eq!(again.get(label), Some(value), "{} / {}", entry.name, label);
                attributes += value.attribute.is_some() as usize;
            }
            identical += (written == entry.data) as usize;
            files += 1;
            entries += msbt.len();
        }
    }
    eprintln!(
        "{} message files, {} entries ({} with attributes), {} byte-identical after a rewrite",
        files, entries, attributes, identical
    );
    assert!(files > 100);
}
