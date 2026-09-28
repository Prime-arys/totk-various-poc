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
    /// Code patches shipped for Atmosphère (`atmosphere/exefs_patches/<name>/`):
    /// `path` is that folder, and its `.ips`/`.pchtxt` files become the mod's
    /// `exefs/` — the layout TKMM and the merger read.
    pub patches: bool,
}

const ROMFS_FOLDERS: [&str; 8] = ["Pack", "RSDB", "GameData", "Mals", "Banc", "Component", "UI", "Model"];

fn has_extension(name: &str, extension: &str) -> bool {
    name.len() > extension.len() && name[name.len() - extension.len()..].eq_ignore_ascii_case(extension)
}

fn folder_size(dir: &str) -> u64 {
    mods::list_files(dir).iter().map(|(_, size)| size).sum()
}

/// Whether a TKMM package ships exefs code — `subsdk*`, `main.npdm`, in the mod
/// itself or in one of its options. TKMM hands those to Atmosphère, which
/// runs them in place of the game's own exefs; here that exefs is Skyline's,
/// so they cannot run, and whoever installs the mod should know before
/// wondering why that part does nothing. Only the package's metadata is read.
fn package_has_code(file: &str) -> bool {
    let Ok(package) = totk_merge::tkcl::read_tkcl_file(file) else {
        return false;
    };
    let ships_code = |changelog: &totk_merge::tkcl::Changelog| {
        !changelog.subsdk_files.is_empty() || !changelog.exe_files.is_empty()
    };
    ships_code(&package.changelog)
        || package
            .option_groups
            .iter()
            .flat_map(|group| group.options.iter())
            .any(|option| ships_code(&option.changelog))
}

/// Code files a merger cannot use: anything in exefs but patches.
fn has_code_files(dir: &str) -> bool {
    mods::list_files(dir)
        .iter()
        .any(|(file, _)| !has_extension(file, ".ips") && !has_extension(file, ".pchtxt") && !file.starts_with('.'))
}

/// The folder called `name` in `dir`, whatever its case: the SD card does not
/// care, and archives write `Romfs` as often as `romfs`.
fn child_dir(dir: &str, name: &str) -> Option<String> {
    fs::read_dir(dir)
        .ok()?
        .into_iter()
        .find(|entry| entry.is_dir && entry.name.eq_ignore_ascii_case(name))
        .map(|entry| path::join(dir, &entry.name))
}

/// What an installed mod is, as the interface names it. Finer than the way it
/// is merged ([`ModKind`]), which says nothing of what a folder holds:
///
/// | type      | the mod holds                                                  |
/// |-----------|----------------------------------------------------------------|
/// | `package` | a TKMM `.tkcl`                                                 |
/// | `romfs`   | a romfs root, its game folders right in the mod folder         |
/// | `folder`  | game files in `romfs/`, maybe with patches or plugins as well  |
/// | `patches` | code patches only (`exefs/*.ips`, or Atmosphère's layout)      |
/// | `code`    | Skyline plugins only (`plugin.nro`)                            |
/// | `exefs`   | exefs executables only (`subsdk*`, `main.npdm`): they would    |
/// |           | replace skyline-totk, so the merger ignores them, and the mod  |
/// |           | does nothing at all — which the list should make plain         |
///
/// `content` is where the mod's files are (the mod folder, or the folder one
/// level down it was unpacked into), `plugins` whether it ships any.
pub fn mod_type(kind: ModKind, content: &str, plugins: bool) -> &'static str {
    match kind {
        ModKind::Package => "package",
        ModKind::Romfs => "romfs",
        ModKind::Folder => {
            if child_dir(content, "romfs").is_some() {
                return "folder";
            }
            let (patches, executables) = match child_dir(content, "exefs") {
                Some(exefs) => {
                    let files = mods::list_files(&exefs);
                    let is_patch = |file: &str| has_extension(file, ".ips") || has_extension(file, ".pchtxt");
                    (
                        files.iter().any(|(file, _)| is_patch(file)),
                        files.iter().any(|(file, _)| !is_patch(file) && !file.starts_with('.')),
                    )
                }
                None => (false, false),
            };
            if patches || mods::has_exefs_patches(content) {
                "patches"
            } else if plugins {
                "code"
            } else if executables {
                "exefs"
            } else {
                "folder"
            }
        }
    }
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
        let package = path::join(dir, &entry.name);
        packages.push(Candidate {
            kind: ModKind::Package,
            has_code: package_has_code(&package),
            path: package,
            label: path::join(&label, &entry.name),
            size: entry.len,
            patches: false,
        });
    }

    // Code patches laid out for Atmosphère: `exefs_patches/<name>/<build
    // id>.ips`, usually under `atmosphere/`. Each named folder is one patch
    // (several are variants, as for a mod's options), holding one file per
    // game version it supports; the merger picks the one for the game.
    if let Some(patches_dir) = entries.iter().find(|e| e.is_dir && e.name.eq_ignore_ascii_case("exefs_patches")) {
        let patches_dir = path::join(dir, &patches_dir.name);
        let mut named = fs::read_dir(&patches_dir).unwrap_or_default();
        named.sort_by(|a, b| a.name.cmp(&b.name));
        for patch in named.iter().filter(|e| e.is_dir) {
            let folder = path::join(&patches_dir, &patch.name);
            let files = patch_files(&folder);
            if files.is_empty() {
                continue;
            }
            folders.push(Candidate {
                kind: ModKind::Folder,
                path: folder.clone(),
                label: path::strip_root(&folder, root).unwrap_or("").to_string(),
                has_code: false,
                size: files.iter().map(|(_, size)| size).sum(),
                patches: true,
            });
        }
        // No return: the same archive may ship a romfs next to its patches
        // (`atmosphere/contents/<title>/romfs`), found further down.
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
            patches: false,
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
            patches: false,
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
            patches: false,
        });
        return;
    }
    // exefs_patches was read above; nothing else in it is a mod.
    for entry in entries.iter().filter(|e| e.is_dir && !e.name.eq_ignore_ascii_case("exefs_patches")) {
        walk(root, &path::join(dir, &entry.name), depth + 1, packages, folders);
    }
}

/// The `.ips` and `.pchtxt` files directly in `dir`, with their sizes: what
/// Atmosphère would read from an `exefs_patches/<name>` folder.
fn patch_files(dir: &str) -> Vec<(String, u64)> {
    let mut files: Vec<(String, u64)> = fs::read_dir(dir)
        .unwrap_or_default()
        .into_iter()
        .filter(|e| !e.is_dir && (has_extension(&e.name, ".ips") || has_extension(&e.name, ".pchtxt")))
        .map(|e| (e.name, e.len))
        .collect();
    files.sort();
    files
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
    /// `source` is an Atmosphère patch folder (see [`Candidate::patches`]).
    pub patches: bool,
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
            // A previous version copied by hand may be in Atmosphère's own
            // layout (atmosphere/exefs_patches): its patches go too, or they
            // would be applied next to the new ones.
            let is_content = if entry.is_dir {
                entry.name.eq_ignore_ascii_case("romfs")
                    || entry.name.eq_ignore_ascii_case("exefs")
                    || entry.name.eq_ignore_ascii_case(mods::PLUGINS_DIR)
                    || entry.name.eq_ignore_ascii_case("atmosphere")
                    || entry.name.eq_ignore_ascii_case("exefs_patches")
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
        // Code patches laid out for Atmosphère become the mod's exefs/, where
        // TKMM puts them and where they are looked for first.
        ModKind::Folder if request.patches => {
            let exefs = path::join(&target, "exefs");
            fs::create_dir_all(&exefs).map_err(|e| e.to_string())?;
            let files = patch_files(request.source);
            if files.is_empty() {
                return Err(format!("{} holds no .ips or .pchtxt patch", request.source));
            }
            for (name, _) in files {
                fs::rename(&path::join(request.source, &name), &path::join(&exefs, &name))
                    .map_err(|e| e.to_string())?;
            }
        }
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
                patches: false,
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

    #[test]
    fn installs_patches_laid_out_for_atmosphere() {
        let root = std::env::temp_dir().join(format!("totk-install-patches-{}", std::process::id()));
        let root = root.to_string_lossy().replace('\\', "/");
        let _ = std::fs::remove_dir_all(&root);
        let write = |relative: &str| {
            let file = format!("{}/{}", root, relative);
            std::fs::create_dir_all(path::parent(&file).unwrap()).unwrap();
            std::fs::write(file, b"IPS32EEOF").unwrap();
        };
        // ultrahand42_switch.zip, unpacked; plus a second patch in the same
        // archive (a variant), a romfs shipped next to them, and a stray file.
        write("archive/atmosphere/exefs_patches/Ultrahand42/9B4E43650501A4D4.ips");
        write("archive/atmosphere/exefs_patches/Ultrahand42/6265F94D606242CE.ips");
        write("archive/atmosphere/exefs_patches/Ultrahand63/9B4E43650501A4D4.ips");
        write("archive/atmosphere/exefs_patches/Empty/readme.txt");
        write("archive/atmosphere/contents/0100F2C0115B6000/romfs/Pack/a.pack.zs");

        let found = find_candidates(&format!("{}/archive", root));
        let labels: Vec<(&str, ModKind, bool)> = found.iter().map(|c| (c.label.as_str(), c.kind, c.patches)).collect();
        assert_eq!(
            labels,
            vec![
                ("atmosphere/contents/0100F2C0115B6000", ModKind::Folder, false),
                ("atmosphere/exefs_patches/Ultrahand42", ModKind::Folder, true),
                ("atmosphere/exefs_patches/Ultrahand63", ModKind::Folder, true),
            ]
        );

        let mut config = Config::default();
        config.mods_dir = format!("{}/mods", root);
        config.profiles_dir = format!("{}/profiles", root);
        config.profile = "Test".into();
        let mut info = ModInfo::default();
        info.name = Some("Ultrahand 42".into());
        let installed = install(
            &config,
            InstallRequest {
                source: &found.iter().find(|c| c.label.ends_with("Ultrahand42")).unwrap().path,
                kind: ModKind::Folder,
                patches: true,
                folder: "Ultrahand 42",
                info,
                thumbnail: None,
            },
        )
        .unwrap();
        // Both game versions' patches, where TKMM and the merger read them.
        assert!(fs::is_file(&format!("{}/exefs/9B4E43650501A4D4.ips", installed)));
        assert!(fs::is_file(&format!("{}/exefs/6265F94D606242CE.ips", installed)));
        // Found back as a mod, and put in the active profile.
        assert_eq!(mods::inspect_folder(&installed).map(|(kind, _)| kind), Some(ModKind::Folder));
        assert_eq!(Profile::load(&config, "Test").unwrap().mods[0].folder, "Ultrahand 42");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn names_every_kind_of_mod() {
        let root = std::env::temp_dir().join(format!("totk-mod-types-{}", std::process::id()));
        let root = root.to_string_lossy().replace('\\', "/");
        let _ = std::fs::remove_dir_all(&root);
        let write = |relative: &str| {
            let file = format!("{}/mods/{}", root, relative);
            std::fs::create_dir_all(path::parent(&file).unwrap()).unwrap();
            std::fs::write(file, b"x").unwrap();
        };
        write("Package/Package.tkcl");
        write("Files/romfs/Pack/a.pack.zs");
        write("Files and patches/romfs/Pack/a.pack.zs");
        write("Files and patches/exefs/9B4E43650501A4D4.ips");
        write("Files with code/Romfs/Pack/a.pack.zs");
        write("Files with code/plugin.nro");
        write("Unpacked/Some Mod v2/romfs/Pack/a.pack.zs");
        write("Game folders/Pack/a.pack.zs");
        write("Patches/exefs/9B4E43650501A4D4.ips");
        write("Pchtxt/exefs/main.pchtxt");
        write("Atmosphere layout/atmosphere/exefs_patches/Ultrahand42/9B4E43650501A4D4.ips");
        write("Plugin/plugin.nro");
        write("Plugins folder/plugins/hud.nro");
        write("Patches and plugin/exefs/9B4E43650501A4D4.ips");
        write("Patches and plugin/plugin.nro");
        write("Exefs code/exefs/subsdk9");
        write("Exefs code/exefs/main.npdm");
        write("Exefs code and plugin/exefs/subsdk9");
        write("Exefs code and plugin/plugin.nro");

        let mut config = Config::default();
        config.mods_dir = format!("{}/mods", root);
        let (installed, problems) = mods::scan(&config);
        assert!(problems.is_empty(), "{:?}", problems);
        let types: Vec<(&str, &str)> = installed
            .iter()
            .map(|m| (m.folder.as_str(), mod_type(m.kind, &m.content, !m.plugins.is_empty())))
            .collect();
        assert_eq!(
            types,
            vec![
                ("Atmosphere layout", "patches"),
                // It does nothing: the list has to say so.
                ("Exefs code", "exefs"),
                // Only its plugin runs.
                ("Exefs code and plugin", "code"),
                ("Files", "folder"),
                ("Files and patches", "folder"),
                ("Files with code", "folder"),
                ("Game folders", "romfs"),
                ("Package", "package"),
                ("Patches", "patches"),
                ("Patches and plugin", "patches"),
                ("Pchtxt", "patches"),
                ("Plugin", "code"),
                ("Plugins folder", "code"),
                ("Unpacked", "folder"),
            ]
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}

