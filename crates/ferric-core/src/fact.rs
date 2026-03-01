//! Fact storage and indexing.
//!
//! Facts are the working memory of the rules engine. This module provides
//! ordered facts, template facts, and the fact base that stores and indexes them.

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use slotmap::SlotMap;
use smallvec::SmallVec;

use crate::symbol::{Symbol, SymbolId};
use crate::value::Value;

/// Monotonically increasing timestamp assigned to facts as they enter working memory.
///
/// Timestamps flow from `FactBase` through `FactEntry` → `Token` (via recency
/// vectors) → `Activation` → `AgendaKey` → `StrategyOrd`. Wrapping them in a
/// newtype prevents accidental confusion with other `u64` values such as
/// `ActivationSeq`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(u64);

impl Timestamp {
    pub const ZERO: Self = Self(0);

    #[must_use]
    pub const fn new(val: u64) -> Self {
        Self(val)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    #[must_use]
    pub fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

fn remove_from_set_index<K>(index: &mut HashMap<K, HashSet<FactId>>, key: K, id: FactId)
where
    K: Copy + Eq + std::hash::Hash,
{
    let mut remove_key = false;
    if let Some(set) = index.get_mut(&key) {
        set.remove(&id);
        remove_key = set.is_empty();
    }

    if remove_key {
        index.remove(&key);
    }
}

#[derive(Debug)]
struct SymbolMap<T> {
    ascii: Vec<Option<T>>,
    utf8: Vec<Option<T>>,
}

impl<T> SymbolMap<T> {
    fn new() -> Self {
        Self {
            ascii: Vec::new(),
            utf8: Vec::new(),
        }
    }

    #[cfg(test)]
    fn contains_key(&self, key: &Symbol) -> bool {
        self.get(key).is_some()
    }

    fn get(&self, key: &Symbol) -> Option<&T> {
        self.slot(*key).and_then(Option::as_ref)
    }

    fn get_mut(&mut self, key: &Symbol) -> Option<&mut T> {
        self.slot_mut(*key).and_then(Option::as_mut)
    }

    fn get_or_insert_with(&mut self, key: Symbol, f: impl FnOnce() -> T) -> &mut T {
        self.slot_mut_or_grow(key).get_or_insert_with(f)
    }

    #[cfg(test)]
    fn values(&self) -> impl Iterator<Item = &T> + '_ {
        self.ascii
            .iter()
            .chain(self.utf8.iter())
            .filter_map(Option::as_ref)
    }

    fn remove(&mut self, key: &Symbol) -> Option<T> {
        self.slot_mut(*key).and_then(Option::take)
    }

    fn slot(&self, key: Symbol) -> Option<&Option<T>> {
        match key.0 {
            SymbolId::Ascii(idx) => self.ascii.get(idx as usize),
            SymbolId::Utf8(idx) => self.utf8.get(idx as usize),
        }
    }

    fn slot_mut(&mut self, key: Symbol) -> Option<&mut Option<T>> {
        match key.0 {
            SymbolId::Ascii(idx) => self.ascii.get_mut(idx as usize),
            SymbolId::Utf8(idx) => self.utf8.get_mut(idx as usize),
        }
    }

    fn slot_mut_or_grow(&mut self, key: Symbol) -> &mut Option<T> {
        match key.0 {
            SymbolId::Ascii(idx) => {
                let idx = idx as usize;
                if idx >= self.ascii.len() {
                    self.ascii.resize_with(idx + 1, || None);
                }
                &mut self.ascii[idx]
            }
            SymbolId::Utf8(idx) => {
                let idx = idx as usize;
                if idx >= self.utf8.len() {
                    self.utf8.resize_with(idx + 1, || None);
                }
                &mut self.utf8[idx]
            }
        }
    }
}

fn remove_from_symbol_set_index(index: &mut SymbolMap<HashSet<FactId>>, key: Symbol, id: FactId) {
    let mut remove_key = false;
    if let Some(set) = index.get_mut(&key) {
        set.remove(&id);
        remove_key = set.is_empty();
    }

    if remove_key {
        index.remove(&key);
    }
}

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

impl AsRef<[Value]> for OrderedFact {
    fn as_ref(&self) -> &[Value] {
        self.fields.as_slice()
    }
}

impl AsMut<[Value]> for OrderedFact {
    fn as_mut(&mut self) -> &mut [Value] {
        self.fields.as_mut_slice()
    }
}

/// A template fact: template ID + slot values.
///
/// The template defines the slot names and types; this fact holds the values.
#[derive(Clone, Debug)]
pub struct TemplateFact {
    pub template_id: TemplateId,
    pub slots: Box<[Value]>,
}

impl AsRef<[Value]> for TemplateFact {
    fn as_ref(&self) -> &[Value] {
        self.slots.as_ref()
    }
}

impl AsMut<[Value]> for TemplateFact {
    fn as_mut(&mut self) -> &mut [Value] {
        self.slots.as_mut()
    }
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
    pub timestamp: Timestamp,
}

/// Fact base: storage and indexing for all facts in working memory.
///
/// Maintains indices for fast lookup by relation and template.
pub struct FactBase {
    facts: SlotMap<FactId, FactEntry>,
    by_template: HashMap<TemplateId, HashSet<FactId>>,
    by_relation: SymbolMap<HashSet<FactId>>,
    next_timestamp: Timestamp,
}

impl FactBase {
    /// Create a new, empty fact base.
    #[must_use]
    pub fn new() -> Self {
        Self {
            facts: SlotMap::with_key(),
            by_template: HashMap::default(),
            by_relation: SymbolMap::new(),
            next_timestamp: Timestamp::ZERO,
        }
    }

    fn insert_fact(&mut self, fact: Fact) -> FactId {
        let timestamp = self.next_timestamp;
        self.next_timestamp = self.next_timestamp.next();

        self.facts.insert_with_key(|id| FactEntry {
            fact,
            id,
            timestamp,
        })
    }

    /// Assert an ordered fact into working memory.
    ///
    /// Returns the unique `FactId` assigned to the fact.
    pub fn assert_ordered(&mut self, relation: Symbol, fields: SmallVec<[Value; 8]>) -> FactId {
        let id = self.insert_fact(Fact::Ordered(OrderedFact { relation, fields }));

        // Update relation index
        self.by_relation
            .get_or_insert_with(relation, HashSet::default)
            .insert(id);

        id
    }

    /// Assert a template fact into working memory.
    ///
    /// Returns the unique `FactId` assigned to the fact.
    pub fn assert_template(&mut self, template_id: TemplateId, slots: Box<[Value]>) -> FactId {
        let id = self.insert_fact(Fact::Template(TemplateFact { template_id, slots }));

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
                remove_from_symbol_set_index(&mut self.by_relation, ordered.relation, id);
            }
            Fact::Template(template) => {
                remove_from_set_index(&mut self.by_template, template.template_id, id);
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
        assert_eq!(entry.timestamp, Timestamp::ZERO);

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

        assert_eq!(fb.get(id1).unwrap().timestamp, Timestamp::new(0));
        assert_eq!(fb.get(id2).unwrap().timestamp, Timestamp::new(1));
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

    // Operation model for generating arbitrary sequences of FactBase mutations.
    #[derive(Clone, Debug)]
    enum Op {
        AssertOrdered {
            relation_idx: usize,
            field_count: usize,
        },
        AssertTemplate {
            template_idx: usize,
        },
        Retract {
            idx: usize,
        },
    }

    fn op_strategy(n_relations: usize, n_templates: usize) -> impl Strategy<Value = Op> {
        prop_oneof![
            (0..n_relations, 0usize..=4).prop_map(|(r, f)| Op::AssertOrdered {
                relation_idx: r,
                field_count: f,
            }),
            (0..n_templates).prop_map(|t| Op::AssertTemplate { template_idx: t }),
            any::<usize>().prop_map(|idx| Op::Retract { idx }),
        ]
    }

    fn scenario_strategy() -> impl Strategy<Value = (usize, usize, Vec<Op>)> {
        (3usize..=5, 2usize..=3).prop_flat_map(|(n_rel, n_tmpl)| {
            let ops = prop::collection::vec(op_strategy(n_rel, n_tmpl), 0..200);
            (Just(n_rel), Just(n_tmpl), ops)
        })
    }

    // Shadow model tracking expected state independently of FactBase.
    #[derive(Clone, Debug)]
    enum FactKind {
        Ordered { relation_idx: usize },
        Template { template_idx: usize },
    }

    struct Model {
        live: HashMap<FactId, FactKind>,
        timestamps: Vec<Timestamp>,
    }

    impl Model {
        fn new() -> Self {
            Self {
                live: HashMap::default(),
                timestamps: Vec::new(),
            }
        }

        fn ids_for_relation(&self, rel_idx: usize) -> HashSet<FactId> {
            self.live
                .iter()
                .filter_map(|(id, kind)| match kind {
                    FactKind::Ordered { relation_idx } if *relation_idx == rel_idx => Some(*id),
                    _ => None,
                })
                .collect()
        }

        fn ids_for_template(&self, tmpl_idx: usize) -> HashSet<FactId> {
            self.live
                .iter()
                .filter_map(|(id, kind)| match kind {
                    FactKind::Template { template_idx } if *template_idx == tmpl_idx => Some(*id),
                    _ => None,
                })
                .collect()
        }
    }

    fn run_scenario(
        n_relations: usize,
        n_templates: usize,
        ops: &[Op],
    ) -> (FactBase, Model, Vec<Symbol>, Vec<TemplateId>, Vec<FactId>) {
        let mut table = SymbolTable::new();
        let relations: Vec<Symbol> = (0..n_relations)
            .map(|i| {
                table
                    .intern_symbol(&format!("rel{i}"), StringEncoding::Ascii)
                    .unwrap()
            })
            .collect();
        let mut tmpl_map: SlotMap<TemplateId, ()> = SlotMap::with_key();
        let template_ids: Vec<TemplateId> = (0..n_templates).map(|_| tmpl_map.insert(())).collect();

        let mut fb = FactBase::new();
        let mut model = Model::new();
        let mut live_ids: Vec<FactId> = Vec::new();

        for op in ops {
            match op {
                Op::AssertOrdered {
                    relation_idx,
                    field_count,
                } => {
                    let rel = relations[*relation_idx];
                    let fields: smallvec::SmallVec<[Value; 8]> = (0..*field_count)
                        .map(|i| {
                            #[allow(clippy::cast_possible_wrap)]
                            Value::Integer(i as i64)
                        })
                        .collect();
                    let id = fb.assert_ordered(rel, fields);
                    let ts = fb.get(id).unwrap().timestamp;
                    model.live.insert(
                        id,
                        FactKind::Ordered {
                            relation_idx: *relation_idx,
                        },
                    );
                    model.timestamps.push(ts);
                    live_ids.push(id);
                }
                Op::AssertTemplate { template_idx } => {
                    let tid = template_ids[*template_idx];
                    let id = fb.assert_template(tid, Box::new([]));
                    let ts = fb.get(id).unwrap().timestamp;
                    model.live.insert(
                        id,
                        FactKind::Template {
                            template_idx: *template_idx,
                        },
                    );
                    model.timestamps.push(ts);
                    live_ids.push(id);
                }
                Op::Retract { idx } => {
                    if live_ids.is_empty() {
                        continue;
                    }
                    let pick = idx % live_ids.len();
                    let id = live_ids.swap_remove(pick);
                    fb.retract(id);
                    model.live.remove(&id);
                }
            }
        }

        (fb, model, relations, template_ids, live_ids)
    }

    proptest! {
        // Invariant 1: Timestamps are strictly monotonic across assertions,
        // even when retractions occur between them.
        #[test]
        fn timestamps_are_strictly_monotonic_across_retractions(
            (n_relations, n_templates, ops) in scenario_strategy()
        ) {
            let mut table = SymbolTable::new();
            let relations: Vec<Symbol> = (0..n_relations)
                .map(|i| table.intern_symbol(&format!("r{i}"), StringEncoding::Ascii).unwrap())
                .collect();
            let mut tmpl_map: SlotMap<TemplateId, ()> = SlotMap::with_key();
            let template_ids: Vec<TemplateId> =
                (0..n_templates).map(|_| tmpl_map.insert(())).collect();

            let mut fb = FactBase::new();
            let mut live_ids: Vec<FactId> = Vec::new();
            let mut last_ts: Option<Timestamp> = None;

            for op in &ops {
                match op {
                    Op::AssertOrdered { relation_idx, field_count } => {
                        let fields: smallvec::SmallVec<[Value; 8]> =
                            (0..*field_count).map(|i| {
                                #[allow(clippy::cast_possible_wrap)]
                                Value::Integer(i as i64)
                            }).collect();
                        let id = fb.assert_ordered(relations[*relation_idx], fields);
                        let ts = fb.get(id).unwrap().timestamp;
                        if let Some(prev) = last_ts {
                            prop_assert!(ts > prev, "timestamp did not advance: prev={prev:?}, current={ts:?}");
                        }
                        last_ts = Some(ts);
                        live_ids.push(id);
                    }
                    Op::AssertTemplate { template_idx } => {
                        let id = fb.assert_template(template_ids[*template_idx], Box::new([]));
                        let ts = fb.get(id).unwrap().timestamp;
                        if let Some(prev) = last_ts {
                            prop_assert!(ts > prev, "timestamp did not advance: prev={prev:?}, current={ts:?}");
                        }
                        last_ts = Some(ts);
                        live_ids.push(id);
                    }
                    Op::Retract { idx } => {
                        if live_ids.is_empty() { continue; }
                        let pick = idx % live_ids.len();
                        let id = live_ids.swap_remove(pick);
                        fb.retract(id);
                    }
                }
            }
        }

        // Invariant 2: Relation index consistency across multiple relations.
        #[test]
        fn relation_index_consistent_across_multiple_relations(
            (n_relations, n_templates, ops) in scenario_strategy()
        ) {
            let (fb, model, relations, _tmpl, _live) =
                run_scenario(n_relations, n_templates, &ops);

            for (rel_idx, &rel) in relations.iter().enumerate() {
                let actual: HashSet<FactId> = fb.iter()
                    .filter_map(|(id, entry)| match &entry.fact {
                        Fact::Ordered(o) if o.relation == rel => Some(id),
                        _ => None,
                    })
                    .collect();
                let indexed: HashSet<FactId> = fb.facts_by_relation(rel).collect();
                let expected = model.ids_for_relation(rel_idx);

                prop_assert_eq!(&indexed, &actual,
                    "relation index for rel{} disagrees with primary storage", rel_idx);
                prop_assert_eq!(&actual, &expected,
                    "primary storage for rel{} disagrees with shadow model", rel_idx);
            }
        }

        // Invariant 3: Template index consistency across multiple templates.
        #[test]
        fn template_index_consistent_across_multiple_templates(
            (n_relations, n_templates, ops) in scenario_strategy()
        ) {
            let (fb, model, _rels, template_ids, _live) =
                run_scenario(n_relations, n_templates, &ops);

            for (tmpl_idx, &tid) in template_ids.iter().enumerate() {
                let actual: HashSet<FactId> = fb.iter()
                    .filter_map(|(id, entry)| match &entry.fact {
                        Fact::Template(t) if t.template_id == tid => Some(id),
                        _ => None,
                    })
                    .collect();
                let indexed: HashSet<FactId> = fb.facts_by_template(tid).collect();
                let expected = model.ids_for_template(tmpl_idx);

                prop_assert_eq!(&indexed, &actual,
                    "template index for template{} disagrees with primary storage", tmpl_idx);
                prop_assert_eq!(&actual, &expected,
                    "primary storage for template{} disagrees with shadow model", tmpl_idx);
            }
        }

        // Invariant 4: len() == iter().count() after every operation.
        #[test]
        fn len_consistent_with_iter_after_every_operation(
            (n_relations, n_templates, ops) in scenario_strategy()
        ) {
            let mut table = SymbolTable::new();
            let relations: Vec<Symbol> = (0..n_relations)
                .map(|i| table.intern_symbol(&format!("r{i}"), StringEncoding::Ascii).unwrap())
                .collect();
            let mut tmpl_map: SlotMap<TemplateId, ()> = SlotMap::with_key();
            let template_ids: Vec<TemplateId> =
                (0..n_templates).map(|_| tmpl_map.insert(())).collect();

            let mut fb = FactBase::new();
            let mut live_ids: Vec<FactId> = Vec::new();

            for op in &ops {
                match op {
                    Op::AssertOrdered { relation_idx, field_count } => {
                        let fields: smallvec::SmallVec<[Value; 8]> =
                            (0..*field_count).map(|i| {
                                #[allow(clippy::cast_possible_wrap)]
                                Value::Integer(i as i64)
                            }).collect();
                        let id = fb.assert_ordered(relations[*relation_idx], fields);
                        live_ids.push(id);
                    }
                    Op::AssertTemplate { template_idx } => {
                        let id = fb.assert_template(template_ids[*template_idx], Box::new([]));
                        live_ids.push(id);
                    }
                    Op::Retract { idx } => {
                        if live_ids.is_empty() { continue; }
                        let pick = idx % live_ids.len();
                        let id = live_ids.swap_remove(pick);
                        fb.retract(id);
                    }
                }
                let iter_count = fb.iter().count();
                prop_assert_eq!(fb.len(), iter_count,
                    "len()={} but iter().count()={}", fb.len(), iter_count);
            }
        }

        // Invariant 5: get() roundtrip for every asserted fact, and retracted
        // facts become unreachable.
        #[test]
        fn get_roundtrip_for_every_asserted_fact(
            (n_relations, n_templates, ops) in scenario_strategy()
        ) {
            let mut table = SymbolTable::new();
            let relations: Vec<Symbol> = (0..n_relations)
                .map(|i| table.intern_symbol(&format!("r{i}"), StringEncoding::Ascii).unwrap())
                .collect();
            let mut tmpl_map: SlotMap<TemplateId, ()> = SlotMap::with_key();
            let template_ids: Vec<TemplateId> =
                (0..n_templates).map(|_| tmpl_map.insert(())).collect();

            let mut fb = FactBase::new();
            let mut live_ids: Vec<FactId> = Vec::new();
            let mut retracted_ids: Vec<FactId> = Vec::new();

            for op in &ops {
                match op {
                    Op::AssertOrdered { relation_idx, field_count } => {
                        let fields: smallvec::SmallVec<[Value; 8]> =
                            (0..*field_count).map(|i| {
                                #[allow(clippy::cast_possible_wrap)]
                                Value::Integer(i as i64)
                            }).collect();
                        let id = fb.assert_ordered(relations[*relation_idx], fields);
                        let entry = fb.get(id);
                        prop_assert!(entry.is_some(), "get() returned None right after assert_ordered");
                        prop_assert_eq!(entry.unwrap().id, id);
                        live_ids.push(id);
                    }
                    Op::AssertTemplate { template_idx } => {
                        let id = fb.assert_template(template_ids[*template_idx], Box::new([]));
                        let entry = fb.get(id);
                        prop_assert!(entry.is_some(), "get() returned None right after assert_template");
                        prop_assert_eq!(entry.unwrap().id, id);
                        live_ids.push(id);
                    }
                    Op::Retract { idx } => {
                        if live_ids.is_empty() { continue; }
                        let pick = idx % live_ids.len();
                        let id = live_ids.swap_remove(pick);
                        fb.retract(id);
                        retracted_ids.push(id);
                    }
                }
            }

            for id in &retracted_ids {
                prop_assert!(fb.get(*id).is_none(), "get() returned Some for a retracted FactId");
            }
            for id in &live_ids {
                let entry = fb.get(*id);
                prop_assert!(entry.is_some(), "get() returned None for a live FactId");
                prop_assert_eq!(entry.unwrap().id, *id);
            }
        }

        // Invariant 6: Retract idempotency — retracting a non-existent ID
        // returns None and doesn't change len().
        #[test]
        fn retract_idempotent_on_already_retracted_ids(
            (n_relations, n_templates, ops) in scenario_strategy()
        ) {
            let (mut fb, _model, _rels, _tmpl, live_ids) =
                run_scenario(n_relations, n_templates, &ops);

            for &id in &live_ids {
                let before_len = fb.len();
                let first = fb.retract(id);
                prop_assert!(first.is_some());
                prop_assert_eq!(fb.len(), before_len - 1);

                let second = fb.retract(id);
                prop_assert!(second.is_none(), "second retract returned Some");
                prop_assert_eq!(fb.len(), before_len - 1, "len changed after idempotent retract");
            }
        }

        // Invariant 7: Empty index pruning — when all facts for a relation/template
        // are retracted, the index entry is removed.
        #[test]
        fn index_entries_pruned_when_all_facts_retracted(
            (n_relations, n_templates, ops) in scenario_strategy()
        ) {
            let (fb, model, relations, template_ids, _live) =
                run_scenario(n_relations, n_templates, &ops);

            for (rel_idx, &rel) in relations.iter().enumerate() {
                let model_set = model.ids_for_relation(rel_idx);
                if model_set.is_empty() {
                    prop_assert!(!fb.by_relation.contains_key(&rel),
                        "by_relation still has entry for rel{rel_idx} with no live facts");
                } else {
                    prop_assert!(fb.by_relation.contains_key(&rel),
                        "by_relation missing entry for rel{rel_idx} that has live facts");
                }
            }

            for (tmpl_idx, &tid) in template_ids.iter().enumerate() {
                let model_set = model.ids_for_template(tmpl_idx);
                if model_set.is_empty() {
                    prop_assert!(!fb.by_template.contains_key(&tid),
                        "by_template still has entry for template{tmpl_idx} with no live facts");
                } else {
                    prop_assert!(fb.by_template.contains_key(&tid),
                        "by_template missing entry for template{tmpl_idx} that has live facts");
                }
            }
        }

        // Invariant 8: Mixed ordered/template operations never cause
        // cross-contamination between the two index types.
        #[test]
        fn mixed_operations_keep_both_indices_consistent(
            (n_relations, n_templates, ops) in scenario_strategy()
        ) {
            let (fb, _model, _rels, _tmpl, _live) =
                run_scenario(n_relations, n_templates, &ops);

            for (id, entry) in fb.iter() {
                match &entry.fact {
                    Fact::Ordered(o) => {
                        let in_rel = fb.by_relation.get(&o.relation).is_some_and(|s| s.contains(&id));
                        prop_assert!(in_rel, "ordered fact {id:?} absent from by_relation");
                        let in_tmpl = fb.by_template.values().any(|s| s.contains(&id));
                        prop_assert!(!in_tmpl, "ordered fact {id:?} incorrectly in by_template");
                    }
                    Fact::Template(t) => {
                        let in_tmpl = fb.by_template.get(&t.template_id).is_some_and(|s| s.contains(&id));
                        prop_assert!(in_tmpl, "template fact {id:?} absent from by_template");
                        let in_rel = fb.by_relation.values().any(|s| s.contains(&id));
                        prop_assert!(!in_rel, "template fact {id:?} incorrectly in by_relation");
                    }
                }
            }
        }

        // Invariant 9: All timestamps across live facts are unique.
        #[test]
        fn timestamps_are_unique_across_all_live_facts(
            (n_relations, n_templates, ops) in scenario_strategy()
        ) {
            let (fb, _model, _rels, _tmpl, _live) =
                run_scenario(n_relations, n_templates, &ops);

            let mut seen: HashSet<u64> = HashSet::default();
            for (_id, entry) in fb.iter() {
                let raw = entry.timestamp.get();
                prop_assert!(seen.insert(raw), "duplicate timestamp {raw} found");
            }
        }

        // Invariant: Total index cardinality equals len().
        #[test]
        fn total_index_cardinality_equals_len(
            (n_relations, n_templates, ops) in scenario_strategy()
        ) {
            let (fb, _model, _rels, _tmpl, _live) =
                run_scenario(n_relations, n_templates, &ops);

            let mut from_rel: HashSet<FactId> = HashSet::default();
            for set in fb.by_relation.values() {
                from_rel.extend(set.iter().copied());
            }
            let mut from_tmpl: HashSet<FactId> = HashSet::default();
            for set in fb.by_template.values() {
                from_tmpl.extend(set.iter().copied());
            }

            let cross: HashSet<_> = from_rel.intersection(&from_tmpl).collect();
            prop_assert!(cross.is_empty(), "same FactId in both by_relation and by_template");

            let all_indexed: HashSet<FactId> = from_rel.union(&from_tmpl).copied().collect();
            let all_live: HashSet<FactId> = fb.iter().map(|(id, _)| id).collect();
            let live_len = all_live.len();
            prop_assert_eq!(all_indexed, all_live);
            prop_assert_eq!(live_len, fb.len());
        }

        // Assert-then-retract is a net no-op on len.
        #[test]
        fn assert_then_retract_is_net_noop_on_len(
            (n_relations, n_templates, ops) in scenario_strategy()
        ) {
            let (mut fb, _model, relations, template_ids, _live) =
                run_scenario(n_relations, n_templates, &ops);

            let baseline_len = fb.len();
            let rel = relations[0];
            let oid = fb.assert_ordered(rel, smallvec::smallvec![]);
            prop_assert_eq!(fb.len(), baseline_len + 1);
            fb.retract(oid);
            prop_assert_eq!(fb.len(), baseline_len);

            let tid = template_ids[0];
            let tmpl_id = fb.assert_template(tid, Box::new([]));
            prop_assert_eq!(fb.len(), baseline_len + 1);
            fb.retract(tmpl_id);
            prop_assert_eq!(fb.len(), baseline_len);
        }
    }
}
