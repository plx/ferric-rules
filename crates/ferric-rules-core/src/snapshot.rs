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
use crate::value::Value;

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
use crate::sequence::SequencePattern;
use crate::token::NodeId;

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

/// Visit projected matches without letting rejected split combinations escape
/// the snapshot work budget. Charge before advancing the unfiltered iterator.
#[allow(clippy::too_many_arguments)]
fn visit_join_matches(
    fact: &Fact,
    token: &crate::token::Token,
    tests: &[JoinTest],
    sequence: Option<&SequencePattern>,
    binding_cost: usize,
    work: &mut Work,
    mut visit: impl FnMut(&Fact, Option<&[usize]>) -> Result<bool, String>,
) -> Result<(), String> {
    let Some(sequence) = sequence else {
        work.spend(tests.len() + binding_cost + 1)?;
        if crate::rete::evaluate_join(fact, Some(token), tests) {
            visit(fact, None)?;
        }
        return Ok(());
    };
    let values = match fact {
        Fact::Ordered(fact) => fact.fields.as_slice(),
        Fact::Template(fact) => fact.slots.as_ref(),
    };
    // Projection clones physical values, including string bytes and nested
    // multifields. Counting only top-level fields would undercharge a large
    // value repeatedly cloned across many rejected split candidates.
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
    let test_cost =
        sequence
            .tests
            .iter()
            .fold(tests.len().saturating_mul(value_cost + 1), |cost, test| {
                cost.saturating_add(match &test.test_type {
                    ConstantTestType::EqualAny(values) => {
                        values.len().saturating_mul(value_cost + 1)
                    }
                    _ => value_cost + 1,
                })
            });
    let candidate_cost = sequence
        .fields
        .len()
        .saturating_add(value_cost)
        .saturating_add(test_cost)
        .saturating_add(binding_cost.saturating_mul(value_cost + 1))
        .saturating_add(1);
    work.spend(sequence.fields.len() + 1)?;
    let mut candidates = sequence.candidates(fact);
    loop {
        work.spend(candidate_cost)?;
        let Some(candidate) = candidates.next() else {
            break;
        };
        if sequence.accepts(&candidate.fact)
            && crate::rete::evaluate_join(&candidate.fact, Some(token), tests)
            && !visit(&candidate.fact, Some(&candidate.lengths))?
        {
            break;
        }
    }
    Ok(())
}

fn validate_sequence_plan(
    sequence: &SequencePattern,
    tests: &[JoinTest],
    bindings: &[(crate::alpha::SlotIndex, crate::binding::VarId)],
    symbols: &SymbolTable,
    work: &mut Work,
) -> Result<(), String> {
    work.spend(sequence.fields.len() + sequence.tests.len() + tests.len() + bindings.len() + 1)?;
    sequence.validate()?;
    let valid_slot = |slot| {
        matches!(slot, crate::alpha::SlotIndex::Ordered(index)
        if index < sequence.fields.len())
    };
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
    match &test.test_type {
        Test::Equal(value)
        | Test::NotEqual(value)
        | Test::GreaterThan(value)
        | Test::LessThan(value)
        | Test::GreaterOrEqual(value)
        | Test::LessOrEqual(value) => symbols.validate_snapshot_value(&value.to_value())?,
        Test::EqualAny(values) => {
            for value in values {
                symbols.validate_snapshot_value(&value.to_value())?;
            }
        }
        _ => {}
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
    /// Validate persisted graph identities, runtime memberships, and reverse indexes.
    #[doc(hidden)]
    #[allow(clippy::too_many_lines)]
    pub fn validate_snapshot(&self, facts: &FactBase, symbols: &SymbolTable) -> Result<(), String> {
        self.validate_consistency()?;
        require!(
            self.pending_predicate_matches.is_empty(),
            "snapshot has unfinished predicate matches"
        );
        let mut work = Work(10_000_000);
        self.validate_alpha_snapshot(facts, symbols, &mut work)?;
        // Alpha nodes are allocated before descendants and have one parent.
        // Carry the entry kind forward so every sequence plan belongs to an
        // ordered path, even when that path currently contains no facts.
        let mut ordered_paths = vec![false; self.alpha.nodes.len()];
        let mut ordered_memories = rustc_hash::FxHashSet::default();
        for (index, node) in self.alpha.nodes.iter().enumerate() {
            work.step()?;
            let (children, memory) = match node {
                AlphaNode::Entry {
                    entry_type,
                    children,
                    memory,
                } => {
                    ordered_paths[index] = matches!(entry_type, AlphaEntryType::OrderedRelation(_));
                    (children, memory)
                }
                AlphaNode::ConstantTest {
                    children, memory, ..
                } => (children, memory),
            };
            for child in children {
                work.step()?;
                ordered_paths[child.0 as usize] = ordered_paths[index];
            }
            if let Some(memory) = memory.filter(|_| ordered_paths[index]) {
                ordered_memories.insert(memory);
            }
        }

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
                    require!(
                        ordered_memories.contains(alpha_memory),
                        "sequence plan belongs to a nonordered alpha path"
                    );
                    validate_sequence_plan(sequence, tests, bindings, symbols, &mut work)?;
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
                    require!(
                        ordered_memories.contains(alpha_memory),
                        "sequence plan belongs to a nonordered alpha path"
                    );
                    validate_sequence_plan(sequence, tests, &[], symbols, &mut work)?;
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
                        let expected = expected.get(key).ok_or("wrong beta binding key")?;
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
                depths[child.0 as usize] = depths[index] + usize::from(!field_count_test);
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
                if let Some(AlphaNode::ConstantTest {
                    test:
                        crate::alpha::ConstantTest {
                            test_type: crate::alpha::ConstantTestType::EqualAny(values),
                            ..
                        },
                    ..
                }) = self.alpha.nodes.get(node.0 as usize)
                {
                    work.spend(values.len())?;
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
                    let expected = expected.get(key).ok_or("unexpected alpha binding key")?;
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
                    for result in results.iter() {
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
                let indexed_tests = if sequence.is_some() {
                    &[][..]
                } else {
                    tests.as_ref()
                };
                for fact in crate::rete::collect_candidate_facts(
                    alpha,
                    crate::rete::indexable_tests(indexed_tests, None),
                    &token.bindings,
                ) {
                    let fact_value = &facts.get(fact).ok_or("missing conditional fact")?.fact;
                    visit_join_matches(
                        fact_value,
                        token,
                        tests,
                        sequence.as_deref(),
                        0,
                        work,
                        |_, _| {
                            matches.insert(fact);
                            // A supporting fact is counted once, regardless of cuts.
                            Ok(false)
                        },
                    )?;
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
                        require!(
                            memory.blocked.get(&parent) == Some(&matches),
                            "incomplete negative blocker membership"
                        );
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
            let mut matches = rustc_hash::FxHashMap::default();
            for id in self
                .beta
                .get_memory(*memory)
                .ok_or("missing join memory")?
                .iter()
            {
                let token = self.token_store.get(id).ok_or("missing join token")?;
                let parent_id = token.parent.ok_or("join token has no parent")?;
                let fact_id = token.fact.ok_or("join token has no fact")?;
                let lengths = self.token_store.match_lengths(id).map(<[usize]>::to_vec);
                require!(
                    sequence.is_some() == lengths.is_some(),
                    "join token has inconsistent sequence metadata"
                );
                require!(
                    matches.insert((parent_id, fact_id, lengths), id).is_none(),
                    "duplicate join match"
                );
            }
            let upstream = self
                .beta
                .memory_id_for_node(*parent)
                .and_then(|id| self.beta.get_memory(id))
                .ok_or("missing join parent memory")?;
            let alpha = self
                .alpha
                .get_memory(*alpha_memory)
                .ok_or("missing join alpha memory")?;
            let indexed_tests = if sequence.is_some() {
                &[][..]
            } else {
                tests.as_ref()
            };
            for parent_id in upstream.iter() {
                let parent_token = self
                    .token_store
                    .get(parent_id)
                    .ok_or("missing upstream token")?;
                for fact_id in crate::rete::collect_candidate_facts(
                    alpha,
                    crate::rete::indexable_tests(indexed_tests, None),
                    &parent_token.bindings,
                ) {
                    let fact = &facts.get(fact_id).ok_or("missing alpha fact")?.fact;
                    visit_join_matches(
                        fact,
                        parent_token,
                        tests,
                        sequence.as_deref(),
                        bindings.len() + parent_token.bindings.capacity(),
                        work,
                        |projected, lengths| {
                            let key = (parent_id, fact_id, lengths.map(<[usize]>::to_vec));
                            let id = matches.remove(&key).ok_or_else(|| {
                                format!("missing positive join match at {node_id:?}")
                            })?;
                            let token = self.token_store.get(id).ok_or("missing join token")?;
                            let mut expected = parent_token.bindings.clone();
                            for &(slot, variable) in bindings.iter() {
                                if let Some(value) = crate::alpha::get_slot_value(projected, slot) {
                                    expected.set(
                                        variable,
                                        crate::binding::ValueRef::new(value.clone()),
                                    );
                                }
                            }
                            require!(
                                same_bindings(&token.bindings, &expected),
                                "inconsistent join token bindings"
                            );
                            Ok(true)
                        },
                    )?;
                }
            }
            require!(matches.is_empty(), "unexpected positive join match");
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
                work.spend(match &expected.test_type {
                    crate::alpha::ConstantTestType::EqualAny(values) => values.len() + 1,
                    _ => 1,
                })?;
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
    fn rejected_sequence_candidates_still_consume_snapshot_work() {
        let mut symbols = SymbolTable::new();
        let relation = symbols
            .intern_symbol("row", crate::StringEncoding::Ascii)
            .unwrap();
        let fact = Fact::Ordered(crate::fact::OrderedFact {
            relation,
            fields: smallvec::smallvec![Value::Integer(1); 4],
        });
        let plan = SequencePattern {
            fields: vec![crate::sequence::SequenceField::Multi; 3],
            tests: vec![crate::alpha::ConstantTest {
                slot: crate::alpha::SlotIndex::Ordered(0),
                test_type: ConstantTestType::Equal(crate::value::AtomKey::Integer(99)),
            }],
        };
        let token = crate::token::Token {
            fact: None,
            parent: None,
            owner_node: NodeId(0),
            bindings: crate::binding::BindingSet::new(),
        };
        let mut visited = 0;
        let result =
            visit_join_matches(&fact, &token, &[], Some(&plan), 0, &mut Work(50), |_, _| {
                visited += 1;
                Ok(true)
            });
        assert_eq!(visited, 0);
        assert_eq!(
            result.unwrap_err(),
            "snapshot validation work limit exceeded"
        );
    }
}
