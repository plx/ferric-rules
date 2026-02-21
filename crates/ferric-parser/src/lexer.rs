//! Lexical analyzer for CLIPS-compatible syntax.

use crate::error::{LexError, ParseErrorKind};
use crate::span::{FileId, Position, Span};

/// A token in the CLIPS lexical grammar.
#[derive(Clone, Debug, PartialEq)]
pub enum Token {
    /// `(`
    LeftParen,
    /// `)`
    RightParen,
    /// Integer literal: `42`, `-7`, `+3`
    Integer(i64),
    /// Floating-point literal: `1.0`, `-3.14`, `2.5e10`
    Float(f64),
    /// String literal: `"hello"`
    String(String),
    /// Bare identifier/symbol: `defrule`, `person`, `name`
    Symbol(String),
    /// Single-field variable: `?x`, `?name`
    SingleVar(String),
    /// Multi-field variable: `$?rest`, `$?values`
    MultiVar(String),
    /// Global variable: `?*name*`
    GlobalVar(String),
    /// `&` (and connective)
    Ampersand,
    /// `|` (or connective)
    Pipe,
    /// `~` (not connective)
    Tilde,
    /// `:` (colon)
    Colon,
    /// `=` (equals)
    Equals,
    /// `<-` (left arrow / assignment)
    LeftArrow,
}

/// A token paired with its source location.
#[derive(Clone, Debug, PartialEq)]
pub struct SpannedToken {
    /// The token itself.
    pub token: Token,
    /// Source location.
    pub span: Span,
}

impl SpannedToken {
    fn new(token: Token, span: Span) -> Self {
        Self { token, span }
    }
}

/// Tokenizes CLIPS source code.
///
/// Returns all successfully lexed tokens. If any errors are encountered,
/// they are collected and returned, but lexing continues to find as many
/// errors as possible in a single pass.
///
/// # Errors
///
/// Returns a vector of `LexError` if any lexical errors are encountered
/// (unterminated strings, invalid characters, etc.).
pub fn lex(source: &str, file_id: FileId) -> Result<Vec<SpannedToken>, Vec<LexError>> {
    let lexer = Lexer::new(source, file_id);
    lexer.lex_all()
}

struct Lexer<'a> {
    chars: std::iter::Peekable<std::str::CharIndices<'a>>,
    position: Position,
    file_id: FileId,
    tokens: Vec<SpannedToken>,
    errors: Vec<LexError>,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str, file_id: FileId) -> Self {
        Self {
            chars: source.char_indices().peekable(),
            position: Position::new(),
            file_id,
            tokens: Vec::new(),
            errors: Vec::new(),
        }
    }

    #[allow(clippy::too_many_lines)]
    fn lex_all(mut self) -> Result<Vec<SpannedToken>, Vec<LexError>> {
        while let Some(&(_, ch)) = self.chars.peek() {
            match ch {
                // Whitespace
                ' ' | '\t' | '\n' | '\r' => {
                    self.advance();
                }
                // Comments
                ';' => {
                    self.advance();
                    self.skip_comment();
                }
                // Delimiters
                '(' => {
                    let start = self.position;
                    self.advance();
                    let span = Span::new(start, self.position, self.file_id);
                    self.tokens.push(SpannedToken::new(Token::LeftParen, span));
                }
                ')' => {
                    let start = self.position;
                    self.advance();
                    let span = Span::new(start, self.position, self.file_id);
                    self.tokens.push(SpannedToken::new(Token::RightParen, span));
                }
                // Strings
                '"' => {
                    self.lex_string();
                }
                // Variables
                '?' => {
                    self.lex_variable();
                }
                '$' => {
                    if self.peek_ahead(1) == Some('?') {
                        self.lex_multivar();
                    } else {
                        self.lex_symbol();
                    }
                }
                // Single-character operators
                '&' => {
                    let start = self.position;
                    self.advance();
                    let span = Span::new(start, self.position, self.file_id);
                    self.tokens.push(SpannedToken::new(Token::Ampersand, span));
                }
                '|' => {
                    let start = self.position;
                    self.advance();
                    let span = Span::new(start, self.position, self.file_id);
                    self.tokens.push(SpannedToken::new(Token::Pipe, span));
                }
                '~' => {
                    let start = self.position;
                    self.advance();
                    let span = Span::new(start, self.position, self.file_id);
                    self.tokens.push(SpannedToken::new(Token::Tilde, span));
                }
                ':' => {
                    let start = self.position;
                    self.advance();
                    let span = Span::new(start, self.position, self.file_id);
                    self.tokens.push(SpannedToken::new(Token::Colon, span));
                }
                '=' => {
                    // Check if this is part of a symbol like `=>`
                    if self.peek_ahead(1).is_some_and(is_symbol_char) {
                        self.lex_symbol();
                    } else {
                        let start = self.position;
                        self.advance();
                        let span = Span::new(start, self.position, self.file_id);
                        self.tokens.push(SpannedToken::new(Token::Equals, span));
                    }
                }
                '<' => {
                    if self.peek_ahead(1) == Some('-') {
                        let start = self.position;
                        self.advance();
                        self.advance();
                        let span = Span::new(start, self.position, self.file_id);
                        self.tokens.push(SpannedToken::new(Token::LeftArrow, span));
                    } else {
                        self.lex_symbol();
                    }
                }
                // Numbers (sign or digit)
                '+' | '-' => {
                    if self.peek_ahead(1).is_some_and(|c| c.is_ascii_digit()) {
                        self.lex_number();
                    } else {
                        self.lex_symbol();
                    }
                }
                '0'..='9' => {
                    self.lex_number();
                }
                // Symbols
                _ if is_symbol_start(ch) => {
                    self.lex_symbol();
                }
                _ => {
                    let start = self.position;
                    self.advance();
                    let span = Span::new(start, self.position, self.file_id);
                    self.errors.push(LexError::new(
                        format!("unexpected character: {ch:?}"),
                        span,
                        ParseErrorKind::UnexpectedCharacter,
                    ));
                }
            }
        }

        if self.errors.is_empty() {
            Ok(self.tokens)
        } else {
            Err(self.errors)
        }
    }

    fn advance(&mut self) -> Option<char> {
        if let Some((_, ch)) = self.chars.next() {
            self.position.advance(ch);
            Some(ch)
        } else {
            None
        }
    }

    fn peek_ahead(&mut self, n: usize) -> Option<char> {
        // Peek at the character n positions ahead
        let mut iter = self.chars.clone();
        for _ in 0..n {
            iter.next();
        }
        iter.peek().map(|&(_, ch)| ch)
    }

    fn skip_comment(&mut self) {
        while let Some(&(_, ch)) = self.chars.peek() {
            if ch == '\n' {
                break;
            }
            self.advance();
        }
    }

    fn lex_string(&mut self) {
        let start = self.position;
        self.advance(); // consume opening "

        let mut string = String::new();
        let mut terminated = false;

        while let Some(&(_, ch)) = self.chars.peek() {
            if ch == '"' {
                self.advance();
                terminated = true;
                break;
            } else if ch == '\\' {
                self.advance();
                if let Some(escaped) = self.advance() {
                    // Simple escape handling: \n, \t, \", \\
                    match escaped {
                        'n' => string.push('\n'),
                        't' => string.push('\t'),
                        'r' => string.push('\r'),
                        '"' => string.push('"'),
                        '\\' => string.push('\\'),
                        _ => {
                            string.push('\\');
                            string.push(escaped);
                        }
                    }
                }
            } else {
                string.push(ch);
                self.advance();
            }
        }

        let span = Span::new(start, self.position, self.file_id);

        if terminated {
            self.tokens
                .push(SpannedToken::new(Token::String(string), span));
        } else {
            self.errors.push(LexError::new(
                "unterminated string literal",
                span,
                ParseErrorKind::UnterminatedString,
            ));
        }
    }

    fn lex_variable(&mut self) {
        let start = self.position;
        self.advance(); // consume ?

        // Check for global variable ?*...*
        if self.chars.peek().map(|&(_, ch)| ch) == Some('*') {
            self.advance(); // consume *
            let mut name = String::new();
            while let Some(&(_, ch)) = self.chars.peek() {
                if ch == '*' {
                    self.advance();
                    break;
                } else if is_symbol_char(ch) {
                    name.push(ch);
                    self.advance();
                } else {
                    break;
                }
            }
            let span = Span::new(start, self.position, self.file_id);
            self.tokens
                .push(SpannedToken::new(Token::GlobalVar(name), span));
        } else {
            // Regular single-field variable
            let mut name = String::new();
            while let Some(&(_, ch)) = self.chars.peek() {
                if is_symbol_char(ch) {
                    name.push(ch);
                    self.advance();
                } else {
                    break;
                }
            }
            let span = Span::new(start, self.position, self.file_id);
            self.tokens
                .push(SpannedToken::new(Token::SingleVar(name), span));
        }
    }

    fn lex_multivar(&mut self) {
        let start = self.position;
        self.advance(); // consume $
        self.advance(); // consume ?

        let mut name = String::new();
        while let Some(&(_, ch)) = self.chars.peek() {
            if is_symbol_char(ch) {
                name.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        let span = Span::new(start, self.position, self.file_id);
        self.tokens
            .push(SpannedToken::new(Token::MultiVar(name), span));
    }

    fn lex_number(&mut self) {
        let start = self.position;
        let mut num_str = String::new();

        // Optional sign
        if let Some(&(_, ch @ ('+' | '-'))) = self.chars.peek() {
            num_str.push(ch);
            self.advance();
        }

        // Integer part
        while let Some(&(_, ch @ '0'..='9')) = self.chars.peek() {
            num_str.push(ch);
            self.advance();
        }

        // Check for float (decimal point or exponent)
        let mut is_float = false;

        if self.chars.peek().map(|&(_, ch)| ch) == Some('.') {
            // Look ahead to ensure it's not a symbol like `1.abc`
            if self.peek_ahead(1).is_some_and(|c| c.is_ascii_digit()) {
                is_float = true;
                num_str.push('.');
                self.advance();

                while let Some(&(_, ch @ '0'..='9')) = self.chars.peek() {
                    num_str.push(ch);
                    self.advance();
                }
            }
        }

        // Exponent
        if let Some(&(_, ch @ ('e' | 'E'))) = self.chars.peek() {
            is_float = true;
            num_str.push(ch);
            self.advance();

            if let Some(&(_, sign @ ('+' | '-'))) = self.chars.peek() {
                num_str.push(sign);
                self.advance();
            }

            while let Some(&(_, ch @ '0'..='9')) = self.chars.peek() {
                num_str.push(ch);
                self.advance();
            }
        }

        let span = Span::new(start, self.position, self.file_id);

        if is_float {
            match num_str.parse::<f64>() {
                Ok(val) => self.tokens.push(SpannedToken::new(Token::Float(val), span)),
                Err(_) => {
                    self.errors.push(LexError::new(
                        format!("invalid floating-point number: {num_str}"),
                        span,
                        ParseErrorKind::InvalidNumber,
                    ));
                }
            }
        } else {
            match num_str.parse::<i64>() {
                Ok(val) => self
                    .tokens
                    .push(SpannedToken::new(Token::Integer(val), span)),
                Err(_) => {
                    self.errors.push(LexError::new(
                        format!("invalid integer: {num_str}"),
                        span,
                        ParseErrorKind::InvalidNumber,
                    ));
                }
            }
        }
    }

    fn lex_symbol(&mut self) {
        let start = self.position;
        let mut symbol = String::new();

        while let Some(&(_, ch)) = self.chars.peek() {
            if is_symbol_char(ch) {
                symbol.push(ch);
                self.advance();
            } else if ch == ':' {
                // Check for module-qualified name: SYMBOL::SYMBOL
                // peek_ahead(0) == ch == ':' (the first colon)
                // peek_ahead(1) is the character after the first colon
                // peek_ahead(2) is the character after that
                if self.peek_ahead(1) == Some(':') && self.peek_ahead(2).is_some_and(is_symbol_char)
                {
                    symbol.push(':');
                    self.advance(); // consume first ':'
                    symbol.push(':');
                    self.advance(); // consume second ':'
                                    // Continue scanning the rest as part of the same symbol
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        let span = Span::new(start, self.position, self.file_id);
        self.tokens
            .push(SpannedToken::new(Token::Symbol(symbol), span));
    }
}

fn is_symbol_start(ch: char) -> bool {
    is_symbol_char(ch)
}

fn is_symbol_char(ch: char) -> bool {
    ch.is_alphanumeric()
        || matches!(
            ch,
            '-' | '_'
                | '.'
                | '*'
                | '+'
                | '/'
                | '>'
                | '<'
                | '='
                | '!'
                | '#'
                | '$'
                | '%'
                | '^'
                | '@'
                | '{'
                | '}'
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file() -> FileId {
        FileId(0)
    }

    #[test]
    fn lex_empty_source() {
        let tokens = lex("", file()).unwrap();
        assert!(tokens.is_empty());
    }

    #[test]
    fn lex_whitespace() {
        let tokens = lex("  \t\n  ", file()).unwrap();
        assert!(tokens.is_empty());
    }

    #[test]
    fn lex_comment() {
        let tokens = lex("; this is a comment", file()).unwrap();
        assert!(tokens.is_empty());
    }

    #[test]
    fn lex_comment_with_tokens() {
        let tokens = lex("(defrule ; comment\ntest)", file()).unwrap();
        assert_eq!(tokens.len(), 4);
        assert!(matches!(tokens[0].token, Token::LeftParen));
        assert!(matches!(tokens[1].token, Token::Symbol(ref s) if s == "defrule"));
        assert!(matches!(tokens[2].token, Token::Symbol(ref s) if s == "test"));
        assert!(matches!(tokens[3].token, Token::RightParen));
    }

    #[test]
    fn lex_parens() {
        let tokens = lex("()", file()).unwrap();
        assert_eq!(tokens.len(), 2);
        assert!(matches!(tokens[0].token, Token::LeftParen));
        assert!(matches!(tokens[1].token, Token::RightParen));
    }

    #[test]
    fn lex_integer() {
        let tokens = lex("42", file()).unwrap();
        assert_eq!(tokens.len(), 1);
        assert!(matches!(tokens[0].token, Token::Integer(42)));
    }

    #[test]
    fn lex_negative_integer() {
        let tokens = lex("-7", file()).unwrap();
        assert_eq!(tokens.len(), 1);
        assert!(matches!(tokens[0].token, Token::Integer(-7)));
    }

    #[test]
    fn lex_positive_integer() {
        let tokens = lex("+3", file()).unwrap();
        assert_eq!(tokens.len(), 1);
        assert!(matches!(tokens[0].token, Token::Integer(3)));
    }

    #[test]
    fn lex_float() {
        let tokens = lex("2.71", file()).unwrap();
        assert_eq!(tokens.len(), 1);
        if let Token::Float(val) = tokens[0].token {
            assert!((val - 2.71).abs() < 0.001);
        } else {
            panic!("expected float token");
        }
    }

    #[test]
    fn lex_float_with_exponent() {
        let tokens = lex("2.5e10", file()).unwrap();
        assert_eq!(tokens.len(), 1);
        if let Token::Float(val) = tokens[0].token {
            assert!((val - 2.5e10).abs() < 1e8);
        } else {
            panic!("expected float token");
        }
    }

    #[test]
    fn lex_float_negative_exponent() {
        let tokens = lex("1e-3", file()).unwrap();
        assert_eq!(tokens.len(), 1);
        if let Token::Float(val) = tokens[0].token {
            assert!((val - 0.001).abs() < 1e-6);
        } else {
            panic!("expected float token");
        }
    }

    #[test]
    fn lex_string() {
        let tokens = lex(r#""hello world""#, file()).unwrap();
        assert_eq!(tokens.len(), 1);
        assert!(matches!(tokens[0].token, Token::String(ref s) if s == "hello world"));
    }

    #[test]
    fn lex_string_with_escape() {
        let tokens = lex(r#""hello \"world\"""#, file()).unwrap();
        assert_eq!(tokens.len(), 1);
        assert!(matches!(tokens[0].token, Token::String(ref s) if s == "hello \"world\""));
    }

    #[test]
    fn lex_unterminated_string() {
        let result = lex(r#""hello"#, file());
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].kind, ParseErrorKind::UnterminatedString);
    }

    #[test]
    fn lex_symbol() {
        let tokens = lex("defrule", file()).unwrap();
        assert_eq!(tokens.len(), 1);
        assert!(matches!(tokens[0].token, Token::Symbol(ref s) if s == "defrule"));
    }

    #[test]
    fn lex_symbol_with_special_chars() {
        let tokens = lex("my-rule*", file()).unwrap();
        assert_eq!(tokens.len(), 1);
        assert!(matches!(tokens[0].token, Token::Symbol(ref s) if s == "my-rule*"));
    }

    #[test]
    fn lex_single_var() {
        let tokens = lex("?name", file()).unwrap();
        assert_eq!(tokens.len(), 1);
        assert!(matches!(tokens[0].token, Token::SingleVar(ref s) if s == "name"));
    }

    #[test]
    fn lex_multi_var() {
        let tokens = lex("$?rest", file()).unwrap();
        assert_eq!(tokens.len(), 1);
        assert!(matches!(tokens[0].token, Token::MultiVar(ref s) if s == "rest"));
    }

    #[test]
    fn lex_global_var() {
        let tokens = lex("?*global*", file()).unwrap();
        assert_eq!(tokens.len(), 1);
        assert!(matches!(tokens[0].token, Token::GlobalVar(ref s) if s == "global"));
    }

    #[test]
    fn lex_connectives() {
        let tokens = lex("& | ~ : =", file()).unwrap();
        assert_eq!(tokens.len(), 5);
        assert!(matches!(tokens[0].token, Token::Ampersand));
        assert!(matches!(tokens[1].token, Token::Pipe));
        assert!(matches!(tokens[2].token, Token::Tilde));
        assert!(matches!(tokens[3].token, Token::Colon));
        assert!(matches!(tokens[4].token, Token::Equals));
    }

    #[test]
    fn lex_left_arrow() {
        let tokens = lex("<-", file()).unwrap();
        assert_eq!(tokens.len(), 1);
        assert!(matches!(tokens[0].token, Token::LeftArrow));
    }

    #[test]
    fn lex_clips_rule() {
        let source = r#"(defrule my-rule (person (name "John") (age ?a)) =>)"#;
        let tokens = lex(source, file()).unwrap();
        assert_eq!(tokens.len(), 16);
        assert!(matches!(tokens[0].token, Token::LeftParen));
        assert!(matches!(tokens[1].token, Token::Symbol(ref s) if s == "defrule"));
        assert!(matches!(tokens[2].token, Token::Symbol(ref s) if s == "my-rule"));
        assert!(matches!(tokens[3].token, Token::LeftParen));
        assert!(matches!(tokens[4].token, Token::Symbol(ref s) if s == "person"));
        assert!(matches!(tokens[5].token, Token::LeftParen));
        assert!(matches!(tokens[6].token, Token::Symbol(ref s) if s == "name"));
        assert!(matches!(tokens[7].token, Token::String(ref s) if s == "John"));
        assert!(matches!(tokens[8].token, Token::RightParen));
        assert!(matches!(tokens[9].token, Token::LeftParen));
        assert!(matches!(tokens[10].token, Token::Symbol(ref s) if s == "age"));
        assert!(matches!(tokens[11].token, Token::SingleVar(ref s) if s == "a"));
        assert!(matches!(tokens[12].token, Token::RightParen));
        assert!(matches!(tokens[13].token, Token::RightParen));
        assert!(matches!(tokens[14].token, Token::Symbol(ref s) if s == "=>"));
        assert!(matches!(tokens[15].token, Token::RightParen));
    }

    #[test]
    fn lex_mixed_symbols_and_operators() {
        let tokens = lex("+ - foo bar", file()).unwrap();
        assert_eq!(tokens.len(), 4);
        assert!(matches!(tokens[0].token, Token::Symbol(ref s) if s == "+"));
        assert!(matches!(tokens[1].token, Token::Symbol(ref s) if s == "-"));
        assert!(matches!(tokens[2].token, Token::Symbol(ref s) if s == "foo"));
        assert!(matches!(tokens[3].token, Token::Symbol(ref s) if s == "bar"));
    }

    #[test]
    fn lex_span_tracking() {
        let tokens = lex("(a)", file()).unwrap();
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0].span.start.line, 1);
        assert_eq!(tokens[0].span.start.column, 1);
        assert_eq!(tokens[1].span.start.line, 1);
        assert_eq!(tokens[1].span.start.column, 2);
        assert_eq!(tokens[2].span.start.line, 1);
        assert_eq!(tokens[2].span.start.column, 3);
    }

    #[test]
    fn lex_multiline_span_tracking() {
        let tokens = lex("(\na\n)", file()).unwrap();
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0].span.start.line, 1);
        assert_eq!(tokens[0].span.start.column, 1);
        assert_eq!(tokens[1].span.start.line, 2);
        assert_eq!(tokens[1].span.start.column, 1);
        assert_eq!(tokens[2].span.start.line, 3);
        assert_eq!(tokens[2].span.start.column, 1);
    }

    #[test]
    fn lex_equals_followed_by_close_paren() {
        let tokens = lex("= )", file()).unwrap();
        assert_eq!(tokens.len(), 2);
        assert!(matches!(tokens[0].token, Token::Equals));
        assert!(matches!(tokens[1].token, Token::RightParen));
    }

    #[test]
    fn lex_arrow_symbol() {
        let tokens = lex("=>", file()).unwrap();
        assert_eq!(tokens.len(), 1);
        assert!(matches!(tokens[0].token, Token::Symbol(ref s) if s == "=>"));
    }

    #[test]
    fn lex_module_qualified_symbol() {
        let tokens = lex("SENSOR::reading", file()).unwrap();
        assert_eq!(tokens.len(), 1);
        assert!(matches!(tokens[0].token, Token::Symbol(ref s) if s == "SENSOR::reading"));
    }

    #[test]
    fn lex_module_qualified_symbol_in_parens() {
        let tokens = lex("(SENSOR::reading)", file()).unwrap();
        assert_eq!(tokens.len(), 3);
        assert!(matches!(tokens[0].token, Token::LeftParen));
        assert!(matches!(tokens[1].token, Token::Symbol(ref s) if s == "SENSOR::reading"));
        assert!(matches!(tokens[2].token, Token::RightParen));
    }

    #[test]
    fn lex_module_qualified_main() {
        let tokens = lex("MAIN::person", file()).unwrap();
        assert_eq!(tokens.len(), 1);
        assert!(matches!(tokens[0].token, Token::Symbol(ref s) if s == "MAIN::person"));
    }

    #[test]
    fn lex_single_colon_not_qualified() {
        // Single colon between symbols still produces separate tokens
        let tokens = lex("foo:bar", file()).unwrap();
        assert_eq!(tokens.len(), 3);
        assert!(matches!(tokens[0].token, Token::Symbol(ref s) if s == "foo"));
        assert!(matches!(tokens[1].token, Token::Colon));
        assert!(matches!(tokens[2].token, Token::Symbol(ref s) if s == "bar"));
    }

    #[test]
    fn lex_double_colon_no_following_symbol() {
        // `FOO::` followed by a space should NOT form a qualified name
        // (no symbol char immediately after the second colon)
        let tokens = lex("FOO:: bar", file()).unwrap();
        // Should be: Symbol("FOO"), Colon, Colon, Symbol("bar")
        assert_eq!(tokens.len(), 4);
        assert!(matches!(tokens[0].token, Token::Symbol(ref s) if s == "FOO"));
        assert!(matches!(tokens[1].token, Token::Colon));
        assert!(matches!(tokens[2].token, Token::Colon));
        assert!(matches!(tokens[3].token, Token::Symbol(ref s) if s == "bar"));
    }

    #[test]
    fn lex_module_qualified_in_function_call() {
        let tokens = lex("(MATH::add 1 2)", file()).unwrap();
        assert_eq!(tokens.len(), 5);
        assert!(matches!(tokens[1].token, Token::Symbol(ref s) if s == "MATH::add"));
    }

    #[test]
    fn lex_module_qualified_preserves_span() {
        // "SENSOR::reading" is 15 characters; start col=1, end col=16
        let tokens = lex("SENSOR::reading", file()).unwrap();
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].span.start.column, 1);
        assert_eq!(tokens[0].span.end.column, 16);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    fn file() -> FileId {
        FileId(0)
    }

    /// Generate valid CLIPS-like token strings.
    fn clips_token() -> impl Strategy<Value = String> {
        prop_oneof![
            // Integers
            (0i64..=9999).prop_map(|n| n.to_string()),
            // Negative integers
            (1i64..=9999).prop_map(|n| format!("-{n}")),
            // Floats
            (0i64..=999, 0i64..=99).prop_map(|(a, b)| format!("{a}.{b:02}")),
            // Simple symbols (alphabetic)
            "[a-z][a-z0-9_-]{0,10}".prop_map(|s| s),
            // String literals
            "[a-zA-Z0-9 ]{0,20}".prop_map(|s| format!("\"{s}\"")),
            // Variables
            "[a-z][a-z0-9]{0,5}".prop_map(|s| format!("?{s}")),
            // Multi-field variables
            "[a-z][a-z0-9]{0,5}".prop_map(|s| format!("$?{s}")),
            // Parentheses
            Just("(".to_string()),
            Just(")".to_string()),
            // Connectives
            Just("&".to_string()),
            Just("|".to_string()),
            Just("~".to_string()),
        ]
    }

    proptest! {
        /// The lexer should never panic on arbitrary ASCII input.
        #[test]
        fn lex_never_panics(s in "[\\x20-\\x7e]{0,100}") {
            // It's fine to get errors, but it should not panic
            let _ = lex(&s, file());
        }

        /// Token spans should be non-overlapping and ordered by start offset.
        #[test]
        fn lex_spans_are_ordered_and_non_overlapping(s in "[\\x20-\\x7e]{0,100}") {
            if let Ok(tokens) = lex(&s, file()) {
                for pair in tokens.windows(2) {
                    prop_assert!(
                        pair[0].span.end.offset <= pair[1].span.start.offset,
                        "Token spans overlap or are out of order: {:?} and {:?}",
                        pair[0], pair[1]
                    );
                }
            }
        }

        /// Token spans should stay within the source string bounds.
        #[test]
        fn lex_spans_within_source_bounds(s in "[\\x20-\\x7e]{0,100}") {
            if let Ok(tokens) = lex(&s, file()) {
                for token in &tokens {
                    prop_assert!(
                        token.span.start.offset <= s.len(),
                        "Token start offset {} exceeds source length {}",
                        token.span.start.offset, s.len()
                    );
                    prop_assert!(
                        token.span.end.offset <= s.len(),
                        "Token end offset {} exceeds source length {}",
                        token.span.end.offset, s.len()
                    );
                }
            }
        }

        /// Sequences of valid tokens separated by spaces should lex without errors.
        #[test]
        fn lex_valid_tokens_no_errors(
            tokens in proptest::collection::vec(clips_token(), 0..10)
        ) {
            let source = tokens.join(" ");
            let result = lex(&source, file());
            prop_assert!(
                result.is_ok(),
                "Expected successful lex for {:?}, got errors: {:?}",
                source, result.unwrap_err()
            );
        }

        /// Lexing an integer always produces an Integer token.
        #[test]
        fn lex_integer_roundtrip(n in -99999i64..=99999) {
            let source = n.to_string();
            let result = lex(&source, file());
            prop_assert!(result.is_ok());
            let tokens = result.unwrap();
            prop_assert_eq!(tokens.len(), 1);
            match &tokens[0].token {
                Token::Integer(v) => prop_assert_eq!(*v, n),
                other => prop_assert!(false, "Expected Integer, got {:?}", other),
            }
        }

        /// Lexing a quoted string always produces a String token with the original content.
        #[test]
        fn lex_string_roundtrip(s in "[a-zA-Z0-9 ]{0,30}") {
            let source = format!("\"{s}\"");
            let result = lex(&source, file());
            prop_assert!(result.is_ok());
            let tokens = result.unwrap();
            prop_assert_eq!(tokens.len(), 1);
            match &tokens[0].token {
                Token::String(v) => prop_assert_eq!(v, &s),
                other => prop_assert!(false, "Expected String, got {:?}", other),
            }
        }
    }
}
