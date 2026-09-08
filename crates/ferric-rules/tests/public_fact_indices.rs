//! Issue #329: `fact-index` reports public assertion indices for query addresses.
//!
//! Fixture goldens were verified with CLIPS 6.30 (3/17/15), Docker image
//! `sha256:b4b99ba2f08102f9c18743c6adaecb5ab82789ddfa8628693cabfeced461a929`,
//! using `docker run --rm -i IMAGE -f2 /dev/stdin`. Ordinary fixtures append
//! `(reset) (run) (exit)`. The lifecycle fixture is split at `RESUME RULE`: run
//! the first part after reset, retract item10 via a CLIPS query, assert item40,
//! load the second part, and run. The global-initializer fixture is split at
//! `INITIALIZE AFTER CAPTURE`: load/reset/run the first part, then load/run the
//! second. Host-lifecycle CLIPS operations are documented beside its helper.
//!
//! These tests use query-bound addresses; pattern-bound address evaluation is
//! the separate issue #328. Host handles are reacquired after deserialization.

use ferric_rules::core::{Fact, Value};
use ferric_rules::runtime::{Engine, EngineConfig, FactHandle, HaltReason, RunLimit};

struct Fixture {
    name: &'static str,
    source: &'static str,
    output: &'static str,
}

macro_rules! fixture {
    ($name:literal) => {
        Fixture {
            name: $name,
            source: include_str!(concat!("fixtures/queries/public_index_", $name, ".clp")),
            output: include_str!(concat!("fixtures/queries/public_index_", $name, ".out")),
        }
    };
}

#[cfg(feature = "serde")]
const ORDINARY: &[Fixture] = &[
    fixture!("original"),
    fixture!("later"),
    fixture!("duplicates"),
    fixture!("nested"),
];

fn load(engine: &mut Engine, source: &str) {
    engine.load_str(source).unwrap_or_else(|errors| {
        panic!("source failed to load: {errors:?}\n{source}");
    });
}

fn run(engine: &mut Engine) {
    let result = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(result.rules_fired, 1);
    assert!(
        engine.action_diagnostics().is_empty(),
        "{:?}",
        engine.action_diagnostics()
    );
}

fn output(engine: &Engine) -> &str {
    engine.get_output("t").unwrap_or("")
}

fn pending(fixture: &Fixture) -> Engine {
    let mut engine = Engine::new(EngineConfig::utf8());
    engine
        .load_str(fixture.source)
        .unwrap_or_else(|errors| panic!("{} failed to load: {errors:?}", fixture.name));
    engine.reset().unwrap();
    engine
}

fn assert_template(engine: &mut Engine, value: i64) -> FactHandle {
    engine.assert_template("item", &["value"], [value]).unwrap()
}

fn item_handle(engine: &Engine, value: i64) -> FactHandle {
    engine.facts().unwrap().find_map(|(handle, fact)| match fact {
        Fact::Template(template)
            if matches!(template.slots.first(), Some(Value::Integer(actual)) if *actual == value) => Some(handle),
        _ => None,
    }).unwrap_or_else(|| panic!("missing item {value}"))
}

#[test]
fn original_query_address_has_public_index_one() {
    let fixture = fixture!("original");
    let mut engine = pending(&fixture);
    run(&mut engine);
    assert_eq!(output(&engine), fixture.output);
}

#[test]
fn later_indices_count_assertions_of_other_relations() {
    let fixture = fixture!("later");
    let mut engine = pending(&fixture);
    run(&mut engine);
    assert_eq!(output(&engine), fixture.output);
}

#[test]
fn rejected_duplicates_consume_no_index_and_enabled_duplicates_do() {
    let fixture = fixture!("duplicates");
    let mut engine = pending(&fixture);
    run(&mut engine);
    assert_eq!(output(&engine), fixture.output);
    assert_eq!(engine.fact_count(), 4);
    assert!(!engine.fact_duplication());
}

#[test]
fn query_addresses_keep_their_index_in_callable_and_loop_contexts() {
    let fixture = fixture!("nested");
    let mut engine = pending(&fixture);
    run(&mut engine);
    assert_eq!(output(&engine), fixture.output);
}

fn capture_and_retract() -> Engine {
    let fixture = fixture!("lifecycle");
    let (capture, _) = fixture.source.split_once(";; RESUME RULE\n").unwrap();
    let mut engine = Engine::new(EngineConfig::utf8());
    load(&mut engine, capture);
    engine.reset().unwrap();
    run(&mut engine);
    assert_eq!(output(&engine), "saved:1\n");
    engine.retract(item_handle(&engine, 10)).unwrap();
    engine
}

fn resume_after_retraction(engine: &mut Engine) {
    let fixture = fixture!("lifecycle");
    let (_, resumed) = fixture.source.split_once(";; RESUME RULE\n").unwrap();
    assert_template(engine, 40);
    load(engine, resumed);
    run(engine);
    assert_eq!(output(engine), fixture.output);
    assert_eq!(engine.fact_count(), 3);

    // Confirm this scenario actually exercises reuse of an arena slot, while
    // the saved address retains its old generation. The encoding itself is
    // unchanged by #329; the public result must be -1 for that stale address.
    let Some(Value::Integer(old)) = engine.get_global("saved") else {
        panic!("missing saved query address");
    };
    let Some(Value::Integer(new)) = engine.get_global("replacement") else {
        panic!("missing replacement query address");
    };
    assert_eq!(old.to_le_bytes()[..4], new.to_le_bytes()[..4]);
    assert_ne!(old, new);
}

#[test]
fn retraction_leaves_index_holes_and_slot_reuse_keeps_old_addresses_stale() {
    resume_after_retraction(&mut capture_and_retract());
}

fn preload(engine: &mut Engine, user_initial_fact: bool, count: i64) {
    if user_initial_fact {
        engine.assert_ordered("initial-fact", ()).unwrap();
    } else {
        engine.assert_ordered("pre", 10_i64).unwrap();
    }
    if count == 2 {
        engine.assert_ordered("pre", 20_i64).unwrap();
    }
}

fn finish_host_lifecycle(mut engine: Engine, user_initial_fact: bool) {
    // Equivalent reference sequence for ordinary host facts:
    // clear; assert(pre10); assert(pre20); load fixture; assert(item30); run;
    // reset; assert(item30); run;
    // clear; assert(pre10); load fixture; assert(item30); run.
    // The host can additionally create a zero-field user `initial-fact` before
    // any source is loaded. CLIPS already has its protected initial fact at
    // startup; this embedding-only state follows the same user-index policy.
    let fixture = fixture!("host_lifecycle");
    load(&mut engine, fixture.source);
    assert_eq!(
        engine.fact_count(),
        2,
        "source load must retain both user facts"
    );
    assert_template(&mut engine, 30);
    run(&mut engine);
    let mut observed = output(&engine).to_owned();
    assert_eq!(engine.fact_count(), 3);

    engine.reset().unwrap();
    assert_template(&mut engine, 30);
    run(&mut engine);
    observed.push_str(output(&engine));
    assert_eq!(engine.fact_count(), 1);

    engine.clear();
    preload(&mut engine, user_initial_fact, 1);
    load(&mut engine, fixture.source);
    assert_eq!(engine.fact_count(), 1);
    assert_template(&mut engine, 30);
    run(&mut engine);
    observed.push_str(output(&engine));
    assert_eq!(engine.fact_count(), 2);
    assert_eq!(observed, fixture.output);
}

#[test]
fn host_assertions_before_source_loading_and_after_clear_count_from_one() {
    let mut engine = Engine::new(EngineConfig::utf8());
    preload(&mut engine, false, 2);
    finish_host_lifecycle(engine, false);
}

#[test]
fn user_initial_fact_is_retained_and_counted_when_protected_initial_is_loaded() {
    let mut engine = Engine::new(EngineConfig::utf8());
    preload(&mut engine, true, 2);
    finish_host_lifecycle(engine, true);
}

fn pending_global_initializer() -> Engine {
    let fixture = fixture!("global_initializer");
    let (capture, _) = fixture
        .source
        .split_once(";; INITIALIZE AFTER CAPTURE\n")
        .unwrap();
    let mut engine = Engine::new(EngineConfig::utf8());
    load(&mut engine, capture);
    engine.reset().unwrap();
    run(&mut engine);
    assert_eq!(output(&engine), "");
    engine
}

fn initialize_from_saved_address(engine: &mut Engine) {
    let fixture = fixture!("global_initializer");
    let (_, initializer) = fixture
        .source
        .split_once(";; INITIALIZE AFTER CAPTURE\n")
        .unwrap();
    load(engine, initializer);
    assert!(matches!(
        engine.get_global("index"),
        Some(Value::Integer(1))
    ));
    run(engine);
    assert_eq!(output(engine), fixture.output);
}

#[test]
fn later_global_initializer_can_read_a_saved_query_address() {
    initialize_from_saved_address(&mut pending_global_initializer());
}

#[cfg(feature = "serde")]
#[test]
fn pending_query_indices_restore_in_all_serialization_formats() {
    use ferric_rules::runtime::SerializationFormat;
    for fixture in ORDINARY {
        let engine = pending(fixture);
        for &format in SerializationFormat::ALL {
            let mut restored =
                Engine::deserialize(&engine.serialize(format).unwrap(), format).unwrap();
            run(&mut restored);
            assert_eq!(
                output(&restored),
                fixture.output,
                "{} / {format:?}",
                fixture.name
            );
        }
    }
}

#[cfg(feature = "serde")]
#[test]
fn survivors_stale_address_and_next_index_resume_in_all_formats() {
    use ferric_rules::runtime::SerializationFormat;
    let engine = capture_and_retract();
    for &format in SerializationFormat::ALL {
        let mut restored = Engine::deserialize(&engine.serialize(format).unwrap(), format).unwrap();
        resume_after_retraction(&mut restored);
    }
}

#[cfg(feature = "serde")]
#[test]
fn host_preload_and_late_initial_states_restore_in_all_formats() {
    use ferric_rules::runtime::SerializationFormat;
    for user_initial_fact in [false, true] {
        for late_initial_loaded in [false, true] {
            let mut engine = Engine::new(EngineConfig::utf8());
            preload(&mut engine, user_initial_fact, 2);
            if late_initial_loaded {
                // A template declaration inserts the protected initial fact,
                // preserving the two earlier user assertions and their count.
                load(&mut engine, "(deftemplate placeholder (slot value))");
            }
            for &format in SerializationFormat::ALL {
                let restored =
                    Engine::deserialize(&engine.serialize(format).unwrap(), format).unwrap();
                finish_host_lifecycle(restored, user_initial_fact);
            }
        }
    }
}

#[cfg(feature = "serde")]
#[test]
fn delayed_global_initializer_reads_restored_address_in_all_formats() {
    use ferric_rules::runtime::SerializationFormat;
    let engine = pending_global_initializer();
    for &format in SerializationFormat::ALL {
        let mut restored = Engine::deserialize(&engine.serialize(format).unwrap(), format).unwrap();
        initialize_from_saved_address(&mut restored);
    }
}
