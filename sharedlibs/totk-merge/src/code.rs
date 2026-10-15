//! Executable modules that mods ship the way TKMM packages them.
//!
//! A TKMM package (or a folder mod) can hold `exefs/subsdkN`: a module the
//! system loader maps next to the game when it starts. That is how exlaunch
//! mods run, and what TKMM calls a mod's code. TKMM writes those modules to
//! its merged output as `exefs/subsdk1`, `subsdk2`… for Atmosphère to load.
//!
//! Nothing can be done better than the loader here. Such a module looks for
//! itself among the modules *the process was created with*, so it cannot be
//! mapped into a game that is already running the way a Skyline plugin is.
//! What this does instead is keep the title's exefs folder in step with the
//! mods being merged, the way TKMM lays it out:
//!
//! ```text
//! sd:/atmosphere/contents/<title>/exefs/
//! ├── subsdk9               skyline-totk — never touched
//! ├── main.npdm             its permissions, which cover what modules need
//! ├── subsdk1 … subsdk8     the modules of the merged mods, the mod that
//! │                         wins conflicts first
//! └── totk-mod-merger.txt   which of those files were put there from here
//! ```
//!
//! Only the modules of mods are ever replaced or removed: the list says which
//! files those are, and a file that no longer matches it (someone put another
//! one there) is left alone, as is any `subsdk` that was there before. The
//! one exception is a file that holds, byte for byte, the module of a mod
//! being merged: it is that mod's module whoever copied it, and is listed
//! rather than installed a second time (a module loaded twice hooks twice).
//!
//! The loader reads that folder when the game starts, so a change made while
//! the game runs (the plugin merging at boot) only shows the next time it is
//! started. Applying the mods from the mod manager does it beforehand.
//!
//! What mods ship besides modules is not installed: `main.npdm` because
//! Skyline's already grants everything, and `main`, `rtld` or `sdk` because
//! replacing the game's own code is not something to do behind anyone's back.

use alloc::collections::BTreeMap;

use crate::merge_cache::{content_name, StoreWriter};
use crate::prelude::*;
use crate::sys::{fs, path};
use crate::tkcl::Changelog;
use crate::{debug, info};

/// The module names mods can be given. `subsdk0` is the game's own in some
/// versions, and `subsdk9` is skyline-totk.
pub const SLOTS: [&str; 8] = [
    "subsdk1", "subsdk2", "subsdk3", "subsdk4", "subsdk5", "subsdk6", "subsdk7", "subsdk8",
];

/// The list of installed modules, in the exefs folder itself: it has to go
/// wherever those files go, or they could never be told from someone else's.
pub const MANIFEST_FILE: &str = "totk-mod-merger.txt";

/// A module a merge wants loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeFile {
    /// The file in the merge store, named after its content.
    pub file: String,
    /// What the mod called it (`subsdk9`).
    pub name: String,
    /// The mod it comes from, for logs and for the list.
    pub source: String,
}

impl CodeFile {
    /// What identifies the content: the store's file name.
    pub fn id(&self) -> &str {
        path::file_name(&self.file)
    }
}

/// What the mods of a merge ship as code.
#[derive(Debug, Default)]
pub struct Collected {
    /// The mod that wins conflicts first.
    pub files: Vec<CodeFile>,
    pub warnings: usize,
}

fn is_module(data: &[u8]) -> bool {
    data.len() >= 0x100 && &data[..4] == b"NSO0"
}

/// Stores the modules that mods ship, the way TkMerger.MergeSubSdk goes
/// through them: highest priority first, each content once. `changelogs`
/// pairs each changelog with the name of its mod, lowest priority first.
pub fn collect(changelogs: &[(&str, &Changelog)], store: &mut StoreWriter) -> Collected {
    fn warn(collected: &mut Collected, message: String) {
        info!("{}", message);
        collected.warnings += 1;
    }

    let mut collected = Collected::default();
    for (source, changelog) in changelogs.iter().rev() {
        for name in &changelog.subsdk_files {
            let Some(data) = changelog.source.read(&format!("exefs/{}", name)) else {
                warn(&mut collected, format!("{}: exefs/{} is listed but missing", source, name));
                continue;
            };
            if !is_module(&data) {
                warn(
                    &mut collected,
                    format!("{}: exefs/{} is not a module (no NSO header), left out", source, name),
                );
                continue;
            }
            let file = match store.put(&data) {
                Ok(file) => file,
                Err(error) => {
                    warn(&mut collected, format!("{}: could not store exefs/{}: {}", source, name, error));
                    continue;
                }
            };
            if collected.files.iter().any(|other| other.id() == path::file_name(&file)) {
                debug!("{}: exefs/{} is the same module as another mod's, kept once", source, name);
                continue;
            }
            collected.files.push(CodeFile {
                file,
                name: name.clone(),
                source: source.to_string(),
            });
        }

        for name in &changelog.exe_files {
            if name.eq_ignore_ascii_case("main.npdm") {
                debug!("{}: its main.npdm is not installed, skyline-totk's is used", source);
            } else {
                warn(
                    &mut collected,
                    format!(
                        "{}: exefs/{} is not installed — of a mod's exefs, only the subsdk modules and the .ips/.pchtxt patches are used",
                        source, name
                    ),
                );
            }
        }
    }
    collected
}

/// What [`install`] left in the exefs folder.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Installation {
    /// Modules in place for the next time the game starts.
    pub installed: usize,
    /// Files were written or removed: a game that is already running still
    /// has the modules it was started with.
    pub changed: bool,
    /// Modules that found no free name, or could not be written.
    pub left_out: usize,
}

/// One line of the list: a file this module wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Owned {
    id: String,
    name: String,
    source: String,
}

fn read_manifest(exefs_dir: &str) -> BTreeMap<String, Owned> {
    let mut owned = BTreeMap::new();
    let Ok(text) = fs::read_to_string(&path::join(exefs_dir, MANIFEST_FILE)) else {
        return owned;
    };
    for line in text.lines() {
        if line.starts_with('#') {
            continue;
        }
        let mut fields = line.split('\t');
        let (Some(slot), Some(id)) = (fields.next(), fields.next()) else {
            continue;
        };
        let slot = slot.trim().to_ascii_lowercase();
        if !SLOTS.contains(&slot.as_str()) || id.trim().is_empty() {
            continue;
        }
        owned.insert(
            slot,
            Owned {
                id: id.trim().to_string(),
                name: fields.next().unwrap_or("").trim().to_string(),
                source: fields.next().unwrap_or("").trim().to_string(),
            },
        );
    }
    owned
}

fn manifest_text(owned: &BTreeMap<String, Owned>) -> String {
    let mut text = String::from(
        "# Modules of the mods being merged, put here by totk-mod-merger so that the\n\
         # game loads them when it starts. They follow the mods: turning a mod off\n\
         # removes its module. Files not listed here are never touched.\n\
         # name in this folder, content, name in the mod, mod\n",
    );
    for (slot, entry) in owned {
        // A tab or a line break in a mod's name would break the line in two.
        let clean = |field: &str| field.replace(['\t', '\n', '\r'], " ");
        text.push_str(&format!("{}\t{}\t{}\t{}\n", slot, entry.id, clean(&entry.name), clean(&entry.source)));
    }
    text
}

/// Size of the content a store name stands for (`<hash>-<size in hex>`).
fn size_of(id: &str) -> Option<u64> {
    u64::from_str_radix(id.rsplit_once('-')?.1, 16).ok()
}

/// Makes the modules of mods in `exefs_dir` exactly `wanted` (the mod that
/// wins conflicts first), touching only the files listed as put there by a
/// previous call. With nothing wanted and nothing installed, the folder is
/// not even created.
pub fn install(exefs_dir: &str, wanted: &[CodeFile]) -> Installation {
    let mut result = Installation::default();
    let mut owned = read_manifest(exefs_dir);
    let had_manifest = fs::is_file(&path::join(exefs_dir, MANIFEST_FILE));
    let recorded = owned.clone();
    if wanted.is_empty() && owned.is_empty() {
        if had_manifest {
            let _ = fs::remove_file(&path::join(exefs_dir, MANIFEST_FILE));
        }
        return result;
    }

    // What is there: a listed file that still holds what was written is a
    // mod's, and so is one that holds a wanted module (the list was lost, or
    // someone copied the same file by hand). Any other file under one of the
    // names belongs to someone else.
    let present = fs::read_dir(exefs_dir).unwrap_or_default();
    let mut free: Vec<&str> = Vec::new();
    for slot in SLOTS {
        let Some(file) = present.iter().find(|entry| !entry.is_dir && entry.name.eq_ignore_ascii_case(slot)) else {
            owned.remove(slot);
            free.push(slot);
            continue;
        };
        let listed = owned.get(slot).map(|entry| entry.id.clone());
        // Reading it only tells something when its size is one of a module
        // it could be.
        let could_be = listed
            .iter()
            .map(String::as_str)
            .chain(wanted.iter().map(CodeFile::id))
            .any(|id| size_of(id) == Some(file.len));
        let actual = match could_be {
            true => fs::read(&path::join(exefs_dir, &file.name)).ok().map(|data| content_name(&data)),
            false => None,
        };
        match actual {
            Some(id) if listed.as_deref() == Some(id.as_str()) => {}
            Some(id) if wanted.iter().any(|file| file.id() == id) => {
                let file = wanted.iter().find(|file| file.id() == id).expect("found above");
                debug!("code: {} in {} is {} of '{}', listed", slot, exefs_dir, file.name, file.source);
                owned.insert(
                    slot.to_string(),
                    Owned {
                        id,
                        name: file.name.clone(),
                        source: file.source.clone(),
                    },
                );
            }
            _ => {
                if owned.remove(slot).is_some() {
                    info!("code: {} in {} is no longer the module put there, left alone", slot, exefs_dir);
                } else {
                    debug!("code: {} in {} belongs to something else, left alone", slot, exefs_dir);
                }
            }
        }
    }

    // No more modules than there are names for them; the mods that lose
    // conflicts are the ones left out.
    let room = free.len() + owned.len();
    let mut kept: Vec<&CodeFile> = Vec::new();
    for file in wanted {
        if kept.iter().any(|other| other.id() == file.id()) {
            continue;
        }
        if kept.len() == room {
            info!(
                "code: no name left in {} for {} of '{}' ({} are taken), left out",
                exefs_dir,
                file.name,
                file.source,
                SLOTS.len() - room + kept.len()
            );
            result.left_out += 1;
            continue;
        }
        kept.push(file);
    }

    // Modules whose mod is gone make room first, and so does a second copy of
    // a module: loaded twice, it would hook the game twice.
    let mut seen: Vec<&str> = Vec::new();
    let mut stale: Vec<String> = Vec::new();
    for (slot, entry) in &owned {
        if !kept.iter().any(|file| file.id() == entry.id) || seen.contains(&entry.id.as_str()) {
            stale.push(slot.clone());
        } else {
            seen.push(&entry.id);
        }
    }
    for slot in stale {
        let entry = owned.remove(&slot).expect("listed above");
        match fs::remove_file(&path::join(exefs_dir, &slot)) {
            Ok(()) => {
                match kept.iter().any(|file| file.id() == entry.id) {
                    true => info!("code: {} removed (a second copy of {} of '{}')", slot, entry.name, entry.source),
                    false => info!("code: {} removed (the module of '{}' is no longer wanted)", slot, entry.source),
                }
                result.changed = true;
                if let Some(name) = SLOTS.iter().find(|name| **name == slot) {
                    free.push(*name);
                }
            }
            Err(error) => {
                // Still there, still ours: the next call tries again.
                info!("code: could not remove {} from {}: {}", slot, exefs_dir, error);
                owned.insert(slot, entry);
            }
        }
    }
    free.sort();

    for file in kept {
        if let Some((_, entry)) = owned.iter_mut().find(|(_, entry)| entry.id == file.id()) {
            // Already in place; the mod may have been renamed since.
            entry.name = file.name.clone();
            entry.source = file.source.clone();
            continue;
        }
        if free.is_empty() {
            // A stale module could not be removed.
            result.left_out += 1;
            continue;
        }
        let slot = free.remove(0);
        let target = path::join(exefs_dir, slot);
        let written = fs::read(&file.file)
            .and_then(|data| fs::create_dir_all(exefs_dir).map(|()| data))
            .and_then(|data| fs::write(&target, &data).map(|()| data.len()));
        match written {
            Ok(size) => {
                info!(
                    "code: {} of '{}' installed as {} ({} KiB)",
                    file.name,
                    file.source,
                    slot,
                    (size + 1023) / 1024
                );
                owned.insert(
                    slot.to_string(),
                    Owned {
                        id: file.id().to_string(),
                        name: file.name.clone(),
                        source: file.source.clone(),
                    },
                );
                result.changed = true;
            }
            Err(error) => {
                info!("code: could not install {} of '{}' as {}: {}", file.name, file.source, target, error);
                // Half a module must not be loaded.
                let _ = fs::remove_file(&target);
                result.left_out += 1;
            }
        }
    }

    result.installed = owned.len();
    let manifest = path::join(exefs_dir, MANIFEST_FILE);
    if owned.is_empty() {
        if had_manifest {
            let _ = fs::remove_file(&manifest);
        }
    } else if owned != recorded || !had_manifest {
        if let Err(error) = fs::write(&manifest, manifest_text(&owned).as_bytes()) {
            info!("code: could not write {}: {}", manifest, error);
        }
    }
    result
}

/// The modules listed as installed in `exefs_dir`: (name there, name in the
/// mod, mod).
pub fn installed(exefs_dir: &str) -> Vec<(String, String, String)> {
    read_manifest(exefs_dir)
        .into_iter()
        .filter(|(slot, _)| fs::is_file(&path::join(exefs_dir, slot)))
        .map(|(slot, entry)| (slot, entry.name, entry.source))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::format;

    fn scratch(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("totk-merge-code-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.to_string_lossy().replace('\\', "/")
    }

    /// A module of `size` bytes whose content depends on `seed`.
    fn module(seed: u8, size: usize) -> Vec<u8> {
        let mut data = vec![seed; size.max(0x100)];
        data[..4].copy_from_slice(b"NSO0");
        data
    }

    fn stored(store: &mut StoreWriter, seed: u8, source: &str) -> CodeFile {
        CodeFile {
            file: store.put(&module(seed, 0x180 + seed as usize)).unwrap(),
            name: "subsdk9".into(),
            source: source.into(),
        }
    }

    fn names(dir: &str) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .map(|entries| entries.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect())
            .unwrap_or_default();
        names.sort();
        names
    }

    #[test]
    fn follows_the_mods() {
        let root = scratch("follows");
        let exefs = format!("{}/exefs", root);
        let mut store = StoreWriter::new(&format!("{}/store", root));
        let (a, b, c) = (stored(&mut store, 1, "A"), stored(&mut store, 2, "B"), stored(&mut store, 3, "C"));

        // Nothing wanted, nothing there: not even a folder.
        assert_eq!(install(&exefs, &[]), Installation::default());
        assert!(!std::path::Path::new(&exefs).exists());

        let first = install(&exefs, &[a.clone(), b.clone()]);
        assert_eq!((first.installed, first.changed, first.left_out), (2, true, 0));
        assert_eq!(names(&exefs), ["subsdk1", "subsdk2", MANIFEST_FILE]);
        assert_eq!(std::fs::read(format!("{}/subsdk1", exefs)).unwrap(), module(1, 0x181));
        assert_eq!(
            installed(&exefs),
            [
                ("subsdk1".to_string(), "subsdk9".to_string(), "A".to_string()),
                ("subsdk2".to_string(), "subsdk9".to_string(), "B".to_string())
            ]
        );

        // The same again writes nothing.
        let again = install(&exefs, &[a.clone(), b.clone()]);
        assert_eq!((again.installed, again.changed), (2, false));

        // A goes, C comes: B stays where it is, C takes the name A left.
        let swapped = install(&exefs, &[c.clone(), b.clone()]);
        assert_eq!((swapped.installed, swapped.changed), (2, true));
        assert_eq!(std::fs::read(format!("{}/subsdk1", exefs)).unwrap(), module(3, 0x183));
        assert_eq!(std::fs::read(format!("{}/subsdk2", exefs)).unwrap(), module(2, 0x182));

        // No mod with code any more: the folder is as it was found.
        let none = install(&exefs, &[]);
        assert_eq!((none.installed, none.changed), (0, true));
        assert!(names(&exefs).is_empty());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn leaves_other_files_alone() {
        let root = scratch("others");
        let exefs = format!("{}/exefs", root);
        std::fs::create_dir_all(&exefs).unwrap();
        // Skyline, and a module someone installed by hand.
        std::fs::write(format!("{}/subsdk9", exefs), module(9, 0x400)).unwrap();
        std::fs::write(format!("{}/main.npdm", exefs), b"npdm").unwrap();
        std::fs::write(format!("{}/subsdk1", exefs), module(7, 0x300)).unwrap();

        let mut store = StoreWriter::new(&format!("{}/store", root));
        let (a, b) = (stored(&mut store, 1, "A"), stored(&mut store, 2, "B"));
        let result = install(&exefs, &[a.clone(), b.clone()]);
        assert_eq!((result.installed, result.left_out), (2, 0));
        // The first free names, not the taken one.
        assert_eq!(std::fs::read(format!("{}/subsdk1", exefs)).unwrap(), module(7, 0x300));
        assert_eq!(std::fs::read(format!("{}/subsdk2", exefs)).unwrap(), module(1, 0x181));
        assert_eq!(std::fs::read(format!("{}/subsdk3", exefs)).unwrap(), module(2, 0x182));

        // Someone replaces one of ours: it is theirs from then on.
        std::fs::write(format!("{}/subsdk2", exefs), module(8, 0x181)).unwrap();
        let result = install(&exefs, &[]);
        assert!(result.changed);
        assert_eq!(names(&exefs), ["main.npdm", "subsdk1", "subsdk2", "subsdk9"]);
        assert_eq!(std::fs::read(format!("{}/subsdk2", exefs)).unwrap(), module(8, 0x181));
        assert_eq!(std::fs::read(format!("{}/subsdk9", exefs)).unwrap(), module(9, 0x400));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_mods_that_win_conflicts_get_the_names() {
        let root = scratch("room");
        let exefs = format!("{}/exefs", root);
        let mut store = StoreWriter::new(&format!("{}/store", root));
        let all: Vec<CodeFile> = (1..=10).map(|seed| stored(&mut store, seed, &format!("mod {}", seed))).collect();

        // Ten modules, eight names: the last two (lowest priority) wait.
        let result = install(&exefs, &all);
        assert_eq!((result.installed, result.left_out), (8, 2));
        assert_eq!(std::fs::read(format!("{}/subsdk8", exefs)).unwrap(), module(8, 0x188));

        // The lowest priority one becomes the highest: it takes the place of
        // the one that now comes ninth.
        let mut reordered = vec![all[9].clone()];
        reordered.extend(all[..9].iter().cloned());
        let result = install(&exefs, &reordered);
        assert_eq!((result.installed, result.left_out, result.changed), (8, 2, true));
        assert_eq!(std::fs::read(format!("{}/subsdk8", exefs)).unwrap(), module(10, 0x18a));
        assert_eq!(std::fs::read(format!("{}/subsdk1", exefs)).unwrap(), module(1, 0x181));

        // The same module from two mods is installed once.
        let twice = [all[0].clone(), CodeFile { source: "copy".into(), ..all[0].clone() }];
        let result = install(&exefs, &twice);
        assert_eq!((result.installed, result.left_out), (1, 0));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_lost_list_is_made_again() {
        let root = scratch("lost");
        let exefs = format!("{}/exefs", root);
        let mut store = StoreWriter::new(&format!("{}/store", root));
        let (a, b) = (stored(&mut store, 1, "A"), stored(&mut store, 2, "B"));
        install(&exefs, &[a.clone(), b.clone()]);

        // Without the list, a file holding a wanted module is still that
        // module: it is listed again, not installed a second time.
        std::fs::remove_file(format!("{}/{}", exefs, MANIFEST_FILE)).unwrap();
        let result = install(&exefs, &[a.clone(), b.clone()]);
        assert_eq!((result.installed, result.changed), (2, false));
        assert_eq!(names(&exefs), ["subsdk1", "subsdk2", MANIFEST_FILE]);

        // Two copies of one module: one goes.
        std::fs::copy(format!("{}/subsdk1", exefs), format!("{}/subsdk5", exefs)).unwrap();
        let result = install(&exefs, &[a.clone(), b.clone()]);
        assert_eq!((result.installed, result.changed), (2, true));
        assert_eq!(names(&exefs), ["subsdk1", "subsdk2", MANIFEST_FILE]);

        // And they still follow their mod.
        let result = install(&exefs, &[b.clone()]);
        assert_eq!((result.installed, result.changed), (1, true));
        assert_eq!(names(&exefs), ["subsdk2", MANIFEST_FILE]);

        let _ = std::fs::remove_dir_all(&root);
    }
}
