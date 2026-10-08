//! Shared runtime formatting and ordered shell output delivery.

use ferric_rules_runtime::{Engine, STANDARD_CHANNELS};

pub(crate) fn print_output(engine: &mut Engine) {
    for (channel, output) in engine.drain_output_events() {
        if STANDARD_CHANNELS.contains(&channel.as_str()) {
            print!("{output}");
        }
    }
}

pub(crate) fn print_facts(engine: &Engine) {
    let facts = engine.fact_listing();
    for (index, fact) in &facts {
        match engine.format_fact(fact) {
            Ok(text) => println!("f-{index:<5} {text}"),
            Err(error) => eprintln!("Error: {error}"),
        }
    }
    // Like CLIPS, an empty listing prints no tally.
    if !facts.is_empty() {
        println!(
            "For a total of {} fact{}.",
            facts.len(),
            if facts.len() == 1 { "" } else { "s" }
        );
    }
}
