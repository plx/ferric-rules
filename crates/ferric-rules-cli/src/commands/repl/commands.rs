//! Parse shell commands using the same syntax as ordinary CLIPS expressions.

use ferric_rules_parser::{parse_sexprs, Atom, FileId, SExpr};
use ferric_rules_runtime::MAX_SOURCE_BYTES;

#[derive(Debug)]
pub(crate) enum ReplCommand {
    Exit,
    Run { limit: Option<usize> },
    Facts,
    Agenda { module: Option<String> },
    Rules,
    Load { path: String },
    Help,
    Construct { source: SourceForm },
    Eval { source: SourceForm },
}

pub(crate) fn is_construct(expr: &SExpr) -> bool {
    matches!(
        expr.as_list()
            .and_then(|items| items.first())
            .and_then(SExpr::as_symbol),
        Some(
            "defrule"
                | "deffacts"
                | "deftemplate"
                | "defglobal"
                | "defmodule"
                | "deffunction"
                | "defgeneric"
                | "defmethod"
        )
    )
}

/// Store only the form text; retain its location without quadratic padding
/// across a file containing many small forms.
#[derive(Debug)]
pub(crate) struct SourceForm {
    text: String,
    line: u32,
    column: u32,
}

impl SourceForm {
    pub fn located(&self) -> String {
        let mut source = "\n".repeat(self.line.saturating_sub(1) as usize);
        source.push_str(&" ".repeat(self.column.saturating_sub(1) as usize));
        source.push_str(&self.text);
        source
    }
}

fn located_source(source: &str, expr: &SExpr) -> SourceForm {
    let span = expr.span();
    SourceForm {
        text: source[span.start.offset..span.end.offset].to_owned(),
        line: span.start.line,
        column: span.start.column,
    }
}

pub(crate) fn parse_commands(source: &str) -> Result<Vec<ReplCommand>, String> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err(format!("source exceeds the {MAX_SOURCE_BYTES}-byte limit"));
    }
    let expressions = parse_sexprs(source, FileId(0))
        .into_result()
        .map_err(|errors| {
            errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        })?;
    expressions
        .iter()
        .map(|expr| parse_command(source, expr))
        .collect()
}

fn parse_command(source: &str, expr: &SExpr) -> Result<ReplCommand, String> {
    if is_construct(expr) {
        return Ok(ReplCommand::Construct {
            source: located_source(source, expr),
        });
    }
    let Some(items) = expr.as_list() else {
        return Ok(ReplCommand::Eval {
            source: located_source(source, expr),
        });
    };
    let Some(name) = items.first().and_then(SExpr::as_symbol) else {
        return Ok(ReplCommand::Eval {
            source: located_source(source, expr),
        });
    };
    let args = &items[1..];
    let command = match (name, args) {
        ("exit" | "quit", []) => ReplCommand::Exit,
        ("run", []) => ReplCommand::Run { limit: None },
        ("run", [SExpr::Atom(Atom::Integer(value), _)]) if *value >= 0 => ReplCommand::Run {
            limit: Some(usize::try_from(*value).map_err(|_| "run limit is too large")?),
        },
        ("run", [SExpr::Atom(Atom::Integer(value), _)]) if *value == -1 => {
            ReplCommand::Run { limit: None }
        }
        ("facts", []) => ReplCommand::Facts,
        ("agenda", []) => ReplCommand::Agenda { module: None },
        ("agenda", [SExpr::Atom(Atom::Symbol(module), _)]) => ReplCommand::Agenda {
            module: Some(module.clone()),
        },
        ("rules", []) => ReplCommand::Rules,
        ("help", []) => ReplCommand::Help,
        ("load", [SExpr::Atom(Atom::String(path) | Atom::Symbol(path), _)]) => {
            ReplCommand::Load { path: path.clone() }
        }
        ("exit" | "quit" | "run" | "facts" | "agenda" | "rules" | "help" | "load", _) => {
            return Err(format!(
                "{}: invalid arguments for {name}",
                expr.span().start
            ));
        }
        _ => ReplCommand::Eval {
            source: located_source(source, expr),
        },
    };
    Ok(command)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_commands_use_parser_whitespace_comments_and_escaping() {
        let commands =
            parse_commands("; comment\n( run\n 3 ) (load \"a\\\"b\\\\c.clp\") (exit ; done\n)")
                .unwrap();
        assert!(matches!(commands[0], ReplCommand::Run { limit: Some(3) }));
        assert!(matches!(&commands[1], ReplCommand::Load { path } if path == "a\"b\\c.clp"));
        assert!(matches!(commands[2], ReplCommand::Exit));
    }

    #[test]
    fn malformed_shell_commands_do_not_fall_through_or_ignore_arguments() {
        for source in [
            "(run bad)",
            "(run 1 2)",
            "(load \"x\" extra)",
            "(exit 1)",
            "(facts MAIN)",
        ] {
            assert!(parse_commands(source).is_err(), "{source}");
        }
        assert!(parse_commands("(+ 1 2) (").is_err());
    }

    #[test]
    fn expressions_and_constructs_are_distinct_and_keep_locations() {
        let commands = parse_commands("\n(defrule r =>)\n  (assert (p))\n(+ 1 2)").unwrap();
        assert!(matches!(commands[0], ReplCommand::Construct { .. }));
        assert!(
            matches!(&commands[1], ReplCommand::Eval { source } if source.located() == "\n\n  (assert (p))")
        );
        assert!(matches!(commands[2], ReplCommand::Eval { .. }));
        assert!(parse_commands("; empty").unwrap().is_empty());
    }
}
