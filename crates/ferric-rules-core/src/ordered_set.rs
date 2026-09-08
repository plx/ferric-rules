//! Insertion-ordered membership with constant-time insertion and removal.
//!
//! The first two keys reuse the endpoint fields while the table has zero capacity.
//! Promoted sets retain their allocation, even when emptied. Snapshots contain
//! only the ordered key sequence and reject duplicate entries on decode.
use rustc_hash::FxHashMap as HashMap;
use std::hash::Hash;

#[derive(Clone, Copy)]
struct Links<K> {
    previous: Option<K>,
    next: Option<K>,
}

pub(crate) struct OrderedSet<K> {
    entries: HashMap<K, Links<K>>,
    first: Option<K>,
    last: Option<K>,
}

impl<K> Default for OrderedSet<K> {
    fn default() -> Self {
        Self {
            entries: HashMap::default(),
            first: None,
            last: None,
        }
    }
}

impl<K: Copy + Eq + Hash> OrderedSet<K> {
    pub(crate) fn insert(&mut self, key: K) -> bool {
        // Zero insertion capacity implies an empty table. It can also follow
        // tombstone-heavy removal from an allocated table; using the endpoints
        // again preserves that allocation for the next promotion.
        if self.entries.capacity() == 0 {
            if self.first.is_none() {
                self.first = Some(key);
                self.last = Some(key);
                return true;
            }
            if self.first == Some(key) || self.last == Some(key) {
                return false;
            }
            if self.first == self.last {
                self.last = Some(key);
                return true;
            }
            let first = self.first.unwrap();
            let middle = self.last.unwrap();
            self.entries.reserve(3);
            self.entries.insert(
                first,
                Links {
                    previous: None,
                    next: Some(middle),
                },
            );
            self.entries.insert(
                middle,
                Links {
                    previous: Some(first),
                    next: Some(key),
                },
            );
            self.entries.insert(
                key,
                Links {
                    previous: Some(middle),
                    next: None,
                },
            );
            self.last = Some(key);
            return true;
        }
        let std::collections::hash_map::Entry::Vacant(entry) = self.entries.entry(key) else {
            return false;
        };
        entry.insert(Links {
            previous: self.last,
            next: None,
        });
        if let Some(previous) = self.last {
            self.entries.get_mut(&previous).unwrap().next = Some(key);
        } else {
            self.first = Some(key);
        }
        self.last = Some(key);
        true
    }

    pub(crate) fn remove(&mut self, key: &K) -> bool {
        if self.entries.capacity() == 0 {
            if self.first.as_ref() == Some(key) {
                if self.first == self.last {
                    self.first = None;
                    self.last = None;
                } else {
                    self.first = self.last;
                }
                return true;
            }
            if self.last.as_ref() == Some(key) {
                self.last = self.first;
                return true;
            }
            return false;
        }
        let Some(links) = self.entries.remove(key) else {
            return false;
        };
        if let Some(previous) = links.previous {
            self.entries.get_mut(&previous).unwrap().next = links.next;
        } else {
            self.first = links.next;
        }
        if let Some(next) = links.next {
            self.entries.get_mut(&next).unwrap().previous = links.previous;
        } else {
            self.last = links.previous;
        }
        true
    }

    pub(crate) fn contains(&self, key: &K) -> bool {
        if self.entries.capacity() == 0 {
            self.first.as_ref() == Some(key) || self.last.as_ref() == Some(key)
        } else {
            self.entries.contains_key(key)
        }
    }

    pub(crate) fn len(&self) -> usize {
        if self.entries.capacity() == 0 {
            usize::from(self.first.is_some()) + usize::from(self.first != self.last)
        } else {
            self.entries.len()
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.first.is_none()
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.first = None;
        self.last = None;
    }

    pub(crate) fn iter(&self) -> Iter<'_, K> {
        Iter {
            set: self,
            front: self.first,
            back: self.last,
            remaining: self.len(),
        }
    }
}

pub(crate) struct Iter<'a, K> {
    set: &'a OrderedSet<K>,
    front: Option<K>,
    back: Option<K>,
    remaining: usize,
}

impl<'a, K: Copy + Eq + Hash> Iterator for Iter<'a, K> {
    type Item = &'a K;
    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        let front = self.front?;
        if let Some((key, links)) = self.set.entries.get_key_value(&front) {
            self.front = links.next;
            self.remaining -= 1;
            return Some(key);
        }
        // A table miss with remaining members is the header-only small case.
        // The linked traversal above retains its original lookup and control flow.
        debug_assert_eq!(self.set.entries.capacity(), 0);
        let key = if self.set.first == Some(front) {
            self.front = self.set.last;
            self.set.first.as_ref()?
        } else {
            self.front = None;
            self.set.last.as_ref()?
        };
        self.remaining -= 1;
        Some(key)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl<K: Copy + Eq + Hash> DoubleEndedIterator for Iter<'_, K> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        let back = self.back?;
        if let Some((key, links)) = self.set.entries.get_key_value(&back) {
            self.back = links.previous;
            self.remaining -= 1;
            return Some(key);
        }
        debug_assert_eq!(self.set.entries.capacity(), 0);
        let key = if self.set.first == Some(back) {
            self.back = None;
            self.set.first.as_ref()?
        } else {
            self.back = self.set.first;
            self.set.last.as_ref()?
        };
        self.remaining -= 1;
        Some(key)
    }
}
impl<K: Copy + Eq + Hash> ExactSizeIterator for Iter<'_, K> {}

impl<'a, K: Copy + Eq + Hash> IntoIterator for &'a OrderedSet<K> {
    type Item = &'a K;
    type IntoIter = Iter<'a, K>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[cfg(feature = "serde")]
impl<K: Copy + Eq + Hash + serde::Serialize> serde::Serialize for OrderedSet<K> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.iter())
    }
}

#[cfg(feature = "serde")]
impl<'de, K: Copy + Eq + Hash + serde::Deserialize<'de>> serde::Deserialize<'de> for OrderedSet<K> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let values = Vec::<K>::deserialize(deserializer)?;
        let mut set = Self::default();
        for value in values {
            if !set.insert(value) {
                return Err(serde::de::Error::custom("duplicate ordered membership"));
            }
        }
        Ok(set)
    }
}

#[cfg(test)]
mod tests {
    use super::OrderedSet;
    use proptest::prelude::*;

    #[test]
    fn promotion_preserves_duplicates_order_and_reuse() {
        let mut set = OrderedSet::default();
        assert!(set.insert(9));
        assert!(set.insert(2));
        assert!(!set.insert(9));
        assert_eq!(set.entries.capacity(), 0);
        assert!(set.remove(&9));
        assert!(set.insert(9));
        assert_eq!(set.iter().copied().collect::<Vec<_>>(), [2, 9]);
        assert!(set.insert(7));
        assert!(set.entries.capacity() > 0);
        assert_eq!(set.iter().copied().collect::<Vec<_>>(), [2, 9, 7]);
        assert!(!set.insert(9));
        assert!(set.remove(&9));
        assert!(set.insert(9));
        assert_eq!(set.iter().copied().collect::<Vec<_>>(), [2, 7, 9]);
        let capacity = set.entries.capacity();
        set.clear();
        assert!(set.is_empty());
        for key in 0..1024 {
            assert!(set.insert(key));
            assert!(set.remove(&key));
        }
        assert_eq!(set.entries.capacity(), capacity);
    }

    #[test]
    fn iterators_keep_exact_lengths_across_promotion() {
        for size in 0..8 {
            let mut set = OrderedSet::default();
            for key in 0..size {
                assert!(set.insert(key));
            }
            let mut iter = set.iter();
            let mut remaining: std::collections::VecDeque<_> = (0..size).collect();
            let mut front = true;
            while !remaining.is_empty() {
                assert_eq!(iter.len(), remaining.len());
                assert_eq!(iter.size_hint(), (remaining.len(), Some(remaining.len())));
                if front {
                    assert_eq!(iter.next().copied(), remaining.pop_front());
                } else {
                    assert_eq!(iter.next_back().copied(), remaining.pop_back());
                }
                front = !front;
            }
            assert_eq!(iter.len(), 0);
            assert_eq!(iter.next(), None);
            assert_eq!(iter.next_back(), None);
        }
    }

    #[test]
    fn full_collision_table_survives_emptying_and_small_reuse() {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        struct Collision(usize);
        impl std::hash::Hash for Collision {
            fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
                state.write_u8(0);
            }
        }

        let mut set = OrderedSet::default();
        for key in 0..3 {
            assert!(set.insert(Collision(key)));
        }
        // Reserve only after promotion, when every member is in the table.
        set.entries.reserve(32);
        let capacity = set.entries.capacity();
        for key in 3..capacity {
            assert!(set.insert(Collision(key)));
        }
        for key in 0..capacity {
            assert!(set.remove(&Collision(key)));
        }
        assert!(set.is_empty());
        assert_eq!(set.len(), 0);
        // A full cluster can leave an allocated table with zero insertion
        // capacity. Reuse must remain correct whichever erase policy Rust uses.
        for _ in 0..4 {
            assert!(set.insert(Collision(90)));
            assert!(set.insert(Collision(80)));
            assert_eq!(set.iter().map(|key| key.0).collect::<Vec<_>>(), [90, 80]);
            assert_eq!(
                set.iter().rev().map(|key| key.0).collect::<Vec<_>>(),
                [80, 90]
            );
            assert!(set.remove(&Collision(90)));
            assert!(set.insert(Collision(70)));
            assert!(set.insert(Collision(60)));
            assert_eq!(
                set.iter().map(|key| key.0).collect::<Vec<_>>(),
                [80, 70, 60]
            );
            set.clear();
            assert_eq!(set.len(), 0);
        }
    }

    proptest! {
        #[test]
        fn arbitrary_membership_changes_preserve_sequence(operations in prop::collection::vec((0u8..16, 0u8..3), 0..200)) {
            let mut set = OrderedSet::default();
            let mut expected = Vec::new();
            for (key, operation) in operations {
                match operation {
                    0 => {
                        let fresh = !expected.contains(&key);
                        prop_assert_eq!(set.insert(key), fresh);
                        if fresh { expected.push(key); }
                    }
                    1 => {
                        let present = expected.contains(&key);
                        prop_assert_eq!(set.remove(&key), present);
                        expected.retain(|item| *item != key);
                    }
                    _ => { set.clear(); expected.clear(); }
                }
                prop_assert_eq!(set.iter().copied().collect::<Vec<_>>(), expected.clone());
                prop_assert_eq!(set.iter().rev().copied().collect::<Vec<_>>(), expected.iter().rev().copied().collect::<Vec<_>>());
                prop_assert_eq!(set.len(), expected.len());
                for key in 0..16 { prop_assert_eq!(set.contains(&key), expected.contains(&key)); }
                let mut iter = set.iter();
                let mut remaining = expected.as_slice();
                while let Some((&first, rest)) = remaining.split_first() {
                    prop_assert_eq!(iter.next(), Some(&first));
                    remaining = rest;
                    if let Some((&last, rest)) = remaining.split_last() {
                        prop_assert_eq!(iter.next_back(), Some(&last));
                        remaining = rest;
                    }
                }
                prop_assert_eq!(iter.next(), None);
                prop_assert_eq!(iter.next_back(), None);
            }
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn snapshots_preserve_order_and_reject_duplicate_members() {
        use serde::de::value::{Error, SeqDeserializer};
        use serde::Deserialize;
        let set = OrderedSet::<u8>::deserialize(SeqDeserializer::<_, Error>::new(
            [9u8, 2, 7].into_iter(),
        ))
        .unwrap();
        assert_eq!(set.iter().copied().collect::<Vec<_>>(), [9, 2, 7]);
        assert!(
            OrderedSet::<u8>::deserialize(SeqDeserializer::<_, Error>::new(
                [9u8, 2, 9].into_iter()
            ))
            .is_err()
        );
    }
}
