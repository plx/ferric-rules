//! REPL command parsing and dispatch.
//!
//! Commands are represented as an enum, parsed from user input strings,
//! and dispatched to handler methods on [`super::session::ReplSession`].

use super::session::ReplSession;

/// A parsed REPL command.
#[derive(Debug)]
pub(crate) enum ReplCommand {
    /// Exit the REPL.
    Exit,
    /// Reset the engine (clear facts, preserve rules, re-assert deffacts).
    Reset,
    /// Clear the engine completely (remove all rules, facts, templates, etc.).
    Clear,
    /// Run the inference engine.
    Run { limit: Option<usize> },
    /// List all facts in working memory.
    Facts,
    /// Show the count of activations on the agenda.
    Agenda,
    /// List all defined rules.
    Rules,
    /// Load a CLIPS file.
    Load { path: String },
    /// Save current facts to a file.
    Save { path: String },
    /// Show help for available commands.
    Help,
    /// Enable tracing for a target.
    Watch { target: WatchTarget },
    /// Disable tracing for a target.
    Unwatch { target: WatchTarget },
    /// Evaluate a general CLIPS form via `load_str`.
    Eval { source: String },
}

/// Targets for the `(watch)` / `(unwatch)` commands.
#[derive(Debug, Clone, Copy)]
pub(crate) enum WatchTarget {
    Facts,
    Rules,
}

/// Parse a trimmed input string into a [`ReplCommand`].
pub(crate) fn parse_command(input: &str) -> ReplCommand {
    match input {
        "(exit)" | "(quit)" => return ReplCommand::Exit,
        "(reset)" => return ReplCommand::Reset,
        "(clear)" => return ReplCommand::Clear,
        "(facts)" => return ReplCommand::Facts,
        "(agenda)" => return ReplCommand::Agenda,
        "(rules)" => return ReplCommand::Rules,
        "(help)" => return ReplCommand::Help,
        "(run)" => return ReplCommand::Run { limit: None },
        "(watch facts)" => {
            return ReplCommand::Watch {
                target: WatchTarget::Facts,
            }
        }
        "(watch rules)" => {
            return ReplCommand::Watch {
                target: WatchTarget::Rules,
            }
        }
        "(unwatch facts)" => {
            return ReplCommand::Unwatch {
                target: WatchTarget::Facts,
            }
        }
        "(unwatch rules)" => {
            return ReplCommand::Unwatch {
                target: WatchTarget::Rules,
            }
        }
        _ => {}
    }

    // (run N)
    if let Some(rest) = input.strip_prefix("(run ") {
        if let Some(num_str) = rest.strip_suffix(')') {
            if let Ok(n) = num_str.trim().parse::<usize>() {
                return ReplCommand::Run { limit: Some(n) };
            }
        }
    }

    // (load "path")
    if let Some(rest) = input.strip_prefix("(load ") {
        if let Some(path_expr) = rest.strip_suffix(')') {
            let path_str = path_expr.trim().trim_matches('"');
            return ReplCommand::Load {
                path: path_str.to_string(),
            };
        }
    }

    // (save "path")
    if let Some(rest) = input.strip_prefix("(save ") {
        if let Some(path_expr) = rest.strip_suffix(')') {
            let path_str = path_expr.trim().trim_matches('"');
            return ReplCommand::Save {
                path: path_str.to_string(),
            };
        }
    }

    // Fall through: evaluate as a general CLIPS form.
    ReplCommand::Eval {
        source: input.to_string(),
    }
}

impl ReplSession {
    /// Dispatch a parsed command. Returns `true` if the REPL should exit.
    #[allow(clippy::needless_pass_by_value)] // Ownership is cleaner for the match + Eval variant.
    pub(crate) fn dispatch(&mut self, cmd: ReplCommand) -> bool {
        match cmd {
            ReplCommand::Exit => return true,
            ReplCommand::Reset => self.cmd_reset(),
            ReplCommand::Clear => self.cmd_clear(),
            ReplCommand::Run { limit } => self.cmd_run(limit),
            ReplCommand::Facts => self.cmd_facts(),
            ReplCommand::Agenda => self.cmd_agenda(),
            ReplCommand::Rules => self.cmd_rules(),
            ReplCommand::Load { ref path } => self.cmd_load(path),
            ReplCommand::Save { ref path } => self.cmd_save(path),
            ReplCommand::Help => self.cmd_help(),
            ReplCommand::Watch { target } => self.cmd_watch(target, true),
            ReplCommand::Unwatch { target } => self.cmd_watch(target, false),
            ReplCommand::Eval { ref source } => self.cmd_eval(source),
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_exit() {
        assert!(matches!(parse_command("(exit)"), ReplCommand::Exit));
        assert!(matches!(parse_command("(quit)"), ReplCommand::Exit));
    }

    #[test]
    fn parse_reset() {
        assert!(matches!(parse_command("(reset)"), ReplCommand::Reset));
    }

    #[test]
    fn parse_run_unlimited() {
        assert!(matches!(
            parse_command("(run)"),
            ReplCommand::Run { limit: None }
        ));
    }

    #[test]
    fn parse_run_with_limit() {
        match parse_command("(run 10)") {
            ReplCommand::Run { limit: Some(10) } => {}
            other => panic!("Expected Run with limit 10, got {other:?}"),
        }
    }

    #[test]
    fn parse_load() {
        match parse_command(r#"(load "test.clp")"#) {
            ReplCommand::Load { path } => assert_eq!(path, "test.clp"),
            other => panic!("Expected Load, got {other:?}"),
        }
    }

    #[test]
    fn parse_save() {
        match parse_command(r#"(save "output.clp")"#) {
            ReplCommand::Save { path } => assert_eq!(path, "output.clp"),
            other => panic!("Expected Save, got {other:?}"),
        }
    }

    #[test]
    fn parse_watch_facts() {
        assert!(matches!(
            parse_command("(watch facts)"),
            ReplCommand::Watch {
                target: WatchTarget::Facts
            }
        ));
    }

    #[test]
    fn parse_unknown_falls_to_eval() {
        match parse_command("(assert (hello world))") {
            ReplCommand::Eval { source } => assert_eq!(source, "(assert (hello world))"),
            other => panic!("Expected Eval, got {other:?}"),
        }
    }
}
