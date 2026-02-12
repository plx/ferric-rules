//! Shared test helpers for Phase 2 integration tests.
//!
//! These helpers provide reusable building blocks for the full pipeline:
//! parse → interpret → compile → run. They are extended incrementally as
//! Phase 2 passes add new capabilities.

use ferric_core::beta::RuleId;
use ferric_core::{AlphaEntryType, AlphaMemoryId, ConstantTest, FactId, ReteNetwork, StringEncoding};

use crate::config::EngineConfig;
use crate::engine::Engine;
use crate::loader::LoadResult;

// ---------------------------------------------------------------------------
// Engine helpers
// ---------------------------------------------------------------------------

/// Create a UTF-8 engine with no pre-loaded content.
pub fn new_utf8_engine() -> Engine {
    Engine::new(EngineConfig::utf8())
}

/// Intern a symbol on the given engine using UTF-8 encoding.
pub fn intern(engine: &mut Engine, name: &str) -> ferric_core::Symbol {
    engine
        .symbol_table
        .intern_symbol(name, StringEncoding::Utf8)
        .expect("symbol interning should succeed in test helper")
}

// ---------------------------------------------------------------------------
// Source loading helpers
// ---------------------------------------------------------------------------

/// Load source and assert it succeeds, returning the `LoadResult`.
///
/// # Panics
///
/// Panics if loading produces errors.
pub fn load_ok(engine: &mut Engine, source: &str) -> LoadResult {
    engine
        .load_str(source)
        .unwrap_or_else(|errors| panic!("load_str failed: {errors:?}"))
}

/// Load source and assert it fails, returning the error list.
///
/// # Panics
///
/// Panics if loading succeeds.
#[allow(dead_code)] // Used by later Phase 2 passes
pub fn load_err(engine: &mut Engine, source: &str) -> Vec<crate::loader::LoadError> {
    engine
        .load_str(source)
        .expect_err("load_str should have failed")
}

// ---------------------------------------------------------------------------
// Rete construction helpers
// ---------------------------------------------------------------------------

/// Build a minimal Rete network with a single ordered-relation pattern rule.
///
/// Creates: alpha entry → alpha memory → join (no tests) → terminal.
pub fn build_single_pattern_rete(
    engine: &mut Engine,
    relation: &str,
    rule_id: RuleId,
) -> ReteNetwork {
    let mut rete = ReteNetwork::new();
    let relation_sym = intern(engine, relation);

    let entry_node = rete
        .alpha
        .create_entry_node(AlphaEntryType::OrderedRelation(relation_sym));
    let alpha_mem_id = rete.alpha.create_memory(entry_node);

    let root_id = rete.beta.root_id();
    let (join_id, _) = rete.beta.create_join_node(root_id, alpha_mem_id, vec![], vec![]);
    let _terminal_id = rete.beta.create_terminal_node(join_id, rule_id);

    rete
}

/// Build a Rete network with one ordered-relation pattern and one constant test.
///
/// Creates: alpha entry → constant test → alpha memory → join (no tests) → terminal.
pub fn build_constant_test_rete(
    engine: &mut Engine,
    relation: &str,
    test: ConstantTest,
    rule_id: RuleId,
) -> ReteNetwork {
    let mut rete = ReteNetwork::new();
    let relation_sym = intern(engine, relation);

    let entry_node = rete
        .alpha
        .create_entry_node(AlphaEntryType::OrderedRelation(relation_sym));
    let test_node = rete.alpha.create_constant_test_node(entry_node, test);
    let alpha_mem_id = rete.alpha.create_memory(test_node);

    let root_id = rete.beta.root_id();
    let (join_id, _) = rete.beta.create_join_node(root_id, alpha_mem_id, vec![], vec![]);
    let _terminal_id = rete.beta.create_terminal_node(join_id, rule_id);

    rete
}

/// Helper result for `build_two_pattern_rete`.
#[allow(dead_code)] // Used by later Phase 2 passes
pub struct TwoPatternRete {
    pub rete: ReteNetwork,
    pub alpha_mem_1: AlphaMemoryId,
    pub alpha_mem_2: AlphaMemoryId,
}

/// Build a Rete network with two ordered-relation patterns joined sequentially.
///
/// Creates: alpha1 → join1 → alpha2 → join2 → terminal.
/// No join tests — variable binding is a Phase 2 addition.
#[allow(dead_code)] // Used by later Phase 2 passes
pub fn build_two_pattern_rete(
    engine: &mut Engine,
    relation1: &str,
    relation2: &str,
    rule_id: RuleId,
) -> TwoPatternRete {
    let mut rete = ReteNetwork::new();
    let rel1_sym = intern(engine, relation1);
    let rel2_sym = intern(engine, relation2);

    let entry1 = rete
        .alpha
        .create_entry_node(AlphaEntryType::OrderedRelation(rel1_sym));
    let alpha_mem_1 = rete.alpha.create_memory(entry1);

    let entry2 = rete
        .alpha
        .create_entry_node(AlphaEntryType::OrderedRelation(rel2_sym));
    let alpha_mem_2 = rete.alpha.create_memory(entry2);

    let root_id = rete.beta.root_id();
    let (join1_id, _) = rete
        .beta
        .create_join_node(root_id, alpha_mem_1, vec![], vec![]);
    let (join2_id, _) = rete
        .beta
        .create_join_node(join1_id, alpha_mem_2, vec![], vec![]);
    let _terminal = rete.beta.create_terminal_node(join2_id, rule_id);

    TwoPatternRete {
        rete,
        alpha_mem_1,
        alpha_mem_2,
    }
}

// ---------------------------------------------------------------------------
// Rete assertion helpers
// ---------------------------------------------------------------------------

/// Assert a set of facts into a Rete network and return the total activation count.
pub fn assert_facts_into_rete(
    rete: &mut ReteNetwork,
    engine: &Engine,
    fact_ids: &[FactId],
) -> usize {
    let mut count = 0;
    for &fact_id in fact_ids {
        let fact = engine
            .fact_base
            .get(fact_id)
            .expect("fact should exist in test helper");
        count += rete.assert_fact(fact_id, &fact.fact, &engine.fact_base).len();
    }
    count
}

/// Assert a single fact and return the list of new activation IDs.
#[allow(dead_code)] // Used by later Phase 2 passes
pub fn assert_one_fact(
    rete: &mut ReteNetwork,
    engine: &Engine,
    fact_id: FactId,
) -> Vec<ferric_core::ActivationId> {
    let fact = engine
        .fact_base
        .get(fact_id)
        .expect("fact should exist in test helper");
    rete.assert_fact(fact_id, &fact.fact, &engine.fact_base)
}

/// Retract a single fact from both the engine's fact base and the Rete network.
///
/// Returns the removed activations.
pub fn retract_one_fact(
    rete: &mut ReteNetwork,
    engine: &mut Engine,
    fact_id: FactId,
) -> Vec<ferric_core::Activation> {
    let fact = engine
        .fact_base
        .get(fact_id)
        .expect("fact should exist for retraction in test helper")
        .fact
        .clone();
    engine
        .fact_base
        .retract(fact_id)
        .expect("retract should succeed in test helper");
    rete.retract_fact(fact_id, &fact)
}

// ---------------------------------------------------------------------------
// Consistency assertion helpers
// ---------------------------------------------------------------------------

/// Assert full rete consistency. Panics on violation.
///
/// This calls `debug_assert_consistency()` on the rete network, which checks
/// token store, alpha network, beta network, agenda, and cross-structure
/// invariants (including any Phase 2 extensions as they are added).
pub fn assert_rete_consistent(rete: &ReteNetwork) {
    rete.debug_assert_consistency();
}

/// Assert that the rete network is fully clean (no tokens, no activations).
pub fn assert_rete_clean(rete: &ReteNetwork) {
    assert!(rete.token_store.is_empty(), "token store should be empty");
    assert!(rete.agenda.is_empty(), "agenda should be empty");
}

// ---------------------------------------------------------------------------
// Stage 2 interpretation helpers
// ---------------------------------------------------------------------------

/// Parse source and run Stage 2 interpretation, returning the result.
///
/// Uses default (non-strict) interpreter configuration.
#[allow(dead_code)] // Used by later Phase 2 passes
pub fn interpret_source(source: &str) -> ferric_parser::InterpretResult {
    let parsed = ferric_parser::parse_sexprs(source, ferric_parser::FileId(0));
    assert!(
        parsed.errors.is_empty(),
        "parse errors in test helper: {:?}",
        parsed.errors
    );
    let config = ferric_parser::InterpreterConfig::default();
    ferric_parser::interpret_constructs(&parsed.exprs, &config)
}

/// Parse source, interpret, and assert no errors. Returns the construct list.
///
/// # Panics
///
/// Panics if parsing or interpretation produces errors.
#[allow(dead_code)] // Used by later Phase 2 passes
pub fn interpret_ok(source: &str) -> Vec<ferric_parser::Construct> {
    let result = interpret_source(source);
    assert!(
        result.errors.is_empty(),
        "interpretation errors in test helper: {:?}",
        result.errors
    );
    result.constructs
}

// ---------------------------------------------------------------------------
// Phase 2 pipeline stubs (filled in by later passes)
// ---------------------------------------------------------------------------

// The following will be added as Phase 2 passes land:
//
// - `compile_rules(engine, constructs) -> ReteNetwork`
//     Compiles interpreted rule constructs into a shared rete network.
//
// - `run_to_completion(engine, rete) -> RunResult`
//     Executes the engine loop until halt or agenda exhaustion.
//
// - `step_once(engine, rete) -> StepResult`
//     Fires a single activation and returns the result.
