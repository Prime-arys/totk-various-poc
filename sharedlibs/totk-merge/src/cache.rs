//! Changelogs worked out for folder mods, kept on the SD card.
//!
//! Working out what a folder mod changes means reading every file it ships and
//! the vanilla files they replace, which is the slow part of a merge on the
//! console. The result only depends on the mod's files and the game version,
//! so it is stored once: the changelog index in TKMM's binary layout, the
//! changelog files concatenated in one blob (one file is far faster to write
//! on an SD card than thousands), and the whole files the mod ships referenced
//! by path rather than copied.

use alloc::collections::BTreeMap;
use alloc::sync::Arc;

use hashbrown::HashMap;

use crate::builder::BuiltChangelog;
use crate::prelude::*;
use crate::sys::sync::Mutex;
use crate::sys::{fs, path};
use crate::tkcl::{Changelog, ChangelogSource};

const CHANGELOG_FILE: &str = "changelog.bin";
const DATA_FILE: &str = "data.bin";
const INDEX_FILE: &str = "index.tsv";

/// Bumped whenever the changelog builders change what they produce.
pub const BUILDER_VERSION: u32 = 1;

enum Location {
    Blob { offset: u64, size: u64 },
    File(String),
}

/// Changelog files held in memory, as a folder build leaves them.
pub struct MemorySource {
    files: BTreeMap<String, Vec<u8>>,
    links: BTreeMap<String, String>,
    label: String,
}

impl ChangelogSource for MemorySource {
    fn read(&self, relative: &str) -> Option<Vec<u8>> {
        if let Some(data) = self.files.get(relative) {
            return Some(data.clone());
        }
        fs::read(self.links.get(relative)?).ok()
    }

    fn file_path(&self, relative: &str) -> Option<String> {
        self.links.get(relative).cloned()
    }

    fn describe(&self) -> String {
        self.label.clone()
    }
}

/// Changelog files stored by [`store`].
pub struct StoredSource {
    /// The blob, opened on first use.
    data: Mutex<Option<Arc<fs::File>>>,
    data_path: String,
    index: HashMap<String, Location>,
    label: String,
}

impl StoredSource {
    fn data(&self) -> Option<Arc<fs::File>> {
        let mut data = self.data.lock();
        if data.is_none() {
            *data = Some(Arc::new(fs::File::open(&self.data_path).ok()?));
        }
        data.clone()
    }
}

impl ChangelogSource for StoredSource {
    fn read(&self, relative: &str) -> Option<Vec<u8>> {
        match self.index.get(relative)? {
            Location::File(file) => fs::read(file).ok(),
            Location::Blob { offset, size } => {
                let mut buffer = vec![0u8; *size as usize];
                let read = self.data()?.read_at(*offset, &mut buffer).ok()?;
                (read == buffer.len()).then_some(buffer)
            }
        }
    }

    fn file_path(&self, relative: &str) -> Option<String> {
        match self.index.get(relative)? {
            Location::File(file) => Some(file.clone()),
            Location::Blob { .. } => None,
        }
    }

    fn read_head(&self, relative: &str, length: usize) -> Option<Vec<u8>> {
        match self.index.get(relative)? {
            Location::File(file) => {
                let file = fs::File::open(file).ok()?;
                let mut buffer = vec![0u8; length];
                let read = file.read_at(0, &mut buffer).ok()?;
                buffer.truncate(read);
                Some(buffer)
            }
            Location::Blob { .. } => self.read(relative).map(|mut data| {
                data.truncate(length);
                data
            }),
        }
    }

    fn describe(&self) -> String {
        self.label.clone()
    }
}

impl BuiltChangelog {
    /// The changelog, reading its files from memory.
    pub fn into_changelog(self, label: &str, game_version: i32) -> Changelog {
        let source = MemorySource {
            files: self.files,
            links: self.links,
            label: label.to_string(),
        };
        Changelog {
            builder_version: 200,
            game_version,
            entries: self.entries,
            mals_files: self.mals_files,
            patches: self.patches,
            cheats: Vec::new(),
            subsdk_files: self.subsdk_files,
            exe_files: self.exe_files,
            source: Arc::new(source),
        }
    }
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('\t', "\\t").replace('\n', "\\n")
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// Writes a folder build to `dir`, replacing whatever was there.
pub fn store(dir: &str, built: &BuiltChangelog, game_version: i32) -> fs::Result<()> {
    if fs::is_dir(dir) {
        fs::remove_dir_all(dir)?;
    }
    fs::create_dir_all(dir)?;

    let mut index = String::new();
    let mut data = fs::Writer::create(&path::join(dir, DATA_FILE))?;
    let mut offset = 0u64;
    for (name, bytes) in &built.files {
        data.write_all(bytes)?;
        index.push_str(&format!("{}\tb\t{}\t{}\n", escape(name), offset, bytes.len()));
        offset += bytes.len() as u64;
    }
    data.finish()?;
    for (name, file) in &built.links {
        index.push_str(&format!("{}\tf\t{}\n", escape(name), escape(file)));
    }
    fs::write(&path::join(dir, INDEX_FILE), index.as_bytes())?;

    let index_only = Changelog {
        builder_version: 200,
        game_version,
        entries: built.entries.clone(),
        mals_files: built.mals_files.clone(),
        patches: built.patches.clone(),
        cheats: Vec::new(),
        subsdk_files: built.subsdk_files.clone(),
        exe_files: built.exe_files.clone(),
        source: Arc::new(MemorySource {
            files: BTreeMap::new(),
            links: BTreeMap::new(),
            label: String::new(),
        }),
    };
    // Written last: its presence marks a complete cache entry.
    fs::write(&path::join(dir, CHANGELOG_FILE), &index_only.write())
}

/// Reads back what [`store`] wrote.
pub fn load(dir: &str, label: &str) -> Option<Changelog> {
    let changelog = fs::read(&path::join(dir, CHANGELOG_FILE)).ok()?;
    let index_text = fs::read_to_string(&path::join(dir, INDEX_FILE)).ok()?;

    let mut index = HashMap::new();
    for line in index_text.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        match fields.as_slice() {
            [name, "b", offset, size] => {
                let (Ok(offset), Ok(size)) = (offset.parse(), size.parse()) else {
                    return None;
                };
                index.insert(unescape(name), Location::Blob { offset, size });
            }
            [name, "f", file] => {
                index.insert(unescape(name), Location::File(unescape(file)));
            }
            _ => return None,
        }
    }

    let source = StoredSource {
        data: Mutex::new(None),
        data_path: path::join(dir, DATA_FILE),
        index,
        label: label.to_string(),
    };
    Changelog::read(&changelog, Arc::new(source)).ok()
}

/// Folder name for a mod's cache entry.
pub fn entry_name(mod_name: &str, fingerprint: &str) -> String {
    let safe: String = mod_name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .take(48)
        .collect();
    format!("{}-{}", safe, fingerprint)
}

/// Deletes cache entries of `mod_name` other than `keep`.
pub fn prune(root: &str, mod_name: &str, keep: &str) {
    let prefix = entry_name(mod_name, "");
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries {
        if entry.is_dir && entry.name.starts_with(&prefix) && entry.name != keep && entry.name.len() == keep.len() {
            let _ = fs::remove_dir_all(&path::join(root, &entry.name));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tkcl::{ChangelogEntry, EntryType};

    #[test]
    fn stored_changelogs_read_back() {
        let dir = std::env::temp_dir().join(format!("totk-cache-test-{}", std::process::id()));
        let linked = dir.with_extension("linked");
        std::fs::write(&linked, b"whole file").unwrap();
        let dir = dir.to_string_lossy().replace('\\', "/");
        let linked = linked.to_string_lossy().replace('\\', "/");

        let mut built = BuiltChangelog::default();
        built.entries.push(ChangelogEntry {
            canonical: "Component/Foo.bgyml".into(),
            kind: EntryType::Changelog,
            attributes: 0,
            zs_dictionary_id: -1,
            versions: vec![],
            archive_canonicals: vec!["Pack/Actor/Foo.pack".into()],
        });
        built.files.insert("romfs/Component/Foo.bgyml".into(), b"changelog".to_vec());
        built.files.insert("romfs/Other\tname".into(), b"x".to_vec());
        built.links.insert("romfs/Model/Foo.bfres.mc".into(), linked.clone());

        store(&dir, &built, 121).unwrap();
        let changelog = load(&dir, "test").unwrap();
        assert_eq!(changelog.entries.len(), 1);
        assert_eq!(changelog.entries[0].archive_canonicals, vec!["Pack/Actor/Foo.pack".to_string()]);
        assert_eq!(changelog.source.read("romfs/Component/Foo.bgyml").unwrap(), b"changelog");
        assert_eq!(changelog.source.read("romfs/Other\tname").unwrap(), b"x");
        assert_eq!(changelog.source.read("romfs/Model/Foo.bfres.mc").unwrap(), b"whole file");
        assert!(changelog.source.file_path("romfs/Model/Foo.bfres.mc").is_some());
        assert!(changelog.source.file_path("romfs/Component/Foo.bgyml").is_none());

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&linked);
    }
}
