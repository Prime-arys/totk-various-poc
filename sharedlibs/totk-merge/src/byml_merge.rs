//! Applying BYML changelogs on top of a base document.
//!
//! Port of BymlMerger and BymlMergeTracking. Array removals and additions are
//! not applied while changelogs are merged but recorded and applied once at
//! the end, so indices in every changelog keep referring to the vanilla
//! layout. TKMM remembers the arrays by object identity; here they are
//! remembered by their path from the root, which stays valid because nothing
//! moves until everything is applied (a node replaced wholesale drops the
//! tracking recorded under it, which is what losing the old object amounts to
//! in TKMM).

use alloc::collections::{BTreeMap, BTreeSet};

use hashbrown::HashMap;
use totk_formats::byml::{ArrayChange, Byml, ChangeType, Key, VecMap};

use crate::byml_changelog::{log_changes_inline, TrackingInfo};
use crate::byml_keys::{self, create_index_cache, merger_key_name, BymlKey, KeyName, KeyRepr};
use crate::prelude::*;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Segment {
    Key(String),
    Hash32(u32),
    Hash64(u64),
    Index(usize),
}

pub type Path = Vec<Segment>;

#[derive(Debug, Default)]
pub struct ArrayEntry {
    pub array_name: Option<String>,
    pub depth: i32,
    /// One list per changelog that added to the array: (insert index, node).
    pub additions: Vec<Vec<(i32, Byml)>>,
    pub removals: BTreeSet<i32>,
    /// Row indices removed by key (RSDB).
    pub keyed_removals: BTreeMap<String, usize>,
}

pub struct Tracking {
    canonical: String,
    /// BymlMergeTracking.Type: overrides the bgyml type when applying.
    pub type_override: Option<String>,
    pub depth: i32,
    maps: BTreeMap<Path, BTreeSet<Segment>>,
    pub arrays: BTreeMap<Path, ArrayEntry>,
}

pub type MergeResult<T = ()> = Result<T, String>;

impl Tracking {
    pub fn new(canonical: &str) -> Tracking {
        Tracking {
            canonical: canonical.to_string(),
            type_override: None,
            depth: 0,
            maps: BTreeMap::new(),
            arrays: BTreeMap::new(),
        }
    }

    fn type_for_merge(&self) -> &str {
        self.type_override.as_deref().unwrap_or("")
    }

    /// A node at `path` was replaced: whatever was tracked inside it is gone.
    fn forget(&mut self, path: &Path) {
        let stale = |key: &Path| key.len() >= path.len() && key[..path.len()] == path[..];
        self.maps.retain(|key, _| !stale(key));
        self.arrays.retain(|key, _| !stale(key));
    }

    pub fn array_entry(&mut self, path: &Path) -> &mut ArrayEntry {
        self.arrays.entry(path.clone()).or_default()
    }

    /// Applies every recorded removal and addition to `root`.
    pub fn apply(self, root: &mut Byml) -> MergeResult {
        let mut info = TrackingInfo::for_canonical(&self.canonical);

        for (path, keys) in &self.maps {
            let Some(node) = resolve(root, path) else {
                continue;
            };
            match node {
                Byml::Map(map) => keys.iter().for_each(|key| {
                    if let Segment::Key(key) = key {
                        map.remove(key.as_str());
                    }
                }),
                Byml::HashMap32(map) => keys.iter().for_each(|key| {
                    if let Segment::Hash32(key) = key {
                        map.remove(key);
                    }
                }),
                Byml::HashMap64(map) => keys.iter().for_each(|key| {
                    if let Segment::Hash64(key) = key {
                        map.remove(key);
                    }
                }),
                _ => {}
            }
        }

        // Deepest arrays first: applying one shifts the indices of everything
        // after it, which only matters to paths going through it.
        let type_override = self.type_override.clone();
        let canonical = self.canonical.clone();
        let mut arrays: Vec<(Path, ArrayEntry)> = self.arrays.into_iter().collect();
        arrays.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

        for (path, entry) in arrays {
            if let Some(Byml::Array(array)) = resolve(root, &path) {
                apply_array_entry(&canonical, type_override.as_deref(), array, entry, &mut info)?;
            }
        }
        Ok(())
    }
}

fn resolve<'a>(root: &'a mut Byml, path: &[Segment]) -> Option<&'a mut Byml> {
    let mut node = root;
    for segment in path {
        node = match (node, segment) {
            (Byml::Map(map), Segment::Key(key)) => map.get_mut(key.as_str())?,
            (Byml::HashMap32(map), Segment::Hash32(key)) => map.get_mut(key)?,
            (Byml::HashMap64(map), Segment::Hash64(key)) => map.get_mut(key)?,
            (Byml::Array(items), Segment::Index(index)) => items.get_mut(*index)?,
            _ => return None,
        };
    }
    Some(node)
}

/// BymlMerger.Merge.
pub fn merge(base: &mut Byml, changelog: Byml, tracking: &mut Tracking, path: &mut Path) -> MergeResult {
    match (base, changelog) {
        (Byml::Map(base), Byml::Map(changelog)) => merge_map(base, changelog, tracking, path, |k| Segment::Key(k.to_string())),
        (Byml::HashMap32(base), Byml::HashMap32(changelog)) => {
            merge_map(base, changelog, tracking, path, |k| Segment::Hash32(*k))
        }
        (Byml::HashMap64(base), Byml::HashMap64(changelog)) => {
            merge_map(base, changelog, tracking, path, |k| Segment::Hash64(*k))
        }
        (Byml::Array(base), Byml::ArrayChangelog(changes)) => merge_array(base, changes, None, tracking, path),
        (Byml::Array(base), Byml::Array(custom)) => {
            base.extend(custom);
            Ok(())
        }
        (base, changelog) => Err(format!(
            "merging a BYML node of type 0x{:02X} into one of type 0x{:02X} is not supported",
            changelog.node_type(),
            base.node_type()
        )),
    }
}

/// BymlMerger.MergeMap.
pub fn merge_map<K: Ord + Clone + MapKeyName>(
    base: &mut VecMap<K, Byml>,
    changelog: VecMap<K, Byml>,
    tracking: &mut Tracking,
    path: &mut Path,
    segment: impl Fn(&K) -> Segment,
) -> MergeResult {
    tracking.depth += 1;

    for (key, entry) in changelog {
        let key_segment = segment(&key);
        if entry.is_remove() {
            tracking.maps.entry(path.clone()).or_default().insert(key_segment);
            continue;
        }

        // Whatever happens next, the key is used again.
        if let Some(removed) = tracking.maps.get_mut(path) {
            removed.remove(&key_segment);
        }

        let Some(base_entry) = base.get_mut(&key) else {
            base.insert(key, entry);
            continue;
        };

        path.push(key_segment);
        let result = match (key.array_name(), entry, &mut *base_entry) {
            (Some(name), Byml::ArrayChangelog(changes), Byml::Array(base_array)) => {
                merge_array(base_array, changes, Some(name), tracking, path)
            }
            (_, entry, base_entry) if entry.is_container() && base_entry.is_container() => {
                merge(base_entry, entry, tracking, path)
            }
            (_, entry, base_entry) => {
                tracking.forget(path);
                *base_entry = entry;
                Ok(())
            }
        };
        path.pop();
        result?;
    }

    tracking.depth -= 1;
    Ok(())
}

/// Map keys that can name an array (only string keys do).
pub trait MapKeyName {
    fn array_name(&self) -> Option<&str>;
}

impl MapKeyName for Key {
    fn array_name(&self) -> Option<&str> {
        Some(self.as_str())
    }
}

impl MapKeyName for u32 {
    fn array_name(&self) -> Option<&str> {
        None
    }
}

impl MapKeyName for u64 {
    fn array_name(&self) -> Option<&str> {
        None
    }
}

/// BymlMerger.MergeArray.
pub fn merge_array(
    base: &mut Vec<Byml>,
    changelog: Vec<ArrayChange>,
    array_name: Option<&str>,
    tracking: &mut Tracking,
    path: &mut Path,
) -> MergeResult {
    let key_name = array_name.and_then(|name| merger_key_name(name, tracking.type_for_merge(), tracking.depth));
    let mut lookup: Option<HashMap<KeyRepr, usize>> = None;
    let mut additions: Option<usize> = None;

    for change in changelog {
        match change.change {
            ChangeType::Add => {
                let depth = tracking.depth;
                let entry = tracking.arrays.entry(path.clone()).or_insert_with(|| ArrayEntry {
                    array_name: array_name.map(str::to_string),
                    depth,
                    ..ArrayEntry::default()
                });
                let list = match additions {
                    Some(list) => list,
                    None => {
                        entry.additions.push(Vec::new());
                        let list = entry.additions.len() - 1;
                        additions = Some(list);
                        list
                    }
                };
                entry.additions[list].push((change.index, change.node));
            }
            ChangeType::Remove => {
                tracking.array_entry(path).removals.insert(change.index);
            }
            ChangeType::Edit => {
                if let Some(entry) = tracking.arrays.get_mut(path) {
                    entry.removals.remove(&change.index);
                }

                let key = BymlKey {
                    primary: change.key_primary,
                    secondary: change.key_secondary,
                };
                let index = best_match(base, change.index, &key, key_name.as_ref(), &mut lookup)?;

                path.push(Segment::Index(index));
                let result = if change.node.is_container() {
                    merge(&mut base[index], change.node, tracking, path)
                } else {
                    tracking.forget(path);
                    base[index] = change.node;
                    Ok(())
                };
                path.pop();
                result?;
            }
        }
    }
    Ok(())
}

fn best_match(
    base: &[Byml],
    index: i32,
    key: &BymlKey,
    key_name: Option<&KeyName>,
    lookup: &mut Option<HashMap<KeyRepr, usize>>,
) -> MergeResult<usize> {
    let in_range = index >= 0 && (index as usize) < base.len();
    let Some(key_name) = key_name else {
        return in_range.then_some(index as usize).ok_or_else(|| out_of_range(index, base.len()));
    };
    if key.is_empty() || (in_range && key_name.get_key(&base[index as usize]).matches(key)) {
        return in_range.then_some(index as usize).ok_or_else(|| out_of_range(index, base.len()));
    }

    let lookup = lookup.get_or_insert_with(|| create_index_cache(base, key_name));
    lookup
        .get(&key.repr())
        .copied()
        .ok_or_else(|| format!("no entry with the key {:?} / {:?}", key.primary, key.secondary))
}

fn out_of_range(index: i32, len: usize) -> String {
    format!("array edit at index {} but the array only has {} entries", index, len)
}

/// BymlMergeTracking.ApplyArrayEntry.
fn apply_array_entry(
    canonical: &str,
    type_override: Option<&str>,
    base: &mut Vec<Byml>,
    entry: ArrayEntry,
    info: &mut TrackingInfo,
) -> MergeResult {
    info.depth = entry.depth;
    let mut offset = 0usize;

    for &index in &entry.removals {
        if index >= 0 && (index as usize) < base.len() {
            base[index as usize] = Byml::Changelog(ChangeType::Remove);
        }
    }
    for &index in entry.keyed_removals.values() {
        if index < base.len() {
            base[index] = Byml::Changelog(ChangeType::Remove);
        }
    }

    // Group additions by insert index (keeping the order they were recorded
    // in), then process the groups by increasing index.
    let mut groups: Vec<(i32, Vec<Byml>)> = Vec::new();
    for (index, node) in entry.additions.into_iter().flatten() {
        match groups.iter_mut().find(|(i, _)| *i == index) {
            Some((_, nodes)) => nodes.push(node),
            None => groups.push((index, vec![node])),
        }
    }
    groups.sort_by_key(|(index, _)| *index);

    let mut keyed: HashMap<KeyRepr, usize> = HashMap::new();
    for (insert_index, additions) in groups {
        if additions.is_empty() {
            continue;
        }

        if let Some(name) = &entry.array_name {
            let bgyml_type = type_override.unwrap_or(&info.bgyml_type).to_string();
            let key_name = merger_key_name(name, &bgyml_type, info.depth);
            keyed_additions(canonical, &mut offset, base, insert_index, additions, key_name, info, &mut keyed)?;
            continue;
        }

        for addition in additions {
            insert_addition(&mut offset, base, insert_index, addition);
        }
    }

    base.retain(|node| !node.is_remove());
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn keyed_additions(
    canonical: &str,
    offset: &mut usize,
    base: &mut Vec<Byml>,
    insert_index: i32,
    additions: Vec<Byml>,
    key_name: Option<KeyName>,
    info: &mut TrackingInfo,
    keyed: &mut HashMap<KeyRepr, usize>,
) -> MergeResult {
    let mut groups: Vec<(BymlKey, KeyRepr, Vec<Byml>)> = Vec::new();
    for addition in additions {
        let key = key_name.map(|name| name.get_key(&addition)).unwrap_or_default();
        let repr = key.repr();
        match groups.iter_mut().find(|(_, r, _)| *r == repr) {
            Some((_, _, nodes)) => nodes.push(addition),
            None => groups.push((key, repr, vec![addition])),
        }
    }

    for (key, repr, mut entries) in groups {
        if entries.is_empty() {
            continue;
        }

        if key.is_empty() {
            for addition in entries {
                insert_addition(offset, base, insert_index, addition);
            }
            continue;
        }

        if let Some(&old_index) = keyed.get(&repr) {
            let existing = core::mem::replace(&mut base[old_index], Byml::Changelog(ChangeType::Remove));
            let index = merge_keyed_additions(canonical, existing, entries, offset, base, insert_index, info)?;
            keyed.insert(repr, index);
            continue;
        }

        let index = if entries.len() == 1 {
            insert_addition(offset, base, insert_index, entries.pop().unwrap())
        } else {
            let first = entries.remove(0);
            merge_keyed_additions(canonical, first, entries, offset, base, insert_index, info)?
        };
        keyed.insert(repr, index);
    }
    Ok(())
}

/// Several mods added an entry with the same key: merge them into one.
fn merge_keyed_additions(
    canonical: &str,
    mut base_node: Byml,
    mut entries: Vec<Byml>,
    offset: &mut usize,
    base: &mut Vec<Byml>,
    insert_index: i32,
    info: &mut TrackingInfo,
) -> MergeResult<usize> {
    for entry in entries.iter_mut() {
        log_changes_inline(info, entry, &base_node);
    }

    let mut tracking = Tracking::new(canonical);
    for changelog in entries {
        merge(&mut base_node, changelog, &mut tracking, &mut Vec::new())?;
    }
    tracking.apply(&mut base_node)?;

    Ok(insert_addition(offset, base, insert_index, base_node))
}

fn insert_addition(offset: &mut usize, base: &mut Vec<Byml>, insert_index: i32, addition: Byml) -> usize {
    let relative = insert_index.max(0) as usize + *offset;
    *offset += 1;
    if base.len() > relative {
        base.insert(relative, addition);
        relative
    } else {
        base.push(addition);
        base.len() - 1
    }
}

/// BymlMerger.Merge for a whole document: vanilla bytes plus changelog
/// documents, lowest priority first.
pub fn merge_documents(canonical: &str, vanilla: Vec<u8>, changelogs: &[Vec<u8>]) -> MergeResult<Vec<u8>> {
    let (mut merged, format) = Byml::parse(&vanilla).map_err(|e| e.to_string())?;
    drop(vanilla);
    let mut tracking = Tracking::new(canonical);
    for changelog in changelogs {
        let changelog = Byml::from_binary(changelog).map_err(|e| e.to_string())?;
        merge(&mut merged, changelog, &mut tracking, &mut Vec::new())?;
    }
    tracking.apply(&mut merged)?;
    Ok(merged.write(format))
}

pub use byml_keys::bgyml_type;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::byml_changelog::log_changes_inline;

    fn map(entries: &[(&str, Byml)]) -> Byml {
        Byml::Map(entries.iter().map(|(k, v)| (k.to_string(), v.clone())).collect())
    }

    fn changelog_of(vanilla: &Byml, modded: &Byml, canonical: &str) -> Byml {
        let mut changelog = modded.clone();
        let mut info = TrackingInfo::for_canonical(canonical);
        log_changes_inline(&mut info, &mut changelog, vanilla);
        changelog
    }

    fn merge_all(canonical: &str, vanilla: &Byml, mods: &[Byml]) -> Byml {
        let mut merged = vanilla.clone();
        let mut tracking = Tracking::new(canonical);
        for modded in mods {
            let changelog = changelog_of(vanilla, modded, canonical);
            merge(&mut merged, changelog, &mut tracking, &mut Vec::new()).unwrap();
        }
        tracking.apply(&mut merged).unwrap();
        merged
    }

    #[test]
    fn independent_map_edits_combine() {
        let vanilla = map(&[("A", Byml::Int(1)), ("B", Byml::Int(2)), ("C", Byml::Int(3))]);
        let mod_a = map(&[("A", Byml::Int(10)), ("B", Byml::Int(2)), ("C", Byml::Int(3))]);
        let mod_b = map(&[("A", Byml::Int(1)), ("B", Byml::Int(2))]);
        let merged = merge_all("Foo.bgyml", &vanilla, &[mod_a, mod_b]);
        assert_eq!(merged, map(&[("A", Byml::Int(10)), ("B", Byml::Int(2))]));
    }

    #[test]
    fn keyed_rows_from_two_mods_both_apply() {
        let row = |name: &str, value: i32| map(&[("BoneName", Byml::from(name)), ("Value", Byml::Int(value))]);
        let vanilla = map(&[("BoneList", Byml::Array(vec![row("Head", 1), row("Arm", 2), row("Leg", 3)]))]);
        // Mod A reorders and edits Arm; mod B edits Leg and adds Tail.
        let mod_a = map(&[("BoneList", Byml::Array(vec![row("Arm", 20), row("Head", 1), row("Leg", 3)]))]);
        let mod_b = map(&[("BoneList", Byml::Array(vec![row("Head", 1), row("Arm", 2), row("Leg", 30), row("Tail", 4)]))]);

        let merged = merge_all("Foo.bgyml", &vanilla, &[mod_a, mod_b]);
        let rows = merged.as_map().unwrap()["BoneList"].as_array().unwrap().clone();
        assert_eq!(rows, vec![row("Head", 1), row("Arm", 20), row("Leg", 30), row("Tail", 4)]);
    }

    #[test]
    fn removals_apply_after_every_edit() {
        let vanilla = map(&[("List", Byml::Array(vec![Byml::from("a"), Byml::from("b"), Byml::from("c")]))]);
        let mod_a = map(&[("List", Byml::Array(vec![Byml::from("a"), Byml::from("c")]))]);
        let mod_b = map(&[("List", Byml::Array(vec![Byml::from("a"), Byml::from("b"), Byml::from("c"), Byml::from("d")]))]);
        let merged = merge_all("Foo.bgyml", &vanilla, &[mod_a, mod_b]);
        assert_eq!(
            merged.as_map().unwrap()["List"],
            Byml::Array(vec![Byml::from("a"), Byml::from("c"), Byml::from("d")])
        );
    }

    #[test]
    fn replaced_nodes_drop_their_tracking() {
        let vanilla = map(&[("Inner", map(&[("List", Byml::Array(vec![Byml::Int(1), Byml::Int(2)]))]))]);
        let mut merged = vanilla.clone();
        let mut tracking = Tracking::new("Foo.bgyml");

        // First changelog removes an entry from Inner/List...
        let first = changelog_of(&vanilla, &map(&[("Inner", map(&[("List", Byml::Array(vec![Byml::Int(1)]))]))]), "Foo.bgyml");
        merge(&mut merged, first, &mut tracking, &mut Vec::new()).unwrap();
        // ...then another replaces Inner with a scalar.
        let second = map(&[("Inner", Byml::Int(7))]);
        merge(&mut merged, second, &mut tracking, &mut Vec::new()).unwrap();
        tracking.apply(&mut merged).unwrap();

        assert_eq!(merged, map(&[("Inner", Byml::Int(7))]));
    }
}
