//! Output formatting for the REPL: values, facts, errors.

use std::io::Write;

use ferric_rules_core::{Fact, Value};
use ferric_rules_runtime::Engine;

/// Format a [`Value`] for display in the REPL.
pub(crate) fn format_value(value: &Value, engine: &Engine) -> String {
    match value {
        Value::Symbol(sym) => display_symbol(engine, *sym),
        Value::InstanceName(name) => format!("[{}]", display_symbol(engine, name.as_symbol())),
        Value::String(s) => format!("\"{}\"", display_bytes(s.as_bytes())),
        Value::Integer(i) => i.to_string(),
        Value::Float(f) => {
            if f.fract() == 0.0 {
                format!("{f:.1}")
            } else {
                f.to_string()
            }
        }
        Value::Multifield(mf) => {
            let items: Vec<String> = mf.iter().map(|v| format_value(v, engine)).collect();
            format!("({})", items.join(" "))
        }
        Value::ExternalAddress(ea) => {
            format!("<External-{}>", ea.type_id.0)
        }
        Value::Void => String::new(),
    }
}

// Interactive inspection escapes invalid bytes explicitly. Captured rule output
// uses write_all below, so this diagnostic representation never changes data.
fn display_bytes(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => bytes.escape_ascii().to_string(),
    }
}

fn display_symbol(engine: &Engine, symbol: ferric_rules_core::Symbol) -> String {
    engine
        .resolve_core_symbol_bytes(symbol)
        .map_or_else(|| "<unknown>".to_string(), display_bytes)
}

/// Print everything written to the `"t"` output channel, then clear it.
pub(crate) fn print_output(engine: &mut Engine) {
    if let Some(output) = engine.get_output_bytes("t") {
        if let Err(error) = std::io::stdout().lock().write_all(output) {
            eprintln!("Error writing output: {error}");
            return;
        }
    }
    engine.clear_output_channel("t");
}

/// List all facts in working memory.
pub(crate) fn print_facts(engine: &Engine) {
    match engine.facts() {
        Ok(iter) => {
            let mut count = 0usize;
            for (id, fact) in iter {
                count += 1;
                let id_num = { id.as_raw() };
                match fact {
                    Fact::Ordered(o) => {
                        let relation = display_symbol(engine, o.relation);
                        print!("f-{id_num:<5}  ({relation}");
                        for field in &o.fields {
                            print!(" {}", format_value(field, engine));
                        }
                        println!(")");
                    }
                    Fact::Template(t) => {
                        print!("f-{id_num:<5}  (template-fact");
                        for slot in t.slots.iter() {
                            print!(" {}", format_value(slot, engine));
                        }
                        println!(")");
                    }
                }
            }
            println!("For a total of {count} facts.");
        }
        Err(err) => eprintln!("Error: {err}"),
    }
}

/// Format a load error with a category prefix for clearer diagnostics.
pub(crate) fn format_load_error(err: &ferric_rules_runtime::LoadError) -> String {
    use ferric_rules_runtime::LoadError;
    match err {
        LoadError::Parse(pe) => format!("[PARSE] {pe}"),
        LoadError::Interpret(ie) => format!("[INTERPRET] {ie}"),
        LoadError::Compile(msg) => format!("[COMPILE] {msg}"),
        LoadError::Validation(errs) => {
            let msgs: Vec<String> = errs.iter().map(ToString::to_string).collect();
            format!("[VALIDATION] {}", msgs.join("\n  "))
        }
        other => format!("{other}"),
    }
}
