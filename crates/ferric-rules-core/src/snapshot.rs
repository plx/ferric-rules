//! Fallible invariant checks shared by snapshot restoration and debug assertions.

macro_rules! require {
    ($condition:expr $(,)?) => {
        if !$condition { return Err(stringify!($condition).to_owned()); }
    };
    ($condition:expr, $($message:tt)+) => {
        if !$condition { return Err(format!($($message)+)); }
    };
}
pub(crate) use require;

macro_rules! require_eq {
    ($left:expr, $right:expr $(,)?) => {
        crate::snapshot::require!($left == $right, "inconsistent {} and {}", stringify!($left), stringify!($right))
    };
    ($left:expr, $right:expr, $($message:tt)+) => {
        crate::snapshot::require!($left == $right, $($message)+)
    };
}
pub(crate) use require_eq;

use crate::fact::{Fact, FactBase};
use crate::symbol::SymbolTable;
use crate::value::{AtomKey, Value};

fn validate_index_key_metadata(actual: &AtomKey, expected: &AtomKey) -> Result<(), String> {
    // Key equality deliberately ignores an address's cached public spelling.
    // Index rebuilds must compare that metadata against the source value too.
    if let (AtomKey::FactAddress(actual), AtomKey::FactAddress(expected)) = (actual, expected) {
        require!(
            actual.public_index() == expected.public_index(),
            "inconsistent fact address index metadata"
        );
    }
    Ok(())
}

impl SymbolTable {
    #[doc(hidden)]
    pub fn validate_snapshot(&self) -> Result<(), String> {
        require_eq!(self.ascii_strings.len(), self.ascii_to_id.len());
        require_eq!(self.utf8_strings.len(), self.utf8_to_id.len());
        for (index, text) in self.ascii_strings.iter().enumerate() {
            require!(text.is_ascii(), "non-ASCII entry in ASCII symbol pool");
            require!(
                self.ascii_to_id
                    .get(text)
                    .is_some_and(|id| *id as usize == index),
                "invalid ASCII symbol index"
            );
        }
        for (index, text) in self.utf8_strings.iter().enumerate() {
            require!(
                self.utf8_to_id
                    .get(text)
                    .is_some_and(|id| *id as usize == index),
                "invalid UTF-8 symbol index"
            );
        }
        Ok(())
    }

    #[doc(hidden)]
    pub fn validate_snapshot_value(&self, value: &Value) -> Result<(), String> {
        let mut pending = vec![(value, 0_usize)];
        let mut count = 0_usize;
        while let Some((value, depth)) = pending.pop() {
            count += 1;
            require!(
                count <= 1_000_000 && depth <= 32,
                "snapshot value limit exceeded"
            );
            match value {
                Value::ExternalAddress(_) => {
                    return Err("snapshot contains unsupported external identity".to_owned())
                }
                Value::Symbol(symbol) => require!(
                    self.resolve_symbol_str(*symbol).is_some(),
                    "dangling symbol in snapshot value"
                ),
                Value::InstanceName(name) => require!(
                    self.resolve_symbol_str(name.as_symbol()).is_some(),
                    "dangling instance name in snapshot value"
                ),
                Value::String(crate::string::FerricString::Ascii(bytes)) => {
                    require!(bytes.is_ascii(), "invalid ASCII string value");
                }
                Value::Multifield(fields) => {
                    pending.extend(fields.iter().map(|item| (item, depth + 1)));
                }
                _ => {}
            }
        }
        Ok(())
    }
}

impl FactBase {
    /// Check a rule-language address against persisted working-memory metadata.
    #[doc(hidden)]
    pub fn validate_snapshot_fact_address(
        &self,
        address: &crate::value::FactAddress,
        epoch: u64,
        initial_fact_id: Option<crate::fact::FactId>,
        zero_based: bool,
    ) -> Result<(), String> {
        use slotmap::Key;
        let Some(id) = address.fact_id() else {
            return Ok(());
        };
        let address_epoch = address.epoch().expect("real address has epoch");
        let timestamp = address.timestamp().expect("real address has timestamp");
        let index = address.public_index().expect("real address has index");
        require!(!id.is_null(), "null fact address key");
        require!(address_epoch <= epoch, "fact address has future epoch");
        require!(
            timestamp.get() < u64::MAX,
            "fact address has exhausted timestamp"
        );
        require!(
            index <= timestamp.get() + 1,
            "fact address index exceeds assertion timestamp"
        );
        require!(
            index == 0 || index == timestamp.get() || index == timestamp.get() + 1,
            "fact address has impossible historical public index"
        );
        if address_epoch == epoch {
            require!(
                timestamp < self.next_timestamp,
                "fact address has future timestamp"
            );
            let expected = if initial_fact_id == Some(id) {
                let initial = self
                    .get(id)
                    .ok_or("fact address has missing initial fact")?;
                require!(
                    initial.timestamp == timestamp,
                    "fact address has inconsistent initial timestamp"
                );
                0
            } else if zero_based
                || initial_fact_id
                    .and_then(|id| self.get(id))
                    .is_some_and(|initial| initial.timestamp < timestamp)
            {
                timestamp.get()
            } else {
                timestamp.get() + 1
            };
            require!(
                index == expected,
                "fact address has inconsistent public index"
            );
            if let Some(entry) = self.get(id) {
                require!(
                    timestamp <= entry.timestamp,
                    "fact address precedes key allocation"
                );
            }
        }
        Ok(())
    }

    #[doc(hidden)]
    pub fn validate_snapshot(&self, symbols: &SymbolTable) -> Result<(), String> {
        // MAX is the exhausted sentinel. Checked insertion preserves this state
        // for reads, retraction, persistence, and reset instead of wrapping.
        let mut timestamps = rustc_hash::FxHashSet::default();
        let mut by_template = rustc_hash::FxHashMap::default();
        let mut by_relation = rustc_hash::FxHashMap::default();
        for (id, entry) in &self.facts {
            require_eq!(id, entry.id);
            require!(
                entry.timestamp < self.next_timestamp,
                "fact timestamp exceeds next timestamp"
            );
            require!(
                timestamps.insert(entry.timestamp),
                "duplicate fact timestamp"
            );
            let values = match &entry.fact {
                Fact::Ordered(fact) => {
                    require!(
                        symbols.resolve_symbol_str(fact.relation).is_some(),
                        "dangling ordered relation"
                    );
                    by_relation
                        .entry(fact.relation)
                        .or_insert_with(rustc_hash::FxHashSet::default)
                        .insert(id);
                    fact.fields.as_slice()
                }
                Fact::Template(fact) => {
                    by_template
                        .entry(fact.template_id)
                        .or_insert_with(rustc_hash::FxHashSet::default)
                        .insert(id);
                    fact.slots.as_ref()
                }
            };
            for value in values {
                symbols.validate_snapshot_value(value)?;
            }
        }
        require!(
            by_template == self.by_template,
            "inconsistent template fact index"
        );
        let mut actual_relations = rustc_hash::FxHashMap::default();
        for (pool, ascii) in [
            (&self.by_relation.ascii, true),
            (&self.by_relation.utf8, false),
        ] {
            for (index, ids) in pool.iter().enumerate() {
                if let Some(ids) = ids {
                    let index = u32::try_from(index).map_err(|_| "oversized relation index")?;
                    let symbol = crate::symbol::Symbol(if ascii {
                        crate::symbol::SymbolId::Ascii(index)
                    } else {
                        crate::symbol::SymbolId::Utf8(index)
                    });
                    require!(!ids.is_empty(), "empty ordered fact index");
                    actual_relations.insert(symbol, ids.clone());
                }
            }
        }
        require!(
            by_relation == actual_relations,
            "inconsistent ordered fact index"
        );
        Ok(())
    }
}

use crate::alpha::{AlphaEntryType, AlphaMemory, AlphaMemoryId, AlphaNode, ConstantTestType};
use crate::beta::{BetaNode, JoinTest, RuleId};
use crate::binding::VarMap;
use crate::rete::ReteNetwork;
use crate::sequence::{SequencePattern, SplitEvent};
use crate::token::NodeId;
use std::ops::ControlFlow;

// Source compilation allows 64 total condition nodes and 64 alpha value tests,
// with one additional ordered field-count test per alpha path.
// Each condition contributes at most one node to a beta parent chain; an NCC
// partner substitutes for its wrapper on a subnetwork path. Include root and
// terminal. Partner callbacks need their own nesting/cycle bound below.
const MAX_ALPHA_DEPTH: usize = 64;
const MAX_BETA_PATH_NODES: usize = 66;
const MAX_NCC_DEPTH: usize = 4;

impl VarMap {
    #[doc(hidden)]
    pub fn validate_snapshot(&self, symbols: &SymbolTable) -> Result<(), String> {
        require_eq!(self.by_id.len(), self.by_name.len());
        require!(self.by_id.len() <= 65_536, "too many rule variables");
        for (index, symbol) in self.by_id.iter().enumerate() {
            require!(
                symbols.resolve_symbol_str(*symbol).is_some(),
                "invalid variable symbol"
            );
            require!(
                self.by_name
                    .get(symbol)
                    .is_some_and(|id| usize::from(id.0) == index),
                "inconsistent variable map"
            );
        }
        Ok(())
    }
}

/// Work bound for graph membership validation (including negative/exists joins).
/// A large but valid engine may exceed this supported snapshot boundary.
struct Work(usize);
impl Work {
    fn step(&mut self) -> Result<(), String> {
        self.spend(1)
    }

    fn spend(&mut self, count: usize) -> Result<(), String> {
        self.0 = self
            .0
            .checked_sub(count)
            .ok_or("snapshot validation work limit exceeded")?;
        Ok(())
    }
}

/// Work to copy every value of `fact`, counting string bytes and nested
/// multifields. It bounds copying or comparing any capture of the fact.
fn fact_value_cost(fact: &Fact, work: &mut Work) -> Result<usize, String> {
    let values = match fact {
        Fact::Ordered(fact) => fact.fields.as_slice(),
        Fact::Template(fact) => fact.slots.as_ref(),
    };
    let mut value_cost = 0_usize;
    let mut pending = vec![values];
    while let Some(values) = pending.pop() {
        for value in values {
            let cost = match value {
                Value::String(text) => text.as_bytes().len().saturating_add(1),
                Value::Multifield(fields) => {
                    pending.push(fields.as_slice());
                    1
                }
                _ => 1,
            };
            work.spend(cost)?;
            value_cost = value_cost.saturating_add(cost);
        }
    }
    Ok(value_cost)
}

/// Work to traverse a constant test, including every alternative.
fn constant_test_cost(test: &crate::alpha::ConstantTest) -> usize {
    let mut pending = vec![test];
    let mut cost = 0_usize;
    while let Some(test) = pending.pop() {
        cost = cost.saturating_add(1);
        match &test.test_type {
            ConstantTestType::EqualAny(values) => cost = cost.saturating_add(values.len()),
            ConstantTestType::Any(branches) => {
                cost = cost.saturating_add(branches.len());
                pending.extend(branches.iter().flatten());
            }
            ConstantTestType::Sequence(plan) => {
                cost = cost.saturating_add(plan.logical_width() + plan.segments.len());
                pending.extend(&plan.tests);
            }
            _ => {}
        }
    }
    cost
}

/// Work to view one split and run its tests, before any capture is copied.
fn split_cost(sequence: &SequencePattern, tests: &[JoinTest]) -> usize {
    sequence.tests.iter().fold(
        sequence.logical_width() + sequence.segments.len() + tests.len() + 1,
        |cost, test| cost.saturating_add(constant_test_cost(test)),
    )
}

/// Whether some split of `fact` satisfies the plan and the join tests.
/// The search is combinatorial, so every capture length it tries is
/// charged before it runs, including those its tests reject; each step of a
/// plan whose tests read a capture is also charged for copying and comparing
/// one, as is a split whose join tests copy one.
fn any_sequence_match(
    fact: &Fact,
    bindings: Option<&crate::binding::BindingSet>,
    tests: &[JoinTest],
    sequence: &SequencePattern,
    work: &mut Work,
) -> Result<bool, String> {
    let value_cost = fact_value_cost(fact, work)?;
    let split_cost = split_cost(sequence, tests);
    let copy_cost = value_cost.saturating_mul(2);
    let step_cost = if sequence_tests_read_captures(sequence) {
        split_cost.saturating_add(copy_cost)
    } else {
        split_cost
    };
    work.spend(split_cost)?;
    let search = sequence.search(fact, &mut |event| {
        let charged = match event {
            SplitEvent::Step => work.spend(step_cost),
            SplitEvent::Match(split) => {
                let matched =
                    crate::rete::evaluate_join_fields(|slot| split.get(slot), bindings, tests);
                match work.spend(if split.copied_capture() { copy_cost } else { 0 }) {
                    Ok(()) if matched => return ControlFlow::Break(Ok(true)),
                    charged => charged,
                }
            }
        };
        match charged {
            Ok(()) => ControlFlow::Continue(()),
            Err(error) => ControlFlow::Break(Err(error)),
        }
    });
    match search {
        ControlFlow::Break(result) => result,
        ControlFlow::Continue(()) => Ok(false),
    }
}

/// Whether any test of the plan reads a capture, which copies it.
fn sequence_tests_read_captures(sequence: &SequencePattern) -> bool {
    let fields: Vec<_> = sequence
        .segments
        .iter()
        .flat_map(|segment| &segment.fields)
        .collect();
    let capture = |slot: crate::alpha::SlotIndex| {
        let (crate::alpha::SlotIndex::Ordered(index) | crate::alpha::SlotIndex::Template(index)) =
            slot;
        fields
            .get(index)
            .is_some_and(|field| **field == crate::sequence::SequenceField::Multi)
    };
    let mut pending: Vec<_> = sequence.tests.iter().collect();
    while let Some(test) = pending.pop() {
        if let ConstantTestType::Any(branches) = &test.test_type {
            pending.extend(branches.iter().flatten());
        }
        if capture(test.slot)
            || matches!(
                test.test_type,
                ConstantTestType::EqualSlot(slot)
                    | ConstantTestType::NotEqualSlot(slot)
                    | ConstantTestType::EqualSlotOffset(slot, _)
                    | ConstantTestType::NotEqualSlotOffset(slot, _)
                    | ConstantTestType::GreaterThanSlotOffset(slot, _)
                    | ConstantTestType::LessThanSlotOffset(slot, _)
                    | ConstantTestType::GreaterOrEqualSlotOffset(slot, _)
                    | ConstantTestType::LessOrEqualSlotOffset(slot, _)
                    if capture(slot)
            )
        {
            return true;
        }
    }
    false
}

/// Bindings a join token must carry for the fields of its fact or split.
fn join_bindings<'v>(
    parent: &crate::token::Token,
    field: impl Fn(crate::alpha::SlotIndex) -> Option<&'v Value>,
    bindings: &[(crate::alpha::SlotIndex, crate::binding::VarId)],
) -> crate::binding::BindingSet {
    let mut expected = parent.bindings.clone();
    for &(slot, variable) in bindings {
        if let Some(value) = field(slot) {
            expected.set(variable, crate::binding::ValueRef::new(value.clone()));
        }
    }
    expected
}

fn validate_sequence_plan(
    sequence: &SequencePattern,
    tests: &[JoinTest],
    bindings: &[(crate::alpha::SlotIndex, crate::binding::VarId)],
    symbols: &SymbolTable,
    work: &mut Work,
) -> Result<(), String> {
    work.spend(split_cost(sequence, tests).saturating_add(bindings.len()))?;
    sequence.validate()?;
    let valid_slot = sequence.logical_slot_validator();
    require!(
        tests.iter().all(|test| valid_slot(test.alpha_slot))
            && bindings.iter().all(|(slot, _)| valid_slot(*slot)),
        "sequence join has an invalid logical field"
    );
    for test in &sequence.tests {
        validate_constant(test, symbols)?;
    }
    Ok(())
}

fn same_members<T: Eq + std::hash::Hash>(actual: &[T], expected: &[T]) -> bool {
    let unique: rustc_hash::FxHashSet<_> = actual.iter().collect();
    actual.len() == expected.len()
        && unique.len() == actual.len()
        && expected.iter().all(|id| unique.contains(id))
}

fn same_bindings(left: &crate::binding::BindingSet, right: &crate::binding::BindingSet) -> bool {
    left.capacity() == right.capacity()
        && left
            .bindings
            .iter()
            .zip(&right.bindings)
            .all(|(a, b)| match (a, b) {
                (Some(a), Some(b)) => a.structural_eq(b),
                (None, None) => true,
                _ => false,
            })
}

fn validate_constant(
    test: &crate::alpha::ConstantTest,
    symbols: &SymbolTable,
) -> Result<(), String> {
    use crate::alpha::ConstantTestType as Test;
    require!(
        crate::alpha::constant_test_count(std::slice::from_ref(test)) <= MAX_ALPHA_DEPTH,
        "snapshot constant constraint exceeds 64 tests"
    );
    let mut pending = vec![(test, false)];
    while let Some((test, nested)) = pending.pop() {
        match &test.test_type {
            Test::Equal(value)
            | Test::NotEqual(value)
            | Test::GreaterThan(value)
            | Test::LessThan(value)
            | Test::GreaterOrEqual(value)
            | Test::LessOrEqual(value) => {
                require!(
                    !matches!(value, crate::value::AtomKey::FactAddress(_)),
                    "fact address cannot be a source constant"
                );
                symbols.validate_snapshot_value(&value.to_value())?;
            }
            Test::EqualAny(values) => {
                for value in values {
                    require!(
                        !matches!(value, crate::value::AtomKey::FactAddress(_)),
                        "fact address cannot be a source constant"
                    );
                    symbols.validate_snapshot_value(&value.to_value())?;
                }
            }
            Test::Any(branches) => {
                pending.extend(branches.iter().flatten().map(|test| (test, true)));
            }
            Test::Sequence(plan) if !nested => {
                plan.validate()?;
                pending.extend(plan.tests.iter().map(|test| (test, true)));
            }
            Test::OrderedFieldCount { .. } | Test::Sequence(_) if nested => {
                return Err("field disjunction contains a whole-fact constraint".to_string());
            }
            _ => {}
        }
    }
    Ok(())
}

fn children(node: &BetaNode) -> &[NodeId] {
    match node {
        BetaNode::Root { children, .. }
        | BetaNode::Join { children, .. }
        | BetaNode::Predicate { children, .. }
        | BetaNode::Negative { children, .. }
        | BetaNode::Ncc { children, .. }
        | BetaNode::Exists { children, .. } => children,
        BetaNode::Terminal { .. } | BetaNode::NccPartner { .. } => &[],
    }
}
fn parent(node: &BetaNode) -> Option<NodeId> {
    match node {
        BetaNode::Root { .. } => None,
        BetaNode::Join { parent, .. }
        | BetaNode::Predicate { parent, .. }
        | BetaNode::Terminal { parent, .. }
        | BetaNode::Negative { parent, .. }
        | BetaNode::Ncc { parent, .. }
        | BetaNode::NccPartner { parent, .. }
        | BetaNode::Exists { parent, .. } => Some(*parent),
    }
}

impl ReteNetwork {
    /// Visit persisted binding values after structural graph validation.
    #[doc(hidden)]
    pub fn validate_snapshot_binding_values(
        &self,
        validate: impl Fn(&Value) -> Result<(), String>,
    ) -> Result<(), String> {
        for token in self.token_store.tokens.values() {
            for value in token.bindings.bindings.iter().flatten() {
                validate(value)?;
            }
        }
        Ok(())
    }

    /// Validate persisted graph identities, runtime memberships, and reverse indexes.
    #[doc(hidden)]
    #[allow(clippy::too_many_lines)]
    pub fn validate_snapshot(&self, facts: &FactBase, symbols: &SymbolTable) -> Result<(), String> {
        self.validate_consistency()?;
        self.validate_block_orders()?;
        require!(
            self.pending_ncc_results.is_none(),
            "snapshot has unfinished NCC results"
        );
        require!(
            self.pending_events.is_empty(),
            "snapshot has unfinished runtime events"
        );
        let mut work = Work(10_000_000);
        self.validate_alpha_snapshot(facts, symbols, &mut work)?;
        let alpha_entries = self.alpha_memory_entry_types(&mut work)?;

        // MAX is a valid exhausted sentinel; rule loading checks capacity before
        // reclaiming or installing anything. Existing graph IDs remain below it.
        let root = self.beta.root_id;
        require!(
            matches!(self.beta.nodes.get(&root), Some(BetaNode::Root { .. })),
            "missing beta root"
        );
        let mut owned_memories = rustc_hash::FxHashSet::default();
        let mut owned_negative = rustc_hash::FxHashSet::default();
        let mut owned_exists = rustc_hash::FxHashSet::default();
        let mut owned_ncc = rustc_hash::FxHashSet::default();
        let mut joins = rustc_hash::FxHashMap::<_, Vec<_>>::default();
        let mut negatives = rustc_hash::FxHashMap::<_, Vec<_>>::default();
        let mut existentials = rustc_hash::FxHashMap::<_, Vec<_>>::default();
        for (&id, node) in &self.beta.nodes {
            work.step()?;
            require!(
                id.0 < self.beta.next_node_id,
                "beta node exceeds allocation counter"
            );
            if let Some(parent_id) = parent(node) {
                let parent_node = self
                    .beta
                    .nodes
                    .get(&parent_id)
                    .ok_or("dangling beta parent")?;
                require!(
                    children(parent_node).contains(&id),
                    "beta parent lacks reciprocal child"
                );
                let mut ancestor = Some(parent_id);
                let mut seen = rustc_hash::FxHashSet::default();
                seen.insert(id);
                while let Some(next) = ancestor {
                    work.step()?;
                    require!(seen.insert(next), "cyclic beta graph");
                    require!(
                        seen.len() <= MAX_BETA_PATH_NODES,
                        "snapshot beta path exceeds 66 nodes"
                    );
                    ancestor = parent(self.beta.nodes.get(&next).ok_or("dangling beta ancestor")?);
                }
            } else {
                require_eq!(id, root);
            }
            require!(
                children(node)
                    .iter()
                    .collect::<rustc_hash::FxHashSet<_>>()
                    .len()
                    == children(node).len(),
                "duplicate beta child"
            );
            for child in children(node) {
                require!(
                    self.beta.nodes.get(child).and_then(parent) == Some(id),
                    "beta child has wrong parent"
                );
            }
            match node {
                BetaNode::Join {
                    sequence: Some(sequence),
                    alpha_memory,
                    tests,
                    bindings,
                    ..
                } => {
                    validate_sequence_plan(sequence, tests, bindings, symbols, &mut work)?;
                    let entry = alpha_entries
                        .get(alpha_memory)
                        .ok_or("sequence plan has no alpha source")?;
                    require!(
                        sequence.is_ordered()
                            == matches!(entry, AlphaEntryType::OrderedRelation(_)),
                        "sequence plan and alpha source have different fact kinds"
                    );
                }
                BetaNode::Negative {
                    sequence: Some(sequence),
                    alpha_memory,
                    tests,
                    ..
                }
                | BetaNode::Exists {
                    sequence: Some(sequence),
                    alpha_memory,
                    tests,
                    ..
                } => {
                    validate_sequence_plan(sequence, tests, &[], symbols, &mut work)?;
                    let entry = alpha_entries
                        .get(alpha_memory)
                        .ok_or("sequence plan has no alpha source")?;
                    require!(
                        sequence.is_ordered()
                            == matches!(entry, AlphaEntryType::OrderedRelation(_)),
                        "sequence plan and alpha source have different fact kinds"
                    );
                }
                _ => {}
            }
            if let Some(memory) = self.beta.memory_id_for_node(id) {
                require!(
                    owned_memories.insert(memory),
                    "beta memory has multiple owners"
                );
                let memory = self.beta.get_memory(memory).ok_or("missing beta memory")?;
                for token_id in &memory.tokens {
                    let token = self
                        .token_store
                        .get(*token_id)
                        .ok_or("dangling beta token")?;
                    require_eq!(token.owner_node, id);
                }
                let mut rebuilt = crate::beta::BetaMemory::new(memory.id);
                require!(
                    memory
                        .indexed_vars
                        .iter()
                        .collect::<rustc_hash::FxHashSet<_>>()
                        .len()
                        == memory.indexed_vars.len(),
                    "duplicate indexed beta variable"
                );
                work.spend(
                    memory
                        .tokens
                        .len()
                        .checked_mul(memory.indexed_vars.len() + 1)
                        .ok_or("snapshot validation work overflow")?,
                )?;
                for variable in &memory.indexed_vars {
                    rebuilt.request_var_index_empty(*variable);
                }
                for token_id in &memory.tokens {
                    rebuilt.insert_indexed(
                        *token_id,
                        &self
                            .token_store
                            .get(*token_id)
                            .ok_or("missing indexed token")?
                            .bindings,
                    );
                }
                require!(
                    memory.var_indices.len() == rebuilt.var_indices.len(),
                    "invalid beta variable index"
                );
                for (variable, keys) in &memory.var_indices {
                    let expected = rebuilt
                        .var_indices
                        .get(variable)
                        .ok_or("unexpected beta variable index")?;
                    require_eq!(keys.len(), expected.len());
                    for (key, ids) in keys {
                        let (expected_key, expected) = expected
                            .get_key_value(key)
                            .ok_or("wrong beta binding key")?;
                        validate_index_key_metadata(key, expected_key)?;
                        require!(
                            ids == expected,
                            "inconsistent beta binding membership or order"
                        );
                    }
                }
            }
            match node {
                BetaNode::Negative { neg_memory, .. } => {
                    require!(owned_negative.insert(*neg_memory), "shared negative memory");
                }
                BetaNode::Exists { exists_memory, .. } => {
                    require!(owned_exists.insert(*exists_memory), "shared exists memory");
                }
                BetaNode::Ncc { ncc_memory, .. } => {
                    require!(owned_ncc.insert(*ncc_memory), "shared NCC memory");
                }
                _ => {}
            }
            match node {
                BetaNode::Join { alpha_memory, .. } => {
                    joins.entry(*alpha_memory).or_default().push(id);
                }
                BetaNode::Negative { alpha_memory, .. } => {
                    negatives.entry(*alpha_memory).or_default().push(id);
                }
                BetaNode::Exists { alpha_memory, .. } => {
                    existentials.entry(*alpha_memory).or_default().push(id);
                }
                _ => {}
            }
        }
        self.validate_ncc_paths(&mut work)?;
        // The compiler allocates parents before descendants and never reuses
        // beta IDs. Right-assert dispatch relies on this order. Check it after
        // path validation so corrupt cycles retain their specific diagnostics.
        for (&id, node) in &self.beta.nodes {
            work.step()?;
            if let Some(parent_id) = parent(node) {
                require!(parent_id.0 < id.0, "invalid beta allocation order");
            }
        }
        require_eq!(owned_memories.len(), self.beta.memories.len());
        require_eq!(owned_negative.len(), self.beta.neg_memories.len());
        require_eq!(owned_exists.len(), self.beta.exists_memories.len());
        require_eq!(owned_ncc.len(), self.beta.ncc_memories.len());
        for (index, memory) in self.beta.memories.iter().enumerate() {
            require_eq!(memory.id.0 as usize, index);
        }
        for (index, memory) in self.beta.neg_memories.iter().enumerate() {
            require_eq!(memory.id.0 as usize, index);
            memory.validate_consistency()?;
        }
        for (index, memory) in self.beta.ncc_memories.iter().enumerate() {
            require_eq!(memory.id.0 as usize, index);
            memory.validate_consistency()?;
        }
        for (index, memory) in self.beta.exists_memories.iter().enumerate() {
            require_eq!(memory.id.0 as usize, index);
            memory.validate_consistency()?;
        }
        require_eq!(self.beta.next_memory_id as usize, self.beta.memories.len());
        require_eq!(
            self.beta.next_neg_memory_id as usize,
            self.beta.neg_memories.len()
        );
        require_eq!(
            self.beta.next_ncc_memory_id as usize,
            self.beta.ncc_memories.len()
        );
        require_eq!(
            self.beta.next_exists_memory_id as usize,
            self.beta.exists_memories.len()
        );
        for (actual, expected) in [
            (&self.beta.alpha_to_joins, joins),
            (&self.beta.alpha_to_negatives, negatives),
            (&self.beta.alpha_to_exists, existentials),
        ] {
            require_eq!(actual.len(), expected.len());
            for (alpha, nodes) in actual {
                require!(
                    self.alpha.get_memory(*alpha).is_some(),
                    "dangling beta alpha memory"
                );
                let expected = expected.get(alpha).ok_or("unexpected alpha subscription")?;
                require!(
                    same_members(nodes, expected),
                    "invalid alpha subscription index"
                );
            }
        }
        for (id, lengths) in &self.token_store.sequence_matches {
            work.spend(lengths.len() + 1)?;
            let token = self
                .token_store
                .get(id)
                .ok_or("dangling sequence match token")?;
            require!(
                token.fact.is_some()
                    && matches!(
                        self.beta.nodes.get(&token.owner_node),
                        Some(BetaNode::Join {
                            sequence: Some(_),
                            ..
                        })
                    ),
                "sequence metadata belongs to a nonsequence token"
            );
        }
        for (fact, tokens) in &self.token_store.fact_to_tokens {
            for token in tokens {
                require!(
                    self.token_store
                        .get(*token)
                        .is_some_and(|token| token.fact == Some(*fact)),
                    "incorrect token fact reverse index"
                );
            }
        }
        for (parent, children) in &self.token_store.parent_to_children {
            for child in children {
                require!(
                    self.token_store
                        .get(*child)
                        .is_some_and(|token| token.parent == Some(*parent)),
                    "incorrect token parent reverse index"
                );
            }
        }
        let root_memory = self
            .beta
            .memory_id_for_node(root)
            .and_then(|id| self.beta.get_memory(id))
            .ok_or("missing root memory")?;
        require_eq!(root_memory.len(), 1);
        let mut passthrough_parents = rustc_hash::FxHashSet::default();
        for (id, token) in &self.token_store.tokens {
            work.step()?;
            let node = self
                .beta
                .nodes
                .get(&token.owner_node)
                .ok_or("dangling token owner")?;
            let memory = self
                .beta
                .memory_id_for_node(token.owner_node)
                .and_then(|id| self.beta.get_memory(id))
                .ok_or("token owner has no memory")?;
            require!(memory.contains(id), "token missing from owner memory");
            match (token.parent, parent(node)) {
                (None, None) => {
                    require!(
                        token.fact.is_none() && token.bindings.bound_count() == 0,
                        "invalid root token"
                    );
                }
                (Some(parent_id), Some(parent_node)) => {
                    let parent_token = self
                        .token_store
                        .get(parent_id)
                        .ok_or("dangling token parent")?;
                    require_eq!(parent_token.owner_node, parent_node);
                    if matches!(node, BetaNode::Predicate { .. }) {
                        self.validate_passthrough(parent_id, id, token.owner_node)?;
                    }
                }
                _ => return Err("invalid token ancestry".to_owned()),
            }
            if token.fact.is_none() {
                require!(
                    passthrough_parents.insert((token.owner_node, token.parent)),
                    "duplicate pass-through match"
                );
            }
            if let Some(fact) = token.fact {
                require!(facts.get(fact).is_some(), "dangling token fact");
            }
            require!(
                token.bindings.capacity() <= 65_536,
                "oversized token bindings"
            );
            for value in token.bindings.bindings.iter().flatten() {
                symbols.validate_snapshot_value(value)?;
            }
        }
        let mut activated_pairs = rustc_hash::FxHashSet::default();
        let terminals: rustc_hash::FxHashSet<_> = self
            .beta
            .nodes
            .values()
            .filter_map(|node| match node {
                BetaNode::Terminal {
                    parent,
                    rule,
                    salience,
                } => Some((*parent, *rule, *salience)),
                _ => None,
            })
            .collect();
        for activation in self.agenda.iter_activations() {
            let token = self
                .token_store
                .get(activation.token)
                .ok_or("dangling activation token")?;
            require!(
                activated_pairs.insert((activation.rule, activation.token)),
                "duplicate rule/token activation"
            );
            work.spend(MAX_BETA_PATH_NODES)?;
            let expected: smallvec::SmallVec<[crate::fact::Timestamp; 4]> = self
                .token_store
                .collect_all_facts(activation.token)
                .iter()
                .map(|id| {
                    facts
                        .get(*id)
                        .map(|entry| entry.timestamp)
                        .ok_or("dangling activation fact")
                })
                .collect::<Result<_, _>>()?;
            require!(
                activation.recency == expected
                    && activation.timestamp
                        == expected
                            .iter()
                            .max()
                            .copied()
                            .unwrap_or(crate::fact::Timestamp::ZERO),
                "invalid activation fact recency"
            );
            require!(
                !self.disabled_rules.contains(&activation.rule),
                "disabled rule has activation"
            );
            require!(
                terminals.contains(&(token.owner_node, activation.rule, activation.salience)),
                "activation does not match a terminal"
            );
        }
        self.validate_join_memberships(facts, &mut work)?;
        self.validate_conditional_memories(facts, &mut work)?;
        Ok(())
    }

    fn validate_ncc_paths(&self, work: &mut Work) -> Result<(), String> {
        let mut dependencies = rustc_hash::FxHashMap::<NodeId, Vec<NodeId>>::default();
        for (&id, node) in &self.beta.nodes {
            let BetaNode::Ncc {
                parent: prefix,
                partner,
                ..
            } = node
            else {
                continue;
            };
            let mut cursor = match self.beta.nodes.get(partner) {
                Some(BetaNode::NccPartner {
                    parent, ncc_node, ..
                }) if *ncc_node == id => *parent,
                _ => return Err("invalid NCC partner link".to_owned()),
            };
            require!(cursor != *prefix, "empty NCC partner branch");
            let nested = dependencies.entry(id).or_default();
            while cursor != *prefix {
                work.step()?;
                require!(cursor != id, "NCC partner crosses its own output");
                let ancestor = self.beta.nodes.get(&cursor).ok_or("dangling NCC branch")?;
                if matches!(ancestor, BetaNode::Ncc { .. }) {
                    nested.push(cursor);
                }
                cursor = parent(ancestor).ok_or("NCC branch does not share its declared prefix")?;
            }
        }
        // Parent paths can be short while partner callbacks form a deep chain
        // or a cycle. Check that separate dependency graph iteratively.
        let mut visiting = rustc_hash::FxHashSet::default();
        let mut depths = rustc_hash::FxHashMap::<NodeId, usize>::default();
        for &root in dependencies.keys() {
            let mut pending = vec![(root, false)];
            while let Some((id, exit)) = pending.pop() {
                work.step()?;
                if depths.contains_key(&id) {
                    continue;
                }
                let nested = dependencies.get(&id).ok_or("missing nested NCC")?;
                if exit {
                    let depth = 1 + nested.iter().map(|child| depths[child]).max().unwrap_or(0);
                    require!(depth <= MAX_NCC_DEPTH, "snapshot NCC nesting exceeds 4");
                    visiting.remove(&id);
                    depths.insert(id, depth);
                } else {
                    require!(visiting.insert(id), "cyclic NCC partner dependencies");
                    pending.push((id, true));
                    pending.extend(nested.iter().map(|child| (*child, false)));
                }
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn validate_alpha_snapshot(
        &self,
        facts: &FactBase,
        symbols: &SymbolTable,
        work: &mut Work,
    ) -> Result<(), String> {
        for memory in &self.alpha.memories {
            require!(
                memory
                    .indexed_slots
                    .iter()
                    .collect::<rustc_hash::FxHashSet<_>>()
                    .len()
                    == memory.indexed_slots.len(),
                "duplicate indexed alpha slot"
            );
        }
        require_eq!(self.alpha.next_node_id as usize, self.alpha.nodes.len());
        require_eq!(
            self.alpha.next_memory_id as usize,
            self.alpha.memories.len()
        );
        let mut entries = rustc_hash::FxHashMap::default();
        let mut owners = rustc_hash::FxHashSet::default();
        let mut incoming = vec![0_usize; self.alpha.nodes.len()];
        let mut depths = vec![0_usize; self.alpha.nodes.len()];
        let mut field_count_depths = vec![0_usize; self.alpha.nodes.len()];
        for (index, node) in self.alpha.nodes.iter().enumerate() {
            let id = NodeId(u32::try_from(index).map_err(|_| "oversized alpha graph")?);
            let (children, memory) = match node {
                AlphaNode::Entry {
                    entry_type,
                    children,
                    memory,
                } => {
                    if let AlphaEntryType::OrderedRelation(symbol) = entry_type {
                        require!(
                            symbols.resolve_symbol_str(*symbol).is_some(),
                            "dangling alpha relation"
                        );
                    }
                    require!(
                        entries.insert(entry_type.clone(), id).is_none(),
                        "duplicate alpha entry"
                    );
                    (children, memory)
                }
                AlphaNode::ConstantTest {
                    test,
                    children,
                    memory,
                } => {
                    validate_constant(test, symbols)?;
                    (children, memory)
                }
            };
            for child in children {
                require!(
                    child.0 as usize > index && (child.0 as usize) < self.alpha.nodes.len(),
                    "cyclic or dangling alpha child"
                );
                incoming[child.0 as usize] += 1;
                let field_count_test = matches!(
                    &self.alpha.nodes[child.0 as usize],
                    AlphaNode::ConstantTest { test, .. }
                        if matches!(test.test_type, ConstantTestType::OrderedFieldCount { .. })
                );
                let test_count = match &self.alpha.nodes[child.0 as usize] {
                    AlphaNode::ConstantTest { test, .. } if !field_count_test => {
                        crate::alpha::constant_test_count(std::slice::from_ref(test))
                    }
                    _ => 0,
                };
                depths[child.0 as usize] = depths[index].saturating_add(test_count);
                field_count_depths[child.0 as usize] =
                    field_count_depths[index] + usize::from(field_count_test);
                require!(
                    depths[child.0 as usize] <= MAX_ALPHA_DEPTH,
                    "snapshot alpha path exceeds 64 tests"
                );
                require!(
                    field_count_depths[child.0 as usize] <= 1,
                    "snapshot alpha path exceeds one ordered field-count test"
                );
            }
            if let Some(memory) = memory {
                require!(owners.insert(*memory), "alpha memory has multiple owners");
            }
        }
        require!(
            entries == self.alpha.entry_nodes,
            "inconsistent alpha entry index"
        );
        require_eq!(owners.len(), self.alpha.memories.len());
        for (index, node) in self.alpha.nodes.iter().enumerate() {
            require_eq!(
                incoming[index],
                usize::from(matches!(node, AlphaNode::ConstantTest { .. }))
            );
        }
        let mut expected: Vec<AlphaMemory> = self
            .alpha
            .memories
            .iter()
            .enumerate()
            .map(|(index, memory)| {
                let mut rebuilt = AlphaMemory::new(AlphaMemoryId(
                    u32::try_from(index).expect("bounded alpha memory count"),
                ));
                for slot in &memory.indexed_slots {
                    rebuilt.request_index_empty(*slot);
                }
                rebuilt
            })
            .collect();
        let mut reverse = slotmap::SparseSecondaryMap::new();
        // Slot reuse changes FactBase's storage order. Alpha traversal follows
        // assertion chronology, which is preserved by each fact's timestamp.
        let mut chronological_facts: Vec<_> = facts.iter().collect();
        let sort_factor = usize::try_from(chronological_facts.len().max(2).ilog2()).unwrap() + 1;
        work.spend(chronological_facts.len().saturating_mul(sort_factor))?;
        chronological_facts.sort_unstable_by_key(|(_, entry)| entry.timestamp);
        for (id, entry) in chronological_facts {
            let entry_type = match &entry.fact {
                Fact::Ordered(fact) => AlphaEntryType::OrderedRelation(fact.relation),
                Fact::Template(fact) => AlphaEntryType::Template(fact.template_id),
            };
            let Some(start) = entries.get(&entry_type) else {
                continue;
            };
            let mut pending = vec![*start];
            let mut memories = smallvec::SmallVec::<[AlphaMemoryId; 4]>::new();
            while let Some(node) = pending.pop() {
                work.step()?;
                if let Some(AlphaNode::ConstantTest { test, .. }) =
                    self.alpha.nodes.get(node.0 as usize)
                {
                    match &test.test_type {
                        ConstantTestType::EqualAny(_) | ConstantTestType::Any(_) => {
                            work.spend(constant_test_cost(test))?;
                        }
                        // Charge the split search the test itself repeats.
                        ConstantTestType::Sequence(plan) => {
                            any_sequence_match(&entry.fact, None, &[], plan, work)?;
                        }
                        _ => {}
                    }
                }
                if let Some((memory, children)) = self.alpha.propagation_plan(node, &entry.fact) {
                    if let Some(memory) = memory {
                        work.spend(
                            expected
                                .get(memory.0 as usize)
                                .ok_or("dangling alpha memory")?
                                .indexed_slots
                                .len(),
                        )?;
                        expected
                            .get_mut(memory.0 as usize)
                            .ok_or("dangling alpha memory")?
                            .insert(id, &entry.fact);
                        memories.push(memory);
                    }
                    pending.extend(children);
                }
            }
            if !memories.is_empty() {
                reverse.insert(id, memories);
            }
        }
        for (index, (actual, expected)) in self.alpha.memories.iter().zip(&expected).enumerate() {
            require_eq!(actual.id.0 as usize, index);
            require!(
                actual.facts.iter().eq(expected.facts.iter()),
                "inconsistent alpha fact membership or order"
            );
            require_eq!(actual.slot_indices.len(), expected.slot_indices.len());
            for (slot, keys) in &actual.slot_indices {
                let expected = expected
                    .slot_indices
                    .get(slot)
                    .ok_or("unexpected indexed alpha slot")?;
                require_eq!(keys.len(), expected.len());
                for (key, ids) in keys {
                    let (expected_key, expected) = expected
                        .get_key_value(key)
                        .ok_or("unexpected alpha binding key")?;
                    validate_index_key_metadata(key, expected_key)?;
                    require!(
                        ids.iter().eq(expected.iter()),
                        "inconsistent alpha index membership or order"
                    );
                }
            }
        }
        require_eq!(self.alpha.fact_to_memories.len(), reverse.len());
        for (fact, actual) in &self.alpha.fact_to_memories {
            let expected = reverse.get(fact).ok_or("stale alpha reverse fact")?;
            require!(
                same_members(actual, expected),
                "invalid alpha fact reverse index"
            );
        }
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn validate_conditional_memories(
        &self,
        facts: &FactBase,
        work: &mut Work,
    ) -> Result<(), String> {
        for (&node_id, node) in &self.beta.nodes {
            let (parent_id, alpha_id, tests, sequence, negative, exists) = match node {
                BetaNode::Negative {
                    parent,
                    alpha_memory,
                    tests,
                    sequence,
                    neg_memory,
                    ..
                } => (
                    *parent,
                    *alpha_memory,
                    tests,
                    sequence,
                    Some(*neg_memory),
                    None,
                ),
                BetaNode::Exists {
                    parent,
                    alpha_memory,
                    tests,
                    sequence,
                    exists_memory,
                    ..
                } => (
                    *parent,
                    *alpha_memory,
                    tests,
                    sequence,
                    None,
                    Some(*exists_memory),
                ),
                BetaNode::Ncc {
                    parent,
                    partner,
                    ncc_memory,
                    ..
                } => {
                    let memory = self
                        .beta
                        .get_ncc_memory(*ncc_memory)
                        .ok_or("dangling NCC memory")?;
                    require!(
                        matches!(self.beta.get_node(*partner), Some(BetaNode::NccPartner { ncc_node, ncc_memory: partner_memory, .. }) if *ncc_node == node_id && partner_memory == ncc_memory),
                        "invalid NCC partner link"
                    );
                    let output = self
                        .beta
                        .memory_id_for_node(node_id)
                        .and_then(|id| self.beta.get_memory(id))
                        .ok_or("missing NCC output memory")?;
                    require_eq!(output.len(), memory.unblocked.len());
                    let upstream = self
                        .beta
                        .memory_id_for_node(*parent)
                        .and_then(|id| self.beta.get_memory(id))
                        .ok_or("missing NCC parent memory")?;
                    let partner_parent = match self.beta.get_node(*partner) {
                        Some(BetaNode::NccPartner { parent, .. }) => *parent,
                        _ => return Err("missing NCC partner parent".to_owned()),
                    };
                    let results = self
                        .beta
                        .memory_id_for_node(partner_parent)
                        .and_then(|id| self.beta.get_memory(id))
                        .ok_or("missing NCC result memory")?;
                    require_eq!(results.len(), memory.result_owner.len());
                    let mut result_ranks = rustc_hash::FxHashMap::default();
                    for (rank, result) in results.iter().enumerate() {
                        result_ranks.insert(result, rank);
                        let mut token = result;
                        let owner = loop {
                            work.step()?;
                            let ancestor = self
                                .token_store
                                .get(token)
                                .and_then(|token| token.parent)
                                .ok_or("NCC result lacks owner ancestor")?;
                            if upstream.contains(ancestor) {
                                break ancestor;
                            }
                            token = ancestor;
                        };
                        require!(
                            memory.result_owner.get(&result) == Some(&owner),
                            "inconsistent NCC result owner"
                        );
                    }
                    for supports in memory.parent_results.values() {
                        let mut previous_rank = usize::MAX;
                        for result in supports {
                            work.step()?;
                            let rank = *result_ranks.get(result).ok_or("dangling NCC support")?;
                            require!(
                                rank < previous_rank,
                                "NCC supports are not in result insertion order"
                            );
                            previous_rank = rank;
                        }
                    }
                    for owner in upstream.iter() {
                        require!(
                            memory.result_count.contains_key(&owner)
                                != memory.unblocked.contains_key(&owner),
                            "NCC parent must be exactly blocked or unblocked"
                        );
                    }
                    for (token, owner) in &memory.result_owner {
                        require!(
                            self.token_store.get(*token).is_some() && upstream.contains(*owner),
                            "dangling NCC result/owner"
                        );
                    }
                    for owner in memory.result_count.keys() {
                        require!(upstream.contains(*owner), "dangling NCC count owner");
                    }
                    for (owner, passthrough) in &memory.unblocked {
                        require!(upstream.contains(*owner), "dangling NCC unblocked owner");
                        self.validate_passthrough(*owner, *passthrough, node_id)?;
                    }
                    continue;
                }
                _ => continue,
            };
            let upstream = self
                .beta
                .memory_id_for_node(parent_id)
                .and_then(|id| self.beta.get_memory(id))
                .ok_or("missing conditional parent memory")?;
            let alpha = self
                .alpha
                .get_memory(alpha_id)
                .ok_or("missing conditional alpha memory")?;
            let mut tracked = rustc_hash::FxHashSet::default();
            for parent in upstream.iter() {
                let token = self
                    .token_store
                    .get(parent)
                    .ok_or("missing conditional parent token")?;
                let mut matches = rustc_hash::FxHashSet::default();
                let sequence = sequence.as_deref();
                for fact in crate::rete::collect_candidate_facts(
                    alpha,
                    crate::rete::indexable_tests(tests, sequence),
                    &token.bindings,
                ) {
                    let fact_value = &facts.get(fact).ok_or("missing conditional fact")?.fact;
                    // A fact counts once, through any matching split.
                    let supports = if let Some(sequence) = sequence {
                        any_sequence_match(
                            fact_value,
                            Some(&token.bindings),
                            tests,
                            sequence,
                            work,
                        )?
                    } else {
                        work.spend(tests.len() + 1)?;
                        crate::rete::evaluate_join(fact_value, Some(token), tests)
                    };
                    if supports {
                        matches.insert(fact);
                    }
                }
                if let Some(id) = negative {
                    let memory = self
                        .beta
                        .get_neg_memory(id)
                        .ok_or("missing negative memory")?;
                    tracked.insert(parent);
                    if matches.is_empty() {
                        let passthrough = memory
                            .unblocked
                            .get(&parent)
                            .ok_or("negative parent missing pass-through")?;
                        require!(
                            !memory.blocked.contains_key(&parent),
                            "negative parent both blocked and unblocked"
                        );
                        self.validate_passthrough(parent, *passthrough, node_id)?;
                    } else {
                        let blockers = memory
                            .blocked
                            .get(&parent)
                            .ok_or("incomplete negative blocker membership")?;
                        require!(
                            blockers.len() == matches.len(),
                            "incomplete negative blocker membership"
                        );
                        // Left scans and later right assertions both retain
                        // oldest-first supports. Reordering identical members
                        // would change the primary blocker after restoration.
                        let mut previous = None;
                        for fact in blockers {
                            work.step()?;
                            require!(
                                matches.contains(fact),
                                "incomplete negative blocker membership"
                            );
                            let timestamp = facts
                                .get(*fact)
                                .ok_or("missing negative blocker")?
                                .timestamp;
                            require!(
                                previous.map_or(true, |earlier| earlier < timestamp),
                                "negative blocker supports are not in assertion order"
                            );
                            previous = Some(timestamp);
                        }
                        require!(
                            !memory.unblocked.contains_key(&parent),
                            "blocked negative parent has pass-through"
                        );
                    }
                }
                if let Some(id) = exists {
                    let memory = self
                        .beta
                        .get_exists_memory(id)
                        .ok_or("missing exists memory")?;
                    if matches.is_empty() {
                        require!(
                            !memory.support.contains_key(&parent)
                                && !memory.satisfied.contains_key(&parent),
                            "unsupported exists parent retained"
                        );
                    } else {
                        tracked.insert(parent);
                        require!(
                            memory.support.get(&parent) == Some(&matches),
                            "incomplete exists support membership"
                        );
                        let passthrough = memory
                            .satisfied
                            .get(&parent)
                            .ok_or("exists parent missing pass-through")?;
                        self.validate_passthrough(parent, *passthrough, node_id)?;
                    }
                }
            }
            if let Some(id) = negative {
                let memory = self
                    .beta
                    .get_neg_memory(id)
                    .ok_or("missing negative memory")?;
                require_eq!(tracked.len(), memory.blocked.len() + memory.unblocked.len());
                let output = self
                    .beta
                    .memory_id_for_node(node_id)
                    .and_then(|id| self.beta.get_memory(id))
                    .ok_or("missing negative output memory")?;
                require_eq!(output.len(), memory.unblocked.len());
            }
            if let Some(id) = exists {
                let memory = self
                    .beta
                    .get_exists_memory(id)
                    .ok_or("missing exists memory")?;
                require_eq!(tracked.len(), memory.support.len());
                let output = self
                    .beta
                    .memory_id_for_node(node_id)
                    .and_then(|id| self.beta.get_memory(id))
                    .ok_or("missing exists output memory")?;
                require_eq!(output.len(), memory.satisfied.len());
            }
        }
        Ok(())
    }

    fn validate_passthrough(
        &self,
        parent: crate::token::TokenId,
        token: crate::token::TokenId,
        node: NodeId,
    ) -> Result<(), String> {
        let token = self
            .token_store
            .get(token)
            .ok_or("dangling pass-through token")?;
        require!(
            token.parent == Some(parent) && token.fact.is_none() && token.owner_node == node,
            "invalid pass-through token"
        );
        require!(
            same_bindings(
                &token.bindings,
                &self
                    .token_store
                    .get(parent)
                    .ok_or("dangling pass-through parent")?
                    .bindings
            ),
            "inconsistent pass-through bindings"
        );
        Ok(())
    }

    fn validate_join_memberships(&self, facts: &FactBase, work: &mut Work) -> Result<(), String> {
        for (&node_id, node) in &self.beta.nodes {
            let BetaNode::Join {
                parent,
                alpha_memory,
                tests,
                bindings,
                memory,
                sequence,
                ..
            } = node
            else {
                continue;
            };
            let memory = self.beta.get_memory(*memory).ok_or("missing join memory")?;
            let alpha = self
                .alpha
                .get_memory(*alpha_memory)
                .ok_or("missing join alpha memory")?;
            if let Some(sequence) = sequence.as_deref() {
                self.validate_sequence_join(memory, alpha, tests, bindings, sequence, facts, work)?;
                continue;
            }
            let mut pairs = rustc_hash::FxHashSet::default();
            for id in memory.iter() {
                let token = self.token_store.get(id).ok_or("missing join token")?;
                let parent_id = token.parent.ok_or("join token has no parent")?;
                let fact_id = token.fact.ok_or("join token has no fact")?;
                require!(pairs.insert((parent_id, fact_id)), "duplicate join match");
                let parent_token = self
                    .token_store
                    .get(parent_id)
                    .ok_or("missing join parent")?;
                let fact = &facts.get(fact_id).ok_or("missing join fact")?.fact;
                work.spend(bindings.len() + parent_token.bindings.capacity() + 1)?;
                require!(
                    same_bindings(
                        &token.bindings,
                        &join_bindings(
                            parent_token,
                            |slot| crate::alpha::get_slot_value(fact, slot),
                            bindings
                        )
                    ),
                    "inconsistent join token bindings"
                );
            }
            let upstream = self
                .beta
                .memory_id_for_node(*parent)
                .and_then(|id| self.beta.get_memory(id))
                .ok_or("missing join parent memory")?;
            for parent_id in upstream.iter() {
                let parent_token = self
                    .token_store
                    .get(parent_id)
                    .ok_or("missing upstream token")?;
                for fact_id in crate::rete::collect_candidate_facts(
                    alpha,
                    crate::rete::indexable_tests(tests, None),
                    &parent_token.bindings,
                ) {
                    work.spend(tests.len() + 1)?;
                    let fact = &facts.get(fact_id).ok_or("missing alpha fact")?.fact;
                    if crate::rete::evaluate_join(fact, Some(parent_token), tests) {
                        require!(
                            pairs.remove(&(parent_id, fact_id)),
                            "missing positive join match at {node_id:?}"
                        );
                    }
                }
            }
            require!(pairs.is_empty(), "unexpected positive join match");
        }
        Ok(())
    }

    /// Sequence tokens record their split, so each one is checked for
    /// soundness by rebuilding that single split in `O(width)`.
    /// Completeness is not re-enumerated: every split of every parent and
    /// alpha fact pair is combinatorial and would not fit the work budget.
    #[allow(clippy::too_many_arguments)]
    fn validate_sequence_join(
        &self,
        memory: &crate::beta::BetaMemory,
        alpha: &AlphaMemory,
        tests: &[JoinTest],
        bindings: &[(crate::alpha::SlotIndex, crate::binding::VarId)],
        sequence: &SequencePattern,
        facts: &FactBase,
        work: &mut Work,
    ) -> Result<(), String> {
        let mut seen = rustc_hash::FxHashSet::default();
        let mut value_costs = rustc_hash::FxHashMap::default();
        for id in memory.iter() {
            let token = self.token_store.get(id).ok_or("missing join token")?;
            let parent_id = token.parent.ok_or("join token has no parent")?;
            let fact_id = token.fact.ok_or("join token has no fact")?;
            let lengths = self
                .token_store
                .match_lengths(id)
                .ok_or("join token has inconsistent sequence metadata")?;
            require!(
                seen.insert((parent_id, fact_id, lengths)),
                "duplicate join match"
            );
            require!(
                alpha.contains(fact_id),
                "join token fact is not in its alpha memory"
            );
            let parent_token = self
                .token_store
                .get(parent_id)
                .ok_or("missing join parent")?;
            let fact = &facts.get(fact_id).ok_or("missing join fact")?.fact;
            let binding_cost = bindings.len() + parent_token.bindings.capacity() + 2;
            work.spend(split_cost(sequence, tests).saturating_add(binding_cost))?;
            let split = sequence
                .project(fact, lengths)
                .ok_or("invalid sequence capture lengths")?;
            require!(
                crate::rete::split_matches(&split, &parent_token.bindings, tests, sequence),
                "unexpected positive join match"
            );
            require!(
                same_bindings(
                    &token.bindings,
                    &join_bindings(parent_token, |slot| split.get(slot), bindings)
                ),
                "inconsistent join token bindings"
            );
            // Tests and bindings that read a capture copied and compared it.
            if split.copied_capture() {
                let value_cost = match value_costs.entry(fact_id) {
                    std::collections::hash_map::Entry::Occupied(entry) => *entry.get(),
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        *entry.insert(fact_value_cost(fact, work)?)
                    }
                };
                work.spend(binding_cost.saturating_mul(value_cost))?;
            }
        }
        Ok(())
    }

    #[doc(hidden)]
    pub fn snapshot_rule_ids(&self) -> impl Iterator<Item = RuleId> + '_ {
        self.beta.nodes.values().filter_map(|node| match node {
            BetaNode::Terminal { rule, .. } => Some(*rule),
            _ => None,
        })
    }

    /// Template sequence plans paired with their original template identities.
    /// The runtime uses this after core validation to check physical sources
    /// against registered slot widths and scalar/multifield declarations.
    #[doc(hidden)]
    pub fn snapshot_template_sequence_patterns(
        &self,
    ) -> Result<Vec<(crate::fact::TemplateId, &SequencePattern)>, String> {
        let mut work = Work(10_000_000);
        let entries = self.alpha_memory_entry_types(&mut work)?;
        let mut plans = Vec::new();
        for node in self.beta.nodes.values() {
            work.step()?;
            if let BetaNode::Join {
                alpha_memory,
                sequence: Some(sequence),
                ..
            }
            | BetaNode::Negative {
                alpha_memory,
                sequence: Some(sequence),
                ..
            }
            | BetaNode::Exists {
                alpha_memory,
                sequence: Some(sequence),
                ..
            } = node
            {
                if let Some(AlphaEntryType::Template(template)) = entries.get(alpha_memory) {
                    plans.push((*template, sequence.as_ref()));
                }
            }
        }
        Ok(plans)
    }

    fn alpha_memory_entry_types(
        &self,
        work: &mut Work,
    ) -> Result<rustc_hash::FxHashMap<AlphaMemoryId, AlphaEntryType>, String> {
        let mut paths = vec![None; self.alpha.nodes.len()];
        let mut memories = rustc_hash::FxHashMap::default();
        for (index, node) in self.alpha.nodes.iter().enumerate() {
            work.step()?;
            let (children, memory) = match node {
                AlphaNode::Entry {
                    entry_type,
                    children,
                    memory,
                } => {
                    paths[index] = Some(entry_type.clone());
                    (children, memory)
                }
                AlphaNode::ConstantTest {
                    children, memory, ..
                } => (children, memory),
            };
            let entry = paths[index]
                .clone()
                .ok_or("alpha path has no entry source")?;
            for child in children {
                work.step()?;
                require!(child.0 as usize > index, "cyclic alpha source path");
                *paths
                    .get_mut(child.0 as usize)
                    .ok_or("dangling alpha source child")? = Some(entry.clone());
            }
            if let Some(memory) = memory {
                memories.insert(*memory, entry);
            }
        }
        Ok(memories)
    }

    #[doc(hidden)]
    pub fn snapshot_template_ids(&self) -> impl Iterator<Item = crate::fact::TemplateId> + '_ {
        self.alpha
            .entry_nodes
            .keys()
            .filter_map(|entry| match entry {
                AlphaEntryType::Template(id) => Some(*id),
                AlphaEntryType::OrderedRelation(_) => None,
            })
    }

    #[doc(hidden)]
    pub fn validate_snapshot_rules(
        &self,
        metadata: impl Fn(RuleId) -> Option<(crate::beta::Salience, usize)>,
    ) -> Result<(), String> {
        for node in self.beta.nodes.values() {
            match node {
                BetaNode::Terminal { rule, salience, .. } => {
                    let (expected, _) = metadata(*rule).ok_or("terminal lacks runtime metadata")?;
                    require_eq!(*salience, expected);
                }
                BetaNode::Predicate {
                    rule,
                    condition_index,
                    ..
                } => {
                    let (_, conditions) =
                        metadata(*rule).ok_or("predicate lacks runtime metadata")?;
                    require!(
                        (*condition_index as usize) < conditions,
                        "dangling predicate condition"
                    );
                }
                _ => {}
            }
        }
        Ok(())
    }
}

impl crate::compiler::ReteCompiler {
    /// Next executable ID, checked against runtime metadata's retained capacity.
    #[doc(hidden)]
    #[must_use]
    pub const fn snapshot_next_rule_id(&self) -> u32 {
        self.next_rule_id
    }

    #[doc(hidden)]
    pub fn validate_snapshot(&self, rete: &ReteNetwork) -> Result<(), String> {
        let mut work = Work(10_000_000);
        require!(
            self.next_rule_id > 0 && self.next_rule_id < u32::MAX,
            "invalid rule allocation counter"
        );
        for node in rete.beta.nodes.values() {
            if let BetaNode::Terminal { rule, .. } | BetaNode::Predicate { rule, .. } = node {
                require!(
                    rule.0 > 0 && rule.0 < self.next_rule_id,
                    "rule exceeds compiler allocation counter"
                );
            }
        }
        require_eq!(self.alpha_path_cache.len(), rete.alpha.memories.len());
        let mut parents = vec![None; rete.alpha.nodes.len()];
        let mut owners = vec![None; rete.alpha.memories.len()];
        for (index, node) in rete.alpha.nodes.iter().enumerate() {
            let id = NodeId(u32::try_from(index).map_err(|_| "oversized alpha graph")?);
            let (children, memory) = match node {
                AlphaNode::Entry {
                    children, memory, ..
                }
                | AlphaNode::ConstantTest {
                    children, memory, ..
                } => (children, memory),
            };
            for child in children {
                *parents
                    .get_mut(child.0 as usize)
                    .ok_or("dangling alpha child")? = Some(id);
            }
            if let Some(memory) = memory {
                *owners
                    .get_mut(memory.0 as usize)
                    .ok_or("dangling alpha memory")? = Some(id);
            }
        }
        let mut cached_memories = rustc_hash::FxHashSet::default();
        for (key, memory) in &self.alpha_path_cache {
            require!(
                cached_memories.insert(*memory),
                "duplicate cached alpha memory"
            );
            let mut id = owners
                .get(memory.0 as usize)
                .copied()
                .flatten()
                .ok_or("cached alpha memory lacks owner")?;
            for expected in key.tests.iter().rev() {
                work.spend(constant_test_cost(expected))?;
                require!(
                    matches!(rete.alpha.nodes.get(id.0 as usize), Some(AlphaNode::ConstantTest { test, .. }) if test == expected),
                    "cached alpha test mismatch"
                );
                id = parents
                    .get(id.0 as usize)
                    .copied()
                    .flatten()
                    .ok_or("cached alpha path lacks ancestor")?;
            }
            require!(
                matches!(rete.alpha.nodes.get(id.0 as usize), Some(AlphaNode::Entry { entry_type, .. }) if *entry_type == key.entry_type),
                "cached alpha entry mismatch"
            );
        }
        require_eq!(
            self.join_node_cache.len(),
            rete.beta
                .nodes
                .values()
                .filter(|node| matches!(node, BetaNode::Join { .. }))
                .count()
        );
        let mut cached_joins = rustc_hash::FxHashSet::default();
        for (key, id) in &self.join_node_cache {
            require!(cached_joins.insert(*id), "duplicate cached join node");
            require!(
                matches!(rete.beta.nodes.get(id), Some(BetaNode::Join { parent, alpha_memory, tests, bindings, sequence, .. }) if *parent == key.parent && *alpha_memory == key.alpha_memory && tests.as_ref() == key.tests.as_slice() && bindings.as_ref() == key.bindings.as_slice() && sequence.as_deref() == key.sequence.as_ref()),
                "cached join node mismatch"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_rejects_reordered_negative_supports_after_fact_slot_reuse() {
        for activate_before_facts in [false, true] {
            let mut symbols = SymbolTable::new();
            let relation = symbols
                .intern_symbol("blocker", crate::StringEncoding::Ascii)
                .unwrap();
            let mut facts = FactBase::new();
            let mut rete = ReteNetwork::new();
            let entry = rete
                .alpha
                .create_entry_node(AlphaEntryType::OrderedRelation(relation));
            let alpha = rete.alpha.create_memory(entry);
            let (negative, _, memory) =
                rete.beta
                    .create_negative_node(rete.beta.root_id(), alpha, vec![]);
            rete.beta
                .create_terminal_node(negative, crate::RuleId(1), crate::Salience::DEFAULT);
            if activate_before_facts {
                rete.activate_root_children(&[negative], &facts);
            }
            let mut asserted = Vec::new();
            for value in 0..3 {
                let id = facts.assert_ordered(relation, smallvec::smallvec![Value::Integer(value)]);
                rete.assert_fact(id, &facts.get(id).unwrap().fact, &facts);
                asserted.push(id);
            }
            if !activate_before_facts {
                rete.activate_root_children(&[negative], &facts);
            }
            let removed = facts.retract(asserted[1]).unwrap();
            rete.retract_fact(asserted[1], &removed.fact, &facts);
            let replacement =
                facts.assert_ordered(relation, smallvec::smallvec![Value::Integer(3)]);
            rete.assert_fact(replacement, &facts.get(replacement).unwrap().fact, &facts);
            rete.validate_snapshot(&facts, &symbols).unwrap();

            let negative_memory = rete.beta.get_neg_memory_mut(memory).unwrap();
            let parent = *negative_memory.blocked.keys().next().unwrap();
            assert_eq!(
                negative_memory.blocked[&parent]
                    .iter()
                    .copied()
                    .collect::<Vec<_>>(),
                vec![asserted[0], asserted[2], replacement]
            );
            let mut reordered = crate::ordered_set::OrderedSet::default();
            for id in [asserted[2], asserted[0], replacement] {
                reordered.insert(id);
            }
            negative_memory.blocked.insert(parent, reordered);
            // Forward/reverse membership and block positions still agree, so
            // only snapshot validation can detect the forged primary support.
            rete.validate_consistency().unwrap();
            assert!(rete
                .validate_snapshot(&facts, &symbols)
                .unwrap_err()
                .contains("negative blocker supports are not in assertion order"));
        }
    }

    #[test]
    fn snapshot_rejects_impossible_public_indexes_from_older_epochs() {
        use crate::value::FactAddress;
        let mut symbols = SymbolTable::new();
        let relation = symbols
            .intern_symbol("item", crate::StringEncoding::Ascii)
            .unwrap();
        let mut previous = FactBase::new();
        for _ in 0..3 {
            previous.assert_ordered(relation, smallvec::smallvec![]);
        }
        let id = previous.assert_ordered(relation, smallvec::smallvec![]);
        let timestamp = previous.get(id).unwrap().timestamp;
        assert_eq!(timestamp.get(), 3);
        let current = FactBase::new();
        for index in [0, 3, 4] {
            let address = FactAddress::new(id, 1, timestamp, index);
            current
                .validate_snapshot_fact_address(&address, 2, None, false)
                .unwrap();
        }
        let malformed = FactAddress::new(id, 1, timestamp, 1);
        assert!(current
            .validate_snapshot_fact_address(&malformed, 2, None, false)
            .unwrap_err()
            .contains("impossible historical public index"));
    }

    #[test]
    fn snapshot_rejects_fact_address_metadata_in_rete_index_keys() {
        use crate::alpha::SlotIndex;
        use crate::beta::{JoinTestType, RuleId, Salience};
        use crate::binding::VarId;
        use crate::value::{AtomKey, FactAddress};

        for (corrupt_alpha, invalid_index) in [(true, 0), (true, 99), (false, 0), (false, 99)] {
            let mut symbols = SymbolTable::new();
            let left = symbols
                .intern_symbol("left", crate::StringEncoding::Ascii)
                .unwrap();
            let right = symbols
                .intern_symbol("right", crate::StringEncoding::Ascii)
                .unwrap();
            let target_relation = symbols
                .intern_symbol("target", crate::StringEncoding::Ascii)
                .unwrap();
            let mut facts = FactBase::new();
            let target = facts.assert_ordered(target_relation, smallvec::smallvec![]);
            let timestamp = facts.get(target).unwrap().timestamp;
            let address = FactAddress::new(target, 0, timestamp, 1);
            let valid_key = AtomKey::FactAddress(address.clone());
            let invalid_address = FactAddress::new(target, 0, timestamp, invalid_index);
            // An older epoch's zero index is individually plausible, but it is
            // still inconsistent with the address stored in facts and bindings.
            if invalid_index == 0 {
                facts
                    .validate_snapshot_fact_address(&invalid_address, 1, None, false)
                    .unwrap();
            }
            let invalid_key = AtomKey::FactAddress(invalid_address);
            assert_eq!(valid_key, invalid_key, "public spelling is not identity");

            let mut rete = ReteNetwork::new();
            let left_entry = rete
                .alpha
                .create_entry_node(AlphaEntryType::OrderedRelation(left));
            let left_memory = rete.alpha.create_memory(left_entry);
            let right_entry = rete
                .alpha
                .create_entry_node(AlphaEntryType::OrderedRelation(right));
            let right_memory = rete.alpha.create_memory(right_entry);
            let variable = VarId(0);
            let slot = SlotIndex::Ordered(0);
            let (first_join, first_memory) = rete.beta.create_join_node(
                rete.beta.root_id(),
                left_memory,
                vec![],
                vec![(slot, variable)],
            );
            rete.beta
                .get_memory_mut(first_memory)
                .unwrap()
                .request_var_index_empty(variable);
            rete.alpha
                .get_memory_mut(right_memory)
                .unwrap()
                .request_index_empty(slot);
            let (last_join, _) = rete.beta.create_join_node(
                first_join,
                right_memory,
                vec![JoinTest {
                    alpha_slot: slot,
                    beta_var: variable,
                    test_type: JoinTestType::Equal,
                }],
                vec![],
            );
            rete.beta
                .create_terminal_node(last_join, RuleId(1), Salience::DEFAULT);
            for relation in [left, right] {
                let id = facts.assert_ordered(
                    relation,
                    smallvec::smallvec![Value::FactAddress(address.clone())],
                );
                rete.assert_fact(id, &facts.get(id).unwrap().fact, &facts);
            }
            rete.validate_snapshot(&facts, &symbols).unwrap();
            if corrupt_alpha {
                let keys = rete
                    .alpha
                    .get_memory_mut(right_memory)
                    .unwrap()
                    .slot_indices
                    .get_mut(&slot)
                    .unwrap();
                let members = keys.remove(&valid_key).unwrap();
                keys.insert(invalid_key, members);
            } else {
                let keys = rete
                    .beta
                    .get_memory_mut(first_memory)
                    .unwrap()
                    .var_indices
                    .get_mut(&variable)
                    .unwrap();
                let members = keys.remove(&valid_key).unwrap();
                keys.insert(invalid_key, members);
            }
            // Address identity still matches, but the persisted presentation
            // metadata must match the rebuilt key for each index family.
            assert!(rete
                .validate_snapshot(&facts, &symbols)
                .unwrap_err()
                .contains("inconsistent fact address index metadata"));
        }
    }

    #[test]
    fn snapshot_disjunction_checks_nested_constants_and_limits() {
        use crate::alpha::{ConstantTest, SlotIndex};
        let test = ConstantTest {
            slot: SlotIndex::Ordered(0),
            test_type: ConstantTestType::Any(vec![vec![ConstantTest {
                slot: SlotIndex::Ordered(0),
                test_type: ConstantTestType::Equal(crate::value::AtomKey::String(
                    crate::string::FerricString::Ascii(Box::new([0xff])),
                )),
            }]]),
        };
        assert!(validate_constant(&test, &SymbolTable::new())
            .unwrap_err()
            .contains("invalid ASCII string"));
        let mut deep = ConstantTest {
            slot: SlotIndex::Ordered(0),
            test_type: ConstantTestType::Equal(crate::value::AtomKey::Integer(0)),
        };
        for _ in 0..MAX_ALPHA_DEPTH {
            deep = ConstantTest {
                slot: SlotIndex::Ordered(0),
                test_type: ConstantTestType::Any(vec![vec![deep]]),
            };
        }
        assert!(validate_constant(&deep, &SymbolTable::new())
            .unwrap_err()
            .contains("exceeds 64 tests"));
    }

    #[test]
    fn snapshot_sequence_disjunction_charges_nested_alternatives() {
        use crate::alpha::{ConstantTest, SlotIndex};
        use crate::sequence::{SequenceField, SequenceSegment, SequenceSource};
        let mut symbols = SymbolTable::new();
        let fact = Fact::Ordered(crate::fact::OrderedFact {
            relation: symbols
                .intern_symbol("row", crate::StringEncoding::Ascii)
                .unwrap(),
            fields: smallvec::smallvec![Value::Integer(0)],
        });
        let test = ConstantTest {
            slot: SlotIndex::Ordered(0),
            test_type: ConstantTestType::EqualAny(vec![crate::value::AtomKey::Integer(-1); 100]),
        };
        let plan = SequencePattern {
            segments: vec![SequenceSegment {
                source: SequenceSource::Ordered,
                fields: vec![SequenceField::Single],
            }],
            tests: vec![ConstantTest {
                slot: SlotIndex::Ordered(0),
                test_type: ConstantTestType::Any(vec![vec![test.clone()], vec![test]]),
            }],
        };
        assert_eq!(
            any_sequence_match(&fact, None, &[], &plan, &mut Work(100)).unwrap_err(),
            "snapshot validation work limit exceeded"
        );
        assert!(!any_sequence_match(&fact, None, &[], &plan, &mut Work(1000)).unwrap());
    }

    #[test]
    fn snapshot_sequence_disjunction_detects_nested_capture_reads() {
        use crate::alpha::{ConstantTest, SlotIndex};
        use crate::sequence::{SequenceField, SequenceSegment, SequenceSource};
        let plan = SequencePattern {
            segments: vec![SequenceSegment {
                source: SequenceSource::Ordered,
                fields: vec![SequenceField::Single, SequenceField::Multi],
            }],
            tests: vec![ConstantTest {
                slot: SlotIndex::Ordered(0),
                test_type: ConstantTestType::Any(vec![vec![ConstantTest {
                    slot: SlotIndex::Ordered(0),
                    test_type: ConstantTestType::EqualSlot(SlotIndex::Ordered(1)),
                }]]),
            }],
        };
        assert!(sequence_tests_read_captures(&plan));
    }

    #[test]
    fn rejected_template_cartesian_candidates_consume_snapshot_work() {
        use crate::sequence::{SequenceField, SequenceSegment, SequenceSource};
        let mut ids: slotmap::SlotMap<crate::fact::TemplateId, ()> = slotmap::SlotMap::with_key();
        let values = Value::Multifield(Box::new(vec![Value::Integer(1); 4].into_iter().collect()));
        let fact = Fact::Template(crate::fact::TemplateFact {
            template_id: ids.insert(()),
            slots: vec![values.clone(), values].into_boxed_slice(),
        });
        let plan = SequencePattern {
            segments: (0..2)
                .map(|index| SequenceSegment {
                    source: SequenceSource::TemplateSlot(index),
                    fields: vec![SequenceField::Multi; 3],
                })
                .collect(),
            // The last capture is placed last, so no split is pruned early.
            tests: vec![crate::alpha::ConstantTest {
                slot: crate::alpha::SlotIndex::Template(5),
                test_type: ConstantTestType::Equal(crate::value::AtomKey::Integer(99)),
            }],
        };
        let token = crate::token::Token {
            fact: None,
            parent: None,
            owner_node: NodeId(0),
            bindings: crate::binding::BindingSet::new(),
        };
        // 15 x 15 splits, all rejected by the constant test.
        assert_eq!(
            any_sequence_match(&fact, Some(&token.bindings), &[], &plan, &mut Work(100))
                .unwrap_err(),
            "snapshot validation work limit exceeded"
        );
        assert!(
            !any_sequence_match(&fact, Some(&token.bindings), &[], &plan, &mut Work(100_000))
                .unwrap()
        );
    }
}
