//! `ferric repl` command — interactive read-eval-print loop.
//!
//! Provides line editing and history via rustyline. Supports
//! multiline input with balanced-parenthesis continuation.
//!
//! ## REPL Commands
//!
//! - `(reset)` — Reset the engine
//! - `(run)` / `(run N)` — Run rules (optionally with a step limit)
//! - `(facts)` — List all facts in working memory
//! - `(agenda)` — Show count of activations on the agenda
//! - `(clear)` — Clear the engine completely
//! - `(exit)` / `(quit)` — Exit the REPL
//! - `(load "file")` — Load a CLIPS file
//!
//! Any other input is evaluated as a CLIPS form via `engine.load_str()`.
//!
//! Exit codes:
//! - 0: Normal exit

use ferric_runtime::{Engine, EngineConfig, RunLimit};
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;

const PROMPT: &str = "CLIPS> ";
const CONTINUATION_PROMPT: &str = "... ";

/// Execute the `repl` subcommand.
pub fn execute(_args: &[String]) -> i32 {
    let mut engine = Engine::new(EngineConfig::default());

    println!("Ferric REPL v{}", env!("CARGO_PKG_VERSION"));
    println!("Type (exit) to quit.");

    let mut editor = match DefaultEditor::new() {
        Ok(e) => e,
        Err(err) => {
            eprintln!("ferric repl: failed to initialize editor: {err}");
            return 1;
        }
    };

    let mut buffer = String::new();

    loop {
        let prompt = if buffer.is_empty() {
            PROMPT
        } else {
            CONTINUATION_PROMPT
        };

        match editor.readline(prompt) {
            Ok(line) => {
                if buffer.is_empty() {
                    buffer = line;
                } else {
                    buffer.push('\n');
                    buffer.push_str(&line);
                }

                // Keep accumulating until parentheses are balanced.
                if !parens_balanced(&buffer) {
                    continue;
                }

                let input = buffer.trim().to_string();
                buffer.clear();

                if input.is_empty() {
                    continue;
                }

                // Record in history.
                let _ = editor.add_history_entry(&input);

                // Dispatch the completed form.
                if process_input(&mut engine, &input) {
                    return 0;
                }
            }
            // Ctrl-D (EOF) or Ctrl-C — exit cleanly.
            Err(ReadlineError::Eof | ReadlineError::Interrupted) => {
                println!();
                return 0;
            }
            Err(err) => {
                eprintln!("ferric repl: read error: {err}");
                return 1;
            }
        }
    }
}

/// Check whether the parentheses in `input` are balanced.
///
/// Respects string literals (ignores parens inside `"..."`) and
/// line comments (ignores everything after `;` until end-of-line).
///
/// Returns `true` when the open-paren count is less than or equal to
/// the close-paren count, meaning the input forms a complete expression.
fn parens_balanced(input: &str) -> bool {
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut in_comment = false;
    let mut prev_char = '\0';

    for ch in input.chars() {
        if in_comment {
            if ch == '\n' {
                in_comment = false;
            }
            prev_char = ch;
            continue;
        }
        if in_string {
            if ch == '"' && prev_char != '\\' {
                in_string = false;
            }
        } else {
            match ch {
                ';' => {
                    in_comment = true;
                    prev_char = ch;
                    continue;
                }
                '"' => in_string = true,
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
        }
        prev_char = ch;
    }

    depth <= 0
}

/// Process one complete CLIPS form or REPL command.
///
/// Returns `true` if the REPL should exit.
fn process_input(engine: &mut Engine, input: &str) -> bool {
    let trimmed = input.trim();

    // Simple exact-match REPL commands.
    match trimmed {
        "(exit)" | "(quit)" => return true,
        "(reset)" => {
            if let Err(err) = engine.reset() {
                eprintln!("Error: {err}");
            }
            return false;
        }
        "(facts)" => {
            print_facts(engine);
            return false;
        }
        "(agenda)" => {
            println!("For a total of {} activations.", engine.agenda_len());
            return false;
        }
        "(clear)" => {
            engine.clear();
            return false;
        }
        _ => {}
    }

    // (run) — unlimited execution.
    if trimmed == "(run)" {
        run_engine(engine, RunLimit::Unlimited);
        return false;
    }

    // (run N) — bounded execution.
    if let Some(rest) = trimmed.strip_prefix("(run ") {
        if let Some(num_str) = rest.strip_suffix(')') {
            if let Ok(n) = num_str.trim().parse::<usize>() {
                run_engine(engine, RunLimit::Count(n));
                return false;
            }
        }
    }

    // (load "path") — load a file.
    if let Some(rest) = trimmed.strip_prefix("(load ") {
        if let Some(path_expr) = rest.strip_suffix(')') {
            let path_str = path_expr.trim().trim_matches('"');
            match engine.load_file(std::path::Path::new(path_str)) {
                Ok(_) => {
                    print_output(engine);
                }
                Err(errors) => {
                    for err in &errors {
                        eprintln!("Error: {err}");
                    }
                }
            }
            return false;
        }
    }

    // General CLIPS form — evaluate via load_str.
    match engine.load_str(trimmed) {
        Ok(_) => {
            print_output(engine);
        }
        Err(errors) => {
            for err in &errors {
                eprintln!("Error: {err}");
            }
        }
    }

    false
}

/// Run the engine and print any captured output and diagnostics.
fn run_engine(engine: &mut Engine, limit: RunLimit) {
    match engine.run(limit) {
        Ok(_result) => {
            print_output(engine);
            for diag in engine.action_diagnostics() {
                eprintln!("Warning: {diag}");
            }
            engine.clear_action_diagnostics();
        }
        Err(err) => {
            eprintln!("Error: {err}");
        }
    }
}

/// Print everything written to the `"t"` output channel, then clear it.
fn print_output(engine: &Engine) {
    if let Some(output) = engine.get_output("t") {
        if !output.is_empty() {
            print!("{output}");
        }
    }
}

/// List all facts in working memory.
fn print_facts(engine: &Engine) {
    match engine.facts() {
        Ok(iter) => {
            let facts: Vec<_> = iter.collect();
            let count = facts.len();
            for (id, fact) in &facts {
                let id_num = {
                    use slotmap::Key as _;
                    id.data().as_ffi()
                };
                match fact {
                    ferric_core::Fact::Ordered(o) => {
                        let relation = engine.resolve_symbol(o.relation).unwrap_or("<unknown>");
                        print!("f-{id_num:<5}  ({relation}");
                        for field in &o.fields {
                            print!(" {}", format_value(field, engine));
                        }
                        println!(")");
                    }
                    ferric_core::Fact::Template(t) => {
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

/// Format a `Value` for display in the REPL.
fn format_value(value: &ferric_core::Value, engine: &Engine) -> String {
    match value {
        ferric_core::Value::Symbol(sym) => engine
            .resolve_symbol(*sym)
            .unwrap_or("<unknown>")
            .to_string(),
        ferric_core::Value::String(s) => format!("\"{}\"", s.as_str()),
        ferric_core::Value::Integer(i) => i.to_string(),
        ferric_core::Value::Float(f) => {
            if f.fract() == 0.0 {
                format!("{f:.1}")
            } else {
                f.to_string()
            }
        }
        ferric_core::Value::Multifield(mf) => {
            let items: Vec<String> = mf.iter().map(|v| format_value(v, engine)).collect();
            format!("({})", items.join(" "))
        }
        ferric_core::Value::ExternalAddress(ea) => {
            format!("<External-{}>", ea.type_id.0)
        }
        ferric_core::Value::Void => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- parens_balanced ----

    #[test]
    fn balanced_empty() {
        assert!(parens_balanced(""));
    }

    #[test]
    fn balanced_complete_form() {
        assert!(parens_balanced("(defrule test (a) => (b))"));
    }

    #[test]
    fn unbalanced_open() {
        assert!(!parens_balanced("(defrule test (a"));
    }

    #[test]
    fn balanced_with_string_containing_close_paren() {
        assert!(parens_balanced(r#"(printout t "hello)" crlf)"#));
    }

    #[test]
    fn balanced_no_parens() {
        assert!(parens_balanced("hello world"));
    }

    #[test]
    fn unbalanced_nested() {
        assert!(!parens_balanced("(defrule test (a (b (c"));
    }

    #[test]
    fn balanced_comment_ignored() {
        // The semicolon starts a comment — the unclosed paren does not count.
        assert!(parens_balanced("; (unclosed comment"));
    }

    #[test]
    fn balanced_multiline_comment() {
        // Comment only spans to end-of-line; subsequent lines are still checked.
        let input = "(defrule test ; this opens\n  (a) => (b))";
        assert!(parens_balanced(input));
    }

    #[test]
    fn unbalanced_multiline_with_comment() {
        // The `(a` on line 2 is real — total depth is 2 after this fragment.
        let input = "(defrule test\n  (a ; comment\n";
        assert!(!parens_balanced(input));
    }

    #[test]
    fn balanced_string_with_escaped_quote() {
        // Escaped quote inside a string should not end the string.
        assert!(parens_balanced(r#"(assert (msg "say \"hi\""))"#));
    }
}
