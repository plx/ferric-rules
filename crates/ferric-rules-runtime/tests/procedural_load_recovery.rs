//! A rejected callable must not reserve a namespace or method index.

use ferric_rules_runtime::{Engine, EngineConfig, HaltReason, RunLimit};

fn call(engine: &mut Engine, expression: &str) -> String {
    engine
        .load_str(&format!(
            "(defrule probe => (printout t {expression} crlf))"
        ))
        .unwrap();
    let module = engine.current_module().to_owned();
    engine.reset().unwrap();
    engine.set_focus(&module).unwrap();
    let result = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert!(engine.action_diagnostics().is_empty());
    engine.get_output("t").unwrap_or_default().to_owned()
}

#[test]
fn invalid_callable_does_not_block_a_later_valid_kind() {
    for (source, expected) in [
        (
            "(deffunction f (?x) (missing ?x))
             (defmethod f ((?x INTEGER)) (+ ?x 200))",
            "207\n",
        ),
        (
            "(deffunction f (?x) (missing ?x))
             (defgeneric f)
             (defmethod f ((?x INTEGER)) (+ ?x 200))",
            "207\n",
        ),
        (
            "(defmethod f ((?x INTEGER)) (missing ?x))
             (deffunction f (?x) (+ ?x 100))",
            "107\n",
        ),
    ] {
        let mut engine = Engine::new(EngineConfig::default());
        let errors = engine.load_str(source).expect_err(source);
        assert!(errors
            .iter()
            .any(|error| error.to_string().contains("missing")));
        assert_eq!(call(&mut engine, "(f 7)"), expected, "{source}");
    }
}

#[test]
fn the_first_valid_callable_kind_wins_in_source_order() {
    for (source, expected) in [
        (
            "(deffunction f (?x) (+ ?x 100))
             (defmethod f ((?x INTEGER)) (+ ?x 200))",
            "107\n",
        ),
        (
            "(defmethod f ((?x INTEGER)) (+ ?x 200))
             (deffunction f (?x) (+ ?x 100))",
            "207\n",
        ),
        (
            "(deffunction f (?x) (missing ?x))
             (defmethod f ((?x INTEGER)) (+ ?x 200))
             (deffunction f (?x) (+ ?x 100))",
            "207\n",
        ),
        (
            "(defmethod f ((?x INTEGER)) (missing ?x))
             (deffunction f (?x) (+ ?x 100))
             (defmethod f ((?x INTEGER)) (+ ?x 200))",
            "107\n",
        ),
    ] {
        let mut engine = Engine::new(EngineConfig::default());
        assert!(engine.load_str(source).is_err(), "{source}");
        assert_eq!(call(&mut engine, "(f 7)"), expected, "{source}");
    }
}

#[test]
fn existing_definitions_keep_their_namespace_after_rejected_replacements() {
    for (original, replacements, expected) in [
        (
            "(deffunction f (?x) (+ ?x 100))",
            "(deffunction f (?x) (missing ?x))
             (defmethod f ((?x INTEGER)) (+ ?x 200))",
            "107\n",
        ),
        (
            "(defmethod f ((?x INTEGER)) (+ ?x 200))",
            "(defmethod f ((?x SYMBOL)) (missing ?x))
             (deffunction f (?x) (+ ?x 100))",
            "207\n",
        ),
    ] {
        let mut engine = Engine::new(EngineConfig::default());
        engine.load_str(original).unwrap();
        assert!(engine.load_str(replacements).is_err());
        assert_eq!(call(&mut engine, "(f 7)"), expected);
    }
}

#[test]
fn callers_remain_valid_when_a_rejected_function_is_replaced_by_a_method() {
    let mut engine = Engine::new(EngineConfig::default());
    assert!(engine
        .load_str(
            "(deffunction top (?x) (f ?x))
             (deffunction f (?x) (missing ?x))
             (defmethod f ((?x INTEGER)) (+ ?x 200))
             (deffunction independent () 9)"
        )
        .is_err());
    assert_eq!(call(&mut engine, "(+ (top 7) (independent))"), "216\n");
}

#[test]
fn invalid_methods_do_not_reserve_explicit_or_automatic_indexes() {
    let mut engine = Engine::new(EngineConfig::default());
    assert!(engine
        .load_str(
            "(defmethod f 1 ((?x INTEGER)) (missing ?x))
             (defmethod f 1 ((?x INTEGER)) (+ ?x 200))"
        )
        .is_err());
    assert_eq!(call(&mut engine, "(f 7)"), "207\n");

    let mut engine = Engine::new(EngineConfig::default());
    assert!(engine
        .load_str(
            "(defmethod f ((?x SYMBOL)) (missing ?x))
             (defmethod f ((?x INTEGER)) (+ ?x 200))"
        )
        .is_err());
    // Ferric's existing duplicate-index rejection proves the surviving method
    // received index 1; the rejected automatic method did not consume it.
    assert!(engine
        .load_str("(defmethod f 1 ((?x SYMBOL)) 300)")
        .is_err());
    assert_eq!(call(&mut engine, "(f 7)"), "207\n");
}

#[test]
fn rejected_kind_does_not_grant_kind_specific_import_visibility() {
    let mut engine = Engine::new(EngineConfig::default());
    assert!(engine
        .load_str(
            "(defmodule A (export ?ALL))
             (deffunction f (?x) (missing ?x))
             (defmethod f ((?x INTEGER)) (+ ?x 200))
             (defmodule B (import A deffunction ?ALL))
             (deffunction rejected (?x) (f ?x))
             (defmodule C (import A defgeneric ?ALL))
             (deffunction accepted (?x) (f ?x))"
        )
        .is_err());
    assert_eq!(call(&mut engine, "(accepted 7)"), "207\n");
    engine
        .load_str("(defmodule B (import A deffunction ?ALL))")
        .unwrap();
    assert!(engine.load_str("(defrule bad => (rejected 7))").is_err());
}

#[cfg(feature = "serde")]
#[test]
fn snapshots_keep_only_surviving_callable_definitions_after_load_errors() {
    use ferric_rules_runtime::SerializationFormat;
    let mut engine = Engine::new(EngineConfig::default());
    assert!(engine
        .load_str(
            "(deffunction f (?x) (missing ?x))
             (defmethod f ((?x INTEGER)) (+ ?x 200))
             (deffunction f (?x) (+ ?x 100))"
        )
        .is_err());
    for &format in SerializationFormat::ALL {
        let bytes = engine.serialize(format).unwrap();
        let mut restored = Engine::deserialize(&bytes, format).unwrap();
        assert_eq!(call(&mut restored, "(f 7)"), "207\n");
    }
}

#[test]
fn selected_kind_cascades_settle_callees_before_forward_callers() {
    let definitions = [
        "(defmodule C (export ?ALL))
         (defmethod h () TRUE)
         (deffunction h () FALSE)",
        "(defmodule B (import C deffunction ?ALL) (export ?ALL))
         (defmethod g () (h))
         (deffunction g () TRUE)",
        "(defmodule A (import B deffunction ?ALL))
         (deffunction f () (g))",
    ];
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        let mut engine = Engine::new(EngineConfig::default());
        let mut source = String::from(
            "(defmodule C (export ?ALL))
             (defmodule B (import C deffunction ?ALL) (export ?ALL))
             (defmodule A (import B deffunction ?ALL))\n",
        );
        for index in order {
            source.push_str(definitions[index]);
            source.push('\n');
        }
        assert!(engine.load_str(&source).is_err());
        engine
            .load_str("(defmodule A (import B deffunction ?ALL))")
            .unwrap();
        assert_eq!(call(&mut engine, "(f)"), "TRUE\n", "order {order:?}");
    }
}

#[test]
fn callers_survive_when_a_failed_kind_dependency_cycle_recovers() {
    let definitions = [
        "(defmodule B (import D deffunction ?ALL) (export ?ALL))
         (defmethod g () (j))
         (deffunction g () TRUE)",
        "(defmodule D (import B deffunction ?ALL) (export ?ALL))
         (defmethod j () (g))
         (deffunction j () TRUE)",
        "(defmodule A (import B deffunction ?ALL))
         (deffunction f () (g))",
    ];
    for order in [[0, 1, 2], [2, 1, 0]] {
        let mut engine = Engine::new(EngineConfig::default());
        let mut source = String::from(
            "(defmodule B (export ?ALL))
             (defmodule D (import B deffunction ?ALL) (export ?ALL))
             (defmodule B (import D deffunction ?ALL) (export ?ALL))
             (defmodule A (import B deffunction ?ALL))\n",
        );
        for index in order {
            source.push_str(definitions[index]);
            source.push('\n');
        }
        assert!(engine.load_str(&source).is_err());
        engine
            .load_str("(defmodule A (import B deffunction ?ALL))")
            .unwrap();
        assert_eq!(call(&mut engine, "(f)"), "TRUE\n", "order {order:?}");
    }
}
