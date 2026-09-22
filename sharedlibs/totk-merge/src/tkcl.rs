//! TKMM mod packages (`.tkcl`) and changelogs.
//!
//! A `.tkcl` is a small binary header describing the mod (name, author, option
//! groups...) followed by a ZIP archive of *changelog files*: for each game file
//! the mod touches, what it changes relative to vanilla. Every changelog also
//! lists those files, with the attributes needed to find the real romfs path
//! back (compression, product version, the packs a file belongs to).
//!
//! The same changelog layout is what this crate caches for plain folder mods
//! once it has worked out their changes, so both kinds merge the same way.

use alloc::collections::BTreeMap;
use alloc::sync::Arc;

use hashbrown::HashMap;
use totk_formats::zip::{FileAccess, RandomAccess, ZipIndex};

use crate::prelude::*;

const TKPK_MAGIC: u32 = 0x504D_4B54; // "TKMP"
const TKPK_VERSION: u32 = 0x20;
const TKCL_MAGIC: u32 = 0x4C43_4B54; // "TKCL"

pub type Result<T> = core::result::Result<T, String>;

pub mod attributes {
    pub const HAS_ZS_EXTENSION: u32 = 1;
    pub const HAS_MC_EXTENSION: u32 = 2;
    pub const IS_PRODUCT_FILE: u32 = 4;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryType {
    /// A changelog: the difference from vanilla, merged with the right merger.
    Changelog = 0,
    /// A whole file, used as is.
    Copy = 1,
    /// "This vanilla file belongs in these archives", with no content.
    Placeholder = 2,
}

impl EntryType {
    fn from_raw(value: i32) -> Result<EntryType> {
        match value {
            0 => Ok(EntryType::Changelog),
            1 => Ok(EntryType::Copy),
            2 => Ok(EntryType::Placeholder),
            _ => Err(format!("unknown changelog entry type {}", value)),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChangelogEntry {
    /// Game path without compression suffix or version, e.g.
    /// "RSDB/ActorInfo.Product.rstbl.byml".
    pub canonical: String,
    pub kind: EntryType,
    pub attributes: u32,
    pub zs_dictionary_id: i32,
    /// Game versions this entry has a dedicated file for (e.g. [110, 121]).
    pub versions: Vec<i32>,
    /// Packs this file lives in, when it is part of one.
    pub archive_canonicals: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Patch {
    pub nso_binary_id: String,
    /// Offset within the NSO -> replacement instruction.
    pub entries: Vec<(u32, u32)>,
}

#[derive(Debug, Clone, Default)]
pub struct Cheat {
    pub name: String,
    pub entries: Vec<(String, Vec<Vec<u32>>)>,
}

/// Where a changelog's files are read from.
pub trait ChangelogSource: Send + Sync {
    /// Reads a file relative to the mod root, e.g. "romfs/Pack/Actor/Foo.pack".
    fn read(&self, relative: &str) -> Option<Vec<u8>>;
    /// A plain file holding exactly that content, which can be handed to the
    /// game as is instead of being copied.
    fn file_path(&self, _relative: &str) -> Option<String> {
        None
    }
    /// The first bytes of a file (enough for a zstd frame header).
    fn read_head(&self, relative: &str, length: usize) -> Option<Vec<u8>> {
        self.read(relative).map(|mut data| {
            data.truncate(length);
            data
        })
    }
    /// A short description for logs.
    fn describe(&self) -> String;
}

pub struct Changelog {
    pub builder_version: i32,
    pub game_version: i32,
    pub entries: Vec<ChangelogEntry>,
    pub mals_files: Vec<String>,
    pub patches: Vec<Patch>,
    pub cheats: Vec<Cheat>,
    pub subsdk_files: Vec<String>,
    pub exe_files: Vec<String>,
    pub source: Arc<dyn ChangelogSource>,
}

impl core::fmt::Debug for Changelog {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Changelog")
            .field("entries", &self.entries.len())
            .field("mals_files", &self.mals_files)
            .field("source", &self.source.describe())
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionGroupType {
    Multi = 0,
    MultiRequired = 1,
    Single = 2,
    SingleRequired = 3,
}

#[derive(Debug)]
pub struct ModOption {
    pub id: [u8; 16],
    pub name: String,
    pub description: String,
    pub changelog: Changelog,
    pub priority: i32,
}

#[derive(Debug)]
pub struct OptionGroup {
    pub name: String,
    pub description: String,
    pub kind: OptionGroupType,
    pub priority: i32,
    pub options: Vec<ModOption>,
    pub default_selected: Vec<usize>,
}

#[derive(Debug)]
pub struct TkMod {
    pub id: [u8; 16],
    pub name: String,
    pub description: String,
    pub version: String,
    pub author: String,
    /// Path of the mod's image inside the package ("img/<ULID>"), if any.
    pub thumbnail: Option<String>,
    pub changelog: Changelog,
    pub option_groups: Vec<OptionGroup>,
}

impl TkMod {
    /// Changelogs to merge for this mod: its own, then those of the selected
    /// options (ordered by option priority, like TKMM).
    ///
    /// `selection` maps a group name to option names; groups it does not
    /// mention use the package's default selection.
    pub fn selected_changelogs(&self, selection: &BTreeMap<String, Vec<String>>) -> Vec<&Changelog> {
        let mut result = vec![&self.changelog];
        let mut chosen: Vec<&ModOption> = Vec::new();

        for group in &self.option_groups {
            let names = selection
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(&group.name))
                .map(|(_, value)| value);

            let mut picked: Vec<&ModOption> = match names {
                Some(names) => group
                    .options
                    .iter()
                    .filter(|option| names.iter().any(|name| name.eq_ignore_ascii_case(&option.name)))
                    .collect(),
                None => group.default_selected.iter().filter_map(|&i| group.options.get(i)).collect(),
            };

            if matches!(group.kind, OptionGroupType::Single | OptionGroupType::SingleRequired) {
                picked.truncate(1);
            }
            if picked.is_empty()
                && matches!(group.kind, OptionGroupType::SingleRequired | OptionGroupType::MultiRequired)
            {
                if let Some(first) = group.options.first() {
                    picked.push(first);
                }
            }
            chosen.extend(picked);
        }

        chosen.sort_by_key(|option| option.priority);
        result.extend(chosen.into_iter().map(|option| &option.changelog));
        result
    }
}

/// ULIDs, as TKMM names option folders: 26 Crockford base32 characters.
pub fn ulid_to_string(id: &[u8; 16]) -> String {
    const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let value = u128::from_be_bytes(*id);
    (0..26)
        .map(|i| ALPHABET[((value >> (5 * (25 - i))) & 31) as usize] as char)
        .collect()
}

// --- binary reading ------------------------------------------------------------

struct Cursor<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        let slice = self
            .data
            .get(self.position..self.position + count)
            .ok_or_else(|| format!("truncated mod metadata at offset {}", self.position))?;
        self.position += count;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn bool(&mut self) -> Result<bool> {
        Ok(self.u8()? != 0)
    }

    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn ulid(&mut self) -> Result<[u8; 16]> {
        Ok(self.take(16)?.try_into().unwrap())
    }

    fn count(&mut self) -> Result<usize> {
        let value = self.i32()?;
        if value < 0 || value > 10_000_000 {
            return Err(format!("implausible count {} in mod metadata", value));
        }
        Ok(value as usize)
    }

    fn string(&mut self) -> Result<String> {
        let length = self.i32()?;
        if length <= 0 {
            return Ok(String::new());
        }
        let bytes = self.take(length as usize)?;
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }

    fn string_list(&mut self) -> Result<Vec<String>> {
        let count = self.count()?;
        (0..count).map(|_| self.string()).collect()
    }

    fn thumbnail(&mut self) -> Result<Option<String>> {
        if self.bool()? {
            return Ok(Some(self.string()?));
        }
        Ok(None)
    }
}

fn read_changelog(cursor: &mut Cursor<'_>, source: Arc<dyn ChangelogSource>) -> Result<Changelog> {
    if cursor.u32()? != TKCL_MAGIC {
        return Err("invalid changelog magic".into());
    }

    let builder_version = cursor.i32()?;
    let game_version = cursor.i32()?;

    let entry_count = cursor.count()?;
    let mut entries = Vec::with_capacity(entry_count);
    for _ in 0..entry_count {
        let canonical = cursor.string()?;
        let kind = EntryType::from_raw(cursor.i32()?)?;
        let attributes = cursor.i32()? as u32;
        let zs_dictionary_id = cursor.i32()?;
        let version_count = cursor.u8()? as usize;
        let versions = (0..version_count).map(|_| cursor.i32()).collect::<Result<Vec<_>>>()?;
        let archive_canonicals = cursor.string_list()?;
        entries.push(ChangelogEntry {
            canonical,
            kind,
            attributes,
            zs_dictionary_id,
            versions,
            archive_canonicals,
        });
    }

    let mals_files = cursor.string_list()?;

    let patch_count = cursor.count()?;
    let mut patches = Vec::with_capacity(patch_count);
    for _ in 0..patch_count {
        let nso_binary_id = cursor.string()?;
        let count = cursor.count()?;
        let mut patch_entries = Vec::with_capacity(count);
        for _ in 0..count {
            patch_entries.push((cursor.u32()?, cursor.u32()?));
        }
        patches.push(Patch {
            nso_binary_id,
            entries: patch_entries,
        });
    }

    let cheat_count = cursor.count()?;
    let mut cheats = Vec::with_capacity(cheat_count);
    for _ in 0..cheat_count {
        let name = cursor.string()?;
        let key_count = cursor.count()?;
        let mut cheat_entries = Vec::with_capacity(key_count);
        for _ in 0..key_count {
            let key = cursor.string()?;
            let line_count = cursor.count()?;
            let mut lines = Vec::with_capacity(line_count);
            for _ in 0..line_count {
                let value_count = cursor.count()?;
                lines.push((0..value_count).map(|_| cursor.u32()).collect::<Result<Vec<_>>>()?);
            }
            cheat_entries.push((key, lines));
        }
        cheats.push(Cheat {
            name,
            entries: cheat_entries,
        });
    }

    let subsdk_files = cursor.string_list()?;
    let exe_files = cursor.string_list()?;
    let _reserved1 = cursor.string_list()?;
    let _reserved2 = cursor.string_list()?;

    Ok(Changelog {
        builder_version,
        game_version,
        entries,
        mals_files,
        patches,
        cheats,
        subsdk_files,
        exe_files,
        source,
    })
}

// --- sources ------------------------------------------------------------------

/// Changelog files stored in the ZIP part of a .tkcl.
pub struct ZipSource {
    access: Arc<dyn RandomAccess + Send + Sync>,
    index: Arc<ZipIndex>,
    /// "romfs/..." logical paths (lowercase) -> entry index, for this scope.
    files: HashMap<String, usize>,
    label: String,
}

impl ZipSource {
    /// `scope` is "" for the mod itself, or an option's ULID string.
    fn new(access: Arc<dyn RandomAccess + Send + Sync>, index: Arc<ZipIndex>, scope: &str, label: String) -> ZipSource {
        let prefix = if scope.is_empty() {
            String::new()
        } else {
            format!("{}/", scope.to_ascii_lowercase())
        };

        let mut files = HashMap::new();
        for (position, entry) in index.entries().iter().enumerate() {
            let lower = entry.name.to_ascii_lowercase();
            let Some(relative) = lower.strip_prefix(prefix.as_str()) else {
                continue;
            };
            // An option's files never belong to the mod's own scope.
            if prefix.is_empty() && is_ulid_folder(relative) {
                continue;
            }
            files.insert(strip_romfs_bucket(relative), position);
        }

        ZipSource {
            access,
            index,
            files,
            label,
        }
    }
}

fn is_ulid_folder(path: &str) -> bool {
    match path.split_once('/') {
        Some((first, _)) => first.len() == 26 && first.bytes().all(|b| b.is_ascii_alphanumeric()),
        None => false,
    }
}

/// "romfs/tkmm001/pack/foo" -> "romfs/pack/foo": TKMM spreads romfs files over
/// numbered buckets to stay under FAT32's per-directory file limit.
fn strip_romfs_bucket(path: &str) -> String {
    let Some(mut rest) = path.strip_prefix("romfs/") else {
        return path.to_string();
    };
    while let Some(after) = rest.strip_prefix("tkmm") {
        let digits = after.bytes().take_while(|b| b.is_ascii_digit()).count();
        if digits == 0 || after.as_bytes().get(digits) != Some(&b'/') {
            break;
        }
        rest = &after[digits + 1..];
    }
    format!("romfs/{}", rest)
}

impl ChangelogSource for ZipSource {
    fn read(&self, relative: &str) -> Option<Vec<u8>> {
        let key = strip_romfs_bucket(&relative.replace('\\', "/").to_ascii_lowercase());
        let &position = self.files.get(&key)?;
        let entry = &self.index.entries()[position];
        self.index.read(self.access.as_ref(), entry).ok()
    }

    fn describe(&self) -> String {
        self.label.clone()
    }
}

/// Changelog files in a folder: a TKMM-style cache directory.
pub struct FolderSource {
    root: String,
}

impl FolderSource {
    pub fn new(root: &str) -> FolderSource {
        FolderSource { root: root.to_string() }
    }
}

impl ChangelogSource for FolderSource {
    fn read(&self, relative: &str) -> Option<Vec<u8>> {
        crate::sys::fs::read(&crate::sys::path::join(&self.root, relative)).ok()
    }

    fn file_path(&self, relative: &str) -> Option<String> {
        let file = crate::sys::path::join(&self.root, relative);
        crate::sys::fs::is_file(&file).then(|| crate::sys::path::normalize(&file))
    }

    fn describe(&self) -> String {
        self.root.clone()
    }
}

// --- reading packages -----------------------------------------------------------

/// The image a package shows for itself, read from its archive.
pub fn read_tkcl_thumbnail(path: &str) -> Option<Vec<u8>> {
    let access = FileAccess::open(path).ok()?;
    let package = read_tkcl(Arc::new(FileAccess::open(path).ok()?), path).ok()?;
    let index = ZipIndex::parse(&access).ok()?;
    let entry = index.find(package.thumbnail.as_deref()?)?;
    index.read(&access, entry).ok()
}

/// Reads a .tkcl from the SD card without loading its content archive.
pub fn read_tkcl_file(path: &str) -> Result<TkMod> {
    let access = FileAccess::open(path).map_err(|e| e.to_string())?;
    read_tkcl(Arc::new(access), path)
}

pub fn read_tkcl(access: Arc<dyn RandomAccess + Send + Sync>, label: &str) -> Result<TkMod> {
    let index = Arc::new(ZipIndex::parse(access.as_ref()).map_err(|e| format!("{}: {}", label, e))?);
    let metadata = access
        .read_vec(0, index.base() as usize)
        .map_err(|e| format!("{}: {}", label, e))?;

    let mut cursor = Cursor {
        data: &metadata,
        position: 0,
    };

    if cursor.u32()? != TKPK_MAGIC {
        return Err(format!("{} is not a TKMM mod package", label));
    }
    let version = cursor.u32()?;
    if version != TKPK_VERSION {
        return Err(format!(
            "{} was packaged by an unsupported TKMM version (format 0x{:X})",
            label, version
        ));
    }

    let source = |scope: &str| -> Arc<dyn ChangelogSource> {
        Arc::new(ZipSource::new(access.clone(), index.clone(), scope, label.to_string()))
    };

    let id = cursor.ulid()?;
    let name = cursor.string()?;
    let description = cursor.string()?;
    let thumbnail = cursor.thumbnail()?;
    let changelog = read_changelog(&mut cursor, source(""))?;
    let mod_version = cursor.string()?;
    let author = cursor.string()?;

    let contributor_count = cursor.count()?;
    for _ in 0..contributor_count {
        cursor.string()?;
        cursor.string()?;
    }

    let group_count = cursor.count()?;
    let mut option_groups = Vec::with_capacity(group_count);
    for _ in 0..group_count {
        let group_name = cursor.string()?;
        let group_description = cursor.string()?;
        cursor.thumbnail()?;
        let kind = match cursor.i32()? {
            0 => OptionGroupType::Multi,
            1 => OptionGroupType::MultiRequired,
            2 => OptionGroupType::Single,
            _ => OptionGroupType::SingleRequired,
        };
        let _icon = cursor.string()?;
        let priority = cursor.i32()?;

        let option_count = cursor.count()?;
        let mut options = Vec::with_capacity(option_count);
        for _ in 0..option_count {
            let option_id = cursor.ulid()?;
            let option_name = cursor.string()?;
            let option_description = cursor.string()?;
            cursor.thumbnail()?;
            let option_changelog = read_changelog(&mut cursor, source(&ulid_to_string(&option_id)))?;
            let option_priority = cursor.i32()?;
            options.push(ModOption {
                id: option_id,
                name: option_name,
                description: option_description,
                changelog: option_changelog,
                priority: option_priority,
            });
        }

        let default_count = cursor.count()?;
        let default_selected = (0..default_count)
            .map(|_| cursor.i32().map(|i| i as usize))
            .collect::<Result<Vec<_>>>()?;

        let dependency_count = cursor.count()?;
        for _ in 0..dependency_count {
            cursor.string()?;
            cursor.ulid()?;
        }

        option_groups.push(OptionGroup {
            name: group_name,
            description: group_description,
            kind,
            priority,
            options,
            default_selected,
        });
    }

    Ok(TkMod {
        id,
        name,
        description,
        version: mod_version,
        author,
        thumbnail,
        changelog,
        option_groups,
    })
}

// --- writing changelogs ----------------------------------------------------------

fn write_string(out: &mut Vec<u8>, value: &str) {
    out.extend_from_slice(&(value.len() as i32).to_le_bytes());
    out.extend_from_slice(value.as_bytes());
}

fn write_string_list(out: &mut Vec<u8>, values: &[String]) {
    out.extend_from_slice(&(values.len() as i32).to_le_bytes());
    for value in values {
        write_string(out, value);
    }
}

impl Changelog {
    /// Serializes the changelog index in TKMM's layout (the files it refers to
    /// live next to it, in its source).
    pub fn write(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&TKCL_MAGIC.to_le_bytes());
        out.extend_from_slice(&self.builder_version.to_le_bytes());
        out.extend_from_slice(&self.game_version.to_le_bytes());

        out.extend_from_slice(&(self.entries.len() as i32).to_le_bytes());
        for entry in &self.entries {
            write_string(&mut out, &entry.canonical);
            out.extend_from_slice(&(entry.kind as i32).to_le_bytes());
            out.extend_from_slice(&(entry.attributes as i32).to_le_bytes());
            out.extend_from_slice(&entry.zs_dictionary_id.to_le_bytes());
            out.push(entry.versions.len() as u8);
            for version in &entry.versions {
                out.extend_from_slice(&version.to_le_bytes());
            }
            write_string_list(&mut out, &entry.archive_canonicals);
        }

        write_string_list(&mut out, &self.mals_files);

        out.extend_from_slice(&(self.patches.len() as i32).to_le_bytes());
        for patch in &self.patches {
            write_string(&mut out, &patch.nso_binary_id);
            out.extend_from_slice(&(patch.entries.len() as i32).to_le_bytes());
            for (key, value) in &patch.entries {
                out.extend_from_slice(&key.to_le_bytes());
                out.extend_from_slice(&value.to_le_bytes());
            }
        }

        out.extend_from_slice(&(self.cheats.len() as i32).to_le_bytes());
        for cheat in &self.cheats {
            write_string(&mut out, &cheat.name);
            out.extend_from_slice(&(cheat.entries.len() as i32).to_le_bytes());
            for (key, lines) in &cheat.entries {
                write_string(&mut out, key);
                out.extend_from_slice(&(lines.len() as i32).to_le_bytes());
                for line in lines {
                    out.extend_from_slice(&(line.len() as i32).to_le_bytes());
                    for value in line {
                        out.extend_from_slice(&value.to_le_bytes());
                    }
                }
            }
        }

        write_string_list(&mut out, &self.subsdk_files);
        write_string_list(&mut out, &self.exe_files);
        write_string_list(&mut out, &[]);
        write_string_list(&mut out, &[]);
        out
    }

    /// Reads a changelog index written by [`Changelog::write`].
    pub fn read(data: &[u8], source: Arc<dyn ChangelogSource>) -> Result<Changelog> {
        let mut cursor = Cursor { data, position: 0 };
        read_changelog(&mut cursor, source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ulids_use_crockford_base32() {
        // 01ARZ3NDEKTSV4RRFFQ69G5FAV is the reference ULID from the spec.
        let bytes: [u8; 16] = [
            0x01, 0x56, 0x3E, 0x3A, 0xB5, 0xD3, 0xD6, 0x76, 0x4C, 0x61, 0xEF, 0xB9, 0x93, 0x02, 0xBD, 0x5B,
        ];
        assert_eq!(ulid_to_string(&bytes), "01ARZ3NDEKTSV4RRFFQ69G5FAV");
    }

    #[test]
    fn strips_romfs_buckets() {
        assert_eq!(strip_romfs_bucket("romfs/tkmm001/pack/foo.pack"), "romfs/pack/foo.pack");
        assert_eq!(strip_romfs_bucket("romfs/pack/foo.pack"), "romfs/pack/foo.pack");
        assert_eq!(strip_romfs_bucket("romfs/tkmmx/foo"), "romfs/tkmmx/foo");
        assert_eq!(strip_romfs_bucket("exefs/main"), "exefs/main");
    }

    struct Nothing;
    impl ChangelogSource for Nothing {
        fn read(&self, _: &str) -> Option<Vec<u8>> {
            None
        }
        fn describe(&self) -> String {
            "nothing".into()
        }
    }

    #[test]
    fn changelogs_round_trip() {
        let changelog = Changelog {
            builder_version: 200,
            game_version: 121,
            entries: vec![ChangelogEntry {
                canonical: "Component/Foo.bgyml".into(),
                kind: EntryType::Changelog,
                attributes: attributes::HAS_ZS_EXTENSION,
                zs_dictionary_id: 3,
                versions: vec![110, 121],
                archive_canonicals: vec!["Pack/Actor/Foo.pack".into()],
            }],
            mals_files: vec!["Mals/USen.Product.sarc".into()],
            patches: vec![Patch {
                nso_binary_id: "abc".into(),
                entries: vec![(0x100, 0xD503201F)],
            }],
            cheats: vec![Cheat {
                name: "c".into(),
                entries: vec![("[x]".into(), vec![vec![1, 2, 3]])],
            }],
            subsdk_files: vec!["subsdk1".into()],
            exe_files: vec![],
            source: Arc::new(Nothing),
        };

        let read = Changelog::read(&changelog.write(), Arc::new(Nothing)).unwrap();
        assert_eq!(read.entries.len(), 1);
        assert_eq!(read.entries[0].versions, vec![110, 121]);
        assert_eq!(read.entries[0].archive_canonicals, vec!["Pack/Actor/Foo.pack".to_string()]);
        assert_eq!(read.mals_files, changelog.mals_files);
        assert_eq!(read.patches[0].entries, vec![(0x100, 0xD503201F)]);
        assert_eq!(read.cheats[0].entries[0].1, vec![vec![1, 2, 3]]);
        assert_eq!(read.subsdk_files, vec!["subsdk1".to_string()]);
    }
}
