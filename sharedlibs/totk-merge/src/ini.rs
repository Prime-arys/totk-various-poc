//! The small INI dialect of the settings files on the SD card.
//!
//! `key = value` lines, `[section]` headers (a section name may repeat: each
//! occurrence is its own section, in file order), `#` and `;` comments. Values
//! are single lines; `\n` and `\\` escapes let a description span lines.

use crate::prelude::*;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Section {
    /// Lowercased; "" for the lines before the first header.
    pub name: String,
    /// Keys keep their case (option group names are shown to users).
    pub entries: Vec<(String, String)>,
}

impl Section {
    /// The value of `key`, compared without case.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(key))
            .map(|(_, value)| value.as_str())
    }

    pub fn get_bool(&self, key: &str, fallback: bool) -> bool {
        self.get(key).map_or(fallback, |value| parse_bool(value, fallback))
    }
}

/// Every section, the unnamed one first (possibly empty).
pub fn parse(contents: &str) -> Vec<Section> {
    let mut sections = vec![Section::default()];
    for line in contents.lines() {
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            sections.push(Section {
                name: line[1..line.len() - 1].trim().to_ascii_lowercase(),
                entries: Vec::new(),
            });
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        sections
            .last_mut()
            .unwrap()
            .entries
            .push((key.trim().to_string(), unescape(value.trim())));
    }
    sections
}

pub fn parse_bool(value: &str, fallback: bool) -> bool {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        _ => fallback,
    }
}

/// A value on one line.
pub fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace("\r\n", "\n").replace('\n', "\\n")
}

pub fn unescape(value: &str) -> String {
    if !value.contains('\\') {
        return value.to_string();
    }
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('\\') => out.push('\\'),
            // Not an escape we write: keep it as it was (Windows paths).
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// `contents` with `key` set in the unnamed section: the existing line is
/// replaced (comments and everything else are kept), or a line is added before
/// the first section.
pub fn set_value(contents: &str, key: &str, value: &str) -> String {
    let line_for = |key: &str| format!("{} = {}", key, escape(value));
    let mut out = Vec::new();
    let mut done = false;
    let mut in_unnamed = true;
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if in_unnamed && !done {
                out.push(line_for(key));
                out.push(String::new());
                done = true;
            }
            in_unnamed = false;
        } else if in_unnamed && !done && !trimmed.starts_with('#') && !trimmed.starts_with(';') {
            if let Some((name, _)) = trimmed.split_once('=') {
                if name.trim().eq_ignore_ascii_case(key) {
                    out.push(line_for(name.trim()));
                    done = true;
                    continue;
                }
            }
        }
        out.push(line.to_string());
    }
    if !done {
        out.push(line_for(key));
    }
    let mut text = out.join("\n");
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_repeated_sections_in_order() {
        let sections = parse("a = 1\n[mod]\nfolder = x\n[mod]\nfolder = y\n# c\n[Options]\nGroup = A; B\n");
        assert_eq!(sections.len(), 4);
        assert_eq!(sections[0].get("A"), Some("1"));
        assert_eq!(sections[1].get("folder"), Some("x"));
        assert_eq!(sections[2].get("folder"), Some("y"));
        assert_eq!(sections[3].name, "options");
        assert_eq!(sections[3].entries[0].0, "Group");
    }

    #[test]
    fn escapes_round_trip() {
        let text = "line one\nline two with \\ and C:\\path";
        assert_eq!(unescape(&escape(text)), text);
        assert_eq!(unescape("C:\\Users"), "C:\\Users");
    }

    #[test]
    fn sets_values_in_place() {
        let original = "# settings\nenabled = 1\nprofile = Old\n\n[extra]\nprofile = untouched\n";
        let updated = set_value(original, "profile", "Nouveau");
        assert_eq!(updated, "# settings\nenabled = 1\nprofile = Nouveau\n\n[extra]\nprofile = untouched\n");
        let added = set_value("enabled = 1\n[extra]\nx = 1\n", "profile", "P");
        assert_eq!(added, "enabled = 1\nprofile = P\n\n[extra]\nx = 1\n");
        assert_eq!(set_value("", "profile", "P"), "profile = P\n");
    }
}
