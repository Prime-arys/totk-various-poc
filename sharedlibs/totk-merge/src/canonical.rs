//! TKMM's canonical file names.
//!
//! A canonical name is a romfs path stripped of what varies between game
//! versions and packaging: the compression suffix (".zs", ".mc") and the
//! product version (`Foo.Product.121.byml` → `Foo.Product.byml`,
//! `Bar.100.bfevfl` → `Bar.bfevfl`). Changelogs are keyed by it, so a mod made
//! for one game version still lines up with another.

use crate::prelude::*;
use crate::tkcl::attributes;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalPath {
    pub canonical: String,
    /// The version found in the name, or -1.
    pub file_version: i32,
    pub attributes: u32,
}

/// Port of TkPathExtensions.GetCanonical, quirks included, so names match what
/// TKMM stores in `.tkcl` files byte for byte.
pub fn get_canonical(path: &str) -> CanonicalPath {
    let chars: Vec<char> = path.chars().collect();
    let mut attributes = 0u32;
    let mut file_version = -1;

    let mut size = chars.len();
    if chars.len() >= 3 {
        let tail: String = chars[chars.len() - 3..].iter().collect();
        if tail == ".zs" {
            attributes |= self::attributes::HAS_ZS_EXTENSION;
            size -= 3;
        } else if tail == ".mc" {
            attributes |= self::attributes::HAS_MC_EXTENSION;
            size -= 3;
        }
    }

    // The span every lookahead below reads from has the length `size` had
    // before the loop started, even though `size` shrinks.
    let span_length = size;
    let mut canonical: Vec<char> = chars[..span_length].to_vec();
    let ends_with = |canonical: &[char], suffix: &str| -> bool {
        let suffix: Vec<char> = suffix.chars().collect();
        canonical.len() >= suffix.len()
            && canonical[canonical.len() - suffix.len()..]
                .iter()
                .zip(&suffix)
                .all(|(a, b)| a.eq_ignore_ascii_case(b))
    };
    let is_pchtxt = ends_with(&canonical, ".pchtxt");
    let is_txtg = canonical.len() >= 5 && canonical[canonical.len() - 5..] == ['.', 't', 'x', 't', 'g'];

    let slice_is = |canonical: &[char], start: usize, text: &str| -> bool {
        let text: Vec<char> = text.chars().collect();
        start + text.len() <= canonical.len() && canonical[start..start + text.len()] == text[..]
    };
    let parse_version = |canonical: &[char], start: usize| -> Option<i32> {
        if start + 3 > canonical.len() {
            return None;
        }
        let digits: String = canonical[start..start + 3].iter().collect();
        digits.trim().parse().ok()
    };

    let mut skipping = false;
    let mut i = 0usize;
    while i < size {
        let slot = i;
        if canonical[i] == '.' && !is_pchtxt {
            let remaining = size as isize - i as isize;
            if remaining > 2 && slice_is(&canonical, i, ".1") && !is_txtg {
                attributes |= self::attributes::IS_PRODUCT_FILE;
                size -= 4;
                if let Some(version) = parse_version(&canonical, i + 1) {
                    file_version = version;
                    skipping = true;
                }
            } else if remaining > 8 && slice_is(&canonical, i, ".Product") {
                attributes |= self::attributes::IS_PRODUCT_FILE;
                size -= 4;
                i += 8;
                file_version = parse_version(&canonical, i + 1).unwrap_or(-1);
                skipping = true;
            }
        }

        let mut value = if skipping {
            canonical.get(i + 4).copied().unwrap_or('\0')
        } else {
            canonical[slot]
        };
        if value == '\\' {
            value = '/';
        }
        canonical[slot] = value;
        i += 1;
    }

    let size = size.min(canonical.len());
    let start = if canonical.first() == Some(&'/') { 1 } else { 0 };
    CanonicalPath {
        canonical: canonical[start.min(size)..size].iter().collect(),
        file_version,
        attributes,
    }
}

/// Which part of a mod a path belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Root {
    Romfs,
    Exefs,
    Cheats,
    Extras,
}

/// Splits "romfs/Pack/Foo.pack.zs" into its root and canonical name, like
/// TkPath.FromPath. `None` for anything outside the four roots.
pub fn from_mod_path(relative: &str) -> Option<(Root, CanonicalPath)> {
    let relative = relative.replace('\\', "/");
    let relative = relative.trim_start_matches('/');
    let (first, rest) = relative.split_once('/')?;
    let root = match first.to_ascii_lowercase().as_str() {
        "romfs" => Root::Romfs,
        "exefs" => Root::Exefs,
        "cheats" => Root::Cheats,
        "extras" => Root::Extras,
        _ => return None,
    };
    if rest.is_empty() {
        return None;
    }
    Some((root, get_canonical(rest)))
}

/// The extension TKMM switches on: everything from the last dot of the file
/// name.
pub fn extension(canonical: &str) -> &str {
    let name = canonical.rsplit('/').next().unwrap_or(canonical);
    match name.rfind('.') {
        Some(index) => &name[index..],
        None => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tkcl::attributes::*;

    fn check(path: &str, canonical: &str, version: i32, attributes: u32) {
        let result = get_canonical(path);
        assert_eq!(result.canonical, canonical, "{}", path);
        assert_eq!(result.file_version, version, "{}", path);
        assert_eq!(result.attributes, attributes, "{}", path);
    }

    #[test]
    fn strips_versions_and_compression() {
        check(
            "RSDB/Tag.Product.121.rstbl.byml.zs",
            "RSDB/Tag.Product.rstbl.byml",
            121,
            HAS_ZS_EXTENSION | IS_PRODUCT_FILE,
        );
        check("Pack/Actor/Foo.pack.zs", "Pack/Actor/Foo.pack", -1, HAS_ZS_EXTENSION);
        check(
            "Event/EventFlow/Dm_ED_0004.100.bfevfl.zs",
            "Event/EventFlow/Dm_ED_0004.bfevfl",
            100,
            HAS_ZS_EXTENSION | IS_PRODUCT_FILE,
        );
        check("Model/Foo.bfres.mc", "Model/Foo.bfres", -1, HAS_MC_EXTENSION);
        check("Mals/USen.Product.121.sarc.zs", "Mals/USen.Product.sarc", 121, HAS_ZS_EXTENSION | IS_PRODUCT_FILE);
        check(
            "Component/GameParameter/Foo.game__component__GameParameterTable.bgyml",
            "Component/GameParameter/Foo.game__component__GameParameterTable.bgyml",
            -1,
            0,
        );
        check("\\Pack\\Foo.pack", "Pack/Foo.pack", -1, 0);
    }

    #[test]
    fn splits_mod_roots() {
        let (root, path) = from_mod_path("romfs/GameData/GameDataList.Product.110.byml.zs").unwrap();
        assert_eq!(root, Root::Romfs);
        assert_eq!(path.canonical, "GameData/GameDataList.Product.byml");
        assert_eq!(path.file_version, 110);
        assert!(from_mod_path("notes/readme.txt").is_none());
        assert_eq!(extension("Pack/Actor/Foo.pack"), ".pack");
        assert_eq!(extension("RSDB/Tag.Product.rstbl.byml"), ".byml");
    }
}
