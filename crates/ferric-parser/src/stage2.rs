//! Stage 2: Construct AST and semantic interpretation.
//!
//! Stage 2 transforms the S-expression trees produced by Stage 1 into typed
//! construct representations for `deftemplate`, `defrule`, and `deffacts`.
//! Source spans are preserved through the transformation for diagnostics.
//!
//! ## Phase 2 implementation plan
//!
//! - Pass 002: AST types and interpreter scaffold
//! - Pass 003: Full interpretation for deftemplate, defrule, deffacts

use crate::sexpr::{Atom, SExpr};
use crate::span::Span;
use std::fmt;

/// A top-level construct produced by Stage 2 interpretation.
#[derive(Clone, Debug)]
pub enum Construct {
    Rule(RuleConstruct),
    Template(TemplateConstruct),
    Facts(FactsConstruct),
}

/// Interpreted `(defrule ...)`.
#[derive(Clone, Debug)]
pub struct RuleConstruct {
    pub name: String,
    pub span: Span,
    /// Optional doc comment string.
    pub comment: Option<String>,
    /// Salience declaration (defaults to 0).
    pub salience: i32,
    /// LHS patterns (raw S-expressions in this pass; typed patterns in Pass 003).
    pub lhs_raw: Vec<SExpr>,
    /// RHS actions (raw S-expressions in this pass; typed actions in Pass 003).
    pub rhs_raw: Vec<SExpr>,
}

/// Interpreted `(deftemplate ...)`.
#[derive(Clone, Debug)]
pub struct TemplateConstruct {
    pub name: String,
    pub span: Span,
    pub comment: Option<String>,
    /// Slot definitions (raw S-expressions in this pass; typed in Pass 003).
    pub slots_raw: Vec<SExpr>,
}

/// Interpreted `(deffacts ...)`.
#[derive(Clone, Debug)]
pub struct FactsConstruct {
    pub name: String,
    pub span: Span,
    pub comment: Option<String>,
    /// Fact bodies (raw S-expressions in this pass; typed in Pass 003).
    pub facts_raw: Vec<SExpr>,
}

/// Configuration for Stage 2 interpretation.
#[derive(Clone, Debug, Default)]
pub struct InterpreterConfig {
    /// If true, stop on first error. If false, collect all errors.
    pub strict: bool,
}

/// Error during Stage 2 construct interpretation.
#[derive(Clone, Debug)]
pub struct InterpretError {
    pub message: String,
    pub span: Span,
    pub kind: InterpretErrorKind,
    pub suggestions: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InterpretErrorKind {
    /// Expected a construct (top-level list), got something else.
    ExpectedConstruct,
    /// Empty construct (e.g., `()`).
    EmptyConstruct,
    /// Expected a keyword like defrule/deftemplate, got something else.
    ExpectedKeyword,
    /// Unknown construct keyword.
    UnknownConstruct,
    /// Missing required element (e.g., rule name, => separator).
    MissingElement,
    /// Invalid construct structure.
    InvalidStructure,
}

impl InterpretError {
    /// Creates an error for expecting a specific element.
    pub fn expected(what: &str, span: Span) -> Self {
        Self {
            message: format!("expected {what}"),
            span,
            kind: InterpretErrorKind::ExpectedConstruct,
            suggestions: Vec::new(),
        }
    }

    /// Creates an error for an empty construct.
    pub fn empty_construct(span: Span) -> Self {
        Self {
            message: "empty construct".to_string(),
            span,
            kind: InterpretErrorKind::EmptyConstruct,
            suggestions: vec!["constructs must have a keyword (defrule, deftemplate, deffacts)".to_string()],
        }
    }

    /// Creates an error for an unknown construct keyword.
    pub fn unknown_construct(keyword: &str, span: Span) -> Self {
        let suggestions = suggest_keyword(keyword);
        Self {
            message: format!("unknown construct keyword: {keyword}"),
            span,
            kind: InterpretErrorKind::UnknownConstruct,
            suggestions,
        }
    }

    /// Creates an error for a missing required element.
    pub fn missing(what: &str, span: Span) -> Self {
        Self {
            message: format!("missing {what}"),
            span,
            kind: InterpretErrorKind::MissingElement,
            suggestions: Vec::new(),
        }
    }

    /// Creates an error for an invalid structure.
    pub fn invalid(what: &str, span: Span) -> Self {
        Self {
            message: format!("invalid {what}"),
            span,
            kind: InterpretErrorKind::InvalidStructure,
            suggestions: Vec::new(),
        }
    }
}

impl fmt::Display for InterpretError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at line {}, column {}",
            self.message, self.span.start.line, self.span.start.column
        )?;
        if !self.suggestions.is_empty() {
            write!(f, "\n  suggestions:")?;
            for suggestion in &self.suggestions {
                write!(f, "\n    - {suggestion}")?;
            }
        }
        Ok(())
    }
}

/// Result of Stage 2 interpretation.
#[derive(Clone, Debug, Default)]
pub struct InterpretResult {
    pub constructs: Vec<Construct>,
    pub errors: Vec<InterpretError>,
}

/// Interpret a slice of S-expressions into typed constructs.
pub fn interpret_constructs(sexprs: &[SExpr], config: &InterpreterConfig) -> InterpretResult {
    let mut result = InterpretResult::default();

    for sexpr in sexprs {
        // Each top-level element must be a list
        let Some(list) = sexpr.as_list() else {
            result.errors.push(InterpretError::expected(
                "a construct (list starting with defrule, deftemplate, or deffacts)",
                sexpr.span(),
            ));
            if config.strict {
                return result;
            }
            continue;
        };

        // List must be non-empty
        if list.is_empty() {
            result
                .errors
                .push(InterpretError::empty_construct(sexpr.span()));
            if config.strict {
                return result;
            }
            continue;
        }

        // First element must be a symbol (the keyword)
        let Some(keyword) = list[0].as_symbol() else {
            result.errors.push(InterpretError {
                message: "expected construct keyword (symbol), got something else".to_string(),
                span: list[0].span(),
                kind: InterpretErrorKind::ExpectedKeyword,
                suggestions: vec!["construct keywords: defrule, deftemplate, deffacts".to_string()],
            });
            if config.strict {
                return result;
            }
            continue;
        };

        // Dispatch on keyword
        match keyword {
            "defrule" => {
                match interpret_rule(&list[1..], sexpr.span()) {
                    Ok(construct) => result.constructs.push(Construct::Rule(construct)),
                    Err(err) => {
                        result.errors.push(err);
                        if config.strict {
                            return result;
                        }
                    }
                }
            }
            "deftemplate" => {
                match interpret_template(&list[1..], sexpr.span()) {
                    Ok(construct) => result.constructs.push(Construct::Template(construct)),
                    Err(err) => {
                        result.errors.push(err);
                        if config.strict {
                            return result;
                        }
                    }
                }
            }
            "deffacts" => {
                match interpret_facts(&list[1..], sexpr.span()) {
                    Ok(construct) => result.constructs.push(Construct::Facts(construct)),
                    Err(err) => {
                        result.errors.push(err);
                        if config.strict {
                            return result;
                        }
                    }
                }
            }
            // Known CLIPS keywords that are not yet supported
            "deffunction" | "defglobal" | "defmodule" | "defclass" | "definstances"
            | "defmessage-handler" | "defgeneric" | "defmethod" => {
                result.errors.push(InterpretError {
                    message: format!("{keyword} is not yet supported"),
                    span: list[0].span(),
                    kind: InterpretErrorKind::UnknownConstruct,
                    suggestions: vec![
                        "currently supported: defrule, deftemplate, deffacts".to_string(),
                    ],
                });
                if config.strict {
                    return result;
                }
            }
            // Unknown keyword
            _ => {
                result
                    .errors
                    .push(InterpretError::unknown_construct(keyword, list[0].span()));
                if config.strict {
                    return result;
                }
            }
        }
    }

    result
}

/// Interprets a `defrule` construct.
///
/// Expects elements after the `defrule` keyword:
/// - name (symbol)
/// - optional comment (string)
/// - optional declare forms
/// - LHS patterns
/// - `=>` separator
/// - RHS actions
fn interpret_rule(elements: &[SExpr], span: Span) -> Result<RuleConstruct, InterpretError> {
    if elements.is_empty() {
        return Err(InterpretError::missing("rule name", span));
    }

    // First element must be the rule name (symbol)
    let name = elements[0]
        .as_symbol()
        .ok_or_else(|| InterpretError::expected("rule name (symbol)", elements[0].span()))?
        .to_string();

    let mut idx = 1;
    let mut comment = None;
    let mut salience = 0;

    // Check for optional comment (string as second element)
    if idx < elements.len() {
        if let Some(Atom::String(s)) = elements[idx].as_atom() {
            comment = Some(s.clone());
            idx += 1;
        }
    }

    // Check for optional declare forms
    while idx < elements.len() {
        if let Some(declare_list) = elements[idx].as_list() {
            if !declare_list.is_empty() && declare_list[0].as_symbol() == Some("declare") {
                // Process declare form - look for (salience N)
                for decl_item in &declare_list[1..] {
                    if let Some(item_list) = decl_item.as_list() {
                        if item_list.len() == 2 && item_list[0].as_symbol() == Some("salience") {
                            if let Some(Atom::Integer(sal)) = item_list[1].as_atom() {
                                #[allow(clippy::cast_possible_truncation)]
                                {
                                    salience = *sal as i32;
                                }
                            }
                        }
                    }
                }
                idx += 1;
            } else {
                break;
            }
        } else {
            break;
        }
    }

    // Find the => separator
    let arrow_pos = elements[idx..]
        .iter()
        .position(|e| e.as_symbol() == Some("=>"))
        .ok_or_else(|| InterpretError::missing("=> separator in rule", span))?;

    let arrow_idx = idx + arrow_pos;

    // LHS is everything between current position and =>
    let lhs_raw = elements[idx..arrow_idx].to_vec();

    // RHS is everything after =>
    let rhs_raw = elements[arrow_idx + 1..].to_vec();

    Ok(RuleConstruct {
        name,
        span,
        comment,
        salience,
        lhs_raw,
        rhs_raw,
    })
}

/// Interprets a `deftemplate` construct.
///
/// Expects elements after the `deftemplate` keyword:
/// - name (symbol)
/// - optional comment (string)
/// - slot definitions
fn interpret_template(elements: &[SExpr], span: Span) -> Result<TemplateConstruct, InterpretError> {
    if elements.is_empty() {
        return Err(InterpretError::missing("template name", span));
    }

    // First element must be the template name (symbol)
    let name = elements[0]
        .as_symbol()
        .ok_or_else(|| InterpretError::expected("template name (symbol)", elements[0].span()))?
        .to_string();

    let mut idx = 1;
    let mut comment = None;

    // Check for optional comment (string as second element)
    if idx < elements.len() {
        if let Some(Atom::String(s)) = elements[idx].as_atom() {
            comment = Some(s.clone());
            idx += 1;
        }
    }

    // Remaining elements are slot definitions
    let slots_raw = elements[idx..].to_vec();

    Ok(TemplateConstruct {
        name,
        span,
        comment,
        slots_raw,
    })
}

/// Interprets a `deffacts` construct.
///
/// Expects elements after the `deffacts` keyword:
/// - name (symbol)
/// - optional comment (string)
/// - fact bodies
fn interpret_facts(elements: &[SExpr], span: Span) -> Result<FactsConstruct, InterpretError> {
    if elements.is_empty() {
        return Err(InterpretError::missing("facts name", span));
    }

    // First element must be the facts name (symbol)
    let name = elements[0]
        .as_symbol()
        .ok_or_else(|| InterpretError::expected("facts name (symbol)", elements[0].span()))?
        .to_string();

    let mut idx = 1;
    let mut comment = None;

    // Check for optional comment (string as second element)
    if idx < elements.len() {
        if let Some(Atom::String(s)) = elements[idx].as_atom() {
            comment = Some(s.clone());
            idx += 1;
        }
    }

    // Remaining elements are fact bodies
    let facts_raw = elements[idx..].to_vec();

    Ok(FactsConstruct {
        name,
        span,
        comment,
        facts_raw,
    })
}

/// Suggests a keyword based on edit distance.
fn suggest_keyword(input: &str) -> Vec<String> {
    let known = [
        "defrule",
        "deftemplate",
        "deffacts",
        "deffunction",
        "defglobal",
        "defmodule",
    ];

    let mut suggestions = Vec::new();

    for &keyword in &known {
        if edit_distance(input, keyword) <= 2 {
            suggestions.push(format!("did you mean '{keyword}'?"));
        }
    }

    if suggestions.is_empty() {
        suggestions.push("valid keywords: defrule, deftemplate, deffacts".to_string());
    }

    suggestions
}

/// Computes Levenshtein edit distance between two strings.
fn edit_distance(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let m = a_chars.len();
    let n = b_chars.len();

    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }

    let mut prev_row: Vec<usize> = (0..=n).collect();
    let mut curr_row = vec![0; n + 1];

    for i in 1..=m {
        curr_row[0] = i;
        for j in 1..=n {
            let cost = usize::from(a_chars[i - 1] != b_chars[j - 1]);
            curr_row[j] = (prev_row[j] + 1)
                .min(curr_row[j - 1] + 1)
                .min(prev_row[j - 1] + cost);
        }
        std::mem::swap(&mut prev_row, &mut curr_row);
    }

    prev_row[n]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexpr::parse_sexprs;
    use crate::span::FileId;

    fn file() -> FileId {
        FileId(0)
    }

    #[test]
    fn interpret_empty_input() {
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&[], &config);
        assert!(result.errors.is_empty());
        assert!(result.constructs.is_empty());
    }

    #[test]
    fn interpret_non_list_top_level() {
        let parsed = parse_sexprs("42", file());
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].kind, InterpretErrorKind::ExpectedConstruct);
        assert!(result.constructs.is_empty());
    }

    #[test]
    fn interpret_empty_list() {
        let parsed = parse_sexprs("()", file());
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].kind, InterpretErrorKind::EmptyConstruct);
        assert!(result.constructs.is_empty());
    }

    #[test]
    fn interpret_non_symbol_head() {
        let parsed = parse_sexprs("(42 foo bar)", file());
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].kind, InterpretErrorKind::ExpectedKeyword);
        assert!(result.constructs.is_empty());
    }

    #[test]
    fn interpret_unknown_keyword() {
        let parsed = parse_sexprs("(defwidget foo)", file());
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].kind, InterpretErrorKind::UnknownConstruct);
        assert!(result.errors[0].message.contains("defwidget"));
        assert!(result.constructs.is_empty());
    }

    #[test]
    fn interpret_defrule_minimal() {
        let parsed = parse_sexprs("(defrule test (a) => (b))", file());
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty());
        assert_eq!(result.constructs.len(), 1);

        if let Construct::Rule(rule) = &result.constructs[0] {
            assert_eq!(rule.name, "test");
            assert_eq!(rule.salience, 0);
            assert!(rule.comment.is_none());
            assert_eq!(rule.lhs_raw.len(), 1);
            assert_eq!(rule.rhs_raw.len(), 1);
        } else {
            panic!("expected Rule construct");
        }
    }

    #[test]
    fn interpret_defrule_missing_name() {
        let parsed = parse_sexprs("(defrule)", file());
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].kind, InterpretErrorKind::MissingElement);
        assert!(result.errors[0].message.contains("rule name"));
    }

    #[test]
    fn interpret_defrule_missing_arrow() {
        let parsed = parse_sexprs("(defrule test (a) (b))", file());
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].kind, InterpretErrorKind::MissingElement);
        assert!(result.errors[0].message.contains("=>"));
    }

    #[test]
    fn interpret_deftemplate_minimal() {
        let parsed = parse_sexprs("(deftemplate person (slot name))", file());
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty());
        assert_eq!(result.constructs.len(), 1);

        if let Construct::Template(template) = &result.constructs[0] {
            assert_eq!(template.name, "person");
            assert!(template.comment.is_none());
            assert_eq!(template.slots_raw.len(), 1);
        } else {
            panic!("expected Template construct");
        }
    }

    #[test]
    fn interpret_deffacts_minimal() {
        let parsed = parse_sexprs("(deffacts startup (person Alice))", file());
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty());
        assert_eq!(result.constructs.len(), 1);

        if let Construct::Facts(facts) = &result.constructs[0] {
            assert_eq!(facts.name, "startup");
            assert!(facts.comment.is_none());
            assert_eq!(facts.facts_raw.len(), 1);
        } else {
            panic!("expected Facts construct");
        }
    }

    #[test]
    fn interpret_strict_stops_on_first_error() {
        let parsed = parse_sexprs("(defwidget foo) (defrule test (a) => (b))", file());
        let config = InterpreterConfig { strict: true };
        let result = interpret_constructs(&parsed.exprs, &config);
        assert_eq!(result.errors.len(), 1);
        assert!(result.constructs.is_empty());
    }

    #[test]
    fn interpret_classic_collects_all_errors() {
        let parsed = parse_sexprs("(defwidget foo) (defrule) (deffacts)", file());
        let config = InterpreterConfig { strict: false };
        let result = interpret_constructs(&parsed.exprs, &config);
        assert_eq!(result.errors.len(), 3);
        assert!(result.constructs.is_empty());
    }

    #[test]
    fn interpret_defrule_with_salience() {
        let parsed = parse_sexprs("(defrule test (declare (salience 10)) (a) => (b))", file());
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty());
        assert_eq!(result.constructs.len(), 1);

        if let Construct::Rule(rule) = &result.constructs[0] {
            assert_eq!(rule.name, "test");
            assert_eq!(rule.salience, 10);
            assert_eq!(rule.lhs_raw.len(), 1);
            assert_eq!(rule.rhs_raw.len(), 1);
        } else {
            panic!("expected Rule construct");
        }
    }

    #[test]
    fn interpret_defrule_with_comment() {
        let parsed = parse_sexprs(r#"(defrule test "A test rule" (a) => (b))"#, file());
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty());
        assert_eq!(result.constructs.len(), 1);

        if let Construct::Rule(rule) = &result.constructs[0] {
            assert_eq!(rule.name, "test");
            assert_eq!(rule.comment, Some("A test rule".to_string()));
            assert_eq!(rule.lhs_raw.len(), 1);
            assert_eq!(rule.rhs_raw.len(), 1);
        } else {
            panic!("expected Rule construct");
        }
    }

    #[test]
    fn interpret_mixed_constructs() {
        let source = r#"
            (deftemplate person (slot name) (slot age))
            (defrule greet (person (name ?n)) => (printout t "Hi " ?n))
            (deffacts initial (person (name Alice) (age 30)))
        "#;
        let parsed = parse_sexprs(source, file());
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty());
        assert_eq!(result.constructs.len(), 3);

        assert!(matches!(result.constructs[0], Construct::Template(_)));
        assert!(matches!(result.constructs[1], Construct::Rule(_)));
        assert!(matches!(result.constructs[2], Construct::Facts(_)));
    }

    #[test]
    fn interpret_known_unsupported() {
        let parsed = parse_sexprs("(deffunction foo () (+ 1 2))", file());
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].kind, InterpretErrorKind::UnknownConstruct);
        assert!(result.errors[0].message.contains("not yet supported"));
        assert!(result.constructs.is_empty());
    }

    #[test]
    fn interpret_deftemplate_with_comment() {
        let parsed = parse_sexprs(r#"(deftemplate person "Person template" (slot name))"#, file());
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty());
        assert_eq!(result.constructs.len(), 1);

        if let Construct::Template(template) = &result.constructs[0] {
            assert_eq!(template.name, "person");
            assert_eq!(template.comment, Some("Person template".to_string()));
            assert_eq!(template.slots_raw.len(), 1);
        } else {
            panic!("expected Template construct");
        }
    }

    #[test]
    fn interpret_deffacts_with_comment() {
        let parsed = parse_sexprs(r#"(deffacts startup "Initial facts" (person Alice))"#, file());
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty());
        assert_eq!(result.constructs.len(), 1);

        if let Construct::Facts(facts) = &result.constructs[0] {
            assert_eq!(facts.name, "startup");
            assert_eq!(facts.comment, Some("Initial facts".to_string()));
            assert_eq!(facts.facts_raw.len(), 1);
        } else {
            panic!("expected Facts construct");
        }
    }

    #[test]
    fn interpret_defrule_with_multiple_lhs_patterns() {
        let parsed = parse_sexprs("(defrule test (a) (b ?x) (c ?y) => (d))", file());
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty());
        assert_eq!(result.constructs.len(), 1);

        if let Construct::Rule(rule) = &result.constructs[0] {
            assert_eq!(rule.lhs_raw.len(), 3);
            assert_eq!(rule.rhs_raw.len(), 1);
        } else {
            panic!("expected Rule construct");
        }
    }

    #[test]
    fn interpret_defrule_with_multiple_rhs_actions() {
        let parsed = parse_sexprs("(defrule test (a) => (b) (c) (d))", file());
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty());
        assert_eq!(result.constructs.len(), 1);

        if let Construct::Rule(rule) = &result.constructs[0] {
            assert_eq!(rule.lhs_raw.len(), 1);
            assert_eq!(rule.rhs_raw.len(), 3);
        } else {
            panic!("expected Rule construct");
        }
    }

    #[test]
    fn interpret_deftemplate_multiple_slots() {
        let parsed = parse_sexprs("(deftemplate person (slot name) (slot age) (multislot hobbies))", file());
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty());
        assert_eq!(result.constructs.len(), 1);

        if let Construct::Template(template) = &result.constructs[0] {
            assert_eq!(template.slots_raw.len(), 3);
        } else {
            panic!("expected Template construct");
        }
    }

    #[test]
    fn interpret_deffacts_multiple_facts() {
        let parsed = parse_sexprs("(deffacts startup (person Alice) (person Bob) (setting debug on))", file());
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty());
        assert_eq!(result.constructs.len(), 1);

        if let Construct::Facts(facts) = &result.constructs[0] {
            assert_eq!(facts.facts_raw.len(), 3);
        } else {
            panic!("expected Facts construct");
        }
    }

    #[test]
    fn interpret_error_display() {
        let span = Span::new(
            crate::span::Position {
                offset: 0,
                line: 5,
                column: 12,
            },
            crate::span::Position {
                offset: 10,
                line: 5,
                column: 22,
            },
            file(),
        );
        let error = InterpretError::unknown_construct("defrulz", span);
        let display = format!("{error}");
        assert!(display.contains("line 5"));
        assert!(display.contains("column 12"));
        assert!(display.contains("defrulz"));
    }

    #[test]
    fn edit_distance_identical() {
        assert_eq!(edit_distance("hello", "hello"), 0);
    }

    #[test]
    fn edit_distance_one_insert() {
        assert_eq!(edit_distance("helo", "hello"), 1);
    }

    #[test]
    fn edit_distance_one_delete() {
        assert_eq!(edit_distance("hello", "helo"), 1);
    }

    #[test]
    fn edit_distance_one_substitution() {
        assert_eq!(edit_distance("hello", "hallo"), 1);
    }

    #[test]
    fn edit_distance_multiple_changes() {
        assert_eq!(edit_distance("kitten", "sitting"), 3);
    }

    #[test]
    fn suggest_keyword_close_match() {
        let suggestions = suggest_keyword("defrulz");
        assert!(!suggestions.is_empty());
        assert!(suggestions.iter().any(|s| s.contains("defrule")));
    }

    #[test]
    fn suggest_keyword_no_close_match() {
        let suggestions = suggest_keyword("foobar");
        assert!(!suggestions.is_empty());
        assert!(suggestions.iter().any(|s| s.contains("valid keywords")));
    }
}
