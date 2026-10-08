//! `ferric repl` command — interactive read-eval-print loop.
//!
//! Provides line editing and persistent history via rustyline. Supports
//! multiline input with balanced-parenthesis continuation, tab completion
//! of built-in commands, and tracing via `(watch)`.
//!
//! ## REPL Commands
//!
//! - `(reset)` — Reset the engine
//! - `(run)` / `(run N)` — Run rules (optionally with a step limit)
//! - `(facts)` — List all facts in working memory
//! - `(rules)` — List the current module's rules
//! - `(agenda [module])` — Show ordered activations and their fact basis
//! - `(clear)` — Clear the engine completely
//! - `(load "file")` — Load a CLIPS file
//! - `(save-facts "file")` / `(load-facts "file")` — Save/load facts
//! - `(watch facts)` / `(watch rules)` — Enable tracing
//! - `(unwatch facts)` / `(unwatch rules)` — Disable tracing
//! - `(help)` — Show available commands
//! - `(exit)` / `(quit)` — Exit the REPL
//!
//! Any other input is evaluated as a CLIPS form via `engine.eval_str()`.
//!
//! Exit codes:
//! - 0: Normal exit

pub(super) mod commands;
mod display;
mod history;
mod input;
pub(super) mod session;

use std::path::PathBuf;

use rustyline::error::ReadlineError;
use rustyline::Editor;

use self::commands::parse_commands;
use self::input::FerricHelper;
use self::session::ReplSession;

const PROMPT: &str = "CLIPS> ";

/// Execute the `repl` subcommand.
///
/// `snapshot` is an optional `(path, format)` pair. When provided, the engine
/// is restored from the snapshot file instead of starting fresh.
pub fn execute(
    load_files: &[PathBuf],
    #[cfg(feature = "serde")] snapshot: Option<(
        PathBuf,
        ferric_rules_runtime::serialization::SerializationFormat,
    )>,
    #[cfg(not(feature = "serde"))] _snapshot: Option<std::convert::Infallible>,
) -> i32 {
    #[cfg(feature = "serde")]
    let mut session = if let Some((path, format)) = snapshot {
        match ReplSession::from_snapshot(&path, format) {
            Ok(s) => s,
            Err(err) => {
                eprintln!("ferric repl: error loading snapshot: {err}");
                return 1;
            }
        }
    } else {
        ReplSession::new()
    };
    #[cfg(not(feature = "serde"))]
    let mut session = ReplSession::new();

    println!("Ferric REPL v{}", env!("CARGO_PKG_VERSION"));
    println!("Type (help) for commands, (exit) to quit.");

    let helper = FerricHelper;
    let config = rustyline::Config::builder().auto_add_history(true).build();

    let mut editor = match Editor::with_config(config) {
        Ok(e) => e,
        Err(err) => {
            eprintln!("ferric repl: failed to initialize editor: {err}");
            return 1;
        }
    };
    editor.set_helper(Some(helper));

    // Load persistent history (ignore errors — file may not exist).
    let history_file = history::history_path();
    if let Some(ref path) = history_file {
        let _ = editor.load_history(path);
    }

    // Preload files specified via --load.
    session.preload_files(load_files);

    loop {
        match editor.readline(PROMPT) {
            Ok(line) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                let commands = match parse_commands(trimmed) {
                    Ok(commands) => commands,
                    Err(error) => {
                        session.error("parse_error", error);
                        continue;
                    }
                };
                let mut exit = false;
                for command in commands {
                    match session.dispatch(command, true) {
                        Ok(true) => {
                            exit = true;
                            break;
                        }
                        Ok(false) => {}
                        Err(()) => break,
                    }
                }
                if exit {
                    break;
                }
            }
            // Ctrl-D (EOF) — exit cleanly.
            Err(ReadlineError::Eof) => {
                println!();
                break;
            }
            // Ctrl-C — cancel current input (rustyline clears the line).
            Err(ReadlineError::Interrupted) => {}
            Err(err) => {
                eprintln!("ferric repl: read error: {err}");
                return 1;
            }
        }
    }

    // Save persistent history.
    if let Some(ref path) = history_file {
        let _ = editor.save_history(path);
    }

    0
}
