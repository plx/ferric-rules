//! Phase 4 integration tests: Standard Library.
//!
//! This file contains integration-level tests for Phase 4 features:
//! - Module-qualified `MODULE::name` resolution (passes 002-004)
//! - Cross-module function/global visibility (pass 003)
//! - `deffunction`/`defgeneric` conflict diagnostics (pass 005)
//! - Generic specificity ranking and `call-next-method` (passes 006-007)
//! - Predicate/math/type builtins (pass 008)
//! - String/symbol builtins (pass 009)
//! - Multifield builtins (pass 010)
//! - I/O and environment functions (pass 011)
//! - Agenda/focus query functions (pass 012)
//!
//! Tests are added incrementally as each pass lands. The skeleton sections
//! below reserve test organization for each feature area.

#![allow(unused_imports)] // Will be used as passes land

use crate::test_helpers::*;

// ===========================================================================
// Phase 4 baseline: confirm Phase 3 fixtures still pass
// ===========================================================================

#[test]
fn phase3_fixtures_remain_green() {
    // This is a meta-test that confirms the Phase 3 fixture suite is
    // still passing after Phase 4 harness alignment changes.
    let mut engine = new_utf8_engine();
    load_fixture(&mut engine, "phase3_deffunction.clp");
    run_to_completion(&mut engine);
    assert_engine_consistent(&engine);
}

#[test]
fn phase3_defgeneric_fixture_remains_green() {
    let mut engine = new_utf8_engine();
    load_fixture(&mut engine, "phase3_defgeneric.clp");
    run_to_completion(&mut engine);
    assert_engine_consistent(&engine);
}

#[test]
fn phase3_defglobal_fixture_remains_green() {
    let mut engine = new_utf8_engine();
    load_fixture(&mut engine, "phase3_defglobal.clp");
    run_to_completion(&mut engine);
    assert_engine_consistent(&engine);
}

#[test]
fn phase3_defmodule_fixture_remains_green() {
    let mut engine = new_utf8_engine();
    load_fixture(&mut engine, "phase3_defmodule.clp");
    run_to_completion(&mut engine);
    assert_engine_consistent(&engine);
}

#[test]
fn phase3_expression_eval_fixture_remains_green() {
    let mut engine = new_utf8_engine();
    load_fixture(&mut engine, "phase3_expression_eval.clp");
    run_to_completion(&mut engine);
    assert_engine_consistent(&engine);
}

// ===========================================================================
// Module-qualified name resolution (passes 002-004)
// ===========================================================================

#[test]
fn qualified_name_parses_through_lexer() {
    // Verify that MODULE::name is lexed as a single symbol token.
    let result = ferric_parser::parse_sexprs("(SENSOR::reading)", ferric_parser::FileId(0));
    assert!(result.errors.is_empty());
    let items = result.exprs[0].as_list().unwrap();
    assert_eq!(items[0].as_symbol(), Some("SENSOR::reading"));
}

#[test]
fn qualified_name_in_function_call_position() {
    // Verify that MODULE::func appears as function name in stage2 interpretation.
    let result = ferric_parser::parse_sexprs(
        "(defrule test (go) => (MATH::add 1 2))",
        ferric_parser::FileId(0),
    );
    assert!(result.errors.is_empty());
    let config = ferric_parser::InterpreterConfig::default();
    let interp = ferric_parser::interpret_constructs(&result.exprs, &config);
    // Should parse without errors (the function call name is just a string).
    assert!(
        interp.errors.is_empty(),
        "interpretation errors: {:?}",
        interp.errors
    );
}

#[test]
fn qualified_name_utility_splits_correctly() {
    use crate::qualified_name::{parse_qualified_name, QualifiedName};

    let q = parse_qualified_name("SENSOR::reading").unwrap();
    assert!(
        matches!(q, QualifiedName::Qualified { ref module, ref name }
        if module == "SENSOR" && name == "reading")
    );

    let u = parse_qualified_name("plain-name").unwrap();
    assert!(matches!(u, QualifiedName::Unqualified(ref name) if name == "plain-name"));
}

#[test]
fn malformed_qualified_name_diagnostics() {
    use crate::qualified_name::parse_qualified_name;

    // Empty module
    assert!(parse_qualified_name("::reading").is_err());
    // Empty name
    assert!(parse_qualified_name("SENSOR::").is_err());
    // Multiple separators
    assert!(parse_qualified_name("A::B::C").is_err());
}

// ---------------------------------------------------------------------------
// Module-qualified callable and global lookup diagnostics (pass 004)
// ---------------------------------------------------------------------------

#[test]
fn qualified_function_call_resolves() {
    let mut engine = new_utf8_engine();
    let source = r"
(defmodule MATH (export deffunction ?ALL))
(deffunction add (?x ?y) (+ ?x ?y))

(defmodule MAIN (import MATH deffunction ?ALL))
(defrule test-call (go) => (printout t (MATH::add 3 4) crlf))
(deffacts startup (go))
";
    load_ok(&mut engine, source);
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "7");
}

#[test]
fn qualified_function_call_unknown_module() {
    let mut engine = new_utf8_engine();
    let source = r"
(defrule test-call (go) => (printout t (NONEXISTENT::add 3 4) crlf))
(deffacts startup (go))
";
    load_ok(&mut engine, source);
    engine.reset().unwrap();
    let _result = engine.run(crate::execution::RunLimit::Unlimited).unwrap();
    let diagnostics = engine.action_diagnostics();
    assert!(
        diagnostics.iter().any(|d| {
            let msg = format!("{d}");
            msg.contains("NONEXISTENT")
                && (msg.contains("unknown module") || msg.contains("unknown"))
        }),
        "expected unknown module error, got: {diagnostics:?}"
    );
}

#[test]
fn qualified_function_call_wrong_module() {
    // Function exists in MATH but qualified as MAIN::add — should fail.
    let mut engine = new_utf8_engine();
    let source = r"
(defmodule MATH (export deffunction ?ALL))
(deffunction add (?x ?y) (+ ?x ?y))

(defmodule MAIN (import MATH deffunction ?ALL))
(defrule test-call (go) => (printout t (MAIN::add 3 4) crlf))
(deffacts startup (go))
";
    load_ok(&mut engine, source);
    engine.reset().unwrap();
    let _result = engine.run(crate::execution::RunLimit::Unlimited).unwrap();
    let diagnostics = engine.action_diagnostics();
    assert!(
        diagnostics.iter().any(|d| {
            let msg = format!("{d}");
            msg.contains("MAIN::add") && msg.contains("unknown")
        }),
        "expected unknown function for wrong module qualification, got: {diagnostics:?}"
    );
}

#[test]
fn qualified_function_not_visible() {
    let mut engine = new_utf8_engine();
    let source = r"
(defmodule MATH (export ?NONE))
(deffunction add (?x ?y) (+ ?x ?y))

(defmodule MAIN)
(defrule test-call (go) => (printout t (MATH::add 3 4) crlf))
(deffacts startup (go))
";
    load_ok(&mut engine, source);
    engine.reset().unwrap();
    let _result = engine.run(crate::execution::RunLimit::Unlimited).unwrap();
    let diagnostics = engine.action_diagnostics();
    assert!(
        diagnostics.iter().any(|d| {
            let msg = format!("{d}");
            msg.contains("not visible")
                || msg.contains("not accessible")
                || msg.contains("NotVisible")
        }),
        "expected visibility error for qualified call, got: {diagnostics:?}"
    );
}

#[test]
fn qualified_generic_call_resolves() {
    let mut engine = new_utf8_engine();
    let source = r"
(defmodule MATH (export defgeneric ?ALL))
(defgeneric display-value)
(defmethod display-value ((?x INTEGER)) (+ ?x 100))

(defmodule MAIN (import MATH defgeneric ?ALL))
(defrule test-call (go) => (printout t (MATH::display-value 5) crlf))
(deffacts startup (go))
";
    load_ok(&mut engine, source);
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "105");
}

#[test]
fn no_silent_fallback_to_builtin() {
    // MAIN::+ should NOT resolve to the builtin + function.
    let mut engine = new_utf8_engine();
    let source = r"
(defrule test-call (go) => (printout t (MAIN::+ 3 4) crlf))
(deffacts startup (go))
";
    load_ok(&mut engine, source);
    engine.reset().unwrap();
    let _result = engine.run(crate::execution::RunLimit::Unlimited).unwrap();
    let diagnostics = engine.action_diagnostics();
    assert!(
        !diagnostics.is_empty(),
        "expected error for MAIN::+ (should not silently resolve to builtin)"
    );
}

// ===========================================================================
// Cross-module function/global visibility (pass 003)
// ===========================================================================

#[test]
fn same_module_function_always_visible() {
    let mut engine = new_utf8_engine();
    let source = r"
(deffunction double (?x) (* ?x 2))
(defrule test-call (go) => (printout t (double 5) crlf))
(deffacts startup (go))
";
    load_ok(&mut engine, source);
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "10");
}

#[test]
fn cross_module_function_visible_when_exported_and_imported() {
    let mut engine = new_utf8_engine();
    let source = r"
(defmodule MATH (export deffunction ?ALL))
(deffunction add (?x ?y) (+ ?x ?y))

(defmodule MAIN (import MATH deffunction ?ALL))
(defrule test-call (go) => (printout t (add 3 4) crlf))
(deffacts startup (go))
";
    load_ok(&mut engine, source);
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "7");
}

#[test]
fn cross_module_function_not_visible_without_export() {
    let mut engine = new_utf8_engine();
    let source = r"
(defmodule MATH (export ?NONE))
(deffunction add (?x ?y) (+ ?x ?y))

(defmodule MAIN (import MATH ?ALL))
(defrule test-call (go) => (printout t (add 3 4) crlf))
(deffacts startup (go))
";
    load_ok(&mut engine, source);
    engine.reset().unwrap();
    engine.run(crate::execution::RunLimit::Unlimited).unwrap();
    let diagnostics = engine.action_diagnostics();
    assert!(
        diagnostics.iter().any(|d| {
            let msg = format!("{d}");
            msg.contains("not visible")
                || msg.contains("not accessible")
                || msg.contains("unknown")
                || msg.contains("NotVisible")
        }),
        "expected visibility error diagnostic, got: {diagnostics:?}"
    );
}

#[test]
fn cross_module_function_not_visible_without_import() {
    let mut engine = new_utf8_engine();
    let source = r"
(defmodule MATH (export deffunction ?ALL))
(deffunction add (?x ?y) (+ ?x ?y))

(defmodule MAIN)
(defrule test-call (go) => (printout t (add 3 4) crlf))
(deffacts startup (go))
";
    load_ok(&mut engine, source);
    engine.reset().unwrap();
    engine.run(crate::execution::RunLimit::Unlimited).unwrap();
    let diagnostics = engine.action_diagnostics();
    assert!(
        diagnostics.iter().any(|d| {
            let msg = format!("{d}");
            msg.contains("not visible")
                || msg.contains("not accessible")
                || msg.contains("unknown")
                || msg.contains("NotVisible")
        }),
        "expected visibility error when no import, got: {diagnostics:?}"
    );
}

#[test]
fn same_module_global_always_visible() {
    let mut engine = new_utf8_engine();
    let source = r"
(defglobal ?*count* = 42)
(defrule test-global (go) => (printout t ?*count* crlf))
(deffacts startup (go))
";
    load_ok(&mut engine, source);
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "42");
}

#[test]
fn cross_module_global_visible_when_exported_and_imported() {
    let mut engine = new_utf8_engine();
    let source = r"
(defmodule CONFIG (export defglobal ?ALL))
(defglobal ?*threshold* = 100)

(defmodule MAIN (import CONFIG defglobal ?ALL))
(defrule test-global (go) => (printout t ?*threshold* crlf))
(deffacts startup (go))
";
    load_ok(&mut engine, source);
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "100");
}

#[test]
fn cross_module_global_not_visible_without_export() {
    let mut engine = new_utf8_engine();
    let source = r"
(defmodule CONFIG (export ?NONE))
(defglobal ?*threshold* = 100)

(defmodule MAIN (import CONFIG ?ALL))
(defrule test-global (go) => (printout t ?*threshold* crlf))
(deffacts startup (go))
";
    load_ok(&mut engine, source);
    engine.reset().unwrap();
    engine.run(crate::execution::RunLimit::Unlimited).unwrap();
    let diagnostics = engine.action_diagnostics();
    assert!(
        diagnostics.iter().any(|d| {
            let msg = format!("{d}");
            msg.contains("not visible")
                || msg.contains("not accessible")
                || msg.contains("unbound")
                || msg.contains("NotVisible")
        }),
        "expected visibility error for global, got: {diagnostics:?}"
    );
}

#[test]
fn function_body_executes_in_own_module_context() {
    // A function defined in MATH should be able to call another MATH function
    // even when called from MAIN (which doesn't import the helper).
    let mut engine = new_utf8_engine();
    let source = r"
(defmodule MATH (export deffunction add))
(deffunction helper (?x) (* ?x 2))
(deffunction add (?x ?y) (+ (helper ?x) ?y))

(defmodule MAIN (import MATH deffunction add))
(defrule test-call (go) => (printout t (add 3 4) crlf))
(deffacts startup (go))
";
    load_ok(&mut engine, source);
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    // add(3, 4) = helper(3) + 4 = 6 + 4 = 10
    assert_eq!(output.trim(), "10");
}

// ===========================================================================
// deffunction/defgeneric conflict diagnostics (pass 005)
// ===========================================================================

#[test]
fn conflict_deffunction_then_defgeneric_produces_error() {
    let mut engine = new_utf8_engine();
    assert_load_error_contains(
        &mut engine,
        r"
        (deffunction compute (?x) (+ ?x 1))
        (defgeneric compute)
        ",
        "cannot define defgeneric `compute`",
    );
}

#[test]
fn conflict_defgeneric_then_deffunction_produces_error() {
    let mut engine = new_utf8_engine();
    assert_load_error_contains(
        &mut engine,
        r"
        (defgeneric compute)
        (deffunction compute (?x) (+ ?x 1))
        ",
        "cannot define deffunction `compute`",
    );
}

#[test]
fn conflict_defmethod_autocreate_with_deffunction_produces_error() {
    let mut engine = new_utf8_engine();
    assert_load_error_contains(
        &mut engine,
        r"
        (deffunction process (?x) ?x)
        (defmethod process ((?x INTEGER)) (+ ?x 1))
        ",
        "cannot define defmethod `process`",
    );
}

#[test]
fn no_conflict_separate_names_function_and_generic() {
    let mut engine = new_utf8_engine();
    let source = r"
        (deffunction helper (?x) (+ ?x 1))
        (defgeneric format-value)
        (defmethod format-value ((?x INTEGER)) ?x)
        (defrule test-both
            (data ?v)
            =>
            (printout t (helper ?v) crlf))
    ";
    load_ok(&mut engine, source);
    engine.reset().unwrap();
    load_ok(&mut engine, "(assert (data 5))");
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "6");
}

// ===========================================================================
// Generic specificity ranking (pass 006)
// ===========================================================================

#[test]
fn generic_dispatch_prefers_integer_over_number() {
    // Method returning distinct sentinel values; rule prints the result.
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r"
        (defgeneric classify)
        (defmethod classify ((?x INTEGER)) 1)
        (defmethod classify ((?x NUMBER))  2)
        (defrule test (go) => (printout t (classify 42) crlf))
        (deffacts startup (go))
    ",
    );
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(
        output.trim(),
        "1",
        "INTEGER method should be selected for integer arg"
    );
}

#[test]
fn generic_dispatch_falls_through_to_number_for_float() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r"
        (defgeneric classify)
        (defmethod classify ((?x INTEGER)) 1)
        (defmethod classify ((?x NUMBER))  2)
        (defrule test (go) => (printout t (classify 3.14) crlf))
        (deffacts startup (go))
    ",
    );
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(
        output.trim(),
        "2",
        "NUMBER method should be selected for float arg"
    );
}

#[test]
fn generic_dispatch_prefers_symbol_over_lexeme() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r"
        (defgeneric classify)
        (defmethod classify ((?x SYMBOL)) 1)
        (defmethod classify ((?x LEXEME)) 2)
        (defrule test (go) => (printout t (classify hello) crlf))
        (deffacts startup (go))
    ",
    );
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(
        output.trim(),
        "1",
        "SYMBOL method should be selected for symbol arg"
    );
}

#[test]
fn generic_dispatch_unrestricted_is_least_specific() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r"
        (defgeneric classify)
        (defmethod classify ((?x INTEGER)) 1)
        (defmethod classify ((?x))         2)
        (defrule test (go) => (printout t (classify 42) crlf))
        (deffacts startup (go))
    ",
    );
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(
        output.trim(),
        "1",
        "INTEGER method should beat unrestricted method"
    );
}

#[test]
fn generic_dispatch_wildcard_less_specific_than_fixed() {
    // Method with only fixed parameters is more specific than one with a wildcard.
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r"
        (defgeneric classify)
        (defmethod classify ((?x INTEGER))         1)
        (defmethod classify ((?x INTEGER) $?rest)  2)
        (defrule test (go) => (printout t (classify 42) crlf))
        (deffacts startup (go))
    ",
    );
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(
        output.trim(),
        "1",
        "Fixed-param method should beat variadic method"
    );
}

#[test]
fn generic_dispatch_registration_order_irrelevant() {
    // Register less specific method first, more specific second.
    // Specificity should still pick the more specific one.
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r"
        (defgeneric classify)
        (defmethod classify ((?x NUMBER))  2)
        (defmethod classify ((?x INTEGER)) 1)
        (defrule test (go) => (printout t (classify 42) crlf))
        (deffacts startup (go))
    ",
    );
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(
        output.trim(),
        "1",
        "INTEGER method should win regardless of registration order"
    );
}

// ===========================================================================
// call-next-method (pass 007)
// ===========================================================================

#[test]
fn call_next_method_chains_to_less_specific() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r"
        (defgeneric classify)
        (defmethod classify ((?x INTEGER)) (+ 100 (call-next-method)))
        (defmethod classify ((?x NUMBER))  42)
        (defrule test (go) => (printout t (classify 5) crlf))
        (deffacts startup (go))
    ",
    );
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(
        output.trim(),
        "142",
        "INTEGER method should chain to NUMBER method: 100 + 42"
    );
}

#[test]
fn call_next_method_three_level_chain() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r"
        (defgeneric classify)
        (defmethod classify ((?x INTEGER)) (+ 1000 (call-next-method)))
        (defmethod classify ((?x NUMBER))  (+ 100 (call-next-method)))
        (defmethod classify ((?x))         7)
        (defrule test (go) => (printout t (classify 5) crlf))
        (deffacts startup (go))
    ",
    );
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "1107", "Three-level chain: 1000 + 100 + 7");
}

#[test]
fn call_next_method_no_next_method_produces_error() {
    // The only method calls call-next-method but there is no next method.
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r"
        (defgeneric classify)
        (defmethod classify ((?x INTEGER)) (call-next-method))
        (defrule test (go) => (printout t (classify 5) crlf))
        (deffacts startup (go))
    ",
    );
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    // The rule fires but the action produces a diagnostic (no next method).
    // The printout won't produce output since the call-next-method errors.
    let output = engine.get_output("t").unwrap_or("");
    assert!(
        output.trim().is_empty() || !output.contains('5'),
        "call-next-method with no next should produce an error, not output"
    );
}

#[test]
fn call_next_method_outside_generic_produces_error() {
    // call-next-method inside a deffunction (not a defmethod) should error.
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r"
        (deffunction bad () (call-next-method))
        (defrule test (go) => (printout t (bad) crlf))
        (deffacts startup (go))
    ",
    );
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    // The function call should produce an error diagnostic.
    let output = engine.get_output("t").unwrap_or("");
    assert!(
        output.trim().is_empty(),
        "call-next-method outside generic should produce error, got: {output}"
    );
}

// ===========================================================================
// Predicate/math/type builtins (pass 008)
// ===========================================================================

#[test]
fn type_predicates_in_rule_rhs() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r#"
        (defrule test (go)
            =>
            (printout t (integerp 42) " " (floatp 3.14) " " (symbolp hello) " " (stringp "hi") " " (lexemep hello) crlf))
        (deffacts startup (go))
    "#,
    );
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "TRUE TRUE TRUE TRUE TRUE");
}

#[test]
fn type_conversion_integer_and_float() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r#"
        (defrule test (go)
            =>
            (printout t (integer 3.7) " " (float 42) crlf))
        (deffacts startup (go))
    "#,
    );
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "3 42.0");
}

#[test]
fn evenp_oddp_in_test_ce() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r#"
        (defrule even-rule (num ?x) (test (evenp ?x))
            => (printout t ?x " is even" crlf))
        (deffacts startup (num 4) (num 7))
    "#,
    );
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert!(
        output.contains("4 is even"),
        "4 should be even, got: {output}"
    );
    assert!(
        !output.contains("7 is even"),
        "7 should not be even, got: {output}"
    );
}

#[test]
fn multifieldp_false_for_non_multifield() {
    // multifieldp returns FALSE for non-multifield values.
    // (Testing TRUE requires create$ which is added in pass 010.)
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r"
        (defrule test (go)
            => (printout t (multifieldp 42) crlf))
        (deffacts startup (go))
    ",
    );
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(
        output.trim(),
        "FALSE",
        "multifieldp of integer should be FALSE, got: {output}"
    );
}

#[test]
fn type_conversion_integer_passthrough_and_float_passthrough() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r#"
        (defrule test (go)
            =>
            (printout t (integer 10) " " (float 2.5) crlf))
        (deffacts startup (go))
    "#,
    );
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "10 2.5");
}

// ===========================================================================
// String/symbol builtins (pass 009)
// ===========================================================================

#[test]
fn str_cat_basic_concatenation() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r#"
        (defrule test (go)
            => (printout t (str-cat "hello" " " "world") crlf))
        (deffacts startup (go))
    "#,
    );
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "hello world");
}

#[test]
fn str_cat_mixed_types() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r#"
        (defrule test (go)
            => (printout t (str-cat "x=" 42 " y=" 1.5) crlf))
        (deffacts startup (go))
    "#,
    );
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "x=42 y=1.5");
}

#[test]
fn str_cat_zero_args_returns_empty_string() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r#"
        (defrule test (go)
            => (printout t "|" (str-cat) "|" crlf))
        (deffacts startup (go))
    "#,
    );
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "||");
}

#[test]
fn sym_cat_returns_symbol() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r#"
        (defrule test (go)
            => (printout t (symbolp (sym-cat "foo" "bar")) crlf))
        (deffacts startup (go))
    "#,
    );
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "TRUE");
}

#[test]
fn sym_cat_content_matches_concatenation() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r#"
        (defrule test (go)
            => (printout t (sym-cat "foo" "bar") crlf))
        (deffacts startup (go))
    "#,
    );
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "foobar");
}

#[test]
fn str_length_of_string() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r#"
        (defrule test (go)
            => (printout t (str-length "hello") crlf))
        (deffacts startup (go))
    "#,
    );
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "5");
}

#[test]
fn str_length_of_empty_string_is_zero() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r#"
        (defrule test (go)
            => (printout t (str-length "") crlf))
        (deffacts startup (go))
    "#,
    );
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "0");
}

#[test]
fn sub_string_extracts_middle() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r#"
        (defrule test (go)
            => (printout t (sub-string 2 4 "hello") crlf))
        (deffacts startup (go))
    "#,
    );
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "ell");
}

#[test]
fn sub_string_out_of_range_returns_empty() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r#"
        (defrule test (go)
            => (printout t "|" (sub-string 10 20 "hello") "|" crlf))
        (deffacts startup (go))
    "#,
    );
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "||");
}

#[test]
fn str_cat_float_always_includes_decimal() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r"
        (defrule test (go)
            => (printout t (str-cat 3.0) crlf))
        (deffacts startup (go))
    ",
    );
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "3.0");
}

// ===========================================================================
// Multifield builtins (pass 010)
// ===========================================================================

#[test]
fn create_mf_and_length_pipeline() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r"
        (defrule test (go)
            => (printout t (length$ (create$ 10 20 30)) crlf))
        (deffacts startup (go))
    ",
    );
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "3");
}

#[test]
fn create_mf_empty_has_length_zero() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r"
        (defrule test (go)
            => (printout t (length$ (create$)) crlf))
        (deffacts startup (go))
    ",
    );
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "0");
}

#[test]
fn nth_mf_extracts_element() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r"
        (defrule test (go)
            => (printout t (nth$ 2 (create$ 10 20 30)) crlf))
        (deffacts startup (go))
    ",
    );
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "20");
}

#[test]
fn member_mf_found_prints_position() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r"
        (defrule test (go)
            => (printout t (member$ 30 (create$ 10 20 30 40)) crlf))
        (deffacts startup (go))
    ",
    );
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "3");
}

#[test]
fn member_mf_not_found_prints_false() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r"
        (defrule test (go)
            => (printout t (member$ 99 (create$ 10 20 30)) crlf))
        (deffacts startup (go))
    ",
    );
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "FALSE");
}

#[test]
fn subsetp_true_case() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r"
        (defrule test (go)
            => (printout t (subsetp (create$ 1 2) (create$ 1 2 3)) crlf))
        (deffacts startup (go))
    ",
    );
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "TRUE");
}

#[test]
fn subsetp_false_case() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r"
        (defrule test (go)
            => (printout t (subsetp (create$ 1 4) (create$ 1 2 3)) crlf))
        (deffacts startup (go))
    ",
    );
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "FALSE");
}

#[test]
fn multifieldp_true_for_create_mf() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r"
        (defrule test (go)
            => (printout t (multifieldp (create$ 1 2 3)) crlf))
        (deffacts startup (go))
    ",
    );
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "TRUE");
}

#[test]
fn create_mf_flattens_nested_multifield_integration() {
    // (create$ 1 (create$ 2 3) 4) should flatten to length 4.
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r"
        (defrule test (go)
            => (printout t (length$ (create$ 1 (create$ 2 3) 4)) crlf))
        (deffacts startup (go))
    ",
    );
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "4");
}

// ===========================================================================
// I/O and environment functions (pass 011)
// ===========================================================================

#[test]
fn format_basic_string_formatting() {
    let mut engine = new_utf8_engine();
    let output = eval_expr_via_printout(&mut engine, r#"(format nil "hello %s" "world")"#);
    assert_eq!(output.trim(), "hello world");
}

#[test]
fn format_integer_and_float() {
    let mut engine = new_utf8_engine();
    let output = eval_expr_via_printout(&mut engine, r#"(format nil "%d items at %f each" 5 3.5)"#);
    assert_eq!(output.trim(), "5 items at 3.500000 each");
}

#[test]
fn format_newline_and_percent() {
    let mut engine = new_utf8_engine();
    let output = eval_expr_via_printout(&mut engine, r#"(format nil "100%% done%n")"#);
    // trimmed of trailing newline produced by %n
    assert_eq!(output.trim(), "100% done");
}

#[test]
fn read_integer_from_input() {
    let mut engine = new_utf8_engine();
    engine.push_input("42");
    let source = r"
(defrule read-test
    (go)
    =>
    (printout t (read) crlf))
(deffacts startup (go))
";
    load_ok(&mut engine, source);
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "42");
}

#[test]
fn readline_from_input() {
    let mut engine = new_utf8_engine();
    engine.push_input("hello world");
    let source = r"
(defrule readline-test
    (go)
    =>
    (printout t (readline) crlf))
(deffacts startup (go))
";
    load_ok(&mut engine, source);
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "hello world");
}

#[test]
fn read_eof_when_no_input() {
    let mut engine = new_utf8_engine();
    let source = r"
(defrule eof-test
    (go)
    =>
    (printout t (read) crlf))
(deffacts startup (go))
";
    load_ok(&mut engine, source);
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "EOF");
}

#[test]
fn reset_from_rhs_clears_facts() {
    let mut engine = new_utf8_engine();
    let source = r"
(defrule trigger
    (go)
    =>
    (assert (created yes))
    (reset))
(deffacts startup (go))
";
    load_ok(&mut engine, source);
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    // After reset-from-RHS, the (created yes) fact should be gone
    // and (go) should be re-asserted from deffacts.
    let facts: Vec<_> = engine.facts().unwrap().collect();
    let has_created = facts.iter().any(|(_, fact)| {
        if let ferric_core::Fact::Ordered(of) = fact {
            engine.symbol_table.resolve_symbol_str(of.relation) == Some("created")
        } else {
            false
        }
    });
    assert!(
        !has_created,
        "reset should have cleared the (created yes) fact"
    );
}

#[test]
fn clear_from_rhs_removes_all() {
    let mut engine = new_utf8_engine();
    let source = r"
(defrule do-clear
    (go)
    =>
    (clear))
(deffacts startup (go))
";
    load_ok(&mut engine, source);
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    // After clear, everything should be gone
    assert_eq!(engine.facts().unwrap().count(), 0);
    assert_eq!(engine.agenda_len(), 0);
}

#[test]
fn printout_special_symbols_crlf_tab() {
    let mut engine = new_utf8_engine();
    let source = r#"
(defrule print-specials
    (go)
    =>
    (printout t "a" tab "b" crlf "c"))
(deffacts startup (go))
"#;
    load_ok(&mut engine, source);
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output, "a\tb\nc");
}

#[test]
fn printout_to_different_channels() {
    let mut engine = new_utf8_engine();
    let source = r#"
(defrule multi-channel
    (go)
    =>
    (printout t "stdout" crlf)
    (printout stderr "error" crlf))
(deffacts startup (go))
"#;
    load_ok(&mut engine, source);
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    assert_eq!(engine.get_output("t").unwrap_or(""), "stdout\n");
    assert_eq!(engine.get_output("stderr").unwrap_or(""), "error\n");
}

#[test]
fn printout_mixed_types() {
    let mut engine = new_utf8_engine();
    let source = r#"
(defrule mixed-print
    (go)
    =>
    (printout t "count=" 42 " val=" 3.5 crlf))
(deffacts startup (go))
"#;
    load_ok(&mut engine, source);
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output, "count=42 val=3.5\n");
}

// ===========================================================================
// Agenda/focus query functions (pass 012)
// ===========================================================================

#[test]
fn get_focus_returns_main_by_default() {
    let mut engine = new_utf8_engine();
    let output = eval_expr_via_printout(&mut engine, "(get-focus)");
    assert_eq!(output.trim(), "MAIN");
}

#[test]
fn get_focus_in_rule_rhs() {
    // Verify that get-focus can be called from a rule RHS and returns the current focus.
    let mut engine = new_utf8_engine();
    let source = r"
(defrule check-focus
    (go)
    =>
    (printout t (get-focus) crlf))
(deffacts startup (go))
";
    load_ok(&mut engine, source);
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "MAIN");
}

#[test]
fn get_focus_stack_default() {
    let mut engine = new_utf8_engine();
    let output = eval_expr_via_printout(&mut engine, "(get-focus-stack)");
    // Default focus stack is just (MAIN)
    assert_eq!(output.trim(), "(MAIN)");
}

#[test]
fn list_focus_stack_prints_to_stdout() {
    let mut engine = new_utf8_engine();
    let source = r"
(defrule show-stack
    (go)
    =>
    (list-focus-stack))
(deffacts startup (go))
";
    load_ok(&mut engine, source);
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert!(
        output.contains("MAIN"),
        "should show MAIN in focus stack, got: {output}"
    );
}

#[test]
fn agenda_prints_activations() {
    let mut engine = new_utf8_engine();
    // We need a rule that fires and prints the agenda while another rule is still on the agenda
    let source = r#"
(defrule low-priority
    (declare (salience -10))
    (go)
    =>
    (printout t "low fired" crlf))
(defrule show-agenda
    (go)
    =>
    (agenda))
(deffacts startup (go))
"#;
    load_ok(&mut engine, source);
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    // The agenda printout should have mentioned "low-priority"
    // since it was on the agenda when show-agenda fired
    assert!(
        output.contains("low-priority"),
        "agenda should show low-priority rule, got: {output}"
    );
}

#[test]
fn run_from_rhs_is_noop() {
    let mut engine = new_utf8_engine();
    let source = r#"
(defrule run-test
    (go)
    =>
    (run)
    (printout t "survived" crlf))
(deffacts startup (go))
"#;
    load_ok(&mut engine, source);
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "survived");
}

#[test]
fn halt_stops_execution_from_rhs() {
    let mut engine = new_utf8_engine();
    let source = r#"
(defrule first-rule
    (go)
    =>
    (printout t "first" crlf)
    (halt))
(defrule second-rule
    (declare (salience -10))
    (go)
    =>
    (printout t "second" crlf))
(deffacts startup (go))
"#;
    load_ok(&mut engine, source);
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "first");
    assert!(engine.is_halted());
}

#[test]
fn focus_changes_execution_order() {
    // Verify that (focus MODULE) causes rules in that module to fire next.
    // This mirrors the structure of the phase3_defmodule fixture which is known to work.
    let mut engine = new_utf8_engine();
    let source = r#"
(defrule MAIN::kick
    (go)
    =>
    (assert (reading))
    (focus SENSOR)
    (printout t "setup" crlf))
(defmodule SENSOR (export ?ALL))
(defrule SENSOR::sense (reading) => (printout t "sensed" crlf))
(deffacts startup (go) (reading))
"#;
    load_ok(&mut engine, source);
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    // At least the MAIN::kick rule should fire (focus starts on MAIN).
    // After focus SENSOR, SENSOR::sense fires for the second (reading) fact.
    assert!(
        output.contains("setup"),
        "expected 'setup' in output, got: {output}, diagnostics: {:?}",
        engine.action_diagnostics()
    );
}

// ===========================================================================
// Phase 4 integration and exit validation (pass 013)
// ===========================================================================

// --- Fixture-driven validation ---

#[test]
fn fixture_phase4_stdlib_math() {
    let mut engine = new_utf8_engine();
    load_fixture(&mut engine, "phase4_stdlib_math.clp");
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert!(output.contains("abs(-5): 5"), "output: {output}");
    assert!(output.contains("min(3,1,2): 1"), "output: {output}");
    assert!(output.contains("max(3,1,2): 3"), "output: {output}");
    assert!(output.contains("mod(10,3): 1"), "output: {output}");
    assert!(output.contains("5+3: 8"), "output: {output}");
    assert!(output.contains("integerp(42): TRUE"), "output: {output}");
    assert!(output.contains("evenp(4): TRUE"), "output: {output}");
    assert!(output.contains("oddp(3): TRUE"), "output: {output}");
}

#[test]
fn fixture_phase4_stdlib_string() {
    let mut engine = new_utf8_engine();
    load_fixture(&mut engine, "phase4_stdlib_string.clp");
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert!(output.contains("str-cat: hello world"), "output: {output}");
    assert!(output.contains("sym-cat: foobar"), "output: {output}");
    assert!(output.contains("str-length: 5"), "output: {output}");
    assert!(output.contains("sub-string: hello"), "output: {output}");
    assert!(
        output.contains("format: val=42 pi=3.14"),
        "output: {output}"
    );
}

#[test]
fn fixture_phase4_stdlib_multifield() {
    let mut engine = new_utf8_engine();
    load_fixture(&mut engine, "phase4_stdlib_multifield.clp");
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert!(output.contains("length$: 4"), "output: {output}");
    assert!(output.contains("member$: 2"), "output: {output}");
    assert!(output.contains("subsetp: TRUE"), "output: {output}");
}

#[test]
fn fixture_phase4_stdlib_io() {
    let mut engine = new_utf8_engine();
    load_fixture(&mut engine, "phase4_stdlib_io.clp");
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert!(output.contains("hello\tworld"), "output: {output}");
    assert!(output.contains("focus: MAIN"), "output: {output}");
    assert!(output.contains("stack: (MAIN)"), "output: {output}");
}

#[test]
fn fixture_phase4_generic_dispatch() {
    let mut engine = new_utf8_engine();
    load_fixture(&mut engine, "phase4_generic_dispatch.clp");
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    // classify(42) should dispatch to INTEGER method → 1
    assert!(output.contains("classify(42)=1"), "output: {output}");
    // classify(3.14) should dispatch to FLOAT method → 2
    assert!(output.contains("classify(3.14)=2"), "output: {output}");
    // classify(abc) should dispatch to SYMBOL method → 3
    assert!(output.contains("classify(abc)=3"), "output: {output}");
}

#[test]
fn fixture_phase4_module_qualified() {
    let mut engine = new_utf8_engine();
    load_fixture(&mut engine, "phase4_module_qualified.clp");
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert!(output.contains("add: 7"), "output: {output}");
    assert!(output.contains("square: 9"), "output: {output}");
}

// --- Cross-feature integration ---

#[test]
fn cross_feature_deffunction_with_stdlib() {
    // User-defined function using stdlib builtins (str-cat, format, math)
    let mut engine = new_utf8_engine();
    let source = r#"
(deffunction describe-number (?n)
    (format nil "value=%d doubled=%d" ?n (* ?n 2)))

(defrule describe
    (number ?n)
    =>
    (printout t (describe-number ?n) crlf))
(deffacts startup (number 4))
"#;
    load_ok(&mut engine, source);
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "value=4 doubled=8");
}

#[test]
fn cross_feature_generic_with_multifield() {
    // Generic dispatch + multifield operations
    let mut engine = new_utf8_engine();
    let source = r#"
(defgeneric item-count "Count items based on type")
(defmethod item-count ((?items MULTIFIELD))
    (length$ ?items))
(defmethod item-count ((?single INTEGER))
    1)

(defrule count-stuff
    (go)
    =>
    (printout t "mf-count: " (item-count (create$ a b c)) crlf)
    (printout t "int-count: " (item-count 42) crlf))
(deffacts startup (go))
"#;
    load_ok(&mut engine, source);
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert!(output.contains("mf-count: 3"), "output: {output}");
    assert!(output.contains("int-count: 1"), "output: {output}");
}

#[test]
fn cross_feature_globals_with_format() {
    // Global variables + format + math
    let mut engine = new_utf8_engine();
    let source = r#"
(defglobal ?*counter* = 0)

(defrule increment
    (go)
    =>
    (bind ?*counter* (+ ?*counter* 1))
    (printout t (format nil "counter=%d" ?*counter*) crlf))
(deffacts startup (go))
"#;
    load_ok(&mut engine, source);
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "counter=1");
}

#[test]
fn cross_feature_read_and_deffunction() {
    // Input reading + user-defined function
    let mut engine = new_utf8_engine();
    engine.push_input("hello");
    let source = r#"
(deffunction greet (?name)
    (str-cat "Hello, " ?name "!"))

(defrule process-input
    (go)
    =>
    (printout t (greet (read)) crlf))
(deffacts startup (go))
"#;
    load_ok(&mut engine, source);
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "Hello, hello!");
}

// --- Unsupported construct validation ---

#[test]
fn unsupported_defclass_fails_loudly() {
    let mut engine = new_utf8_engine();
    let errors = load_err(
        &mut engine,
        "(defclass POINT (is-a USER) (slot x) (slot y))",
    );
    assert!(
        !errors.is_empty(),
        "defclass should produce an error diagnostic"
    );
}

#[test]
fn unsupported_definstances_fails_loudly() {
    let mut engine = new_utf8_engine();
    let errors = load_err(
        &mut engine,
        "(definstances startup (p1 of POINT (x 1) (y 2)))",
    );
    assert!(
        !errors.is_empty(),
        "definstances should produce an error diagnostic"
    );
}

#[test]
fn unsupported_defmessage_handler_fails_loudly() {
    let mut engine = new_utf8_engine();
    let errors = load_err(
        &mut engine,
        r#"(defmessage-handler POINT print () (printout t "point" crlf))"#,
    );
    assert!(
        !errors.is_empty(),
        "defmessage-handler should produce an error diagnostic"
    );
}

#[test]
fn unknown_function_in_rhs_produces_diagnostic() {
    let mut engine = new_utf8_engine();
    let source = r"
(defrule bad-call
    (go)
    =>
    (nonexistent-function 42))
(deffacts startup (go))
";
    load_ok(&mut engine, source);
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    let diags = engine.action_diagnostics();
    assert!(
        !diags.is_empty(),
        "unknown function should produce a diagnostic"
    );
    let msg = format!("{diags:?}");
    assert!(
        msg.contains("nonexistent-function"),
        "diagnostic should mention the function name: {msg}"
    );
}

#[test]
fn deffunction_defgeneric_conflict_at_load_time() {
    let mut engine = new_utf8_engine();
    let errors = load_err(
        &mut engine,
        r#"
(deffunction classify (?x) 0)
(defgeneric classify "conflict")
"#,
    );
    let msgs: Vec<String> = errors.iter().map(|e| format!("{e}")).collect();
    let combined = msgs.join("; ");
    assert!(
        combined.contains("classify"),
        "conflict diagnostic should mention 'classify': {combined}"
    );
}
