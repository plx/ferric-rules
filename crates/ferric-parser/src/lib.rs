//! # Ferric Parser
//!
//! Lexer, S-expression parser, and AST for CLIPS-compatible rule syntax.
//!
//! This crate provides a two-stage parser:
//! - **Stage 1 (this module)**: Lexical analysis and S-expression parsing
//! - **Stage 2 (future)**: AST construction and semantic validation
//!
//! This crate is not intended for direct use by end-users; prefer the
//! `ferric` facade crate instead.
//!
//! # Examples
//!
//! ```
//! use ferric_parser::{parse_sexprs, FileId};
//!
//! let file_id = FileId(0);
//! let result = parse_sexprs("(defrule my-rule (fact ?x) => (assert (result ?x)))", file_id);
//!
//! if result.errors.is_empty() {
//!     println!("Parsed {} expressions", result.exprs.len());
//! } else {
//!     for error in &result.errors {
//!         eprintln!("Error: {}", error);
//!     }
//! }
//! ```

pub mod error;
pub mod lexer;
pub mod sexpr;
pub mod span;

// Re-export commonly used types for convenience
pub use error::{LexError, ParseError, ParseErrorKind};
pub use lexer::{lex, SpannedToken, Token};
pub use sexpr::{parse_sexprs, Atom, Connective, ParseResult, SExpr};
pub use span::{FileId, Position, Span};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoke_test_parse_integration() {
        let source = "(defrule test (person ?x) => (printout t ?x crlf))";
        let result = parse_sexprs(source, FileId(0));
        assert!(result.errors.is_empty());
        assert_eq!(result.exprs.len(), 1);

        // Verify it's a list
        let list = result.exprs[0].as_list().expect("expected list");
        assert!(!list.is_empty());

        // Verify first element is 'defrule'
        assert_eq!(list[0].as_symbol(), Some("defrule"));
    }

    #[test]
    fn smoke_test_lex_integration() {
        let source = "(a b c)";
        let tokens = lex(source, FileId(0)).expect("lex failed");
        assert_eq!(tokens.len(), 5); // ( a b c )
    }
}
