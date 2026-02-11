//! Parse error types with source location information.

use crate::span::Span;
use std::fmt;

/// A parse error with location information.
#[derive(Clone, Debug)]
pub struct ParseError {
    /// Human-readable error message.
    pub message: String,
    /// Location of the error in source code.
    pub span: Span,
    /// Category of error for programmatic handling.
    pub kind: ParseErrorKind,
}

impl ParseError {
    /// Creates a new parse error.
    #[must_use]
    pub fn new(message: impl Into<String>, span: Span, kind: ParseErrorKind) -> Self {
        Self {
            message: message.into(),
            span,
            kind,
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}: {}",
            self.span.start.line, self.span.start.column, self.message
        )
    }
}

impl std::error::Error for ParseError {}

/// Categories of parse errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseErrorKind {
    /// Encountered a character that is not valid in any token.
    UnexpectedCharacter,
    /// String literal was not closed before end of input.
    UnterminatedString,
    /// Number literal could not be parsed.
    InvalidNumber,
    /// Token appeared where it was not expected.
    UnexpectedToken,
    /// Opening parenthesis was not closed.
    UnclosedParen,
    /// Closing parenthesis without matching opening parenthesis.
    UnexpectedCloseParen,
}

/// A lexical error encountered during tokenization.
#[derive(Clone, Debug)]
pub struct LexError {
    /// Human-readable error message.
    pub message: String,
    /// Location of the error in source code.
    pub span: Span,
    /// Category of error.
    pub kind: ParseErrorKind,
}

impl LexError {
    /// Creates a new lexical error.
    #[must_use]
    pub fn new(message: impl Into<String>, span: Span, kind: ParseErrorKind) -> Self {
        Self {
            message: message.into(),
            span,
            kind,
        }
    }
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}: {}",
            self.span.start.line, self.span.start.column, self.message
        )
    }
}

impl std::error::Error for LexError {}
