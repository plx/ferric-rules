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
//! - `(rules)` — List all defined rules
//! - `(agenda)` — Show count of activations on the agenda
//! - `(clear)` — Clear the engine completely
//! - `(load "file")` — Load a CLIPS file
//! - `(save "file")` — Save current facts to a file
//! - `(watch facts)` / `(watch rules)` — Enable tracing
//! - `(unwatch facts)` / `(unwatch rules)` — Disable tracing
//! - `(help)` — Show available commands
//! - `(exit)` / `(quit)` — Exit the REPL
//!
//! Any other input is evaluated as a CLIPS form via `engine.load_str()`.
//!
//! Exit codes:
//! - 0: Normal exit

mod commands;
mod display;
mod history;
mod input;
mod session;

use std::path::PathBuf;

use rustyline::error::ReadlineError;
use rustyline::Editor;

use self::commands::parse_command;
use self::input::FerricHelper;
use self::session::ReplSession;

const PROMPT: &str = "CLIPS> ";

/// Execute the `repl` subcommand.
pub fn execute(load_files: &[PathBuf]) -> i32 {
    let mut session = ReplSession::new();

    println!("Ferric REPL v{}", env!("CARGO_PKG_VERSION"));
    println!("Type (help) for commands, (exit) to quit.");

    let helper = FerricHelper;
    let config = rustyline::Config::builder()
        .auto_add_history(true)
        .build();

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

                let cmd = parse_command(trimmed);
                if session.dispatch(cmd) {
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
