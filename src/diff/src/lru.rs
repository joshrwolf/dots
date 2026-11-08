//! A least-recently-used map bounded by the bytes its values hold.

use std::collections::{BTreeMap, HashMap};
use std::hash::Hash;

#[derive(Debug)]
pub(crate) struct Lru<K, V> {
    entries: HashMap<K, Entry<V>>,
    /// Keys by last use, oldest first.
    order: BTreeMap<u64, K>,
    tick: u64,
    bytes: usize,
    budget: usize,
}

#[derive(Debug)]
struct Entry<V> {
    value: V,
    tick: u64,
    size: usize,
}

impl<K: Hash + Eq + Clone, V> Lru<K, V> {
    pub(crate) fn new(budget: usize) -> Self {
        Self {
            entries: HashMap::new(),
            order: BTreeMap::new(),
            tick: 0,
            bytes: 0,
            budget,
        }
    }

    /// The value for `key`, marking it used.
    pub(crate) fn get(&mut self, key: &K) -> Option<&V> {
        let entry = self.entries.get_mut(key)?;
        self.order.remove(&entry.tick);
        self.tick += 1;
        entry.tick = self.tick;
        self.order.insert(self.tick, key.clone());
        Some(&entry.value)
    }

    /// The value for `key` without marking it used.
    pub(crate) fn peek(&self, key: &K) -> Option<&V> {
        self.entries.get(key).map(|e| &e.value)
    }

    pub(crate) fn contains(&self, key: &K) -> bool {
        self.entries.contains_key(key)
    }

    /// Inserts `value`, `size` bytes, then drops the least recently used
    /// entries until the rest fit the budget. The newest entry always stays,
    /// even when it alone is over budget.
    pub(crate) fn insert(&mut self, key: K, value: V, size: usize) {
        self.remove(&key);
        self.tick += 1;
        self.order.insert(self.tick, key.clone());
        self.entries.insert(
            key,
            Entry {
                value,
                tick: self.tick,
                size,
            },
        );
        self.bytes += size;
        while self.bytes > self.budget && self.entries.len() > 1 {
            let Some((_, oldest)) = self.order.pop_first() else {
                break;
            };
            if let Some(e) = self.entries.remove(&oldest) {
                self.bytes -= e.size;
            }
        }
    }

    pub(crate) fn remove(&mut self, key: &K) -> Option<V> {
        let e = self.entries.remove(key)?;
        self.order.remove(&e.tick);
        self.bytes -= e.size;
        Some(e.value)
    }

    pub(crate) fn bytes(&self) -> usize {
        self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evicts_least_recently_used_beyond_the_budget() {
        let mut lru = Lru::new(10);
        lru.insert("a", 1, 4);
        lru.insert("b", 2, 4);
        assert_eq!(lru.get(&"a"), Some(&1), "a is now the newest");
        lru.insert("c", 3, 4);
        assert!(!lru.contains(&"b"), "b was least recently used");
        assert!(lru.contains(&"a") && lru.contains(&"c"));
        assert_eq!(lru.bytes(), 8);
    }

    #[test]
    fn keeps_an_oversized_newest_entry_and_replaces_in_place() {
        let mut lru = Lru::new(10);
        lru.insert("a", 1, 4);
        lru.insert("big", 2, 50);
        assert!(!lru.contains(&"a"));
        assert_eq!(lru.bytes(), 50);
        lru.insert("big", 3, 2);
        assert_eq!(lru.peek(&"big"), Some(&3));
        assert_eq!(lru.bytes(), 2);
        assert_eq!(lru.remove(&"big"), Some(3));
        assert_eq!(lru.bytes(), 0);
    }
}
