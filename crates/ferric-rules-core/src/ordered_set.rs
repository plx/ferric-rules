//! Insertion-ordered membership with constant-time insertion and removal.
//!
//! Links are owned keys rather than pointers. Snapshots contain only the ordered
//! key sequence; deserialization rebuilds links and rejects duplicate entries.
use rustc_hash::FxHashMap as HashMap;
use std::hash::Hash;

#[derive(Clone, Copy)]
struct Links<K> {
    previous: Option<K>,
    next: Option<K>,
}

pub(crate) struct LinkedSet<K> {
    entries: HashMap<K, Links<K>>,
    first: Option<K>,
    last: Option<K>,
}

impl<K> Default for LinkedSet<K> {
    fn default() -> Self {
        Self {
            entries: HashMap::default(),
            first: None,
            last: None,
        }
    }
}

impl<K: Copy + Eq + Hash> LinkedSet<K> {
    /// Build the first linked block without repeatedly looking up its predecessors.
    /// The inline caller has already checked that the three keys are distinct.
    fn from_three(first: K, middle: K, last: K) -> Self {
        Self {
            entries: [
                (
                    first,
                    Links {
                        previous: None,
                        next: Some(middle),
                    },
                ),
                (
                    middle,
                    Links {
                        previous: Some(first),
                        next: Some(last),
                    },
                ),
                (
                    last,
                    Links {
                        previous: Some(middle),
                        next: None,
                    },
                ),
            ]
            .into_iter()
            .collect(),
            first: Some(first),
            last: Some(last),
        }
    }

    pub(crate) fn insert(&mut self, key: K) -> bool {
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
        self.entries.contains_key(key)
    }
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.first = None;
        self.last = None;
    }
    pub(crate) fn iter(&self) -> LinkedIter<'_, K> {
        LinkedIter {
            set: self,
            front: self.first,
            back: self.last,
            remaining: self.len(),
        }
    }
}

pub(crate) struct LinkedIter<'a, K> {
    set: &'a LinkedSet<K>,
    front: Option<K>,
    back: Option<K>,
    remaining: usize,
}

impl<'a, K: Copy + Eq + Hash> Iterator for LinkedIter<'a, K> {
    type Item = &'a K;
    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        let (key, links) = self.set.entries.get_key_value(&self.front?)?;
        self.front = links.next;
        self.remaining -= 1;
        Some(key)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl<K: Copy + Eq + Hash> DoubleEndedIterator for LinkedIter<'_, K> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        let (key, links) = self.set.entries.get_key_value(&self.back?)?;
        self.back = links.previous;
        self.remaining -= 1;
        Some(key)
    }
}
impl<K: Copy + Eq + Hash> ExactSizeIterator for LinkedIter<'_, K> {}

/// Keep sets of up to two keys inline; larger sets retain the hash-linked
/// representation and its insertion order.
pub(crate) enum OrderedSet<K> {
    Inline(smallvec::SmallVec<[K; 2]>),
    Linked(LinkedSet<K>),
}

impl<K> Default for OrderedSet<K> {
    fn default() -> Self {
        Self::Inline(smallvec::SmallVec::new())
    }
}

impl<K: Copy + Eq + Hash> OrderedSet<K> {
    pub(crate) fn insert(&mut self, key: K) -> bool {
        match self {
            Self::Inline(values) => {
                if values.contains(&key) {
                    return false;
                }
                if values.len() < 2 {
                    values.push(key);
                } else {
                    *self = Self::Linked(LinkedSet::from_three(values[0], values[1], key));
                }
                true
            }
            Self::Linked(linked) => linked.insert(key),
        }
    }

    pub(crate) fn remove(&mut self, key: &K) -> bool {
        match self {
            Self::Inline(values) => {
                let Some(index) = values.iter().position(|value| value == key) else {
                    return false;
                };
                values.remove(index);
                true
            }
            Self::Linked(linked) => linked.remove(key),
        }
    }

    pub(crate) fn contains(&self, key: &K) -> bool {
        match self {
            Self::Inline(values) => values.contains(key),
            Self::Linked(linked) => linked.contains(key),
        }
    }

    pub(crate) fn len(&self) -> usize {
        match self {
            Self::Inline(values) => values.len(),
            Self::Linked(linked) => linked.len(),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub(crate) fn clear(&mut self) {
        match self {
            Self::Inline(values) => values.clear(),
            // Retain the existing allocation for large-set reset/reuse.
            Self::Linked(linked) => linked.clear(),
        }
    }

    pub(crate) fn iter(&self) -> Iter<'_, K> {
        match self {
            Self::Inline(values) => Iter::Inline(values.iter()),
            Self::Linked(linked) => Iter::Linked(linked.iter()),
        }
    }
}

pub(crate) enum Iter<'a, K> {
    Inline(std::slice::Iter<'a, K>),
    Linked(LinkedIter<'a, K>),
}

impl<'a, K: Copy + Eq + Hash> Iterator for Iter<'a, K> {
    type Item = &'a K;
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Inline(values) => values.next(),
            Self::Linked(linked) => linked.next(),
        }
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            Self::Inline(values) => values.size_hint(),
            Self::Linked(linked) => linked.size_hint(),
        }
    }
}

impl<K: Copy + Eq + Hash> DoubleEndedIterator for Iter<'_, K> {
    fn next_back(&mut self) -> Option<Self::Item> {
        match self {
            Self::Inline(values) => values.next_back(),
            Self::Linked(linked) => linked.next_back(),
        }
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
        assert!(matches!(set, OrderedSet::Inline(_)));
        assert!(set.remove(&9));
        assert!(set.insert(9));
        assert_eq!(set.iter().copied().collect::<Vec<_>>(), [2, 9]);
        assert!(set.insert(7));
        assert!(matches!(set, OrderedSet::Linked(_)));
        assert_eq!(set.iter().copied().collect::<Vec<_>>(), [2, 9, 7]);
        assert!(!set.insert(9));
        assert!(set.remove(&9));
        assert!(set.insert(9));
        assert_eq!(set.iter().copied().collect::<Vec<_>>(), [2, 7, 9]);
        let capacity = match &set {
            OrderedSet::Linked(linked) => linked.entries.capacity(),
            OrderedSet::Inline(_) => unreachable!(),
        };
        set.clear();
        assert!(set.is_empty());
        for key in 0..1024 {
            assert!(set.insert(key));
            assert!(set.remove(&key));
        }
        match &set {
            OrderedSet::Linked(linked) => assert_eq!(linked.entries.capacity(), capacity),
            OrderedSet::Inline(_) => panic!("large-set allocation should be retained"),
        }
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
