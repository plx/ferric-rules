//! CLIPS compatibility test harness.
//!
//! Provides helper functions for running CLIPS source through the Ferric engine
//! and asserting on the resulting output and working-memory state. Smoke tests
//! validate the harness itself before compatibility fixtures are added.

use ferric::core::Fact;
use ferric::runtime::{Engine, EngineConfig, HaltReason, LoadError, RunLimit};
use std::path::Path;

// ---------------------------------------------------------------------------
// Result type
// ---------------------------------------------------------------------------

/// Result of running a CLIPS compatibility test.
pub struct CompatResult {
    /// Number of rules that fired.
    pub rules_fired: usize,
    /// Captured output from the `t` (stdout) channel.
    pub output: String,
    /// Number of user-visible facts in working memory after execution.
    pub fact_count: usize,
}

// ---------------------------------------------------------------------------
// CompatEngine — retains the engine for post-execution inspection
// ---------------------------------------------------------------------------

/// An engine that has been loaded, reset, and run — ready for post-execution inspection.
///
/// Unlike [`CompatResult`], this wrapper keeps the engine alive so callers can
/// query working-memory state (fact counts, relation membership, etc.) after the
/// run has completed.
pub struct CompatEngine {
    engine: Engine,
    /// Number of rules that fired during the run.
    pub rules_fired: usize,
    /// Captured output from the `t` (stdout) channel.
    pub output: String,
}

impl CompatEngine {
    /// Count user-visible facts in working memory (excluding `initial-fact`).
    ///
    /// # Panics
    ///
    /// Panics if the engine returns an error from `facts()`.
    #[must_use]
    pub fn fact_count(&self) -> usize {
        self.engine
            .facts()
            .expect("CompatEngine::fact_count: facts() failed")
            .count()
    }

    /// Check whether any ordered fact with the given relation name exists in
    /// working memory.
    ///
    /// Iterates all user-visible facts and compares the relation symbol to
    /// `relation` by resolving the interned symbol back to a string. Template
    /// facts are never matched by this method.
    ///
    /// # Panics
    ///
    /// Panics if the engine returns an error from `facts()`.
    #[must_use]
    pub fn has_fact(&self, relation: &str) -> bool {
        self.engine
            .facts()
            .expect("CompatEngine::has_fact: facts() failed")
            .any(|(_, fact)| match fact {
                Fact::Ordered(of) => self.engine.resolve_symbol(of.relation) == Some(relation),
                Fact::Template(_) => false,
            })
    }

    /// Borrow the underlying engine for further inspection.
    #[must_use]
    pub fn engine(&self) -> &Engine {
        &self.engine
    }
}

// ---------------------------------------------------------------------------
// Core harness helpers
// ---------------------------------------------------------------------------

/// Default maximum rule firings per compatibility fixture run.
///
/// A finite ceiling prevents runaway fixtures from spinning forever and leaking
/// long-lived `clips_compat-*` processes.
const DEFAULT_COMPAT_RUN_LIMIT: usize = 10_000;

/// Environment variable for overriding the compatibility run limit locally.
const COMPAT_RUN_LIMIT_ENV: &str = "FERRIC_COMPAT_RUN_LIMIT";

/// Resolve the compatibility run limit from environment (or default).
fn compat_run_limit_count() -> usize {
    match std::env::var(COMPAT_RUN_LIMIT_ENV) {
        Ok(raw) => {
            let parsed = raw.trim().parse::<usize>().unwrap_or_else(|_| {
                panic!("{COMPAT_RUN_LIMIT_ENV} must be a positive integer, got {raw:?}")
            });
            assert!(
                parsed > 0,
                "{COMPAT_RUN_LIMIT_ENV} must be > 0, got {parsed}"
            );
            parsed
        }
        Err(std::env::VarError::NotPresent) => DEFAULT_COMPAT_RUN_LIMIT,
        Err(err) => panic!("failed to read {COMPAT_RUN_LIMIT_ENV}: {err}"),
    }
}

/// Run with the compatibility fixture safety limit and fail fast on non-quiescence.
fn run_compat_with_guard(engine: &mut Engine, context: &str) -> usize {
    let limit = compat_run_limit_count();
    let run_result = engine
        .run(RunLimit::Count(limit))
        .unwrap_or_else(|err| panic!("{context} run failed: {err:?}"));

    assert_ne!(
        run_result.halt_reason,
        HaltReason::LimitReached,
        "{context} reached compatibility run limit ({limit}). \
         Possible non-quiescing fixture/regression. \
         Increase {COMPAT_RUN_LIMIT_ENV} for local debugging if needed."
    );

    run_result.rules_fired
}

/// Build and execute a fresh compatibility engine, returning it for inspection.
fn run_clips_compat_engine(source: &str, context: &str) -> CompatEngine {
    let mut engine = Engine::new(EngineConfig::utf8());

    engine
        .load_str(source)
        .unwrap_or_else(|errors| panic!("{context} load_str failed: {errors:?}"));

    engine
        .reset()
        .unwrap_or_else(|_| panic!("{context} reset failed"));

    let rules_fired = run_compat_with_guard(&mut engine, context);
    let output = engine.get_output("t").unwrap_or("").to_string();

    CompatEngine {
        engine,
        rules_fired,
        output,
    }
}

/// Run CLIPS source through a fresh engine and return the compatibility result.
///
/// The sequence is:
/// 1. Create a new UTF-8 engine.
/// 2. Load `source` via `load_str`.
/// 3. Call `reset()` to assert deffacts and initialise globals.
/// 4. Call `run(Count(limit))` to fire all eligible rules with a safety ceiling.
/// 5. Capture output from the `t` channel and count facts.
///
/// # Panics
///
/// Panics if loading, reset, or run returns an error.
pub fn run_clips_compat(source: &str) -> CompatResult {
    let compat = run_clips_compat_engine(source, "clips_compat");
    let fact_count = compat
        .engine
        .facts()
        .expect("clips_compat facts() failed")
        .count();

    CompatResult {
        rules_fired: compat.rules_fired,
        output: compat.output,
        fact_count,
    }
}

/// Run CLIPS source and return a [`CompatEngine`] for post-execution inspection.
///
/// Unlike [`run_clips_compat`], this function retains the engine so callers can
/// query working-memory state after the run.
///
/// # Panics
///
/// Panics if loading, reset, or run returns an error.
pub fn run_clips_compat_full(source: &str) -> CompatEngine {
    run_clips_compat_engine(source, "run_clips_compat_full")
}

/// Run CLIPS source and assert the `t` channel output equals `expected`.
///
/// # Panics
///
/// Panics if the output does not match.
pub fn assert_clips_compat(source: &str, expected: &str) {
    let _ = assert_clips_compat_returns(source, expected);
}

/// Run a fixture `.clp` file relative to `tests/clips_compat/fixtures/` and
/// return the compatibility result.
///
/// The `fixture_name` may include subdirectory path components, e.g.
/// `"core/basic_match.clp"` or `"negation/simple_negation.clp"`.
///
/// The path to `fixtures/` is resolved from the workspace root using the
/// `CARGO_MANIFEST_DIR` of this crate, navigating up to the workspace root.
///
/// # Panics
///
/// Panics if the file cannot be read or if the engine returns an error.
pub fn run_clips_compat_file(fixture_name: &str) -> CompatResult {
    // CARGO_MANIFEST_DIR points to `crates/ferric`; walk up two levels to the
    // workspace root, then into `tests/clips_compat/fixtures/`.
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent() // crates/
        .and_then(|p| p.parent()) // workspace root
        .expect("could not navigate to workspace root from CARGO_MANIFEST_DIR");

    let fixture_path = workspace_root
        .join("tests")
        .join("clips_compat")
        .join("fixtures")
        .join(fixture_name);

    let source = std::fs::read_to_string(&fixture_path)
        .unwrap_or_else(|e| panic!("could not read fixture {fixture_name:?}: {e}"));

    run_clips_compat(&source)
}

// ---------------------------------------------------------------------------
// Assertion convention helpers
// ---------------------------------------------------------------------------

/// Assert that the engine output exactly matches `expected`.
///
/// # Panics
///
/// Panics if the output does not match.
pub fn assert_output_exact(result: &CompatResult, expected: &str) {
    assert_eq!(
        result.output, expected,
        "Output mismatch:\n  expected: {:?}\n    actual: {:?}",
        expected, result.output
    );
}

/// Assert the number of rules fired.
///
/// # Panics
///
/// Panics if the rule-fired count does not match.
pub fn assert_rules_fired(result: &CompatResult, expected: usize) {
    assert_eq!(
        result.rules_fired, expected,
        "Rules fired mismatch: expected {expected}, got {}",
        result.rules_fired
    );
}

/// Assert fact count (excluding `initial-fact`).
///
/// # Panics
///
/// Panics if the fact count does not match.
pub fn assert_fact_count_compat(result: &CompatResult, expected: usize) {
    assert_eq!(
        result.fact_count, expected,
        "Fact count mismatch: expected {expected}, got {}",
        result.fact_count
    );
}

// ---------------------------------------------------------------------------
// Smoke tests
// ---------------------------------------------------------------------------

#[test]
fn test_harness_smoke_simple_assert() {
    let source = r#"
(deffacts startup (person Alice))
(defrule greet
    (person ?name)
    =>
    (printout t "Hello " ?name crlf))
"#;
    let result = assert_clips_compat_returns(source, "Hello Alice\n");
    assert_eq!(result.rules_fired, 1, "expected exactly 1 rule to fire");
    assert_eq!(result.fact_count, 1, "expected 1 fact in working memory");
}

#[test]
fn test_harness_smoke_no_rules() {
    // Only deffacts, no rules — nothing should fire.
    let source = "(deffacts startup (data 42))";
    let result = run_clips_compat(source);
    assert_eq!(result.rules_fired, 0, "expected 0 rules to fire");
    assert_eq!(result.fact_count, 1, "expected 1 fact in working memory");
    assert_eq!(result.output, "", "expected no output");
}

#[test]
fn test_harness_smoke_chain() {
    // Rule 1 asserts (step-two); rule 2 fires on (step-two) and prints.
    let source = r#"
(deffacts startup (step-one))

(defrule rule-one
    (step-one)
    =>
    (assert (step-two)))

(defrule rule-two
    (step-two)
    =>
    (printout t "chain fired" crlf))
"#;
    let result = run_clips_compat(source);
    assert_eq!(result.rules_fired, 2, "expected both rules to fire");
    assert_eq!(result.output, "chain fired\n");
}

#[test]
fn test_harness_smoke_fixture_file() {
    // Load the trivial smoke.clp fixture and verify the output.
    let result = run_clips_compat_file("smoke.clp");
    assert_eq!(result.rules_fired, 1, "expected 1 rule from smoke.clp");
    assert_eq!(result.output, "Got: hello\n");
}

#[test]
fn test_harness_compat_engine_fact_count() {
    let source = r"
(deffacts startup (a 1) (b 2) (c 3))
(defrule noop (a ?x) => )
";
    let compat = run_clips_compat_full(source);
    assert_eq!(compat.rules_fired, 1);
    assert_eq!(compat.fact_count(), 3);
}

#[test]
fn test_harness_compat_engine_has_fact() {
    let source = r"
(deffacts startup (person Alice) (city London))
(defrule noop (person ?x) => )
";
    let compat = run_clips_compat_full(source);
    assert!(compat.has_fact("person"), "expected 'person' fact");
    assert!(compat.has_fact("city"), "expected 'city' fact");
    assert!(
        !compat.has_fact("country"),
        "should not have 'country' fact"
    );
}

#[test]
fn test_harness_assertion_helpers() {
    let source = r"
(deffacts startup (item x) (item y))
(defrule count-items (item ?x) => (printout t ?x crlf))
";
    let result = run_clips_compat(source);
    assert_rules_fired(&result, 2);
    assert_fact_count_compat(&result, 2);
}

#[test]
fn test_harness_fixture_subdirectory() {
    // Verify that subdirectory paths work with run_clips_compat_file.
    // Since core/ only has .gitkeep, exercise the path-join logic via
    // the existing smoke.clp at the top level.
    let result = run_clips_compat_file("smoke.clp");
    assert_eq!(result.output, "Got: hello\n");
}

// ---------------------------------------------------------------------------
// Internal helper (not part of the public harness API)
// ---------------------------------------------------------------------------

/// Run and assert output, returning the full result for further inspection.
fn assert_clips_compat_returns(source: &str, expected: &str) -> CompatResult {
    let result = run_clips_compat(source);
    assert_eq!(
        result.output, expected,
        "CLIPS compat output mismatch\n  expected: {expected:?}\n  actual:   {:?}",
        result.output,
    );
    result
}

// ===========================================================================
// Module domain compatibility tests
// ===========================================================================

#[test]
fn test_compat_modules_basic_module() {
    let result = run_clips_compat_file("modules/basic_module.clp");
    assert_eq!(result.rules_fired, 1, "expected 1 rule to fire");
    assert_eq!(result.output, "Sensor temp = 72\n");
}

#[test]
fn test_compat_modules_global_scope() {
    let result = run_clips_compat_file("modules/global_scope.clp");
    assert_eq!(result.rules_fired, 2, "expected 2 rules to fire");
    // Both items fire; counter increments 0->1 then 1->2.
    assert_eq!(result.output, "count = 1\ncount = 2\n");
}

#[test]
fn test_compat_modules_qualified_names() {
    let result = run_clips_compat_file("modules/qualified_names.clp");
    assert_eq!(result.rules_fired, 1, "expected 1 rule to fire");
    assert_eq!(result.output, "sum: 7\nthreshold: 10\n");
}

// ===========================================================================
// Generic domain compatibility tests
// ===========================================================================

#[test]
fn test_compat_generics_basic_dispatch() {
    let result = run_clips_compat_file("generics/basic_dispatch.clp");
    assert_eq!(result.rules_fired, 2, "expected 2 rules to fire");
    // Both lines must appear; order depends on conflict resolution strategy.
    assert!(
        result.output.contains("integer: 42\n"),
        "expected 'integer: 42' in output, got: {:?}",
        result.output
    );
    assert!(
        result.output.contains("string: hello\n"),
        "expected 'string: hello' in output, got: {:?}",
        result.output
    );
}

#[test]
fn test_compat_generics_specificity() {
    let result = run_clips_compat_file("generics/specificity.clp");
    assert_eq!(result.rules_fired, 1, "expected 1 rule to fire");
    assert_eq!(
        result.output, "integer\n",
        "INTEGER method should win over NUMBER method"
    );
}

// ===========================================================================
// Stdlib domain compatibility tests
// ===========================================================================

#[test]
fn test_compat_stdlib_math_ops() {
    let result = run_clips_compat_file("stdlib/math_ops.clp");
    assert_eq!(result.rules_fired, 1, "expected 1 rule to fire");
    assert_eq!(
        result.output,
        // Note: Ferric returns 25.0 for (/ 100 4) — division always returns float.
        "add: 30\nsub: 42\nmul: 42\ndiv: 25.0\nmod: 2\nabs: 42\nmin: 1\nmax: 9\n"
    );
}

#[test]
fn test_compat_stdlib_string_ops() {
    let result = run_clips_compat_file("stdlib/string_ops.clp");
    assert_eq!(result.rules_fired, 1, "expected 1 rule to fire");
    assert_eq!(result.output, "cat: hello world\nlen: 5\nsub: hel\n");
}

#[test]
fn test_compat_stdlib_multifield_ops() {
    let result = run_clips_compat_file("stdlib/multifield_ops.clp");
    assert_eq!(result.rules_fired, 1, "expected 1 rule to fire");
    // length$ = 4, nth$ 2 = b, member$ b = 2 (1-based position)
    assert_eq!(result.output, "len: 4\nnth: b\nmember: 2\n");
}

#[test]
fn test_compat_stdlib_predicate_ops() {
    let result = run_clips_compat_file("stdlib/predicate_ops.clp");
    assert_eq!(result.rules_fired, 1, "expected 1 rule to fire");
    assert_eq!(
        result.output,
        "int? TRUE\nfloat? TRUE\nsym? TRUE\nstr? TRUE\nnum? TRUE\neq: TRUE\n"
    );
}

// ===========================================================================
// Core domain compatibility tests
// ===========================================================================

#[test]
fn test_compat_core_basic_match() {
    let result = run_clips_compat_file("core/basic_match.clp");
    assert_eq!(result.rules_fired, 3);
    // Depth strategy: most recently asserted facts match first (reverse assertion order)
    assert_eq!(result.output, "Color: green\nColor: blue\nColor: red\n");
}

#[test]
fn test_compat_core_retract_cycle() {
    let result = run_clips_compat_file("core/retract_cycle.clp");
    assert_eq!(result.rules_fired, 1);
    assert_eq!(result.fact_count, 1); // processed fact remains
    assert_eq!(result.output, "");
}

#[test]
fn test_compat_core_salience_order() {
    let result = run_clips_compat_file("core/salience_order.clp");
    assert_eq!(result.rules_fired, 2);
    assert_eq!(result.output, "high\nlow\n");
}

#[test]
fn test_compat_core_chain_rules() {
    let result = run_clips_compat_file("core/chain_rules.clp");
    assert_eq!(result.rules_fired, 3);
    assert_eq!(result.output, "1->2\n2->3\ndone\n");
}

#[test]
fn test_compat_core_modify_duplicate() {
    let result = run_clips_compat_file("core/modify_duplicate.clp");
    assert_eq!(result.rules_fired, 1);
    assert_eq!(result.output, "Alice is now 31\n");
}

// ===========================================================================
// Negation domain compatibility tests
// ===========================================================================

#[test]
fn test_compat_negation_simple_not() {
    let result = run_clips_compat_file("negation/simple_not.clp");
    assert_eq!(result.rules_fired, 1);
    assert_eq!(result.output, "lamp is safe\n");
}

#[test]
fn test_compat_negation_not_retract() {
    let result = run_clips_compat_file("negation/not_retract.clp");
    assert_eq!(result.rules_fired, 2);
    assert_eq!(result.output, "danger removed\nlamp is safe\n");
}

#[test]
fn test_compat_negation_exists() {
    let result = run_clips_compat_file("negation/exists_ce.clp");
    assert_eq!(result.rules_fired, 1);
    assert_eq!(result.output, "signal detected\n");
}

#[test]
fn test_compat_negation_forall_basic() {
    let result = run_clips_compat_file("negation/forall_basic.clp");
    assert_eq!(result.rules_fired, 1);
    assert_eq!(result.output, "all items checked\n");
}

#[test]
fn test_compat_negation_forall_fail() {
    let result = run_clips_compat_file("negation/forall_fail.clp");
    assert_eq!(result.rules_fired, 0);
    assert_eq!(result.output, "");
}

// ===========================================================================
// Core domain — deeper coverage
// ===========================================================================

/// Multi-pattern join: a rule with two patterns joined by a shared variable.
#[test]
fn test_compat_core_multi_pattern_join() {
    let result = run_clips_compat_file("core/multi_pattern_join.clp");
    assert_rules_fired(&result, 1);
    assert_output_exact(&result, "Alice is 30\n");
    assert_fact_count_compat(&result, 2);
}

/// Refraction: a rule fires at most once per token even if re-run would match.
#[test]
fn test_compat_core_refraction() {
    let result = run_clips_compat_file("core/refraction.clp");
    assert_rules_fired(&result, 1);
    assert_output_exact(&result, "processed a\n");
}

/// Depth conflict-resolution strategy fires the most recently asserted fact first.
#[test]
fn test_compat_core_multiple_activations_depth() {
    let result = run_clips_compat_file("core/multiple_activations_depth.clp");
    assert_rules_fired(&result, 3);
    // deffacts asserts a, b, c in order; depth fires most recent first: c, b, a
    assert_output_exact(&result, "c\nb\na\n");
}

/// Chained retraction: higher-salience rule retracts a fact before the lower rule can fire.
#[test]
fn test_compat_core_retract_chain() {
    let result = run_clips_compat_file("core/retract_chain.clp");
    // rule-a fires (salience 10), retracts (a 1); rule-b never fires
    assert_rules_fired(&result, 1);
    assert_output_exact(&result, "retracted a\n");
    // (b 1) remains in WM; (a 1) was retracted
    assert_fact_count_compat(&result, 1);
}

/// Halt stops the run loop: step2 must not fire after step1 halts.
#[test]
fn test_compat_core_halt_stops_execution() {
    let result = run_clips_compat_file("core/halt_stops_execution.clp");
    assert_rules_fired(&result, 1);
    assert_output_exact(&result, "step1\n");
}

/// Bind action in RHS: bind runs without error and subsequent actions still execute.
#[test]
fn test_compat_core_bind_in_rhs() {
    let result = run_clips_compat_file("core/bind_in_rhs.clp");
    assert_rules_fired(&result, 1);
    assert_output_exact(&result, "computed\n");
}

// ===========================================================================
// Negation domain — deeper coverage
// ===========================================================================

/// Not with variable binding: only persons not listed in banned are allowed.
#[test]
fn test_compat_negation_not_multiple_patterns() {
    let result = run_clips_compat_file("negation/not_multiple_patterns.clp");
    // Alice is not banned → fires; Bob is banned → does not fire
    assert_rules_fired(&result, 1);
    assert_output_exact(&result, "Alice allowed\n");
}

/// Exists fires exactly once regardless of how many facts satisfy the pattern.
#[test]
fn test_compat_negation_exists_count() {
    let result = run_clips_compat_file("negation/exists_count.clp");
    assert_rules_fired(&result, 1);
    assert_output_exact(&result, "signal present\n");
}

/// Forall with empty quantified set is vacuously true and the rule fires.
#[test]
fn test_compat_negation_forall_vacuous_truth() {
    let result = run_clips_compat_file("negation/forall_vacuous_truth.clp");
    assert_rules_fired(&result, 1);
    assert_output_exact(&result, "all done\n");
}

/// Negated conjunction (NCC): rule fires when it is NOT the case that both (a) and (b) exist.
#[test]
fn test_compat_negation_ncc_basic() {
    let result = run_clips_compat_file("negation/ncc_basic.clp");
    assert_rules_fired(&result, 1);
    assert_output_exact(&result, "no a+b pair\n");
}

/// Forall retract invalidation: forall fires when satisfied, then retraction of a supporting
/// fact fires the remove-check rule but does NOT re-fire check-all (refraction prevents it).
#[test]
fn test_compat_negation_forall_retract_invalidation() {
    let result = run_clips_compat_file("negation/forall_retract_invalidation.clp");
    assert_rules_fired(&result, 2);
    assert_output_exact(&result, "all checked\nremoved check\n");
}

// ===========================================================================
// Module domain — deeper coverage
// ===========================================================================

/// Focus stack drives execution order across modules.
/// MAIN fires first (default module), then A, then B after focus is pushed.
#[test]
fn test_compat_modules_multi_module_focus() {
    let result = run_clips_compat_file("modules/multi_module_focus.clp");
    assert_rules_fired(&result, 3);
    assert_output_exact(&result, "MAIN\nA\nB\n");
}

/// Global variable incremented from RHS across multiple rule firings.
#[test]
fn test_compat_modules_global_bind() {
    let result = run_clips_compat_file("modules/global_bind.clp");
    assert_rules_fired(&result, 3);
    // Each of the 3 item facts fires the rule; counter increments 0->1->2->3.
    assert_output_exact(&result, "count now 1\ncount now 2\ncount now 3\n");
}

/// User-defined function (deffunction) called from rule RHS.
#[test]
fn test_compat_modules_deffunction_call() {
    let result = run_clips_compat_file("modules/deffunction_call.clp");
    assert_rules_fired(&result, 1);
    assert_output_exact(&result, "double: 42\n");
}

/// Deffunction using str-cat for string construction, called from rule RHS.
#[test]
fn test_compat_modules_deffunction_str() {
    let result = run_clips_compat_file("modules/deffunction_str.clp");
    assert_rules_fired(&result, 1);
    assert_output_exact(&result, "Hello Alice!\n");
}

// ===========================================================================
// Generic domain — deeper coverage
// ===========================================================================

/// Multiple methods on a generic dispatch by type: INTEGER and SYMBOL.
#[test]
fn test_compat_generics_multi_method() {
    let result = run_clips_compat_file("generics/multi_method.clp");
    assert_rules_fired(&result, 1);
    assert_output_exact(&result, "int:42\nsym:hello\n");
}

/// Generic called from within a deffunction; deffunction called from rule RHS.
#[test]
fn test_compat_generics_method_with_deffunction() {
    let result = run_clips_compat_file("generics/method_with_deffunction.clp");
    assert_rules_fired(&result, 1);
    assert_output_exact(&result, "integer value\nfloat value\nsymbol value\n");
}

// ===========================================================================
// Stdlib domain — deeper coverage
// ===========================================================================

/// Advanced math: integer division, type conversion, abs on negative.
#[test]
fn test_compat_stdlib_math_advanced() {
    let result = run_clips_compat_file("stdlib/math_advanced.clp");
    assert_rules_fired(&result, 1);
    // div returns integer; float conversion of 42 gives 42.0; integer(3) stays 3; abs(-99)=99
    assert_output_exact(
        &result,
        "int-div: 3\nfloat: 42.0\ninteger: 3\nabs-neg: 99\n",
    );
}

/// String functions: sym-cat, str-length, sub-string.
#[test]
fn test_compat_stdlib_string_advanced() {
    let result = run_clips_compat_file("stdlib/string_advanced.clp");
    assert_rules_fired(&result, 1);
    assert_output_exact(&result, "sym-cat: abcdef\nstr-len: 11\nsub-str: hello\n");
}

/// Comparison operators: >, <, >=, <=, <>, eq.
/// Note: numeric `=` cannot be used as a function call expression (lexer limitation).
#[test]
fn test_compat_stdlib_comparison_ops() {
    let result = run_clips_compat_file("stdlib/comparison_ops.clp");
    assert_rules_fired(&result, 1);
    assert_output_exact(
        &result,
        "gt: TRUE\nlt: TRUE\ngte: TRUE\nlte: TRUE\nneq-num: TRUE\neq-sym: TRUE\n",
    );
}

/// Logical operations: and, or, not.
#[test]
fn test_compat_stdlib_logical_ops() {
    let result = run_clips_compat_file("stdlib/logical_ops.clp");
    assert_rules_fired(&result, 1);
    assert_output_exact(
        &result,
        "and: TRUE\nor: TRUE\nnot: TRUE\nand-false: FALSE\n",
    );
}

/// Type predicate functions: evenp, oddp, lexemep.
#[test]
fn test_compat_stdlib_type_predicates() {
    let result = run_clips_compat_file("stdlib/type_predicates.clp");
    assert_rules_fired(&result, 1);
    assert_output_exact(
        &result,
        "evenp-4: TRUE\nevenp-3: FALSE\noddp-7: TRUE\noddp-6: FALSE\nlexemep-sym: TRUE\nlexemep-int: FALSE\n",
    );
}

// ===========================================================================
// Module domain — qualified names and cross-module visibility
// ===========================================================================

/// Module-qualified global variable: read and write using `?*MODULE::name*` syntax.
/// CONFIG exports its global; MAIN imports it and binds via the qualified name.
#[test]
fn test_compat_modules_qualified_global_bind() {
    let result = run_clips_compat_file("modules/qualified_global_bind.clp");
    assert_rules_fired(&result, 1);
    // base-value starts at 10, bind sets it to 10*3=30.
    assert_output_exact(&result, "value: 30\n");
}

/// Cross-module function import: UTILS exports deffunction; MAIN imports and calls it.
#[test]
fn test_compat_modules_cross_module_import() {
    let result = run_clips_compat_file("modules/cross_module_import.clp");
    assert_rules_fired(&result, 1);
    assert_output_exact(&result, "square-5: 25\nsquare-9: 81\n");
}

/// Visibility boundary: loading a function from a non-exporting module via
/// qualified call should produce an action diagnostic (not silently succeed).
#[test]
fn test_compat_modules_visibility_boundary_not_exported() {
    let source = r"
(defmodule MATH (export ?NONE))
(deffunction add (?x ?y) (+ ?x ?y))

(defmodule MAIN)
(defrule call-hidden (go) => (printout t (MATH::add 1 2) crlf))
(deffacts startup (go))
";
    let compat = run_clips_compat_full(source);
    // The rule fires (the call executes), but a visibility error must be recorded.
    let diagnostics = compat.engine().action_diagnostics();
    assert!(
        !diagnostics.is_empty(),
        "expected a visibility diagnostic for unexported MATH::add, got none"
    );
    let has_visibility_error = diagnostics.iter().any(|d| {
        let msg = format!("{d}").to_ascii_lowercase();
        msg.contains("not visible") || msg.contains("notvisible") || msg.contains("visibility")
    });
    assert!(
        has_visibility_error,
        "expected visibility error in diagnostics, got: {diagnostics:?}"
    );
}

/// Unsupported top-level form: loading `defclass` produces a source-located `LoadError`.
#[test]
fn test_compat_modules_unsupported_form_diagnostic() {
    let mut engine = Engine::new(EngineConfig::utf8());
    let source = "(defclass Point (is-a USER) (slot x) (slot y))";
    let errors = engine
        .load_str(source)
        .expect_err("expected load to fail for unsupported defclass");
    let has_unsupported = errors.iter().any(|e| {
        matches!(
            e,
            LoadError::UnsupportedForm {
                name,
                ..
            } if name == "defclass"
        )
    });
    assert!(
        has_unsupported,
        "expected UnsupportedForm(defclass) in load errors, got: {errors:?}"
    );
    // Verify the error includes line/column location information.
    let located = errors.iter().any(|e| {
        if let LoadError::UnsupportedForm { line, column, .. } = e {
            *line >= 1 && *column >= 1
        } else {
            false
        }
    });
    assert!(
        located,
        "expected source-located (line >= 1, column >= 1) UnsupportedForm error"
    );
}

// ===========================================================================
// Generic domain — dispatch ordering and call-next-method
// ===========================================================================

/// Generic dispatch ordering: most specific type wins (INTEGER > NUMBER > any).
#[test]
fn test_compat_generics_dispatch_ordering() {
    let result = run_clips_compat_file("generics/dispatch_ordering.clp");
    assert_rules_fired(&result, 1);
    assert_output_exact(&result, "int: integer\nfloat: float\nsym: any\n");
}

/// call-next-method: INTEGER method calls next-less-specific NUMBER method,
/// composing the results.
#[test]
fn test_compat_generics_call_next_method() {
    let result = run_clips_compat_file("generics/call_next_method.clp");
    assert_rules_fired(&result, 1);
    // Integer 7: INTEGER method prepends "int+" then delegates to NUMBER method → "int+num(7)"
    // Float 2.5: only NUMBER method applies → "num(2.5)"
    assert_output_exact(&result, "int+num(7)\nnum(2.5)\n");
}

// ===========================================================================
// Stdlib domain — multifield, format, math edge cases
// ===========================================================================

/// Advanced multifield: create$, length$, nth$ (1-based), member$ (returns position).
#[test]
fn test_compat_stdlib_multifield_advanced() {
    let result = run_clips_compat_file("stdlib/multifield_advanced.clp");
    assert_rules_fired(&result, 1);
    // create$(a b c d e): length=5, 2nd=b, member$(c)=3 (1-based), member$(z)=FALSE
    assert_output_exact(&result, "len: 5\n2nd: b\npos-c: 3\npos-z: FALSE\n");
}

/// format function: printf-style formatting returns a string, printed via printout.
/// Note: format does not write to the router; the result must be passed to printout.
#[test]
fn test_compat_stdlib_format_output() {
    let result = run_clips_compat_file("stdlib/format_output.clp");
    assert_rules_fired(&result, 1);
    assert_output_exact(&result, "num=42\nstr=hello\nflt=3.5\n");
}

/// Math edge cases: multi-arg min/max, abs, integer division, modulus.
#[test]
fn test_compat_stdlib_math_edge_cases() {
    let result = run_clips_compat_file("stdlib/math_edge_cases.clp");
    assert_rules_fired(&result, 1);
    assert_output_exact(
        &result,
        "min3: 1\nmax3: 3\nneg-abs: 7\ndiv-trunc: 3\nmod-neg: 1\n",
    );
}
