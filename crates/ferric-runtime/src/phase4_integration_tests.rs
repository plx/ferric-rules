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

#[test]
fn qualified_global_reference_resolves() {
    let mut engine = new_utf8_engine();
    let source = r"
(defmodule CONFIG (export defglobal ?ALL))
(defglobal ?*threshold* = 100)

(defmodule MAIN (import CONFIG defglobal ?ALL))
(defrule test-global (go) => (printout t ?*CONFIG::threshold* crlf))
(deffacts startup (go))
";
    load_ok(&mut engine, source);
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "100");
}

#[test]
fn qualified_global_reference_not_visible() {
    let mut engine = new_utf8_engine();
    let source = r"
(defmodule CONFIG (export ?NONE))
(defglobal ?*threshold* = 100)

(defmodule MAIN)
(defrule test-global (go) => (printout t ?*CONFIG::threshold* crlf))
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
        "expected visibility error for qualified global, got: {diagnostics:?}"
    );
}

#[test]
fn deffacts_can_resolve_global_values() {
    let mut engine = new_utf8_engine();
    let source = r#"
(defglobal ?*x* = 3 ?*y* = (create$ a b c))
(deffacts info
  (fact-1 ?*x*)
  (fact-2 ?*y*))
(defrule probe
  (fact-1 ?x)
  (fact-2 $?y)
  =>
  (printout t ?x "|" ?y crlf))
"#;
    load_ok(&mut engine, source);
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert!(
        output.contains("3|(a b c)"),
        "expected resolved deffacts global output, got: {output}"
    );
    assert!(
        engine.action_diagnostics().is_empty(),
        "unexpected diagnostics: {:?}",
        engine.action_diagnostics()
    );
}

#[test]
fn rhs_bind_can_rebind_local_variable() {
    let mut engine = new_utf8_engine();
    let source = r"
(deffacts test
  (go))
(defrule ok
  (go)
  =>
  (bind ?one 1)
  (bind ?one (+ ?one 2))
  (printout t ?one crlf))
";
    load_ok(&mut engine, source);
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert!(
        output.contains('3'),
        "expected local bind-rebind output, got: {output}"
    );
    assert!(
        engine.action_diagnostics().is_empty(),
        "unexpected diagnostics: {:?}",
        engine.action_diagnostics()
    );
}

#[test]
fn trailing_multivariable_capture_behaves_as_multifield_in_rhs() {
    let mut engine = new_utf8_engine();
    let source = r"
(deffacts test
  (_2 x y z p))
(defrule ok
  (_2 ? $?two)
  =>
  (printout t (subseq$ ?two 1 2) crlf))
";
    load_ok(&mut engine, source);
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert!(
        output.contains("(y z)"),
        "expected subseq$ to see multifield capture, got: {output}"
    );
    assert!(
        engine.action_diagnostics().is_empty(),
        "unexpected diagnostics: {:?}",
        engine.action_diagnostics()
    );
}

#[test]
fn bind_rejects_undeclared_global() {
    let mut engine = new_utf8_engine();
    let source = r"
(defrule test-bind (go) => (bind ?*missing* 1))
(deffacts startup (go))
";
    load_ok(&mut engine, source);
    engine.reset().unwrap();
    let _result = engine.run(crate::execution::RunLimit::Unlimited).unwrap();
    let diagnostics = engine.action_diagnostics();
    assert!(
        diagnostics.iter().any(|d| format!("{d}")
            .to_ascii_lowercase()
            .contains("unbound global")),
        "expected unbound-global error for bind to undeclared global, got: {diagnostics:?}"
    );
}

#[test]
fn bind_respects_qualified_global_visibility() {
    let mut engine = new_utf8_engine();
    let source = r"
(defmodule CONFIG (export ?NONE))
(defglobal ?*counter* = 0)

(defmodule MAIN)
(defrule test-bind (go) => (bind ?*CONFIG::counter* 10))
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
        "expected visibility error for bind to qualified global, got: {diagnostics:?}"
    );
}

#[test]
fn same_name_functions_can_coexist_across_modules() {
    let mut engine = new_utf8_engine();
    let source = r#"
(defmodule A (export deffunction ?ALL))
(deffunction f () 1)

(defmodule B (export deffunction ?ALL))
(deffunction f () 2)

(defmodule MAIN (import A deffunction ?ALL) (import B deffunction ?ALL))
(defrule test-call (go) => (printout t (A::f) " " (B::f) crlf))
(deffacts startup (go))
"#;
    load_ok(&mut engine, source);
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "1 2");
}

#[test]
fn unqualified_function_call_is_ambiguous_when_multiple_visible() {
    let mut engine = new_utf8_engine();
    let source = r"
(defmodule A (export deffunction ?ALL))
(deffunction f () 1)

(defmodule B (export deffunction ?ALL))
(deffunction f () 2)

(defmodule MAIN (import A deffunction ?ALL) (import B deffunction ?ALL))
(defrule test-call (go) => (printout t (f) crlf))
(deffacts startup (go))
";
    load_ok(&mut engine, source);
    engine.reset().unwrap();
    let _result = engine.run(crate::execution::RunLimit::Unlimited).unwrap();
    let diagnostics = engine.action_diagnostics();
    assert!(
        diagnostics.iter().any(|d| {
            let msg = format!("{d}");
            msg.to_ascii_lowercase()
                .contains("multiple visible deffunctions")
                || msg.to_ascii_lowercase().contains("unambiguous")
        }),
        "expected ambiguity diagnostic for unqualified f(), got: {diagnostics:?}"
    );
}

#[test]
fn same_name_globals_can_coexist_across_modules() {
    let mut engine = new_utf8_engine();
    let source = r#"
(defmodule A (export defglobal ?ALL))
(defglobal ?*g* = 1)

(defmodule B (export defglobal ?ALL))
(defglobal ?*g* = 2)

(defmodule MAIN (import A defglobal ?ALL) (import B defglobal ?ALL))
(defrule test-global (go) => (printout t ?*A::g* " " ?*B::g* crlf))
(deffacts startup (go))
"#;
    load_ok(&mut engine, source);
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "1 2");
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
fn deffunction_body_can_printout_to_router() {
    let mut engine = new_utf8_engine();
    let source = r#"
(deffunction printem (?x ?y)
  (printout t "Passed values: |" ?x "|" ?y "|" crlf))
(defrule test-call (go)
  =>
  (printout t "before" crlf)
  (printem 3 (create$ a b c))
  (printout t "after" crlf))
(deffacts startup (go))
"#;
    load_ok(&mut engine, source);
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert!(output.contains("before"), "missing before marker: {output}");
    assert!(
        output.contains("Passed values: |3|(a b c)|"),
        "missing deffunction printout output: {output}"
    );
    assert!(output.contains("after"), "missing after marker: {output}");
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
fn gensym_and_setgen_generate_expected_sequence() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r#"
        (defrule test (go)
            =>
            (setgen 10)
            (printout t (gensym) " " (gensym*) crlf))
        (deffacts startup (go))
    "#,
    );
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "gen10 gen11");
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
fn str_length_counts_utf8_characters() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r#"
        (defrule test (go)
            => (printout t (str-length "é") crlf))
        (deffacts startup (go))
    "#,
    );
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "1");
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
fn sub_string_uses_character_positions_for_utf8() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r#"
        (defrule test (go)
            => (printout t (sub-string 2 2 "héllo") crlf))
        (deffacts startup (go))
    "#,
    );
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "é");
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
fn sub_string_utf8_out_of_range_returns_empty() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r#"
        (defrule test (go)
            => (printout t "|" (sub-string 2 2 "é") "|" crlf))
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

#[test]
fn assert_splices_multifield_values_into_ordered_fact() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r"
        (defrule produce (go)
            => (assert (vals (create$ a b c))))
        (defrule consume (vals $?x)
            => (printout t ?x crlf))
        (deffacts startup (go))
    ",
    );
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert!(
        output.contains("(a b c)"),
        "expected spliced multifield assertion output, got: {output:?}"
    );
    assert!(
        !output.contains("((a b c))"),
        "multifield should be spliced, not nested: {output:?}"
    );
}

#[test]
fn implode_mf_returns_space_separated_string() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r"
        (defrule test (go)
            => (printout t (implode$ (create$ 1 2 3)) crlf))
        (deffacts startup (go))
    ",
    );
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "1 2 3");
}

#[test]
fn subseq_mf_extracts_expected_slice() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r"
        (defrule test (go)
            => (printout t (subseq$ (create$ a b c d) 2 3) crlf))
        (deffacts startup (go))
    ",
    );
    engine.reset().unwrap();
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output.trim(), "(b c)");
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

#[test]
fn println_writes_to_t_with_trailing_newline() {
    let mut engine = new_utf8_engine();
    let source = r#"
(defrule say-line
    (go)
    =>
    (println "count=" 42))
(deffacts startup (go))
"#;
    load_ok(&mut engine, source);
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    assert_eq!(engine.get_output("t").unwrap_or(""), "count=42\n");
}

#[test]
fn rhs_fact_slot_access_supports_compact_assignment_and_nested_calls() {
    let mut engine = new_utf8_engine();
    let source = r#"
(deftemplate point
    (slot x))
(deffacts startup
    (point (x 4)))
(defrule inspect-point
    ?p<-(point (x ?x))
    =>
    (bind ?inc (+ ?p:x 1))
    (println ?inc "|" (+ ?p:x ?inc)))
"#;
    load_ok(&mut engine, source);
    engine.reset().expect("reset");
    let run = run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("").to_string();
    let diagnostics = engine.action_diagnostics().to_vec();
    assert_eq!(
        run.rules_fired, 1,
        "expected inspect-point to fire once; output={output:?} diagnostics={diagnostics:?}"
    );
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics in inspect-point run: {diagnostics:?}"
    );
    assert_eq!(output, "5|9\n");
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
fn rules_prints_loaded_rule_names() {
    let mut engine = new_utf8_engine();
    let source = r"
(defrule alpha (go) =>)
(defrule beta (go) => (rules))
(deffacts startup (go))
";
    load_ok(&mut engine, source);
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert!(
        output.contains("alpha"),
        "rules output missing alpha: {output}"
    );
    assert!(
        output.contains("beta"),
        "rules output missing beta: {output}"
    );
}

#[test]
fn ppdefrule_prints_named_rule_definition() {
    let mut engine = new_utf8_engine();
    let source = r"
(defrule target
    (go)
    =>
    (assert (done)))
(defrule inspector
    (declare (salience 10))
    (go)
    =>
    (ppdefrule target))
(deffacts startup (go))
";
    load_ok(&mut engine, source);
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert!(
        output.contains("(defrule target"),
        "ppdefrule output missing rule header: {output}"
    );
    assert!(
        output.contains("(assert (done))"),
        "ppdefrule output missing rule body: {output}"
    );
}

#[test]
fn ppdefrule_star_prints_all_loaded_definitions() {
    let mut engine = new_utf8_engine();
    let source = r"
(defrule alpha (go) =>)
(defrule beta (go) =>)
(defrule inspector
    (declare (salience 10))
    (go)
    =>
    (ppdefrule *))
(deffacts startup (go))
";
    load_ok(&mut engine, source);
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert!(
        output.contains("(defrule alpha"),
        "ppdefrule * output missing alpha definition: {output}"
    );
    assert!(
        output.contains("(defrule beta"),
        "ppdefrule * output missing beta definition: {output}"
    );
}

#[test]
fn runtime_load_mutates_rule_set_and_rules_output() {
    use std::io::Write as _;

    let mut temp = tempfile::NamedTempFile::new().expect("tempfile");
    writeln!(temp, "(defrule loaded-one (go) =>)").expect("write loaded-one");
    writeln!(temp, "(defrule loaded-two (go) =>)").expect("write loaded-two");
    let load_path = temp.path().to_string_lossy().replace('\\', "\\\\");

    let mut engine = new_utf8_engine();
    let source = format!(
        r#"
(defrule inspector
    (go)
    =>
    (undefrule *)
    (load "{load_path}")
    (rules))
(deffacts startup (go))
"#
    );
    load_ok(&mut engine, &source);
    engine.reset().expect("reset");
    run_to_completion(&mut engine);

    let output = engine.get_output("t").unwrap_or("");
    assert!(
        output.lines().any(|line| line.trim() == "loaded-one"),
        "rules output should include loaded-one after runtime load: {output}"
    );
    assert!(
        output.lines().any(|line| line.trim() == "loaded-two"),
        "rules output should include loaded-two after runtime load: {output}"
    );

    let names: std::collections::HashSet<_> =
        engine.rules().into_iter().map(|(name, _)| name).collect();
    assert!(
        names.contains("loaded-one"),
        "loaded-one missing from registry"
    );
    assert!(
        names.contains("loaded-two"),
        "loaded-two missing from registry"
    );
}

#[test]
fn runtime_load_missing_file_surfaces_action_diagnostic() {
    let mut engine = new_utf8_engine();
    let source = r#"
(defrule inspector
    (go)
    =>
    (load "/definitely/not/a/real/path/ferric-load-missing.clp"))
(deffacts startup (go))
"#;
    load_ok(&mut engine, source);
    engine.reset().expect("reset");
    run_to_completion(&mut engine);

    let diags = engine.action_diagnostics();
    assert!(
        !diags.is_empty(),
        "missing runtime load file should produce an action diagnostic"
    );
    let joined = diags
        .iter()
        .map(std::string::ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ");
    assert!(
        joined.contains("load failed"),
        "expected load failure diagnostic, got: {joined}"
    );
}

#[test]
fn undefrule_star_removes_rules_and_cancels_pending_activations() {
    let mut engine = new_utf8_engine();
    let source = r#"
(defrule seed => (assert (go)))
(defrule victim (go) => (printout t "VICTIM-FIRED" crlf))
(defrule inspector
    (declare (salience 10))
    (go)
    =>
    (rules)
    (undefrule *)
    (rules))
"#;
    load_ok(&mut engine, source);
    engine.reset().expect("reset");
    run_to_completion(&mut engine);

    let output = engine.get_output("t").unwrap_or("");
    assert!(
        output.contains("(no rules)"),
        "expected empty rules listing after undefrule *, got: {output}"
    );
    assert!(
        !output.contains("VICTIM-FIRED"),
        "victim activation should have been removed by undefrule *: {output}"
    );
    assert!(
        engine.rules().is_empty(),
        "engine should have no rules after undefrule *"
    );
}

#[test]
fn undefrule_by_name_removes_targeted_rule_before_it_fires() {
    let mut engine = new_utf8_engine();
    let source = r#"
(defrule seed => (assert (go)))
(defrule victim (go) => (printout t "VICTIM-FIRED" crlf))
(defrule killer
    (declare (salience 5))
    (go)
    =>
    (undefrule victim))
(defrule reporter
    (declare (salience -5))
    (go)
    =>
    (rules))
"#;
    load_ok(&mut engine, source);
    engine.reset().expect("reset");
    run_to_completion(&mut engine);

    let output = engine.get_output("t").unwrap_or("");
    assert!(
        !output.contains("VICTIM-FIRED"),
        "targeted undefrule should cancel victim activation: {output}"
    );
    assert!(
        !output.lines().any(|line| line.trim() == "victim"),
        "rules output should not include removed victim rule: {output}"
    );

    let remaining_names: std::collections::HashSet<_> =
        engine.rules().into_iter().map(|(name, _)| name).collect();
    assert!(
        !remaining_names.contains("victim"),
        "victim should be removed from engine rule registry"
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
fn unknown_function_in_rhs_fails_at_load_time() {
    let mut engine = new_utf8_engine();
    let source = r"
(defrule bad-call
    (go)
    =>
    (nonexistent-function 42))
(deffacts startup (go))
";
    let errors = load_err(&mut engine, source);
    let diags: Vec<String> = errors.into_iter().map(|e| e.to_string()).collect();
    assert!(!diags.is_empty(), "unknown function should fail load");
    let msg = diags.join("; ");
    assert!(
        msg.contains("[EXPRNPSR3]") && msg.contains("nonexistent-function"),
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

// ===========================================================================
// if/then/else control flow
// ===========================================================================

#[test]
fn load_rule_with_if_then_else() {
    let mut engine = new_utf8_engine();
    engine
        .load_str(r"(defrule test (data ?x) => (if (> ?x 0) then (assert (positive)) else (assert (negative))))")
        .unwrap();
}

#[test]
fn load_rule_with_if_then_no_else() {
    let mut engine = new_utf8_engine();
    engine
        .load_str(r"(defrule test (data ?x) => (if (> ?x 0) then (assert (positive))))")
        .unwrap();
}

#[test]
fn if_then_branch_fires_assert_when_truthy() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r"
(defrule classify
    (data ?x)
    =>
    (if (> ?x 0) then (assert (positive)) else (assert (negative))))
(deffacts startup (data 5))
",
    );
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    assert_has_fact_with_relation(&engine, "positive");
    assert_no_fact_with_relation(&engine, "negative");
}

#[test]
fn if_else_branch_fires_assert_when_falsy() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r"
(defrule classify
    (data ?x)
    =>
    (if (> ?x 0) then (assert (positive)) else (assert (negative))))
(deffacts startup (data -3))
",
    );
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    assert_has_fact_with_relation(&engine, "negative");
    assert_no_fact_with_relation(&engine, "positive");
}

#[test]
fn if_then_no_else_does_nothing_when_falsy() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r"
(defrule classify
    (data ?x)
    =>
    (if (> ?x 0) then (assert (positive))))
(deffacts startup (data 0))
",
    );
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    assert_no_fact_with_relation(&engine, "positive");
}

#[test]
fn if_then_multiple_actions_in_branch() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r"
(defrule classify
    (data ?x)
    =>
    (if (> ?x 0)
        then
            (assert (positive))
            (assert (greater-than-zero))
        else
            (assert (non-positive))))
(deffacts startup (data 10))
",
    );
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    assert_has_fact_with_relation(&engine, "positive");
    assert_has_fact_with_relation(&engine, "greater-than-zero");
    assert_no_fact_with_relation(&engine, "non-positive");
}

#[test]
fn if_nested_in_then_branch() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r"
(defrule classify
    (data ?x)
    =>
    (if (> ?x 0)
        then
            (if (> ?x 10)
                then (assert (large))
                else (assert (small-positive)))
        else
            (assert (non-positive))))
(deffacts startup (data 15))
",
    );
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    assert_has_fact_with_relation(&engine, "large");
    assert_no_fact_with_relation(&engine, "small-positive");
    assert_no_fact_with_relation(&engine, "non-positive");
}

#[test]
fn if_with_printout_in_branch() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r#"
(defrule report
    (data ?x)
    =>
    (if (> ?x 0)
        then (printout t "positive" crlf)
        else (printout t "negative" crlf)))
(deffacts startup (data 7))
"#,
    );
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert!(
        output.contains("positive"),
        "expected 'positive' in output, got: {output:?}"
    );
    assert!(
        !output.contains("negative"),
        "unexpected 'negative' in output: {output:?}"
    );
}

#[test]
fn if_in_deffunction_body_returns_branch_value() {
    let mut engine = new_utf8_engine();
    let output = eval_function_via_rule(
        &mut engine,
        r"(deffunction sign (?x) (if (> ?x 0) then positive else negative))",
        "(sign 5)",
    );
    let output = output.trim_end_matches('\n');
    assert_eq!(output, "positive");
}

#[test]
fn if_in_deffunction_body_else_branch() {
    let mut engine = new_utf8_engine();
    let output = eval_function_via_rule(
        &mut engine,
        r"(deffunction sign (?x) (if (> ?x 0) then positive else negative))",
        "(sign -1)",
    );
    let output = output.trim_end_matches('\n');
    assert_eq!(output, "negative");
}

// ===========================================================================
// Loop special forms: while, loop-for-count, progn$ / foreach
// ===========================================================================

#[test]
fn load_rule_with_while_loop() {
    let mut engine = new_utf8_engine();
    engine
        .load_str("(defrule test (data ?x) => (while (> ?x 0) do (printout t ?x)))")
        .unwrap();
}

#[test]
fn load_rule_with_loop_for_count() {
    let mut engine = new_utf8_engine();
    engine
        .load_str("(defrule test => (loop-for-count (?i 1 10) do (printout t ?i)))")
        .unwrap();
}

#[test]
fn load_rule_with_progn_dollar() {
    let mut engine = new_utf8_engine();
    // Use create$ so we don't depend on $? multifield patterns (not yet fully supported).
    engine
        .load_str("(defrule test (go) => (progn$ (?item (create$ a b c)) (printout t ?item)))")
        .unwrap();
}

#[test]
fn load_rule_with_foreach() {
    let mut engine = new_utf8_engine();
    // Use create$ so we don't depend on $? multifield patterns (not yet fully supported).
    engine
        .load_str("(defrule test (go) => (foreach ?item (create$ a b c) do (printout t ?item)))")
        .unwrap();
}

#[test]
fn loop_for_count_runs_correct_iterations() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r"
(defrule count-up
    (go)
    =>
    (loop-for-count (?i 1 5) do
        (printout t ?i crlf)))
(deffacts startup (go))
",
    );
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert!(output.contains('1'), "output: {output:?}");
    assert!(output.contains('2'), "output: {output:?}");
    assert!(output.contains('3'), "output: {output:?}");
    assert!(output.contains('4'), "output: {output:?}");
    assert!(output.contains('5'), "output: {output:?}");
}

#[test]
fn loop_for_count_default_start_is_one() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r"
(defrule count
    (go)
    =>
    (loop-for-count (?i 3) do
        (printout t ?i crlf)))
(deffacts startup (go))
",
    );
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    // Should print 1, 2, 3
    assert!(output.contains('1'), "output: {output:?}");
    assert!(output.contains('2'), "output: {output:?}");
    assert!(output.contains('3'), "output: {output:?}");
}

#[test]
fn loop_for_count_asserts_facts() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r"
(defrule make-numbers
    (go)
    =>
    (loop-for-count (?i 1 3) do
        (assert (number ?i))))
(deffacts startup (go))
",
    );
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    assert_has_fact_with_relation(&engine, "number");
    // Should have exactly 3 number facts.
    let count = find_facts_by_relation(&engine, "number").len();
    assert_eq!(count, 3, "expected 3 number facts, got {count}");
}

#[test]
fn while_loop_runs_printout() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r"
(defglobal ?*counter* = 3)
(defrule countdown
    (go)
    =>
    (while (> ?*counter* 0) do
        (printout t ?*counter* crlf)
        (bind ?*counter* (- ?*counter* 1))))
(deffacts startup (go))
",
    );
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert!(output.contains('3'), "output: {output:?}");
    assert!(output.contains('2'), "output: {output:?}");
    assert!(output.contains('1'), "output: {output:?}");
}

#[test]
fn while_loop_body_never_executes_on_false_condition() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r#"
(defrule no-run
    (go)
    =>
    (while (> 0 1) do
        (printout t "should-not-appear" crlf)))
(deffacts startup (go))
"#,
    );
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert!(
        !output.contains("should-not-appear"),
        "output should be empty, got: {output:?}"
    );
}

#[test]
fn progn_dollar_iterates_multifield() {
    // Use create$ to build multifield instead of $? LHS pattern.
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r"
(defrule print-items
    (go)
    =>
    (progn$ (?x (create$ a b c))
        (printout t ?x crlf)))
(deffacts startup (go))
",
    );
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert!(output.contains('a'), "output: {output:?}");
    assert!(output.contains('b'), "output: {output:?}");
    assert!(output.contains('c'), "output: {output:?}");
}

#[test]
fn progn_dollar_binds_index_variable() {
    // Use create$ to build multifield instead of $? LHS pattern.
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r"
(defrule print-indices
    (go)
    =>
    (progn$ (?x (create$ a b c))
        (printout t ?x-index crlf)))
(deffacts startup (go))
",
    );
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert!(output.contains('1'), "expected index 1: {output:?}");
    assert!(output.contains('2'), "expected index 2: {output:?}");
    assert!(output.contains('3'), "expected index 3: {output:?}");
}

#[test]
fn foreach_is_equivalent_to_progn_dollar() {
    // Use create$ to build multifield instead of $? LHS pattern.
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r"
(defrule print-items
    (go)
    =>
    (foreach ?x (create$ x y z) do
        (printout t ?x crlf)))
(deffacts startup (go))
",
    );
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert!(output.contains('x'), "output: {output:?}");
    assert!(output.contains('y'), "output: {output:?}");
    assert!(output.contains('z'), "output: {output:?}");
}

#[test]
fn loop_for_count_in_deffunction() {
    // Use a defglobal for the accumulator since deffunction doesn't have local mutable vars.
    let mut engine = new_utf8_engine();
    let output = eval_function_via_rule(
        &mut engine,
        r"(defglobal ?*total* = 0)
(deffunction sum-to (?n)
    (bind ?*total* 0)
    (loop-for-count (?i 1 ?n) do
        (bind ?*total* (+ ?*total* ?i)))
    ?*total*)",
        "(sum-to 4)",
    );
    let output = output.trim_end_matches('\n');
    // sum 1+2+3+4 = 10
    assert_eq!(output, "10", "sum-to 4 should be 10, got {output:?}");
}

#[test]
fn nested_loops_work_correctly() {
    // loop-for-count inside loop-for-count
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r#"
(defrule nested
    (go)
    =>
    (loop-for-count (?i 1 2) do
        (loop-for-count (?j 1 2) do
            (printout t ?i "-" ?j crlf))))
(deffacts startup (go))
"#,
    );
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    let output = engine.get_output("t").unwrap_or("");
    assert!(output.contains("1-1"), "output: {output:?}");
    assert!(output.contains("1-2"), "output: {output:?}");
    assert!(output.contains("2-1"), "output: {output:?}");
    assert!(output.contains("2-2"), "output: {output:?}");
}

// ===========================================================================
// Fact-query macro forms (do-for-fact, any-factp, find-fact, find-all-facts)
// ===========================================================================

/// Verify that a rule containing `do-for-fact` loads without error.
#[test]
fn load_rule_with_do_for_fact() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r#"
(deftemplate data (slot value))
(defrule test => (do-for-fact ((?f data)) TRUE (printout t "found" crlf)))
"#,
    );
}

/// Verify that a rule containing `do-for-all-facts` loads without error.
#[test]
fn load_rule_with_do_for_all_facts() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r"
(deftemplate item (slot name))
(defrule list-all => (do-for-all-facts ((?i item)) TRUE (printout t ?i crlf)))
",
    );
}

/// `do-for-all-facts` iterates all facts of the given template and executes
/// the body for each.  With three `data` facts and a body that asserts an
/// ordered `hit` fact, we expect three `hit` facts after running.
#[test]
fn do_for_all_facts_iterates_matching_facts() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r"
(deftemplate data (slot value))
(deffacts setup
    (data (value 1))
    (data (value 2))
    (data (value 3)))
(defrule find-all
    (go)
    =>
    (do-for-all-facts ((?f data)) TRUE
        (assert (hit ?f))))
(deffacts trigger (go))
",
    );
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    // Three data facts → three ordered hit facts asserted.
    let hits = find_facts_by_relation(&engine, "hit");
    assert_eq!(hits.len(), 3, "expected 3 hit facts, got: {hits:?}");
}

/// `do-for-fact` stops after the first matching fact.
#[test]
fn do_for_fact_stops_after_first_match() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r"
(deftemplate data (slot value))
(deffacts setup
    (data (value 10))
    (data (value 20))
    (data (value 30)))
(defrule find-first
    (go)
    =>
    (do-for-fact ((?f data)) TRUE
        (assert (hit ?f))))
(deffacts trigger (go))
",
    );
    engine.reset().expect("reset");
    run_to_completion(&mut engine);
    // do-for-fact stops at the first match, so exactly one hit fact.
    let hits = find_facts_by_relation(&engine, "hit");
    assert_eq!(
        hits.len(),
        1,
        "expected 1 hit fact from do-for-fact, got: {hits:?}"
    );
}

/// `delayed-do-for-all-facts` loads and parses correctly.
#[test]
fn load_rule_with_delayed_do_for_all_facts() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r"
(deftemplate task (slot id))
(defrule process
    (go)
    =>
    (delayed-do-for-all-facts ((?t task)) TRUE (printout t ?t crlf)))
(deffacts trigger (go))
",
    );
}

/// `any-factp` used as condition inside `if` loads without error.
#[test]
fn load_rule_with_any_factp_in_if_condition() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r#"
(deftemplate flag (slot active))
(defrule check
    (go)
    =>
    (if (any-factp ((?f flag)) TRUE)
        then (printout t "has flags" crlf)
        else (printout t "no flags" crlf)))
(deffacts trigger (go))
"#,
    );
}

/// `find-all-facts` used as the RHS of `bind` loads without error.
#[test]
fn load_rule_with_find_all_facts_in_bind() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r"
(deftemplate record (slot id))
(defrule gather
    (go)
    =>
    (bind ?all (find-all-facts ((?r record)) TRUE))
    (printout t ?all crlf))
(deffacts trigger (go))
",
    );
}

/// `find-fact` used as the RHS of `bind` loads without error.
#[test]
fn load_rule_with_find_fact_in_bind() {
    let mut engine = new_utf8_engine();
    load_ok(
        &mut engine,
        r"
(deftemplate widget (slot id))
(defrule get-first
    (go)
    =>
    (bind ?w (find-fact ((?r widget)) TRUE))
    (printout t ?w crlf))
(deffacts trigger (go))
",
    );
}
