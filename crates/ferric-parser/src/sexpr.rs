//! S-expression tree types and parser.

use crate::error::{ParseError, ParseErrorKind};
use crate::lexer::{lex, SpannedToken, Token};
use crate::span::{FileId, Span};

/// An S-expression: either an atom or a list.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SExpr {
    /// Atomic value (number, string, symbol, variable, connective).
    Atom(Atom, Span),
    /// List of S-expressions.
    List(Vec<SExpr>, Span),
}

impl SExpr {
    /// Returns the span of this S-expression.
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Self::Atom(_, span) | Self::List(_, span) => *span,
        }
    }

    /// Returns the list contents if this is a `List`, otherwise `None`.
    #[must_use]
    pub fn as_list(&self) -> Option<&[SExpr]> {
        match self {
            Self::List(items, _) => Some(items),
            Self::Atom(_, _) => None,
        }
    }

    /// Returns the atom if this is an `Atom`, otherwise `None`.
    #[must_use]
    pub fn as_atom(&self) -> Option<&Atom> {
        match self {
            Self::Atom(atom, _) => Some(atom),
            Self::List(_, _) => None,
        }
    }

    /// Returns the symbol string if this is `Atom(Symbol(_))`, otherwise `None`.
    #[must_use]
    pub fn as_symbol(&self) -> Option<&str> {
        match self {
            Self::Atom(Atom::Symbol(s), _) => Some(s),
            _ => None,
        }
    }
}

/// An atomic value in an S-expression.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Atom {
    /// Integer literal.
    Integer(i64),
    /// Floating-point literal.
    Float(f64),
    /// String literal.
    String(String),
    /// Symbol/identifier.
    Symbol(String),
    /// Single-field variable (`?name`).
    SingleVar(String),
    /// Multi-field variable (`$?name`).
    MultiVar(String),
    /// Global variable (`?*name*`).
    GlobalVar(String),
    /// Connective operator.
    Connective(Connective),
}

/// Connective operators in CLIPS.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Connective {
    /// `&` (and)
    And,
    /// `|` (or)
    Or,
    /// `~` (not)
    Not,
    /// `:` (colon)
    Colon,
    /// `=` (equals)
    Equals,
    /// `<-` (assign)
    Assign,
}

impl std::fmt::Display for Connective {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let symbol = match self {
            Self::And => "&",
            Self::Or => "|",
            Self::Not => "~",
            Self::Colon => ":",
            Self::Equals => "=",
            Self::Assign => "<-",
        };
        f.write_str(symbol)
    }
}

/// Result of parsing S-expressions.
///
/// Contains both successfully parsed expressions and any errors encountered.
/// The parser attempts error recovery to parse as much as possible.
#[derive(Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ParseResult {
    /// Successfully parsed S-expressions.
    pub exprs: Vec<SExpr>,
    /// Parse errors encountered.
    pub errors: Vec<ParseError>,
}

impl ParseResult {
    /// Returns `Ok` if there were no errors, `Err` otherwise.
    pub fn into_result(self) -> Result<Vec<SExpr>, Vec<ParseError>> {
        if self.errors.is_empty() {
            Ok(self.exprs)
        } else {
            Err(self.errors)
        }
    }
}

/// Parses CLIPS source code into S-expressions.
///
/// This function performs both lexing and parsing. It attempts to recover
/// from errors to provide multiple diagnostics in a single pass.
///
/// # Examples
///
/// ```
/// use ferric_parser::{parse_sexprs, FileId};
///
/// let file_id = FileId(0);
/// let result = parse_sexprs("(a (b c) d)", file_id);
/// assert!(result.errors.is_empty());
/// assert_eq!(result.exprs.len(), 1);
/// ```
pub fn parse_sexprs(source: &str, file_id: FileId) -> ParseResult {
    // First, lex the source
    let tokens = match lex(source, file_id) {
        Ok(tokens) => tokens,
        Err(lex_errors) => {
            // Convert lex errors to parse errors
            let errors = lex_errors.into_iter().map(ParseError::from).collect();
            return ParseResult {
                exprs: Vec::new(),
                errors,
            };
        }
    };

    // Then, parse the tokens
    let parser = Parser::new(tokens);
    parser.parse_all()
}

struct Parser {
    tokens: Vec<SpannedToken>,
    position: usize,
    errors: Vec<ParseError>,
}

impl Parser {
    fn new(tokens: Vec<SpannedToken>) -> Self {
        Self {
            tokens,
            position: 0,
            errors: Vec::new(),
        }
    }

    fn parse_all(mut self) -> ParseResult {
        let mut exprs = Vec::new();

        while self.position < self.tokens.len() {
            match self.parse_expr() {
                Some(expr) => exprs.push(expr),
                None => {
                    // Error recovery: skip unexpected tokens
                    if let Some(token) = self.current() {
                        if matches!(token.token, Token::RightParen) {
                            self.errors.push(ParseError::new(
                                "unexpected closing parenthesis",
                                token.span,
                                ParseErrorKind::UnexpectedCloseParen,
                            ));
                            self.position += 1;
                        } else {
                            // Should not happen in normal cases
                            self.position += 1;
                        }
                    } else {
                        break;
                    }
                }
            }
        }

        ParseResult {
            exprs,
            errors: self.errors,
        }
    }

    fn current(&self) -> Option<&SpannedToken> {
        self.tokens.get(self.position)
    }

    fn advance(&mut self) -> Option<&SpannedToken> {
        if self.position < self.tokens.len() {
            let token = &self.tokens[self.position];
            self.position += 1;
            Some(token)
        } else {
            None
        }
    }

    fn parse_expr(&mut self) -> Option<SExpr> {
        let (atom, span) = {
            let token = self.current()?;
            let span = token.span;
            let atom = match &token.token {
                Token::LeftParen => return self.parse_list(),
                Token::RightParen => return None, // Handled by caller
                Token::Integer(n) => Atom::Integer(*n),
                Token::Float(f) => Atom::Float(*f),
                Token::String(s) => Atom::String(s.clone()),
                Token::Symbol(s) => Atom::Symbol(s.clone()),
                Token::SingleVar(v) => Atom::SingleVar(v.clone()),
                Token::MultiVar(v) => Atom::MultiVar(v.clone()),
                Token::GlobalVar(v) => Atom::GlobalVar(v.clone()),
                Token::Ampersand => Atom::Connective(Connective::And),
                Token::Pipe => Atom::Connective(Connective::Or),
                Token::Tilde => Atom::Connective(Connective::Not),
                Token::Colon => Atom::Connective(Connective::Colon),
                Token::Equals => Atom::Connective(Connective::Equals),
                Token::LeftArrow => Atom::Connective(Connective::Assign),
            };
            (atom, span)
        };

        self.advance();
        Some(SExpr::Atom(atom, span))
    }

    fn parse_list(&mut self) -> Option<SExpr> {
        let open_token = self.current()?;
        debug_assert!(matches!(open_token.token, Token::LeftParen));
        let start_span = open_token.span;
        self.advance();

        let mut items = Vec::new();

        loop {
            if let Some(token) = self.current() {
                if matches!(token.token, Token::RightParen) {
                    let end_span = token.span;
                    self.advance();
                    return Some(build_list_expr(items, start_span, end_span));
                }
            } else {
                // EOF without closing paren
                self.errors.push(ParseError::new(
                    "unclosed parenthesis",
                    start_span,
                    ParseErrorKind::UnclosedParen,
                ));
                return Some(SExpr::List(items, start_span));
            }

            if let Some(expr) = self.parse_expr() {
                items.push(expr);
            } else {
                // Unexpected token; caller handles recovery.
                break;
            }
        }

        // Should not reach here in normal cases
        Some(SExpr::List(items, start_span))
    }
}

fn build_list_expr(items: Vec<SExpr>, start_span: Span, end_span: Span) -> SExpr {
    SExpr::List(items, start_span.merge(end_span))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file() -> FileId {
        FileId(0)
    }

    #[test]
    fn parse_empty_source() {
        let result = parse_sexprs("", file());
        assert!(result.errors.is_empty());
        assert!(result.exprs.is_empty());
    }

    #[test]
    fn parse_integer_atom() {
        let result = parse_sexprs("42", file());
        assert!(result.errors.is_empty());
        assert_eq!(result.exprs.len(), 1);
        assert!(matches!(result.exprs[0], SExpr::Atom(Atom::Integer(42), _)));
    }

    #[test]
    fn parse_float_atom() {
        let result = parse_sexprs("2.71", file());
        assert!(result.errors.is_empty());
        assert_eq!(result.exprs.len(), 1);
        if let SExpr::Atom(Atom::Float(val), _) = result.exprs[0] {
            assert!((val - 2.71).abs() < 0.001);
        } else {
            panic!("expected float atom");
        }
    }

    #[test]
    fn parse_string_atom() {
        let result = parse_sexprs(r#""hello""#, file());
        assert!(result.errors.is_empty());
        assert_eq!(result.exprs.len(), 1);
        assert!(matches!(
            &result.exprs[0],
            SExpr::Atom(Atom::String(s), _) if s == "hello"
        ));
    }

    #[test]
    fn parse_symbol_atom() {
        let result = parse_sexprs("foo", file());
        assert!(result.errors.is_empty());
        assert_eq!(result.exprs.len(), 1);
        assert!(matches!(
            &result.exprs[0],
            SExpr::Atom(Atom::Symbol(s), _) if s == "foo"
        ));
    }

    #[test]
    fn parse_variable_atoms() {
        let result = parse_sexprs("?x $?rest ?*global*", file());
        assert!(result.errors.is_empty());
        assert_eq!(result.exprs.len(), 3);
        assert!(matches!(
            &result.exprs[0],
            SExpr::Atom(Atom::SingleVar(v), _) if v == "x"
        ));
        assert!(matches!(
            &result.exprs[1],
            SExpr::Atom(Atom::MultiVar(v), _) if v == "rest"
        ));
        assert!(matches!(
            &result.exprs[2],
            SExpr::Atom(Atom::GlobalVar(v), _) if v == "global"
        ));
    }

    #[test]
    fn parse_connectives() {
        let result = parse_sexprs("& | ~ : = <-", file());
        assert!(result.errors.is_empty());
        assert_eq!(result.exprs.len(), 6);
        assert!(matches!(
            result.exprs[0],
            SExpr::Atom(Atom::Connective(Connective::And), _)
        ));
        assert!(matches!(
            result.exprs[1],
            SExpr::Atom(Atom::Connective(Connective::Or), _)
        ));
        assert!(matches!(
            result.exprs[2],
            SExpr::Atom(Atom::Connective(Connective::Not), _)
        ));
        assert!(matches!(
            result.exprs[3],
            SExpr::Atom(Atom::Connective(Connective::Colon), _)
        ));
        assert!(matches!(
            result.exprs[4],
            SExpr::Atom(Atom::Connective(Connective::Equals), _)
        ));
        assert!(matches!(
            result.exprs[5],
            SExpr::Atom(Atom::Connective(Connective::Assign), _)
        ));
    }

    #[test]
    fn parse_empty_list() {
        let result = parse_sexprs("()", file());
        assert!(result.errors.is_empty());
        assert_eq!(result.exprs.len(), 1);
        if let SExpr::List(items, _) = &result.exprs[0] {
            assert!(items.is_empty());
        } else {
            panic!("expected list");
        }
    }

    #[test]
    fn parse_simple_list() {
        let result = parse_sexprs("(a b c)", file());
        assert!(result.errors.is_empty());
        assert_eq!(result.exprs.len(), 1);
        if let SExpr::List(items, _) = &result.exprs[0] {
            assert_eq!(items.len(), 3);
            assert!(matches!(&items[0], SExpr::Atom(Atom::Symbol(s), _) if s == "a"));
            assert!(matches!(&items[1], SExpr::Atom(Atom::Symbol(s), _) if s == "b"));
            assert!(matches!(&items[2], SExpr::Atom(Atom::Symbol(s), _) if s == "c"));
        } else {
            panic!("expected list");
        }
    }

    #[test]
    fn parse_nested_list() {
        let result = parse_sexprs("(a (b c) d)", file());
        assert!(result.errors.is_empty());
        assert_eq!(result.exprs.len(), 1);
        if let SExpr::List(items, _) = &result.exprs[0] {
            assert_eq!(items.len(), 3);
            assert!(matches!(&items[0], SExpr::Atom(Atom::Symbol(s), _) if s == "a"));
            if let SExpr::List(inner, _) = &items[1] {
                assert_eq!(inner.len(), 2);
            } else {
                panic!("expected nested list");
            }
            assert!(matches!(&items[2], SExpr::Atom(Atom::Symbol(s), _) if s == "d"));
        } else {
            panic!("expected list");
        }
    }

    #[test]
    fn parse_clips_rule_structure() {
        let source = r#"(defrule example (person (name ?n)) => (printout t "Hello " ?n crlf))"#;
        let result = parse_sexprs(source, file());
        assert!(result.errors.is_empty());
        assert_eq!(result.exprs.len(), 1);

        if let SExpr::List(items, _) = &result.exprs[0] {
            assert!(items.len() >= 4);
            assert_eq!(items[0].as_symbol(), Some("defrule"));
            assert_eq!(items[1].as_symbol(), Some("example"));
        } else {
            panic!("expected list");
        }
    }

    #[test]
    fn parse_deeply_nested_list() {
        let result = parse_sexprs("(a (b (c (d))))", file());
        assert!(result.errors.is_empty());
        assert_eq!(result.exprs.len(), 1);
    }

    #[test]
    fn parse_multiple_top_level_exprs() {
        let result = parse_sexprs("(a) (b) (c)", file());
        assert!(result.errors.is_empty());
        assert_eq!(result.exprs.len(), 3);
    }

    #[test]
    fn parse_unclosed_paren() {
        let result = parse_sexprs("(a b", file());
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].kind, ParseErrorKind::UnclosedParen);
        assert_eq!(result.exprs.len(), 1); // Still produces a list
    }

    #[test]
    fn parse_unexpected_close_paren() {
        let result = parse_sexprs(")", file());
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].kind, ParseErrorKind::UnexpectedCloseParen);
        assert!(result.exprs.is_empty());
    }

    #[test]
    fn parse_mixed_errors() {
        let result = parse_sexprs("(a b) ) (c", file());
        assert_eq!(result.errors.len(), 2);
        assert_eq!(result.errors[0].kind, ParseErrorKind::UnexpectedCloseParen);
        assert_eq!(result.errors[1].kind, ParseErrorKind::UnclosedParen);
        assert_eq!(result.exprs.len(), 2); // Two lists parsed
    }

    #[test]
    fn parse_span_correctness() {
        let result = parse_sexprs("(a)", file());
        assert!(result.errors.is_empty());
        assert_eq!(result.exprs.len(), 1);

        let span = result.exprs[0].span();
        assert_eq!(span.start.line, 1);
        assert_eq!(span.start.column, 1);
        assert_eq!(span.end.line, 1);
        assert_eq!(span.end.column, 4); // After the closing paren
    }

    #[test]
    fn sexpr_as_list() {
        let result = parse_sexprs("(a b c)", file());
        assert!(result.errors.is_empty());
        let list = result.exprs[0].as_list();
        assert!(list.is_some());
        assert_eq!(list.unwrap().len(), 3);
    }

    #[test]
    fn sexpr_as_atom() {
        let result = parse_sexprs("42", file());
        assert!(result.errors.is_empty());
        let atom = result.exprs[0].as_atom();
        assert!(atom.is_some());
        assert!(matches!(atom.unwrap(), Atom::Integer(42)));
    }

    #[test]
    fn sexpr_as_symbol() {
        let result = parse_sexprs("foo", file());
        assert!(result.errors.is_empty());
        let symbol = result.exprs[0].as_symbol();
        assert_eq!(symbol, Some("foo"));
    }

    #[test]
    fn sexpr_as_symbol_on_non_symbol_returns_none() {
        let result = parse_sexprs("42", file());
        assert!(result.errors.is_empty());
        let symbol = result.exprs[0].as_symbol();
        assert_eq!(symbol, None);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    fn file() -> FileId {
        FileId(0)
    }

    /// Generate balanced, well-formed S-expression source strings.
    fn balanced_sexpr() -> impl Strategy<Value = String> {
        let leaf = prop_oneof![
            "[a-z][a-z0-9]{0,5}".prop_map(|s| s),
            (0i64..=999).prop_map(|n| n.to_string()),
            "[a-z]{0,8}".prop_map(|s| format!("?{s}")),
        ];

        leaf.prop_recursive(
            4,  // max depth
            32, // max nodes
            8,  // items per list
            |inner| {
                proptest::collection::vec(inner, 0..5)
                    .prop_map(|items| format!("({})", items.join(" ")))
            },
        )
    }

    proptest! {
        /// The parser should never panic on arbitrary ASCII input.
        #[test]
        fn parse_never_panics(s in "[\\x20-\\x7e]{0,100}") {
            let _ = parse_sexprs(&s, file());
        }

        /// Balanced parenthesized input should parse without errors.
        #[test]
        fn parse_balanced_parens_no_errors(s in balanced_sexpr()) {
            let result = parse_sexprs(&s, file());
            prop_assert!(
                result.errors.is_empty(),
                "Expected no errors for balanced input {:?}, got: {:?}",
                s, result.errors
            );
        }

        /// Top-level atom count + list count should equal total parsed expressions.
        #[test]
        fn parse_preserves_atom_and_list_structure(s in balanced_sexpr()) {
            let result = parse_sexprs(&s, file());
            if result.errors.is_empty() {
                prop_assert!(
                    !result.exprs.is_empty(),
                    "Non-empty balanced input {:?} should produce at least one expr",
                    s
                );
            }
        }

        /// An unmatched `)` at the top level always produces an error.
        #[test]
        fn parse_unmatched_close_paren_is_error(
            prefix in "[a-z ]{0,10}",
        ) {
            let source = format!("{prefix})");
            let result = parse_sexprs(&source, file());
            prop_assert!(
                result.errors.iter().any(|e| e.kind == crate::error::ParseErrorKind::UnexpectedCloseParen),
                "Expected UnexpectedCloseParen error for {:?}, got: {:?}",
                source, result.errors
            );
        }

        /// An unclosed `(` with valid tokens always produces an UnclosedParen error.
        #[test]
        fn parse_unclosed_paren_is_error(
            content in "[a-z ]{0,15}",
        ) {
            let source = format!("({content}");
            let result = parse_sexprs(&source, file());
            prop_assert!(
                result.errors.iter().any(|e| e.kind == crate::error::ParseErrorKind::UnclosedParen),
                "Expected UnclosedParen error for {:?}, got: {:?}",
                source, result.errors
            );
        }

        /// For valid balanced input, span of root expression covers entire source (modulo whitespace).
        #[test]
        fn parse_root_span_covers_source(s in balanced_sexpr()) {
            let result = parse_sexprs(&s, file());
            if result.errors.is_empty() && result.exprs.len() == 1 {
                let span = result.exprs[0].span();
                // The span should start at or near offset 0 and end at or near s.len()
                prop_assert!(
                    span.start.offset <= 1,
                    "Root span start offset {} too far from 0 for {:?}",
                    span.start.offset, s
                );
                prop_assert!(
                    span.end.offset >= s.trim_end().len() - 1,
                    "Root span end offset {} too far from source end {} for {:?}",
                    span.end.offset, s.len(), s
                );
            }
        }
    }
}
