//! Turning a modded BYML document into a changelog against vanilla.
//!
//! Port of BymlChangelogBuilder.LogChangesInline and the array changelog
//! builders. The changelog is built in place: keys equal to vanilla are
//! dropped from maps, keys missing from the mod become "remove" markers, and
//! arrays become ArrayChangelog nodes.

use alloc::collections::VecDeque;

use totk_formats::byml::{ArrayChange, Byml, ChangeType, VecMap};

use crate::byml_keys::{self, create_index_cache, ArrayBuilder, KeyName};
use crate::debug;
use crate::prelude::*;

/// BymlTrackingInfo: the document's bgyml type and the current map depth.
#[derive(Debug, Clone, Default)]
pub struct TrackingInfo {
    pub bgyml_type: String,
    pub depth: i32,
}

impl TrackingInfo {
    pub fn for_canonical(canonical: &str) -> TrackingInfo {
        TrackingInfo {
            bgyml_type: byml_keys::bgyml_type(canonical),
            depth: 0,
        }
    }
}

/// Picks the array changelog builder for a named array inside a map.
pub type ArrayBuilderProvider = fn(&TrackingInfo, &str) -> ArrayBuilder;

pub fn default_provider(info: &TrackingInfo, name: &str) -> ArrayBuilder {
    byml_keys::array_builder(name, &info.bgyml_type, info.depth)
}

/// Replaces `src` with its changelog against `vanilla`. Returns true when the
/// two are equal (nothing to record).
pub fn log_changes_inline(info: &mut TrackingInfo, src: &mut Byml, vanilla: &Byml) -> bool {
    log_changes_with(info, src, vanilla, default_provider)
}

pub fn log_changes_with(
    info: &mut TrackingInfo,
    src: &mut Byml,
    vanilla: &Byml,
    provider: ArrayBuilderProvider,
) -> bool {
    if src.node_type() != vanilla.node_type() {
        return false;
    }

    match (src, vanilla) {
        (Byml::HashMap32(src), Byml::HashMap32(vanilla)) => log_map_changes(info, src, vanilla, provider, |_| None),
        (Byml::HashMap64(src), Byml::HashMap64(vanilla)) => log_map_changes(info, src, vanilla, provider, |_| None),
        (Byml::Map(src), Byml::Map(vanilla)) => log_map_changes(info, src, vanilla, provider, |k| Some(k.as_str())),
        (node @ Byml::Array(_), Byml::Array(vanilla)) => {
            let builder = if info.bgyml_type == "ecocat" && info.depth == 0 {
                ArrayBuilder::Keyed(KeyName {
                    primary: "AreaNumber",
                    secondary: None,
                })
            } else {
                ArrayBuilder::Default
            };
            log_array_changes(builder, info, node, vanilla)
        }
        (Byml::ArrayChangelog(_), _) | (Byml::Changelog(_), _) => false,
        (src, vanilla) => src.value_eq(vanilla),
    }
}

fn log_map_changes<K: Ord + Clone>(
    info: &mut TrackingInfo,
    src: &mut VecMap<K, Byml>,
    vanilla: &VecMap<K, Byml>,
    provider: ArrayBuilderProvider,
    name_of: impl Fn(&K) -> Option<&str>,
) -> bool {
    info.depth += 1;

    let mut keys: Vec<K> = src.keys().cloned().collect();
    keys.extend(vanilla.keys().filter(|key| !src.contains_key(key)).cloned());

    for key in keys {
        let Some(mut value) = src.remove(&key) else {
            src.insert(key, Byml::Changelog(ChangeType::Remove));
            continue;
        };
        let Some(vanilla_value) = vanilla.get(&key) else {
            src.insert(key, value);
            continue;
        };

        let is_vanilla = match (name_of(&key), &value, vanilla_value) {
            (Some(name), Byml::Array(_), Byml::Array(vanilla_array)) => {
                log_array_changes(provider(info, name), info, &mut value, vanilla_array)
            }
            // Nested values always use the default provider, like TKMM.
            _ => log_changes_inline(info, &mut value, vanilla_value),
        };

        if !is_vanilla {
            src.insert(key, value);
        }
    }

    info.depth -= 1;
    src.is_empty()
}

/// Runs one of the array changelog builders on `root` (an array node), which
/// becomes an ArrayChangelog. Returns true when nothing changed.
pub fn log_array_changes(builder: ArrayBuilder, info: &mut TrackingInfo, root: &mut Byml, vanilla: &[Byml]) -> bool {
    let src = match core::mem::take(root) {
        Byml::Array(items) => items,
        other => {
            *root = other;
            return false;
        }
    };

    let changelog = match builder {
        ArrayBuilder::Default => default_array_changes(info, src, vanilla),
        ArrayBuilder::Keyed(key_name) => keyed_array_changes(info, &key_name, src, vanilla),
        ArrayBuilder::NameHash => name_hash_array_changes(info, src, vanilla),
        ArrayBuilder::DirectIndex => direct_index_array_changes(info, src, vanilla),
    };

    let unchanged = changelog.is_empty();
    *root = Byml::ArrayChangelog(changelog);
    unchanged
}

fn change(index: usize, kind: ChangeType, node: Byml) -> ArrayChange {
    ArrayChange {
        index: index as i32,
        change: kind,
        node,
        key_primary: None,
        key_secondary: None,
    }
}

/// BymlArrayChangelogBuilder: entries matched by value.
fn default_array_changes(info: &mut TrackingInfo, src: Vec<Byml>, vanilla: &[Byml]) -> Vec<ArrayChange> {
    let mut changelog = Vec::new();
    let mut additions: VecDeque<usize> = VecDeque::new();
    let mut vanilla_found = vec![false; vanilla.len()];
    let mut src_is_vanilla = vec![false; src.len()];

    for (i, element) in src.iter().enumerate() {
        let found = vanilla
            .iter()
            .enumerate()
            .position(|(j, candidate)| !vanilla_found[j] && candidate.value_eq(element));
        match found {
            Some(j) => {
                src_is_vanilla[i] = true;
                vanilla_found[j] = true;
            }
            None => additions.push_back(i),
        }
    }

    for i in 0..vanilla.len() {
        if vanilla_found[i] {
            continue;
        }
        if i < src.len() && !src_is_vanilla[i] {
            let mut element = src[i].clone();
            log_changes_inline(info, &mut element, &vanilla[i]);
            changelog.push(change(i, ChangeType::Edit, element));
            additions.pop_front();
            continue;
        }
        changelog.push(change(i, ChangeType::Remove, Byml::Null));
    }

    // Every addition goes where the first one was found.
    if let Some(&index) = additions.front() {
        for i in additions {
            changelog.push(change(index, ChangeType::Add, src[i].clone()));
        }
    }

    changelog
}

/// BymlKeyedArrayChangelogBuilder: entries matched by key field(s).
fn keyed_array_changes(info: &mut TrackingInfo, key_name: &KeyName, src: Vec<Byml>, vanilla: &[Byml]) -> Vec<ArrayChange> {
    let mut changelog = Vec::new();
    let mut detected_additions = 0usize;
    let vanilla_index = create_index_cache(vanilla, key_name);
    let mut vanilla_found = vec![false; vanilla.len()];

    for (i, mut node) in src.into_iter().enumerate() {
        let Some(key) = key_name.try_get_key(&node) else {
            debug!(
                "entry {} in '{}' is missing its {} field",
                i, info.bgyml_type, key_name.primary
            );
            changelog.push(change(i - detected_additions, ChangeType::Add, node));
            detected_additions += 1;
            continue;
        };

        let Some(&vanilla_index) = vanilla_index.get(&key.repr()) else {
            let relative = i - detected_additions;
            let index = if vanilla.len() > relative { relative } else { i };
            changelog.push(change(index, ChangeType::Add, node));
            detected_additions += 1;
            continue;
        };

        if !log_changes_inline(info, &mut node, &vanilla[vanilla_index]) {
            changelog.push(ArrayChange {
                index: vanilla_index as i32,
                change: ChangeType::Edit,
                node,
                key_primary: key.primary,
                key_secondary: key.secondary,
            });
        }
        vanilla_found[vanilla_index] = true;
    }

    for (i, found) in vanilla_found.iter().enumerate() {
        if !found {
            changelog.push(change(i, ChangeType::Remove, Byml::Null));
        }
    }

    changelog
}

/// BymlNameHashArrayChangelogBuilder ("Property" arrays).
fn name_hash_array_changes(info: &mut TrackingInfo, src: Vec<Byml>, vanilla: &[Byml]) -> Vec<ArrayChange> {
    const KEY: &str = "NameHash";

    let mut changelog = Vec::new();
    let mut vanilla_found = vec![false; vanilla.len()];

    let find = |hash: &Byml| -> Option<usize> {
        vanilla.iter().position(|entry| match entry.as_map().and_then(|m| m.get(KEY)) {
            Some(value) => value == hash,
            None => false,
        })
    };

    for (i, mut node) in src.into_iter().enumerate() {
        let hash = match node.as_map().and_then(|m| m.get(KEY)) {
            Some(value @ (Byml::UInt32(_) | Byml::Int(_))) => value.clone(),
            _ => {
                debug!("'Property' entry {} has no usable NameHash", i);
                changelog.push(change(i, ChangeType::Add, node));
                continue;
            }
        };

        let Some(vanilla_index) = find(&hash) else {
            changelog.push(change(i, ChangeType::Add, node));
            continue;
        };

        if !log_changes_inline(info, &mut node, &vanilla[vanilla_index]) {
            changelog.push(ArrayChange {
                index: vanilla_index as i32,
                change: ChangeType::Edit,
                node,
                key_primary: Some(hash),
                key_secondary: None,
            });
        }

        // TKMM marks the entry's own position here, not the vanilla match.
        if let Some(found) = vanilla_found.get_mut(i) {
            *found = true;
        }
    }

    for (i, found) in vanilla_found.iter().enumerate() {
        if !found {
            changelog.push(change(i, ChangeType::Remove, Byml::Null));
        }
    }

    changelog
}

/// BymlDirectIndexArrayChangelogBuilder: position by position.
fn direct_index_array_changes(info: &mut TrackingInfo, src: Vec<Byml>, vanilla: &[Byml]) -> Vec<ArrayChange> {
    let mut changelog = Vec::new();
    let vanilla_is_smaller = vanilla.len() < src.len();
    let smaller = src.len().min(vanilla.len());
    let larger = src.len().max(vanilla.len());

    let mut src = src.into_iter();
    for i in 0..smaller {
        let mut entry = src.next().unwrap();
        if !log_changes_inline(info, &mut entry, &vanilla[i]) {
            changelog.push(change(i, ChangeType::Edit, entry));
        }
    }

    for i in smaller..larger {
        if vanilla_is_smaller {
            changelog.push(change(i, ChangeType::Add, src.next().unwrap()));
        } else {
            changelog.push(change(i, ChangeType::Remove, Byml::Null));
        }
    }

    changelog
}

/// BymlChangelogBuilder.Build for a whole document: the changelog to store,
/// or `None` when the document matches vanilla.
pub fn build_document(canonical: &str, src: &[u8], vanilla: &[u8]) -> Result<Option<Vec<u8>>, String> {
    let vanilla = Byml::from_binary(vanilla).map_err(|e| e.to_string())?;
    let (mut document, format) = Byml::parse(src).map_err(|e| e.to_string())?;
    let mut info = TrackingInfo::for_canonical(canonical);
    if log_changes_inline(&mut info, &mut document, &vanilla) {
        return Ok(None);
    }
    Ok(Some(document.write(format)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(entries: &[(&str, Byml)]) -> Byml {
        Byml::Map(entries.iter().map(|(k, v)| (k.to_string(), v.clone())).collect())
    }

    #[test]
    fn maps_keep_only_changes() {
        let vanilla = map(&[("A", Byml::Int(1)), ("B", Byml::Int(2)), ("C", Byml::Int(3))]);
        let mut modded = map(&[("A", Byml::Int(1)), ("B", Byml::Int(20)), ("D", Byml::Int(4))]);
        let mut info = TrackingInfo::default();
        assert!(!log_changes_inline(&mut info, &mut modded, &vanilla));
        let result = modded.as_map().unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result["B"], Byml::Int(20));
        assert_eq!(result["C"], Byml::Changelog(ChangeType::Remove));
        assert_eq!(result["D"], Byml::Int(4));
    }

    #[test]
    fn identical_documents_are_vanilla() {
        let vanilla = map(&[("List", Byml::Array(vec![Byml::Int(1), Byml::Int(2)]))]);
        let mut modded = vanilla.clone();
        let mut info = TrackingInfo::default();
        assert!(log_changes_inline(&mut info, &mut modded, &vanilla));
        assert!(modded.as_map().unwrap().is_empty());
    }

    #[test]
    fn default_arrays_record_additions_and_removals() {
        let vanilla = vec![Byml::from("a"), Byml::from("b"), Byml::from("c")];
        let mut root = Byml::Array(vec![Byml::from("a"), Byml::from("c"), Byml::from("d")]);
        let mut info = TrackingInfo::default();
        assert!(!log_array_changes(ArrayBuilder::Default, &mut info, &mut root, &vanilla));
        let Byml::ArrayChangelog(changes) = root else { panic!() };
        // "b" (index 1) is not matched and index 1 of the mod is vanilla: removed.
        assert_eq!(changes[0].change, ChangeType::Remove);
        assert_eq!(changes[0].index, 1);
        assert_eq!(changes[1].change, ChangeType::Add);
        assert_eq!(changes[1].node, Byml::from("d"));
        assert_eq!(changes[1].index, 2);
    }

    #[test]
    fn keyed_arrays_record_edits_by_key() {
        let row = |name: &str, value: i32| map(&[("BoneName", Byml::from(name)), ("Value", Byml::Int(value))]);
        let vanilla = vec![row("Head", 1), row("Arm", 2)];
        let mut root = Byml::Array(vec![row("Arm", 5), row("Head", 1), row("Tail", 3)]);
        let mut info = TrackingInfo::default();
        let key = KeyName {
            primary: "BoneName",
            secondary: None,
        };
        assert!(!log_array_changes(ArrayBuilder::Keyed(key), &mut info, &mut root, &vanilla));
        let Byml::ArrayChangelog(changes) = root else { panic!() };
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].change, ChangeType::Edit);
        assert_eq!(changes[0].index, 1);
        assert_eq!(changes[0].key_primary, Some(Byml::from("Arm")));
        assert_eq!(changes[0].node, map(&[("Value", Byml::Int(5))]));
        assert_eq!(changes[1].change, ChangeType::Add);
    }
}
