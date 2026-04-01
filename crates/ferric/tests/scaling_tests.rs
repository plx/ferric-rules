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

use ferric::runtime::{Engine, EngineConfig, RunLimit};

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
