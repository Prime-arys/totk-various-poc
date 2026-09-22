//! A map stored as a sorted vector.
//!
//! BYML documents hold hundreds of thousands of small maps (the GameDataList
//! alone has ~200 000). A `BTreeMap` costs a node allocation of several hundred
//! bytes even for three entries; a sorted vector costs one allocation sized to
//! its entries, which divides the memory a parsed document takes by three —
//! the difference between fitting on the console or not.

use core::borrow::Borrow;

use crate::prelude::*;

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct VecMap<K, V> {
    entries: Vec<(K, V)>,
}

impl<K, V> Default for VecMap<K, V> {
    fn default() -> Self {
        VecMap { entries: Vec::new() }
    }
}

impl<K: core::fmt::Debug, V: core::fmt::Debug> core::fmt::Debug for VecMap<K, V> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_map().entries(self.entries.iter().map(|(k, v)| (k, v))).finish()
    }
}

impl<K: Ord, V> VecMap<K, V> {
    pub fn new() -> Self {
        VecMap { entries: Vec::new() }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        VecMap {
            entries: Vec::with_capacity(capacity),
        }
    }

    fn find<Q: ?Sized + Ord>(&self, key: &Q) -> Result<usize, usize>
    where
        K: Borrow<Q>,
    {
        self.entries.binary_search_by(|(k, _)| k.borrow().cmp(key))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get<Q: ?Sized + Ord>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
    {
        self.find(key).ok().map(|i| &self.entries[i].1)
    }

    pub fn get_mut<Q: ?Sized + Ord>(&mut self, key: &Q) -> Option<&mut V>
    where
        K: Borrow<Q>,
    {
        match self.find(key) {
            Ok(i) => Some(&mut self.entries[i].1),
            Err(_) => None,
        }
    }

    pub fn contains_key<Q: ?Sized + Ord>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
    {
        self.find(key).is_ok()
    }

    /// Inserts or replaces, returning the previous value.
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        // Documents are read in key order, so appending is the common case.
        if self.entries.last().map_or(true, |(last, _)| *last < key) {
            self.entries.push((key, value));
            return None;
        }
        match self.find(&key) {
            Ok(i) => Some(core::mem::replace(&mut self.entries[i].1, value)),
            Err(i) => {
                self.entries.insert(i, (key, value));
                None
            }
        }
    }

    pub fn remove<Q: ?Sized + Ord>(&mut self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
    {
        match self.find(key) {
            Ok(i) => Some(self.entries.remove(i).1),
            Err(_) => None,
        }
    }

    pub fn retain(&mut self, mut keep: impl FnMut(&K, &mut V) -> bool) {
        self.entries.retain_mut(|(k, v)| keep(k, v));
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = (&K, &V)> + ExactSizeIterator {
        self.entries.iter().map(|(k, v)| (k, v))
    }

    pub fn iter_mut(&mut self) -> impl DoubleEndedIterator<Item = (&K, &mut V)> + ExactSizeIterator {
        self.entries.iter_mut().map(|(k, v)| (&*k, v))
    }

    pub fn keys(&self) -> impl DoubleEndedIterator<Item = &K> + ExactSizeIterator {
        self.entries.iter().map(|(k, _)| k)
    }

    pub fn values(&self) -> impl DoubleEndedIterator<Item = &V> + ExactSizeIterator {
        self.entries.iter().map(|(_, v)| v)
    }

    pub fn values_mut(&mut self) -> impl DoubleEndedIterator<Item = &mut V> + ExactSizeIterator {
        self.entries.iter_mut().map(|(_, v)| v)
    }

    pub fn into_values(self) -> impl DoubleEndedIterator<Item = V> + ExactSizeIterator {
        self.entries.into_iter().map(|(_, v)| v)
    }

    pub fn into_keys(self) -> impl DoubleEndedIterator<Item = K> + ExactSizeIterator {
        self.entries.into_iter().map(|(k, _)| k)
    }
}

impl<K: Ord, V> FromIterator<(K, V)> for VecMap<K, V> {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        let mut entries: Vec<(K, V)> = iter.into_iter().collect();
        // Stable, so the last of duplicate keys can be kept.
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        let mut deduplicated: Vec<(K, V)> = Vec::with_capacity(entries.len());
        for entry in entries {
            match deduplicated.last_mut() {
                Some(last) if last.0 == entry.0 => *last = entry,
                _ => deduplicated.push(entry),
            }
        }
        VecMap { entries: deduplicated }
    }
}

impl<K: Ord + Borrow<Q>, Q: ?Sized + Ord, V> core::ops::Index<&Q> for VecMap<K, V> {
    type Output = V;

    fn index(&self, key: &Q) -> &V {
        self.get(key).expect("key not in map")
    }
}

impl<K: Ord, V> Extend<(K, V)> for VecMap<K, V> {
    fn extend<I: IntoIterator<Item = (K, V)>>(&mut self, iter: I) {
        for (k, v) in iter {
            self.insert(k, v);
        }
    }
}

impl<K, V> IntoIterator for VecMap<K, V> {
    type Item = (K, V);
    type IntoIter = alloc::vec::IntoIter<(K, V)>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
    }
}

impl<'a, K, V> IntoIterator for &'a VecMap<K, V> {
    type Item = (&'a K, &'a V);
    type IntoIter = core::iter::Map<core::slice::Iter<'a, (K, V)>, fn(&'a (K, V)) -> (&'a K, &'a V)>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter().map(|(k, v)| (k, v))
    }
}

impl<'a, K, V> IntoIterator for &'a mut VecMap<K, V> {
    type Item = (&'a K, &'a mut V);
    type IntoIter = core::iter::Map<core::slice::IterMut<'a, (K, V)>, fn(&'a mut (K, V)) -> (&'a K, &'a mut V)>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter_mut().map(|(k, v)| (&*k, v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn behaves_like_an_ordered_map() {
        let mut map = VecMap::new();
        assert_eq!(map.insert("b".to_string(), 2), None);
        assert_eq!(map.insert("a".to_string(), 1), None);
        assert_eq!(map.insert("c".to_string(), 3), None);
        assert_eq!(map.insert("b".to_string(), 20), Some(2));
        assert_eq!(map.keys().map(String::as_str).collect::<Vec<_>>(), ["a", "b", "c"]);
        assert_eq!(map.get("b"), Some(&20));
        assert_eq!(map.remove("a"), Some(1));
        assert!(!map.contains_key("a"));

        let collected: VecMap<u32, &str> = [(3, "x"), (1, "y"), (3, "z")].into_iter().collect();
        assert_eq!(collected.iter().collect::<Vec<_>>(), [(&1, &"y"), (&3, &"z")]);
    }
}
