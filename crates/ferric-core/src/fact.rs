//! Fact storage and indexing.
//!
//! Facts are the working memory of the rules engine. This module provides
//! ordered facts, template facts, and the fact base that stores and indexes them.

use slotmap::SlotMap;
use smallvec::SmallVec;
use std::collections::{HashMap, HashSet};

use crate::symbol::Symbol;
use crate::value::Value;

slotmap::new_key_type! {
    /// Unique identifier for a fact within a fact base.
    pub struct FactId;
}

slotmap::new_key_type! {
    /// Unique identifier for a template definition.
    pub struct TemplateId;
}

/// An ordered fact: relation name + field values.
///
/// Example: `(person "Alice" 30)` has relation `person` and two fields.
#[derive(Clone, Debug)]
pub struct OrderedFact {
    pub relation: Symbol,
    pub fields: SmallVec<[Value; 8]>,
}

/// A template fact: template ID + slot values.
///
/// The template defines the slot names and types; this fact holds the values.
#[derive(Clone, Debug)]
pub struct TemplateFact {
    pub template_id: TemplateId,
    pub slots: Box<[Value]>,
}

/// A fact: either ordered or template-based.
#[derive(Clone, Debug)]
pub enum Fact {
    Ordered(OrderedFact),
    Template(TemplateFact),
}

/// A fact entry: the fact itself plus metadata.
#[derive(Clone, Debug)]
pub struct FactEntry {
    pub fact: Fact,
    pub id: FactId,
    pub timestamp: u64,
}

/// Fact base: storage and indexing for all facts in working memory.
///
/// Maintains indices for fast lookup by relation and template.
pub struct FactBase {
    facts: SlotMap<FactId, FactEntry>,
    by_template: HashMap<TemplateId, HashSet<FactId>>,
    by_relation: HashMap<Symbol, HashSet<FactId>>,
    next_timestamp: u64,
}

impl FactBase {
    /// Create a new, empty fact base.
    #[must_use]
    pub fn new() -> Self {
        Self {
            facts: SlotMap::with_key(),
            by_template: HashMap::new(),
            by_relation: HashMap::new(),
            next_timestamp: 0,
        }
    }

    /// Assert an ordered fact into working memory.
    ///
    /// Returns the unique `FactId` assigned to the fact.
    pub fn assert_ordered(&mut self, relation: Symbol, fields: SmallVec<[Value; 8]>) -> FactId {
        let timestamp = self.next_timestamp;
        self.next_timestamp += 1;

        let fact = Fact::Ordered(OrderedFact { relation, fields });
        let id = self.facts.insert_with_key(|id| FactEntry {
            fact,
            id,
            timestamp,
        });

        // Update relation index
        self.by_relation.entry(relation).or_default().insert(id);

        id
    }

    /// Assert a template fact into working memory.
    ///
    /// Returns the unique `FactId` assigned to the fact.
    pub fn assert_template(&mut self, template_id: TemplateId, slots: Box<[Value]>) -> FactId {
        let timestamp = self.next_timestamp;
        self.next_timestamp += 1;

        let fact = Fact::Template(TemplateFact { template_id, slots });
        let id = self.facts.insert_with_key(|id| FactEntry {
            fact,
            id,
            timestamp,
        });

        // Update template index
        self.by_template.entry(template_id).or_default().insert(id);

        id
    }

    /// Retract a fact from working memory.
    ///
    /// Returns the removed fact entry if it existed, or `None` if not found.
    pub fn retract(&mut self, id: FactId) -> Option<FactEntry> {
        let entry = self.facts.remove(id)?;

        // Clean up indices
        match &entry.fact {
            Fact::Ordered(ordered) => {
                if let Some(set) = self.by_relation.get_mut(&ordered.relation) {
                    set.remove(&id);
                    if set.is_empty() {
                        self.by_relation.remove(&ordered.relation);
                    }
                }
            }
            Fact::Template(template) => {
                if let Some(set) = self.by_template.get_mut(&template.template_id) {
                    set.remove(&id);
                    if set.is_empty() {
                        self.by_template.remove(&template.template_id);
                    }
                }
            }
        }

        Some(entry)
    }

    /// Lookup a fact by ID.
    #[must_use]
    pub fn get(&self, id: FactId) -> Option<&FactEntry> {
        self.facts.get(id)
    }

    /// Iterate over all facts.
    pub fn iter(&self) -> impl Iterator<Item = (FactId, &FactEntry)> {
        self.facts.iter()
    }

    /// Query facts by relation (ordered facts only).
    pub fn facts_by_relation(&self, relation: Symbol) -> impl Iterator<Item = FactId> + '_ {
        self.by_relation
            .get(&relation)
            .into_iter()
            .flat_map(|set| set.iter().copied())
    }

    /// Query facts by template (template facts only).
    pub fn facts_by_template(&self, template_id: TemplateId) -> impl Iterator<Item = FactId> + '_ {
        self.by_template
            .get(&template_id)
            .into_iter()
            .flat_map(|set| set.iter().copied())
    }

    /// Returns the number of facts in working memory.
    #[must_use]
    pub fn len(&self) -> usize {
        self.facts.len()
    }

    /// Returns `true` if there are no facts in working memory.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.facts.is_empty()
    }
}

impl Default for FactBase {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbol::SymbolTable;
    use crate::StringEncoding;

    #[test]
    fn new_fact_base_is_empty() {
        let fb = FactBase::new();
        assert!(fb.is_empty());
        assert_eq!(fb.len(), 0);
    }

    #[test]
    fn assert_ordered_fact() {
        let mut table = SymbolTable::new();
        let mut fb = FactBase::new();
        let rel = table
            .intern_symbol("person", StringEncoding::Ascii)
            .unwrap();
        let fields = smallvec::smallvec![Value::Integer(42)];

        let id = fb.assert_ordered(rel, fields.clone());

        assert_eq!(fb.len(), 1);
        let entry = fb.get(id).unwrap();
        assert_eq!(entry.id, id);
        assert_eq!(entry.timestamp, 0);

        if let Fact::Ordered(ordered) = &entry.fact {
            assert_eq!(ordered.relation, rel);
            assert_eq!(ordered.fields.len(), 1);
        } else {
            panic!("Expected ordered fact");
        }
    }

    #[test]
    fn assert_multiple_facts_increments_timestamp() {
        let mut table = SymbolTable::new();
        let mut fb = FactBase::new();
        let rel = table.intern_symbol("test", StringEncoding::Ascii).unwrap();

        let id1 = fb.assert_ordered(rel, smallvec::smallvec![]);
        let id2 = fb.assert_ordered(rel, smallvec::smallvec![]);

        assert_eq!(fb.get(id1).unwrap().timestamp, 0);
        assert_eq!(fb.get(id2).unwrap().timestamp, 1);
    }

    #[test]
    fn retract_fact() {
        let mut table = SymbolTable::new();
        let mut fb = FactBase::new();
        let rel = table.intern_symbol("test", StringEncoding::Ascii).unwrap();
        let id = fb.assert_ordered(rel, smallvec::smallvec![]);

        assert_eq!(fb.len(), 1);
        let removed = fb.retract(id);
        assert!(removed.is_some());
        assert_eq!(fb.len(), 0);
        assert!(fb.get(id).is_none());
    }

    #[test]
    fn retract_nonexistent_fact_returns_none() {
        let mut table = SymbolTable::new();
        let mut fb = FactBase::new();
        let rel = table.intern_symbol("test", StringEncoding::Ascii).unwrap();
        let id = fb.assert_ordered(rel, smallvec::smallvec![]);

        // Retract once
        fb.retract(id);
        // Retract again
        let result = fb.retract(id);
        assert!(result.is_none());
    }

    #[test]
    fn facts_by_relation() {
        let mut table = SymbolTable::new();
        let mut fb = FactBase::new();
        let person = table
            .intern_symbol("person", StringEncoding::Ascii)
            .unwrap();
        let car = table.intern_symbol("car", StringEncoding::Ascii).unwrap();

        let id1 = fb.assert_ordered(person, smallvec::smallvec![]);
        let id2 = fb.assert_ordered(car, smallvec::smallvec![]);
        let id3 = fb.assert_ordered(person, smallvec::smallvec![]);

        let person_facts: Vec<_> = fb.facts_by_relation(person).collect();
        assert_eq!(person_facts.len(), 2);
        assert!(person_facts.contains(&id1));
        assert!(person_facts.contains(&id3));

        let car_facts: Vec<_> = fb.facts_by_relation(car).collect();
        assert_eq!(car_facts.len(), 1);
        assert!(car_facts.contains(&id2));
    }

    #[test]
    fn facts_by_relation_after_retraction() {
        let mut table = SymbolTable::new();
        let mut fb = FactBase::new();
        let rel = table.intern_symbol("test", StringEncoding::Ascii).unwrap();

        let id1 = fb.assert_ordered(rel, smallvec::smallvec![]);
        let id2 = fb.assert_ordered(rel, smallvec::smallvec![]);

        fb.retract(id1);

        let facts: Vec<_> = fb.facts_by_relation(rel).collect();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0], id2);
    }

    #[test]
    fn relation_index_cleaned_up_when_empty() {
        let mut table = SymbolTable::new();
        let mut fb = FactBase::new();
        let rel = table.intern_symbol("test", StringEncoding::Ascii).unwrap();

        let id = fb.assert_ordered(rel, smallvec::smallvec![]);
        assert!(fb.by_relation.contains_key(&rel));

        fb.retract(id);
        assert!(!fb.by_relation.contains_key(&rel));
    }

    #[test]
    fn assert_template_fact() {
        // Create a distinct template ID using a temporary SlotMap
        let mut temp: SlotMap<TemplateId, ()> = SlotMap::with_key();
        let template_id = temp.insert(());

        let mut fb = FactBase::new();
        let slots = Box::new([Value::Integer(42)]);

        let id = fb.assert_template(template_id, slots);

        assert_eq!(fb.len(), 1);
        let entry = fb.get(id).unwrap();

        if let Fact::Template(template) = &entry.fact {
            assert_eq!(template.template_id, template_id);
            assert_eq!(template.slots.len(), 1);
        } else {
            panic!("Expected template fact");
        }
    }

    #[test]
    fn facts_by_template() {
        // Create distinct template IDs using a temporary SlotMap
        let mut temp: SlotMap<TemplateId, ()> = SlotMap::with_key();
        let t1 = temp.insert(());
        let t2 = temp.insert(());

        let mut fb = FactBase::new();

        let id1 = fb.assert_template(t1, Box::new([]));
        let id2 = fb.assert_template(t2, Box::new([]));
        let id3 = fb.assert_template(t1, Box::new([]));

        let t1_facts: Vec<_> = fb.facts_by_template(t1).collect();
        assert_eq!(t1_facts.len(), 2);
        assert!(t1_facts.contains(&id1));
        assert!(t1_facts.contains(&id3));

        let t2_facts: Vec<_> = fb.facts_by_template(t2).collect();
        assert_eq!(t2_facts.len(), 1);
        assert!(t2_facts.contains(&id2));
    }

    #[test]
    fn template_index_cleaned_up_when_empty() {
        // Create a distinct template ID using a temporary SlotMap
        let mut temp: SlotMap<TemplateId, ()> = SlotMap::with_key();
        let template_id = temp.insert(());

        let mut fb = FactBase::new();

        let id = fb.assert_template(template_id, Box::new([]));
        assert!(fb.by_template.contains_key(&template_id));

        fb.retract(id);
        assert!(!fb.by_template.contains_key(&template_id));
    }

    #[test]
    fn iter_all_facts() {
        let mut table = SymbolTable::new();
        let mut fb = FactBase::new();
        let rel = table.intern_symbol("test", StringEncoding::Ascii).unwrap();

        let id1 = fb.assert_ordered(rel, smallvec::smallvec![]);
        let id2 = fb.assert_ordered(rel, smallvec::smallvec![]);

        let all: Vec<_> = fb.iter().map(|(id, _)| id).collect();
        assert_eq!(all.len(), 2);
        assert!(all.contains(&id1));
        assert!(all.contains(&id2));
    }

    #[test]
    fn query_nonexistent_relation_returns_empty() {
        let mut table = SymbolTable::new();
        let fb = FactBase::new();
        let rel = table
            .intern_symbol("nonexistent", StringEncoding::Ascii)
            .unwrap();
        let facts: Vec<_> = fb.facts_by_relation(rel).collect();
        assert!(facts.is_empty());
    }

    #[test]
    fn query_nonexistent_template_returns_empty() {
        // Create a distinct template ID using a temporary SlotMap
        let mut temp: SlotMap<TemplateId, ()> = SlotMap::with_key();
        let template_id = temp.insert(());

        let fb = FactBase::new();
        let facts: Vec<_> = fb.facts_by_template(template_id).collect();
        assert!(facts.is_empty());
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::symbol::SymbolTable;
    use crate::StringEncoding;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn assert_and_retract_preserves_count_invariant(ops in prop::collection::vec(any::<bool>(), 0..100)) {
            let mut table = SymbolTable::new();
            let mut fb = FactBase::new();
            let rel = table.intern_symbol("test", StringEncoding::Ascii).unwrap();
            let mut ids = Vec::new();
            let mut expected_count = 0;

            for op in ops {
                if op && !ids.is_empty() {
                    // Retract
                    let idx = ids.len() / 2;
                    let id = ids.swap_remove(idx);
                    fb.retract(id);
                    expected_count -= 1;
                } else {
                    // Assert
                    let id = fb.assert_ordered(rel, smallvec::smallvec![]);
                    ids.push(id);
                    expected_count += 1;
                }
                prop_assert_eq!(fb.len(), expected_count);
            }
        }

        #[test]
        fn relation_index_always_consistent(ops in prop::collection::vec(any::<bool>(), 0..50)) {
            let mut table = SymbolTable::new();
            let mut fb = FactBase::new();
            let rel = table.intern_symbol("test", StringEncoding::Ascii).unwrap();
            let mut ids = Vec::new();

            for op in ops {
                if op && !ids.is_empty() {
                    let idx = ids.len() / 2;
                    let id = ids.swap_remove(idx);
                    fb.retract(id);
                } else {
                    let id = fb.assert_ordered(rel, smallvec::smallvec![]);
                    ids.push(id);
                }
            }

            // Verify relation index matches actual facts
            let indexed: HashSet<_> = fb.facts_by_relation(rel).collect();
            let actual: HashSet<_> = fb.iter()
                .filter_map(|(id, entry)| {
                    if let Fact::Ordered(ordered) = &entry.fact {
                        if ordered.relation == rel {
                            return Some(id);
                        }
                    }
                    None
                })
                .collect();

            prop_assert_eq!(indexed, actual);
        }

        #[test]
        fn timestamps_are_monotonic(count in 1..100_usize) {
            let mut table = SymbolTable::new();
            let mut fb = FactBase::new();
            let rel = table.intern_symbol("test", StringEncoding::Ascii).unwrap();
            let mut timestamps = Vec::new();

            for _ in 0..count {
                let id = fb.assert_ordered(rel, smallvec::smallvec![]);
                timestamps.push(fb.get(id).unwrap().timestamp);
            }

            // Verify timestamps are strictly increasing
            for i in 1..timestamps.len() {
                prop_assert!(timestamps[i] > timestamps[i - 1]);
            }
        }
    }
}
