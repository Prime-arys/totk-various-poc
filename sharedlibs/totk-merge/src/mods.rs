//! Finding mods to merge.
//!
//! Every mod has its own folder in the mods directory:
//!
//! ```text
//! sd:/totk/mods/<mod>/mod.ini          name, version, author... (optional)
//! sd:/totk/mods/<mod>/romfs/, exefs/   a folder mod, as TKMM exports them
//! sd:/totk/mods/<mod>/<name>.tkcl      or a TKMM package
//! sd:/totk/mods/<mod>/plugin.nro       code of its own (see [`plugin_files`])
//! ```
//!
//! Which mods are merged, in which order and with which options comes from the
//! active profile (see `profile`); without one, every mod is merged, ordered by
//! the `priority` in its `mod.ini`.

use alloc::collections::BTreeMap;

use crate::config::{self, Config};
use crate::info;
use crate::ini;
use crate::prelude::*;
use crate::profile::Profile;
use crate::sys::{fs, path};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ModKind {
    /// A folder holding `romfs` (and maybe `exefs`), as TKMM exports them.
    #[default]
    Folder,
    /// A TKMM package.
    Package,
    /// A folder that *is* a romfs root (RomFSlite exports, bare dumps).
    Romfs,
}

impl ModKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ModKind::Folder => "folder",
            ModKind::Package => "package",
            ModKind::Romfs => "romfs",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ModSpec {
    pub name: String,
    pub kind: ModKind,
    /// The folder to merge, or the `.tkcl` file of a package.
    pub path: String,
    pub priority: i32,
    /// Option group → selected options (packages only).
    pub options: BTreeMap<String, Vec<String>>,
    /// Skyline plugins (`.nro`) the mod ships, in load order. Only the plugin
    /// running inside the game loads them; see [`plugin_files`].
    pub plugins: Vec<String>,
}

/// What to merge, and where.
pub struct Plan {
    /// Lowest priority first.
    pub mods: Vec<ModSpec>,
    pub merged_dir: String,
    /// The profile, or who chose the mods through the API.
    pub profile: String,
}

pub const MOD_INI: &str = "mod.ini";

/// A mod's `mod.ini`.
#[derive(Debug, Clone)]
pub struct ModInfo {
    pub name: Option<String>,
    pub version: String,
    pub author: String,
    pub description: String,
    /// Where it came from (a GameBanana page...).
    pub url: String,
    /// An image file in the mod folder.
    pub thumbnail: String,
    /// Without a profile: whether it is merged, and in which order (lower
    /// first, so higher wins conflicts).
    pub enabled: bool,
    pub priority: i32,
    /// Whether the plugins the mod ships are loaded (`plugins = 0` in
    /// `mod.ini` keeps its files and leaves its code out).
    pub load_plugins: bool,
    /// Without a profile, or for groups a profile does not mention: the
    /// options to use instead of the package's defaults.
    pub options: BTreeMap<String, Vec<String>>,
    /// Everything else in the file, kept when it is written back.
    pub extra: Vec<(String, String)>,
}

impl Default for ModInfo {
    fn default() -> Self {
        ModInfo {
            name: None,
            version: String::new(),
            author: String::new(),
            description: String::new(),
            url: String::new(),
            thumbnail: String::new(),
            enabled: true,
            priority: 100,
            load_plugins: true,
            options: BTreeMap::new(),
            extra: Vec::new(),
        }
    }
}

impl ModInfo {
    pub fn load(file: &str) -> ModInfo {
        match fs::read_to_string(file) {
            Ok(contents) => ModInfo::parse(&contents),
            Err(_) => ModInfo::default(),
        }
    }

    pub fn parse(contents: &str) -> ModInfo {
        let mut info = ModInfo::default();
        for section in ini::parse(contents) {
            match section.name.as_str() {
                "" => {
                    for (key, value) in section.entries {
                        match key.to_ascii_lowercase().as_str() {
                            "name" if !value.is_empty() => info.name = Some(value),
                            "version" => info.version = value,
                            "author" => info.author = value,
                            "description" => info.description = value,
                            "url" => info.url = value,
                            "thumbnail" => info.thumbnail = value,
                            "enabled" => info.enabled = ini::parse_bool(&value, info.enabled),
                            "priority" => info.priority = value.trim().parse().unwrap_or(info.priority),
                            "plugins" => info.load_plugins = ini::parse_bool(&value, info.load_plugins),
                            _ => info.extra.push((key, value)),
                        }
                    }
                }
                "options" => info.options = config::parse_options(&section.entries, ""),
                _ => {}
            }
        }
        info
    }

    pub fn to_ini(&self) -> String {
        let mut out = String::new();
        let mut line = |key: &str, value: &str| {
            out.push_str(&format!("{} = {}\n", key, ini::escape(value)));
        };
        line("name", self.name.as_deref().unwrap_or(""));
        for (key, value) in [
            ("version", &self.version),
            ("author", &self.author),
            ("description", &self.description),
            ("url", &self.url),
            ("thumbnail", &self.thumbnail),
        ] {
            if !value.is_empty() {
                line(key, value);
            }
        }
        for (key, value) in &self.extra {
            line(key, value);
        }
        if !self.enabled {
            line("enabled", "0");
        }
        if self.priority != 100 {
            line("priority", &self.priority.to_string());
        }
        if !self.load_plugins {
            line("plugins", "0");
        }
        if !self.options.is_empty() {
            out.push_str("\n[options]\n");
            for (group, selected) in &self.options {
                out.push_str(&format!("{} = {}\n", group, ini::escape(&selected.join("; "))));
            }
        }
        out
    }
}

/// A mod folder in the mods directory.
#[derive(Debug, Clone)]
pub struct InstalledMod {
    /// The folder name, which identifies the mod in profiles.
    pub folder: String,
    pub path: String,
    pub kind: ModKind,
    /// What gets merged: the folder itself, or its `.tkcl`.
    pub content: String,
    /// The Skyline plugins it ships, whether or not `mod.ini` allows them.
    pub plugins: Vec<String>,
    pub info: ModInfo,
}

impl InstalledMod {
    pub fn name(&self) -> &str {
        self.info.name.as_deref().unwrap_or(&self.folder)
    }
}

fn has_extension(name: &str, extension: &str) -> bool {
    name.len() > extension.len() && name[name.len() - extension.len()..].eq_ignore_ascii_case(extension)
}

/// The folder a mod keeps extra plugins in, next to `romfs`.
pub const PLUGINS_DIR: &str = "plugins";

/// The Skyline plugins (`.nro`) a mod ships, in the order they are loaded:
/// the mod folder itself first (`plugin.nro`), then its `plugins` folder, each
/// sorted by name.
///
/// A mod that needs code of its own — what would otherwise be an exefs
/// replacement, of which a console can only run one — ships it this way, and
/// the merger loads it inside the game once the mods are merged.
pub fn plugin_files(dir: &str) -> Vec<String> {
    let mut found = Vec::new();
    for folder in [dir.to_string(), path::join(dir, PLUGINS_DIR)] {
        let Ok(mut entries) = fs::read_dir(&folder) else {
            continue;
        };
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        for entry in entries {
            if !entry.is_dir && has_extension(&entry.name, ".nro") {
                found.push(path::join(&folder, &entry.name));
            }
        }
    }
    found
}

/// A romfs root has the game's top-level folders.
fn looks_like_romfs(entries: &[fs::Entry]) -> bool {
    const ROMFS_FOLDERS: [&str; 8] = ["Pack", "RSDB", "GameData", "Mals", "Banc", "Component", "UI", "Model"];
    entries
        .iter()
        .any(|entry| entry.is_dir && ROMFS_FOLDERS.iter().any(|f| entry.name.eq_ignore_ascii_case(f)))
}

/// What a mod folder holds: (kind, path to merge). A mod one folder further
/// down (`<mod>/<name>/romfs`, as many archives unpack) is found too.
pub fn inspect_folder(dir: &str) -> Option<(ModKind, String)> {
    let entries = fs::read_dir(dir).ok()?;
    if let Some(found) = inspect_entries(dir, &entries) {
        return Some(found);
    }
    let mut folders = entries.iter().filter(|entry| entry.is_dir);
    match (folders.next(), folders.next()) {
        (Some(only), None) => {
            let child = path::join(dir, &only.name);
            let child_entries = fs::read_dir(&child).ok()?;
            inspect_entries(&child, &child_entries)
        }
        _ => None,
    }
}

fn inspect_entries(dir: &str, entries: &[fs::Entry]) -> Option<(ModKind, String)> {
    let mut packages: Vec<&str> = entries
        .iter()
        .filter(|entry| !entry.is_dir && has_extension(&entry.name, ".tkcl"))
        .map(|entry| entry.name.as_str())
        .collect();
    packages.sort();
    if let Some(first) = packages.first() {
        if packages.len() > 1 {
            info!("{} holds {} .tkcl files, using {}", dir, packages.len(), first);
        }
        return Some((ModKind::Package, path::join(dir, first)));
    }
    if entries
        .iter()
        .any(|entry| entry.is_dir && (entry.name.eq_ignore_ascii_case("romfs") || entry.name.eq_ignore_ascii_case("exefs")))
    {
        return Some((ModKind::Folder, dir.to_string()));
    }
    if looks_like_romfs(&entries) {
        return Some((ModKind::Romfs, dir.to_string()));
    }
    // A mod that is only code (`plugin.nro`) has nothing to merge, but is a
    // mod all the same: it is listed, ordered and turned on and off like the
    // others.
    (!plugin_files(dir).is_empty()).then(|| (ModKind::Folder, dir.to_string()))
}

/// What a path handed over by a plugin is: a `.tkcl`, or a folder as
/// [`inspect_folder`] sees it. Returns the kind and the path to merge.
pub fn classify(target: &str) -> Option<(ModKind, String)> {
    match fs::metadata(target) {
        Some(metadata) if !metadata.is_dir => {
            has_extension(target, ".tkcl").then(|| (ModKind::Package, target.to_string()))
        }
        Some(_) => inspect_folder(target),
        None => None,
    }
}

/// Every mod folder in the mods directory, sorted by folder name, and what
/// was left out (as log-ready messages).
pub fn scan(config: &Config) -> (Vec<InstalledMod>, Vec<String>) {
    let mut mods = Vec::new();
    let mut problems = Vec::new();
    let Ok(mut entries) = fs::read_dir(&config.mods_dir) else {
        problems.push(format!("no mods directory at {}", config.mods_dir));
        return (mods, problems);
    };
    entries.sort_by(|a, b| a.name.cmp(&b.name));

    for entry in entries {
        let dir = path::join(&config.mods_dir, &entry.name);
        if !entry.is_dir {
            if has_extension(&entry.name, ".tkcl") {
                problems.push(format!(
                    "'{}' ignored: every mod needs its own folder ({}/<mod>/{})",
                    entry.name, config.mods_dir, entry.name
                ));
            }
            continue;
        }
        let Some((kind, content)) = inspect_folder(&dir) else {
            problems.push(format!("'{}' ignored: no romfs or exefs folder, and no .tkcl file", entry.name));
            continue;
        };
        // Plugins sit next to `romfs`, which for a mod unpacked one folder
        // deeper is not the mod folder itself.
        let mut plugins = plugin_files(&dir);
        if content != dir && kind != ModKind::Package {
            plugins.extend(plugin_files(&content));
        }
        mods.push(InstalledMod {
            info: ModInfo::load(&path::join(&dir, MOD_INI)),
            folder: entry.name,
            path: dir,
            kind,
            content,
            plugins,
        });
    }
    (mods, problems)
}

fn spec_for(installed: &InstalledMod, priority: i32, options: BTreeMap<String, Vec<String>>) -> ModSpec {
    ModSpec {
        name: installed.name().to_string(),
        kind: installed.kind,
        path: installed.content.clone(),
        priority,
        options,
        plugins: if installed.info.load_plugins {
            installed.plugins.clone()
        } else {
            Vec::new()
        },
    }
}

/// Options for a mod: the profile's choice per group, over `mod.ini`'s.
fn merged_options(installed: &InstalledMod, chosen: &BTreeMap<String, Vec<String>>) -> BTreeMap<String, Vec<String>> {
    let mut options = installed.info.options.clone();
    for (group, selected) in chosen {
        options.insert(group.clone(), selected.clone());
    }
    options
}

/// The enabled mods, lowest priority first.
pub fn discover(config: &Config) -> Vec<ModSpec> {
    let mut specs = Vec::new();

    if config.use_romfslite && fs::is_dir(&config.romfslite_dir) {
        if let Some((kind @ (ModKind::Folder | ModKind::Romfs), content)) = inspect_folder(&config.romfslite_dir) {
            info!(
                "found a RomFSlite export in {}, merged under every other mod",
                config.romfslite_dir
            );
            specs.push(ModSpec {
                name: "RomFSlite".into(),
                kind,
                path: content,
                priority: i32::MIN,
                ..ModSpec::default()
            });
        }
    }

    let (installed, problems) = scan(config);
    for problem in problems {
        info!("{}", problem);
    }

    let profile = if config.profile.trim().is_empty() {
        None
    } else {
        match Profile::load(config, &config.profile) {
            Some(profile) => Some(profile),
            None => {
                info!(
                    "profile '{}' not found in {}, merging every mod instead",
                    config.profile, config.profiles_dir
                );
                None
            }
        }
    };

    match profile {
        Some(profile) => {
            let count = profile.mods.len() as i32;
            let mut chosen = Vec::new();
            for (index, entry) in profile.mods.iter().enumerate() {
                // A mod listed twice (a hand-edited profile) counts once, where
                // it comes first.
                if profile.mods[..index].iter().any(|earlier| earlier.folder.eq_ignore_ascii_case(&entry.folder)) {
                    info!("profile '{}': '{}' is listed twice, the second is ignored", profile.name, entry.folder);
                    continue;
                }
                let Some(installed) = installed.iter().find(|m| m.folder.eq_ignore_ascii_case(&entry.folder)) else {
                    if entry.enabled {
                        info!("profile '{}': '{}' is not installed", profile.name, entry.folder);
                    }
                    continue;
                };
                if entry.enabled {
                    chosen.push(spec_for(installed, count - index as i32, merged_options(installed, &entry.options)));
                }
            }
            for installed in &installed {
                if !profile.mods.iter().any(|entry| entry.folder.eq_ignore_ascii_case(&installed.folder)) {
                    info!("'{}' is not in profile '{}', left out", installed.folder, profile.name);
                }
            }
            // The profile lists the winner first; merges go the other way.
            chosen.reverse();
            specs.extend(chosen);
        }
        None => {
            let mut found: Vec<ModSpec> = installed
                .iter()
                .filter(|m| {
                    if !m.info.enabled {
                        info!("skipping '{}': disabled in its mod.ini", m.folder);
                    }
                    m.info.enabled
                })
                .map(|m| spec_for(m, m.info.priority, m.info.options.clone()))
                .collect();
            found.sort_by(|a, b| a.priority.cmp(&b.priority).then_with(|| a.name.cmp(&b.name)));
            specs.extend(found);
        }
    }
    specs
}

/// The mods on the SD card, as the plugin merges them when no other plugin
/// takes control.
pub fn local_plan(config: &Config) -> Plan {
    Plan {
        mods: discover(config),
        merged_dir: config.merged_dir.clone(),
        profile: if config.profile.trim().is_empty() {
            "local".into()
        } else {
            config.profile.clone()
        },
    }
}

/// Files under a folder, relative with forward slashes, sorted, with sizes.
pub fn list_files(root: &str) -> Vec<(String, u64)> {
    fn walk(dir: &str, relative: &str, out: &mut Vec<(String, u64)>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries {
            let child_relative = if relative.is_empty() {
                entry.name.clone()
            } else {
                format!("{}/{}", relative, entry.name)
            };
            if entry.is_dir {
                walk(&path::join(dir, &entry.name), &child_relative, out);
            } else {
                out.push((child_relative, entry.len));
            }
        }
    }
    let mut files = Vec::new();
    walk(root, "", &mut files);
    files.sort();
    files
}

/// FNV-1a, for cache fingerprints.
pub struct Fingerprint(u64);

impl Default for Fingerprint {
    fn default() -> Self {
        Fingerprint(0xcbf2_9ce4_8422_2325)
    }
}

impl Fingerprint {
    pub fn mix(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= *byte as u64;
            self.0 = self.0.wrapping_mul(0x100_0000_01b3);
        }
        // Separator, so ("ab", "c") and ("a", "bc") differ.
        self.0 ^= 0xFF;
        self.0 = self.0.wrapping_mul(0x100_0000_01b3);
    }

    pub fn mix_str(&mut self, text: &str) {
        self.mix(text.as_bytes());
    }

    pub fn value(&self) -> u64 {
        self.0
    }

    pub fn finish(&self) -> String {
        format!("{:016x}", self.0)
    }
}

/// Fingerprint of a mod's contents: names and sizes of its files.
pub fn content_fingerprint(spec: &ModSpec) -> String {
    let mut fingerprint = Fingerprint::default();
    fingerprint.mix_str(&spec.path);
    match spec.kind {
        ModKind::Package => {
            let size = fs::metadata(&spec.path).map(|m| m.len).unwrap_or(0);
            fingerprint.mix(&size.to_le_bytes());
        }
        ModKind::Folder | ModKind::Romfs => {
            for (file, size) in list_files(&spec.path) {
                // mod.ini, thumbnails and read-mes are not part of what gets
                // merged: editing them must not call for a new merge.
                let top = file.split('/').next().unwrap_or("");
                let merged = match spec.kind {
                    ModKind::Folder => file.contains('/') && (top.eq_ignore_ascii_case("romfs") || top.eq_ignore_ascii_case("exefs")),
                    _ => !file.eq_ignore_ascii_case(MOD_INI),
                };
                if !merged {
                    continue;
                }
                fingerprint.mix_str(&file);
                fingerprint.mix(&size.to_le_bytes());
            }
        }
    }
    fingerprint.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mod_info_round_trips() {
        let text = "name = Weapons\nversion = 1.2\ndescription = Two\\nlines\ngamebanana_file = 42\npriority = 5\n\n[options]\nWeapon Pack = Swords; Bows\n";
        let info = ModInfo::parse(text);
        assert_eq!(info.name.as_deref(), Some("Weapons"));
        assert_eq!(info.description, "Two\nlines");
        assert_eq!(info.priority, 5);
        assert_eq!(info.extra, vec![("gamebanana_file".to_string(), "42".to_string())]);
        assert_eq!(info.options["Weapon Pack"], vec!["Swords".to_string(), "Bows".to_string()]);
        let again = ModInfo::parse(&info.to_ini());
        assert_eq!(again.description, info.description);
        assert_eq!(again.options, info.options);
        assert_eq!(again.priority, 5);
        assert_eq!(again.extra, info.extra);
    }

    #[test]
    fn finds_mods_in_their_folders() {
        let root = std::env::temp_dir().join(format!("totk-mods-test-{}", std::process::id()));
        let root = root.to_string_lossy().replace('\\', "/");
        let _ = std::fs::remove_dir_all(&root);
        let write = |relative: &str| {
            let file = format!("{}/{}", root, relative);
            std::fs::create_dir_all(path::parent(&file).unwrap()).unwrap();
            std::fs::write(file, b"x").unwrap();
        };
        write("mods/Folder/romfs/Pack/a.pack.zs");
        write("mods/Folder/mod.ini");
        write("mods/Folder/plugin.nro");
        write("mods/Folder/plugins/extra.nro");
        write("mods/Package/Package.tkcl");
        write("mods/Nested/Nested Mod (v2)/romfs/Pack/b.pack.zs");
        write("mods/Nested/Nested Mod (v2)/plugin.nro");
        write("mods/Nested/readme.txt");
        write("mods/Code only/plugin.nro");
        write("mods/Empty/readme.txt");
        write("mods/Loose.tkcl");

        let mut config = Config::default();
        config.mods_dir = format!("{}/mods", root);
        let (installed, problems) = scan(&config);
        let found: Vec<(&str, ModKind, &str)> = installed
            .iter()
            .map(|m| (m.folder.as_str(), m.kind, path::strip_root(&m.content, &config.mods_dir).unwrap()))
            .collect();
        assert_eq!(
            found,
            vec![
                ("Code only", ModKind::Folder, "Code only"),
                ("Folder", ModKind::Folder, "Folder"),
                ("Nested", ModKind::Folder, "Nested/Nested Mod (v2)"),
                ("Package", ModKind::Package, "Package/Package.tkcl"),
            ]
        );
        assert_eq!(problems.len(), 2, "{:?}", problems);

        let plugins = |folder: &str| {
            let plugins = &installed.iter().find(|m| m.folder == folder).unwrap().plugins;
            plugins
                .iter()
                .map(|p| path::strip_root(p, &config.mods_dir).unwrap().to_string())
                .collect::<Vec<_>>()
        };
        // The mod folder first, then its plugins folder.
        assert_eq!(plugins("Folder"), vec!["Folder/plugin.nro", "Folder/plugins/extra.nro"]);
        // A mod unpacked one folder deeper keeps its plugin next to its romfs.
        assert_eq!(plugins("Nested"), vec!["Nested/Nested Mod (v2)/plugin.nro"]);
        assert_eq!(plugins("Code only"), vec!["Code only/plugin.nro"]);
        assert!(plugins("Package").is_empty());

        // `plugins = 0` in mod.ini keeps the mod's files and leaves its code out.
        std::fs::write(format!("{}/mods/Folder/mod.ini", root), b"plugins = 0
").unwrap();
        let specs = discover(&config);
        let spec = specs.iter().find(|s| s.name == "Folder").unwrap();
        assert!(spec.plugins.is_empty());

        let _ = std::fs::remove_dir_all(&root);
    }
}
