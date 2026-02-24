//! Output formatting for the REPL: values, facts, errors.

use ferric_core::{Fact, Value};
use ferric_runtime::Engine;

/// Format a [`Value`] for display in the REPL.
pub(crate) fn format_value(value: &Value, engine: &Engine) -> String {
    match value {
        Value::Symbol(sym) => engine
            .resolve_symbol(*sym)
            .unwrap_or("<unknown>")
            .to_string(),
        Value::String(s) => format!("\"{}\"", s.as_str()),
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

/// Print everything written to the `"t"` output channel, then clear it.
pub(crate) fn print_output(engine: &mut Engine) {
    if let Some(output) = engine.get_output("t") {
        if !output.is_empty() {
            print!("{output}");
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
                let id_num = {
                    use slotmap::Key as _;
                    id.data().as_ffi()
                };
                match fact {
                    Fact::Ordered(o) => {
                        let relation =
                            engine.resolve_symbol(o.relation).unwrap_or("<unknown>");
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
pub(crate) fn format_load_error(err: &ferric_runtime::LoadError) -> String {
    use ferric_runtime::LoadError;
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
