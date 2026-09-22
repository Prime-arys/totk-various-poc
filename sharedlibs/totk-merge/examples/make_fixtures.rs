//! Generates mod folders with overlapping, realistic edits made from vanilla
//! files, to compare this merger with TKMM's on the same input.
//!
//!     cargo run -p totk-merge --example make_fixtures -- <romfs> <out dir>
//!
//! Writes `<out>/fixture-a` and `<out>/fixture-b` (plain mod folders):
//! - both edit different fields of the same document inside the same pack,
//! - both change ActorInfo (an edited row, another edited row, a new row),
//! - both change the GameDataList (a new flag, an edited default),
//! - both edit texts in the USen message archive,
//! - A adds a file to a pack and a new top-level file, B adds a tag.

use std::path::{Path, PathBuf};

use totk_formats::byml::Byml;
use totk_formats::msbt::{Msbt, MsbtEntry};
use totk_formats::sarc::{Sarc, SarcBuilder};
use totk_formats::zstd::{compress_raw, Zstd};

struct Rom {
    root: PathBuf,
    zstd: Zstd,
}

impl Rom {
    fn read(&mut self, relative: &str) -> Vec<u8> {
        let raw = std::fs::read(self.root.join(relative)).unwrap_or_else(|e| panic!("{}: {}", relative, e));
        self.zstd.decompress(&raw).unwrap()
    }

    fn find(&self, dir: &str, prefix: &str) -> String {
        let mut names: Vec<String> = std::fs::read_dir(self.root.join(dir))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.starts_with(prefix))
            .collect();
        names.sort();
        format!("{}/{}", dir, names.first().unwrap_or_else(|| panic!("{}/{}*", dir, prefix)))
    }
}

fn write(out: &Path, mod_name: &str, relative: &str, data: &[u8]) {
    let path = out.join(mod_name).join("romfs").join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let bytes = if relative.ends_with(".zs") { compress_raw(data) } else { data.to_vec() };
    std::fs::write(&path, bytes).unwrap();
    println!("  {}/{}", mod_name, relative);
}

/// Paths (in a map tree) of numeric scalars, depth first.
fn numeric_fields(node: &Byml, path: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
    if let Byml::Map(map) = node {
        for (key, value) in map {
            path.push(key.to_string());
            match value {
                Byml::Int(_) | Byml::Float(_) | Byml::UInt32(_) => out.push(path.clone()),
                Byml::Map(_) => numeric_fields(value, path, out),
                _ => {}
            }
            path.pop();
        }
    }
}

fn bump(node: &mut Byml, path: &[String]) {
    let mut current = node;
    for key in path {
        current = current.as_map_mut().unwrap().get_mut(key.as_str()).unwrap();
    }
    let bumped = match &*current {
        Byml::Int(v) => Byml::Int(*v + 7),
        Byml::UInt32(v) => Byml::UInt32(*v + 7),
        Byml::Float(v) => Byml::Float(*v + 1.5),
        other => other.clone(),
    };
    *current = bumped;
}

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(romfs), Some(out)) = (args.next(), args.next()) else {
        eprintln!("usage: make_fixtures <romfs> <out dir>");
        std::process::exit(2);
    };
    let out = PathBuf::from(out);
    let _ = std::fs::remove_dir_all(out.join("fixture-a"));
    let _ = std::fs::remove_dir_all(out.join("fixture-b"));

    let mut rom = Rom {
        root: PathBuf::from(romfs),
        zstd: Zstd::new(),
    };
    let dictionaries = rom.read("Pack/ZsDic.pack.zs");
    rom.zstd.load_dictionaries(&dictionaries).unwrap();

    // 1. Two edits to one document inside one pack.
    let mut packs: Vec<String> = std::fs::read_dir(rom.root.join("Pack/Actor"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.starts_with("Weapon_Sword_") && n.ends_with(".pack.zs"))
        .collect();
    packs.sort();
    let mut chosen = None;
    'search: for pack_name in &packs {
        let relative = format!("Pack/Actor/{}", pack_name);
        let data = rom.read(&relative);
        let sarc = Sarc::parse(&data).unwrap();
        for entry in sarc.entries() {
            if !entry.name.ends_with(".bgyml") {
                continue;
            }
            let Ok(document) = Byml::from_binary(entry.data) else {
                continue;
            };
            let mut fields = Vec::new();
            numeric_fields(&document, &mut Vec::new(), &mut fields);
            if fields.len() >= 4 {
                chosen = Some((relative, data.clone(), entry.name.to_string(), fields));
                break 'search;
            }
        }
    }
    let (pack_path, pack_data, doc_name, fields) = chosen.expect("a weapon pack with a numeric document");
    println!("pack {} / {} ({} numeric fields)", pack_path, doc_name, fields.len());

    let pack = Sarc::parse(&pack_data).unwrap();
    let (document, format) = Byml::parse(pack.get(&doc_name).unwrap().data).unwrap();
    for (mod_name, field) in [("fixture-a", &fields[0]), ("fixture-b", &fields[fields.len() - 1])] {
        let mut edited = document.clone();
        bump(&mut edited, field);
        let mut builder = SarcBuilder::from_sarc(&pack);
        builder.insert(&doc_name, edited.write(format));
        if mod_name == "fixture-a" {
            builder.insert("Component/Fixture/Fixture.game__fixture__Custom.bgyml", {
                let mut map = totk_formats::byml::Map::new();
                map.insert("Fixture".into(), Byml::from("added by fixture-a"));
                Byml::Map(map).to_binary(7)
            });
        }
        write(&out, mod_name, &pack_path, &builder.build());
        println!("    {} edits {}", mod_name, field.join("."));
    }

    // 2. ActorInfo rows.
    let actor_info = rom.find("RSDB", "ActorInfo.Product.");
    let (table, format) = Byml::parse(&rom.read(&actor_info)).unwrap();
    let rows = table.as_array().unwrap();
    let numeric_row_field = |row: &Byml| -> Option<Vec<String>> {
        let mut fields = Vec::new();
        numeric_fields(row, &mut Vec::new(), &mut fields);
        fields.into_iter().next()
    };
    let mut table_a = table.clone();
    let row_a = (0..rows.len()).find(|&i| numeric_row_field(&rows[i]).is_some()).unwrap();
    bump(&mut table_a.as_array_mut().unwrap()[row_a], &numeric_row_field(&rows[row_a]).unwrap());
    write(&out, "fixture-a", &format!("{}.zs", actor_info.trim_end_matches(".zs")), &table_a.write(format));

    let mut table_b = table.clone();
    let row_b = (row_a + 10..rows.len()).find(|&i| numeric_row_field(&rows[i]).is_some()).unwrap();
    bump(&mut table_b.as_array_mut().unwrap()[row_b], &numeric_row_field(&rows[row_b]).unwrap());
    let mut new_row = rows[row_b].clone();
    new_row
        .as_map_mut()
        .unwrap()
        .insert("__RowId".into(), Byml::from("Fixture_New_Actor"));
    table_b.as_array_mut().unwrap().push(new_row);
    write(&out, "fixture-b", &format!("{}.zs", actor_info.trim_end_matches(".zs")), &table_b.write(format));

    // 3. GameDataList: a new Bool flag in A, an edited Int default in B.
    let gdl_path = rom.find("GameData", "GameDataList.Product.");
    let (gdl, format) = Byml::parse(&rom.read(&gdl_path)).unwrap();
    let mut gdl_a = gdl.clone();
    {
        let bools = gdl_a.as_map_mut().unwrap().get_mut("Data").unwrap().as_map_mut().unwrap().get_mut("Bool").unwrap();
        let rows = bools.as_array_mut().unwrap();
        let mut flag = rows[0].clone();
        flag.as_map_mut().unwrap().insert("Hash".into(), Byml::UInt32(0xF1C7_0001));
        rows.push(flag);
    }
    write(&out, "fixture-a", &gdl_path, &gdl_a.write(format));

    let mut gdl_b = gdl.clone();
    {
        let ints = gdl_b.as_map_mut().unwrap().get_mut("Data").unwrap().as_map_mut().unwrap().get_mut("Int").unwrap();
        let row = &mut ints.as_array_mut().unwrap()[3];
        if let Some(Byml::Int(value)) = row.as_map_mut().unwrap().get_mut("DefaultValue") {
            *value += 42;
        }
    }
    write(&out, "fixture-b", &gdl_path, &gdl_b.write(format));

    // 4. Texts.
    let mals_path = rom.find("Mals", "USen.Product.");
    let mals = rom.read(&mals_path);
    let mals_sarc = Sarc::parse(&mals).unwrap();
    let msbt_names: Vec<String> = mals_sarc
        .entries()
        .filter(|e| e.name.ends_with(".msbt"))
        .map(|e| e.name.to_string())
        .take(2)
        .collect();
    for (mod_name, msbt_name) in [("fixture-a", &msbt_names[0]), ("fixture-b", &msbt_names[1])] {
        let mut msbt = Msbt::parse(mals_sarc.get(msbt_name).unwrap().data).unwrap();
        let label = msbt.entries().next().unwrap().0.to_string();
        let text: Vec<u8> = format!("Edited by {}", mod_name)
            .encode_utf16()
            .flat_map(|u| u.to_le_bytes())
            .collect();
        msbt.insert(label.clone(), MsbtEntry { attribute: None, text });
        let mut builder = SarcBuilder::from_sarc(&mals_sarc);
        builder.insert(msbt_name, msbt.write());
        write(&out, mod_name, &mals_path, &builder.build());
        println!("    {} edits {} / {}", mod_name, msbt_name, label);
    }

    // 5. Extras: a new top-level file (A), a tag added to an entry (B).
    write(&out, "fixture-a", "Fixture/Readme.txt", b"added by fixture-a");

    let tag_path = rom.find("RSDB", "Tag.Product.");
    let (tags, format) = Byml::parse(&rom.read(&tag_path)).unwrap();
    let mut tags_b = tags.clone();
    {
        let root = tags_b.as_map_mut().unwrap();
        let tag_list: Vec<String> = root["TagList"].as_array().unwrap().iter().map(|t| t.as_str().unwrap().to_string()).collect();
        let entry_count = root["PathList"].as_array().unwrap().len() / 3;
        let Byml::Binary(bits) = root.get_mut("BitTable").unwrap() else { panic!("BitTable") };
        // Set the last tag on the first entry that does not have it.
        let tag = tag_list.len() - 1;
        for entry in 0..entry_count {
            let bit = entry * tag_list.len() + tag;
            if bits[bit / 8] & (1 << (bit % 8)) == 0 {
                bits[bit / 8] |= 1 << (bit % 8);
                println!("    fixture-b tags entry {} with {}", entry, tag_list[tag]);
                break;
            }
        }
    }
    write(&out, "fixture-b", &tag_path, &tags_b.write(format));
}
