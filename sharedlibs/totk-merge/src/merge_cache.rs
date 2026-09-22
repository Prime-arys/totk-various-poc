//! Merges kept on the SD card: going back to mods merged before (another
//! profile, a mod turned off then on again) serves the earlier result instead
//! of merging again.
//!
//! ```text
//! <merged_dir>/
//! ├── store/<xx>/<hash>-<size>   merged files, named after their content: a
//! │                              file several merges produce is written once
//! ├── <id>/                      one merge
//! │   ├── index.tsv              romfs path → file served (store or mod folder)
//! │   ├── patches.tsv            code patches
//! │   ├── plan.txt               mods and settings part of the stamp
//! │   ├── locales.txt            message archives merged ("all" or "EUfr,USen")
//! │   ├── profile.txt            the profile it was made for, for people
//! │   ├── conflicts.tsv          conflicts found when it was made
//! │   └── stamp.txt              written last: the merge is complete
//! └── recent.txt                 merge ids, most recently used first
//! ```
//!
//! Beyond the configured number of merges, the least recently used ones are
//! deleted, with the stored files no remaining merge serves.

use alloc::collections::{BTreeMap, BTreeSet};

use totk_formats::xxhash::xxh64;

use crate::config::Locales;
use crate::mods::Fingerprint;
use crate::prelude::*;
use crate::sys::{fs, path};
use crate::{debug, info};

pub const STORE_DIR: &str = "store";
pub const INDEX_FILE: &str = "index.tsv";
pub const PATCHES_FILE: &str = "patches.tsv";
pub const STAMP_FILE: &str = "stamp.txt";
pub const PLAN_FILE: &str = "plan.txt";
pub const LOCALES_FILE: &str = "locales.txt";
pub const PROFILE_FILE: &str = "profile.txt";
pub const CONFLICTS_FILE: &str = "conflicts.tsv";
const RECENT_FILE: &str = "recent.txt";

/// romfs-relative path -> absolute SD path to serve instead.
pub type Redirects = BTreeMap<String, String>;

/// A complete merge in the cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedMerge {
    pub id: String,
    /// `<plan>-<game version>-<code build id>`.
    pub stamp: String,
    pub plan: String,
    pub locales: String,
    pub profile: String,
}

/// What a cached merge serves.
#[derive(Debug, Default)]
pub struct Served {
    pub redirects: Redirects,
    pub patches: Vec<(u32, u32)>,
}

/// Whether merged message archives (`record`: "all" or "EUfr,USen") include
/// every one `wanted`.
pub fn locales_cover(record: &str, wanted: &Locales) -> bool {
    let record = record.trim();
    if record == "all" {
        return true;
    }
    match wanted {
        Locales::All => false,
        Locales::Only(list) => list.iter().all(|locale| record.split(',').any(|merged| merged == locale)),
    }
}

/// The folder name of a merge.
pub fn merge_id(stamp: &str, locales: &str) -> String {
    let mut fingerprint = Fingerprint::default();
    fingerprint.mix_str(stamp);
    fingerprint.mix_str(locales);
    fingerprint.finish()
}

pub struct MergeCache {
    dir: String,
}

impl MergeCache {
    pub fn new(dir: &str) -> MergeCache {
        MergeCache { dir: dir.to_string() }
    }

    pub fn store_dir(&self) -> String {
        path::join(&self.dir, STORE_DIR)
    }

    pub fn entry_dir(&self, id: &str) -> String {
        path::join(&self.dir, id)
    }

    fn read(&self, id: &str, file: &str) -> Option<String> {
        fs::read_to_string(&path::join(&self.entry_dir(id), file))
            .ok()
            .map(|text| text.trim().to_string())
    }

    fn recent_ids(&self) -> Vec<String> {
        fs::read_to_string(&path::join(&self.dir, RECENT_FILE))
            .unwrap_or_default()
            .lines()
            .map(|line| line.trim().to_string())
            .filter(|line| !line.is_empty())
            .collect()
    }

    fn write_recent(&self, ids: &[String]) {
        let mut text = String::new();
        for id in ids {
            text.push_str(id);
            text.push('\n');
        }
        if let Err(error) = fs::write(&path::join(&self.dir, RECENT_FILE), text.as_bytes()) {
            info!("could not update the merge cache list: {}", error);
        }
    }

    /// The folders holding a merge, complete or not.
    fn entry_folders(&self) -> Vec<String> {
        fs::read_dir(&self.dir)
            .map(|entries| {
                entries
                    .into_iter()
                    .filter(|entry| entry.is_dir && entry.name != STORE_DIR && !entry.name.eq_ignore_ascii_case("romfs"))
                    .map(|entry| entry.name)
                    .collect()
            })
            .unwrap_or_default()
    }

    fn load_entry(&self, id: &str) -> Option<CachedMerge> {
        Some(CachedMerge {
            id: id.to_string(),
            stamp: self.read(id, STAMP_FILE)?,
            plan: self.read(id, PLAN_FILE).unwrap_or_default(),
            locales: self.read(id, LOCALES_FILE).unwrap_or_default(),
            profile: self.read(id, PROFILE_FILE).unwrap_or_default(),
        })
    }

    /// Complete merges, most recently used first.
    pub fn entries(&self) -> Vec<CachedMerge> {
        let mut ids = self.recent_ids();
        // Merges missing from the list (it was lost) come last.
        for folder in self.entry_folders() {
            if !ids.contains(&folder) {
                ids.push(folder);
            }
        }
        ids.iter().filter_map(|id| self.load_entry(id)).collect()
    }

    /// The files a merge serves.
    pub fn load(&self, id: &str) -> Option<Served> {
        let mut served = Served::default();
        for line in self.read(id, INDEX_FILE)?.lines() {
            if let Some((romfs, sd)) = line.split_once('\t') {
                served.redirects.insert(romfs.to_string(), sd.to_string());
            }
        }
        for line in self.read(id, PATCHES_FILE).unwrap_or_default().lines() {
            let Some((offset, value)) = line.split_once('\t') else {
                continue;
            };
            if let (Ok(offset), Ok(value)) = (u32::from_str_radix(offset, 16), u32::from_str_radix(value, 16)) {
                served.patches.push((offset, value));
            }
        }
        Some(served)
    }

    /// Conflicts recorded with a merge.
    pub fn conflicts(&self, id: &str) -> Option<String> {
        fs::read_to_string(&path::join(&self.entry_dir(id), CONFLICTS_FILE)).ok()
    }

    /// Puts a merge first in the list.
    pub fn touch(&self, id: &str) {
        let mut ids = self.recent_ids();
        if ids.first().map(String::as_str) == Some(id) {
            return;
        }
        ids.retain(|other| other != id);
        ids.insert(0, id.to_string());
        self.write_recent(&ids);
    }

    /// Makes an empty folder for a merge, replacing whatever was there.
    pub fn begin(&self, id: &str) -> fs::Result<String> {
        let dir = self.entry_dir(id);
        if fs::is_dir(&dir) {
            fs::remove_dir_all(&dir)?;
        }
        fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// Records a merge written to the store; `stamp.txt` last.
    pub fn save(&self, merge: &CachedMerge, served: &Served, conflicts: &str) -> fs::Result<()> {
        let dir = self.entry_dir(&merge.id);
        let mut index = String::new();
        for (romfs, sd) in &served.redirects {
            index.push_str(&format!("{}\t{}\n", romfs, sd));
        }
        let mut patches = String::new();
        for (offset, value) in &served.patches {
            patches.push_str(&format!("{:08x}\t{:08x}\n", offset, value));
        }
        fs::write(&path::join(&dir, INDEX_FILE), index.as_bytes())?;
        fs::write(&path::join(&dir, PATCHES_FILE), patches.as_bytes())?;
        fs::write(&path::join(&dir, PLAN_FILE), merge.plan.as_bytes())?;
        fs::write(&path::join(&dir, LOCALES_FILE), merge.locales.as_bytes())?;
        fs::write(&path::join(&dir, PROFILE_FILE), merge.profile.as_bytes())?;
        fs::write(&path::join(&dir, CONFLICTS_FILE), conflicts.as_bytes())?;
        fs::write(&path::join(&dir, STAMP_FILE), merge.stamp.as_bytes())?;
        self.touch(&merge.id);
        Ok(())
    }

    /// Keeps the `keep` most recently used merges, deleting the others,
    /// unfinished ones, and the stored files nothing serves any more.
    pub fn evict(&self, keep: usize) {
        let keep = keep.max(1);
        let complete: Vec<String> = self.entries().into_iter().map(|merge| merge.id).collect();
        let kept: Vec<String> = complete.iter().take(keep).cloned().collect();

        let mut removed = 0;
        for folder in self.entry_folders() {
            if kept.contains(&folder) {
                continue;
            }
            match fs::remove_dir_all(&self.entry_dir(&folder)) {
                Ok(()) => removed += 1,
                Err(error) => info!("could not remove the cached merge {}: {}", folder, error),
            }
        }
        if self.recent_ids() != kept {
            self.write_recent(&kept);
        }
        if removed > 0 {
            info!("merge cache: {} older merge(s) removed, {} kept", removed, kept.len());
            self.collect_garbage(&kept);
        }
    }

    /// Deletes stored files no kept merge serves.
    fn collect_garbage(&self, kept: &[String]) {
        let store = path::normalize(&self.store_dir());
        let mut used: BTreeSet<String> = BTreeSet::new();
        for id in kept {
            for line in self.read(id, INDEX_FILE).unwrap_or_default().lines() {
                if let Some((_, sd)) = line.split_once('\t') {
                    if let Some(name) = path::strip_root(sd, &store) {
                        used.insert(name.to_string());
                    }
                }
            }
        }

        let mut deleted = 0;
        let mut freed = 0u64;
        for folder in fs::read_dir(&store).unwrap_or_default() {
            if !folder.is_dir {
                continue;
            }
            let dir = path::join(&store, &folder.name);
            for file in fs::read_dir(&dir).unwrap_or_default() {
                if file.is_dir || used.contains(&format!("{}/{}", folder.name, file.name)) {
                    continue;
                }
                if fs::remove_file(&path::join(&dir, &file.name)).is_ok() {
                    deleted += 1;
                    freed += file.len;
                }
            }
        }
        if deleted > 0 {
            info!("merge cache: {} unused file(s) deleted ({} MiB)", deleted, freed >> 20);
        }
    }

    /// The single merge older versions wrote straight into the folder.
    pub fn remove_old_layout(&self) {
        let stamp = path::join(&self.dir, STAMP_FILE);
        let romfs = path::join(&self.dir, "romfs");
        if !fs::exists(&stamp) && !fs::is_dir(&romfs) {
            return;
        }
        info!("removing the merge of an older version from {}", self.dir);
        for file in [STAMP_FILE, INDEX_FILE, PATCHES_FILE, PLAN_FILE, LOCALES_FILE] {
            let _ = fs::remove_file(&path::join(&self.dir, file));
        }
        if fs::is_dir(&romfs) {
            let _ = fs::remove_dir_all(&romfs);
        }
    }

    /// Number of merges and the size of the stored files.
    pub fn usage(&self) -> (usize, u64) {
        let mut bytes = 0;
        for folder in fs::read_dir(&self.store_dir()).unwrap_or_default() {
            if folder.is_dir {
                for file in fs::read_dir(&path::join(&self.store_dir(), &folder.name)).unwrap_or_default() {
                    bytes += file.len;
                }
            }
        }
        (self.entries().len(), bytes)
    }
}

/// Writes merged files into the store, skipping those already there.
pub struct StoreWriter {
    store: String,
    /// Store folders known to exist; `true` when this merge created them
    /// (nothing to look for in there).
    folders: BTreeMap<String, bool>,
    pub written: usize,
    pub reused: usize,
    pub bytes_written: u64,
}

impl StoreWriter {
    pub fn new(store: &str) -> StoreWriter {
        StoreWriter {
            store: path::normalize(store),
            folders: BTreeMap::new(),
            written: 0,
            reused: 0,
            bytes_written: 0,
        }
    }

    /// Stores `data`, returning the path to serve.
    pub fn put(&mut self, data: &[u8]) -> fs::Result<String> {
        let name = format!("{:016x}-{:x}", xxh64(data, 0), data.len());
        let folder = name[..2].to_string();
        let dir = path::join(&self.store, &folder);
        let file = path::join(&dir, &name);

        let fresh = match self.folders.get(&folder) {
            Some(fresh) => *fresh,
            None => {
                let fresh = !fs::is_dir(&dir);
                if fresh {
                    fs::create_dir_all(&dir)?;
                }
                self.folders.insert(folder, fresh);
                fresh
            }
        };

        // A file of that name and size holds these bytes: a merge made it
        // (an interrupted write would have left it shorter).
        if !fresh && fs::metadata(&file).map_or(false, |m| m.len == data.len() as u64) {
            self.reused += 1;
            return Ok(file);
        }
        fs::write(&file, data)?;
        self.written += 1;
        self.bytes_written += data.len() as u64;
        debug!("stored {}", name);
        Ok(file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::format;

    fn scratch(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("totk-merge-cache-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.to_string_lossy().replace('\\', "/")
    }

    fn merge(cache: &MergeCache, id: &str, files: &[(&str, &[u8])]) -> CachedMerge {
        cache.begin(id).unwrap();
        let mut writer = StoreWriter::new(&cache.store_dir());
        let mut served = Served::default();
        for (romfs, data) in files {
            served.redirects.insert(romfs.to_string(), writer.put(data).unwrap());
        }
        let merge = CachedMerge {
            id: id.into(),
            stamp: format!("stamp-{}", id),
            plan: format!("plan-{}", id),
            locales: "EUfr".into(),
            profile: "Défaut".into(),
        };
        cache.save(&merge, &served, "").unwrap();
        merge
    }

    #[test]
    fn keeps_the_most_recent_merges_and_their_files() {
        let dir = scratch("evict");
        let cache = MergeCache::new(&dir);

        let a = merge(&cache, "a", &[("Pack/A.pack.zs", b"shared"), ("Pack/OnlyA.pack.zs", b"only a")]);
        let b = merge(&cache, "b", &[("Pack/A.pack.zs", b"shared"), ("Pack/OnlyB.pack.zs", b"only b")]);
        assert_eq!(cache.entries(), vec![b.clone(), a.clone()]);
        // The shared file was written once.
        assert_eq!(cache.usage(), (2, 18));

        cache.touch("a");
        let c = merge(&cache, "c", &[("Pack/OnlyC.pack.zs", b"only c")]);
        assert_eq!(cache.entries(), vec![c.clone(), a.clone(), b.clone()]);

        cache.evict(2);
        assert_eq!(cache.entries(), vec![c, a]);
        // "only b" went with b; "shared" stays for a.
        assert_eq!(cache.usage(), (2, 18));
        let served = cache.load("a").unwrap();
        assert_eq!(std::fs::read(&served.redirects["Pack/A.pack.zs"]).unwrap(), b"shared");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unfinished_merges_are_not_served_and_go_away() {
        let dir = scratch("unfinished");
        let cache = MergeCache::new(&dir);
        merge(&cache, "done", &[("A", b"a")]);
        cache.begin("interrupted").unwrap();
        assert_eq!(cache.entries().len(), 1);
        cache.evict(10);
        assert!(!fs::is_dir(&cache.entry_dir("interrupted")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_truncated_stored_file_is_written_again() {
        let dir = scratch("truncated");
        let store = format!("{}/store", dir);
        let path = StoreWriter::new(&store).put(b"complete data").unwrap();
        std::fs::write(&path, b"compl").unwrap();
        let mut writer = StoreWriter::new(&store);
        assert_eq!(writer.put(b"complete data").unwrap(), path);
        assert_eq!((writer.written, writer.reused), (1, 0));
        assert_eq!(std::fs::read(&path).unwrap(), b"complete data");
        let mut writer = StoreWriter::new(&store);
        writer.put(b"complete data").unwrap();
        assert_eq!((writer.written, writer.reused), (0, 1));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn languages_cover_what_is_needed() {
        assert!(locales_cover("all", &Locales::All));
        assert!(locales_cover("EUfr,USen", &Locales::Only(vec!["EUfr".into()])));
        assert!(!locales_cover("EUfr", &Locales::All));
        assert!(!locales_cover("EUfr", &Locales::Only(vec!["USen".into()])));
    }
}
