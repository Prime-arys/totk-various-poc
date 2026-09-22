//! Installing a downloaded mod: finding what an extracted archive holds, the
//! way TKMM's archive reader does, and moving it into its own mod folder.

use totk_merge::config::Config;
use totk_merge::mods::{self, ModInfo, ModKind};
use totk_merge::prelude::*;
use totk_merge::profile::{self, Profile};
use totk_merge::sys::{fs, path};

/// Something in an extracted archive that can become a mod.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub kind: ModKind,
    /// The `.tkcl`, the folder holding `romfs`/`exefs`, or the romfs root.
    pub path: String,
    /// Where it sits in the archive ("" for the top), to tell options apart.
    pub label: String,
    /// Executables TotK mods cannot use through the merger (subsdk, main.npdm).
    pub has_code: bool,
    pub size: u64,
}

const ROMFS_FOLDERS: [&str; 8] = ["Pack", "RSDB", "GameData", "Mals", "Banc", "Component", "UI", "Model"];

fn has_extension(name: &str, extension: &str) -> bool {
    name.len() > extension.len() && name[name.len() - extension.len()..].eq_ignore_ascii_case(extension)
}

fn folder_size(dir: &str) -> u64 {
    mods::list_files(dir).iter().map(|(_, size)| size).sum()
}

/// Code files a merger cannot use: anything in exefs but patches.
fn has_code_files(dir: &str) -> bool {
    mods::list_files(dir)
        .iter()
        .any(|(file, _)| !has_extension(file, ".ips") && !has_extension(file, ".pchtxt") && !file.starts_with('.'))
}

/// Every mod an extracted archive holds. Packages come first: TKMM prefers an
/// embedded `.tkcl` over the loose files next to it.
pub fn find_candidates(root: &str) -> Vec<Candidate> {
    let mut packages = Vec::new();
    let mut folders = Vec::new();
    walk(root, root, 0, &mut packages, &mut folders);
    packages.sort_by(|a, b| a.label.cmp(&b.label));
    folders.sort_by(|a, b| a.label.cmp(&b.label));
    packages.extend(folders);
    packages
}

fn walk(root: &str, dir: &str, depth: usize, packages: &mut Vec<Candidate>, folders: &mut Vec<Candidate>) {
    if depth > 8 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let label = path::strip_root(dir, root).unwrap_or("").to_string();

    for entry in entries.iter().filter(|e| !e.is_dir && has_extension(&e.name, ".tkcl")) {
        packages.push(Candidate {
            kind: ModKind::Package,
            path: path::join(dir, &entry.name),
            label: path::join(&label, &entry.name),
            has_code: false,
            size: entry.len,
        });
    }

    let child = |name: &str| entries.iter().find(|e| e.is_dir && e.name.eq_ignore_ascii_case(name));
    let romfs = child("romfs");
    let exefs = child("exefs");
    if romfs.is_some() || exefs.is_some() {
        folders.push(Candidate {
            kind: ModKind::Folder,
            path: dir.to_string(),
            label,
            has_code: exefs.map_or(false, |e| has_code_files(&path::join(dir, &e.name))),
            size: folder_size(dir),
        });
        return;
    }
    if entries.iter().any(|e| e.is_dir && ROMFS_FOLDERS.iter().any(|f| e.name.eq_ignore_ascii_case(f))) {
        folders.push(Candidate {
            kind: ModKind::Romfs,
            path: dir.to_string(),
            label,
            has_code: false,
            size: folder_size(dir),
        });
        return;
    }
    // A mod that is only a Skyline plugin (`plugin.nro`), which the merger
    // loads inside the game.
    if !mods::plugin_files(dir).is_empty() {
        folders.push(Candidate {
            kind: ModKind::Folder,
            path: dir.to_string(),
            label,
            has_code: false,
            size: folder_size(dir),
        });
        return;
    }
    for entry in entries.iter().filter(|e| e.is_dir) {
        walk(root, &path::join(dir, &entry.name), depth + 1, packages, folders);
    }
}

/// A mod folder name made from a display name: what FAT32 accepts, trimmed.
pub fn folder_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_control() || "\\/:*?\"<>|".contains(c) { '_' } else { c })
        .collect();
    let trimmed = cleaned.trim().trim_end_matches('.').trim();
    let mut result: String = trimmed.chars().take(64).collect();
    if result.is_empty() {
        result = "mod".into();
    }
    result
}

pub struct InstallRequest<'a> {
    pub source: &'a str,
    pub kind: ModKind,
    pub folder: &'a str,
    pub info: ModInfo,
    /// An image to move next to the mod (e.g. a GameBanana screenshot).
    pub thumbnail: Option<&'a str>,
}

/// Moves `request.source` into `<mods>/<folder>`, replacing the content of an
/// existing mod of that name (its settings are kept), writes its `mod.ini` and
/// puts it at the top of the active profile.
pub fn install(config: &Config, request: InstallRequest<'_>) -> Result<String, String> {
    let target = config.mod_path(request.folder);
    fs::create_dir_all(&target).map_err(|e| e.to_string())?;

    // An update: previous content goes, settings stay.
    if let Some((_, _)) = mods::inspect_folder(&target) {
        for entry in fs::read_dir(&target).map_err(|e| e.to_string())? {
            let child = path::join(&target, &entry.name);
            let is_content = if entry.is_dir {
                entry.name.eq_ignore_ascii_case("romfs")
                    || entry.name.eq_ignore_ascii_case("exefs")
                    || entry.name.eq_ignore_ascii_case(mods::PLUGINS_DIR)
            } else {
                has_extension(&entry.name, ".tkcl") || has_extension(&entry.name, ".nro")
            };
            if is_content {
                let removed = if entry.is_dir { fs::remove_dir_all(&child) } else { fs::remove_file(&child) };
                removed.map_err(|e| e.to_string())?;
            }
        }
    }

    match request.kind {
        ModKind::Package => {
            let name = path::file_name(request.source);
            fs::rename(request.source, &path::join(&target, name)).map_err(|e| e.to_string())?;
        }
        ModKind::Folder => {
            let mut moved = 0;
            for entry in fs::read_dir(request.source).map_err(|e| e.to_string())? {
                // Its files, and the code it ships (`plugin.nro`, `plugins/`),
                // which the merger loads in the game.
                let content = match entry.is_dir {
                    true => {
                        entry.name.eq_ignore_ascii_case("romfs")
                            || entry.name.eq_ignore_ascii_case("exefs")
                            || entry.name.eq_ignore_ascii_case(mods::PLUGINS_DIR)
                    }
                    false => has_extension(&entry.name, ".nro"),
                };
                if !content {
                    continue;
                }
                let name = match entry.is_dir {
                    true => entry.name.to_ascii_lowercase(),
                    false => entry.name.clone(),
                };
                fs::rename(&path::join(request.source, &entry.name), &path::join(&target, &name))
                    .map_err(|e| e.to_string())?;
                moved += 1;
            }
            if moved == 0 {
                return Err(format!("{} holds no romfs or exefs folder", request.source));
            }
        }
        ModKind::Romfs => {
            fs::rename(request.source, &path::join(&target, "romfs")).map_err(|e| e.to_string())?;
        }
    }

    let ini_path = path::join(&target, mods::MOD_INI);
    let mut info = request.info;
    if let Some(thumbnail) = request.thumbnail.filter(|t| fs::is_file(t)) {
        let extension = match path::file_name(thumbnail).rsplit_once('.') {
            Some((_, extension)) if extension.len() <= 4 => extension.to_ascii_lowercase(),
            _ => "jpg".into(),
        };
        let name = format!("thumbnail.{}", extension);
        let destination = path::join(&target, &name);
        if fs::exists(&destination) {
            let _ = fs::remove_file(&destination);
        }
        if fs::rename(thumbnail, &destination).is_ok() {
            info.thumbnail = name;
        }
    }
    // Settings of a mod being updated survive, new metadata wins.
    if fs::is_file(&ini_path) {
        let previous = ModInfo::load(&ini_path);
        info.enabled = previous.enabled;
        info.priority = previous.priority;
        info.load_plugins = previous.load_plugins;
        if info.options.is_empty() {
            info.options = previous.options;
        }
        if info.thumbnail.is_empty() {
            info.thumbnail = previous.thumbnail;
        }
    }
    fs::write(&ini_path, info.to_ini().as_bytes()).map_err(|e| e.to_string())?;

    if !config.profile.trim().is_empty() {
        let mut active = Profile::load(config, &config.profile).unwrap_or_else(|| Profile::new(&config.profile));
        if active.entry(request.folder).is_none() {
            active.mods.insert(
                0,
                profile::ProfileMod {
                    folder: request.folder.to_string(),
                    enabled: true,
                    options: Default::default(),
                },
            );
            active.save(config).map_err(|e| e.to_string())?;
        }
    }
    Ok(target)
}

/// Deletes a mod folder and its entries in every profile.
pub fn uninstall(config: &Config, folder: &str) -> Result<(), String> {
    let target = config.mod_path(folder);
    if fs::is_dir(&target) {
        fs::remove_dir_all(&target).map_err(|e| e.to_string())?;
    }
    for name in profile::list(config) {
        if let Some(mut listed) = Profile::load(config, &name) {
            let before = listed.mods.len();
            listed.mods.retain(|m| !m.folder.eq_ignore_ascii_case(folder));
            if listed.mods.len() != before {
                listed.save(config).map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}

/// Moves mods left in the old layout (`mods/X.tkcl` + `mods/X.tkcl.ini`)
/// into folders of their own. Returns the new folder names.
pub fn migrate_loose_packages(config: &Config) -> Vec<String> {
    let mut moved = Vec::new();
    let Ok(entries) = fs::read_dir(&config.mods_dir) else {
        return moved;
    };
    for entry in entries.iter().filter(|e| !e.is_dir && has_extension(&e.name, ".tkcl")) {
        let stem = &entry.name[..entry.name.len() - 5];
        let folder = folder_name(stem);
        let target = config.mod_path(&folder);
        if fs::exists(&target) {
            continue;
        }
        if fs::create_dir_all(&target).is_err() {
            continue;
        }
        let source = path::join(&config.mods_dir, &entry.name);
        if fs::rename(&source, &path::join(&target, &entry.name)).is_err() {
            continue;
        }
        let old_ini = format!("{}.ini", source);
        if fs::is_file(&old_ini) {
            let _ = fs::rename(&old_ini, &path::join(&target, mods::MOD_INI));
        }
        moved.push(folder);
    }
    moved
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;

    #[test]
    fn folder_names_are_fat32_safe() {
        assert_eq!(folder_name("Link: Warrior of <Sunlight>?"), "Link_ Warrior of _Sunlight__");
        assert_eq!(folder_name("  ...  "), "mod");
        assert_eq!(folder_name("Weapons v1.2."), "Weapons v1.2");
    }

    #[test]
    fn finds_what_archives_hold() {
        let root = std::env::temp_dir().join(format!("totk-install-test-{}", std::process::id()));
        let root = root.to_string_lossy().replace('\\', "/");
        let _ = std::fs::remove_dir_all(&root);
        let write = |relative: &str| {
            let file = format!("{}/{}", root, relative);
            std::fs::create_dir_all(path::parent(&file).unwrap()).unwrap();
            std::fs::write(file, b"x").unwrap();
        };
        write("Mod/Option A/romfs/Pack/a.pack.zs");
        write("Mod/Option A/plugin.nro");
        write("Mod/Option B/romfs/Pack/b.pack.zs");
        write("Mod/Option B/exefs/subsdk9");
        write("Other/Pack/c.pack.zs");
        write("Packaged/Mod.tkcl");
        write("Code/plugins/hud.nro");

        let found = find_candidates(&root);
        let labels: Vec<(&str, ModKind, bool)> = found.iter().map(|c| (c.label.as_str(), c.kind, c.has_code)).collect();
        assert_eq!(
            labels,
            vec![
                ("Packaged/Mod.tkcl", ModKind::Package, false),
                // A mod that is only code is a mod too.
                ("Code", ModKind::Folder, false),
                ("Mod/Option A", ModKind::Folder, false),
                ("Mod/Option B", ModKind::Folder, true),
                ("Other", ModKind::Romfs, false),
            ]
        );

        let mut config = Config::default();
        config.mods_dir = format!("{}/mods", root);
        config.profiles_dir = format!("{}/profiles", root);
        config.profile = "Test".into();
        let mut info = ModInfo::default();
        info.name = Some("Option A".into());
        let installed = install(
            &config,
            InstallRequest {
                source: &found.iter().find(|c| c.label == "Mod/Option A").unwrap().path,
                kind: ModKind::Folder,
                folder: "Option A",
                info,
                thumbnail: None,
            },
        )
        .unwrap();
        assert!(fs::is_file(&format!("{}/romfs/Pack/a.pack.zs", installed)));
        // The plugin it ships is installed with it, and found back.
        assert!(fs::is_file(&format!("{}/plugin.nro", installed)));
        assert_eq!(mods::plugin_files(&installed), vec![format!("{}/plugin.nro", installed)]);
        assert_eq!(Profile::load(&config, "Test").unwrap().mods[0].folder, "Option A");

        uninstall(&config, "Option A").unwrap();
        assert!(!fs::exists(&installed));
        assert!(Profile::load(&config, "Test").unwrap().mods.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}
