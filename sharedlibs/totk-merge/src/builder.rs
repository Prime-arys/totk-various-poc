//! Building changelogs from plain mod folders.
//!
//! Port of TkChangelogBuilder and the per-format changelog builders. The result
//! is the same data a `.tkcl` package holds, so folder mods and packages merge
//! through one code path.

use alloc::collections::BTreeMap;

use hashbrown::HashMap;
use totk_formats::msbt::{Duplicates, Msbt};
use totk_formats::sarc::{Sarc, SarcBuilder};

use crate::canonical::{extension, from_mod_path, Root};
use crate::prelude::*;
use crate::rom::TkRom;
use crate::sys::{fs, path};
use crate::tkcl::{ChangelogEntry, EntryType, Patch};
use crate::{byml_changelog, gamedata, info, rsdb};

pub const DELETED_MARK: &[u8; 8] = b"TKSCRMVD";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuilderKind {
    Pack,
    Bntx,
    GameData,
    RsdbTag,
    RsdbRow(&'static str),
    Msbt,
    Sarc,
    Byml,
}

/// TkChangelogBuilder.GetChangelogBuilder.
pub fn builder_for(canonical: &str) -> Option<BuilderKind> {
    let ext = extension(canonical);
    if ext == ".pack" {
        return Some(BuilderKind::Pack);
    }
    if ext == ".bntx" && canonical.ends_with("__Combined.bntx") {
        return Some(BuilderKind::Bntx);
    }
    if canonical == gamedata::CANONICAL {
        return Some(BuilderKind::GameData);
    }
    if canonical == rsdb::TAG_TABLE {
        return Some(BuilderKind::RsdbTag);
    }
    if let Some(key) = rsdb::row_key(canonical) {
        return Some(BuilderKind::RsdbRow(key));
    }
    match ext {
        ".msbt" => Some(BuilderKind::Msbt),
        ".bfarc" | ".bkres" | ".blarc" | ".genvb" | ".sarc" | ".ta" => Some(BuilderKind::Sarc),
        ".bgyml" => Some(BuilderKind::Byml),
        ".byml" if !canonical.starts_with("RSDB") && !canonical.starts_with("GameData") => Some(BuilderKind::Byml),
        _ => None,
    }
}

impl BuilderKind {
    pub fn can_process_without_vanilla(self) -> bool {
        matches!(self, BuilderKind::Pack | BuilderKind::Bntx)
    }
}

pub enum Payload<'a> {
    Data(&'a [u8]),
    Placeholder,
}

/// Where builders put what they produce (TKMM's OpenWriteChangelog).
pub trait ChangelogSink {
    /// `file_version` is the version of the path being built, `archive` the
    /// pack a nested file belongs to.
    fn write(
        &mut self,
        file_version: i32,
        canonical: &str,
        archive: Option<&str>,
        kind: EntryType,
        payload: Payload<'_>,
    ) -> Result<(), String>;
}

pub struct BuildContext<'a> {
    pub rom: &'a TkRom,
}

/// Runs a builder. Returns false when the target matches vanilla.
pub fn build(
    ctx: &BuildContext,
    kind: BuilderKind,
    canonical: &str,
    file_version: i32,
    src: &[u8],
    vanilla: Option<&[u8]>,
    sink: &mut dyn ChangelogSink,
) -> Result<bool, String> {
    let single = |output: Option<Vec<u8>>, sink: &mut dyn ChangelogSink| -> Result<bool, String> {
        match output {
            Some(data) => {
                sink.write(file_version, canonical, None, EntryType::Changelog, Payload::Data(&data))?;
                Ok(true)
            }
            None => Ok(false),
        }
    };

    match kind {
        BuilderKind::Pack => build_pack(ctx, canonical, file_version, src, vanilla, sink),
        BuilderKind::Bntx => Err("BNTX changelogs are not supported".into()),
        _ => {
            let Some(vanilla) = vanilla else {
                return Err(format!("{} needs the vanilla file to build a changelog", canonical));
            };
            let output = match kind {
                BuilderKind::GameData => gamedata::build_changelog(src, vanilla)?,
                BuilderKind::RsdbTag => rsdb::build_tag_changelog(src, vanilla)?,
                BuilderKind::RsdbRow(key) => rsdb::build_row_changelog(canonical, key, src, vanilla)?,
                BuilderKind::Byml => byml_changelog::build_document(canonical, src, vanilla)?,
                BuilderKind::Msbt => build_msbt(canonical, src, vanilla)?,
                BuilderKind::Sarc => build_sarc(ctx, canonical, file_version, src, vanilla)?,
                BuilderKind::Pack | BuilderKind::Bntx => unreachable!(),
            };
            single(output, sink)
        }
    }
}

/// MsbtChangelogBuilder.
fn build_msbt(canonical: &str, src: &[u8], vanilla: &[u8]) -> Result<Option<Vec<u8>>, String> {
    if !src.starts_with(b"MsgStdBn") {
        info!("expected an MSBT file but found invalid magic: {}", canonical);
        return Ok(None);
    }
    let vanilla = Msbt::parse(vanilla).map_err(|e| e.to_string())?;
    let src = Msbt::parse_with(src, Duplicates::KeepLast).map_err(|e| e.to_string())?;

    let mut changelog = Msbt::new(src.encoding());
    for (label, entry) in src.entries() {
        if vanilla.get(label) == Some(entry) {
            continue;
        }
        changelog.insert(label.to_string(), entry.clone());
    }
    Ok((!changelog.is_empty()).then(|| changelog.write()))
}

/// Collects nested outputs into a SARC changelog.
struct SarcSink<'a> {
    changelog: &'a mut SarcBuilder,
}

impl ChangelogSink for SarcSink<'_> {
    fn write(&mut self, _: i32, canonical: &str, _: Option<&str>, _: EntryType, payload: Payload<'_>) -> Result<(), String> {
        let data = match payload {
            Payload::Data(data) => data.to_vec(),
            Payload::Placeholder => Vec::new(),
        };
        self.changelog.insert(canonical, data);
        Ok(())
    }
}

/// SarcChangelogBuilder.
fn build_sarc(
    ctx: &BuildContext,
    _canonical: &str,
    file_version: i32,
    src: &[u8],
    vanilla: &[u8],
) -> Result<Option<Vec<u8>>, String> {
    let vanilla = Sarc::parse(vanilla).map_err(|e| e.to_string())?;
    let sarc = Sarc::parse(src).map_err(|e| e.to_string())?;
    let mut changelog = SarcBuilder::new();

    for entry in sarc.entries() {
        let name = entry.name;
        let data = entry.data;

        let nested = match vanilla.get(name) {
            None => None,
            Some(vanilla_entry) if vanilla_entry.data == data => continue,
            Some(vanilla_entry) => builder_for(name)
                .filter(|kind| !matches!(kind, BuilderKind::Pack | BuilderKind::Bntx))
                .map(|kind| (kind, vanilla_entry.data)),
        };

        match nested {
            Some((kind, vanilla_data)) => {
                let mut sink = SarcSink {
                    changelog: &mut changelog,
                };
                build(ctx, kind, name, file_version, data, Some(vanilla_data), &mut sink)?;
            }
            None => changelog.insert(name, data.to_vec()),
        }
    }

    Ok((!changelog.is_empty()).then(|| changelog.build()))
}

/// Forwards nested outputs of a pack to the parent sink, tagged with the pack.
struct PackSink<'a> {
    parent: &'a mut dyn ChangelogSink,
    archive: &'a str,
}

impl ChangelogSink for PackSink<'_> {
    fn write(
        &mut self,
        file_version: i32,
        canonical: &str,
        _: Option<&str>,
        _: EntryType,
        payload: Payload<'_>,
    ) -> Result<(), String> {
        self.parent
            .write(file_version, canonical, Some(self.archive), EntryType::Changelog, payload)
    }
}

/// PackChangelogBuilder: files inside packs become entries of their own,
/// tagged with the pack; the pack's own changelog only records deletions.
fn build_pack(
    ctx: &BuildContext,
    canonical: &str,
    file_version: i32,
    src: &[u8],
    vanilla: Option<&[u8]>,
    sink: &mut dyn ChangelogSink,
) -> Result<bool, String> {
    let sarc = Sarc::parse(src).map_err(|e| e.to_string())?;

    let Some(vanilla) = vanilla.filter(|v| !v.is_empty()) else {
        extract_custom_pack(ctx, canonical, file_version, &sarc, sink)?;
        return Ok(true);
    };

    let vanilla = Sarc::parse(vanilla).map_err(|e| e.to_string())?;
    let mut changelog = SarcBuilder::new();
    let mut has_nested_changes = false;

    for entry in sarc.entries() {
        let (name, data) = (entry.name, entry.data);
        if data == DELETED_MARK {
            return Err(format!("unexpected deletion mark for {} in {}", name, canonical));
        }

        let from_elsewhere;
        let vanilla_data: &[u8] = match vanilla.get(name) {
            Some(vanilla_entry) => vanilla_entry.data,
            None => match ctx.rom.get_vanilla(name).0 {
                Some(found) => {
                    from_elsewhere = found;
                    &from_elsewhere
                }
                None => {
                    move_content(canonical, file_version, name, data, &mut changelog, sink)?;
                    has_nested_changes = true;
                    continue;
                }
            },
        };
        let in_this_pack = vanilla.get(name).is_some();

        if data == vanilla_data {
            if in_this_pack {
                continue;
            }
            // A vanilla file this pack does not normally contain.
            sink.write(file_version, name, Some(canonical), EntryType::Placeholder, Payload::Placeholder)?;
            has_nested_changes = true;
        }

        let Some(kind) = builder_for(name).filter(|kind| *kind != BuilderKind::Bntx) else {
            move_content(canonical, file_version, name, data, &mut changelog, sink)?;
            has_nested_changes = true;
            continue;
        };

        let mut nested = PackSink { parent: sink, archive: canonical };
        if build(ctx, kind, name, file_version, data, Some(vanilla_data), &mut nested)? {
            has_nested_changes = true;
        }
    }

    for entry in vanilla.entries() {
        if sarc.get(entry.name).is_none() {
            changelog.insert(entry.name, DELETED_MARK.to_vec());
        }
    }

    if changelog.is_empty() {
        return Ok(has_nested_changes);
    }
    sink.write(file_version, canonical, None, EntryType::Changelog, Payload::Data(&changelog.build()))?;
    Ok(true)
}

fn move_content(
    archive: &str,
    file_version: i32,
    name: &str,
    data: &[u8],
    changelog: &mut SarcBuilder,
    sink: &mut dyn ChangelogSink,
) -> Result<(), String> {
    changelog.insert(name, Vec::new());
    sink.write(file_version, name, Some(archive), EntryType::Copy, Payload::Data(data))
}

/// PackChangelogBuilder.ExtractCustom: a pack the game does not have.
fn extract_custom_pack(
    ctx: &BuildContext,
    canonical: &str,
    file_version: i32,
    sarc: &Sarc,
    sink: &mut dyn ChangelogSink,
) -> Result<(), String> {
    for entry in sarc.entries() {
        let (name, data) = (entry.name, entry.data);
        let Some(vanilla) = ctx.rom.get_vanilla(name).0 else {
            sink.write(file_version, name, Some(canonical), EntryType::Copy, Payload::Data(data))?;
            continue;
        };

        if data == vanilla.as_slice() {
            sink.write(file_version, name, Some(canonical), EntryType::Placeholder, Payload::Placeholder)?;
            continue;
        }

        let Some(kind) = builder_for(name).filter(|kind| *kind != BuilderKind::Bntx) else {
            sink.write(file_version, name, Some(canonical), EntryType::Copy, Payload::Data(data))?;
            continue;
        };

        let mut nested = PackSink { parent: sink, archive: canonical };
        if !build(ctx, kind, name, file_version, data, Some(&vanilla), &mut nested)? {
            sink.write(file_version, name, Some(canonical), EntryType::Placeholder, Payload::Placeholder)?;
        }
    }
    Ok(())
}

struct CaptureSink {
    output: Option<Vec<u8>>,
}

impl ChangelogSink for CaptureSink {
    fn write(&mut self, _: i32, _: &str, _: Option<&str>, _: EntryType, payload: Payload<'_>) -> Result<(), String> {
        if let Payload::Data(data) = payload {
            self.output = Some(data.to_vec());
        }
        Ok(())
    }
}

/// TkChangelogBuilder.CreateChangelogsExternal: changelogs of full files
/// against a base that is not vanilla (a file several mods add).
pub fn create_changelogs_external(
    ctx: &BuildContext,
    canonical: &str,
    base: &[u8],
    targets: &[Vec<u8>],
) -> Result<Vec<Vec<u8>>, String> {
    let kind = builder_for(canonical)
        .filter(|kind| !matches!(kind, BuilderKind::Pack | BuilderKind::Bntx))
        .ok_or_else(|| format!("{} cannot be merged as a custom file: no changelog builder", canonical))?;

    let mut result = Vec::new();
    for target in targets {
        let mut sink = CaptureSink { output: None };
        if build(ctx, kind, canonical, 100, target, Some(base), &mut sink)? {
            if let Some(output) = sink.output {
                result.push(output);
            }
        }
    }
    Ok(result)
}

// --- folder mods ---------------------------------------------------------------

/// A changelog built from a folder, before it is stored anywhere.
#[derive(Default)]
pub struct BuiltChangelog {
    pub entries: Vec<ChangelogEntry>,
    pub mals_files: Vec<String>,
    pub patches: Vec<Patch>,
    pub subsdk_files: Vec<String>,
    pub exe_files: Vec<String>,
    /// Changelog files by path relative to the mod root ("romfs/…").
    pub files: BTreeMap<String, Vec<u8>>,
    /// Files served straight from the mod folder: relative path → file.
    pub links: BTreeMap<String, String>,
    pub skipped_cheats: usize,
}

struct FolderSink<'a> {
    built: &'a mut BuiltChangelog,
    index: &'a mut HashMap<String, usize>,
    path_attributes: u32,
    zs_dictionary_id: i32,
}

fn has_invalid_file_name(canonical: &str) -> bool {
    let name = canonical.rsplit('/').next().unwrap_or(canonical);
    name.chars()
        .any(|c| matches!(c, '<' | '>' | '|' | ':' | '*' | '?' | '"' | '\\') || (c as u32) < 32)
}

impl FolderSink<'_> {
    /// TkChangelogBuilder.AddChangelogMetadata. Returns the file name the
    /// entry's content is stored under, or `None` for message archives.
    fn add_metadata(
        &mut self,
        canonical: &str,
        kind: EntryType,
        file_version: i32,
        archive: Option<&str>,
    ) -> Option<String> {
        if canonical.len() > 4 && canonical.starts_with("Mals") {
            self.built.mals_files.push(canonical.to_string());
            return Some(canonical.to_string());
        }

        // Nested files take the attributes of the pack they are in, which are
        // those of the path being built.
        let attributes = self.path_attributes;
        let position = *self.index.entry(canonical.to_string()).or_insert_with(|| {
            self.built.entries.push(ChangelogEntry {
                canonical: canonical.to_string(),
                kind,
                attributes,
                zs_dictionary_id: self.zs_dictionary_id,
                versions: Vec::new(),
                archive_canonicals: Vec::new(),
            });
            self.built.entries.len() - 1
        });
        let entry = &mut self.built.entries[position];

        let mut stored = canonical.to_string();
        if file_version != -1 {
            entry.versions.push(file_version);
            stored.push_str(&file_version.to_string());
        }
        if let Some(archive) = archive {
            entry.archive_canonicals.push(archive.to_string());
        }
        entry.kind = kind;
        Some(stored)
    }
}

impl ChangelogSink for FolderSink<'_> {
    fn write(
        &mut self,
        file_version: i32,
        canonical: &str,
        archive: Option<&str>,
        kind: EntryType,
        payload: Payload<'_>,
    ) -> Result<(), String> {
        if has_invalid_file_name(canonical) {
            info!("'{}' was ignored: invalid characters in the file name", canonical);
            return Ok(());
        }
        if let Some(stored) = self.add_metadata(canonical, kind, file_version, archive) {
            if let Payload::Data(data) = payload {
                self.built.files.insert(format!("romfs/{}", stored), data.to_vec());
            }
        }
        Ok(())
    }
}

/// (path relative to the mod root, full path) of every file under `dir`.
fn collect_files(dir: &str, relative: &str, out: &mut Vec<(String, String)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let full = path::join(dir, &entry.name);
        let child = if relative.is_empty() {
            entry.name.clone()
        } else {
            format!("{}/{}", relative, entry.name)
        };
        if entry.is_dir {
            collect_files(&full, &child, out);
        } else {
            out.push((child, full));
        }
    }
}

/// TkChangelogBuilder.Build for a mod folder holding `romfs`/`exefs`/…, or
/// for a romfs root when `romfs_only` is set.
pub fn build_folder(rom: &TkRom, mod_root: &str, romfs_only: bool) -> BuiltChangelog {
    let ctx = BuildContext { rom };
    let mut built = BuiltChangelog::default();
    let mut index: HashMap<String, usize> = HashMap::new();

    let mut files = Vec::new();
    collect_files(mod_root, "", &mut files);
    if romfs_only {
        for (relative, _) in files.iter_mut() {
            *relative = format!("romfs/{}", relative);
        }
    }
    files.sort();

    for (relative, file) in files {
        let Some((root, path)) = from_mod_path(&relative) else {
            continue;
        };
        let canonical = path.canonical.clone();
        let ext = extension(&canonical);
        let file_string = path::normalize(&file);

        match root {
            Root::Exefs if ext == ".ips" => {
                if let Some(patch) = fs::read(&file)
                    .ok()
                    .and_then(|data| parse_ips(&data, &canonical[..canonical.len() - 4]))
                {
                    built.patches.push(patch);
                }
                continue;
            }
            Root::Exefs if ext == ".pchtxt" => {
                if let Some(patch) = fs::read_to_string(&file).ok().and_then(|text| parse_pchtxt(&text)) {
                    built.patches.push(patch);
                }
                continue;
            }
            Root::Exefs => {
                if canonical.len() == 7 && canonical.starts_with("subsdk") {
                    built.subsdk_files.push(canonical.clone());
                } else {
                    built.exe_files.push(canonical.clone());
                }
                built.links.insert(format!("exefs/{}", canonical), file_string);
                continue;
            }
            Root::Extras => continue,
            Root::Cheats => {
                built.skipped_cheats += 1;
                continue;
            }
            Root::Romfs => {}
        }

        if ext == ".rsizetable" || canonical == "desktop.ini" {
            continue;
        }

        let mut sink = FolderSink {
            built: &mut built,
            index: &mut index,
            path_attributes: path.attributes,
            zs_dictionary_id: -1,
        };

        let Some(kind) = builder_for(&canonical).filter(|kind| *kind != BuilderKind::Bntx) else {
            if let Some(stored) = sink.add_metadata(&canonical, EntryType::Copy, path.file_version, None) {
                sink.built.links.insert(format!("romfs/{}", stored), file_string);
            }
            continue;
        };

        let raw = match fs::read(&file) {
            Ok(raw) => raw,
            Err(error) => {
                info!("{}: {}", relative, error);
                continue;
            }
        };
        let zs_dictionary_id = zstd_dictionary_id(&raw);
        let data = match rom.decompress(&raw) {
            Ok(data) => data,
            Err(error) => {
                info!("{}: {}", relative, error);
                continue;
            }
        };
        sink.zs_dictionary_id = zs_dictionary_id;

        let vanilla = rom.get_vanilla_canonical(&canonical, path.attributes);
        if vanilla.is_none() && !kind.can_process_without_vanilla() {
            if let Some(stored) = sink.add_metadata(&canonical, EntryType::Copy, path.file_version, None) {
                sink.built.links.insert(format!("romfs/{}", stored), file_string);
            }
            continue;
        }

        match build(&ctx, kind, &canonical, path.file_version, &data, vanilla.as_deref(), &mut sink) {
            Ok(true) => {}
            Ok(false) => crate::debug!("{} matches vanilla", relative),
            Err(error) => {
                info!("could not work out the changes in {}: {}; the whole file will be used", relative, error);
                if let Some(stored) = sink.add_metadata(&canonical, EntryType::Copy, path.file_version, None) {
                    sink.built.links.insert(format!("romfs/{}", stored), file_string);
                }
            }
        }
    }

    built
}

/// Dictionary a zstd frame was compressed with, or -1.
pub fn zstd_dictionary_id(data: &[u8]) -> i32 {
    if data.len() < 6 || data[..4] != [0x28, 0xB5, 0x2F, 0xFD] {
        return -1;
    }
    let descriptor = data[4];
    let window = if descriptor & 0b0010_0000 != 0 { 0 } else { 1 };
    let size = [0usize, 1, 2, 4][(descriptor & 0b11) as usize];
    let start = 5 + window;
    let Some(bytes) = data.get(start..start + size) else {
        return -1;
    };
    let mut id = 0u32;
    for (i, byte) in bytes.iter().enumerate() {
        id |= (*byte as u32) << (8 * i);
    }
    if size == 0 {
        -1
    } else {
        id as i32
    }
}

const NSO_HEADER_LENGTH: u32 = 0x100;

/// TkPatch.FromIps: IPS32 patches of 4 byte values.
pub fn parse_ips(data: &[u8], nso_binary_id: &str) -> Option<Patch> {
    if data.len() < 5 || &data[..5] != b"IPS32" {
        return None;
    }
    let mut patch = Patch {
        nso_binary_id: nso_binary_id.to_string(),
        entries: Vec::new(),
    };
    let mut entries: BTreeMap<u32, u32> = BTreeMap::new();
    let mut position = 5;
    while position + 4 <= data.len() {
        let address = u32::from_be_bytes(data[position..position + 4].try_into().unwrap());
        position += 4;
        if address == 0x4545_4F46 {
            break;
        }
        let Some(size) = data.get(position..position + 2).map(|b| u16::from_be_bytes([b[0], b[1]]) as usize) else {
            break;
        };
        position += 2;
        if size == 4 {
            if let Some(value) = data.get(position..position + 4) {
                entries.insert(address.wrapping_sub(NSO_HEADER_LENGTH), u32::from_be_bytes(value.try_into().unwrap()));
            }
        }
        // TKMM reads on after a value of another size as if it were 4 bytes
        // long; stepping over the real size is the only sane reading.
        position += size;
    }
    patch.entries = entries.into_iter().collect();
    Some(patch)
}

/// TkPatch.FromPchTxt.
pub fn parse_pchtxt(text: &str) -> Option<Patch> {
    let mut lines = text.lines();
    let first = lines.next()?;
    let nso_binary_id = first.strip_prefix("@nsobid")?.trim().trim_start_matches('-').to_string();
    let mut entries: BTreeMap<u32, u32> = BTreeMap::new();
    let mut enabled = false;
    for line in lines {
        if enabled {
            if line.starts_with("@stop") {
                enabled = false;
                continue;
            }
            if line.starts_with('@') {
                continue;
            }
            let hex_prefix = |s: &str| -> (String, usize) {
                let digits: String = s.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
                let len = digits.len();
                (digits, len)
            };
            let (address, len) = hex_prefix(line);
            let rest = line.get(len + 1..).unwrap_or("");
            let (value, _) = hex_prefix(rest);
            if let (Ok(address), Ok(value)) = (u32::from_str_radix(&address, 16), u32::from_str_radix(&value, 16)) {
                entries.insert(address, value);
            }
            continue;
        }
        if line.starts_with("@enabled") {
            enabled = true;
        }
    }
    Some(Patch {
        nso_binary_id,
        entries: entries.into_iter().collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_builders_like_tkmm() {
        assert_eq!(builder_for("Pack/Actor/Foo.pack"), Some(BuilderKind::Pack));
        assert_eq!(builder_for("RSDB/ActorInfo.Product.rstbl.byml"), Some(BuilderKind::RsdbRow("__RowId")));
        assert_eq!(builder_for("RSDB/Unknown.Product.rstbl.byml"), None);
        assert_eq!(builder_for("GameData/GameDataList.Product.byml"), Some(BuilderKind::GameData));
        assert_eq!(builder_for("Banc/Foo.bcett.byml"), Some(BuilderKind::Byml));
        assert_eq!(builder_for("UI/Foo.blarc"), Some(BuilderKind::Sarc));
        assert_eq!(builder_for("Model/Foo.bfres"), None);
    }

    #[test]
    fn reads_ips32_patches() {
        let mut ips = b"IPS32".to_vec();
        ips.extend_from_slice(&0x0000_1100u32.to_be_bytes());
        ips.extend_from_slice(&4u16.to_be_bytes());
        ips.extend_from_slice(&0xD503_201Fu32.to_be_bytes());
        ips.extend_from_slice(b"EEOF");
        let patch = parse_ips(&ips, "abc").unwrap();
        assert_eq!(patch.entries, vec![(0x1000, 0xD503_201F)]);
    }

    #[test]
    fn reads_pchtxt_patches() {
        let text = "@nsobid-9B4E43650501A4D4489B4BBFDB740F26AF3CF850\n@enabled\n01acb308 E8FFBF52\n@stop\n00000000 00000000\n";
        let patch = parse_pchtxt(text).unwrap();
        assert_eq!(patch.nso_binary_id, "9B4E43650501A4D4489B4BBFDB740F26AF3CF850");
        assert_eq!(patch.entries, vec![(0x01acb308, 0xE8FFBF52)]);
    }
}
