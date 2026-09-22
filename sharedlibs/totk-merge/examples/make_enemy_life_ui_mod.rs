//! Brings back the enemy life numbers over the health gauge.
//!
//!     cargo run --release -p totk-merge --example make_enemy_life_ui_mod -- \
//!         <romfs> <project dir> [locale,locale...]
//!
//! Writes a TKMM project (`romfs/`, `options/`, `.tkproj`) that
//! `tkmm-oracle package` turns into a `.tkcl`: the messages go in its base,
//! the layout in an option group with one option per text size ([`SIZES`]).
//!
//! Tears of the Kingdom still *fills in* those numbers: the code reads the
//! enemy's current and maximum life, formats each through the message
//! `LayoutMsg/EnemyInfo_00` (labels `0000` and `0001`) and writes them into
//! panes named `T_CurrentLife_00` and `T_MaxLife_00` of the enemy gauge — the
//! same way Breath of the Wild did with the Champion's Tunic. What the game no
//! longer ships is the other half:
//!
//! - `blyt/PaEnemyLife_00.bflyt` (in the Common layout archive) has no text in
//!   it any more, only the empty anchor `N_TextVisible_00` that its
//!   `TextVisible` animation shows and hides;
//! - `LayoutMsg/EnemyInfo_00.msbt` is gone from the message archives.
//!
//! So this builds both back: two text panes under the anchor, and the two
//! messages, each a single "number" tag copied from a message that still uses
//! one.
//!
//! The big gauge of bosses and mini-bosses (`blyt/BossLife_00.bflyt`, same
//! archive) never had numbers, and the game has no code to fill them: it gets
//! one text pane, `T_BossLife_00`, empty, under the left end of its bar, which
//! the mod's plugin writes "current/max" into (`plugins/enemy-hp/src/boss.rs`).

use std::path::{Path, PathBuf};

use totk_formats::msbt::{Msbt, MsbtEntry, ENCODING_UTF16};
use totk_formats::sarc::{Sarc, SarcBuilder};
use totk_formats::zstd::compress_raw;
use totk_merge::rom::TkRom;

const LAYOUT: &str = "blyt/PaEnemyLife_00.bflyt";
const ANCHOR: &str = "N_TextVisible_00";
const CURRENT_PANE: &str = "T_CurrentLife_00";
const MAX_PANE: &str = "T_MaxLife_00";
const MESSAGE: &str = "LayoutMsg/EnemyInfo_00.msbt";
/// A message that still carries a number tag, which ours is copied from.
const NUMBER_SOURCE: (&str, &str) = ("LayoutMsg/NumDisplay_00.msbt", "0000");
/// The game's own number font (Rodin bold, which `NumDisplay_00` uses).
const FONT: &str = "Normal_00.fcpx";

/// The boss gauge, its bar's anchor, and the pane the plugin writes into.
const BOSS_LAYOUT: &str = "blyt/BossLife_00.bflyt";
const BOSS_ANCHOR: &str = "N_Gauge_00";
const BOSS_PANE: &str = "T_BossLife_00";
/// Where the bar starts: its background (`W_BaseSh_00`) is anchored by its
/// left end there and grows to the right with the boss's maximum life (the
/// `GaugeMax` animation), from 32 to 1206 wide.
const BOSS_BAR_LEFT: f32 = -306.0;
/// Half the bar's height (23 × 0.94 / 2).
const BOSS_BAR_HALF_HEIGHT: f32 = 10.8;

/// The option group the sizes are offered in, and each size: (folder, name,
/// description, font size of the enemy numbers and of the boss numbers in
/// layout units, selected by default). The boss gauge is drawn at screen
/// scale, the enemy gauge smaller: the boss numbers get bigger sizes.
const SIZE_GROUP: &str = "1 Taille des chiffres";
const SIZES: &[(&str, &str, &str, f32, f32, bool)] = &[
    ("1 Petite", "Petite", "La taille du premier essai.", 12.0, 20.0, false),
    ("2 Moyenne", "Moyenne", "Un tiers plus grand.", 16.0, 24.0, false),
    ("3 Grande", "Grande", "Lisible de loin.", 20.0, 28.0, true),
    ("4 Tres grande", "Très grande", "Pour jouer loin de l'écran.", 26.0, 34.0, false),
];

/// TKMM's option group types, as its project files number them.
const SINGLE_REQUIRED: u32 = 3;

// --- bflyt ---------------------------------------------------------------

struct Section {
    tag: [u8; 4],
    payload: Vec<u8>,
}

impl Section {
    fn name(&self) -> String {
        match &self.tag {
            b"pan1" | b"pic1" | b"txt1" | b"wnd1" | b"prt1" | b"bnd1" => {
                let raw = &self.payload[4..28];
                String::from_utf8_lossy(&raw[..raw.iter().position(|b| *b == 0).unwrap_or(24)]).into_owned()
            }
            _ => String::new(),
        }
    }
}

fn read_sections(data: &[u8]) -> (Vec<u8>, Vec<Section>) {
    let header_size = u16::from_le_bytes([data[6], data[7]]) as usize;
    let count = u16::from_le_bytes([data[0x10], data[0x11]]) as usize;
    let mut sections = Vec::with_capacity(count);
    let mut offset = header_size;
    for _ in 0..count {
        let tag = [data[offset], data[offset + 1], data[offset + 2], data[offset + 3]];
        let length = u32::from_le_bytes(data[offset + 4..offset + 8].try_into().unwrap()) as usize;
        sections.push(Section {
            tag,
            payload: data[offset + 8..offset + length].to_vec(),
        });
        offset += length;
    }
    (data[..header_size].to_vec(), sections)
}

fn write_sections(header: &[u8], sections: &[Section]) -> Vec<u8> {
    let mut out = header.to_vec();
    for section in sections {
        out.extend_from_slice(&section.tag);
        out.extend_from_slice(&((section.payload.len() + 8) as u32).to_le_bytes());
        out.extend_from_slice(&section.payload);
    }
    let size = out.len() as u32;
    out[0x0C..0x10].copy_from_slice(&size.to_le_bytes());
    out[0x10..0x12].copy_from_slice(&(sections.len() as u16).to_le_bytes());
    out
}

/// A `fnl1` payload listing `fonts`. Its offsets are relative to the start of
/// the offset table (section + 0xC).
fn font_list(fonts: &[String]) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&(fonts.len() as u16).to_le_bytes());
    payload.extend_from_slice(&0u16.to_le_bytes());
    let mut offset = 4 * fonts.len();
    for font in fonts {
        payload.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += font.len() + 1;
    }
    for font in fonts {
        payload.extend_from_slice(font.as_bytes());
        payload.push(0);
    }
    while payload.len() % 4 != 0 {
        payload.push(0);
    }
    payload
}

/// The fonts a `fnl1` payload lists.
fn font_names(payload: &[u8]) -> Vec<String> {
    let count = u16::from_le_bytes([payload[0], payload[1]]) as usize;
    (0..count)
        .map(|i| {
            let start = 4 + u32::from_le_bytes(payload[4 + i * 4..8 + i * 4].try_into().unwrap()) as usize;
            let end = start + payload[start..].iter().position(|b| *b == 0).unwrap_or(0);
            String::from_utf8_lossy(&payload[start..end]).into_owned()
        })
        .collect()
}

/// The index of `font` in the layout's font list, which gets it (or gets a
/// list, after the textures where the game puts it) when it lacks it.
fn use_font(sections: &mut Vec<Section>, font: &str) -> u16 {
    match sections.iter().position(|s| &s.tag == b"fnl1") {
        Some(at) => {
            let mut fonts = font_names(&sections[at].payload);
            if let Some(index) = fonts.iter().position(|name| name == font) {
                return index as u16;
            }
            fonts.push(font.to_string());
            sections[at].payload = font_list(&fonts);
            (fonts.len() - 1) as u16
        }
        None => {
            let textures = sections.iter().position(|s| &s.tag == b"txl1").expect("txl1");
            sections.insert(
                textures + 1,
                Section {
                    tag: *b"fnl1",
                    payload: font_list(&[font.to_string()]),
                },
            );
            0
        }
    }
}

/// Appends a text material to the layout's list (so the other indices stay
/// put) and returns its index.
fn add_text_material(sections: &mut [Section], name: &str) -> u16 {
    let at = sections.iter().position(|s| &s.tag == b"mat1").expect("mat1");
    let mut list = materials(&sections[at].payload);
    list.push(text_material(name));
    sections[at].payload = material_list(&list);
    (list.len() - 1) as u16
}

/// Splits a `mat1` payload into its materials.
fn materials(payload: &[u8]) -> Vec<Vec<u8>> {
    let count = u16::from_le_bytes([payload[0], payload[1]]) as usize;
    let offsets: Vec<usize> = (0..count)
        .map(|i| u32::from_le_bytes(payload[4 + i * 4..8 + i * 4].try_into().unwrap()) as usize - 8)
        .collect();
    (0..count)
        .map(|i| {
            let end = offsets.get(i + 1).copied().unwrap_or(payload.len());
            payload[offsets[i]..end].to_vec()
        })
        .collect()
}

/// Rebuilds a `mat1` payload from its materials (offsets count the section's
/// own 8 byte header).
fn material_list(materials: &[Vec<u8>]) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&(materials.len() as u16).to_le_bytes());
    payload.extend_from_slice(&0u16.to_le_bytes());
    let mut offset = 8 + 4 + materials.len() * 4;
    for material in materials {
        payload.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += material.len();
    }
    for material in materials {
        payload.extend_from_slice(material);
    }
    payload
}

/// A material a text pane can use: a name and the fields of the game's own
/// text materials, which carry no texture.
fn text_material(name: &str) -> Vec<u8> {
    let mut material = vec![0u8; 20];
    let bytes = name.as_bytes();
    material[..bytes.len().min(20)].copy_from_slice(&bytes[..bytes.len().min(20)]);
    // Taken from `T_Name_00` of BossLife_00: no texture, no blending tables,
    // white text with the game's usual interpolation.
    material.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    material.extend_from_slice(&[0x00, 0x02, 0x04, 0x08]);
    material.extend_from_slice(&[0, 0, 0, 0]);
    material.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);
    material
}

/// Where a text sits in its box, packed as horizontal | vertical << 2 with
/// 0 centre, 1 left/top, 2 right/bottom.
const TEXT_RIGHT: u8 = 2;
const TEXT_LEFT: u8 = 1;

#[allow(clippy::too_many_arguments)]
fn text_pane(
    name: &str,
    material: u16,
    font: u16,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    size: f32,
    position: u8,
    initial: &str,
) -> Section {
    let mut string: Vec<u8> = initial.encode_utf16().flat_map(|unit| unit.to_le_bytes()).collect();
    string.extend_from_slice(&[0, 0]); // terminator
    let box_name = format!("@{}", name);

    let mut payload = Vec::new();
    payload.extend_from_slice(&[0x01, 0x00, 0xFF, 0x00]); // visible, origin, alpha, magnify
    let mut fixed_name = [0u8; 24];
    fixed_name[..name.len()].copy_from_slice(name.as_bytes());
    payload.extend_from_slice(&fixed_name);
    payload.extend_from_slice(&[0u8; 8]); // user data
    payload.extend_from_slice(&x.to_le_bytes());
    payload.extend_from_slice(&y.to_le_bytes());
    payload.extend_from_slice(&0f32.to_le_bytes());
    payload.extend_from_slice(&[0u8; 12]); // rotation
    payload.extend_from_slice(&1f32.to_le_bytes());
    payload.extend_from_slice(&1f32.to_le_bytes());
    payload.extend_from_slice(&width.to_le_bytes());
    payload.extend_from_slice(&height.to_le_bytes());

    // Where the strings land, counting the section's 8 byte header.
    let text_offset = 8 + 76 + 84;
    let name_offset = text_offset + ((string.len() + 3) & !3);

    payload.extend_from_slice(&64u16.to_le_bytes()); // text buffer, room for the number
    payload.extend_from_slice(&(string.len() as u16).to_le_bytes());
    payload.extend_from_slice(&material.to_le_bytes());
    payload.extend_from_slice(&font.to_le_bytes());
    payload.push(position); // where the text sits in its box
    payload.push(0); // line alignment: as the text is written
    // The text stays within its buffer. No drop shadow (bit 0): the game's
    // own texts never use one either.
    payload.push(0x02);
    payload.push(0);
    payload.extend_from_slice(&0f32.to_le_bytes()); // italic tilt
    payload.extend_from_slice(&(text_offset as u32).to_le_bytes());
    payload.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]); // top colour, RGBA
    payload.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]); // bottom colour
    payload.extend_from_slice(&size.to_le_bytes());
    payload.extend_from_slice(&size.to_le_bytes());
    payload.extend_from_slice(&0f32.to_le_bytes()); // character spacing
    payload.extend_from_slice(&0f32.to_le_bytes()); // line spacing
    payload.extend_from_slice(&(name_offset as u32).to_le_bytes());
    payload.extend_from_slice(&1f32.to_le_bytes()); // shadow offset x
    payload.extend_from_slice(&(-1f32).to_le_bytes()); // shadow offset y
    payload.extend_from_slice(&1f32.to_le_bytes()); // shadow scale x
    payload.extend_from_slice(&1f32.to_le_bytes()); // shadow scale y
    payload.extend_from_slice(&[0x00, 0x00, 0x00, 0xFF]); // shadow top colour
    payload.extend_from_slice(&[0x00, 0x00, 0x00, 0xFF]); // shadow bottom colour
    payload.extend_from_slice(&0f32.to_le_bytes()); // shadow italic
    payload.extend_from_slice(&0u32.to_le_bytes()); // line width offsets
    payload.extend_from_slice(&0u32.to_le_bytes()); // per character transform

    payload.extend_from_slice(&string);
    while (payload.len() + 8) % 4 != 0 {
        payload.push(0);
    }
    payload.extend_from_slice(box_name.as_bytes());
    payload.push(0);
    while (payload.len() + 8) % 4 != 0 {
        payload.push(0);
    }

    Section {
        tag: *b"txt1",
        payload,
    }
}

/// Adds the two life panes to the enemy gauge layout, with text of `size`.
fn patch_layout(data: &[u8], size: f32) -> Vec<u8> {
    let (header, mut sections) = read_sections(data);
    let font = use_font(&mut sections, FONT);
    // One material for both panes.
    let material = add_text_material(&mut sections, CURRENT_PANE);

    // Under the anchor the TextVisible animation shows and hides, so the
    // numbers appear exactly when the game asks for them.
    let anchor = sections
        .iter()
        .position(|s| s.name() == ANCHOR)
        .expect("the layout has no N_TextVisible_00 pane");
    //
    // The two boxes meet at x = 0, the current life pushed against it from the
    // left and "/max" from the right, so they read as one "35/35" over the bar
    // (which spans y = -3..3). The game writes both.
    let width = size * 4.0;
    let height = size * 1.4;
    let y = 3.0 + height / 2.0 + 1.0;
    let children = vec![
        Section {
            tag: *b"pas1",
            payload: Vec::new(),
        },
        text_pane(CURRENT_PANE, material, font, -width / 2.0, y, width, height, size, TEXT_RIGHT, "88888"),
        text_pane(MAX_PANE, material, font, width / 2.0, y, width, height, size, TEXT_LEFT, "88888"),
        Section {
            tag: *b"pae1",
            payload: Vec::new(),
        },
    ];
    for (index, section) in children.into_iter().enumerate() {
        sections.insert(anchor + 1 + index, section);
    }

    write_sections(&header, &sections)
}

/// Adds the life pane to the boss gauge layout, with text of `size`.
fn patch_boss_layout(data: &[u8], size: f32) -> Vec<u8> {
    let (header, mut sections) = read_sections(data);
    let font = use_font(&mut sections, FONT);
    let material = add_text_material(&mut sections, BOSS_PANE);

    // A child of the bar's own pane, so it moves and fades in with the bar.
    // That pane has children already: the new one goes first among them.
    let anchor = sections
        .iter()
        .position(|s| s.name() == BOSS_ANCHOR)
        .expect("the boss layout has no N_Gauge_00 pane");
    assert_eq!(&sections[anchor + 1].tag, b"pas1", "N_Gauge_00 has no children");
    //
    // Under the bar, starting where it starts: however long the bar is, the
    // numbers sit under its left end. Empty until the plugin writes "1200/1500"
    // into it, so a game without the plugin shows nothing there.
    let width = size * 7.0;
    let height = size * 1.4;
    let y = -(BOSS_BAR_HALF_HEIGHT + 4.0 + height / 2.0);
    let pane = text_pane(
        BOSS_PANE,
        material,
        font,
        BOSS_BAR_LEFT + width / 2.0,
        y,
        width,
        height,
        size,
        TEXT_LEFT,
        "",
    );
    sections.insert(anchor + 2, pane);

    write_sections(&header, &sections)
}

// --- texts ---------------------------------------------------------------

/// The two messages the game asks for, each the number tag of an existing
/// message (the second with a slash, as "current/max").
fn build_messages(number_tag: &[u8]) -> Vec<u8> {
    let slash: Vec<u8> = "/".encode_utf16().flat_map(|unit| unit.to_le_bytes()).collect();
    let mut file = Msbt::new(ENCODING_UTF16);
    file.insert(
        "0000".to_string(),
        MsbtEntry {
            attribute: None,
            text: number_tag.to_vec(),
        },
    );
    let mut with_slash = slash;
    with_slash.extend_from_slice(number_tag);
    file.insert(
        "0001".to_string(),
        MsbtEntry {
            attribute: None,
            text: with_slash,
        },
    );
    file.write()
}

// --- the mod -------------------------------------------------------------

/// The file in `folder` whose name starts with `prefix` and ends with `suffix`
/// (the game versions its archives: `Common.Product.110.…`).
fn versioned(root: &str, folder: &str, prefix: &str, suffix: &str) -> String {
    let dir = format!("{}/{}", root, folder);
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {}", dir, e))
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.starts_with(prefix) && name.ends_with(suffix))
        .collect();
    names.sort();
    let name = names.pop().unwrap_or_else(|| panic!("no {}*{} in {}", prefix, suffix, dir));
    format!("{}/{}", folder, name)
}

/// Writes `data` as `<root>/romfs/<relative>`, `root` being the project or
/// one of its options.
fn write_archive(root: &Path, relative: &str, data: Vec<u8>) {
    let file = root.join("romfs").join(relative);
    std::fs::create_dir_all(file.parent().expect("parent")).expect("create the mod folder");
    std::fs::write(&file, compress_raw(&data)).expect("write");
    println!("  {} ({} KiB)", relative, (data.len() + 1023) / 1024);
}

/// A TKMM project file (`info.json`, `.tkproj`), which it reads as JSON.
fn write_json(file: &Path, fields: &[(&str, String)]) {
    let body: Vec<String> = fields.iter().map(|(key, value)| format!("  \"{}\": {}", key, value)).collect();
    std::fs::create_dir_all(file.parent().expect("parent")).expect("create folder");
    std::fs::write(file, format!("{{\n{}\n}}\n", body.join(",\n"))).expect("write");
}

fn quoted(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("usage: make_enemy_life_ui_mod <romfs> <project dir> [locale,locale...]");
        std::process::exit(2);
    }
    let root = args[0].trim_end_matches(['/', '\\']).to_string();
    let out = PathBuf::from(&args[1]);
    // Every language by default: a game in a language without the messages
    // would fill the panes with nothing.
    let locales: Vec<String> = match args.get(2) {
        Some(value) => value.split(',').map(str::to_string).collect(),
        None => {
            let mut found: Vec<String> = std::fs::read_dir(format!("{}/Mals", root))
                .expect("Mals")
                .filter_map(|entry| entry.ok())
                .filter_map(|entry| entry.file_name().into_string().ok())
                .filter(|name| name.ends_with(".sarc.zs"))
                .filter_map(|name| name.split('.').next().map(str::to_string))
                .collect();
            found.sort();
            found.dedup();
            found
        }
    };
    let rom = TkRom::open(&format!("{}/", root)).expect("romfs");

    // The layout, in the Common archive: one variant per text size, each in
    // an option of its own.
    let archive = versioned(&root, "UI/LayoutArchive", "Common.Product.", ".blarc.zs");
    let data = rom.get_vanilla(&archive).0.expect("layout archive");
    let sarc = Sarc::parse(&data).expect("archive");
    let layout = sarc.get(LAYOUT).expect("the enemy gauge layout").data;
    let boss_layout = sarc.get(BOSS_LAYOUT).expect("the boss gauge layout").data;
    let group = out.join("options").join(SIZE_GROUP);
    write_json(
        &group.join("info.json"),
        &[
            ("Name", quoted("Taille des chiffres")),
            ("Description", quoted("Taille des PV affichés au-dessus de la jauge des ennemis et sous celle des boss.")),
            ("Type", SINGLE_REQUIRED.to_string()),
            ("Priority", "0".to_string()),
        ],
    );
    for (folder, name, description, size, boss_size, default) in SIZES {
        let option = group.join(folder);
        let mut builder = SarcBuilder::from_sarc(&sarc);
        builder.insert(LAYOUT, patch_layout(layout, *size));
        builder.insert(BOSS_LAYOUT, patch_boss_layout(boss_layout, *boss_size));
        println!(
            "{} ({}, bosses {}): {} and {} added to {}, {} to {}",
            name, size, boss_size, CURRENT_PANE, MAX_PANE, LAYOUT, BOSS_PANE, BOSS_LAYOUT
        );
        write_archive(&option, &archive, builder.build());
        write_json(
            &option.join("info.json"),
            &[
                ("Name", quoted(name)),
                ("Description", quoted(description)),
                ("Priority", "0".to_string()),
                ("IsDefaultSelected", default.to_string()),
            ],
        );
    }

    // The messages, in every language asked for.
    for locale in &locales {
        let archive = versioned(&root, "Mals", &format!("{}.Product.", locale), ".sarc.zs");
        let data = rom.get_vanilla(&archive).0.expect("message archive");
        let sarc = Sarc::parse(&data).expect("archive");
        let source = sarc.get(NUMBER_SOURCE.0).expect("a message with a number");
        let tag = Msbt::parse(source.data)
            .expect("message file")
            .get(NUMBER_SOURCE.1)
            .expect("the number message")
            .text
            .clone();
        let mut builder = SarcBuilder::from_sarc(&sarc);
        builder.insert(MESSAGE, build_messages(&tag));
        println!("{}: {} added", archive, MESSAGE);
        write_archive(&out, &archive, builder.build());
    }

    // What TKMM (and the merger's log) calls the package.
    write_json(
        &out.join(".tkproj"),
        &[(
            "Mod",
            format!(
                "{{\"Name\": {}, \"Version\": {}, \"Author\": {}, \"Description\": {}}}",
                quoted("Enemy HP"),
                quoted("2.3"),
                quoted("totk-mod-merger"),
                quoted("Les PV des ennemis en chiffres au-dessus de leur jauge, et sous celle des boss.")
            ),
        )],
    );
    println!("project written to {}", out.display());
}
