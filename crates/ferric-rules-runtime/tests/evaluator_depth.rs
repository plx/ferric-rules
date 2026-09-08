use std::fmt::Write;

use ferric_rules_runtime::{Engine, EngineConfig, HaltReason, RunLimit};

fn recursive_engine(source: &str, requested: usize) -> Engine {
    let mut config = EngineConfig::default();
    config.max_call_depth = requested;
    Engine::with_rules_config(source, config).unwrap()
}

#[test]
fn bounded_native_evaluation_child() {
    if std::env::var_os("FERRIC_DEPTH_CHILD").is_none() {
        return;
    }
    // Release embeddings commonly have 512 KiB worker stacks; unoptimized
    // builds need the ordinary Rust 2 MiB thread stack for these runtime paths.
    let stack = if cfg!(debug_assertions) {
        2 * 1024 * 1024
    } else {
        512 * 1024
    };
    std::thread::Builder::new().stack_size(stack).spawn(|| {
        for source in [
            "(deffunction recurse () (recurse)) (defrule run => (recurse))",
            "(deffunction recurse (?x) (if (> ?x 0) then (recurse (- ?x 1)) else 0)) (defrule run => (recurse 100000))",
            "(defgeneric recurse) (defmethod recurse ((?x INTEGER)) (recurse ?x)) (defrule run => (recurse 1))",
            "(deffunction recurse () (if TRUE then (if TRUE then (if TRUE then (if TRUE then (recurse)))))) (defrule run => (recurse))",
        ] {
            let mut engine = recursive_engine(source, 100_000);
            assert_eq!(engine.max_call_depth(), 100_000);
            assert_eq!(engine.effective_max_call_depth(), 32);
            assert_eq!(engine.run(RunLimit::Unlimited).unwrap().halt_reason, HaltReason::ActionError);
            assert!(engine.action_diagnostics().iter().any(|error| error.to_string().contains("limit exceeded")), "{:?}", engine.action_diagnostics());
            engine.clear();
            engine.load_str("(defrule recovered => (assert (done)))").unwrap();
            assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
            assert_eq!(engine.find_facts("done").unwrap().len(), 1);
        }
        let mut chain = "(defgeneric chain)".to_owned();
        for index in 1..35 {
            write!(chain, "(defmethod chain {index} () (call-next-method))").unwrap();
        }
        chain.push_str("(defmethod chain 35 () 42) (defrule run => (chain))");
        let mut engine = recursive_engine(&chain, 100_000);
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().halt_reason, HaltReason::ActionError);
        assert!(engine.action_diagnostics()[0].to_string().contains("call-next-method"));

        // Translation and cached parser-body cloning must reject before their
        // own recursive traversal, even when user-call depth is only one.
        let mut body = "(recurse)".to_owned();
        for _ in 0..48 { body = format!("(if TRUE then {body})"); }
        let mut engine = Engine::with_rules("(deffunction recurse () 7) (defrule run => (assert (value (recurse))))").unwrap();
        let errors = engine.load_str(&format!("(deffunction recurse () {body})")).unwrap_err();
        assert!(errors.iter().any(|error| error.to_string().contains("limit 16")));
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
        assert_eq!(engine.find_facts("value").unwrap().len(), 1);
    }).unwrap().join().unwrap();
}

#[test]
fn evaluator_limits_prevent_native_thread_failure() {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "bounded_native_evaluation_child", "--nocapture"])
        .env("FERRIC_DEPTH_CHILD", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "child status {}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn configured_and_effective_depth_are_distinct() {
    for requested in [0, 1, 8, 32, 64, usize::MAX] {
        let engine = recursive_engine("", requested);
        assert_eq!(engine.max_call_depth(), requested);
        assert_eq!(engine.effective_max_call_depth(), requested.min(32));
    }
}

#[cfg(feature = "serde")]
#[test]
fn snapshot_preserves_requested_depth_and_reapplies_effective_ceiling() {
    use ferric_rules_runtime::SerializationFormat;
    let engine = recursive_engine(
        "(deffunction recurse () (recurse)) (defrule run => (recurse))",
        100_000,
    );
    let bytes = engine.serialize(SerializationFormat::RECOMMENDED).unwrap();
    let mut restored = Engine::deserialize(&bytes, SerializationFormat::RECOMMENDED).unwrap();
    assert_eq!(restored.max_call_depth(), 100_000);
    assert_eq!(restored.effective_max_call_depth(), 32);
    assert_eq!(
        restored.run(RunLimit::Unlimited).unwrap().halt_reason,
        HaltReason::ActionError
    );
}

#[test]
fn excessive_callable_bodies_reject_before_registry_changes() {
    let mut body = "7".to_owned();
    for _ in 0..20 {
        body = format!("(if TRUE then {body})");
    }
    let mut engine = Engine::with_rules(
        "(deffunction keep () 42) (defgeneric method) (defmethod method 1 () 7)",
    )
    .unwrap();
    for source in [
        format!("(deffunction keep () {body})"),
        format!("(defmethod method 1 () {body})"),
        format!("(defmethod fresh () {body})"),
    ] {
        let errors = engine.load_str(&source).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.to_string().contains("limit 16")),
            "{errors:?}"
        );
    }
    // A failed implicit generic registration must not reserve its name.
    engine.load_str(r#"(deffunction fresh () 9) (defrule check => (printout t (keep) ":" (method) ":" (fresh)))"#).unwrap();
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
    assert_eq!(engine.get_output("t").unwrap(), Some("42:7:9"));
}
