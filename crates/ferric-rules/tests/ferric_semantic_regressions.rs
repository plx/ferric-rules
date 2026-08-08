//! Ferric semantic regression test harness.
//!
//! Provides helper functions for running CLIPS-language source through the
//! Ferric engine and asserting on the resulting output and working-memory
//! state. This suite does not execute reference CLIPS; external conformance is
//! owned by the differential compatibility lane.

use ferric_rules::core::{ConflictResolutionStrategy, Fact};
use ferric_rules::runtime::evaluator::EvalError;
use ferric_rules::runtime::{ActionError, Engine, EngineConfig, HaltReason, LoadError, RunLimit};
use std::path::Path;

// ---------------------------------------------------------------------------
// Result type
// ---------------------------------------------------------------------------

/// Result of running a Ferric semantic regression test.
pub struct RegressionResult {
    /// Number of rules that fired.
    pub rules_fired: usize,
    /// Captured output from the `t` (stdout) channel.
    pub output: String,
    /// Number of user-visible facts in working memory after execution.
    pub fact_count: usize,
}

// ---------------------------------------------------------------------------
// RegressionEngine — retains the engine for post-execution inspection
// ---------------------------------------------------------------------------

/// An engine that has been loaded, reset, and run — ready for post-execution inspection.
///
/// Unlike [`RegressionResult`], this wrapper keeps the engine alive so callers can
/// query working-memory state (fact counts, relation membership, etc.) after the
/// run has completed.
pub struct RegressionEngine {
    engine: Engine,
    /// Number of rules that fired during the run.
    pub rules_fired: usize,
    /// Captured output from the `t` (stdout) channel.
    pub output: String,
}

impl RegressionEngine {
    /// Count user-visible facts in working memory (excluding `initial-fact`).
    ///
    /// # Panics
    ///
    /// Panics if the engine returns an error from `facts()`.
    #[must_use]
    pub fn fact_count(&self) -> usize {
        self.engine
            .facts()
            .expect("RegressionEngine::fact_count: facts() failed")
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
            .expect("RegressionEngine::has_fact: facts() failed")
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

/// Default maximum rule firings per semantic-regression fixture run.
///
/// A finite ceiling prevents runaway fixtures from spinning forever and leaking
/// long-lived `ferric_semantic_regression-*` processes.
const DEFAULT_SEMANTIC_REGRESSION_RUN_LIMIT: usize = 10_000;

/// Environment variable for overriding the semantic-regression run limit locally.
const SEMANTIC_REGRESSION_RUN_LIMIT_ENV: &str = "FERRIC_SEMANTIC_REGRESSION_RUN_LIMIT";

/// Resolve the semantic-regression run limit from environment (or default).
fn semantic_regression_run_limit_count() -> usize {
    match std::env::var(SEMANTIC_REGRESSION_RUN_LIMIT_ENV) {
        Ok(raw) => {
            let parsed = raw.trim().parse::<usize>().unwrap_or_else(|_| {
                panic!(
                    "{SEMANTIC_REGRESSION_RUN_LIMIT_ENV} must be a positive integer, got {raw:?}"
                )
            });
            assert!(
                parsed > 0,
                "{SEMANTIC_REGRESSION_RUN_LIMIT_ENV} must be > 0, got {parsed}"
            );
            parsed
        }
        Err(std::env::VarError::NotPresent) => DEFAULT_SEMANTIC_REGRESSION_RUN_LIMIT,
        Err(err) => panic!("failed to read {SEMANTIC_REGRESSION_RUN_LIMIT_ENV}: {err}"),
    }
}

/// Run with the fixture safety limit and fail fast on non-quiescence.
fn run_regression_with_guard(engine: &mut Engine, context: &str) -> usize {
    let limit = semantic_regression_run_limit_count();
    let run_result = engine
        .run(RunLimit::Count(limit))
        .unwrap_or_else(|err| panic!("{context} run failed: {err:?}"));

    assert_ne!(
        run_result.halt_reason,
        HaltReason::LimitReached,
        "{context} reached semantic-regression run limit ({limit}). \
         Possible non-quiescing fixture/regression. \
         Increase {SEMANTIC_REGRESSION_RUN_LIMIT_ENV} for local debugging if needed."
    );

    run_result.rules_fired
}

/// Build and execute a fresh Ferric engine, returning it for inspection.
fn run_ferric_semantic_regression_engine(source: &str, context: &str) -> RegressionEngine {
    run_ferric_semantic_regression_engine_with_strategy(
        source,
        context,
        ConflictResolutionStrategy::Depth,
    )
}

/// Build and execute a fresh Ferric engine with an explicit agenda strategy.
fn run_ferric_semantic_regression_engine_with_strategy(
    source: &str,
    context: &str,
    strategy: ConflictResolutionStrategy,
) -> RegressionEngine {
    let mut engine = Engine::new(EngineConfig::utf8().with_strategy(strategy));

    engine
        .load_str(source)
        .unwrap_or_else(|errors| panic!("{context} load_str failed: {errors:?}"));

    engine
        .reset()
        .unwrap_or_else(|_| panic!("{context} reset failed"));

    let rules_fired = run_regression_with_guard(&mut engine, context);
    let output = engine.get_output("t").unwrap_or("").to_string();

    RegressionEngine {
        engine,
        rules_fired,
        output,
    }
}

/// Run CLIPS-language source through a fresh Ferric engine.
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
pub fn run_ferric_semantic_regression(source: &str) -> RegressionResult {
    let regression = run_ferric_semantic_regression_engine(source, "ferric_semantic_regression");
    let fact_count = regression
        .engine
        .facts()
        .expect("ferric_semantic_regression facts() failed")
        .count();

    RegressionResult {
        rules_fired: regression.rules_fired,
        output: regression.output,
        fact_count,
    }
}

/// Run CLIPS source and return a [`RegressionEngine`] for post-execution inspection.
///
/// Unlike [`run_ferric_semantic_regression`], this function retains the engine so callers can
/// query working-memory state after the run.
///
/// # Panics
///
/// Panics if loading, reset, or run returns an error.
pub fn run_ferric_semantic_regression_full(source: &str) -> RegressionEngine {
    run_ferric_semantic_regression_engine(source, "run_ferric_semantic_regression_full")
}

/// Run CLIPS source and assert the `t` channel output equals `expected`.
///
/// # Panics
///
/// Panics if the output does not match.
pub fn assert_ferric_semantic_regression(source: &str, expected: &str) {
    let _ = assert_ferric_semantic_regression_returns(source, expected);
}

/// Run a fixture `.clp` file relative to this package's `tests/fixtures/` and
/// return the semantic-regression result.
///
/// The `fixture_name` may include subdirectory path components, e.g.
/// `"core/basic_match.clp"` or `"negation/simple_negation.clp"`.
///
/// The path to `fixtures/` is resolved from this package's
/// `CARGO_MANIFEST_DIR`, so the same tests run from an extracted `.crate`.
///
/// # Panics
///
/// Panics if the file cannot be read or if the engine returns an error.
pub fn run_ferric_semantic_regression_file(fixture_name: &str) -> RegressionResult {
    let fixture_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(fixture_name);

    let source = std::fs::read_to_string(&fixture_path)
        .unwrap_or_else(|e| panic!("could not read fixture {fixture_name:?}: {e}"));

    run_ferric_semantic_regression(&source)
}

// ---------------------------------------------------------------------------
// Assertion convention helpers
// ---------------------------------------------------------------------------

/// Assert that the engine output exactly matches `expected`.
///
/// # Panics
///
/// Panics if the output does not match.
pub fn assert_output_exact(result: &RegressionResult, expected: &str) {
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
pub fn assert_rules_fired(result: &RegressionResult, expected: usize) {
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
pub fn assert_fact_count(result: &RegressionResult, expected: usize) {
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
    let result = assert_ferric_semantic_regression_returns(source, "Hello Alice\n");
    assert_eq!(result.rules_fired, 1, "expected exactly 1 rule to fire");
    assert_eq!(result.fact_count, 1, "expected 1 fact in working memory");
}

#[test]
fn test_harness_smoke_no_rules() {
    // Only deffacts, no rules — nothing should fire.
    let source = "(deffacts startup (data 42))";
    let result = run_ferric_semantic_regression(source);
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
    let result = run_ferric_semantic_regression(source);
    assert_eq!(result.rules_fired, 2, "expected both rules to fire");
    assert_eq!(result.output, "chain fired\n");
}

#[test]
fn test_harness_smoke_fixture_file() {
    // Load the trivial smoke.clp fixture and verify the output.
    let result = run_ferric_semantic_regression_file("smoke.clp");
    assert_eq!(result.rules_fired, 1, "expected 1 rule from smoke.clp");
    assert_eq!(result.output, "Got: hello\n");
}

#[test]
fn test_harness_regression_engine_fact_count() {
    let source = r"
(deffacts startup (a 1) (b 2) (c 3))
(defrule noop (a ?x) => )
";
    let regression = run_ferric_semantic_regression_full(source);
    assert_eq!(regression.rules_fired, 1);
    assert_eq!(regression.fact_count(), 3);
}

#[test]
fn test_harness_regression_engine_has_fact() {
    let source = r"
(deffacts startup (person Alice) (city London))
(defrule noop (person ?x) => )
";
    let regression = run_ferric_semantic_regression_full(source);
    assert!(regression.has_fact("person"), "expected 'person' fact");
    assert!(regression.has_fact("city"), "expected 'city' fact");
    assert!(
        !regression.has_fact("country"),
        "should not have 'country' fact"
    );
}

#[test]
fn test_harness_assertion_helpers() {
    let source = r"
(deffacts startup (item x) (item y))
(defrule count-items (item ?x) => (printout t ?x crlf))
";
    let result = run_ferric_semantic_regression(source);
    assert_rules_fired(&result, 2);
    assert_fact_count(&result, 2);
}

#[test]
fn test_harness_fixture_subdirectory() {
    // Verify that subdirectory paths work with run_ferric_semantic_regression_file.
    // Since core/ only has .gitkeep, exercise the path-join logic via
    // the existing smoke.clp at the top level.
    let result = run_ferric_semantic_regression_file("smoke.clp");
    assert_eq!(result.output, "Got: hello\n");
}

// ---------------------------------------------------------------------------
// Internal helper (not part of the public harness API)
// ---------------------------------------------------------------------------

/// Run and assert output, returning the full result for further inspection.
fn assert_ferric_semantic_regression_returns(source: &str, expected: &str) -> RegressionResult {
    let result = run_ferric_semantic_regression(source);
    assert_eq!(
        result.output, expected,
        "Ferric semantic regression output mismatch\n  expected: {expected:?}\n  actual:   {:?}",
        result.output,
    );
    result
}

/// Run a fixture file and assert exact rule/output expectations.
fn assert_fixture_output(
    fixture_name: &str,
    expected_rules_fired: usize,
    expected_output: &str,
) -> RegressionResult {
    let result = run_ferric_semantic_regression_file(fixture_name);
    assert_rules_fired(&result, expected_rules_fired);
    assert_output_exact(&result, expected_output);
    result
}

// ===========================================================================
// Module-domain semantic regression tests
// ===========================================================================

#[test]
fn test_semantic_regression_modules_basic_module() {
    let _ = assert_fixture_output("modules/basic_module.clp", 1, "Sensor temp = 72\n");
}

#[test]
fn test_semantic_regression_modules_global_scope() {
    // Both items fire; counter increments 0->1 then 1->2.
    let _ = assert_fixture_output("modules/global_scope.clp", 2, "count = 1\ncount = 2\n");
}

#[test]
fn test_semantic_regression_modules_qualified_names() {
    let _ = assert_fixture_output("modules/qualified_names.clp", 1, "sum: 7\nthreshold: 10\n");
}

// ===========================================================================
// Generic-domain semantic regression tests
// ===========================================================================

#[test]
fn test_semantic_regression_generics_basic_dispatch() {
    let result = run_ferric_semantic_regression_file("generics/basic_dispatch.clp");
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
fn test_semantic_regression_generics_specificity() {
    let _ = assert_fixture_output("generics/specificity.clp", 1, "integer\n");
}

// ===========================================================================
// Callable-return semantic regression tests
// ===========================================================================

#[test]
fn test_semantic_regression_return_unwinds_only_the_current_callable() {
    // Pinned against the repository's CLIPS 6.30 reference image. This covers
    // deffunction and defmethod callables, nested calls, structured control
    // forms, every source-constructible Ferric value kind, and side effects
    // after a taken return.
    let source = r#"
(deffunction return-early ()
    (printout t "function-before|" crlf)
    (return 1)
    (printout t "function-after|" crlf)
    2)

(deffunction return-if ()
    (if TRUE
        then (return if-value)
        else wrong)
    after-if)

(deffunction return-while ()
    (while TRUE do
        (return while-value)
        (printout t "while-after|" crlf))
    after-while)

(deffunction return-count ()
    (loop-for-count (?i 1 3) do
        (return ?i)
        (printout t "count-after|" crlf))
    after-count)

(deffunction return-inner ()
    (return 7)
    9)

(deffunction return-outer ()
    (printout t "inner=" (return-inner) crlf)
    8)

(deffunction return-void () (return) 99)
(deffunction return-int () (return 42) 99)
(deffunction return-float () (return 3.5) 99)
(deffunction return-symbol () (return alpha) omega)
(deffunction return-string () (return "hello") "after")
(deffunction return-multifield ()
    (return (create$ alpha 2 3.5))
    omega)

(defmethod return-method ((?x INTEGER))
    (return (+ ?x 1))
    999)

(defrule exercise-return
    =>
    (printout t "early=")
    (printout t (return-early) crlf)
    (printout t "if=")
    (printout t (return-if) crlf)
    (printout t "while=")
    (printout t (return-while) crlf)
    (printout t "count=")
    (printout t (return-count) crlf)
    (printout t "outer=")
    (printout t (return-outer) crlf)
    (printout t "void=" (return-void) "|" crlf)
    (printout t "int=" (return-int) crlf)
    (printout t "float=" (return-float) crlf)
    (printout t "symbol=" (return-symbol) crlf)
    (printout t "string=" (return-string) crlf)
    (printout t "multifield=" (return-multifield) crlf)
    (printout t "method=" (return-method 4) crlf))
"#;

    let expected = concat!(
        "early=function-before|\n",
        "1\n",
        "if=if-value\n",
        "while=while-value\n",
        "count=1\n",
        "outer=inner=7\n",
        "8\n",
        "void=|\n",
        "int=42\n",
        "float=3.5\n",
        "symbol=alpha\n",
        "string=hello\n",
        "multifield=(alpha 2 3.5)\n",
        "method=5\n",
    );
    let result = assert_ferric_semantic_regression_returns(source, expected);
    assert_eq!(result.rules_fired, 1);
}

#[test]
fn test_semantic_regression_action_loop_budget_preserves_boundary_and_stops_overrun() {
    let mut config = EngineConfig::utf8();
    config.max_action_loop_iterations = 3;

    let mut exact = Engine::new(config.clone());
    exact
        .load_str(
            r#"
(defrule exact-loop
    =>
    (loop-for-count (?i 1 3) do
        (printout t ?i "|"))
    (assert (completed)))
"#,
        )
        .expect("load exact loop");
    exact.reset().expect("reset exact loop");
    let exact_run = exact.run(RunLimit::Unlimited).expect("run exact loop");
    assert_eq!(exact_run.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(exact.get_output("t"), Some("1|2|3|"));
    assert_eq!(
        exact
            .find_facts("completed")
            .expect("completed facts")
            .len(),
        1
    );
    assert!(exact.action_diagnostics().is_empty());

    let mut over = Engine::new(config);
    over.load_str(
        r#"
(defrule over-budget-loop
    =>
    (loop-for-count (?i 1 4) do
        (printout t ?i "|"))
    (assert (completed)))
"#,
    )
    .expect("load over-budget loop");
    over.reset().expect("reset over-budget loop");
    let over_run = over.run(RunLimit::Unlimited).expect("run over-budget loop");
    assert_eq!(over_run.halt_reason, HaltReason::ActionError);
    assert_eq!(over.get_output("t"), Some("1|2|3|"));
    assert!(over
        .find_facts("completed")
        .expect("completed facts")
        .is_empty());
    assert!(matches!(
        over.action_diagnostics(),
        [ActionError::Evaluator(EvalError::ActionIterationLimit {
            function,
            limit: 3,
            ..
        })] if function == "loop-for-count"
    ));
}

#[test]
fn test_semantic_regression_return_stops_the_current_rule_rhs() {
    // CLIPS permits `return` in a rule RHS and uses it to stop the remaining
    // action sequence. Its value is discarded; it does not assert the sentinel
    // or emit an error.
    let source = r#"
(defrule return-from-rhs
    =>
    (printout t "rhs-before|" crlf)
    (return 42)
    (printout t "rhs-after|" crlf)
    (assert (rhs-after)))
"#;
    let result = run_ferric_semantic_regression_full(source);
    assert_eq!(result.output, "rhs-before|\n");
    assert_eq!(result.rules_fired, 1);
    assert!(!result.has_fact("rhs-after"));
}

#[test]
fn test_semantic_regression_return_argument_error_preserves_diagnostic_class() {
    let source = r#"
(deffunction return-error ()
    (return (/ 1 0))
    (printout t "function-after-error|" crlf))

(defrule exercise-return-error
    =>
    (printout t (return-error) crlf))
"#;
    let result = run_ferric_semantic_regression_full(source);
    assert_eq!(result.output, "");
    assert!(matches!(
        result.engine().action_diagnostics(),
        [ActionError::Evaluator(EvalError::DivisionByZero { .. })]
    ));
}

#[test]
fn test_semantic_regression_rhs_error_stops_run_and_retains_later_activation() {
    // Pinned CLIPS 6.30 behavior: an RHS evaluation error consumes the
    // failing activation, stops its remaining actions and the current run,
    // and leaves lower-priority activations available to a later run.
    let source = r#"
(deffacts startup (trigger))

(defrule failing
    (declare (salience 10))
    (trigger)
    =>
    (printout t "before-error|" crlf)
    (/ 1 0)
    (printout t "after-error|" crlf)
    (assert (rhs-after)))

(defrule later
    (declare (salience 0))
    (trigger)
    =>
    (printout t "later-activation|" crlf)
    (assert (later-ran)))
"#;
    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str(source).expect("load RHS error fixture");
    engine.reset().expect("reset RHS error fixture");

    let first = engine.run(RunLimit::Unlimited).expect("first run");
    assert_eq!(first.rules_fired, 1);
    assert_eq!(first.halt_reason, HaltReason::ActionError);
    assert_eq!(engine.get_output("t").unwrap_or(""), "before-error|\n");
    assert!(engine
        .find_facts("rhs-after")
        .expect("rhs-after facts")
        .is_empty());
    assert!(engine
        .find_facts("later-ran")
        .expect("later-ran facts")
        .is_empty());
    assert_eq!(engine.agenda_len(), 1);
    assert!(!engine.is_halted());
    assert!(matches!(
        engine.action_diagnostics(),
        [ActionError::Evaluator(EvalError::DivisionByZero { .. })]
    ));

    let second = engine.run(RunLimit::Unlimited).expect("second run");
    assert_eq!(second.rules_fired, 1);
    assert_eq!(second.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(
        engine.get_output("t").unwrap_or(""),
        "before-error|\nlater-activation|\n"
    );
    assert_eq!(
        engine
            .find_facts("later-ran")
            .expect("later-ran facts")
            .len(),
        1
    );
    assert!(engine.action_diagnostics().is_empty());
}

// ===========================================================================
// Stdlib-domain semantic regression tests
// ===========================================================================

#[test]
fn test_semantic_regression_stdlib_math_ops() {
    let _ = assert_fixture_output(
        "stdlib/math_ops.clp",
        1,
        // Note: Ferric returns 25.0 for (/ 100 4) — division always returns float.
        "add: 30\nsub: 42\nmul: 42\ndiv: 25.0\nmod: 2\nabs: 42\nmin: 1\nmax: 9\n",
    );
}

#[test]
fn test_semantic_regression_stdlib_string_ops() {
    let _ = assert_fixture_output(
        "stdlib/string_ops.clp",
        1,
        "cat: hello world\nlen: 5\nsub: hel\n",
    );
}

#[test]
fn test_semantic_regression_stdlib_multifield_ops() {
    let _ = assert_fixture_output(
        "stdlib/multifield_ops.clp",
        1,
        // length$ = 4, nth$ 2 = b, member$ b = 2 (1-based position)
        "len: 4\nnth: b\nmember: 2\n",
    );
}

#[test]
fn test_semantic_regression_stdlib_predicate_ops() {
    let _ = assert_fixture_output(
        "stdlib/predicate_ops.clp",
        1,
        "int? TRUE\nfloat? TRUE\nsym? TRUE\nstr? TRUE\nnum? TRUE\neq: TRUE\n",
    );
}

// ===========================================================================
// Core-domain semantic regression tests
// ===========================================================================

#[test]
fn test_semantic_regression_core_basic_match() {
    let _ = assert_fixture_output(
        "core/basic_match.clp",
        3,
        // Depth strategy: most recently asserted facts match first (reverse assertion order)
        "Color: green\nColor: blue\nColor: red\n",
    );
}

#[test]
fn test_semantic_regression_core_retract_cycle() {
    let result = assert_fixture_output("core/retract_cycle.clp", 1, "");
    assert_eq!(result.fact_count, 1); // processed fact remains
}

#[test]
fn test_semantic_regression_core_salience_order() {
    let _ = assert_fixture_output("core/salience_order.clp", 2, "high\nlow\n");
}

#[test]
fn test_semantic_regression_core_chain_rules() {
    let _ = assert_fixture_output("core/chain_rules.clp", 3, "1->2\n2->3\ndone\n");
}

#[test]
fn test_semantic_regression_core_modify_duplicate() {
    let _ = assert_fixture_output("core/modify_duplicate.clp", 1, "Alice is now 31\n");
}

#[test]
fn test_semantic_regression_core_fact_duplication_policy() {
    // Pinned against CLIPS 6.30: duplicate assertions are rejected by
    // default, allowed after enabling, and rejected immediately after
    // disabling again. The setter reports the prior policy.
    let _ = assert_fixture_output(
        "core/fact_duplication_policy.clp",
        4,
        "default=FALSE\nenable-old=FALSE\ndisable-old=TRUE\nitem=enabled\nitem=enabled\nitem=default\n",
    );
}

// ===========================================================================
// Negation-domain semantic regression tests
// ===========================================================================

#[test]
fn test_semantic_regression_negation_simple_not() {
    let _ = assert_fixture_output("negation/simple_not.clp", 1, "lamp is safe\n");
}

#[test]
fn test_semantic_regression_negation_not_retract() {
    let _ = assert_fixture_output(
        "negation/not_retract.clp",
        2,
        "danger removed\nlamp is safe\n",
    );
}

#[test]
fn fr_rete_002_ferric_regression_leading_not_transition_fixture() {
    // Pinned against the repository's CLIPS 6.30 reference image.
    let _ = assert_fixture_output(
        "negation/leading_not_transitions.clp",
        5,
        "empty\nassert-blocker\nblocked\nretract-blocker\nreactivated\n",
    );
}

#[test]
fn test_semantic_regression_negation_exists() {
    let _ = assert_fixture_output("negation/exists_ce.clp", 1, "signal detected\n");
}

#[test]
fn test_semantic_regression_negation_forall_basic() {
    let _ = assert_fixture_output("negation/forall_basic.clp", 1, "all items checked\n");
}

#[test]
fn test_semantic_regression_negation_forall_fail() {
    let _ = assert_fixture_output("negation/forall_fail.clp", 0, "");
}

// ===========================================================================
// Core domain — deeper coverage
// ===========================================================================

#[test]
fn fr_rete_003_ferric_regression_staged_late_rule_backfill_fixture() {
    // Keeping assertion and rule-definition stages separate is essential: a
    // single load would compile all Ferric rules before processing assertions.
    // Cross-engine evidence for this behavior belongs to the differential lane.
    let fixture_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("core");
    let prelude = std::fs::read_to_string(fixture_root.join("late_rule_backfill_prelude.clp"))
        .expect("read staged backfill prelude");
    let rules = std::fs::read_to_string(fixture_root.join("late_rule_backfill_rules.clp"))
        .expect("read staged backfill rules");

    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str(&prelude).expect("load pre-existing WMEs");
    engine.load_str(&rules).expect("load late rules");
    let rules_fired = run_regression_with_guard(&mut engine, "FR-RETE-003 staged fixture");

    assert_eq!(rules_fired, 3);
    assert_eq!(
        engine.get_output("t"),
        Some("single\njoin 1\nstate alice\n")
    );
}

#[test]
fn fr_rete_004_ferric_regression_match_time_transition_fixture() {
    // Pinned against the repository's CLIPS 6.30 reference image. The absent
    // "historical-fired" line is significant: changing a global does not
    // retroactively admit a partial match that failed its test CE.
    let _ = assert_fixture_output(
        "core/test_ce_match_time_transitions.clp",
        5,
        "open\nfresh\nretract\nreassert\npositive 2\n",
    );
}

#[test]
fn fr_rete_005_ferric_regression_double_not_existential_transition_fixture() {
    // Pinned against the repository's CLIPS 6.30 reference image. The single
    // "one" line is significant: a second support does not create a second
    // activation, and retracting only the first support keeps the match true.
    let _ = assert_fixture_output(
        "core/double_not_existential_transitions.clp",
        5,
        "zero\none\ntwo\npartial\nzero-again\n",
    );
}

#[test]
fn fr_rete_006_ferric_regression_multi_pattern_exists_transition_fixture() {
    // Pinned against the repository's CLIPS 6.30 reference image. Complete
    // joined tuples are Boolean support for the whole exists CE, so only one
    // "one" line appears and partial tuple retraction preserves the match.
    let _ = assert_fixture_output(
        "core/multi_pattern_exists_transitions.clp",
        5,
        "zero\none\ntwo\npartial\nzero-again\n",
    );
}

#[test]
fn fr_rete_006_ferric_regression_multi_pattern_exists_nested_join_fixture() {
    // Pinned against the repository's CLIPS 6.30 reference image. The test CE
    // participates in each three-pattern tuple before both complete tuples
    // collapse to one existential activation.
    let _ = assert_fixture_output("core/multi_pattern_exists_nested_join.clp", 1, "nested\n");
}

#[test]
fn fr_rete_008_depth_and_breadth_expose_recreated_activation_chronology() {
    // The paired traces are pinned against CLIPS 6.30 by the differential
    // lane. CLIPS yields N,P under depth and P,N under breadth. Ferric's
    // fact-timestamp key currently reverses both results; retaining both here
    // prevents either strategy branch from becoming vacuous coverage.
    let source = r#"
(deffacts startup
  (start))

(defrule establish-order
  (declare (salience 100))
  ?start <- (start)
  =>
  (retract ?start)
  (assert (positive-ready))
  (assert (blocker))
  (assert (release-blocker)))

(defrule release-negative
  (declare (salience 90))
  ?release <- (release-blocker)
  ?blocker <- (blocker)
  =>
  (retract ?release ?blocker))

(defrule positive-activation
  (positive-ready)
  =>
  (printout t "P" crlf)
  (assert (result P)))

(defrule recreated-negative-activation
  (not (blocker))
  =>
  (printout t "N" crlf)
  (assert (result N)))
"#;

    let depth = run_ferric_semantic_regression_engine_with_strategy(
        source,
        "FR-RETE-008 depth fixture",
        ConflictResolutionStrategy::Depth,
    );
    let breadth = run_ferric_semantic_regression_engine_with_strategy(
        source,
        "FR-RETE-008 breadth fixture",
        ConflictResolutionStrategy::Breadth,
    );

    assert_eq!(depth.rules_fired, 4);
    assert_eq!(depth.output, "P\nN\n");
    assert_eq!(breadth.rules_fired, 4);
    assert_eq!(breadth.output, "N\nP\n");
    assert_ne!(depth.output, breadth.output);
}

#[test]
fn fr_rete_009_lex_and_mea_expose_canonical_recency_vector_gap() {
    // CLIPS LEX sorts each recency vector descending and yields
    // LX,LY,MX,MY. CLIPS MEA compares the first CE before that canonical LEX
    // fallback and yields LY,LX,MX,MY. Ferric currently compares remaining
    // timetags in pattern order, so MY precedes MX under both strategies.
    let source = r#"
(deffacts startup
  (t1)
  (t2)
  (t3)
  (t4)
  (t5))

(defrule lex-X
  (declare (salience 20))
  (t1)
  (t4)
  =>
  (printout t "LX" crlf)
  (assert (result LX)))

(defrule lex-Y
  (declare (salience 20))
  (t3)
  (t2)
  =>
  (printout t "LY" crlf)
  (assert (result LY)))

(defrule mea-X
  (declare (salience 10))
  (t1)
  (t2)
  (t5)
  =>
  (printout t "MX" crlf)
  (assert (result MX)))

(defrule mea-Y
  (declare (salience 10))
  (t1)
  (t4)
  (t3)
  =>
  (printout t "MY" crlf)
  (assert (result MY)))
"#;

    let lex = run_ferric_semantic_regression_engine_with_strategy(
        source,
        "FR-RETE-009 LEX fixture",
        ConflictResolutionStrategy::Lex,
    );
    let mea = run_ferric_semantic_regression_engine_with_strategy(
        source,
        "FR-RETE-009 MEA fixture",
        ConflictResolutionStrategy::Mea,
    );

    assert_eq!(lex.rules_fired, 4);
    assert_eq!(lex.output, "LY\nLX\nMY\nMX\n");
    assert_eq!(mea.rules_fired, 4);
    assert_eq!(mea.output, "LY\nLX\nMY\nMX\n");
}

/// Multi-pattern join: a rule with two patterns joined by a shared variable.
#[test]
fn test_semantic_regression_core_multi_pattern_join() {
    let result = run_ferric_semantic_regression_file("core/multi_pattern_join.clp");
    assert_rules_fired(&result, 1);
    assert_output_exact(&result, "Alice is 30\n");
    assert_fact_count(&result, 2);
}

/// Refraction: a rule fires at most once per token even if re-run would match.
#[test]
fn test_semantic_regression_core_refraction() {
    let result = run_ferric_semantic_regression_file("core/refraction.clp");
    assert_rules_fired(&result, 1);
    assert_output_exact(&result, "processed a\n");
}

/// Depth conflict-resolution strategy fires the most recently asserted fact first.
#[test]
fn test_semantic_regression_core_multiple_activations_depth() {
    let result = run_ferric_semantic_regression_file("core/multiple_activations_depth.clp");
    assert_rules_fired(&result, 3);
    // deffacts asserts a, b, c in order; depth fires most recent first: c, b, a
    assert_output_exact(&result, "c\nb\na\n");
}

/// Chained retraction: higher-salience rule retracts a fact before the lower rule can fire.
#[test]
fn test_semantic_regression_core_retract_chain() {
    let result = run_ferric_semantic_regression_file("core/retract_chain.clp");
    // rule-a fires (salience 10), retracts (a 1); rule-b never fires
    assert_rules_fired(&result, 1);
    assert_output_exact(&result, "retracted a\n");
    // (b 1) remains in WM; (a 1) was retracted
    assert_fact_count(&result, 1);
}

/// Halt stops the run loop: step2 must not fire after step1 halts.
#[test]
fn test_semantic_regression_core_halt_stops_execution() {
    let result = run_ferric_semantic_regression_file("core/halt_stops_execution.clp");
    assert_rules_fired(&result, 1);
    assert_output_exact(&result, "step1\n");
}

/// Bind action in RHS: bind runs without error and subsequent actions still execute.
#[test]
fn test_semantic_regression_core_bind_in_rhs() {
    let result = run_ferric_semantic_regression_file("core/bind_in_rhs.clp");
    assert_rules_fired(&result, 1);
    assert_output_exact(&result, "computed\n");
}

// ===========================================================================
// Negation domain — deeper coverage
// ===========================================================================

/// Not with variable binding: only persons not listed in banned are allowed.
#[test]
fn test_semantic_regression_negation_not_multiple_patterns() {
    let result = run_ferric_semantic_regression_file("negation/not_multiple_patterns.clp");
    // Alice is not banned → fires; Bob is banned → does not fire
    assert_rules_fired(&result, 1);
    assert_output_exact(&result, "Alice allowed\n");
}

/// Exists fires exactly once regardless of how many facts satisfy the pattern.
#[test]
fn test_semantic_regression_negation_exists_count() {
    let result = run_ferric_semantic_regression_file("negation/exists_count.clp");
    assert_rules_fired(&result, 1);
    assert_output_exact(&result, "signal present\n");
}

/// Forall with empty quantified set is vacuously true and the rule fires.
#[test]
fn test_semantic_regression_negation_forall_vacuous_truth() {
    let result = run_ferric_semantic_regression_file("negation/forall_vacuous_truth.clp");
    assert_rules_fired(&result, 1);
    assert_output_exact(&result, "all done\n");
}

/// Negated conjunction (NCC): rule fires when it is NOT the case that both (a) and (b) exist.
#[test]
fn test_semantic_regression_negation_ncc_basic() {
    let result = run_ferric_semantic_regression_file("negation/ncc_basic.clp");
    assert_rules_fired(&result, 1);
    assert_output_exact(&result, "no a+b pair\n");
}

/// Forall retract invalidation: forall fires when satisfied, then retraction of a supporting
/// fact fires the remove-check rule but does NOT re-fire check-all (refraction prevents it).
#[test]
fn test_semantic_regression_negation_forall_retract_invalidation() {
    let result = run_ferric_semantic_regression_file("negation/forall_retract_invalidation.clp");
    assert_rules_fired(&result, 2);
    assert_output_exact(&result, "all checked\nremoved check\n");
}

// ===========================================================================
// Module domain — deeper coverage
// ===========================================================================

/// Focus stack drives execution order across modules.
/// MAIN fires first (default module), then A, then B after focus is pushed.
#[test]
fn test_semantic_regression_modules_multi_module_focus() {
    let result = run_ferric_semantic_regression_file("modules/multi_module_focus.clp");
    assert_rules_fired(&result, 3);
    assert_output_exact(&result, "MAIN\nA\nB\n");
}

/// Global variable incremented from RHS across multiple rule firings.
#[test]
fn test_semantic_regression_modules_global_bind() {
    let result = run_ferric_semantic_regression_file("modules/global_bind.clp");
    assert_rules_fired(&result, 3);
    // Each of the 3 item facts fires the rule; counter increments 0->1->2->3.
    assert_output_exact(&result, "count now 1\ncount now 2\ncount now 3\n");
}

/// User-defined function (deffunction) called from rule RHS.
#[test]
fn test_semantic_regression_modules_deffunction_call() {
    let result = run_ferric_semantic_regression_file("modules/deffunction_call.clp");
    assert_rules_fired(&result, 1);
    assert_output_exact(&result, "double: 42\n");
}

/// Deffunction using str-cat for string construction, called from rule RHS.
#[test]
fn test_semantic_regression_modules_deffunction_str() {
    let result = run_ferric_semantic_regression_file("modules/deffunction_str.clp");
    assert_rules_fired(&result, 1);
    assert_output_exact(&result, "Hello Alice!\n");
}

// ===========================================================================
// Generic domain — deeper coverage
// ===========================================================================

/// Multiple methods on a generic dispatch by type: INTEGER and SYMBOL.
#[test]
fn test_semantic_regression_generics_multi_method() {
    let result = run_ferric_semantic_regression_file("generics/multi_method.clp");
    assert_rules_fired(&result, 1);
    assert_output_exact(&result, "int:42\nsym:hello\n");
}

/// Generic called from within a deffunction; deffunction called from rule RHS.
#[test]
fn test_semantic_regression_generics_method_with_deffunction() {
    let result = run_ferric_semantic_regression_file("generics/method_with_deffunction.clp");
    assert_rules_fired(&result, 1);
    assert_output_exact(&result, "integer value\nfloat value\nsymbol value\n");
}

// ===========================================================================
// Stdlib domain — deeper coverage
// ===========================================================================

/// Advanced math: integer division, type conversion, abs on negative.
#[test]
fn test_semantic_regression_stdlib_math_advanced() {
    let result = run_ferric_semantic_regression_file("stdlib/math_advanced.clp");
    assert_rules_fired(&result, 1);
    // div returns integer; float conversion of 42 gives 42.0; integer(3) stays 3; abs(-99)=99
    assert_output_exact(
        &result,
        "int-div: 3\nfloat: 42.0\ninteger: 3\nabs-neg: 99\n",
    );
}

/// String functions: sym-cat, str-length, sub-string.
#[test]
fn test_semantic_regression_stdlib_string_advanced() {
    let result = run_ferric_semantic_regression_file("stdlib/string_advanced.clp");
    assert_rules_fired(&result, 1);
    assert_output_exact(&result, "sym-cat: abcdef\nstr-len: 11\nsub-str: hello\n");
}

/// Comparison operators: >, <, >=, <=, <>, eq.
/// Note: numeric `=` cannot be used as a function call expression (lexer limitation).
#[test]
fn test_semantic_regression_stdlib_comparison_ops() {
    let result = run_ferric_semantic_regression_file("stdlib/comparison_ops.clp");
    assert_rules_fired(&result, 1);
    assert_output_exact(
        &result,
        "gt: TRUE\nlt: TRUE\ngte: TRUE\nlte: TRUE\nneq-num: TRUE\neq-sym: TRUE\n",
    );
}

/// Logical operations: and, or, not.
#[test]
fn test_semantic_regression_stdlib_logical_ops() {
    let result = run_ferric_semantic_regression_file("stdlib/logical_ops.clp");
    assert_rules_fired(&result, 1);
    assert_output_exact(
        &result,
        "and: TRUE\nor: TRUE\nnot: TRUE\nand-false: FALSE\n",
    );
}

/// Type predicate functions: evenp, oddp, lexemep.
#[test]
fn test_semantic_regression_stdlib_type_predicates() {
    let result = run_ferric_semantic_regression_file("stdlib/type_predicates.clp");
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
fn test_semantic_regression_modules_qualified_global_bind() {
    let result = run_ferric_semantic_regression_file("modules/qualified_global_bind.clp");
    assert_rules_fired(&result, 1);
    // base-value starts at 10, bind sets it to 10*3=30.
    assert_output_exact(&result, "value: 30\n");
}

/// Cross-module function import: UTILS exports deffunction; MAIN imports and calls it.
#[test]
fn test_semantic_regression_modules_cross_module_import() {
    let result = run_ferric_semantic_regression_file("modules/cross_module_import.clp");
    assert_rules_fired(&result, 1);
    assert_output_exact(&result, "square-5: 25\nsquare-9: 81\n");
}

/// Visibility boundary: loading a function from a non-exporting module via
/// qualified call should produce an action diagnostic (not silently succeed).
#[test]
fn test_semantic_regression_modules_visibility_boundary_not_exported() {
    let source = r"
(defmodule MATH (export ?NONE))
(deffunction add (?x ?y) (+ ?x ?y))

(defmodule MAIN)
(defrule call-hidden (go) => (printout t (MATH::add 1 2) crlf))
(deffacts startup (go))
";
    let regression = run_ferric_semantic_regression_full(source);
    // The rule fires (the call executes), but a visibility error must be recorded.
    let diagnostics = regression.engine().action_diagnostics();
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
fn test_semantic_regression_modules_unsupported_form_diagnostic() {
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
fn test_semantic_regression_generics_dispatch_ordering() {
    let result = run_ferric_semantic_regression_file("generics/dispatch_ordering.clp");
    assert_rules_fired(&result, 1);
    assert_output_exact(&result, "int: integer\nfloat: float\nsym: any\n");
}

/// call-next-method: INTEGER method calls next-less-specific NUMBER method,
/// composing the results.
#[test]
fn test_semantic_regression_generics_call_next_method() {
    let result = run_ferric_semantic_regression_file("generics/call_next_method.clp");
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
fn test_semantic_regression_stdlib_multifield_advanced() {
    let result = run_ferric_semantic_regression_file("stdlib/multifield_advanced.clp");
    assert_rules_fired(&result, 1);
    // create$(a b c d e): length=5, 2nd=b, member$(c)=3 (1-based), member$(z)=FALSE
    assert_output_exact(&result, "len: 5\n2nd: b\npos-c: 3\npos-z: FALSE\n");
}

/// format function: printf-style formatting returns a string, printed via printout.
/// Note: format does not write to the router; the result must be passed to printout.
#[test]
fn test_semantic_regression_stdlib_format_output() {
    let result = run_ferric_semantic_regression_file("stdlib/format_output.clp");
    assert_rules_fired(&result, 1);
    assert_output_exact(&result, "num=42\nstr=hello\nflt=3.5\n");
}

/// Math edge cases: multi-arg min/max, abs, integer division, modulus.
#[test]
fn test_semantic_regression_stdlib_math_edge_cases() {
    let result = run_ferric_semantic_regression_file("stdlib/math_edge_cases.clp");
    assert_rules_fired(&result, 1);
    assert_output_exact(
        &result,
        "min3: 1\nmax3: 3\nneg-abs: 7\ndiv-trunc: 3\nmod-neg: 1\n",
    );
}

// ===========================================================================
// Mobile App Engagement example (README demo)
//
// These tests verify the rules in
//   tests/fixtures/examples/mobile_engagement.clp
// by composing the shared ruleset with scenario-specific deffacts.
// Each scenario asserts exactly the user-state facts needed and checks
// that the engine picks the single correct action.
// ===========================================================================

/// Load the shared engagement rules from the fixture file.
fn engagement_rules() -> String {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/examples/mobile_engagement.clp");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("could not read engagement rules: {e}"))
}

/// Run the engagement rules with scenario-specific deffacts.
fn run_engagement(deffacts: &str) -> RegressionResult {
    let rules = engagement_rules();
    let source = format!("{deffacts}\n{rules}");
    run_ferric_semantic_regression(&source)
}

/// Brand-new free user (2 sessions) → signup incentive.
#[test]
fn test_engagement_new_user_gets_signup_incentive() {
    let result = run_engagement(
        r"(deffacts scenario
            (user-tier free)
            (session-count 2)
            (has-crashed no)
            (social-shares 0))",
    );
    assert_rules_fired(&result, 1);
    assert_output_exact(&result, "ACTION: signup-incentive\n");
}

/// Engaged free user who has not yet rated → rating prompt.
#[test]
fn test_engagement_engaged_user_gets_rating_prompt() {
    let result = run_engagement(
        r"(deffacts scenario
            (user-tier free)
            (session-count 12)
            (days-since-install 14)
            (has-rated no)
            (has-crashed no)
            (feature-usage high)
            (social-shares 1))",
    );
    assert_rules_fired(&result, 1);
    assert_output_exact(&result, "ACTION: rate-app\n");
}

/// Engaged free user who already rated and uses features heavily → upsell to paid.
#[test]
fn test_engagement_rated_power_user_gets_upsell_paid() {
    let result = run_engagement(
        r"(deffacts scenario
            (user-tier free)
            (session-count 12)
            (days-since-install 30)
            (has-rated yes)
            (has-crashed no)
            (feature-usage high)
            (social-shares 2))",
    );
    assert_rules_fired(&result, 1);
    assert_output_exact(&result, "ACTION: upsell-paid\n");
}

/// Paid subscriber with heavy use → upsell to premium.
#[test]
fn test_engagement_paid_power_user_gets_upsell_premium() {
    let result = run_engagement(
        r"(deffacts scenario
            (user-tier paid)
            (session-count 25)
            (has-rated yes)
            (has-crashed no)
            (feature-usage high)
            (social-shares 5))",
    );
    assert_rules_fired(&result, 1);
    assert_output_exact(&result, "ACTION: upsell-premium\n");
}

/// Recent crash → all prompts suppressed, nothing shown.
#[test]
fn test_engagement_crash_suppresses_all_prompts() {
    let result = run_engagement(
        r"(deffacts scenario
            (user-tier free)
            (session-count 12)
            (days-since-install 14)
            (has-rated no)
            (has-crashed yes)
            (feature-usage high)
            (social-shares 0))",
    );
    // suppress-after-crash fires (asserts prompt-shown + prompt-suppressed)
    // but prints nothing.
    assert_rules_fired(&result, 1);
    assert_output_exact(&result, "");
}

/// Lapsed user returning after 10 days → retention discount.
#[test]
fn test_engagement_lapsed_user_gets_retention_discount() {
    let result = run_engagement(
        r"(deffacts scenario
            (user-tier paid)
            (session-count 50)
            (days-since-last-open 10)
            (has-rated yes)
            (has-crashed no)
            (feature-usage medium)
            (social-shares 3))",
    );
    assert_rules_fired(&result, 1);
    assert_output_exact(&result, "ACTION: retention-discount\n");
}

/// Free user who hits a premium feature → paywall (highest action priority).
#[test]
fn test_engagement_premium_access_shows_paywall() {
    let result = run_engagement(
        r"(deffacts scenario
            (user-tier free)
            (session-count 12)
            (days-since-install 14)
            (has-rated no)
            (has-crashed no)
            (accessed-premium-feature)
            (feature-usage high)
            (social-shares 0))",
    );
    assert_rules_fired(&result, 1);
    assert_output_exact(&result, "ACTION: paywall\n");
}

/// Low-engagement user with no social shares → share credit offer.
#[test]
fn test_engagement_quiet_user_gets_share_credit() {
    let result = run_engagement(
        r"(deffacts scenario
            (user-tier free)
            (session-count 8)
            (days-since-install 30)
            (has-rated yes)
            (has-crashed no)
            (feature-usage low)
            (social-shares 0))",
    );
    assert_rules_fired(&result, 1);
    assert_output_exact(&result, "ACTION: share-credit\n");
}
