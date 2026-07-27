//! Input handling: parenthesis balancing, rustyline helpers.

use std::borrow::Cow;

use rustyline::completion::{Completer, Pair};
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::{ValidationContext, ValidationResult, Validator};
use rustyline::Helper;

/// Command names available for tab completion.
const COMMAND_NAMES: &[&str] = &[
    "(agenda)",
    "(clear)",
    "(exit)",
    "(facts)",
    "(help)",
    "(load ",
    "(quit)",
    "(reset)",
    "(rules)",
    "(run)",
    "(run ",
    "(save ",
    "(unwatch ",
    "(watch ",
];

/// Rustyline helper providing validation and completion for the Ferric REPL.
pub(crate) struct FerricHelper;

impl Helper for FerricHelper {}

impl Validator for FerricHelper {
    fn validate(&self, ctx: &mut ValidationContext<'_>) -> rustyline::Result<ValidationResult> {
        if parens_balanced(ctx.input()) {
            Ok(ValidationResult::Valid(None))
        } else {
            Ok(ValidationResult::Incomplete)
        }
    }
}

impl Completer for FerricHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &rustyline::Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        let prefix = &line[..pos];

        // Only complete when the input starts with '('.
        if !prefix.starts_with('(') {
            return Ok((0, Vec::new()));
        }

        let matches: Vec<Pair> = COMMAND_NAMES
            .iter()
            .filter(|cmd| cmd.starts_with(prefix))
            .map(|cmd| Pair {
                display: (*cmd).to_string(),
                replacement: (*cmd).to_string(),
            })
            .collect();

        Ok((0, matches))
    }
}

impl Highlighter for FerricHelper {
    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(
        &'s self,
        prompt: &'p str,
        _default: bool,
    ) -> Cow<'b, str> {
        Cow::Borrowed(prompt)
    }
}

impl Hinter for FerricHelper {
    type Hint = String;
}

/// Check whether the parentheses in `input` are balanced.
///
/// Respects string literals (ignores parens inside `"..."`) and
/// line comments (ignores everything after `;` until end-of-line).
///
/// Returns `true` when the open-paren count is less than or equal to
/// the close-paren count, meaning the input forms a complete expression.
pub(crate) fn parens_balanced(input: &str) -> bool {
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
        assert!(parens_balanced("; (unclosed comment"));
    }

    #[test]
    fn balanced_multiline_comment() {
        let input = "(defrule test ; this opens\n  (a) => (b))";
        assert!(parens_balanced(input));
    }

    #[test]
    fn unbalanced_multiline_with_comment() {
        let input = "(defrule test\n  (a ; comment\n";
        assert!(!parens_balanced(input));
    }

    #[test]
    fn balanced_string_with_escaped_quote() {
        assert!(parens_balanced(r#"(assert (msg "say \"hi\""))"#));
    }
}
