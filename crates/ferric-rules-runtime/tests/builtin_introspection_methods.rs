//! Metadata and generic control behavior verified against CLIPS 6.30.
use ferric_rules_runtime::{Engine, EngineConfig, RunLimit};

fn output(source: &str) -> String {
    let mut engine = Engine::with_rules(source).unwrap();
    engine.run(RunLimit::Unlimited).unwrap();
    assert!(
        engine.action_diagnostics().is_empty(),
        "{:?}",
        engine.action_diagnostics()
    );
    engine.get_output("t").unwrap_or("").to_owned()
}

#[test]
fn template_metadata_preserves_types_facets_and_dynamic_default_effects() {
    assert_eq!(output(r#"
        (defglobal ?*calls* = 0)
        (deffunction next () (bind ?*calls* (+ ?*calls* 1)))
        (deftemplate item
          (slot free)
          (slot values (allowed-values a 1 "s" 2.5 [one]))
          (slot mixed (allowed-integers 1 2) (allowed-symbols x y) (allowed-strings "s" "t"))
          (slot n (type NUMBER) (range 1 9.0))
          (multislot tags (allowed-symbols x y) (cardinality 2 3))
          (slot required (default ?NONE))
          (slot dynamic (default-dynamic (next))))
        (defrule report =>
          (printout t (deftemplate-slot-names item) crlf
            (deftemplate-slot-types item free) crlf
            (deftemplate-slot-allowed-values item free) "|" (deftemplate-slot-range item free) crlf
            (deftemplate-slot-allowed-values item values) crlf
            (deftemplate-slot-allowed-values item mixed) crlf
            (deftemplate-slot-types item n) "|" (deftemplate-slot-range item n) "|" (deftemplate-slot-default-value item n) crlf
            (deftemplate-slot-multip item tags) "|" (deftemplate-slot-singlep item tags) "|" (deftemplate-slot-cardinality item tags) "|" (deftemplate-slot-default-value item tags) crlf
            (deftemplate-slot-defaultp item required) "|" (deftemplate-slot-default-value item required) crlf
            (deftemplate-slot-defaultp item dynamic) "|" ?*calls* "|"
            (deftemplate-slot-default-value item dynamic) "|" (deftemplate-slot-default-value item dynamic) "|" ?*calls* crlf))
    "#), "(free values mixed n tags required dynamic)\n(FLOAT INTEGER SYMBOL STRING EXTERNAL-ADDRESS FACT-ADDRESS INSTANCE-ADDRESS INSTANCE-NAME)\nFALSE|(-oo +oo)\n(a 1 \"s\" 2.5 [one])\n(1 2 x y \"s\" \"t\")\n(FLOAT INTEGER)|(1 9.0)|1\nTRUE|FALSE|(2 3)|(x x)\nFALSE|?NONE\ndynamic|0|1|2|2\n");
}

#[test]
fn method_queries_are_lazy_and_next_methodp_does_not_advance() {
    assert_eq!(output(r#"
        (defglobal ?*q* = 0)
        (deffunction probe () (bind ?*q* (+ ?*q* 1)) TRUE)
        (defgeneric chain)
        (defmethod chain 1 (?x) (printout t "base:" ?x ":" (next-methodp) ";") ?x)
        (defmethod chain 2 ((?x NUMBER (probe))) (printout t "number:" ?x ":" (next-methodp) ";") (call-next-method))
        (defmethod chain 3 ((?x INTEGER)) (printout t "integer:" ?x ":" (next-methodp) ":" (next-methodp) ";") (call-next-method))
        (defrule report => (printout t (chain 7) "|" ?*q* crlf (call-specific-method chain 2 8) "|" ?*q* crlf (next-methodp) crlf))
    "#), "integer:7:TRUE:TRUE;number:7:TRUE;base:7:FALSE;7|3\nnumber:8:TRUE;base:8:FALSE;8|4\nFALSE\n");
}

#[test]
fn override_changes_arguments_without_restarting_or_mutating_caller_chain() {
    assert_eq!(
        output(
            r#"
        (defgeneric change)
        (defmethod change 1 (?x) (printout t "B:" ?x ":" (next-methodp) ";") ?x)
        (defmethod change 2 ((?x SYMBOL)) (printout t "S:" ?x ";") ?x)
        (defmethod change 3 ((?x INTEGER)) (printout t "I:" ?x ";") (create$ (override-next-method text) (call-next-method)))
        (defrule report => (printout t (change 7) crlf))
    "#
        ),
        "I:7;B:text:FALSE;B:7:FALSE;(text 7)\n"
    );
}

#[test]
fn nested_specific_dispatch_restores_outer_chain_and_functions_do_not_inherit_it() {
    assert_eq!(
        output(
            r#"
        (deffunction helper () (next-methodp))
        (defgeneric cross)
        (defmethod cross 1 (?x) (printout t "cross:" ?x ":" (next-methodp) ";") ?x)
        (defgeneric scope)
        (defmethod scope 1 (?x) (printout t "base:" ?x ";") ?x)
        (defmethod scope 2 ((?x INTEGER)) (create$ (helper) (call-specific-method cross 1 8) (next-methodp) (call-next-method)))
        (defrule report => (printout t (scope 7) crlf))
    "#
        ),
        "cross:8:FALSE;base:7;(FALSE 8 TRUE 7)\n"
    );
}

#[test]
fn missing_method_selectors_skip_later_operand_effects() {
    for expression in [
        "(call-specific-method missing (mark) (mark))",
        "(call-specific-method known 99 (mark))",
        "(override-next-method (mark))",
    ] {
        let source = format!("(deffunction mark () (printout t bad) 1) (defgeneric known) (defmethod known 1 (?x) ?x) (defrule report => {expression})");
        let mut engine = Engine::with_rules(&source).unwrap();
        engine.run(RunLimit::Unlimited).unwrap();
        assert!(!engine.action_diagnostics().is_empty(), "{expression}");
        assert_eq!(engine.get_output("t").unwrap_or(""), "", "{expression}");
    }
}

#[test]
fn template_declarations_survive_retraction_reset_and_redefinition() {
    let mut engine = Engine::new(EngineConfig::default());
    engine.load_str("(deftemplate z (slot x)) (deftemplate a (slot x)) (deftemplate z (slot y)) (deffacts seed (ordered 1)) (deffunction dormant () (assert (nested 1)))").unwrap();
    engine.load_str("(defrule report => (printout t (get-deftemplate-list) crlf (deftemplate-slot-names ordered) crlf))").unwrap();
    engine.reset().unwrap();
    engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(
        engine.get_output("t"),
        Some("(initial-fact a z ordered nested)\n(implied)\n")
    );
    engine
        .load_str("(assert (transient 1)) (retract 1)")
        .unwrap_err();
    // The public host path must also retain a relation after the last fact goes.
    let id = engine.assert_ordered("host-only", ()).unwrap();
    engine.retract(id).unwrap();
    engine.reset().unwrap();
    engine.run(RunLimit::Unlimited).unwrap();
    assert!(engine.get_output("t").unwrap().contains("host-only"));
}

#[test]
fn construct_lists_are_owned_by_the_active_callable_module() {
    assert_eq!(
        output(
            r#"
        (defmodule A (export ?ALL))
        (deftemplate p (slot x))
        (defglobal ?*x* = 1)
        (defrule r (p (x none)) =>)
        (deffunction lists () (create$ (get-deftemplate-list) (get-defglobal-list) (get-defrule-list)))
        (defmodule B (import A ?ALL))
        (deftemplate local (slot x))
        (defrule report => (printout t (get-deftemplate-list) "|" (get-defglobal-list) "|" (get-defrule-list) crlf (lists) crlf (get-deftemplate-list *) crlf))
        (defrule MAIN::start => (focus B))
    "#
        ),
        "(local)|()|(report)\n(p x r)\n(MAIN::initial-fact A::p B::local)\n"
    );
}

#[test]
fn qualified_metadata_and_specific_methods_can_name_private_constructs() {
    assert_eq!(
        output(
            r#"
        (defmodule A)
        (deftemplate item (slot x))
        (defgeneric g)
        (defmethod g 1 (?x) ?x)
        (defmodule B)
        (defrule report => (printout t (deftemplate-slot-names A::item) "|" (call-specific-method A::g 1 7) crlf))
        (defrule MAIN::start => (focus B))
    "#
        ),
        "(x)|7\n"
    );
}

#[test]
fn rule_list_order_survives_slot_reuse_and_replacement() {
    let mut engine = Engine::new(EngineConfig::default());
    engine.load_str("(defrule z (never) =>) (defrule a (never) =>) (deffunction list-rules () (get-defrule-list))").unwrap();
    engine
        .load_str("(defrule z (never) => (printout t bad)) (defrule b (never) =>)")
        .unwrap();
    engine.load_str("(defrule report => (printout t (get-defrule-list) crlf) (undefrule a) (printout t (get-defrule-list) crlf))").unwrap();
    engine.reset().unwrap();
    engine.run(RunLimit::Unlimited).unwrap();
    assert!(engine.action_diagnostics().is_empty());
    assert_eq!(
        engine.get_output("t"),
        Some("(a z b report)\n(z b report)\n")
    );
}

#[cfg(feature = "serde")]
#[test]
fn restored_declaration_membership_deduplicates_and_clear_discards_old_names() {
    use ferric_rules_runtime::SerializationFormat;
    let mut engine = Engine::new(EngineConfig::default());
    let fact = engine.assert_ordered("old", ()).unwrap();
    engine.retract(fact).unwrap();
    engine
        .load_str("(defrule report => (printout t (get-deftemplate-list) crlf))")
        .unwrap();
    for format in [SerializationFormat::Json, SerializationFormat::Cbor] {
        let mut restored = Engine::deserialize(&engine.serialize(format).unwrap(), format).unwrap();
        restored.assert_ordered("old", ()).unwrap();
        restored.assert_ordered("new", ()).unwrap();
        restored.reset().unwrap();
        restored.run(RunLimit::Unlimited).unwrap();
        assert_eq!(restored.get_output("t"), Some("(initial-fact old new)\n"));
        restored.clear();
        restored.assert_ordered("new", ()).unwrap();
        restored
            .load_str("(defrule report => (printout t (get-deftemplate-list) crlf))")
            .unwrap();
        restored.run(RunLimit::Unlimited).unwrap();
        assert_eq!(restored.get_output("t"), Some("(initial-fact new)\n"));
    }
}
