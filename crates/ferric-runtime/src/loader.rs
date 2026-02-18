//! Source code loader for CLIPS-compatible syntax.
//!
//! This module provides functionality to load CLIPS source code from strings
//! or files and convert it into engine-level constructs. Phase 1 supports:
//! - `(assert ...)` forms — load facts into working memory
//! - `(defrule ...)` forms — store raw rule definitions for later compilation
//!
//! Full rule compilation and pattern matching will be added in later phases.

use std::collections::HashMap;
use std::path::Path;
use thiserror::Error;

use ferric_core::{
    AlphaEntryType, AtomKey, CompilableCondition, CompilablePattern, CompileResult, ConstantTest,
    ConstantTestType, FactId, FerricString, RuleId, SlotIndex, Value,
};
use ferric_parser::{
    interpret_constructs, parse_sexprs, Atom, Constraint, Construct, FactBody, FactValue, FileId,
    InterpreterConfig, LiteralKind, OrderedFactBody, Pattern, RuleConstruct, SExpr,
    TemplateConstruct, TemplateFactBody,
};

use crate::actions::CompiledRuleInfo;
use crate::engine::{Engine, EngineError};

/// Translated rule data including fact-address variable bindings.
struct TranslatedRule {
    rule_id: RuleId,
    salience: i32,
    conditions: Vec<CompilableCondition>,
    fact_address_vars: HashMap<String, usize>,
}

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

    #[error("compile error: {0}")]
    Compile(String),

    #[error("pattern validation failed")]
    Validation(Vec<ferric_core::PatternValidationError>),

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
                    let (name, line, column) = if let Some(head) = list.first() {
                        (
                            head.as_symbol().unwrap_or("<non-symbol>").to_string(),
                            head.span().start.line,
                            head.span().start.column,
                        )
                    } else {
                        (
                            "<empty-list>".to_string(),
                            expr.span().start.line,
                            expr.span().start.column,
                        )
                    };
                    errors.push(LoadError::UnsupportedForm { name, line, column });
                }
            } else {
                result.warnings.push(format!(
                    "skipping non-list top-level form at line {}",
                    expr.span().start.line
                ));
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

            // Collect constructs by type (don't assert deffacts yet)
            let mut deffacts_constructs = Vec::new();
            for construct in interpret_result.constructs {
                match construct {
                    Construct::Rule(rule) => {
                        result.rules.push(rule);
                    }
                    Construct::Template(template) => {
                        result.templates.push(template);
                    }
                    Construct::Facts(facts) => {
                        deffacts_constructs.push(facts);
                    }
                }
            }

            // Compile rules first so rete has patterns before facts arrive
            for rule in &result.rules {
                match self.compile_rule_construct(rule) {
                    Ok(_) => {}
                    Err(e) => errors.push(e),
                }
            }

            // Now process deffacts (facts will flow through compiled rete via assert_ordered)
            for facts in &deffacts_constructs {
                if let Err(e) = self.process_deffacts_construct(facts, &mut result) {
                    errors.push(e);
                }
            }
        }

        // Process assert forms AFTER rules are compiled so facts flow through rete
        for expr in &assert_forms {
            if let Some(list) = expr.as_list() {
                if let Err(e) = self.process_assert(&list[1..], &mut result) {
                    errors.push(e);
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
        let mut constructed_facts = Vec::new();
        for fact_body in &facts_construct.facts {
            let fact_id = self.process_fact_body(fact_body, result)?;
            result.asserted_facts.push(fact_id);
            // Collect the fact for deffacts registration
            if let Some(entry) = self.fact_base.get(fact_id) {
                constructed_facts.push(entry.fact.clone());
            }
        }
        // Register for reset
        self.registered_deffacts.push(constructed_facts);
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
    fn fact_value_to_value(
        &mut self,
        fact_value: &FactValue,
        result: &mut LoadResult,
    ) -> Option<Value> {
        match fact_value {
            FactValue::Literal(lit) => {
                self.literal_to_value(&lit.value, lit.span.start.line, result)
            }
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
            LiteralKind::String(s) => self.warned_string_value(s, line, result),
            LiteralKind::Symbol(s) => self.warned_symbol_value(s, line, result),
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
        let line = expr.span().start.line;

        match atom {
            Atom::Integer(n) => Some(Value::Integer(*n)),
            Atom::Float(f) => Some(Value::Float(*f)),
            Atom::String(s) => self.warned_string_value(s, line, result),
            Atom::Symbol(s) => self.warned_symbol_value(s, line, result),
            // Variables and connectives are not supported as fact values in Phase 1
            Atom::SingleVar(_) | Atom::MultiVar(_) | Atom::GlobalVar(_) | Atom::Connective(_) => {
                None
            }
        }
    }

    fn warned_string_value(
        &self,
        value: &str,
        line: u32,
        result: &mut LoadResult,
    ) -> Option<Value> {
        match FerricString::new(value, self.config.string_encoding) {
            Ok(fs) => Some(Value::String(fs)),
            Err(error) => {
                Self::warn_with_detail(result, line, "string encoding error", &error);
                None
            }
        }
    }

    fn warned_symbol_value(
        &mut self,
        symbol: &str,
        line: u32,
        result: &mut LoadResult,
    ) -> Option<Value> {
        match self
            .symbol_table
            .intern_symbol(symbol, self.config.string_encoding)
        {
            Ok(sym) => Some(Value::Symbol(sym)),
            Err(error) => {
                Self::warn_with_detail(result, line, "symbol encoding error", &error);
                None
            }
        }
    }

    // -----------------------------------------------------------------------
    // Rule compilation pipeline
    // -----------------------------------------------------------------------

    /// Compile a `RuleConstruct` into the engine's rete network.
    fn compile_rule_construct(&mut self, rule: &RuleConstruct) -> Result<CompileResult, LoadError> {
        // Validate patterns first (max nesting depth: 2)
        let validation_errors = validate_rule_patterns(&rule.patterns, 2);
        if !validation_errors.is_empty() {
            return Err(LoadError::Validation(validation_errors));
        }

        let translated = self
            .translate_rule_construct(rule)
            .map_err(|e| LoadError::Compile(format!("{e}")))?;

        let compile_result = self
            .compiler
            .compile_conditions(
                &mut self.rete,
                translated.rule_id,
                translated.salience,
                &translated.conditions,
            )
            .map_err(|e| LoadError::Compile(format!("{e}")))?;

        // Store rule info for action execution
        let info = CompiledRuleInfo {
            name: rule.name.clone(),
            actions: rule.actions.clone(),
            var_map: compile_result.var_map.clone(),
            fact_address_vars: translated.fact_address_vars,
            salience: rule.salience,
        };
        self.rule_info.insert(compile_result.rule_id, info);

        Ok(compile_result)
    }

    /// Translate a `RuleConstruct` (parser types) into a `CompilableRule` (core types).
    fn translate_rule_construct(
        &mut self,
        rule: &RuleConstruct,
    ) -> Result<TranslatedRule, LoadError> {
        let rule_id = self.compiler.allocate_rule_id();
        let mut conditions = Vec::new();
        let mut fact_address_vars = HashMap::new();
        let mut fact_index = 0usize;

        for pattern in &rule.patterns {
            // Check for Pattern::Assigned to track fact-address variables
            let (var_name, is_negated) = match pattern {
                Pattern::Assigned {
                    variable,
                    pattern: inner,
                    ..
                } => {
                    // Check if inner is negated (which wouldn't make sense for fact address)
                    let negated = matches!(inner.as_ref(), Pattern::Not(..));
                    (Some(variable.clone()), negated)
                }
                Pattern::Not(..) => (None, true),
                _ => (None, false),
            };

            let condition = self.translate_condition(pattern)?;
            if let Some(name) = var_name {
                if !is_negated && Self::condition_has_fact_address(&condition) {
                    fact_address_vars.insert(name, fact_index);
                }
            }
            if Self::condition_has_fact_address(&condition) {
                fact_index += 1;
            }
            conditions.push(condition);
        }

        Ok(TranslatedRule {
            rule_id,
            salience: rule.salience,
            conditions,
            fact_address_vars,
        })
    }

    fn condition_has_fact_address(condition: &CompilableCondition) -> bool {
        match condition {
            CompilableCondition::Pattern(pattern) => !pattern.negated && !pattern.exists,
            CompilableCondition::Ncc(_) => false,
        }
    }

    fn translate_condition(&mut self, pattern: &Pattern) -> Result<CompilableCondition, LoadError> {
        match pattern {
            Pattern::Assigned { pattern, .. } => self.translate_condition(pattern),
            Pattern::Not(inner, span) => {
                if let Pattern::And(inner_patterns, and_span) = inner.as_ref() {
                    if inner_patterns.is_empty() {
                        return Err(Self::unsupported_pattern(
                            "not/and",
                            span,
                            "not(and ...) requires at least one inner pattern",
                        ));
                    }

                    let mut subpatterns = Vec::with_capacity(inner_patterns.len());
                    for sub in inner_patterns {
                        let translated = self.translate_pattern(sub)?;
                        if translated.negated {
                            return Err(Self::unsupported_pattern(
                                "not/and",
                                and_span,
                                "nested negation inside and is not supported yet",
                            ));
                        }
                        subpatterns.push(translated);
                    }
                    Ok(CompilableCondition::Ncc(subpatterns))
                } else {
                    let mut compilable = self.translate_pattern(inner)?;
                    compilable.negated = true;
                    Ok(CompilableCondition::Pattern(compilable))
                }
            }
            Pattern::And(_, span) => Err(Self::unsupported_pattern(
                "and",
                span,
                "standalone and conditional elements are not supported; use (not (and ...))",
            )),
            _ => Ok(CompilableCondition::Pattern(
                self.translate_pattern(pattern)?,
            )),
        }
    }

    /// Translate a single `Pattern` into a `CompilablePattern`.
    fn translate_pattern(&mut self, pattern: &Pattern) -> Result<CompilablePattern, LoadError> {
        match pattern {
            Pattern::Ordered(ordered) => {
                let sym = self.compile_symbol(&ordered.relation)?;
                let entry_type = AlphaEntryType::OrderedRelation(sym);
                let mut constant_tests = Vec::new();
                let mut variable_slots = Vec::new();

                for (i, constraint) in ordered.constraints.iter().enumerate() {
                    let slot = SlotIndex::Ordered(i);
                    self.translate_constraint(
                        constraint,
                        slot,
                        &mut constant_tests,
                        &mut variable_slots,
                    )?;
                }

                Ok(CompilablePattern {
                    entry_type,
                    constant_tests,
                    variable_slots,
                    negated: false,
                    exists: false,
                })
            }
            Pattern::Assigned { pattern, .. } => {
                // Unwrap the assignment and compile the inner pattern
                self.translate_pattern(pattern)
            }
            Pattern::Not(inner, _span) => {
                // Unwrap the inner pattern and set negated flag
                let mut compilable = self.translate_pattern(inner)?;
                compilable.negated = true;
                Ok(compilable)
            }
            Pattern::Exists(patterns, span) => {
                // For single-pattern exists, compile as an exists pattern
                if patterns.len() == 1 {
                    let mut compilable = self.translate_pattern(&patterns[0])?;
                    compilable.exists = true;
                    Ok(compilable)
                } else {
                    Err(Self::unsupported_pattern(
                        "exists",
                        span,
                        &format!(
                            "multi-pattern exists is not supported yet (received {} patterns)",
                            patterns.len()
                        ),
                    ))
                }
            }
            Pattern::Test(_, span) => Err(Self::unsupported_pattern(
                "test",
                span,
                "test conditional elements are not supported yet",
            )),
            Pattern::Template(template) => Err(Self::unsupported_pattern(
                "template",
                &template.span,
                &format!(
                    "template pattern `{}` is not supported yet",
                    template.template
                ),
            )),
            Pattern::And(_, span) => Err(Self::unsupported_pattern(
                "and",
                span,
                "and conditional elements are only supported inside (not (and ...))",
            )),
        }
    }

    /// Translate a single `Constraint` into constant tests and/or variable slots.
    fn translate_constraint(
        &mut self,
        constraint: &Constraint,
        slot: SlotIndex,
        constant_tests: &mut Vec<ConstantTest>,
        variable_slots: &mut Vec<(SlotIndex, ferric_core::Symbol)>,
    ) -> Result<(), LoadError> {
        match constraint {
            Constraint::Literal(lit) => {
                if let Some(key) = self.literal_to_atom_key(&lit.value)? {
                    constant_tests.push(ConstantTest {
                        slot,
                        test_type: ConstantTestType::Equal(key),
                    });
                }
            }
            Constraint::Variable(name, _span) => {
                let sym = self.compile_symbol(name)?;
                variable_slots.push((slot, sym));
            }
            Constraint::Wildcard(_) | Constraint::MultiWildcard(_) => {
                // No test needed — matches anything
            }
            Constraint::MultiVariable(name, span) => {
                return Err(Self::unsupported_constraint(
                    "multi-variable",
                    span,
                    &format!("multi-field variable `$?{name}` is not supported yet"),
                ));
            }
            Constraint::Not(inner, span) => {
                // ~literal → NotEqual constant test
                if let Constraint::Literal(lit) = inner.as_ref() {
                    if let Some(key) = self.literal_to_atom_key(&lit.value)? {
                        constant_tests.push(ConstantTest {
                            slot,
                            test_type: ConstantTestType::NotEqual(key),
                        });
                    }
                } else {
                    return Err(Self::unsupported_constraint(
                        "not",
                        span,
                        "only negated literals (~<literal>) are supported",
                    ));
                }
            }
            Constraint::And(constraints, _span) => {
                // Process each sub-constraint against the same slot
                for sub in constraints {
                    self.translate_constraint(sub, slot, constant_tests, variable_slots)?;
                }
            }
            Constraint::Or(_constraints, span) => {
                return Err(Self::unsupported_constraint(
                    "or",
                    span,
                    "or constraints are not supported yet",
                ));
            }
        }
        Ok(())
    }

    /// Convert a `LiteralKind` to an `AtomKey` for constant test matching.
    fn literal_to_atom_key(&mut self, literal: &LiteralKind) -> Result<Option<AtomKey>, LoadError> {
        match literal {
            LiteralKind::Integer(n) => Ok(Some(AtomKey::Integer(*n))),
            LiteralKind::Float(f) => Ok(Some(AtomKey::FloatBits(f.to_bits()))),
            LiteralKind::Symbol(s) => {
                let sym = self.compile_symbol(s)?;
                Ok(Some(AtomKey::Symbol(sym)))
            }
            LiteralKind::String(s) => {
                let fs = self.compile_string(s)?;
                Ok(Some(AtomKey::String(fs)))
            }
        }
    }

    fn compile_encoding_error(error: impl std::fmt::Display) -> LoadError {
        LoadError::Compile(format!("encoding error: {error}"))
    }

    fn compile_symbol(&mut self, symbol: &str) -> Result<ferric_core::Symbol, LoadError> {
        self.symbol_table
            .intern_symbol(symbol, self.config.string_encoding)
            .map_err(Self::compile_encoding_error)
    }

    fn compile_string(&self, value: &str) -> Result<FerricString, LoadError> {
        FerricString::new(value, self.config.string_encoding).map_err(Self::compile_encoding_error)
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

    fn unsupported_pattern(kind: &str, span: &ferric_parser::Span, detail: &str) -> LoadError {
        Self::unsupported_compile_form("pattern", kind, span, detail)
    }

    fn unsupported_constraint(kind: &str, span: &ferric_parser::Span, detail: &str) -> LoadError {
        Self::unsupported_compile_form("constraint", kind, span, detail)
    }

    fn unsupported_compile_form(
        category: &str,
        kind: &str,
        span: &ferric_parser::Span,
        detail: &str,
    ) -> LoadError {
        LoadError::Compile(format!(
            "unsupported {category} form `{kind}` at line {}, column {}: {detail}",
            span.start.line, span.start.column
        ))
    }
}

// ============================================================================
// Pattern Validation
// ============================================================================

/// Validate rule patterns before Rete compilation.
///
/// Checks pattern restrictions according to Section 7.7 of the implementation plan:
/// - E0001: Nesting depth limit (not/exists)
/// - E0005: Unsupported nesting combinations (exists containing not)
///
/// Returns a vector of validation errors. Empty vector means validation passed.
fn validate_rule_patterns(
    patterns: &[Pattern],
    max_nesting_depth: usize,
) -> Vec<ferric_core::PatternValidationError> {
    let mut errors = Vec::new();
    for pattern in patterns {
        validate_pattern_recursive(pattern, 0, max_nesting_depth, &mut errors);
    }
    errors
}

/// Recursively validate a pattern and its nested children.
///
/// # Arguments
/// * `pattern` - The pattern to validate
/// * `depth` - Current nesting depth (0 at top level)
/// * `max_depth` - Maximum allowed nesting depth
/// * `errors` - Accumulator for validation errors
fn validate_pattern_recursive(
    pattern: &Pattern,
    depth: usize,
    max_depth: usize,
    errors: &mut Vec<ferric_core::PatternValidationError>,
) {
    match pattern {
        Pattern::Not(inner, span) => {
            let new_depth = depth + 1;
            if new_depth > max_depth {
                let kind = ferric_core::PatternViolation::NestingTooDeep {
                    depth: new_depth,
                    max: max_depth,
                };
                let location = Some(span_to_source_location(span));
                let error = ferric_core::PatternValidationError::new(
                    kind,
                    location,
                    ferric_core::ValidationStage::ReteCompilation,
                );
                errors.push(error);
            }
            // Continue validating the inner pattern regardless of depth violation
            validate_pattern_recursive(inner, new_depth, max_depth, errors);
        }

        Pattern::Exists(inner_patterns, span) => {
            let new_depth = depth + 1;
            if new_depth > max_depth {
                let kind = ferric_core::PatternViolation::NestingTooDeep {
                    depth: new_depth,
                    max: max_depth,
                };
                let location = Some(span_to_source_location(span));
                let error = ferric_core::PatternValidationError::new(
                    kind,
                    location,
                    ferric_core::ValidationStage::ReteCompilation,
                );
                errors.push(error);
            }

            // Check for unsupported combination: exists containing not
            for inner in inner_patterns {
                if matches!(inner, Pattern::Not(..)) {
                    let kind = ferric_core::PatternViolation::UnsupportedNestingCombination {
                        description: "exists containing not is not supported".to_string(),
                    };
                    let location = Some(span_to_source_location(span));
                    let error = ferric_core::PatternValidationError::new(
                        kind,
                        location,
                        ferric_core::ValidationStage::ReteCompilation,
                    );
                    errors.push(error);
                }
                validate_pattern_recursive(inner, new_depth, max_depth, errors);
            }
        }

        Pattern::Assigned { pattern: inner, .. } => {
            // Assigned pattern: unwrap and validate the inner pattern
            validate_pattern_recursive(inner, depth, max_depth, errors);
        }

        Pattern::And(inner_patterns, _) => {
            for inner in inner_patterns {
                validate_pattern_recursive(inner, depth, max_depth, errors);
            }
        }

        Pattern::Ordered(..) | Pattern::Template(..) | Pattern::Test(..) => {
            // Leaf patterns - nothing to validate at this level
        }
    }
}

/// Convert a parser `Span` to a core `SourceLocation`.
fn span_to_source_location(span: &ferric_parser::Span) -> ferric_core::SourceLocation {
    ferric_core::SourceLocation::new(
        span.start.line,
        span.start.column,
        span.end.line,
        span.end.column,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EngineConfig;
    use ferric_core::Fact;

    fn assert_single_compile_error_contains(errors: &[LoadError], expected: &str) {
        assert_eq!(
            errors.len(),
            1,
            "expected exactly one error, got {errors:?}"
        );
        match &errors[0] {
            LoadError::Compile(message) => {
                assert!(
                    message.contains(expected),
                    "expected compile error to contain `{expected}`, got `{message}`"
                );
            }
            other => panic!("expected LoadError::Compile, got {other:?}"),
        }
    }

    fn test_span(line: u32, column: u32) -> ferric_parser::Span {
        let pos = ferric_parser::Position {
            offset: 0,
            line,
            column,
        };
        ferric_parser::Span::new(pos, pos, FileId(0))
    }

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
    fn load_rule_with_test_pattern_returns_compile_error() {
        let mut engine = Engine::new(EngineConfig::utf8());
        let errors = engine
            .load_str("(defrule t (test (> 1 0)) => (assert (ok)))")
            .unwrap_err();

        assert_single_compile_error_contains(&errors, "unsupported pattern form `test`");
    }

    #[test]
    fn load_rule_with_template_pattern_returns_compile_error() {
        let mut engine = Engine::new(EngineConfig::utf8());
        let errors = engine
            .load_str("(defrule t (person (name Alice)) => (assert (ok)))")
            .unwrap_err();

        assert_single_compile_error_contains(&errors, "unsupported pattern form `template`");
    }

    #[test]
    fn load_rule_with_multi_pattern_exists_returns_compile_error() {
        let mut engine = Engine::new(EngineConfig::utf8());
        let errors = engine
            .load_str("(defrule t (exists (a) (b)) => (assert (ok)))")
            .unwrap_err();

        assert_single_compile_error_contains(&errors, "unsupported pattern form `exists`");
    }

    #[test]
    fn load_rule_with_not_and_compiles() {
        let mut engine = Engine::new(EngineConfig::utf8());
        let result = engine.load_str(
            "(defrule t (item ?x) (not (and (block ?x) (reason ?x))) => (assert (ok ?x)))",
        );

        assert!(result.is_ok(), "not(and ...) should compile in Phase 2");
    }

    #[test]
    fn load_rule_with_multivariable_constraint_returns_compile_error() {
        let mut engine = Engine::new(EngineConfig::utf8());
        let errors = engine
            .load_str("(defrule t (items $?values) => (assert (ok)))")
            .unwrap_err();

        assert_single_compile_error_contains(
            &errors,
            "unsupported constraint form `multi-variable`",
        );
    }

    #[test]
    fn translate_or_constraint_returns_compile_error() {
        let mut engine = Engine::new(EngineConfig::utf8());
        let mut constant_tests = Vec::new();
        let mut variable_slots = Vec::new();
        let span = test_span(9, 4);
        let constraint = Constraint::Or(Vec::new(), span);

        let error = engine
            .translate_constraint(
                &constraint,
                SlotIndex::Ordered(0),
                &mut constant_tests,
                &mut variable_slots,
            )
            .unwrap_err();

        match error {
            LoadError::Compile(message) => {
                assert!(message.contains("unsupported constraint form `or`"));
                assert!(message.contains("line 9, column 4"));
            }
            other => panic!("expected compile error, got {other:?}"),
        }
    }

    #[test]
    fn translate_negated_non_literal_constraint_returns_compile_error() {
        let mut engine = Engine::new(EngineConfig::utf8());
        let mut constant_tests = Vec::new();
        let mut variable_slots = Vec::new();
        let outer_span = test_span(7, 2);
        let inner_span = test_span(7, 3);
        let constraint = Constraint::Not(
            Box::new(Constraint::Variable("x".to_string(), inner_span)),
            outer_span,
        );

        let error = engine
            .translate_constraint(
                &constraint,
                SlotIndex::Ordered(0),
                &mut constant_tests,
                &mut variable_slots,
            )
            .unwrap_err();

        match error {
            LoadError::Compile(message) => {
                assert!(message.contains("unsupported constraint form `not`"));
                assert!(message.contains("line 7, column 2"));
            }
            other => panic!("expected compile error, got {other:?}"),
        }
    }

    #[test]
    fn translate_negated_literal_constraint_still_compiles() {
        let mut engine = Engine::new(EngineConfig::utf8());
        let mut constant_tests = Vec::new();
        let mut variable_slots = Vec::new();
        let span = test_span(3, 8);
        let literal = ferric_parser::LiteralValue {
            value: LiteralKind::Integer(42),
            span,
        };
        let constraint = Constraint::Not(Box::new(Constraint::Literal(literal)), span);

        engine
            .translate_constraint(
                &constraint,
                SlotIndex::Ordered(0),
                &mut constant_tests,
                &mut variable_slots,
            )
            .unwrap();

        assert_eq!(constant_tests.len(), 1);
        assert!(matches!(
            constant_tests[0].test_type,
            ConstantTestType::NotEqual(AtomKey::Integer(42))
        ));
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
        let result = engine.load_str("(deftemplate person (slot name))").unwrap();

        assert_eq!(result.templates.len(), 1);
        assert_eq!(result.templates[0].name, "person");
        assert_eq!(result.templates[0].slots.len(), 1);
        assert_eq!(result.templates[0].slots[0].name, "name");
    }

    #[test]
    fn load_unsupported_form_returns_error() {
        let mut engine = Engine::new(EngineConfig::utf8());
        let errors = engine.load_str("(deffunction foo () (+ 1 2))").unwrap_err();

        assert_eq!(errors.len(), 1);
        match &errors[0] {
            LoadError::UnsupportedForm { name, .. } => {
                assert_eq!(name, "deffunction");
            }
            _ => panic!("expected UnsupportedForm error"),
        }
    }

    #[test]
    fn load_empty_top_level_list_returns_error_not_panic() {
        let mut engine = Engine::new(EngineConfig::utf8());
        let errors = engine.load_str("()").unwrap_err();

        assert_eq!(errors.len(), 1);
        match &errors[0] {
            LoadError::UnsupportedForm { name, line, column } => {
                assert_eq!(name, "<empty-list>");
                assert_eq!((*line, *column), (1, 1));
            }
            other => panic!("expected UnsupportedForm, got {other:?}"),
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
    fn load_invalid_defrule_not_with_multiple_patterns() {
        let mut engine = Engine::new(EngineConfig::utf8());
        let errors = engine
            .load_str("(defrule test (not (a) (b)) => (printout t ok))")
            .unwrap_err();

        assert_eq!(errors.len(), 1);
        match &errors[0] {
            LoadError::Interpret(message) => {
                assert!(message.contains("expected exactly one pattern"));
                assert!(message.contains("line 1, column "));
            }
            other => panic!("expected interpret error, got {other:?}"),
        }
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
            let source = format!("(defrule {name} (item ?x) => (assert (result ?x)))");

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
