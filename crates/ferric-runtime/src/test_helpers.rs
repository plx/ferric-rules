//! Shared test helpers for integration tests.
//!
//! These helpers provide reusable building blocks for the full pipeline:
//! parse → interpret → compile → run. Originally established in Phase 2
//! and extended incrementally as new capabilities land.
//!
//! ## Phase 3 extensions
//!
//! - `run_to_completion`: Run engine until halt or agenda exhaustion.
//! - `load_and_run`: Convenience for load + run in one call.
//! - `assert_fact_count`: Verify expected fact count.
//! - `find_facts_by_relation`: Query facts by relation name.
//! - `assert_has_fact_with_relation`: Assert a fact with given relation exists.
//! - `load_fixture`: Load a `.clp` fixture file by name.

use ferric_core::beta::{RuleId, Salience};
use ferric_core::{
    AlphaEntryType, AlphaMemoryId, ConstantTest, FactId, ReteNetwork, StringEncoding,
};

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
    let (join_id, _) = rete
        .beta
        .create_join_node(root_id, alpha_mem_id, vec![], vec![]);
    let _terminal_id = rete
        .beta
        .create_terminal_node(join_id, rule_id, Salience::DEFAULT);

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
    let (join_id, _) = rete
        .beta
        .create_join_node(root_id, alpha_mem_id, vec![], vec![]);
    let _terminal_id = rete
        .beta
        .create_terminal_node(join_id, rule_id, Salience::DEFAULT);

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
    let _terminal = rete
        .beta
        .create_terminal_node(join2_id, rule_id, Salience::DEFAULT);

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
        count += rete
            .assert_fact(fact_id, &fact.fact, &engine.fact_base)
            .len();
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
    rete.retract_fact(fact_id, &fact, &engine.fact_base)
}

// ---------------------------------------------------------------------------
// Consistency assertion helpers
// ---------------------------------------------------------------------------

/// Assert full rete consistency. Panics on violation.
///
/// This calls `debug_assert_consistency()` on the rete network, which checks
/// token store, alpha network, beta network, agenda, and cross-structure
/// invariants (including any Phase 2 extensions as they are added).
#[cfg(any(test, debug_assertions))]
pub fn assert_rete_consistent(rete: &ReteNetwork) {
    rete.debug_assert_consistency();
}

/// Assert full engine consistency, including Phase 3 registries.
#[cfg(any(test, debug_assertions))]
pub fn assert_engine_consistent(engine: &Engine) {
    engine.debug_assert_consistency();
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
// Execution helpers (Phase 3)
// ---------------------------------------------------------------------------

/// Run the engine until the agenda is empty or halt is requested.
///
/// # Panics
///
/// Panics if the engine returns an error.
#[allow(dead_code)] // Will be used as Phase 3 passes land
pub fn run_to_completion(engine: &mut Engine) -> crate::execution::RunResult {
    engine
        .run(crate::execution::RunLimit::Unlimited)
        .expect("run should succeed in test helper")
}

/// Load source, then run to completion. Returns `(LoadResult, RunResult)`.
///
/// # Panics
///
/// Panics if loading or running produces errors.
#[allow(dead_code)] // Will be used as Phase 3 passes land
pub fn load_and_run(
    engine: &mut Engine,
    source: &str,
) -> (LoadResult, crate::execution::RunResult) {
    let load_result = load_ok(engine, source);
    let run_result = run_to_completion(engine);
    (load_result, run_result)
}

// ---------------------------------------------------------------------------
// Fact query helpers (Phase 3)
// ---------------------------------------------------------------------------

/// Assert that the engine contains exactly `expected` facts.
///
/// # Panics
///
/// Panics if the count doesn't match.
#[allow(dead_code)] // Will be used as Phase 3 passes land
pub fn assert_fact_count(engine: &Engine, expected: usize) {
    let actual = engine.facts().unwrap().count();
    assert_eq!(
        actual, expected,
        "expected {expected} facts, found {actual}"
    );
}

/// Find all fact IDs whose relation matches the given name.
///
/// Works with ordered facts only.
#[allow(dead_code)] // Will be used as Phase 3 passes land
pub fn find_facts_by_relation(engine: &Engine, relation: &str) -> Vec<ferric_core::FactId> {
    let relation_bytes = relation.as_bytes();
    engine
        .facts()
        .unwrap()
        .filter_map(|(fid, fact)| {
            if let ferric_core::Fact::Ordered(ordered) = fact {
                if engine.symbol_table.resolve_symbol(ordered.relation) == relation_bytes {
                    return Some(fid);
                }
            }
            None
        })
        .collect()
}

/// Assert that at least one fact with the given relation name exists.
///
/// # Panics
///
/// Panics if no matching fact is found.
#[allow(dead_code)] // Will be used as Phase 3 passes land
pub fn assert_has_fact_with_relation(engine: &Engine, relation: &str) {
    let facts = find_facts_by_relation(engine, relation);
    assert!(
        !facts.is_empty(),
        "expected at least one fact with relation `{relation}`"
    );
}

/// Assert that no fact with the given relation name exists.
///
/// # Panics
///
/// Panics if a matching fact is found.
#[allow(dead_code)] // Will be used as Phase 3 passes land
pub fn assert_no_fact_with_relation(engine: &Engine, relation: &str) {
    let facts = find_facts_by_relation(engine, relation);
    assert!(
        facts.is_empty(),
        "expected no facts with relation `{relation}`, found {}",
        facts.len()
    );
}

/// Get the first ordered fact matching the given relation, returning its fields.
///
/// # Panics
///
/// Panics if no matching fact is found or if the fact is not ordered.
#[allow(dead_code)] // Will be used as Phase 3 passes land
pub fn get_ordered_fields(engine: &Engine, relation: &str) -> Vec<ferric_core::Value> {
    let facts = find_facts_by_relation(engine, relation);
    assert!(
        !facts.is_empty(),
        "expected at least one `{relation}` fact for field inspection"
    );
    let entry = engine
        .fact_base
        .get(facts[0])
        .expect("fact should exist in test helper");
    match &entry.fact {
        ferric_core::Fact::Ordered(ordered) => ordered.fields.to_vec(),
        ferric_core::Fact::Template(_) => {
            panic!("expected ordered fact for `{relation}`, found template")
        }
    }
}

/// Get the ordered fields of a specific fact by its `FactId`.
///
/// # Panics
///
/// Panics if the fact does not exist or is not an ordered fact.
#[allow(dead_code)] // Used by Phase 3 generic dispatch tests
pub fn get_ordered_fields_for_fact(
    engine: &Engine,
    fact_id: ferric_core::FactId,
) -> Vec<ferric_core::Value> {
    let entry = engine
        .fact_base
        .get(fact_id)
        .expect("fact should exist in test helper");
    match &entry.fact {
        ferric_core::Fact::Ordered(ordered) => ordered.fields.to_vec(),
        ferric_core::Fact::Template(_) => {
            panic!("expected ordered fact for fact_id {fact_id:?}, found template")
        }
    }
}

// ---------------------------------------------------------------------------
// Fixture loading helpers (Phase 3)
// ---------------------------------------------------------------------------

/// Load a `.clp` fixture file by name from the `tests/fixtures/` directory.
///
/// # Panics
///
/// Panics if the file cannot be loaded.
#[allow(dead_code)] // Will be used as Phase 3 passes land
pub fn load_fixture(engine: &mut Engine, fixture_name: &str) -> LoadResult {
    let path = std::path::Path::new("tests/fixtures").join(fixture_name);
    engine
        .load_file(&path)
        .unwrap_or_else(|errors| panic!("fixture load failed for {fixture_name}: {errors:?}"))
}

// ---------------------------------------------------------------------------
// Diagnostic assertion helpers (Phase 3)
// ---------------------------------------------------------------------------

/// Load source and assert it fails with exactly one `UnsupportedForm` error
/// whose name matches `expected_form`.
///
/// # Panics
///
/// Panics if the load succeeds, or if the error doesn't match.
#[allow(dead_code)] // Will be used as Phase 3 passes land
pub fn assert_unsupported_form(engine: &mut Engine, source: &str, expected_form: &str) {
    let errors = load_err(engine, source);
    assert_eq!(
        errors.len(),
        1,
        "expected exactly one error for unsupported form `{expected_form}`, got {errors:?}"
    );
    match &errors[0] {
        crate::loader::LoadError::UnsupportedForm { name, .. } => {
            assert_eq!(
                name, expected_form,
                "expected unsupported form `{expected_form}`, got `{name}`"
            );
        }
        other => panic!("expected UnsupportedForm error for `{expected_form}`, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Phase 4: Module-qualified name and visibility helpers
// ---------------------------------------------------------------------------

/// Load source and assert it fails, returning the error messages as strings.
#[allow(dead_code)]
pub fn load_err_messages(engine: &mut Engine, source: &str) -> Vec<String> {
    let errors = load_err(engine, source);
    errors.iter().map(|e| format!("{e}")).collect()
}

/// Assert that loading source produces an error containing the given substring.
#[allow(dead_code)]
pub fn assert_load_error_contains(engine: &mut Engine, source: &str, expected_substring: &str) {
    let errors = load_err(engine, source);
    let messages: Vec<String> = errors.iter().map(|e| format!("{e}")).collect();
    assert!(
        messages.iter().any(|m| m.contains(expected_substring)),
        "expected error containing `{expected_substring}`, got: {messages:?}"
    );
}

/// Load source, run to completion, and return captured output for a channel.
#[allow(dead_code)]
pub fn load_run_output(engine: &mut Engine, source: &str, channel: &str) -> String {
    load_ok(engine, source);
    run_to_completion(engine);
    engine.get_output(channel).unwrap_or("").to_string()
}

/// Load source, run to completion, and return the stdout output (alias for "t" channel).
#[allow(dead_code)]
pub fn load_run_stdout(engine: &mut Engine, source: &str) -> String {
    load_run_output(engine, source, "t")
}

// ---------------------------------------------------------------------------
// Phase 4: Evaluator/expression test helpers
// ---------------------------------------------------------------------------

fn run_printout_expr_rule(
    engine: &mut Engine,
    setup_source: &str,
    rule_name: &str,
    expr: &str,
) -> String {
    let source = format!(
        r"
{setup_source}
(defrule {rule_name}
    (go)
    =>
    (printout t {expr} crlf))
(deffacts startup (go))
"
    );
    load_ok(engine, &source);
    engine.reset().expect("reset should succeed");
    run_to_completion(engine);
    engine.get_output("t").unwrap_or("").to_string()
}

/// Load a deffunction and a rule that calls it, run, and return the output.
///
/// The rule asserts `(go)`, the function is called in RHS via printout.
/// Returns the captured "t" channel output.
#[allow(dead_code)]
pub fn eval_function_via_rule(
    engine: &mut Engine,
    function_source: &str,
    call_expr: &str,
) -> String {
    run_printout_expr_rule(engine, function_source, "test-call", call_expr)
}

/// Evaluate an expression by embedding it in printout and capturing the output.
///
/// Wraps the expression in a rule RHS that prints the result.
/// Returns the captured "t" channel output (trimmed of trailing newline).
#[allow(dead_code)]
pub fn eval_expr_via_printout(engine: &mut Engine, expr: &str) -> String {
    run_printout_expr_rule(engine, "", "eval-test", expr)
        .trim_end_matches('\n')
        .to_string()
}

// ---------------------------------------------------------------------------
// Phase 4: Generic dispatch test helpers
// ---------------------------------------------------------------------------

/// Assert that calling a generic function with given arguments produces the
/// expected output value (printed to "t" channel).
#[allow(dead_code)]
pub fn assert_generic_dispatch_output(
    engine: &mut Engine,
    generic_source: &str,
    call_expr: &str,
    expected_output: &str,
) {
    let output = run_printout_expr_rule(engine, generic_source, "test-generic", call_expr);
    let output = output.trim_end_matches('\n');
    assert_eq!(
        output, expected_output,
        "generic dispatch output mismatch for `{call_expr}`"
    );
}

// ---------------------------------------------------------------------------
// Phase 4: Fact inspection helpers
// ---------------------------------------------------------------------------

/// Find all template facts for a given template name and return their slot maps.
#[allow(dead_code)]
pub fn find_template_facts(
    engine: &Engine,
    template_name: &str,
) -> Vec<Vec<(String, ferric_core::Value)>> {
    let Some(template_id) = engine.template_ids.get(template_name) else {
        return Vec::new();
    };
    engine
        .facts()
        .unwrap()
        .filter_map(|(_fid, fact)| {
            if let ferric_core::Fact::Template(tf) = fact {
                if tf.template_id == *template_id {
                    let mut slots: Vec<(String, ferric_core::Value)> = Vec::new();
                    if let Some(def) = engine.template_defs.get(*template_id) {
                        for name in &def.slot_names {
                            if let Some(&idx) = def.slot_index.get(name) {
                                if let Some(val) = tf.slots.get(idx) {
                                    slots.push((name.clone(), val.clone()));
                                }
                            }
                        }
                    }
                    return Some(slots);
                }
            }
            None
        })
        .collect()
}
