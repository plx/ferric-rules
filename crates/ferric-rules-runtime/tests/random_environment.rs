//! Random streams belong to one engine and survive resets and clears without reseeding.
use ferric_rules_runtime::{Engine, EngineConfig};

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
