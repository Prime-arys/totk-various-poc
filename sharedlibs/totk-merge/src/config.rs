//! Settings, read from `sd:/totk/config.ini`.
//!
//! Every setting has a default, so the file is optional. The mod manager
//! homebrew edits the same file (see [`set_value`]).

use crate::ini;
use crate::prelude::*;
use crate::sys::{fs, path};

#[derive(Clone)]
pub struct Config {
    /// Where mods live: one folder per mod, holding `romfs`/`exefs` or a `.tkcl`.
    pub mods_dir: String,
    /// Mod lists: `<name>.ini` files.
    pub profiles_dir: String,
    /// The profile to merge; empty (or missing) merges every mod in `mods_dir`.
    pub profile: String,
    /// Where merges are kept (see [`crate::merge_cache`]).
    pub merged_dir: String,
    /// How many merges to keep, most recently used first.
    pub merge_cache_size: usize,
    /// Changelogs worked out for folder mods, kept between boots.
    pub cache_dir: String,
    /// Log file, or empty to only log through Skyline.
    pub log_path: String,
    /// Merge again even if the cache looks current.
    pub force_merge: bool,
    /// When the mods changed since the last merge: merge while the game boots
    /// (true), or keep serving the last merge until the manager applies the
    /// changes (false).
    pub merge_at_boot: bool,
    /// Master switch.
    pub enabled: bool,
    /// Log the first few files the game reads from the merged output.
    pub log_redirects: bool,
    /// Log details of the merge.
    pub verbose: bool,
    /// Also use what TKMM exported for RomFSlite, as the lowest priority mod.
    pub use_romfslite: bool,
    pub romfslite_dir: String,
    /// Message archive locales to merge: "auto" (the one the game was last
    /// seen reading, see [`LOCALE_PATH`]), "all", or a list ("USen,EUfr").
    pub locales: String,
    /// Apply code patches (.ips/.pchtxt from mods and TKMM's defaults).
    pub apply_patches: bool,
    /// Load the Skyline plugins mods ship (see `mods::plugin_files`).
    pub mod_plugins: bool,
    /// TKMM raises the shop parameter limit to this on every merge (0: off).
    pub shop_param_limit: u32,
    /// How long to wait for a plugin that took control of the mod list.
    pub control_timeout_ms: u32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            mods_dir: "sd:/totk/mods".to_string(),
            profiles_dir: "sd:/totk/profiles".to_string(),
            profile: String::new(),
            merged_dir: "sd:/totk/merged".to_string(),
            merge_cache_size: 10,
            cache_dir: "sd:/totk/cache".to_string(),
            log_path: "sd:/totk/merger.log".to_string(),
            force_merge: false,
            merge_at_boot: true,
            enabled: true,
            log_redirects: false,
            verbose: false,
            use_romfslite: true,
            romfslite_dir: format!("sd:/atmosphere/contents/{}/romfslite", TITLE_ID),
            locales: "auto".to_string(),
            apply_patches: true,
            mod_plugins: true,
            shop_param_limit: 512,
            control_timeout_ms: 60_000,
        }
    }
}

pub const TITLE_ID: &str = "0100F2C0115B6000";
pub const CONFIG_PATH: &str = "sd:/totk/config.ini";
/// The message archive locale the game last opened (e.g. "EUfr"), written by
/// the plugin: TotK reads one language only, which spares merging the others.
pub const LOCALE_PATH: &str = "sd:/totk/locale.txt";

/// Which message archives a merge needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Locales {
    All,
    Only(Vec<String>),
}

impl Config {
    pub fn load() -> Config {
        Config::load_from(CONFIG_PATH)
    }

    pub fn load_from(file: &str) -> Config {
        match fs::read_to_string(file) {
            Ok(contents) => Config::parse(&contents),
            Err(_) => Config::default(),
        }
    }

    pub fn parse(contents: &str) -> Config {
        let mut config = Config::default();
        let sections = ini::parse(contents);
        for (key, value) in &sections[0].entries {
            let value = value.clone();
            let flag = |current: bool| ini::parse_bool(&value, current);
            match key.to_ascii_lowercase().as_str() {
                "mods_dir" => config.mods_dir = value,
                "profiles_dir" => config.profiles_dir = value,
                "profile" => config.profile = value,
                "merged_dir" => config.merged_dir = value,
                "merge_cache_size" => {
                    config.merge_cache_size = value.parse::<usize>().map(|n| n.clamp(1, 100)).unwrap_or(config.merge_cache_size)
                }
                "cache_dir" => config.cache_dir = value,
                "log_path" => config.log_path = value,
                "force_merge" => config.force_merge = flag(config.force_merge),
                "merge_at_boot" => config.merge_at_boot = flag(config.merge_at_boot),
                "enabled" => config.enabled = flag(config.enabled),
                "log_redirects" => config.log_redirects = flag(config.log_redirects),
                "verbose" => config.verbose = flag(config.verbose),
                "use_romfslite" => config.use_romfslite = flag(config.use_romfslite),
                "romfslite_dir" => config.romfslite_dir = value,
                "locales" => config.locales = value,
                "apply_patches" => config.apply_patches = flag(config.apply_patches),
                "mod_plugins" => config.mod_plugins = flag(config.mod_plugins),
                "shop_param_limit" => config.shop_param_limit = value.parse().unwrap_or(config.shop_param_limit),
                "control_timeout_ms" => {
                    config.control_timeout_ms = value.parse().unwrap_or(config.control_timeout_ms)
                }
                _ => {}
            }
        }
        config
    }

    /// The locales setting, with "auto" resolved through [`LOCALE_PATH`]
    /// (every locale until the game has been seen reading one).
    pub fn locales(&self) -> Locales {
        let setting = self.locales.trim();
        if setting.eq_ignore_ascii_case("auto") {
            return match detected_locale() {
                Some(locale) => Locales::Only(vec![locale]),
                None => Locales::All,
            };
        }
        if setting.is_empty() || setting.eq_ignore_ascii_case("all") {
            return Locales::All;
        }
        let list: Vec<String> = setting
            .split(',')
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
        if list.is_empty() {
            Locales::All
        } else {
            Locales::Only(list)
        }
    }

    /// The locales setting resolved against what the game ships.
    pub fn locale_list(&self, available: &[String]) -> Vec<String> {
        match self.locales() {
            Locales::All => available.to_vec(),
            Locales::Only(list) => {
                let known: Vec<String> = list.into_iter().filter(|l| available.iter().any(|a| a == l)).collect();
                // A locale this game does not ship (a typo, another version)
                // must not leave every text unmerged.
                if known.is_empty() {
                    available.to_vec()
                } else {
                    known
                }
            }
        }
    }

    /// `sd:/totk/mods/<folder>`.
    pub fn mod_path(&self, folder: &str) -> String {
        path::join(&self.mods_dir, folder)
    }
}

/// The locale recorded in [`LOCALE_PATH`], when it looks like one.
pub fn detected_locale() -> Option<String> {
    let text = fs::read_to_string(LOCALE_PATH).ok()?;
    let locale = text.trim();
    (locale.len() == 4 && locale.chars().all(|c| c.is_ascii_alphabetic())).then(|| locale.to_string())
}

/// "EUfr" from a romfs path such as "content:/Mals/EUfr.Product.121.sarc.zs".
pub fn locale_of_message_archive(path: &str) -> Option<&str> {
    let start = path.find("Mals/")? + 5;
    let name = path.get(start..)?;
    let locale = name.get(..4)?;
    (name.get(4..12) == Some(".Product") && name.ends_with(".sarc.zs") && locale.chars().all(|c| c.is_ascii_alphabetic()))
        .then_some(locale)
}

/// Changes one setting in a config file, keeping the rest of it as it is.
pub fn set_value(file: &str, key: &str, value: &str) -> fs::Result<()> {
    let contents = fs::read_to_string(file).unwrap_or_default();
    if let Some(parent) = path::parent(file) {
        fs::create_dir_all(parent)?;
    }
    fs::write(file, ini::set_value(&contents, key, value).as_bytes())
}

/// Parses `Group = A; B` option selections.
pub fn parse_options(entries: &[(String, String)], prefix: &str) -> alloc::collections::BTreeMap<String, Vec<String>> {
    let mut options = alloc::collections::BTreeMap::new();
    for (key, value) in entries {
        let Some(group) = strip_prefix_ignore_case(key, prefix) else {
            continue;
        };
        let selected: Vec<String> = value
            .split(';')
            .map(|option| option.trim().to_string())
            .filter(|option| !option.is_empty())
            .collect();
        options.insert(group.trim().to_string(), selected);
    }
    options
}

fn strip_prefix_ignore_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    if text.len() >= prefix.len() && text.is_char_boundary(prefix.len()) && text[..prefix.len()].eq_ignore_ascii_case(prefix) {
        Some(&text[prefix.len()..])
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_settings() {
        let config = Config::parse("profile = Défaut\nmerge_at_boot = 0\nLOCALES = EUfr\n[other]\nprofile = x\n");
        assert_eq!(config.profile, "Défaut");
        assert!(!config.merge_at_boot);
        assert_eq!(config.locales, "EUfr");
    }

    #[test]
    fn resolves_locales() {
        let available = vec!["EUfr".to_string(), "USen".to_string()];
        let mut config = Config::default();
        config.locales = "all".into();
        assert_eq!(config.locale_list(&available), available);
        config.locales = "USen, JPja".into();
        assert_eq!(config.locale_list(&available), vec!["USen".to_string()]);
        config.locales = "KRko".into();
        assert_eq!(config.locale_list(&available), available);
    }

    #[test]
    fn recognizes_message_archives() {
        assert_eq!(locale_of_message_archive("content:/Mals/EUfr.Product.121.sarc.zs"), Some("EUfr"));
        assert_eq!(locale_of_message_archive("Mals/USen.Product.110.sarc.zs"), Some("USen"));
        assert_eq!(locale_of_message_archive("content:/Pack/Actor/Mals.pack.zs"), None);
        assert_eq!(locale_of_message_archive("content:/Mals/EUfr.Product.121.sarc"), None);
    }

    #[test]
    fn reads_option_selections() {
        let entries = vec![
            ("option.Weapon Pack".to_string(), "Swords; Bows".to_string()),
            ("folder".to_string(), "x".to_string()),
            ("Option.Color".to_string(), "Red".to_string()),
        ];
        let options = parse_options(&entries, "option.");
        assert_eq!(options["Weapon Pack"], vec!["Swords".to_string(), "Bows".to_string()]);
        assert_eq!(options["Color"], vec!["Red".to_string()]);
        assert_eq!(options.len(), 2);
    }
}
