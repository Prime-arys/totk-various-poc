//! TkRom against a real romfs. Set `TOTK_VANILLA_DIR` to an extracted romfs.

use totk_merge::rom::TkRom;
use totk_merge::tkcl::attributes::*;

fn open() -> Option<TkRom> {
    let dir = std::env::var("TOTK_VANILLA_DIR").ok()?;
    let prefix = format!("{}/", dir.trim_end_matches(['/', '\\']));
    Some(TkRom::open(&prefix).expect("open romfs"))
}

#[test]
fn resolves_versioned_paths() {
    let Some(rom) = open() else {
        eprintln!("skipped: TOTK_VANILLA_DIR unset");
        return;
    };
    eprintln!("game version {}, nso {}", rom.game_version, rom.nso_binary_id);
    assert!(rom.game_version >= 100);

    let relative = rom.canonical_to_relative("RSDB/ActorInfo.Product.rstbl.byml", HAS_ZS_EXTENSION | IS_PRODUCT_FILE);
    eprintln!("ActorInfo -> {}", relative);
    assert!(relative.starts_with("RSDB/ActorInfo.Product.1"));
    assert!(relative.ends_with(".rstbl.byml.zs"));
    assert!(rom.get_vanilla(&relative).0.is_some());

    let gdl = rom.canonical_to_relative("GameData/GameDataList.Product.byml", HAS_ZS_EXTENSION | IS_PRODUCT_FILE);
    eprintln!("GameDataList -> {}", gdl);
    assert!(rom.get_vanilla(&gdl).0.is_some());

    let mals = rom.canonical_to_relative("Mals/USen.Product.sarc", HAS_ZS_EXTENSION | IS_PRODUCT_FILE);
    eprintln!("Mals -> {}", mals);
    assert!(rom.get_vanilla(&mals).0.is_some());

    eprintln!("locales: {:?}", rom.locales());
    eprintln!("rstb: {}", rom.resource_size_table_path());
    assert!(rom.get_vanilla(&rom.resource_size_table_path()).0.is_some());
}

#[test]
fn finds_files_that_only_live_in_packs() {
    let Some(rom) = open() else {
        eprintln!("skipped: TOTK_VANILLA_DIR unset");
        return;
    };

    let dir = std::env::var("TOTK_VANILLA_DIR").unwrap();
    let pack = std::fs::read_dir(format!("{}/Pack/Actor", dir))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .find(|name| name.ends_with(".pack.zs"))
        .unwrap();
    let data = rom.get_vanilla(&format!("Pack/Actor/{}", pack)).0.unwrap();
    let sarc = totk_formats::sarc::Sarc::parse(&data).unwrap();

    let mut found = 0;
    for entry in sarc.entries().take(20) {
        let (nested, missing) = rom.get_vanilla(entry.name);
        if let Some(nested) = nested {
            assert_eq!(nested.len(), entry.data.len(), "{}", entry.name);
            found += 1;
        } else {
            eprintln!("not found: {} (missing={})", entry.name, missing);
        }
    }
    eprintln!("{} nested files resolved from {}", found, pack);
    assert!(found > 0);
}
