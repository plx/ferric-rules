//! Random streams belong to one engine and survive resets and clears without reseeding.
use ferric_rules_runtime::{Engine, EngineConfig, Value};

fn seeded_engine(seed: i64) -> Engine {
    let mut engine = Engine::new(EngineConfig::default());
    engine
        .load_str(&format!("(assert (seeded (progn (seed {seed}) ready)))"))
        .unwrap();
    engine
}

fn draw(engine: &mut Engine) -> String {
    engine
        .load_str("(assert (sample (progn (printout t (random) crlf) ready)))")
        .unwrap();
    let output = engine.get_output("t").unwrap().to_owned();
    engine.clear_output_channel("t");
    output
}

#[test]
fn resetting_and_clearing_preserve_each_engines_independent_stream() {
    let mut first = seeded_engine(42);
    let mut second = seeded_engine(42);
    assert_eq!(draw(&mut first), "71876166\n");
    assert_eq!(draw(&mut first), "708592740\n");
    assert_eq!(draw(&mut second), "71876166\n");
    first.reset().unwrap();
    assert_eq!(draw(&mut first), "1483128881\n");
    assert_eq!(draw(&mut second), "708592740\n");
    second.clear();
    assert_eq!(draw(&mut second), "1483128881\n");
}

#[test]
fn full_integer_range_is_defined_and_consumes_one_draw() {
    // This is a Ferric guarantee, not a CLIPS oracle: the reference computes
    // the inclusive width in signed long long and overflows for these bounds.
    let mut engine = Engine::new(EngineConfig::default());
    engine.eval_str("(seed 42)").unwrap();
    for (expression, expected) in [
        (
            "(random -9223372036854775808 9223372036854775807)",
            -9_223_372_036_782_899_642,
        ),
        ("(random)", 708_592_740),
        ("(random 9223372036854775807 9223372036854775807)", i64::MAX),
        ("(random)", 907_283_241),
    ] {
        let Value::Integer(actual) = engine.eval_str(expression).unwrap() else {
            panic!("random must return an INTEGER");
        };
        assert_eq!(actual, expected, "{expression}");
    }
}
