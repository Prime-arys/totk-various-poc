//! Profiles: named mod lists, like TKMM's.
//!
//! `sd:/totk/profiles/<name>.ini` lists mods by folder name, the one that wins
//! conflicts first, each with whether it is enabled and which package options
//! it uses:
//!
//! ```ini
//! [mod]
//! folder = Weapons of Legend Redux
//! enabled = 1
//! option.Weapon Pack = Swords; Bows
//!
//! [mod]
//! folder = Even More Wonderful Capsules
//! enabled = 0
//! ```
//!
//! Mods installed but not listed are not merged. The active profile is the
//! `profile` setting of `config.ini`.

use alloc::collections::BTreeMap;

use crate::config::{self, Config};
use crate::ini;
use crate::prelude::*;
use crate::sys::{fs, path};

const OPTION_PREFIX: &str = "option.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileMod {
    pub folder: String,
    pub enabled: bool,
    /// Option group → selected options; groups left out use the defaults.
    pub options: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub name: String,
    /// Highest priority first.
    pub mods: Vec<ProfileMod>,
}

impl Profile {
    pub fn new(name: &str) -> Profile {
        Profile {
            name: name.to_string(),
            mods: Vec::new(),
        }
    }

    pub fn parse(name: &str, contents: &str) -> Profile {
        let mut profile = Profile::new(name);
        for section in ini::parse(contents) {
            if section.name != "mod" {
                continue;
            }
            let Some(folder) = section.get("folder").filter(|f| !f.is_empty()) else {
                continue;
            };
            if profile.mods.iter().any(|m| m.folder.eq_ignore_ascii_case(folder)) {
                continue;
            }
            profile.mods.push(ProfileMod {
                folder: folder.to_string(),
                enabled: section.get_bool("enabled", true),
                options: config::parse_options(&section.entries, OPTION_PREFIX),
            });
        }
        profile
    }

    pub fn to_ini(&self) -> String {
        let mut out = format!(
            "# Profile '{}' (totk-mod-manager)\n# Mods are listed by folder name; the first one wins conflicts.\n",
            self.name
        );
        for entry in &self.mods {
            out.push_str(&format!(
                "\n[mod]\nfolder = {}\nenabled = {}\n",
                ini::escape(&entry.folder),
                if entry.enabled { 1 } else { 0 }
            ));
            for (group, selected) in &entry.options {
                out.push_str(&format!("{}{} = {}\n", OPTION_PREFIX, group, ini::escape(&selected.join("; "))));
            }
        }
        out
    }

    pub fn load(config: &Config, name: &str) -> Option<Profile> {
        let contents = fs::read_to_string(&file_path(config, name)).ok()?;
        Some(Profile::parse(name, &contents))
    }

    pub fn save(&self, config: &Config) -> fs::Result<()> {
        fs::create_dir_all(&config.profiles_dir)?;
        let file = file_path(config, &self.name);
        // Written next to the real file and moved over it, so a profile is
        // never left half written.
        let temporary = format!("{}.tmp", file);
        fs::write(&temporary, self.to_ini().as_bytes())?;
        if fs::exists(&file) {
            fs::remove_file(&file)?;
        }
        fs::rename(&temporary, &file)
    }

    pub fn entry(&self, folder: &str) -> Option<&ProfileMod> {
        self.mods.iter().find(|m| m.folder.eq_ignore_ascii_case(folder))
    }
}

pub fn file_path(config: &Config, name: &str) -> String {
    path::join(&config.profiles_dir, &format!("{}.ini", name))
}

/// Profile names, sorted.
pub fn list(config: &Config) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(&config.profiles_dir)
        .map(|entries| {
            entries
                .into_iter()
                .filter(|entry| !entry.is_dir && entry.name.len() > 4 && entry.name.to_ascii_lowercase().ends_with(".ini"))
                .map(|entry| entry.name[..entry.name.len() - 4].to_string())
                .collect()
        })
        .unwrap_or_default();
    names.sort_by_key(|name| name.to_lowercase());
    names
}

/// Whether `name` can be a profile file name on a FAT32/exFAT SD card.
pub fn is_valid_name(name: &str) -> bool {
    let trimmed = name.trim();
    !trimmed.is_empty()
        && trimmed == name
        && name.len() <= 64
        && !name.ends_with('.')
        && !name.chars().any(|c| c.is_control() || "\\/:*?\"<>|".contains(c))
}

pub fn delete(config: &Config, name: &str) -> fs::Result<()> {
    fs::remove_file(&file_path(config, name))
}

pub fn rename(config: &Config, from: &str, to: &str) -> fs::Result<()> {
    let mut profile = Profile::load(config, from).ok_or_else(|| fs::Error(format!("no profile '{}'", from)))?;
    profile.name = to.to_string();
    profile.save(config)?;
    if !from.eq_ignore_ascii_case(to) {
        delete(config, from)?;
    }
    if config.profile == from {
        config::set_value(config::CONFIG_PATH, "profile", to)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_round_trip() {
        let text = "[mod]\nfolder = Weapons of Legend = Redux\nenabled = 1\noption.Weapon Pack = Swords; Bows\n\n[mod]\nfolder = Capsules\nenabled = 0\n[mod]\nfolder = capsules\n";
        let profile = Profile::parse("Défaut", text);
        assert_eq!(profile.mods.len(), 2);
        assert_eq!(profile.mods[0].folder, "Weapons of Legend = Redux");
        assert_eq!(profile.mods[0].options["Weapon Pack"], vec!["Swords".to_string(), "Bows".to_string()]);
        assert!(!profile.mods[1].enabled);
        assert_eq!(Profile::parse("Défaut", &profile.to_ini()), profile);
    }

    #[test]
    fn validates_names() {
        assert!(is_valid_name("Défaut"));
        assert!(is_valid_name("En ligne 2"));
        assert!(!is_valid_name(""));
        assert!(!is_valid_name(" x"));
        assert!(!is_valid_name("a/b"));
        assert!(!is_valid_name("what?"));
    }
}
