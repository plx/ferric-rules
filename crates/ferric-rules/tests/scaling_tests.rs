//! Scaling regression tests.
//!
//! Each test measures a core engine operation at two input sizes (4x apart),
//! computes the time ratio, and asserts it stays within bounds consistent with
//! the expected complexity class. This catches full complexity-class regressions
//! (e.g. O(N) → O(N²)) while tolerating normal measurement noise.
//!
//! These tests are `#[ignore]` because they need release-mode compilation for
//! meaningful timings. Run via:
//!
//! ```sh
//! just scaling-check
//! ```

use std::fmt::Write as FmtWrite;
use std::hint::black_box;
use std::time::{Duration, Instant};

use ferric_rules::runtime::{Engine, EngineConfig, RunLimit};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const WARMUP: usize = 3;
const SAMPLES: usize = 7;

/// Run `f` repeatedly, return the median duration of `SAMPLES` post-warmup runs.
fn measure_median<F: FnMut()>(mut f: F) -> Duration {
    for _ in 0..WARMUP {
        f();
    }
    let mut times = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let start = Instant::now();
        f();
        times.push(start.elapsed());
    }
    times.sort();
    times[SAMPLES / 2]
}

/// Run `setup` to prepare state, then time only `op`. Returns median of `op`.
/// Used when the operation under test is destructive and needs fresh state each time.
fn measure_op_median<S, O, T>(mut setup: S, mut op: O) -> Duration
where
    S: FnMut() -> T,
    O: FnMut(T),
{
    for _ in 0..WARMUP {
        let state = setup();
        op(state);
    }
    let mut times = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let state = setup();
        let start = Instant::now();
        op(state);
        times.push(start.elapsed());
    }
    times.sort();
    times[SAMPLES / 2]
}

/// Assert that the scaling ratio between two sizes is within `max_ratio`.
/// Always prints diagnostics (useful even when passing, to watch for drift).
fn assert_scaling(
    name: &str,
    small_n: usize,
    large_n: usize,
    t_small: Duration,
    t_large: Duration,
    max_ratio: f64,
) {
    let ratio = t_large.as_secs_f64() / t_small.as_secs_f64();
    let input_ratio = large_n / small_n;

    eprintln!(
        "[scaling] {name}: N={small_n} → {large_n} ({input_ratio}x), \
         time={t_small:.2?} → {t_large:.2?}, ratio={ratio:.2} (max={max_ratio:.1})"
    );

    assert!(
        ratio <= max_ratio,
        "SCALING REGRESSION in {name}: ratio {ratio:.2} exceeds max {max_ratio:.1} \
         (input grew {input_ratio:.0}x, time grew {ratio:.1}x — \
         suggests worse-than-expected complexity)"
    );
}

// ---------------------------------------------------------------------------
// Source generators
// ---------------------------------------------------------------------------

/// Join propagation: N items in group "a", N in group "b", joined on key.
/// Each pair matches exactly once → N rule firings.
fn generate_join_source(n: usize) -> String {
    let mut source = String::from(
        "(deftemplate item (slot key) (slot group))\n\
         (defrule match-pairs\n    \
             (item (key ?k) (group a))\n    \
             (item (key ?k) (group b))\n    \
             =>\n    \
             (assert (matched ?k)))\n\n\
         (deffacts preload\n",
    );
    for i in 0..n {
        writeln!(source, "    (item (key k{i}) (group a))").unwrap();
        writeln!(source, "    (item (key k{i}) (group b))").unwrap();
    }
    source.push_str(")\n");
    source
}

/// Simple engine run: N ordered facts, one rule fires per fact.
fn generate_run_source(n: usize) -> String {
    let mut source =
        String::from("(defrule process (item ?x) => (assert (done ?x)))\n(deffacts items\n");
    for i in 0..n {
        writeln!(source, "    (item f{i})").unwrap();
    }
    source.push_str(")\n");
    source
}

/// Retraction cascade: 1 base fact joined with N partner facts → N tokens.
/// Retracting the base fact cascades through all N tokens.
fn generate_cascade_source(n: usize) -> String {
    let mut source = String::from(
        "(defrule cascade-match\n    \
             (base anchor)\n    \
             (partner anchor ?id)\n    \
             =>\n    \
             (assert (matched ?id)))\n\n\
         (deffacts data\n    \
             (base anchor)\n",
    );
    for i in 0..n {
        writeln!(source, "    (partner anchor p{i})").unwrap();
    }
    source.push_str(")\n");
    source
}

/// Churn lifecycle: N items go through assert(pending) → modify(done) → retract.
/// Total: 2N+1 rule firings, ~3N Rete operations.
fn generate_churn_source(n: usize) -> String {
    let mut source = String::from(
        "(deftemplate item (slot id) (slot status (default pending)))\n\
         (deftemplate phase (slot name))\n\n\
         (deffacts initial\n    \
             (phase (name run))\n",
    );
    for i in 0..n {
        writeln!(source, "    (item (id {i}) (status pending))").unwrap();
    }
    source.push_str(
        ")\n\n\
         (defrule process-item\n    \
             (declare (salience 10))\n    \
             (phase (name run))\n    \
             ?item <- (item (id ?id) (status pending))\n    \
             =>\n    \
             (modify ?item (status done)))\n\n\
         (defrule cleanup-item\n    \
             (declare (salience 5))\n    \
             (phase (name run))\n    \
             ?item <- (item (id ?id) (status done))\n    \
             =>\n    \
             (retract ?item))\n\n\
         (defrule all-done\n    \
             (declare (salience -10))\n    \
             (phase (name run))\n    \
             (not (item))\n    \
             =>\n    \
             (printout t \"done\" crlf))\n",
    );
    source
}

/// Alpha fanout: R rules each matching a different constant on the `type` slot.
/// Fixed fact count (100 events cycling through types).
fn generate_alpha_fanout_source(n_rules: usize) -> String {
    let n_facts = 100;
    let mut source = String::from("(deftemplate event (slot type) (slot value))\n\n");
    for i in 0..n_rules {
        writeln!(
            source,
            "(defrule handle-{i}\n    (event (type t{i}) (value ?v))\n    =>\n    (assert (handled-{i} ?v)))\n"
        )
        .unwrap();
    }
    source.push_str("(deffacts events\n");
    for i in 0..n_facts {
        let type_idx = i % n_rules;
        writeln!(source, "    (event (type t{type_idx}) (value v{i}))").unwrap();
    }
    source.push_str(")\n");
    source
}

// ---------------------------------------------------------------------------
// Scaling tests
// ---------------------------------------------------------------------------

/// Join propagation: batch of N join-matches should scale as O(N).
/// Catches: broken indexing degrading per-match cost from O(1) to O(N).
#[test]
#[ignore = "requires release mode; run via just scaling-check"]
fn test_scaling_join_propagation() {
    let (small, large) = (200, 800);
    let src_s = generate_join_source(small);
    let src_l = generate_join_source(large);

    let t_small = measure_median(|| {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine.load_str(&src_s).unwrap();
        engine.reset().unwrap();
        black_box(engine.run(RunLimit::Unlimited).unwrap());
    });

    let t_large = measure_median(|| {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine.load_str(&src_l).unwrap();
        engine.reset().unwrap();
        black_box(engine.run(RunLimit::Unlimited).unwrap());
    });

    assert_scaling("join_propagation", small, large, t_small, t_large, 8.0);
}

/// Engine run loop: N facts, N simple rule firings, no joins.
/// Catches: quadratic behavior in the fire loop or agenda scanning.
#[test]
#[ignore = "requires release mode; run via just scaling-check"]
fn test_scaling_engine_run() {
    let (small, large) = (500, 2000);
    let src_s = generate_run_source(small);
    let src_l = generate_run_source(large);

    let t_small = measure_median(|| {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine.load_str(&src_s).unwrap();
        engine.reset().unwrap();
        black_box(engine.run(RunLimit::Unlimited).unwrap());
    });

    let t_large = measure_median(|| {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine.load_str(&src_l).unwrap();
        engine.reset().unwrap();
        black_box(engine.run(RunLimit::Unlimited).unwrap());
    });

    assert_scaling("engine_run", small, large, t_small, t_large, 8.0);
}

/// Retraction cascade: retracting one base fact cascades through N tokens.
/// Catches: quadratic cascade cleanup in `TokenStore::remove_cascade`.
#[test]
#[ignore = "requires release mode; run via just scaling-check"]
fn test_scaling_retraction_cascade() {
    let (small, large) = (200, 800);
    let src_s = generate_cascade_source(small);
    let src_l = generate_cascade_source(large);

    // We measure ONLY the retract() call. Setup (load+reset+run) is excluded
    // from timing via measure_op_median, which is critical: setup is O(N) and
    // would mask a retraction regression if included.
    let setup_engine = |src: &str| {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine.load_str(src).unwrap();
        engine.reset().unwrap();
        engine.run(RunLimit::Unlimited).unwrap();
        let base_id = engine.find_facts("base").unwrap()[0].0;
        (engine, base_id)
    };

    let t_small = measure_op_median(
        || setup_engine(&src_s),
        |(mut engine, base_id)| {
            engine.retract(base_id).unwrap();
            black_box(());
        },
    );

    let t_large = measure_op_median(
        || setup_engine(&src_l),
        |(mut engine, base_id)| {
            engine.retract(base_id).unwrap();
            black_box(());
        },
    );

    assert_scaling("retraction_cascade", small, large, t_small, t_large, 8.0);
}

/// Churn lifecycle: N items through assert → modify → retract cycle.
/// Catches: quadratic cost in modify/retract lifecycle.
#[test]
#[ignore = "requires release mode; run via just scaling-check"]
fn test_scaling_churn_lifecycle() {
    let (small, large) = (250, 1000);
    let src_s = generate_churn_source(small);
    let src_l = generate_churn_source(large);

    let t_small = measure_median(|| {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine.load_str(&src_s).unwrap();
        engine.reset().unwrap();
        black_box(engine.run(RunLimit::Unlimited).unwrap());
    });

    let t_large = measure_median(|| {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine.load_str(&src_l).unwrap();
        engine.reset().unwrap();
        black_box(engine.run(RunLimit::Unlimited).unwrap());
    });

    assert_scaling("churn_lifecycle", small, large, t_small, t_large, 8.0);
}

/// Alpha fanout: R rules sharing a template, fixed fact count.
/// Catches: quadratic alpha routing when many rules match one template.
#[test]
#[ignore = "requires release mode; run via just scaling-check"]
fn test_scaling_alpha_fanout() {
    let (small, large) = (50, 200);
    let src_s = generate_alpha_fanout_source(small);
    let src_l = generate_alpha_fanout_source(large);

    let t_small = measure_median(|| {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine.load_str(&src_s).unwrap();
        engine.reset().unwrap();
        black_box(engine.run(RunLimit::Unlimited).unwrap());
    });

    let t_large = measure_median(|| {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine.load_str(&src_l).unwrap();
        engine.reset().unwrap();
        black_box(engine.run(RunLimit::Unlimited).unwrap());
    });

    assert_scaling("alpha_fanout", small, large, t_small, t_large, 8.0);
}

/// Incoming existential support must not scan every unrelated parent token.
/// Keep supports per sensor fixed, so a full parent scan is quadratic in N.
#[test]
#[ignore = "requires release mode; run via just scaling-check"]
fn test_scaling_exists_support_assertion() {
    fn measure(n: usize) -> Duration {
        measure_op_median(
            || {
                let mut engine = Engine::with_rules(
                    "(defrule ready (sensor ?id) (exists (reading ?id ?sample)) =>)",
                )
                .unwrap();
                for sensor in 0..n {
                    engine
                        .assert_ordered("sensor", i64::try_from(sensor).unwrap())
                        .unwrap();
                }
                engine
            },
            |mut engine| {
                for sensor in 0..n {
                    for sample in 0..8_i64 {
                        engine
                            .assert_ordered(
                                "reading",
                                vec![
                                    ferric_rules::core::Value::Integer(
                                        i64::try_from(sensor).unwrap(),
                                    ),
                                    ferric_rules::core::Value::Integer(sample),
                                ],
                            )
                            .unwrap();
                    }
                }
                let result = engine.run(RunLimit::Unlimited).unwrap();
                assert_eq!(result.rules_fired, n);
                assert_eq!(
                    result.halt_reason,
                    ferric_rules::runtime::HaltReason::AgendaEmpty
                );
                assert!(engine.action_diagnostics().is_empty());
                assert_eq!(engine.fact_count(), n * 9);
                black_box(engine);
            },
        )
    }

    let (small, large) = (512, 2048);
    assert_scaling(
        "exists_support_assertion",
        small,
        large,
        measure(small),
        measure(large),
        8.0,
    );
}

/// Completing an indexed NCC result must order just its matching parents.
/// Each key has two parents: sorting a batch must not scan all 2N parents.
#[test]
#[ignore = "requires release mode; run via just scaling-check"]
fn test_scaling_indexed_ncc_completion() {
    fn measure(n: usize) -> Duration {
        measure_op_median(
            || {
                let mut engine = Engine::with_rules(
                    "(defrule absent (item ?key ?copy)
                       (not (and (blocker ?key) (other ?key))) =>)",
                )
                .unwrap();
                for key in 0..n {
                    let key = i64::try_from(key).unwrap();
                    for copy in 0..2_i64 {
                        engine
                            .assert_ordered(
                                "item",
                                vec![
                                    ferric_rules::core::Value::Integer(key),
                                    ferric_rules::core::Value::Integer(copy),
                                ],
                            )
                            .unwrap();
                    }
                    engine.assert_ordered("blocker", key).unwrap();
                }
                engine
            },
            |mut engine| {
                for key in 0..n {
                    engine
                        .assert_ordered("other", i64::try_from(key).unwrap())
                        .unwrap();
                }
                let result = engine.run(RunLimit::Unlimited).unwrap();
                assert_eq!(result.rules_fired, 0);
                assert_eq!(
                    result.halt_reason,
                    ferric_rules::runtime::HaltReason::AgendaEmpty
                );
                assert!(engine.action_diagnostics().is_empty());
                assert_eq!(engine.fact_count(), n * 4);
                black_box(engine);
            },
        )
    }

    let (small, large) = (512, 2048);
    assert_scaling(
        "indexed_ncc_completion",
        small,
        large,
        measure(small),
        measure(large),
        8.0,
    );
}

/// Retracting N independent parents must not scan N unrelated negative memories.
#[test]
#[ignore = "requires release mode; run via just scaling-check"]
fn test_scaling_independent_negative_cleanup() {
    fn measure(n: usize) -> Duration {
        let mut source = String::new();
        for group in 0..n {
            writeln!(
                source,
                "(defrule r-{group} (item {group}) (not (block {group})) =>)"
            )
            .unwrap();
        }
        measure_op_median(
            || {
                let mut engine = Engine::with_rules(&source).unwrap();
                let handles: Vec<_> = (0..n)
                    .map(|group| {
                        engine
                            .assert_ordered("item", i64::try_from(group).unwrap())
                            .unwrap()
                    })
                    .collect();
                (engine, handles)
            },
            |(mut engine, handles)| {
                for handle in handles {
                    engine.retract(handle).unwrap();
                }
                let result = engine.run(RunLimit::Unlimited).unwrap();
                assert_eq!(result.rules_fired, 0);
                assert_eq!(
                    result.halt_reason,
                    ferric_rules::runtime::HaltReason::AgendaEmpty
                );
                assert!(engine.action_diagnostics().is_empty());
                assert_eq!(engine.fact_count(), 0);
                black_box(engine);
            },
        )
    }
    let (small, large) = (256, 1024);
    assert_scaling(
        "independent_negative_cleanup",
        small,
        large,
        measure(small),
        measure(large),
        8.0,
    );
}

/// Focus selection must not rescan every dormant activation for each firing.
#[test]
#[ignore = "requires release mode; run via just scaling-check"]
fn test_scaling_dormant_focus_selection() {
    fn measure(n: usize) -> Duration {
        let mut source = String::from(
            "(defmodule MAIN (export ?ALL))\n\
             (deftemplate MAIN::item (slot id))\n\
             (deffacts MAIN::seed\n",
        );
        for id in 0..n {
            writeln!(source, "(item (id {id}))").unwrap();
        }
        source.push_str(
            ")\n\
             (defmodule DORMANT (import MAIN ?ALL))\n\
             (defmodule ACTIVE (import MAIN ?ALL))\n\
             (defrule DORMANT::wait (declare (salience 100)) (MAIN::item (id ?id)) =>)\n\
             (defrule ACTIVE::work (MAIN::item (id ?id)) =>)\n\
             (defrule MAIN::start => (focus ACTIVE))\n",
        );
        measure_op_median(
            || {
                let mut engine = Engine::with_rules(&source).unwrap();
                engine.reset().unwrap();
                assert_eq!(engine.agenda_len(), 2 * n + 1);
                engine
            },
            |mut engine| {
                let result = engine.run(RunLimit::Unlimited).unwrap();
                assert_eq!(result.rules_fired, n + 1);
                assert_eq!(
                    result.halt_reason,
                    ferric_rules::runtime::HaltReason::AgendaEmpty
                );
                assert_eq!(engine.agenda_len(), n);
                assert!(engine.action_diagnostics().is_empty());
                assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 0);
                black_box(engine);
            },
        )
    }
    let (small, large) = (1024, 4096);
    assert_scaling(
        "dormant_focus_selection",
        small,
        large,
        measure(small),
        measure(large),
        8.0,
    );
}

/// A join on a scalar template slot must stay indexed when the pattern also
/// constrains a multislot: both a whole-slot capture and a positional split.
#[test]
#[ignore = "requires release mode; run via just scaling-check"]
fn test_scaling_template_multislot_join() {
    fn measure(n: usize, tags: &str) -> Duration {
        let mut source = format!(
            "(deftemplate order (slot id))\n\
             (deftemplate item (slot id) (multislot tags))\n\
             (defrule match (order (id ?id)) (item (id ?id) (tags {tags})) =>)\n\
             (deffacts seed\n"
        );
        for id in 0..n {
            writeln!(source, "(order (id {id})) (item (id {id}) (tags a b))").unwrap();
        }
        source.push_str(")\n");
        measure_op_median(
            || Engine::with_rules(&source).unwrap(),
            |mut engine| {
                engine.reset().unwrap();
                assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, n);
                black_box(engine);
            },
        )
    }
    let (small, large) = (1000, 4000);
    for tags in ["$?t", "$? b $?"] {
        assert_scaling(
            &format!("template_multislot_join ({tags})"),
            small,
            large,
            measure(small, tags),
            measure(large, tags),
            8.0,
        );
    }
}

/// Split enumeration must reject a placed constant before trying the later
/// captures: against a fact with no `x`, `$? x $? y $?` is linear in the
/// fact's length, not quadratic.
#[test]
#[ignore = "requires release mode; run via just scaling-check"]
fn test_scaling_sequence_constant_pruning() {
    fn measure(n: usize) -> Duration {
        let mut source = String::from("(defrule m (data $? x $? y $?) =>)\n(deffacts seed (data");
        for i in 0..n {
            write!(source, " f{i}").unwrap();
        }
        source.push_str("))\n");
        measure_op_median(
            || Engine::with_rules(&source).unwrap(),
            |mut engine| {
                engine.reset().unwrap();
                assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 0);
                black_box(engine);
            },
        )
    }
    let (small, large) = (2000, 8000);
    assert_scaling(
        "sequence_constant_pruning",
        small,
        large,
        measure(small),
        measure(large),
        8.0,
    );
}

/// A sequence pattern's constants filter facts once, in the alpha network:
/// a negated `$? red $?` over N items without `red` costs each of N parent
/// tokens nothing, instead of a split search per item.
#[test]
#[ignore = "requires release mode; run via just scaling-check"]
fn test_scaling_sequence_negative_admission() {
    fn measure(n: usize) -> Duration {
        let mut source = String::from(
            "(deftemplate item (slot id) (multislot tags))\n\
             (defrule none-red (go ?i) (not (item (tags $? red $?))) =>)\n\
             (deffacts seed\n",
        );
        for i in 0..n {
            writeln!(source, "(go {i}) (item (id {i}) (tags a b c d e f g h))").unwrap();
        }
        source.push_str(")\n");
        measure_op_median(
            || Engine::with_rules(&source).unwrap(),
            |mut engine| {
                engine.reset().unwrap();
                assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, n);
                black_box(engine);
            },
        )
    }
    let (small, large) = (500, 2000);
    assert_scaling(
        "sequence_negative_admission",
        small,
        large,
        measure(small),
        measure(large),
        8.0,
    );
}

/// With four fixed bound keys and unique list values, placement pruning should
/// scale with list length. Each direction must still produce exactly six pairs.
#[test]
#[ignore = "requires release mode; run via just scaling-check"]
fn test_scaling_bound_sequence_join() {
    const N_KEYS: usize = 4;
    const RULE: &str = "
        (defrule bound-sequence
            (key ?a) (key ?b) (lst $? ?a $? ?b $?)
            => (assert (hit ?a ?b)))";

    fn prepare(length: usize, list_first: bool) -> (Engine, Vec<i64>) {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine.load_str(RULE).unwrap();
        engine.reset().unwrap();
        let keys = (0..N_KEYS)
            .map(|index| i64::try_from((index + 1) * length / (N_KEYS + 1)).unwrap())
            .collect::<Vec<_>>();
        let list = (0..i64::try_from(length).unwrap()).collect::<Vec<_>>();
        let incoming = if list_first {
            engine.assert_ordered("lst", list).unwrap();
            keys
        } else {
            for key in keys {
                engine.assert_ordered("key", key).unwrap();
            }
            list
        };
        (engine, incoming)
    }

    fn complete(mut engine: Engine, incoming: Vec<i64>, list_first: bool) -> Engine {
        if list_first {
            for key in incoming {
                engine.assert_ordered("key", key).unwrap();
            }
        } else {
            engine.assert_ordered("lst", incoming).unwrap();
        }
        let result = engine.run(RunLimit::Unlimited).unwrap();
        assert_eq!(result.rules_fired, N_KEYS * (N_KEYS - 1) / 2);
        assert!(engine.action_diagnostics().is_empty());
        engine
    }

    fn measure(length: usize, list_first: bool) -> Duration {
        measure_op_median(
            || prepare(length, list_first),
            |(engine, incoming)| {
                black_box(complete(engine, incoming, list_first));
            },
        )
    }

    let (small, large) = (2_000, 8_000);
    for (arrival, list_first) in [("list_arrives", false), ("keys_arrive", true)] {
        assert_scaling(
            &format!("bound_sequence_join ({arrival})"),
            small,
            large,
            measure(small, list_first),
            measure(large, list_first),
            8.0,
        );
    }
}
