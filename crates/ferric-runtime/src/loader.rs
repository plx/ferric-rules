//! Source code loader for CLIPS-compatible syntax.
//!
//! This module provides functionality to load CLIPS source code from strings
//! or files and convert it into engine-level constructs. Phase 1 supports:
//! - `(assert ...)` forms — load facts into working memory
//! - `(defrule ...)` forms — store raw rule definitions for later compilation
//!
//! Full rule compilation and pattern matching will be added in later phases.

use std::path::Path;
use thiserror::Error;

use ferric_core::{FactId, FerricString, Value};
use ferric_parser::{
    interpret_constructs, parse_sexprs, Atom, Construct, FactBody, FactValue, FileId,
    InterpreterConfig, LiteralKind, OrderedFactBody, RuleConstruct,
    SExpr, TemplateConstruct, TemplateFactBody,
};

use crate::engine::{Engine, EngineError};

/// Errors that can occur during source loading.
#[derive(Debug, Error)]
pub enum LoadError {
    #[error("parse error: {0}")]
    Parse(String),

    #[error("interpret error: {0}")]
    Interpret(String),

    #[error("unsupported top-level form: {name} at line {line}, column {column}")]
    UnsupportedForm {
        name: String,
        line: u32,
        column: u32,
    },

    #[error("invalid assert form: {0}")]
    InvalidAssert(String),

    #[error("invalid defrule form: {0}")]
    InvalidDefrule(String),

    #[error("engine error: {0}")]
    Engine(#[from] EngineError),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// A minimal rule definition stored at S-expression level.
///
/// This is Phase 1's placeholder for rules — it captures the raw S-expression
/// structure without full Stage 2 interpretation. Phase 2 replaces this with
/// a Stage 2 AST that is compiled into the rete network. This type is retained
/// for backward compatibility during the transition.
#[derive(Clone, Debug)]
pub struct RuleDef {
    /// Rule name
    pub name: String,
    /// Raw LHS patterns (S-expressions before the `=>`)
    pub lhs: Vec<SExpr>,
    /// Raw RHS actions (S-expressions after the `=>`)
    pub rhs: Vec<SExpr>,
}

/// Result of loading source code.
#[derive(Debug, Default)]
pub struct LoadResult {
    /// Facts asserted during loading.
    pub asserted_facts: Vec<FactId>,
    /// Rules registered during loading (typed constructs from Stage 2).
    pub rules: Vec<RuleConstruct>,
    /// Templates registered during loading.
    pub templates: Vec<TemplateConstruct>,
    /// Warnings/diagnostics (non-fatal).
    pub warnings: Vec<String>,
}

impl Engine {
    /// Load CLIPS source code from a string.
    ///
    /// Parses and processes top-level forms:
    /// - `(assert ...)` — assert facts into working memory
    /// - `(defrule ...)` — register rule definitions
    /// - Other forms produce `UnsupportedForm` errors
    ///
    /// # Errors
    ///
    /// Returns a vector of errors if:
    /// - Parse errors occur
    /// - Top-level forms are invalid or unsupported
    /// - Engine operations fail (e.g., encoding errors, wrong thread)
    ///
    /// # Examples
    ///
    /// ```
    /// use ferric_runtime::{Engine, EngineConfig};
    ///
    /// let mut engine = Engine::new(EngineConfig::utf8());
    /// let result = engine.load_str("(assert (person John 30))").unwrap();
    /// assert_eq!(result.asserted_facts.len(), 1);
    /// ```
    pub fn load_str(&mut self, source: &str) -> Result<LoadResult, Vec<LoadError>> {
        self.check_thread_affinity()
            .map_err(|e| vec![LoadError::Engine(e)])?;

        // Parse the source into S-expressions (Stage 1)
        let parse_result = parse_sexprs(source, FileId(0));

        // Convert parse errors to LoadError
        if !parse_result.errors.is_empty() {
            let errors = parse_result
                .errors
                .into_iter()
                .map(|e| LoadError::Parse(format!("{e}")))
                .collect();
            return Err(errors);
        }

        let mut result = LoadResult::default();
        let mut errors = Vec::new();

        // Separate assert forms from constructs
        // Assert forms are processed directly for Phase 1 compatibility
        let mut assert_forms = Vec::new();
        let mut construct_forms = Vec::new();

        for expr in &parse_result.exprs {
            if let Some(list) = expr.as_list() {
                if !list.is_empty() && list[0].as_symbol() == Some("assert") {
                    assert_forms.push(expr);
                } else if !list.is_empty()
                    && matches!(
                        list[0].as_symbol(),
                        Some("defrule" | "deftemplate" | "deffacts")
                    )
                {
                    construct_forms.push(expr.clone());
                } else {
                    // Unknown top-level form
                    let name = list[0]
                        .as_symbol()
                        .unwrap_or("<non-symbol>")
                        .to_string();
                    errors.push(LoadError::UnsupportedForm {
                        name,
                        line: list[0].span().start.line,
                        column: list[0].span().start.column,
                    });
                }
            } else {
                result.warnings.push(format!(
                    "skipping non-list top-level form at line {}",
                    expr.span().start.line
                ));
            }
        }

        // Process assert forms
        for expr in &assert_forms {
            if let Some(list) = expr.as_list() {
                if let Err(e) = self.process_assert(&list[1..], &mut result) {
                    errors.push(e);
                }
            }
        }

        // Interpret constructs via Stage 2
        if !construct_forms.is_empty() {
            let config = InterpreterConfig::default();
            let interpret_result = interpret_constructs(&construct_forms, &config);

            // Convert interpret errors to LoadError
            if !interpret_result.errors.is_empty() {
                for e in interpret_result.errors {
                    errors.push(LoadError::Interpret(format!("{e}")));
                }
            }

            // Process the interpreted constructs
            for construct in interpret_result.constructs {
                match construct {
                    Construct::Rule(rule) => {
                        result.rules.push(rule);
                    }
                    Construct::Template(template) => {
                        result.templates.push(template);
                    }
                    Construct::Facts(facts) => {
                        // Assert the facts from deffacts
                        if let Err(e) = self.process_deffacts_construct(&facts, &mut result) {
                            errors.push(e);
                        }
                    }
                }
            }
        }

        if errors.is_empty() {
            Ok(result)
        } else {
            Err(errors)
        }
    }

    /// Load CLIPS source code from a file.
    ///
    /// Reads the file contents and delegates to `load_str`.
    ///
    /// # Errors
    ///
    /// Returns errors if:
    /// - File cannot be read
    /// - Source parsing or processing fails
    pub fn load_file(&mut self, path: &Path) -> Result<LoadResult, Vec<LoadError>> {
        let source = std::fs::read_to_string(path).map_err(|e| vec![LoadError::Io(e)])?;
        self.load_str(&source)
    }

    /// Process a deffacts construct and assert its facts.
    fn process_deffacts_construct(
        &mut self,
        facts_construct: &ferric_parser::FactsConstruct,
        result: &mut LoadResult,
    ) -> Result<(), LoadError> {
        for fact_body in &facts_construct.facts {
            let fact_id = self.process_fact_body(fact_body, result)?;
            result.asserted_facts.push(fact_id);
        }
        Ok(())
    }

    /// Process a single fact body from a deffacts construct.
    fn process_fact_body(
        &mut self,
        fact_body: &FactBody,
        result: &mut LoadResult,
    ) -> Result<FactId, LoadError> {
        match fact_body {
            FactBody::Ordered(ordered) => self.process_ordered_fact_body(ordered, result),
            FactBody::Template(template) => self.process_template_fact_body(template, result),
        }
    }

    /// Process an ordered fact body.
    fn process_ordered_fact_body(
        &mut self,
        ordered: &OrderedFactBody,
        result: &mut LoadResult,
    ) -> Result<FactId, LoadError> {
        let mut fields = Vec::new();
        for fact_value in &ordered.values {
            if let Some(value) = self.fact_value_to_value(fact_value, result) {
                fields.push(value);
            }
        }

        self.assert_ordered(&ordered.relation, fields)
            .map_err(LoadError::Engine)
    }

    /// Process a template fact body.
    fn process_template_fact_body(
        &mut self,
        template: &TemplateFactBody,
        result: &mut LoadResult,
    ) -> Result<FactId, LoadError> {
        // For Phase 2, we treat template facts as ordered facts with the slot values
        // Full template support with slot matching will be added in later phases
        let mut fields = Vec::new();
        for slot_value in &template.slot_values {
            if let Some(value) = self.fact_value_to_value(&slot_value.value, result) {
                fields.push(value);
            }
        }

        self.assert_ordered(&template.template, fields)
            .map_err(LoadError::Engine)
    }

    /// Convert a `FactValue` to an engine Value.
    fn fact_value_to_value(&mut self, fact_value: &FactValue, result: &mut LoadResult) -> Option<Value> {
        match fact_value {
            FactValue::Literal(lit) => self.literal_to_value(&lit.value, lit.span.start.line, result),
            FactValue::Variable(_name, span) => {
                Self::warn_at_line(
                    result,
                    span.start.line,
                    "variables in deffacts not supported, skipping",
                );
                None
            }
            FactValue::GlobalVariable(_name, span) => {
                Self::warn_at_line(
                    result,
                    span.start.line,
                    "global variables in deffacts not supported, skipping",
                );
                None
            }
        }
    }

    /// Convert a `LiteralKind` to an engine Value.
    fn literal_to_value(
        &mut self,
        literal: &LiteralKind,
        line: u32,
        result: &mut LoadResult,
    ) -> Option<Value> {
        match literal {
            LiteralKind::Integer(n) => Some(Value::Integer(*n)),
            LiteralKind::Float(f) => Some(Value::Float(*f)),
            LiteralKind::String(s) => match FerricString::new(s, self.config.string_encoding) {
                Ok(fs) => Some(Value::String(fs)),
                Err(e) => {
                    Self::warn_with_detail(result, line, "string encoding error", &e);
                    None
                }
            },
            LiteralKind::Symbol(s) => {
                match self
                    .symbol_table
                    .intern_symbol(s, self.config.string_encoding)
                {
                    Ok(sym) => Some(Value::Symbol(sym)),
                    Err(e) => {
                        Self::warn_with_detail(result, line, "symbol encoding error", &e);
                        None
                    }
                }
            }
        }
    }

    /// Process an `(assert ...)` form.
    ///
    /// Each sub-list after `assert` is treated as a fact to assert.
    fn process_assert(&mut self, args: &[SExpr], result: &mut LoadResult) -> Result<(), LoadError> {
        for fact_expr in args {
            let fact_id = self.process_assert_fact(fact_expr, result)?;
            result.asserted_facts.push(fact_id);
        }
        Ok(())
    }

    /// Process a single fact within an assert form.
    fn process_assert_fact(
        &mut self,
        fact_expr: &SExpr,
        result: &mut LoadResult,
    ) -> Result<FactId, LoadError> {
        let fact_list = fact_expr
            .as_list()
            .ok_or_else(|| LoadError::InvalidAssert("expected list for fact".to_string()))?;

        if fact_list.is_empty() {
            return Err(LoadError::InvalidAssert("empty fact list".to_string()));
        }

        // First element is the relation name
        let relation = fact_list[0].as_symbol().ok_or_else(|| {
            LoadError::InvalidAssert("fact relation must be a symbol".to_string())
        })?;

        // Remaining elements are field values
        let mut fields = Vec::new();
        for field_expr in &fact_list[1..] {
            match self.atom_to_value(field_expr, result) {
                Some(value) => fields.push(value),
                None => {
                    // Skip unsupported values with a warning
                    Self::warn_at_line(
                        result,
                        field_expr.span().start.line,
                        "skipping unsupported field value",
                    );
                }
            }
        }

        self.assert_ordered(relation, fields)
            .map_err(LoadError::Engine)
    }


    /// Convert an S-expression atom to a Value.
    ///
    /// Returns `None` for unsupported atom types (variables, connectives).
    fn atom_to_value(&mut self, expr: &SExpr, result: &mut LoadResult) -> Option<Value> {
        let atom = expr.as_atom()?;

        match atom {
            Atom::Integer(n) => Some(Value::Integer(*n)),
            Atom::Float(f) => Some(Value::Float(*f)),
            Atom::String(s) => match FerricString::new(s, self.config.string_encoding) {
                Ok(fs) => Some(Value::String(fs)),
                Err(e) => {
                    Self::warn_with_detail(
                        result,
                        expr.span().start.line,
                        "string encoding error",
                        &e,
                    );
                    None
                }
            },
            Atom::Symbol(s) => {
                match self
                    .symbol_table
                    .intern_symbol(s, self.config.string_encoding)
                {
                    Ok(sym) => Some(Value::Symbol(sym)),
                    Err(e) => {
                        Self::warn_with_detail(
                            result,
                            expr.span().start.line,
                            "symbol encoding error",
                            &e,
                        );
                        None
                    }
                }
            }
            // Variables and connectives are not supported as fact values in Phase 1
            Atom::SingleVar(_) | Atom::MultiVar(_) | Atom::GlobalVar(_) | Atom::Connective(_) => {
                None
            }
        }
    }

    fn warn_at_line(result: &mut LoadResult, line: u32, message: &str) {
        result.warnings.push(format!("{message} at line {line}"));
    }

    fn warn_with_detail(
        result: &mut LoadResult,
        line: u32,
        message: &str,
        detail: &dyn std::fmt::Display,
    ) {
        result
            .warnings
            .push(format!("{message} at line {line}: {detail}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EngineConfig;
    use ferric_core::Fact;

    #[test]
    fn load_empty_string_returns_empty_result() {
        let mut engine = Engine::new(EngineConfig::utf8());
        let result = engine.load_str("").unwrap();
        assert!(result.asserted_facts.is_empty());
        assert!(result.rules.is_empty());
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn load_single_assert_form() {
        let mut engine = Engine::new(EngineConfig::utf8());
        let result = engine.load_str("(assert (person John 30))").unwrap();

        assert_eq!(result.asserted_facts.len(), 1);
        assert!(result.rules.is_empty());

        // Verify the fact was actually asserted
        let fact_id = result.asserted_facts[0];
        let fact = engine.get_fact(fact_id).unwrap().unwrap();
        if let Fact::Ordered(ordered) = fact {
            assert_eq!(ordered.fields.len(), 2);
        } else {
            panic!("expected ordered fact");
        }
    }

    #[test]
    fn load_multiple_assert_forms() {
        let mut engine = Engine::new(EngineConfig::utf8());
        let source = r"
            (assert (person Alice 25))
            (assert (person Bob 30))
            (assert (person Carol 35))
        ";
        let result = engine.load_str(source).unwrap();

        assert_eq!(result.asserted_facts.len(), 3);
        assert!(result.rules.is_empty());
    }

    #[test]
    fn load_assert_with_multiple_facts() {
        let mut engine = Engine::new(EngineConfig::utf8());
        let result = engine
            .load_str("(assert (person Alice) (person Bob))")
            .unwrap();

        assert_eq!(result.asserted_facts.len(), 2);
    }

    #[test]
    fn load_assert_with_various_value_types() {
        let mut engine = Engine::new(EngineConfig::utf8());
        let result = engine
            .load_str(r#"(assert (data 42 3.14 "hello" world))"#)
            .unwrap();

        assert_eq!(result.asserted_facts.len(), 1);

        let fact_id = result.asserted_facts[0];
        let fact = engine.get_fact(fact_id).unwrap().unwrap();
        if let Fact::Ordered(ordered) = fact {
            assert_eq!(ordered.fields.len(), 4);
            assert!(matches!(ordered.fields[0], Value::Integer(42)));
            #[allow(clippy::approx_constant)]
            {
                assert!(matches!(ordered.fields[1], Value::Float(f) if (f - 3.14).abs() < 0.001));
            }
            assert!(matches!(&ordered.fields[2], Value::String(s) if s.as_str() == "hello"));
            assert!(matches!(&ordered.fields[3], Value::Symbol(_)));
        } else {
            panic!("expected ordered fact");
        }
    }

    #[test]
    fn load_simple_defrule() {
        let mut engine = Engine::new(EngineConfig::utf8());
        let result = engine
            .load_str("(defrule test (person ?x) => (printout t ?x crlf))")
            .unwrap();

        assert!(result.asserted_facts.is_empty());
        assert_eq!(result.rules.len(), 1);

        let rule = &result.rules[0];
        assert_eq!(rule.name, "test");
        assert_eq!(rule.patterns.len(), 1);
        assert_eq!(rule.actions.len(), 1);
    }

    #[test]
    fn load_defrule_with_multiple_patterns() {
        let mut engine = Engine::new(EngineConfig::utf8());
        let source = r"
            (defrule match-pair
                (person ?x)
                (person ?y)
                =>
                (assert (pair ?x ?y)))
        ";
        let result = engine.load_str(source).unwrap();

        assert_eq!(result.rules.len(), 1);
        let rule = &result.rules[0];
        assert_eq!(rule.name, "match-pair");
        assert_eq!(rule.patterns.len(), 2);
        assert_eq!(rule.actions.len(), 1);
    }

    #[test]
    fn load_mixed_assert_and_defrule() {
        let mut engine = Engine::new(EngineConfig::utf8());
        let source = r#"
            (assert (person Alice))
            (defrule greet (person ?x) => (printout t "Hello " ?x crlf))
            (assert (person Bob))
        "#;
        let result = engine.load_str(source).unwrap();

        assert_eq!(result.asserted_facts.len(), 2);
        assert_eq!(result.rules.len(), 1);
    }

    #[test]
    fn load_deftemplate() {
        let mut engine = Engine::new(EngineConfig::utf8());
        let result = engine
            .load_str("(deftemplate person (slot name))")
            .unwrap();

        assert_eq!(result.templates.len(), 1);
        assert_eq!(result.templates[0].name, "person");
        assert_eq!(result.templates[0].slots.len(), 1);
        assert_eq!(result.templates[0].slots[0].name, "name");
    }

    #[test]
    fn load_unsupported_form_returns_error() {
        let mut engine = Engine::new(EngineConfig::utf8());
        let errors = engine
            .load_str("(deffunction foo () (+ 1 2))")
            .unwrap_err();

        assert_eq!(errors.len(), 1);
        match &errors[0] {
            LoadError::UnsupportedForm { name, .. } => {
                assert_eq!(name, "deffunction");
            }
            _ => panic!("expected UnsupportedForm error"),
        }
    }

    #[test]
    fn load_invalid_assert_empty_fact() {
        let mut engine = Engine::new(EngineConfig::utf8());
        let errors = engine.load_str("(assert ())").unwrap_err();

        assert_eq!(errors.len(), 1);
        assert!(matches!(errors[0], LoadError::InvalidAssert(_)));
    }

    #[test]
    fn load_invalid_assert_non_symbol_relation() {
        let mut engine = Engine::new(EngineConfig::utf8());
        let errors = engine.load_str("(assert (42 value))").unwrap_err();

        assert_eq!(errors.len(), 1);
        assert!(matches!(errors[0], LoadError::InvalidAssert(_)));
    }

    #[test]
    fn load_invalid_defrule_missing_name() {
        let mut engine = Engine::new(EngineConfig::utf8());
        let errors = engine.load_str("(defrule)").unwrap_err();

        assert_eq!(errors.len(), 1);
        assert!(matches!(errors[0], LoadError::Interpret(_)));
    }

    #[test]
    fn load_invalid_defrule_missing_arrow() {
        let mut engine = Engine::new(EngineConfig::utf8());
        let errors = engine
            .load_str("(defrule test (person ?x) (printout t ?x))")
            .unwrap_err();

        assert_eq!(errors.len(), 1);
        assert!(matches!(errors[0], LoadError::Interpret(_)));
    }

    #[test]
    fn load_invalid_defrule_non_symbol_name() {
        let mut engine = Engine::new(EngineConfig::utf8());
        let errors = engine
            .load_str("(defrule 123 (person ?x) => (printout t ?x))")
            .unwrap_err();

        assert_eq!(errors.len(), 1);
        assert!(matches!(errors[0], LoadError::Interpret(_)));
    }

    #[test]
    fn load_parse_error() {
        let mut engine = Engine::new(EngineConfig::utf8());
        let errors = engine.load_str("(assert (person)").unwrap_err();

        assert_eq!(errors.len(), 1);
        assert!(matches!(errors[0], LoadError::Parse(_)));
    }

    #[test]
    fn load_deffacts() {
        let mut engine = Engine::new(EngineConfig::utf8());
        let source = r"
            (deffacts startup
                (person Alice)
                (person Bob))
        ";
        let result = engine.load_str(source).unwrap();

        assert_eq!(result.asserted_facts.len(), 2);
        assert!(result.rules.is_empty());
    }

    #[test]
    fn load_nested_fact_produces_warning() {
        let mut engine = Engine::new(EngineConfig::utf8());
        let source = r#"(assert (person (name "John") (age 30)))"#;
        let result = engine.load_str(source).unwrap();

        // The nested lists will be skipped with warnings
        assert_eq!(result.asserted_facts.len(), 1);
        assert!(!result.warnings.is_empty());
    }

    #[test]
    fn load_encoding_error_produces_warning() {
        let mut engine = Engine::new(EngineConfig::ascii());
        let source = "(assert (person \"héllo\"))";
        let result = engine.load_str(source).unwrap();

        // The invalid string should produce a warning and be skipped
        assert_eq!(result.asserted_facts.len(), 1);
        assert!(!result.warnings.is_empty());
    }

    #[test]
    fn load_file_reads_from_disk() {
        use std::io::Write;
        let mut temp = tempfile::NamedTempFile::new().unwrap();
        write!(temp, "(assert (test 123))").unwrap();

        let mut engine = Engine::new(EngineConfig::utf8());
        let result = engine.load_file(temp.path()).unwrap();

        assert_eq!(result.asserted_facts.len(), 1);
    }

    #[test]
    fn load_nonexistent_file_returns_error() {
        let mut engine = Engine::new(EngineConfig::utf8());
        let errors = engine
            .load_file(Path::new("/nonexistent/path"))
            .unwrap_err();

        assert_eq!(errors.len(), 1);
        assert!(matches!(errors[0], LoadError::Io(_)));
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::config::EngineConfig;
    use proptest::prelude::*;

    proptest! {
        /// Any valid assert form should produce at least one fact.
        #[test]
        fn valid_assert_produces_facts(
            relation in "[a-z][a-z0-9]{0,10}",
            values in prop::collection::vec(0i64..=100, 0..5)
        ) {
            let mut engine = Engine::new(EngineConfig::utf8());
            let fields = values.iter()
                .map(std::string::ToString::to_string)
                .collect::<Vec<_>>()
                .join(" ");
            let source = format!("(assert ({relation} {fields}))");

            if let Ok(result) = engine.load_str(&source) {
                prop_assert!(!result.asserted_facts.is_empty(),
                    "Valid assert should produce facts: {}", source);
            }
        }

        /// Rule name should be preserved in RuleDef.
        #[test]
        fn rule_name_preserved(
            name in "[a-z][a-z0-9-]{0,15}",
        ) {
            let mut engine = Engine::new(EngineConfig::utf8());
            let source = format!("(defrule {name} (test ?x) => (assert (result ?x)))");

            if let Ok(result) = engine.load_str(&source) {
                prop_assert_eq!(result.rules.len(), 1);
                prop_assert_eq!(&result.rules[0].name, &name);
            }
        }

        /// The loader should never panic on arbitrary input.
        #[test]
        fn loader_never_panics(source in "[\\x20-\\x7e]{0,200}") {
            let mut engine = Engine::new(EngineConfig::utf8());
            let _ = engine.load_str(&source);
        }
    }
}
