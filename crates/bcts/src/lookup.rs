//! Hash maps and sets that cannot be walked in order.
//!
//! A salsa struct is ordered and hashed by its salsa id, and an id is the order
//! of first interning, which a parallel compile does not fix: two compiles of
//! one world give the same module or name different ids. So a container keyed
//! by one has no order worth having. `LookupMap` and `LookupSet` say so in the
//! type. They are for lookup, and the only ways to walk them are named for what
//! the walk may do with the order: `*_unordered` for a walk whose result
//! cannot depend on it, such as `all` or collecting into another lookup map,
//! and `sorted_by_key` for one that reaches output, ordered by content.
//!
//! They hash independently of order, so unlike a `HashMap` they can sit in a
//! memoized value or a salsa struct's identity, which is what the `BTreeMap`s
//! they replace were for. See the determinism section of salsa-patterns.md.

use rmx::prelude::*;
use rmx::std::collections::hash_map::Entry;
use rmx::std::fmt;
use rmx::std::hash::{BuildHasher, Hash, Hasher};
use rustc_hash::{FxBuildHasher, FxHashMap, FxHashSet};

/// A hash map for lookup, with no order to iterate in.
#[derive(Clone, salsa::SalsaValue)]
pub struct LookupMap<K, V>(FxHashMap<K, V>);

impl<K: Hash + Eq, V: PartialEq> PartialEq for LookupMap<K, V> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<K: Hash + Eq, V: Eq> Eq for LookupMap<K, V> {}

impl<K, V> Default for LookupMap<K, V> {
    fn default() -> Self {
        LookupMap(FxHashMap::default())
    }
}

impl<K: Hash + Eq, V> LookupMap<K, V> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        self.0.get(key)
    }

    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        self.0.get_mut(key)
    }

    pub fn contains_key(&self, key: &K) -> bool {
        self.0.contains_key(key)
    }

    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        self.0.insert(key, value)
    }

    pub fn remove(&mut self, key: &K) -> Option<V> {
        self.0.remove(key)
    }

    pub fn entry(&mut self, key: K) -> Entry<'_, K, V> {
        self.0.entry(key)
    }

    /// The entries in no particular order, for a walk whose result does not
    /// depend on it.
    pub fn iter_unordered(&self) -> impl Iterator<Item = (&K, &V)> {
        self.0.iter()
    }

    /// The keys in no particular order; see `iter_unordered`.
    pub fn keys_unordered(&self) -> impl Iterator<Item = &K> {
        self.0.keys()
    }

    /// The values in no particular order; see `iter_unordered`.
    pub fn values_unordered(&self) -> impl Iterator<Item = &V> {
        self.0.values()
    }

    /// The values in no particular order, mutably; see `iter_unordered`.
    pub fn values_mut_unordered(&mut self) -> impl Iterator<Item = &mut V> {
        self.0.values_mut()
    }

    /// The entries by value in no particular order; see `iter_unordered`.
    pub fn into_iter_unordered(self) -> impl Iterator<Item = (K, V)> {
        self.0.into_iter()
    }

    /// Keep the entries `keep` accepts, which it sees in no particular order.
    pub fn retain_unordered(&mut self, keep: impl FnMut(&K, &mut V) -> bool) {
        self.0.retain(keep)
    }

    /// The entries ordered by `key` of each key, for a walk that reaches output.
    pub fn sorted_by_key<T: Ord>(&self, mut key: impl FnMut(&K) -> T) -> Vec<(&K, &V)> {
        let mut entries: Vec<_> = self.0.iter().collect();
        entries.sort_by_cached_key(|(k, _)| key(k));
        entries
    }
}

impl<K: Hash + Eq, V> rmx::std::ops::Index<&K> for LookupMap<K, V> {
    type Output = V;

    fn index(&self, key: &K) -> &V {
        &self.0[key]
    }
}

impl<K: Hash + Eq, V> FromIterator<(K, V)> for LookupMap<K, V> {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        LookupMap(iter.into_iter().collect())
    }
}

impl<K: Hash + Eq, V> Extend<(K, V)> for LookupMap<K, V> {
    fn extend<I: IntoIterator<Item = (K, V)>>(&mut self, iter: I) {
        self.0.extend(iter)
    }
}

impl<K: Hash, V: Hash> Hash for LookupMap<K, V> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        unordered_hash(self.0.iter(), state)
    }
}

impl<K: fmt::Debug, V: fmt::Debug> fmt::Debug for LookupMap<K, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.0.iter()).finish()
    }
}

/// A hash set for lookup, with no order to iterate in.
#[derive(Clone, salsa::SalsaValue)]
pub struct LookupSet<K>(FxHashSet<K>);

impl<K: Hash + Eq> PartialEq for LookupSet<K> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<K: Hash + Eq> Eq for LookupSet<K> {}

impl<K> Default for LookupSet<K> {
    fn default() -> Self {
        LookupSet(FxHashSet::default())
    }
}

impl<K: Hash + Eq> LookupSet<K> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn contains(&self, key: &K) -> bool {
        self.0.contains(key)
    }

    /// Add `key`, true if it was not there already.
    pub fn insert(&mut self, key: K) -> bool {
        self.0.insert(key)
    }

    pub fn remove(&mut self, key: &K) -> bool {
        self.0.remove(key)
    }

    /// The members in no particular order, for a walk whose result does not
    /// depend on it.
    pub fn iter_unordered(&self) -> impl Iterator<Item = &K> {
        self.0.iter()
    }

    /// The members by value in no particular order; see `iter_unordered`.
    pub fn into_iter_unordered(self) -> impl Iterator<Item = K> {
        self.0.into_iter()
    }

    /// The members ordered by `key`, for a walk that reaches output.
    pub fn sorted_by_key<T: Ord>(&self, key: impl FnMut(&&K) -> T) -> Vec<&K> {
        let mut members: Vec<_> = self.0.iter().collect();
        members.sort_by_cached_key(key);
        members
    }
}

impl<K: Hash + Eq> FromIterator<K> for LookupSet<K> {
    fn from_iter<I: IntoIterator<Item = K>>(iter: I) -> Self {
        LookupSet(iter.into_iter().collect())
    }
}

impl<K: Hash + Eq> Extend<K> for LookupSet<K> {
    fn extend<I: IntoIterator<Item = K>>(&mut self, iter: I) {
        self.0.extend(iter)
    }
}

impl<K: Hash> Hash for LookupSet<K> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        unordered_hash(self.0.iter(), state)
    }
}

impl<K: fmt::Debug> fmt::Debug for LookupSet<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.0.iter()).finish()
    }
}

/// Hash `items` so that the order they come in does not matter.
///
/// Each item is hashed alone and the hashes summed, which commutes; the count
/// goes in too. Each hash is mixed before it is summed because `FxHasher` is
/// linear in what it is fed, so that summed raw it would see the keys and the
/// values but not which value went with which key.
fn unordered_hash<T: Hash, H: Hasher>(items: impl ExactSizeIterator<Item = T>, state: &mut H) {
    let len = items.len();
    let sum = items.fold(0u64, |sum, item| sum.wrapping_add(mix(FxBuildHasher.hash_one(item))));
    state.write_usize(len);
    state.write_u64(sum);
}

/// The splitmix64 finalizer.
fn mix(mut x: u64) -> u64 {
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
    x ^ (x >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash_of<T: Hash>(value: &T) -> u64 {
        FxBuildHasher.hash_one(value)
    }

    #[test]
    fn hash_does_not_depend_on_insertion_order() {
        let forward: LookupMap<u32, u32> = (0..100).map(|i| (i, i * 7)).collect();
        let backward: LookupMap<u32, u32> = (0..100).rev().map(|i| (i, i * 7)).collect();
        assert_eq!(forward, backward);
        assert_eq!(hash_of(&forward), hash_of(&backward));

        let forward: LookupSet<u32> = (0..100).collect();
        let backward: LookupSet<u32> = (0..100).rev().collect();
        assert_eq!(hash_of(&forward), hash_of(&backward));
    }

    #[test]
    fn hash_sees_values() {
        let a: LookupMap<u32, u32> = [(1, 2), (3, 4)].into_iter().collect();
        let b: LookupMap<u32, u32> = [(1, 4), (3, 2)].into_iter().collect();
        assert_ne!(hash_of(&a), hash_of(&b));
    }

    #[test]
    fn sorted_by_key_orders_by_content() {
        let map: LookupMap<u32, &str> = [(3, "c"), (1, "a"), (2, "b")].into_iter().collect();
        let values: Vec<_> = map.sorted_by_key(|k| *k).into_iter().map(|(_, v)| *v).collect();
        assert_eq!(values, ["a", "b", "c"]);
    }
}
