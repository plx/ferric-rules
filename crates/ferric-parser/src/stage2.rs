//! Stage 2: Construct AST and semantic interpretation.
//!
//! Stage 2 transforms the S-expression trees produced by Stage 1 into typed
//! construct representations for `deftemplate`, `defrule`, `deffacts`,
//! `deffunction`, and `defglobal`.
//! Source spans are preserved through the transformation for diagnostics.
//!
//! ## Phase 2 complete
//!
//! - Full interpretation for `deftemplate`, `defrule`, `deffacts`.
//! - Typed AST with patterns, constraints, actions, slot definitions, and
//!   fact bodies.
//!
//! ## Phase 3 scope
//!
//! - `deffunction` and `defglobal` interpretation (Pass 005).
//! - Add interpretation for `defmodule`, `defgeneric`, `defmethod`.

use crate::sexpr::{Atom, Connective, SExpr};
use crate::span::Span;
use std::fmt;

// ============================================================================
// Pattern types for rule LHS
// ============================================================================

/// A pattern in a rule's LHS.
#[derive(Clone, Debug)]
pub enum Pattern {
    /// Ordered fact pattern: (relation constraint ...)
    Ordered(OrderedPattern),
    /// Template fact pattern: (template (slot-name constraint) ...)
    Template(TemplatePattern),
    /// Conjunction CE: (and <pattern> <pattern> ...)
    And(Vec<Pattern>, Span),
    /// Negation CE: (not <pattern>)
    Not(Box<Pattern>, Span),
    /// Test CE: (test <expression>) -- kept as raw `SExpr` for Phase 2
    Test(SExpr, Span),
    /// Exists CE: (exists <pattern> ...)
    Exists(Vec<Pattern>, Span),
    /// Forall CE: (forall <condition> <then-clause>...)
    Forall(Vec<Pattern>, Span),
    /// Logical CE: (logical <pattern> ...) — truth maintenance wrapper
    Logical(Vec<Pattern>, Span),
    /// Disjunction CE: (or <pattern> <pattern> ...)
    Or(Vec<Pattern>, Span),
    /// Assigned pattern: ?var <- <pattern>
    Assigned {
        variable: String,
        pattern: Box<Pattern>,
        span: Span,
    },
}

#[derive(Clone, Debug)]
pub struct OrderedPattern {
    pub relation: String,
    pub constraints: Vec<Constraint>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct TemplatePattern {
    pub template: String,
    pub slot_constraints: Vec<SlotConstraint>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct SlotConstraint {
    pub slot_name: String,
    pub constraint: Constraint,
    pub span: Span,
}

// ============================================================================
// Constraint types
// ============================================================================

/// A constraint on a pattern field or slot.
#[derive(Clone, Debug)]
pub enum Constraint {
    /// Literal value
    Literal(LiteralValue),
    /// Single-field variable: ?x
    Variable(String, Span),
    /// Multi-field variable: $?x
    MultiVariable(String, Span),
    /// Wildcard: ? (matches any single value)
    Wildcard(Span),
    /// Multi-field wildcard: $? (matches zero or more values)
    MultiWildcard(Span),
    /// Predicate constraint: :(<expr>)
    Predicate(SExpr, Span),
    /// Return-value constraint: =(<expr>)
    ReturnValue(SExpr, Span),
    /// Negation: ~<constraint>
    Not(Box<Constraint>, Span),
    /// Conjunction: constraint & constraint
    And(Vec<Constraint>, Span),
    /// Disjunction: constraint | constraint
    Or(Vec<Constraint>, Span),
}

/// A literal value in a pattern or fact body.
#[derive(Clone, Debug)]
pub struct LiteralValue {
    pub value: LiteralKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum LiteralKind {
    Integer(i64),
    Float(f64),
    String(String),
    Symbol(String),
}

// ============================================================================
// Action types for rule RHS
// ============================================================================

/// An action in a rule's RHS.
#[derive(Clone, Debug)]
pub struct Action {
    pub call: FunctionCall,
}

#[derive(Clone, Debug)]
pub struct FunctionCall {
    pub name: String,
    pub args: Vec<ActionExpr>,
    pub span: Span,
}

const FACT_SLOT_REF_FN: &str = "__fact_slot_ref";

#[derive(Clone, Debug)]
pub enum ActionExpr {
    Literal(LiteralValue),
    Variable(String, Span),
    GlobalVariable(String, Span),
    FunctionCall(FunctionCall),
    /// CLIPS `(if <condition> then <action>* [else <action>*])` form.
    If {
        condition: Box<ActionExpr>,
        then_actions: Vec<ActionExpr>,
        else_actions: Vec<ActionExpr>,
        span: Span,
    },
    /// CLIPS `(while <condition> do <action>*)` loop form.
    While {
        condition: Box<ActionExpr>,
        body: Vec<ActionExpr>,
        span: Span,
    },
    /// CLIPS `(loop-for-count (?var <start> <end>) do <action>*)` loop form.
    ///
    /// If `var_name` is `None`, no variable is bound (anonymous counter).
    LoopForCount {
        var_name: Option<String>,
        start: Box<ActionExpr>,
        end: Box<ActionExpr>,
        body: Vec<ActionExpr>,
        span: Span,
    },
    /// CLIPS `(progn$ (?var <multifield-expr>) <action>*)` /
    /// `(foreach ?var <multifield-expr> do <action>*)` form.
    ///
    /// Iterates over each element of a multifield value, binding `var_name`
    /// to each element and `<var_name>-index` to the 1-based iteration index.
    Progn {
        var_name: String,
        list_expr: Box<ActionExpr>,
        body: Vec<ActionExpr>,
        span: Span,
    },
    /// CLIPS fact-query macro forms:
    /// - `(do-for-fact ((?v tmpl)) <query> <action>*)`
    /// - `(do-for-all-facts ((?v tmpl)) <query> <action>*)`
    /// - `(delayed-do-for-all-facts ((?v tmpl)) <query> <action>*)`
    /// - `(any-factp ((?v tmpl)) <query>)` — no body
    /// - `(find-fact ((?v tmpl)) <query>)` — no body
    /// - `(find-all-facts ((?v tmpl)) <query>)` — no body
    ///
    /// `name` is the macro name. `bindings` lists `(variable, template)` pairs.
    /// `query` is the filter expression. `body` holds body actions (empty for
    /// `any-factp`, `find-fact`, and `find-all-facts`).
    QueryAction {
        /// The specific macro name (e.g. `"do-for-fact"`).
        name: String,
        /// Binding specifications: `(variable_name, template_name)` pairs.
        bindings: Vec<(String, String)>,
        /// Query expression evaluated against each candidate fact.
        query: Box<ActionExpr>,
        /// Body actions executed for matching facts (empty for query-only forms).
        body: Vec<ActionExpr>,
        span: Span,
    },
    /// CLIPS `(switch <expr> (case <value> then <action>*) ... [(default <action>*)])` form.
    Switch {
        expr: Box<ActionExpr>,
        cases: Vec<(ActionExpr, Vec<ActionExpr>)>,
        default: Option<Vec<ActionExpr>>,
        span: Span,
    },
}

// ============================================================================
// Slot definition types for deftemplate
// ============================================================================

#[derive(Clone, Debug)]
pub struct SlotDefinition {
    pub name: String,
    pub slot_type: SlotType,
    pub default: Option<DefaultValue>,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotType {
    Single,
    Multi,
}

#[derive(Clone, Debug)]
pub enum DefaultValue {
    /// (default ?NONE) - field is required
    None,
    /// (default ?DERIVE) - system derives default
    Derive,
    /// (default <value>)
    Value(LiteralValue),
}

// ============================================================================
// Fact body types for deffacts
// ============================================================================

#[derive(Clone, Debug)]
pub enum FactBody {
    Ordered(OrderedFactBody),
    Template(TemplateFactBody),
}

#[derive(Clone, Debug)]
pub struct OrderedFactBody {
    pub relation: String,
    pub values: Vec<FactValue>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct TemplateFactBody {
    pub template: String,
    pub slot_values: Vec<FactSlotValue>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct FactSlotValue {
    pub name: String,
    pub value: FactValue,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum FactValue {
    Literal(LiteralValue),
    Variable(String, Span),
    GlobalVariable(String, Span),
    /// Empty multifield value — represents `(slot-name)` with no values.
    EmptyMultifield(Span),
}

// ============================================================================
// Construct types
// ============================================================================

/// A top-level construct produced by Stage 2 interpretation.
#[derive(Clone, Debug)]
pub enum Construct {
    Rule(RuleConstruct),
    Template(TemplateConstruct),
    Facts(FactsConstruct),
    Function(FunctionConstruct),
    Global(GlobalConstruct),
    Module(ModuleConstruct),
    Generic(GenericConstruct),
    Method(MethodConstruct),
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
    /// LHS patterns (typed).
    pub patterns: Vec<Pattern>,
    /// RHS actions (typed).
    pub actions: Vec<Action>,
}

/// Interpreted `(deftemplate ...)`.
#[derive(Clone, Debug)]
pub struct TemplateConstruct {
    pub name: String,
    pub span: Span,
    pub comment: Option<String>,
    /// Slot definitions (typed).
    pub slots: Vec<SlotDefinition>,
}

/// Interpreted `(deffacts ...)`.
#[derive(Clone, Debug)]
pub struct FactsConstruct {
    pub name: String,
    pub span: Span,
    pub comment: Option<String>,
    /// Fact bodies (typed).
    pub facts: Vec<FactBody>,
}

/// Interpreted `(deffunction ...)`.
#[derive(Clone, Debug)]
pub struct FunctionConstruct {
    /// Function name.
    pub name: String,
    /// Source span of the entire construct.
    pub span: Span,
    /// Optional doc comment.
    pub comment: Option<String>,
    /// Regular parameters (e.g., `?x`, `?y`).  Names do not include the `?` prefix.
    pub parameters: Vec<String>,
    /// Optional wildcard parameter (e.g., `$?rest`).  Name does not include the `$?` prefix.
    pub wildcard_parameter: Option<String>,
    /// Function body expressions (one or more).
    pub body: Vec<ActionExpr>,
}

/// A single global variable definition.
#[derive(Clone, Debug)]
pub struct GlobalDefinition {
    /// Global variable name (without `?*` and `*` delimiters).
    pub name: String,
    /// Source span of this definition (the `?*name*` token).
    pub span: Span,
    /// Initial value expression.
    pub value: ActionExpr,
}

/// Interpreted `(defglobal ...)`.
#[derive(Clone, Debug)]
pub struct GlobalConstruct {
    /// Source span of the entire construct.
    pub span: Span,
    /// The global definitions.
    pub globals: Vec<GlobalDefinition>,
}

/// A module import/export specification.
#[derive(Clone, Debug)]
pub enum ModuleSpec {
    /// Export/import everything (`?ALL`).
    All,
    /// Export/import nothing (`?NONE`).
    None,
    /// Export/import specific constructs of a given type.
    /// e.g., `(export deftemplate reading sensor)` →
    /// `construct_type` = `"deftemplate"`, `names` = `["reading", "sensor"]`
    Specific {
        /// The construct type keyword (e.g., `"deftemplate"`).
        construct_type: String,
        /// The specific construct names.
        names: Vec<String>,
    },
}

/// An import declaration within a defmodule.
#[derive(Clone, Debug)]
pub struct ImportSpec {
    /// The module to import from.
    pub module_name: String,
    /// What to import.
    pub spec: ModuleSpec,
    /// Source span.
    pub span: Span,
}

/// Interpreted `(defmodule ...)`.
#[derive(Clone, Debug)]
pub struct ModuleConstruct {
    /// Module name.
    pub name: String,
    /// Source span of the entire construct.
    pub span: Span,
    /// Optional doc comment.
    pub comment: Option<String>,
    /// Export specifications.
    pub exports: Vec<ModuleSpec>,
    /// Import specifications.
    pub imports: Vec<ImportSpec>,
}

/// Interpreted `(defgeneric ...)`.
#[derive(Clone, Debug)]
pub struct GenericConstruct {
    /// Generic function name.
    pub name: String,
    /// Source span.
    pub span: Span,
    /// Optional doc comment.
    pub comment: Option<String>,
}

/// A method parameter with optional type restriction.
#[derive(Clone, Debug)]
pub struct MethodParameter {
    /// Parameter name (without `?` prefix).
    pub name: String,
    /// Type restrictions (e.g., `["INTEGER"]`, `["INTEGER", "FLOAT"]`), empty = any type.
    pub type_restrictions: Vec<String>,
    /// Source span of this parameter.
    pub span: Span,
}

/// Interpreted `(defmethod ...)`.
#[derive(Clone, Debug)]
pub struct MethodConstruct {
    /// The generic function name this method belongs to.
    pub name: String,
    /// Source span.
    pub span: Span,
    /// Optional method index (explicit dispatch priority).
    pub index: Option<i32>,
    /// Regular parameters with optional type restrictions.
    pub parameters: Vec<MethodParameter>,
    /// Optional wildcard parameter name (without `$?` prefix).
    pub wildcard_parameter: Option<String>,
    /// Method body expressions.
    pub body: Vec<ActionExpr>,
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
        Self::expected_with_kind(what, span, InterpretErrorKind::InvalidStructure)
    }

    /// Creates an error for expecting a top-level construct.
    pub fn expected_construct(what: &str, span: Span) -> Self {
        Self::expected_with_kind(what, span, InterpretErrorKind::ExpectedConstruct)
    }

    fn expected_with_kind(what: &str, span: Span, kind: InterpretErrorKind) -> Self {
        Self {
            message: format!("expected {what}"),
            span,
            kind,
            suggestions: Vec::new(),
        }
    }

    /// Creates an error for an empty construct.
    pub fn empty_construct(span: Span) -> Self {
        Self {
            message: "empty construct".to_string(),
            span,
            kind: InterpretErrorKind::EmptyConstruct,
            suggestions: vec![
                "constructs must have a keyword (defrule, deftemplate, deffacts)".to_string(),
            ],
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

impl std::error::Error for InterpretError {}

/// Result of Stage 2 interpretation.
#[derive(Clone, Debug, Default)]
pub struct InterpretResult {
    pub constructs: Vec<Construct>,
    pub errors: Vec<InterpretError>,
}

fn push_interpret_error(
    result: &mut InterpretResult,
    config: &InterpreterConfig,
    error: InterpretError,
) -> bool {
    result.errors.push(error);
    config.strict
}

/// Interpret a slice of S-expressions into typed constructs.
#[allow(clippy::too_many_lines)] // Dispatch table grows as constructs are added; each arm is small
pub fn interpret_constructs(sexprs: &[SExpr], config: &InterpreterConfig) -> InterpretResult {
    let mut result = InterpretResult::default();

    for sexpr in sexprs {
        // Each top-level element must be a list
        let Some(list) = sexpr.as_list() else {
            if push_interpret_error(
                &mut result,
                config,
                InterpretError::expected_construct(
                    "a construct (list starting with defrule, deftemplate, or deffacts)",
                    sexpr.span(),
                ),
            ) {
                return result;
            }
            continue;
        };

        // List must be non-empty
        if list.is_empty() {
            if push_interpret_error(
                &mut result,
                config,
                InterpretError::empty_construct(sexpr.span()),
            ) {
                return result;
            }
            continue;
        }

        // First element must be a symbol (the keyword)
        let Some(keyword) = list[0].as_symbol() else {
            if push_interpret_error(
                &mut result,
                config,
                InterpretError {
                    message: "expected construct keyword (symbol), got something else".to_string(),
                    span: list[0].span(),
                    kind: InterpretErrorKind::ExpectedKeyword,
                    suggestions: vec![
                        "construct keywords: defrule, deftemplate, deffacts".to_string()
                    ],
                },
            ) {
                return result;
            }
            continue;
        };

        // Dispatch on keyword
        match keyword {
            "defrule" => match interpret_rule(&list[1..], sexpr.span()) {
                Ok(construct) => result.constructs.push(Construct::Rule(construct)),
                Err(err) => {
                    if push_interpret_error(&mut result, config, err) {
                        return result;
                    }
                }
            },
            "deftemplate" => match interpret_template(&list[1..], sexpr.span()) {
                Ok(construct) => result.constructs.push(Construct::Template(construct)),
                Err(err) => {
                    if push_interpret_error(&mut result, config, err) {
                        return result;
                    }
                }
            },
            "deffacts" => match interpret_facts(&list[1..], sexpr.span()) {
                Ok(construct) => result.constructs.push(Construct::Facts(construct)),
                Err(err) => {
                    if push_interpret_error(&mut result, config, err) {
                        return result;
                    }
                }
            },
            "deffunction" => match interpret_function(&list[1..], sexpr.span()) {
                Ok(construct) => result.constructs.push(Construct::Function(construct)),
                Err(err) => {
                    if push_interpret_error(&mut result, config, err) {
                        return result;
                    }
                }
            },
            "defglobal" => match interpret_global(&list[1..], sexpr.span()) {
                Ok(construct) => result.constructs.push(Construct::Global(construct)),
                Err(err) => {
                    if push_interpret_error(&mut result, config, err) {
                        return result;
                    }
                }
            },
            "defmodule" => match interpret_module(&list[1..], sexpr.span()) {
                Ok(construct) => result.constructs.push(Construct::Module(construct)),
                Err(err) => {
                    if push_interpret_error(&mut result, config, err) {
                        return result;
                    }
                }
            },
            "defgeneric" => match interpret_generic(&list[1..], sexpr.span()) {
                Ok(construct) => result.constructs.push(Construct::Generic(construct)),
                Err(err) => {
                    if push_interpret_error(&mut result, config, err) {
                        return result;
                    }
                }
            },
            "defmethod" => match interpret_method(&list[1..], sexpr.span()) {
                Ok(construct) => result.constructs.push(Construct::Method(construct)),
                Err(err) => {
                    if push_interpret_error(&mut result, config, err) {
                        return result;
                    }
                }
            },
            // Known CLIPS keywords that are not yet supported
            "defclass" | "definstances" | "defmessage-handler" => {
                if push_interpret_error(
                    &mut result,
                    config,
                    InterpretError {
                        message: format!("{keyword} is not yet supported"),
                        span: list[0].span(),
                        kind: InterpretErrorKind::UnknownConstruct,
                        suggestions: vec![
                            "currently supported: defrule, deftemplate, deffacts, deffunction, defglobal, defmodule, defgeneric, defmethod".to_string()
                        ],
                    },
                ) {
                    return result;
                }
            }
            // Unknown keyword
            _ => {
                if push_interpret_error(
                    &mut result,
                    config,
                    InterpretError::unknown_construct(keyword, list[0].span()),
                ) {
                    return result;
                }
            }
        }
    }

    result
}

fn parse_optional_comment(elements: &[SExpr], idx: &mut usize) -> Option<String> {
    let comment = elements
        .get(*idx)
        .and_then(SExpr::as_atom)
        .and_then(|atom| match atom {
            Atom::String(s) => Some(s.clone()),
            _ => None,
        });

    if comment.is_some() {
        *idx += 1;
    }

    comment
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
    let mut salience = 0;

    // Check for optional comment (string as second element)
    let comment = parse_optional_comment(elements, &mut idx);

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

    // Interpret LHS patterns
    let lhs_elements = &elements[idx..arrow_idx];
    let mut patterns = Vec::new();
    let mut i = 0;
    while i < lhs_elements.len() {
        // Check for ?var <- (pattern) syntax
        if i + 2 < lhs_elements.len() {
            if let Some(Atom::SingleVar(var_name)) = lhs_elements[i].as_atom() {
                if let Some(Atom::Connective(Connective::Assign)) = lhs_elements[i + 1].as_atom() {
                    let inner_pattern = interpret_pattern(&lhs_elements[i + 2])?;
                    let pat_span = Span::merge(lhs_elements[i].span(), lhs_elements[i + 2].span());
                    patterns.push(Pattern::Assigned {
                        variable: var_name.clone(),
                        pattern: Box::new(inner_pattern),
                        span: pat_span,
                    });
                    i += 3;
                    continue;
                }
            }
        }
        patterns.push(interpret_pattern(&lhs_elements[i])?);
        i += 1;
    }

    // Interpret RHS actions
    let rhs_elements = &elements[arrow_idx + 1..];
    let mut actions = Vec::new();
    for rhs_expr in rhs_elements {
        actions.push(interpret_action(rhs_expr)?);
    }

    Ok(RuleConstruct {
        name,
        span,
        comment,
        salience,
        patterns,
        actions,
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
    let comment = parse_optional_comment(elements, &mut idx);

    // Parse slot definitions
    let mut slots = Vec::new();
    for slot_expr in &elements[idx..] {
        slots.push(interpret_slot_definition(slot_expr)?);
    }

    Ok(TemplateConstruct {
        name,
        span,
        comment,
        slots,
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
    let comment = parse_optional_comment(elements, &mut idx);

    // Parse fact bodies
    let mut facts = Vec::new();
    for fact_expr in &elements[idx..] {
        facts.push(interpret_fact_body(fact_expr)?);
    }

    Ok(FactsConstruct {
        name,
        span,
        comment,
        facts,
    })
}

// ============================================================================
// Deffunction interpretation
// ============================================================================

/// Interprets a `deffunction` construct.
///
/// Expects elements after the `deffunction` keyword:
/// - name (symbol)
/// - optional comment (string)
/// - parameter list (a list of `?name` and optionally a trailing `$?name`)
/// - one or more body expressions
#[allow(clippy::too_many_lines)] // Parameter and body parsing sections keep the logic clear inline
fn interpret_function(elements: &[SExpr], span: Span) -> Result<FunctionConstruct, InterpretError> {
    if elements.is_empty() {
        return Err(InterpretError {
            message: "deffunction requires a name".to_string(),
            span,
            kind: InterpretErrorKind::MissingElement,
            suggestions: vec!["(deffunction name (<parameters>) <body>)".to_string()],
        });
    }

    let name = elements[0]
        .as_symbol()
        .ok_or_else(|| InterpretError {
            message: "deffunction name must be a symbol".to_string(),
            span: elements[0].span(),
            kind: InterpretErrorKind::InvalidStructure,
            suggestions: vec![],
        })?
        .to_string();

    let mut idx = 1;

    // Optional doc comment (string literal immediately after the name)
    let comment = parse_optional_comment(elements, &mut idx);

    // Parameter list (required; must be a list)
    if idx >= elements.len() {
        return Err(InterpretError {
            message: "deffunction requires a parameter list".to_string(),
            span,
            kind: InterpretErrorKind::MissingElement,
            suggestions: vec!["(deffunction name (<parameters>) <body>)".to_string()],
        });
    }

    let param_list = elements[idx].as_list().ok_or_else(|| InterpretError {
        message: "deffunction parameter list must be a list".to_string(),
        span: elements[idx].span(),
        kind: InterpretErrorKind::InvalidStructure,
        suggestions: vec!["(deffunction name (?x ?y) <body>)".to_string()],
    })?;
    idx += 1;

    let mut parameters = Vec::new();
    let mut wildcard_parameter = None;

    for param_expr in param_list {
        match param_expr.as_atom() {
            Some(Atom::SingleVar(var_name)) => {
                if wildcard_parameter.is_some() {
                    return Err(InterpretError {
                        message: "regular parameters cannot follow a wildcard parameter"
                            .to_string(),
                        span: param_expr.span(),
                        kind: InterpretErrorKind::InvalidStructure,
                        suggestions: vec![],
                    });
                }
                parameters.push(var_name.clone());
            }
            Some(Atom::MultiVar(var_name)) => {
                if wildcard_parameter.is_some() {
                    return Err(InterpretError {
                        message: "only one wildcard parameter is allowed".to_string(),
                        span: param_expr.span(),
                        kind: InterpretErrorKind::InvalidStructure,
                        suggestions: vec![],
                    });
                }
                wildcard_parameter = Some(var_name.clone());
            }
            _ => {
                return Err(InterpretError {
                    message: "deffunction parameter must be a variable (?name or $?name)"
                        .to_string(),
                    span: param_expr.span(),
                    kind: InterpretErrorKind::InvalidStructure,
                    suggestions: vec![],
                });
            }
        }
    }

    // Body (one or more expressions)
    if idx >= elements.len() {
        return Err(InterpretError {
            message: "deffunction requires at least one body expression".to_string(),
            span,
            kind: InterpretErrorKind::MissingElement,
            suggestions: vec!["(deffunction name (?x) (+ ?x 1))".to_string()],
        });
    }

    let mut body = Vec::new();
    for elem in &elements[idx..] {
        body.push(interpret_action_expr(elem)?);
    }

    Ok(FunctionConstruct {
        name,
        span,
        comment,
        parameters,
        wildcard_parameter,
        body,
    })
}

// ============================================================================
// Defglobal interpretation
// ============================================================================

/// Interprets a `defglobal` construct.
///
/// Expects elements after the `defglobal` keyword in repeating triplets:
/// `?*name* = <value-expression>`
///
/// At least one global definition is required.
fn interpret_global(elements: &[SExpr], span: Span) -> Result<GlobalConstruct, InterpretError> {
    let mut globals = Vec::new();
    let mut idx = 0;

    // CLIPS allows an optional module name prefix: (defglobal MAIN ?*x* = 1)
    // If the first element is a plain symbol (not a global var), skip it as
    // the module qualifier.
    if idx < elements.len() {
        if let Some(Atom::Symbol(_)) = elements[idx].as_atom() {
            idx += 1;
        }
    }

    while idx < elements.len() {
        // Expect a global variable atom: ?*name*
        let global_name = match elements[idx].as_atom() {
            Some(Atom::GlobalVar(name)) => name.clone(),
            _ => {
                return Err(InterpretError {
                    message: format!(
                        "expected global variable name (?*name*), found {:?}",
                        elements[idx]
                    ),
                    span: elements[idx].span(),
                    kind: InterpretErrorKind::InvalidStructure,
                    suggestions: vec!["(defglobal ?*name* = value)".to_string()],
                });
            }
        };
        let def_span = elements[idx].span();
        idx += 1;

        // Expect `=` sign (may be Symbol("=") or Connective(Equals))
        if idx >= elements.len() {
            return Err(InterpretError {
                message: format!("expected `=` after ?*{global_name}*"),
                span: def_span,
                kind: InterpretErrorKind::MissingElement,
                suggestions: vec!["(defglobal ?*name* = value)".to_string()],
            });
        }

        let is_equals = match elements[idx].as_atom() {
            Some(Atom::Symbol(s)) => s == "=",
            Some(Atom::Connective(Connective::Equals)) => true,
            _ => false,
        };
        if !is_equals {
            return Err(InterpretError {
                message: format!(
                    "expected `=` after ?*{global_name}*, found {:?}",
                    elements[idx]
                ),
                span: elements[idx].span(),
                kind: InterpretErrorKind::InvalidStructure,
                suggestions: vec!["(defglobal ?*name* = value)".to_string()],
            });
        }
        idx += 1;

        // Expect value expression
        if idx >= elements.len() {
            return Err(InterpretError {
                message: format!("expected value expression after ?*{global_name}* ="),
                span: def_span,
                kind: InterpretErrorKind::MissingElement,
                suggestions: vec!["(defglobal ?*name* = value)".to_string()],
            });
        }

        let value = interpret_action_expr(&elements[idx])?;
        idx += 1;

        globals.push(GlobalDefinition {
            name: global_name,
            span: def_span,
            value,
        });
    }

    if globals.is_empty() {
        return Err(InterpretError {
            message: "defglobal requires at least one global definition".to_string(),
            span,
            kind: InterpretErrorKind::MissingElement,
            suggestions: vec!["(defglobal ?*name* = value)".to_string()],
        });
    }

    Ok(GlobalConstruct { span, globals })
}

// ============================================================================
// Defmodule interpretation
// ============================================================================

/// Interprets a `defmodule` construct.
///
/// Syntax:
/// ```clips
/// (defmodule NAME
///   "optional comment"
///   (export ?ALL)
///   (export deftemplate name1 name2)
///   (import OTHER-MODULE ?ALL)
///   (import OTHER-MODULE deftemplate name1))
/// ```
fn interpret_module(elements: &[SExpr], span: Span) -> Result<ModuleConstruct, InterpretError> {
    if elements.is_empty() {
        return Err(InterpretError::missing("module name", span));
    }

    let name = elements[0]
        .as_symbol()
        .ok_or_else(|| InterpretError::expected("module name (symbol)", elements[0].span()))?
        .to_string();

    let mut idx = 1;

    // Optional comment
    let comment = parse_optional_comment(elements, &mut idx);

    let mut exports = Vec::new();
    let mut imports = Vec::new();

    // Process remaining elements — each should be a list starting with "export" or "import"
    while idx < elements.len() {
        let spec_list = elements[idx].as_list().ok_or_else(|| {
            InterpretError::expected(
                "export or import specification (list)",
                elements[idx].span(),
            )
        })?;

        if spec_list.is_empty() {
            return Err(InterpretError::invalid(
                "empty module specification",
                elements[idx].span(),
            ));
        }

        let keyword = spec_list[0].as_symbol().ok_or_else(|| {
            InterpretError::expected("\"export\" or \"import\" keyword", spec_list[0].span())
        })?;

        match keyword {
            "export" => {
                let spec = interpret_module_spec(&spec_list[1..], elements[idx].span())?;
                exports.push(spec);
            }
            "import" => {
                let import = interpret_import_spec(&spec_list[1..], elements[idx].span())?;
                imports.push(import);
            }
            other => {
                return Err(InterpretError::invalid(
                    &format!(
                        "unknown module specification keyword `{other}`; expected `export` or `import`"
                    ),
                    spec_list[0].span(),
                ));
            }
        }

        idx += 1;
    }

    Ok(ModuleConstruct {
        name,
        span,
        comment,
        exports,
        imports,
    })
}

/// Parse a module spec like `?ALL`, `?NONE`, or `deftemplate name1 name2`.
fn interpret_module_spec(elements: &[SExpr], span: Span) -> Result<ModuleSpec, InterpretError> {
    if elements.is_empty() {
        return Err(InterpretError::missing("export specification", span));
    }

    // Check for ?ALL or ?NONE (single-field variables in the parser)
    if let Some(atom) = elements[0].as_atom() {
        match atom {
            Atom::SingleVar(v) if v == "ALL" => return Ok(ModuleSpec::All),
            Atom::SingleVar(v) if v == "NONE" => return Ok(ModuleSpec::None),
            // Also handle as symbols in case parser treats them differently
            Atom::Symbol(s) if s == "?ALL" => return Ok(ModuleSpec::All),
            Atom::Symbol(s) if s == "?NONE" => return Ok(ModuleSpec::None),
            _ => {}
        }
    }

    // Otherwise: construct-type followed by names
    let construct_type = elements[0]
        .as_symbol()
        .ok_or_else(|| {
            InterpretError::expected("construct type (symbol) or ?ALL/?NONE", elements[0].span())
        })?
        .to_string();

    let mut names = Vec::new();
    for elem in &elements[1..] {
        // Names can also be ?ALL or ?NONE for construct-type-level wildcards
        if let Some(atom) = elem.as_atom() {
            match atom {
                Atom::SingleVar(v) if v == "ALL" => {
                    // (export deftemplate ?ALL) — export all deftemplates
                    return Ok(ModuleSpec::Specific {
                        construct_type,
                        names: vec!["?ALL".to_string()],
                    });
                }
                Atom::SingleVar(v) if v == "NONE" => {
                    return Ok(ModuleSpec::Specific {
                        construct_type,
                        names: vec!["?NONE".to_string()],
                    });
                }
                _ => {}
            }
        }
        let name = elem
            .as_symbol()
            .ok_or_else(|| InterpretError::expected("construct name (symbol)", elem.span()))?
            .to_string();
        names.push(name);
    }

    Ok(ModuleSpec::Specific {
        construct_type,
        names,
    })
}

/// Parse an import spec like `MODULE-NAME ?ALL` or `MODULE-NAME deftemplate name1`.
fn interpret_import_spec(elements: &[SExpr], span: Span) -> Result<ImportSpec, InterpretError> {
    if elements.is_empty() {
        return Err(InterpretError::missing("module name for import", span));
    }

    let module_name = elements[0]
        .as_symbol()
        .ok_or_else(|| InterpretError::expected("module name (symbol)", elements[0].span()))?
        .to_string();

    if elements.len() < 2 {
        return Err(InterpretError::missing(
            "import specification after module name",
            span,
        ));
    }

    let spec = interpret_module_spec(&elements[1..], span)?;

    Ok(ImportSpec {
        module_name,
        spec,
        span,
    })
}

// ============================================================================
// Defgeneric interpretation
// ============================================================================

/// Interprets a `defgeneric` construct.
///
/// Syntax:
/// ```clips
/// (defgeneric display "Display any value")
/// ```
fn interpret_generic(elements: &[SExpr], span: Span) -> Result<GenericConstruct, InterpretError> {
    if elements.is_empty() {
        return Err(InterpretError::missing("generic function name", span));
    }

    let name = elements[0]
        .as_symbol()
        .ok_or_else(|| {
            InterpretError::expected("generic function name (symbol)", elements[0].span())
        })?
        .to_string();

    let mut idx = 1;

    // Optional comment
    let comment = parse_optional_comment(elements, &mut idx);

    // defgeneric should have no more elements after name and optional comment
    if idx < elements.len() {
        return Err(InterpretError::invalid(
            "unexpected elements after defgeneric declaration",
            elements[idx].span(),
        ));
    }

    Ok(GenericConstruct {
        name,
        span,
        comment,
    })
}

// ============================================================================
// Defmethod interpretation
// ============================================================================

/// Interprets a `defmethod` construct.
///
/// Syntax:
/// ```clips
/// (defmethod display ((?x INTEGER))        ; typed param
///   (printout t "Int:" ?x crlf))
///
/// (defmethod display 1 ((?x INTEGER))      ; with explicit index
///   (printout t "Int:" ?x crlf))
///
/// (defmethod display ((?x) $?rest) ?x)     ; untyped + wildcard
/// ```
fn interpret_method(elements: &[SExpr], span: Span) -> Result<MethodConstruct, InterpretError> {
    if elements.is_empty() {
        return Err(InterpretError::missing("method name", span));
    }

    let name = elements[0]
        .as_symbol()
        .ok_or_else(|| InterpretError::expected("method name (symbol)", elements[0].span()))?
        .to_string();

    let mut idx = 1;

    // Optional method index (integer)
    let index = if idx < elements.len() {
        if let Some(Atom::Integer(n)) = elements[idx].as_atom() {
            idx += 1;
            #[allow(clippy::cast_possible_truncation)]
            Some(*n as i32)
        } else {
            None
        }
    } else {
        None
    };

    // Required parameter list
    if idx >= elements.len() {
        return Err(InterpretError::missing("parameter restrictions list", span));
    }

    let param_list = elements[idx].as_list().ok_or_else(|| {
        InterpretError::expected("parameter restrictions list", elements[idx].span())
    })?;
    idx += 1;

    // Parse parameters
    let mut parameters = Vec::new();
    let mut wildcard_parameter = None;

    for param_expr in param_list {
        // Check for wildcard ($?rest)
        if let Some(Atom::MultiVar(var_name)) = param_expr.as_atom() {
            wildcard_parameter = Some(var_name.clone());
            continue;
        }

        // Regular parameter: must be a list like (?name TYPE1 TYPE2 ...)
        let restriction_list = param_expr.as_list().ok_or_else(|| {
            InterpretError::expected(
                "parameter restriction (list like (?x INTEGER))",
                param_expr.span(),
            )
        })?;

        if restriction_list.is_empty() {
            return Err(InterpretError::invalid(
                "empty parameter restriction",
                param_expr.span(),
            ));
        }

        // First element must be a single variable
        let param_name = match restriction_list[0].as_atom() {
            Some(Atom::SingleVar(v)) => v.clone(),
            _ => {
                return Err(InterpretError::expected(
                    "parameter variable (?name)",
                    restriction_list[0].span(),
                ));
            }
        };

        // Remaining elements are type restrictions (symbols)
        let mut type_restrictions = Vec::new();
        for type_expr in &restriction_list[1..] {
            let type_name = type_expr
                .as_symbol()
                .ok_or_else(|| {
                    InterpretError::expected(
                        "type restriction (symbol like INTEGER, FLOAT)",
                        type_expr.span(),
                    )
                })?
                .to_string();
            type_restrictions.push(type_name);
        }

        parameters.push(MethodParameter {
            name: param_name,
            type_restrictions,
            span: param_expr.span(),
        });
    }

    // Body expressions (at least one required)
    if idx >= elements.len() {
        return Err(InterpretError::missing("method body", span));
    }

    let mut body = Vec::new();
    for elem in &elements[idx..] {
        body.push(interpret_action_expr(elem)?);
    }

    Ok(MethodConstruct {
        name,
        span,
        index,
        parameters,
        wildcard_parameter,
        body,
    })
}

// ============================================================================
// Pattern interpretation
// ============================================================================

/// Interpret a single pattern element from a rule's LHS.
fn interpret_pattern(expr: &SExpr) -> Result<Pattern, InterpretError> {
    let list = expr
        .as_list()
        .ok_or_else(|| InterpretError::expected("pattern (list)", expr.span()))?;

    if list.is_empty() {
        return Err(InterpretError::invalid("empty pattern", expr.span()));
    }

    if let Some(conditional) = interpret_conditional_pattern(list, expr)? {
        return Ok(conditional);
    }

    interpret_relation_pattern(list, expr)
}

fn interpret_conditional_pattern(
    list: &[SExpr],
    expr: &SExpr,
) -> Result<Option<Pattern>, InterpretError> {
    match list[0].as_symbol() {
        Some("and") => {
            if list.len() < 2 {
                return Err(InterpretError::missing(
                    "pattern after 'and'",
                    list[0].span(),
                ));
            }
            let mut patterns = Vec::new();
            for pattern_expr in &list[1..] {
                patterns.push(interpret_pattern(pattern_expr)?);
            }
            Ok(Some(Pattern::And(patterns, expr.span())))
        }
        Some("not") => {
            if list.len() < 2 {
                return Err(InterpretError::missing(
                    "pattern after 'not'",
                    list[0].span(),
                ));
            }
            if list.len() > 2 {
                return Err(InterpretError::invalid(
                    "'not' conditional element: expected exactly one pattern",
                    list[2].span(),
                ));
            }
            let inner_pattern = interpret_pattern(&list[1])?;
            Ok(Some(Pattern::Not(Box::new(inner_pattern), expr.span())))
        }
        Some("test") => {
            if list.len() < 2 {
                return Err(InterpretError::missing(
                    "expression after 'test'",
                    expr.span(),
                ));
            }
            // Store the test expression as raw S-expr (full compilation in Phase 3)
            Ok(Some(Pattern::Test(list[1].clone(), expr.span())))
        }
        Some("exists") => {
            let mut patterns = Vec::new();
            for pattern_expr in &list[1..] {
                patterns.push(interpret_pattern(pattern_expr)?);
            }
            Ok(Some(Pattern::Exists(patterns, expr.span())))
        }
        Some("forall") => {
            if list.len() < 3 {
                return Err(InterpretError::missing(
                    "condition and then-clause (forall requires at least two sub-patterns)",
                    expr.span(),
                ));
            }
            let mut patterns = Vec::new();
            for pattern_expr in &list[1..] {
                patterns.push(interpret_pattern(pattern_expr)?);
            }
            Ok(Some(Pattern::Forall(patterns, expr.span())))
        }
        Some("logical") => {
            let mut patterns = Vec::new();
            for pattern_expr in &list[1..] {
                patterns.push(interpret_pattern(pattern_expr)?);
            }
            Ok(Some(Pattern::Logical(patterns, expr.span())))
        }
        Some("or") => {
            if list.len() < 3 {
                return Err(InterpretError::missing(
                    "at least two patterns in (or ...)",
                    expr.span(),
                ));
            }
            let mut patterns = Vec::new();
            for pattern_expr in &list[1..] {
                patterns.push(interpret_pattern(pattern_expr)?);
            }
            Ok(Some(Pattern::Or(patterns, expr.span())))
        }
        _ => Ok(None),
    }
}

fn interpret_relation_pattern(list: &[SExpr], expr: &SExpr) -> Result<Pattern, InterpretError> {
    // Template patterns have slot-value pairs like: (template (slot-name value) ...)
    // Ordered patterns have fields like: (relation value1 value2 ...)
    let relation = list[0]
        .as_symbol()
        .ok_or_else(|| InterpretError::expected("pattern name (symbol)", list[0].span()))?
        .to_string();

    if is_template_style_fields(&list[1..]) {
        interpret_template_pattern(relation, &list[1..], expr.span())
    } else {
        interpret_ordered_pattern(relation, &list[1..], expr.span())
    }
}

fn is_template_style_fields(fields: &[SExpr]) -> bool {
    !fields.is_empty() && fields.iter().all(is_named_pair_list)
}

fn is_named_pair_list(expr: &SExpr) -> bool {
    let Some(sub_list) = expr.as_list() else {
        return false;
    };

    !sub_list.is_empty() && sub_list[0].as_symbol().is_some()
}

fn interpret_template_pattern(
    relation: String,
    slot_exprs: &[SExpr],
    span: Span,
) -> Result<Pattern, InterpretError> {
    let mut slot_constraints = Vec::with_capacity(slot_exprs.len());
    for slot_expr in slot_exprs {
        slot_constraints.push(interpret_pattern_slot_constraint(slot_expr)?);
    }

    Ok(Pattern::Template(TemplatePattern {
        template: relation,
        slot_constraints,
        span,
    }))
}

fn interpret_pattern_slot_constraint(slot_expr: &SExpr) -> Result<SlotConstraint, InterpretError> {
    let slot_list = slot_expr
        .as_list()
        .ok_or_else(|| InterpretError::expected("slot constraint (list)", slot_expr.span()))?;

    if slot_list.is_empty() {
        return Err(InterpretError::invalid(
            "empty slot constraint",
            slot_expr.span(),
        ));
    }

    let slot_name = slot_list[0]
        .as_symbol()
        .ok_or_else(|| InterpretError::expected("slot name (symbol)", slot_list[0].span()))?
        .to_string();

    let constraint = if slot_list.len() > 1 {
        let (c, _consumed) = interpret_constraint_sequence(&slot_list[1..])?;
        c
    } else {
        Constraint::Wildcard(slot_expr.span())
    };

    Ok(SlotConstraint {
        slot_name,
        constraint,
        span: slot_expr.span(),
    })
}

fn interpret_ordered_pattern(
    relation: String,
    field_exprs: &[SExpr],
    span: Span,
) -> Result<Pattern, InterpretError> {
    let mut constraints = Vec::new();
    let mut i = 0;
    while i < field_exprs.len() {
        let (constraint, consumed) = interpret_constraint_sequence(&field_exprs[i..])?;
        constraints.push(constraint);
        i += consumed;
    }

    Ok(Pattern::Ordered(OrderedPattern {
        relation,
        constraints,
        span,
    }))
}

/// Interpret a sequence of S-expressions as a single constraint, handling
/// connective operators (`&`, `|`, `~`) with correct precedence.
///
/// Precedence (highest to lowest): `~` (prefix not) > `&` (and) > `|` (or).
///
/// Returns the parsed `Constraint` and the number of S-expressions consumed
/// from `exprs`.
fn interpret_constraint_sequence(exprs: &[SExpr]) -> Result<(Constraint, usize), InterpretError> {
    if exprs.is_empty() {
        // Should not happen in normal flow, but guard anyway.
        return Err(InterpretError::invalid(
            "empty constraint sequence",
            // We have no span; callers always check length first.
            Span::point(crate::span::Position::new(), crate::span::FileId(0)),
        ));
    }
    parse_or_expr(exprs)
}

/// Parse an or-expression: `and_expr ("|" and_expr)*`.
///
/// Returns `(constraint, consumed)`.
fn parse_or_expr(exprs: &[SExpr]) -> Result<(Constraint, usize), InterpretError> {
    let (mut lhs, mut pos) = parse_and_expr(exprs)?;

    loop {
        if pos >= exprs.len() {
            break;
        }
        // Check if the next token is `|`
        if !is_connective(&exprs[pos], Connective::Or) {
            break;
        }
        // Consume the `|`
        pos += 1;

        if pos >= exprs.len() {
            return Err(InterpretError::invalid(
                "expected constraint after `|`",
                exprs[pos - 1].span(),
            ));
        }

        let (rhs, rhs_len) = parse_and_expr(&exprs[pos..])?;
        let combined_span = lhs.span().merge(rhs.span());

        // Flatten nested Or into a single Or if possible.
        lhs = match lhs {
            Constraint::Or(mut terms, _) => {
                terms.push(rhs);
                Constraint::Or(terms, combined_span)
            }
            other => Constraint::Or(vec![other, rhs], combined_span),
        };
        pos += rhs_len;
    }

    Ok((lhs, pos))
}

/// Parse an and-expression: `unary_expr ("&" unary_expr)*`.
///
/// Returns `(constraint, consumed)`.
fn parse_and_expr(exprs: &[SExpr]) -> Result<(Constraint, usize), InterpretError> {
    let (mut lhs, mut pos) = parse_unary_expr(exprs)?;

    loop {
        if pos >= exprs.len() {
            break;
        }
        // Check if the next token is `&`
        if !is_connective(&exprs[pos], Connective::And) {
            break;
        }
        // Consume the `&`
        pos += 1;

        if pos >= exprs.len() {
            return Err(InterpretError::invalid(
                "expected constraint after `&`",
                exprs[pos - 1].span(),
            ));
        }

        let (rhs, rhs_len) = parse_unary_expr(&exprs[pos..])?;
        let combined_span = lhs.span().merge(rhs.span());

        // Flatten nested And into a single And if possible.
        lhs = match lhs {
            Constraint::And(mut terms, _) => {
                terms.push(rhs);
                Constraint::And(terms, combined_span)
            }
            other => Constraint::And(vec![other, rhs], combined_span),
        };
        pos += rhs_len;
    }

    Ok((lhs, pos))
}

/// Parse a unary expression: `"~" unary_expr | atom_constraint`.
///
/// Returns `(constraint, consumed)`.
fn parse_unary_expr(exprs: &[SExpr]) -> Result<(Constraint, usize), InterpretError> {
    if exprs.is_empty() {
        return Err(InterpretError::invalid(
            "expected constraint",
            Span::point(crate::span::Position::new(), crate::span::FileId(0)),
        ));
    }

    if is_connective(&exprs[0], Connective::Not) {
        let not_span = exprs[0].span();
        // Consume the `~` and parse the inner constraint.
        if exprs.len() < 2 {
            return Err(InterpretError::invalid(
                "expected constraint after `~`",
                not_span,
            ));
        }
        let (inner, inner_len) = parse_unary_expr(&exprs[1..])?;
        let combined_span = not_span.merge(inner.span());
        return Ok((
            Constraint::Not(Box::new(inner), combined_span),
            1 + inner_len,
        ));
    }

    // `:` and `=` introduce predicate/return-value constraint forms.
    // Consume the connective and one expression as a first-class constraint node.
    if is_connective(&exprs[0], Connective::Colon) || is_connective(&exprs[0], Connective::Equals) {
        let conn_span = exprs[0].span();
        if exprs.len() < 2 {
            return Err(InterpretError::invalid(
                "expected expression after predicate connective",
                conn_span,
            ));
        }
        let combined_span = conn_span.merge(exprs[1].span());
        if is_connective(&exprs[0], Connective::Colon) {
            return Ok((Constraint::Predicate(exprs[1].clone(), combined_span), 2));
        }
        return Ok((Constraint::ReturnValue(exprs[1].clone(), combined_span), 2));
    }

    // Atom constraint — exactly one S-expression.
    let c = interpret_constraint(&exprs[0])?;
    Ok((c, 1))
}

/// Returns `true` if `expr` is a connective atom of the given kind.
fn is_connective(expr: &SExpr, kind: Connective) -> bool {
    matches!(expr.as_atom(), Some(Atom::Connective(c)) if *c == kind)
}

impl Constraint {
    /// Returns the span of this constraint.
    pub fn span(&self) -> Span {
        match self {
            Self::Literal(lit) => lit.span,
            Self::Variable(_, span)
            | Self::MultiVariable(_, span)
            | Self::Wildcard(span)
            | Self::MultiWildcard(span)
            | Self::Predicate(_, span)
            | Self::ReturnValue(_, span)
            | Self::Not(_, span)
            | Self::And(_, span)
            | Self::Or(_, span) => *span,
        }
    }
}

/// Interpret a single constraint from a pattern field.
fn interpret_constraint(expr: &SExpr) -> Result<Constraint, InterpretError> {
    // Check if this is a list (might be a connected constraint expression)
    if let Some(_list) = expr.as_list() {
        // For Phase 2, treat lists as errors (connected constraints require more parsing)
        return Err(InterpretError::invalid(
            "complex constraint expressions not yet supported",
            expr.span(),
        ));
    }

    // Must be an atom
    let atom = expr
        .as_atom()
        .ok_or_else(|| InterpretError::expected("constraint atom", expr.span()))?;

    match atom {
        Atom::Integer(n) => Ok(Constraint::Literal(LiteralValue {
            value: LiteralKind::Integer(*n),
            span: expr.span(),
        })),
        Atom::Float(f) => Ok(Constraint::Literal(LiteralValue {
            value: LiteralKind::Float(*f),
            span: expr.span(),
        })),
        Atom::String(s) => Ok(Constraint::Literal(LiteralValue {
            value: LiteralKind::String(s.clone()),
            span: expr.span(),
        })),
        Atom::Symbol(s) => Ok(Constraint::Literal(LiteralValue {
            value: LiteralKind::Symbol(s.clone()),
            span: expr.span(),
        })),
        Atom::SingleVar(name) => {
            if name.is_empty() {
                // Just "?" without a name
                Ok(Constraint::Wildcard(expr.span()))
            } else {
                Ok(Constraint::Variable(name.clone(), expr.span()))
            }
        }
        Atom::MultiVar(name) => {
            if name.is_empty() {
                // Just "$?" without a name
                Ok(Constraint::MultiWildcard(expr.span()))
            } else {
                Ok(Constraint::MultiVariable(name.clone(), expr.span()))
            }
        }
        Atom::GlobalVar(_) => Err(InterpretError::invalid(
            "global variables not supported in patterns",
            expr.span(),
        )),
        Atom::Connective(_) => Err(InterpretError::invalid(
            "bare connective in pattern (use in constraint expression)",
            expr.span(),
        )),
    }
}

// ============================================================================
// Action interpretation
// ============================================================================

/// Interpret a single action from a rule's RHS.
fn interpret_action(expr: &SExpr) -> Result<Action, InterpretError> {
    // Detect special forms at the action level and wrap them as synthetic
    // `FunctionCall`s so the action executor can route them.
    if let Some(list) = expr.as_list() {
        if !list.is_empty() {
            match list[0].as_symbol() {
                Some("if") => {
                    let if_expr = interpret_if_expr(&list[1..], expr.span())?;
                    // Wrap the parsed `if` form as the sole argument to a synthetic
                    // `FunctionCall` with name `"if"`.  The action executor detects
                    // this name and unpacks `args[0]` as `ActionExpr::If`.
                    return Ok(Action {
                        call: FunctionCall {
                            name: "if".to_string(),
                            args: vec![if_expr],
                            span: expr.span(),
                        },
                    });
                }
                Some("while") => {
                    let while_expr = interpret_while_expr(&list[1..], expr.span())?;
                    return Ok(Action {
                        call: FunctionCall {
                            name: "while".to_string(),
                            args: vec![while_expr],
                            span: expr.span(),
                        },
                    });
                }
                Some("loop-for-count") => {
                    let lfc_expr = interpret_loop_for_count_expr(&list[1..], expr.span())?;
                    return Ok(Action {
                        call: FunctionCall {
                            name: "loop-for-count".to_string(),
                            args: vec![lfc_expr],
                            span: expr.span(),
                        },
                    });
                }
                Some("progn$") => {
                    let progn_expr = interpret_progn_dollar_expr(&list[1..], expr.span())?;
                    return Ok(Action {
                        call: FunctionCall {
                            name: "progn$".to_string(),
                            args: vec![progn_expr],
                            span: expr.span(),
                        },
                    });
                }
                Some("foreach") => {
                    let foreach_expr = interpret_foreach_expr(&list[1..], expr.span())?;
                    return Ok(Action {
                        call: FunctionCall {
                            name: "foreach".to_string(),
                            args: vec![foreach_expr],
                            span: expr.span(),
                        },
                    });
                }
                Some(
                    name @ ("do-for-fact"
                    | "do-for-all-facts"
                    | "delayed-do-for-all-facts"
                    | "any-factp"
                    | "find-fact"
                    | "find-all-facts"),
                ) => {
                    let name = name.to_string();
                    let query_expr = interpret_query_action_expr(&name, &list[1..], expr.span())?;
                    return Ok(Action {
                        call: FunctionCall {
                            name,
                            args: vec![query_expr],
                            span: expr.span(),
                        },
                    });
                }
                // `modify` and `duplicate` use slot-value pair syntax for their
                // arguments after the fact variable.  We must NOT interpret the
                // slot name position as a keyword — `(modify ?f (if ?rest))` means
                // slot name `if`, not an if/then/else construct.
                Some("modify" | "duplicate") => {
                    let name = list[0].as_symbol().unwrap().to_string();
                    let call = interpret_fact_mutation_call(&name, &list[1..], expr.span())?;
                    return Ok(Action { call });
                }
                _ => {}
            }
        }
    }
    let call = interpret_function_call(expr)?;
    Ok(Action { call })
}

/// Parse a `modify` or `duplicate` action call.
///
/// Syntax: `(modify ?var (slot-name value ...) ...)`
///
/// The first argument is the fact variable, parsed normally.  The remaining
/// arguments are slot-value pairs where the first element of each sub-list is
/// a slot name — which can be ANY symbol, including keywords like `if`, `while`,
/// etc.  We therefore parse those sub-expressions without keyword interception.
fn interpret_fact_mutation_call(
    name: &str,
    rest: &[SExpr],
    span: Span,
) -> Result<FunctionCall, InterpretError> {
    let mut args = Vec::new();

    // First arg is the fact variable, parse as normal action expression
    if let Some(first) = rest.first() {
        args.push(interpret_action_expr(first)?);
    }

    // Remaining args are slot-value pairs: parse without keyword interception
    for slot_expr in &rest[1..] {
        args.push(interpret_action_expr_as_slot_pair(slot_expr)?);
    }

    Ok(FunctionCall {
        name: name.to_string(),
        args,
        span,
    })
}

/// Parse an action expression in slot-value pair context.
///
/// If the expression is a list `(name ...)` where `name` is a symbol, treat it
/// as a function call regardless of whether `name` is a keyword.  This allows
/// slot names like `if`, `while`, etc. to pass through without being
/// intercepted as control-flow constructs.
fn interpret_action_expr_as_slot_pair(expr: &SExpr) -> Result<ActionExpr, InterpretError> {
    if let Some(list) = expr.as_list() {
        if !list.is_empty() {
            if let Some(name) = list[0].as_symbol() {
                // Build a FunctionCall directly, parsing sub-args normally
                let mut args = Vec::new();
                for arg_expr in &list[1..] {
                    args.push(interpret_action_expr(arg_expr)?);
                }
                return Ok(ActionExpr::FunctionCall(FunctionCall {
                    name: name.to_string(),
                    args,
                    span: expr.span(),
                }));
            }
        }
    }
    interpret_action_expr(expr)
}

/// Interpret a function call expression.
fn interpret_function_call(expr: &SExpr) -> Result<FunctionCall, InterpretError> {
    let list = expr
        .as_list()
        .ok_or_else(|| InterpretError::expected("function call (list)", expr.span()))?;

    if list.is_empty() {
        return Err(InterpretError::invalid("empty function call", expr.span()));
    }

    // Accept symbols directly, and also connective operators as function names.
    // CLIPS allows `=` and constraint connectives (`&`, `|`, `~`) in
    // function-call position, mapping to their function equivalents.
    let name = if let Some(s) = list[0].as_symbol() {
        s.to_string()
    } else if let Some(Atom::Connective(c)) = list[0].as_atom() {
        match c {
            Connective::Equals => "=".to_string(),
            Connective::And => "and".to_string(),
            Connective::Or => "or".to_string(),
            Connective::Not => "not".to_string(),
            _ => {
                return Err(InterpretError::expected(
                    "function name (symbol)",
                    list[0].span(),
                ));
            }
        }
    } else {
        return Err(InterpretError::expected(
            "function name (symbol)",
            list[0].span(),
        ));
    };

    let mut args = Vec::new();
    let mut i = 1usize;
    while i < list.len() {
        if let Some((slot_ref, consumed)) = try_interpret_fact_slot_ref_argument(&list[i..]) {
            args.push(slot_ref);
            i += consumed;
            continue;
        }
        args.push(interpret_action_expr(&list[i])?);
        i += 1;
    }

    Ok(FunctionCall {
        name,
        args,
        span: expr.span(),
    })
}

fn try_interpret_fact_slot_ref_argument(exprs: &[SExpr]) -> Option<(ActionExpr, usize)> {
    if exprs.len() < 3 {
        return None;
    }

    let Some(Atom::SingleVar(var_name)) = exprs[0].as_atom() else {
        return None;
    };
    let Some(Atom::Connective(Connective::Colon)) = exprs[1].as_atom() else {
        return None;
    };
    let slot_name = exprs[2].as_symbol()?;

    let var_span = exprs[0].span();
    let slot_span = exprs[2].span();
    let call_span = var_span.merge(slot_span);

    let slot_ref = ActionExpr::FunctionCall(FunctionCall {
        name: FACT_SLOT_REF_FN.to_string(),
        args: vec![
            ActionExpr::Variable(var_name.clone(), var_span),
            ActionExpr::Literal(LiteralValue {
                value: LiteralKind::Symbol(slot_name.to_owned()),
                span: slot_span,
            }),
        ],
        span: call_span,
    });
    Some((slot_ref, 3))
}

/// Interpret a CLIPS `(if <cond> then <action>* [else <action>*])` form.
///
/// The S-expression list has already had its head `if` consumed — `rest` is
/// the remaining elements: `[<cond>, then, <action>*, [else, <action>*]]`.
fn interpret_if_expr(rest: &[SExpr], span: Span) -> Result<ActionExpr, InterpretError> {
    if rest.is_empty() {
        return Err(InterpretError::missing("condition in (if ...)", span));
    }

    let condition = interpret_action_expr(&rest[0])?;

    // Find `then` keyword.
    let then_pos = rest[1..]
        .iter()
        .position(|e| e.as_symbol() == Some("then"))
        .ok_or_else(|| InterpretError::missing("then keyword in (if ... then ...)", span))?;
    // `then_pos` is relative to `rest[1..]`, so absolute index is `then_pos + 1`.
    let then_abs = then_pos + 1;

    // Elements between condition and `then` are not valid — the condition is
    // always a single expression (rest[0]).  The `then` must appear at index 1.
    if then_abs != 1 {
        return Err(InterpretError::invalid(
            "expected 'then' immediately after the if-condition",
            span,
        ));
    }

    // Elements after `then` until `else` (or end) are the then-branch actions.
    let after_then = &rest[then_abs + 1..];

    let else_pos = after_then
        .iter()
        .position(|e| e.as_symbol() == Some("else"));

    let (then_exprs, else_exprs) = if let Some(ep) = else_pos {
        (&after_then[..ep], &after_then[ep + 1..])
    } else {
        (after_then, [].as_slice())
    };

    let mut then_actions = Vec::with_capacity(then_exprs.len());
    for e in then_exprs {
        then_actions.push(interpret_action_expr(e)?);
    }

    let mut else_actions = Vec::with_capacity(else_exprs.len());
    for e in else_exprs {
        else_actions.push(interpret_action_expr(e)?);
    }

    Ok(ActionExpr::If {
        condition: Box::new(condition),
        then_actions,
        else_actions,
        span,
    })
}

/// Interpret a CLIPS `(while <cond> do <action>*)` form.
///
/// `rest` is the elements after the `while` keyword.
fn interpret_while_expr(rest: &[SExpr], span: Span) -> Result<ActionExpr, InterpretError> {
    if rest.is_empty() {
        return Err(InterpretError::missing("condition in (while ...)", span));
    }

    let condition = interpret_action_expr(&rest[0])?;

    // The `do` keyword is optional. If present immediately after the condition,
    // consume it; otherwise body starts right after the condition.
    let has_do = rest.len() > 1 && rest[1].as_symbol() == Some("do");
    let body_start = 1 + usize::from(has_do);

    let body_exprs = &rest[body_start..];
    let mut body = Vec::with_capacity(body_exprs.len());
    for e in body_exprs {
        body.push(interpret_action_expr(e)?);
    }

    Ok(ActionExpr::While {
        condition: Box::new(condition),
        body,
        span,
    })
}

/// Interpret a CLIPS `(switch <expr> (case <value> then <action>*) ... [(default <action>*)])` form.
///
/// `rest` is the elements after the `switch` keyword.
fn interpret_switch_expr(rest: &[SExpr], span: Span) -> Result<ActionExpr, InterpretError> {
    if rest.is_empty() {
        return Err(InterpretError::missing("expression in (switch ...)", span));
    }

    // First element is the discriminant expression.
    let discriminant = interpret_action_expr(&rest[0])?;

    let mut cases = Vec::new();
    let mut default = None;

    for clause in &rest[1..] {
        let clause_list = clause
            .as_list()
            .ok_or_else(|| InterpretError::expected("case or default clause", clause.span()))?;

        if clause_list.is_empty() {
            return Err(InterpretError::expected(
                "case or default clause",
                clause.span(),
            ));
        }

        match clause_list[0].as_symbol() {
            Some("case") => {
                // (case <value> then <action>*)
                if clause_list.len() < 4 {
                    return Err(InterpretError::missing(
                        "value and 'then' keyword in case clause",
                        clause.span(),
                    ));
                }
                let case_value = interpret_action_expr(&clause_list[1])?;

                // Find 'then' keyword
                if clause_list[2].as_symbol() != Some("then") {
                    return Err(InterpretError::expected(
                        "'then' keyword after case value",
                        clause_list[2].span(),
                    ));
                }

                let mut actions = Vec::new();
                for action_expr in &clause_list[3..] {
                    actions.push(interpret_action_expr(action_expr)?);
                }
                cases.push((case_value, actions));
            }
            Some("default") => {
                if default.is_some() {
                    return Err(InterpretError::invalid(
                        "duplicate default clause in switch",
                        clause.span(),
                    ));
                }
                let mut actions = Vec::new();
                for action_expr in &clause_list[1..] {
                    actions.push(interpret_action_expr(action_expr)?);
                }
                default = Some(actions);
            }
            _ => {
                return Err(InterpretError::expected(
                    "case or default clause",
                    clause.span(),
                ));
            }
        }
    }

    Ok(ActionExpr::Switch {
        expr: Box::new(discriminant),
        cases,
        default,
        span,
    })
}

/// Interpret a CLIPS `(loop-for-count ...)` form.
///
/// `rest` is the elements after the `loop-for-count` keyword.
/// Forms:
///   `(loop-for-count (?var start end) do <action>*)`
///   `(loop-for-count (?var end) do <action>*)`
///   `(loop-for-count (end) do <action>*)`
fn interpret_loop_for_count_expr(rest: &[SExpr], span: Span) -> Result<ActionExpr, InterpretError> {
    if rest.is_empty() {
        return Err(InterpretError::missing(
            "loop spec in (loop-for-count ...)",
            span,
        ));
    }

    // First element must be a list containing the loop spec.
    let spec_list = rest[0]
        .as_list()
        .ok_or_else(|| InterpretError::expected("loop spec list", rest[0].span()))?;

    if spec_list.is_empty() {
        return Err(InterpretError::missing(
            "loop bound in loop-for-count spec",
            rest[0].span(),
        ));
    }

    // Parse the spec: `(?var start end)`, `(?var end)`, or `(end)`.
    let (var_name, start_expr, end_expr) = match spec_list.len() {
        1 => {
            // `(end)` — anonymous counter, start=1
            let end = interpret_action_expr(&spec_list[0])?;
            let start = ActionExpr::Literal(LiteralValue {
                value: LiteralKind::Integer(1),
                span: spec_list[0].span(),
            });
            (None, start, end)
        }
        2 => {
            // `(?var end)` — named variable, start=1
            let var = match spec_list[0].as_atom() {
                Some(Atom::SingleVar(name)) => name.clone(),
                _ => {
                    return Err(InterpretError::expected(
                        "?variable in loop-for-count spec",
                        spec_list[0].span(),
                    ))
                }
            };
            let end = interpret_action_expr(&spec_list[1])?;
            let start = ActionExpr::Literal(LiteralValue {
                value: LiteralKind::Integer(1),
                span: spec_list[0].span(),
            });
            (Some(var), start, end)
        }
        3 => {
            // `(?var start end)` — named variable with explicit start
            let var = match spec_list[0].as_atom() {
                Some(Atom::SingleVar(name)) => name.clone(),
                _ => {
                    return Err(InterpretError::expected(
                        "?variable in loop-for-count spec",
                        spec_list[0].span(),
                    ))
                }
            };
            let start = interpret_action_expr(&spec_list[1])?;
            let end = interpret_action_expr(&spec_list[2])?;
            (Some(var), start, end)
        }
        _ => {
            return Err(InterpretError::invalid(
                "loop-for-count spec must be (?var end), (?var start end), or (end)",
                rest[0].span(),
            ))
        }
    };

    // The `do` keyword is optional. If present immediately after the spec,
    // consume it; otherwise body starts right after the spec.
    let after_spec = &rest[1..];
    let has_do = !after_spec.is_empty() && after_spec[0].as_symbol() == Some("do");
    let body_start = usize::from(has_do);

    let body_exprs = &after_spec[body_start..];
    let mut body = Vec::with_capacity(body_exprs.len());
    for e in body_exprs {
        body.push(interpret_action_expr(e)?);
    }

    Ok(ActionExpr::LoopForCount {
        var_name,
        start: Box::new(start_expr),
        end: Box::new(end_expr),
        body,
        span,
    })
}

/// Interpret a CLIPS `(progn$ (?var <expr>) <action>*)` form.
///
/// `rest` is the elements after the `progn$` keyword.
fn interpret_progn_dollar_expr(rest: &[SExpr], span: Span) -> Result<ActionExpr, InterpretError> {
    if rest.is_empty() {
        return Err(InterpretError::missing(
            "binding spec in (progn$ ...)",
            span,
        ));
    }

    // First element is the binding spec: `(?var <expr>)`.
    let spec_list = rest[0].as_list().ok_or_else(|| {
        InterpretError::expected("binding spec list (?var <expr>) in progn$", rest[0].span())
    })?;

    if spec_list.len() < 2 {
        return Err(InterpretError::missing(
            "?variable and list expression in progn$ spec",
            rest[0].span(),
        ));
    }

    let var_name = match spec_list[0].as_atom() {
        Some(Atom::SingleVar(name)) => name.clone(),
        _ => {
            return Err(InterpretError::expected(
                "?variable in progn$ spec",
                spec_list[0].span(),
            ))
        }
    };

    let list_expr = interpret_action_expr(&spec_list[1])?;

    // Remaining elements after the spec are body actions (no `do` delimiter for progn$).
    let body_exprs = &rest[1..];
    let mut body = Vec::with_capacity(body_exprs.len());
    for e in body_exprs {
        body.push(interpret_action_expr(e)?);
    }

    Ok(ActionExpr::Progn {
        var_name,
        list_expr: Box::new(list_expr),
        body,
        span,
    })
}

/// Interpret a CLIPS `(foreach ?var <expr> do <action>*)` form.
///
/// `rest` is the elements after the `foreach` keyword.
/// This is semantically equivalent to `progn$` but uses a different syntax.
fn interpret_foreach_expr(rest: &[SExpr], span: Span) -> Result<ActionExpr, InterpretError> {
    // foreach ?var <expr> do <action>*
    if rest.len() < 2 {
        return Err(InterpretError::missing(
            "?variable and list expression in (foreach ...)",
            span,
        ));
    }

    let var_name = match rest[0].as_atom() {
        Some(Atom::SingleVar(name)) => name.clone(),
        _ => {
            return Err(InterpretError::expected(
                "?variable in foreach",
                rest[0].span(),
            ))
        }
    };

    let list_expr = interpret_action_expr(&rest[1])?;

    // Find optional `do` keyword (some CLIPS variants include it, some don't).
    let body_start = if rest.len() > 2 && rest[2].as_symbol() == Some("do") {
        3
    } else {
        2
    };

    let body_exprs = &rest[body_start..];
    let mut body = Vec::with_capacity(body_exprs.len());
    for e in body_exprs {
        body.push(interpret_action_expr(e)?);
    }

    Ok(ActionExpr::Progn {
        var_name,
        list_expr: Box::new(list_expr),
        body,
        span,
    })
}

/// Interpret a CLIPS fact-query macro form.
///
/// Handles: `do-for-fact`, `do-for-all-facts`, `delayed-do-for-all-facts`,
/// `any-factp`, `find-fact`, and `find-all-facts`.
///
/// `name` is the macro keyword that was already consumed. `rest` is the
/// remaining elements: `[<binding-list>, <query>, <action>*]`.
/// For query-only forms (`any-factp`, `find-fact`, `find-all-facts`) there
/// are no trailing action elements.
fn interpret_query_action_expr(
    name: &str,
    rest: &[SExpr],
    span: Span,
) -> Result<ActionExpr, InterpretError> {
    if rest.len() < 2 {
        return Err(InterpretError::missing(
            &format!("binding list and query in ({name} ...)"),
            span,
        ));
    }

    // First element is the binding list: a list of `(?var template)` pairs.
    let binding_list = rest[0].as_list().ok_or_else(|| {
        InterpretError::expected(
            &format!("binding list ((?var template) ...) in ({name} ...)"),
            rest[0].span(),
        )
    })?;

    let mut bindings = Vec::with_capacity(binding_list.len());
    for binding_expr in binding_list {
        let pair = binding_expr.as_list().ok_or_else(|| {
            InterpretError::expected(
                "(?variable template-name) binding pair",
                binding_expr.span(),
            )
        })?;

        if pair.len() < 2 {
            return Err(InterpretError::missing(
                "template name in binding pair (?var template)",
                binding_expr.span(),
            ));
        }

        let var_name = match pair[0].as_atom() {
            Some(Atom::SingleVar(name)) => name.clone(),
            _ => {
                return Err(InterpretError::expected(
                    "?variable in binding pair",
                    pair[0].span(),
                ))
            }
        };

        let template_name = pair[1]
            .as_symbol()
            .ok_or_else(|| {
                InterpretError::expected("template name (symbol) in binding pair", pair[1].span())
            })?
            .to_string();

        bindings.push((var_name, template_name));
    }

    // Second element is the query expression.
    let query = interpret_action_expr(&rest[1])?;

    // Remaining elements (if any) are body actions.
    let mut body = Vec::with_capacity(rest.len().saturating_sub(2));
    for body_expr in &rest[2..] {
        body.push(interpret_action_expr(body_expr)?);
    }

    Ok(ActionExpr::QueryAction {
        name: name.to_string(),
        bindings,
        query: Box::new(query),
        body,
        span,
    })
}

/// Interpret an expression in an action context (RHS).
fn interpret_action_expr(expr: &SExpr) -> Result<ActionExpr, InterpretError> {
    // Check if it's a list (nested function call or special form)
    if let Some(list) = expr.as_list() {
        // Detect the `(if ...)` special form.
        if !list.is_empty() && list[0].as_symbol() == Some("if") {
            return interpret_if_expr(&list[1..], expr.span());
        }
        // Detect loop special forms.
        if !list.is_empty() {
            match list[0].as_symbol() {
                Some("while") => return interpret_while_expr(&list[1..], expr.span()),
                Some("loop-for-count") => {
                    return interpret_loop_for_count_expr(&list[1..], expr.span())
                }
                Some("switch") => return interpret_switch_expr(&list[1..], expr.span()),
                Some("progn$") => return interpret_progn_dollar_expr(&list[1..], expr.span()),
                Some("foreach") => return interpret_foreach_expr(&list[1..], expr.span()),
                Some(
                    name @ ("do-for-fact"
                    | "do-for-all-facts"
                    | "delayed-do-for-all-facts"
                    | "any-factp"
                    | "find-fact"
                    | "find-all-facts"),
                ) => {
                    let name = name.to_string();
                    return interpret_query_action_expr(&name, &list[1..], expr.span());
                }
                _ => {}
            }
        }
        let call = interpret_function_call(expr)?;
        return Ok(ActionExpr::FunctionCall(call));
    }

    // Must be an atom
    let atom = expr
        .as_atom()
        .ok_or_else(|| InterpretError::expected("action expression", expr.span()))?;

    match atom {
        Atom::Integer(n) => Ok(ActionExpr::Literal(LiteralValue {
            value: LiteralKind::Integer(*n),
            span: expr.span(),
        })),
        Atom::Float(f) => Ok(ActionExpr::Literal(LiteralValue {
            value: LiteralKind::Float(*f),
            span: expr.span(),
        })),
        Atom::String(s) => Ok(ActionExpr::Literal(LiteralValue {
            value: LiteralKind::String(s.clone()),
            span: expr.span(),
        })),
        Atom::Symbol(s) => Ok(ActionExpr::Literal(LiteralValue {
            value: LiteralKind::Symbol(s.clone()),
            span: expr.span(),
        })),
        Atom::SingleVar(name) => Ok(ActionExpr::Variable(name.clone(), expr.span())),
        Atom::MultiVar(name) => Ok(ActionExpr::Variable(format!("$?{name}"), expr.span())),
        Atom::GlobalVar(name) => Ok(ActionExpr::GlobalVariable(name.clone(), expr.span())),
        Atom::Connective(_) => Err(InterpretError::invalid(
            "connectives not allowed in actions",
            expr.span(),
        )),
    }
}

// ============================================================================
// Template slot interpretation
// ============================================================================

/// Interpret a slot definition in a deftemplate.
fn interpret_slot_definition(expr: &SExpr) -> Result<SlotDefinition, InterpretError> {
    let list = expr
        .as_list()
        .ok_or_else(|| InterpretError::expected("slot definition (list)", expr.span()))?;

    if list.is_empty() {
        return Err(InterpretError::invalid(
            "empty slot definition",
            expr.span(),
        ));
    }

    let keyword = list[0].as_symbol().ok_or_else(|| {
        InterpretError::expected(
            "slot keyword (slot, multislot, field, or multifield)",
            list[0].span(),
        )
    })?;

    let (slot_type, name_idx) = match keyword {
        "slot" | "field" => (SlotType::Single, 1),
        "multislot" | "multifield" => (SlotType::Multi, 1),
        _ => {
            return Err(InterpretError::invalid(
                "expected 'slot', 'multislot', 'field', or 'multifield'",
                list[0].span(),
            ))
        }
    };

    if list.len() < name_idx + 1 {
        return Err(InterpretError::missing("slot name", expr.span()));
    }

    let name = list[name_idx]
        .as_symbol()
        .ok_or_else(|| InterpretError::expected("slot name (symbol)", list[name_idx].span()))?
        .to_string();

    // Check for optional default value
    let mut default = None;
    if list.len() > name_idx + 1 {
        // Look for (default ...) form
        for option_expr in &list[name_idx + 1..] {
            if let Some(option_list) = option_expr.as_list() {
                if !option_list.is_empty() && option_list[0].as_symbol() == Some("default") {
                    if option_list.len() < 2 {
                        return Err(InterpretError::missing("default value", option_expr.span()));
                    }
                    default = Some(interpret_default_value(&option_list[1])?);
                }
            }
        }
    }

    Ok(SlotDefinition {
        name,
        slot_type,
        default,
        span: expr.span(),
    })
}

/// Interpret a default value specification.
fn interpret_default_value(expr: &SExpr) -> Result<DefaultValue, InterpretError> {
    // Check for special symbols ?NONE and ?DERIVE
    if let Some(Atom::SingleVar(name)) = expr.as_atom() {
        if name.eq_ignore_ascii_case("NONE") {
            return Ok(DefaultValue::None);
        } else if name.eq_ignore_ascii_case("DERIVE") {
            return Ok(DefaultValue::Derive);
        }
    }

    // Handle function-call default values like `(create$)` or `(create$ val1 val2)`.
    // `(create$)` with no args produces an empty multifield default.
    if let Some(list) = expr.as_list() {
        if !list.is_empty() && list[0].as_symbol() == Some("create$") {
            // (create$) → empty multifield default.  With args, we still treat
            // it as Derive since we'd need full expression evaluation.
            return Ok(DefaultValue::Derive);
        }
        // Other function-call defaults: accept but treat as Derive.
        return Ok(DefaultValue::Derive);
    }

    // Otherwise, treat as a literal value
    let atom = expr
        .as_atom()
        .ok_or_else(|| InterpretError::expected("default value", expr.span()))?;

    let literal = match atom {
        Atom::Integer(n) => LiteralValue {
            value: LiteralKind::Integer(*n),
            span: expr.span(),
        },
        Atom::Float(f) => LiteralValue {
            value: LiteralKind::Float(*f),
            span: expr.span(),
        },
        Atom::String(s) => LiteralValue {
            value: LiteralKind::String(s.clone()),
            span: expr.span(),
        },
        Atom::Symbol(s) => LiteralValue {
            value: LiteralKind::Symbol(s.clone()),
            span: expr.span(),
        },
        _ => {
            return Err(InterpretError::invalid(
                "invalid default value type",
                expr.span(),
            ))
        }
    };

    Ok(DefaultValue::Value(literal))
}

// ============================================================================
// Fact body interpretation
// ============================================================================

/// Interpret a fact body in a deffacts.
fn interpret_fact_body(expr: &SExpr) -> Result<FactBody, InterpretError> {
    let list = expr
        .as_list()
        .ok_or_else(|| InterpretError::expected("fact (list)", expr.span()))?;

    if list.is_empty() {
        return Err(InterpretError::invalid("empty fact", expr.span()));
    }

    let name = list[0]
        .as_symbol()
        .ok_or_else(|| {
            InterpretError::expected("fact relation or template name (symbol)", list[0].span())
        })?
        .to_string();

    if is_template_style_fields(&list[1..]) {
        interpret_template_fact(name, &list[1..], expr.span())
    } else {
        interpret_ordered_fact(name, &list[1..], expr.span())
    }
}

fn interpret_template_fact(
    template: String,
    slot_exprs: &[SExpr],
    span: Span,
) -> Result<FactBody, InterpretError> {
    let mut slot_values = Vec::with_capacity(slot_exprs.len());
    for slot_expr in slot_exprs {
        slot_values.push(interpret_fact_slot_value(slot_expr)?);
    }

    Ok(FactBody::Template(TemplateFactBody {
        template,
        slot_values,
        span,
    }))
}

fn interpret_fact_slot_value(slot_expr: &SExpr) -> Result<FactSlotValue, InterpretError> {
    let slot_list = slot_expr
        .as_list()
        .ok_or_else(|| InterpretError::expected("slot value (list)", slot_expr.span()))?;

    if slot_list.is_empty() {
        return Err(InterpretError::invalid(
            "empty slot value",
            slot_expr.span(),
        ));
    }

    let slot_name = slot_list[0]
        .as_symbol()
        .ok_or_else(|| InterpretError::expected("slot name (symbol)", slot_list[0].span()))?
        .to_string();

    if slot_list.len() < 2 {
        // Empty slot: `(slot-name)` with no values — valid for multislots,
        // produces an empty multifield value.
        return Ok(FactSlotValue {
            name: slot_name,
            value: FactValue::EmptyMultifield(slot_expr.span()),
            span: slot_expr.span(),
        });
    }

    let value = interpret_fact_value(&slot_list[1])?;

    Ok(FactSlotValue {
        name: slot_name,
        value,
        span: slot_expr.span(),
    })
}

fn interpret_ordered_fact(
    relation: String,
    value_exprs: &[SExpr],
    span: Span,
) -> Result<FactBody, InterpretError> {
    let mut values = Vec::with_capacity(value_exprs.len());
    for value_expr in value_exprs {
        values.push(interpret_fact_value(value_expr)?);
    }

    Ok(FactBody::Ordered(OrderedFactBody {
        relation,
        values,
        span,
    }))
}

/// Interpret a value in a fact body.
fn interpret_fact_value(expr: &SExpr) -> Result<FactValue, InterpretError> {
    let atom = expr
        .as_atom()
        .ok_or_else(|| InterpretError::expected("fact value (atom)", expr.span()))?;

    match atom {
        Atom::Integer(n) => Ok(FactValue::Literal(LiteralValue {
            value: LiteralKind::Integer(*n),
            span: expr.span(),
        })),
        Atom::Float(f) => Ok(FactValue::Literal(LiteralValue {
            value: LiteralKind::Float(*f),
            span: expr.span(),
        })),
        Atom::String(s) => Ok(FactValue::Literal(LiteralValue {
            value: LiteralKind::String(s.clone()),
            span: expr.span(),
        })),
        Atom::Symbol(s) => Ok(FactValue::Literal(LiteralValue {
            value: LiteralKind::Symbol(s.clone()),
            span: expr.span(),
        })),
        Atom::SingleVar(name) => Ok(FactValue::Variable(name.clone(), expr.span())),
        Atom::MultiVar(name) => Ok(FactValue::Variable(format!("$?{name}"), expr.span())),
        Atom::GlobalVar(name) => Ok(FactValue::GlobalVariable(name.clone(), expr.span())),
        Atom::Connective(_) => Err(InterpretError::invalid(
            "connectives not allowed in facts",
            expr.span(),
        )),
    }
}

// ============================================================================
// Helper functions
// ============================================================================

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
    use proptest::prelude::*;

    fn file() -> FileId {
        FileId(0)
    }

    /// Strategy that produces valid CLIPS identifiers: a letter followed by up to 10
    /// alphanumeric-or-underscore characters.
    fn arb_identifier() -> impl Strategy<Value = String> {
        "[a-zA-Z][a-zA-Z0-9_]{0,10}".prop_filter("not a reserved keyword", |s| {
            !matches!(
                s.as_str(),
                "defrule"
                    | "deftemplate"
                    | "deffacts"
                    | "deffunction"
                    | "defglobal"
                    | "defmodule"
                    | "defgeneric"
                    | "defmethod"
                    | "declare"
                    | "salience"
                    | "slot"
                    | "multislot"
                    | "not"
                    | "and"
                    | "or"
                    | "test"
                    | "exists"
                    | "forall"
                    | "logical"
            )
        })
    }

    proptest! {
        /// Panic-safety invariant: interpret_constructs must never panic on any input
        /// produced by parse_sexprs, even if it returns errors.  This validates the
        /// robustness contract of the entire Stage 2 pipeline.
        #[test]
        fn interpret_never_panics(source in "([a-zA-Z0-9?$_\\-]+ ?){0,10}") {
            let parsed = parse_sexprs(&source, file());
            let config = InterpreterConfig::default();
            // The call itself is the assertion — we must not panic
            let _result = interpret_constructs(&parsed.exprs, &config);
        }

        /// Postcondition: a well-formed `(defrule NAME (PAT) => (ACT))` must parse
        /// without errors and produce exactly one Rule construct whose name matches.
        #[test]
        fn valid_defrule_parses_successfully(
            name in arb_identifier(),
            pat in arb_identifier(),
            act in arb_identifier(),
        ) {
            let source = format!("(defrule {name} ({pat}) => ({act}))");
            let parsed = parse_sexprs(&source, file());
            let config = InterpreterConfig::default();
            let result = interpret_constructs(&parsed.exprs, &config);
            // A syntactically correct defrule must produce no interpreter errors
            prop_assert!(result.errors.is_empty(),
                "valid defrule produced errors: {:?}", result.errors);
            // Exactly one construct must be emitted
            prop_assert_eq!(result.constructs.len(), 1,
                "expected exactly 1 construct, got {}", result.constructs.len());
            // That construct must be a Rule
            let Construct::Rule(ref rule) = result.constructs[0] else {
                return Err(TestCaseError::fail("expected Construct::Rule"));
            };
            // The rule name must match the generated identifier
            prop_assert_eq!(&rule.name, &name,
                "rule name must round-trip through parsing");
        }

        /// Postcondition: a well-formed `(deftemplate NAME (slot SLOT_NAME))` must parse
        /// without errors and produce a Template construct with correct name and slot.
        #[test]
        fn valid_deftemplate_parses_successfully(
            name in arb_identifier(),
            slot_name in arb_identifier(),
        ) {
            let source = format!("(deftemplate {name} (slot {slot_name}))");
            let parsed = parse_sexprs(&source, file());
            let config = InterpreterConfig::default();
            let result = interpret_constructs(&parsed.exprs, &config);
            // A well-formed deftemplate must produce no interpreter errors
            prop_assert!(result.errors.is_empty(),
                "valid deftemplate produced errors: {:?}", result.errors);
            // Exactly one construct
            prop_assert_eq!(result.constructs.len(), 1,
                "expected exactly 1 construct");
            // Must be a Template construct
            let Construct::Template(ref tmpl) = result.constructs[0] else {
                return Err(TestCaseError::fail("expected Construct::Template"));
            };
            // Template name must round-trip
            prop_assert_eq!(&tmpl.name, &name,
                "template name must match generated identifier");
            // The single slot must be present with the correct name
            prop_assert_eq!(tmpl.slots.len(), 1,
                "template must have exactly one slot");
            prop_assert_eq!(&tmpl.slots[0].name, &slot_name,
                "slot name must match generated identifier");
        }

        /// Invariant: the salience value declared in a defrule is preserved verbatim
        /// through Stage 2 interpretation; no clamping or alteration must occur.
        #[test]
        fn salience_value_preserved(
            name in arb_identifier(),
            pat in arb_identifier(),
            act in arb_identifier(),
            salience in -10000i32..=10000i32,
        ) {
            let source = format!(
                "(defrule {name} (declare (salience {salience})) ({pat}) => ({act}))"
            );
            let parsed = parse_sexprs(&source, file());
            let config = InterpreterConfig::default();
            let result = interpret_constructs(&parsed.exprs, &config);
            // A valid defrule with declare/salience must not produce errors
            prop_assert!(result.errors.is_empty(),
                "defrule with salience produced errors: {:?}", result.errors);
            prop_assert_eq!(result.constructs.len(), 1);
            let Construct::Rule(ref rule) = result.constructs[0] else {
                return Err(TestCaseError::fail("expected Construct::Rule"));
            };
            // The salience stored on the rule must equal the declared value exactly
            prop_assert_eq!(rule.salience, salience,
                "salience must be preserved without alteration");
        }

        /// Postcondition: a well-formed `(deffacts NAME (fact1) (fact2))` must parse
        /// without errors and produce a Facts construct with the correct name and fact
        /// count.
        #[test]
        fn valid_deffacts_parses_successfully(
            name in arb_identifier(),
            fact1 in arb_identifier(),
            fact2 in arb_identifier(),
        ) {
            let source = format!("(deffacts {name} ({fact1}) ({fact2}))");
            let parsed = parse_sexprs(&source, file());
            let config = InterpreterConfig::default();
            let result = interpret_constructs(&parsed.exprs, &config);
            // A well-formed deffacts must produce no interpreter errors
            prop_assert!(result.errors.is_empty(),
                "valid deffacts produced errors: {:?}", result.errors);
            // Exactly one construct
            prop_assert_eq!(result.constructs.len(), 1,
                "expected exactly 1 construct");
            // Must be a Facts construct
            let Construct::Facts(ref facts) = result.constructs[0] else {
                return Err(TestCaseError::fail("expected Construct::Facts"));
            };
            // Name must round-trip
            prop_assert_eq!(&facts.name, &name,
                "deffacts name must match generated identifier");
            // Both facts must be recorded
            prop_assert_eq!(facts.facts.len(), 2,
                "deffacts must contain exactly 2 facts");
        }
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
            assert_eq!(rule.patterns.len(), 1);
            assert_eq!(rule.actions.len(), 1);
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
            assert_eq!(template.slots.len(), 1);
            assert_eq!(template.slots[0].name, "name");
            assert_eq!(template.slots[0].slot_type, SlotType::Single);
            assert!(template.slots[0].default.is_none());
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
            assert_eq!(facts.facts.len(), 1);
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
            assert_eq!(rule.patterns.len(), 1);
            assert_eq!(rule.actions.len(), 1);
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
            assert_eq!(rule.patterns.len(), 1);
            assert_eq!(rule.actions.len(), 1);
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
        // defclass is still unsupported; verify it produces the right error
        let parsed = parse_sexprs("(defclass Sensor (is-a USER))", file());
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].kind, InterpretErrorKind::UnknownConstruct);
        assert!(result.errors[0].message.contains("not yet supported"));
        assert!(result.constructs.is_empty());
    }

    #[test]
    fn interpret_deftemplate_with_comment() {
        let parsed = parse_sexprs(
            r#"(deftemplate person "Person template" (slot name))"#,
            file(),
        );
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty());
        assert_eq!(result.constructs.len(), 1);

        if let Construct::Template(template) = &result.constructs[0] {
            assert_eq!(template.name, "person");
            assert_eq!(template.comment, Some("Person template".to_string()));
            assert_eq!(template.slots.len(), 1);
        } else {
            panic!("expected Template construct");
        }
    }

    #[test]
    fn interpret_deffacts_with_comment() {
        let parsed = parse_sexprs(
            r#"(deffacts startup "Initial facts" (person Alice))"#,
            file(),
        );
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty());
        assert_eq!(result.constructs.len(), 1);

        if let Construct::Facts(facts) = &result.constructs[0] {
            assert_eq!(facts.name, "startup");
            assert_eq!(facts.comment, Some("Initial facts".to_string()));
            assert_eq!(facts.facts.len(), 1);
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
            assert_eq!(rule.patterns.len(), 3);
            assert_eq!(rule.actions.len(), 1);
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
            assert_eq!(rule.patterns.len(), 1);
            assert_eq!(rule.actions.len(), 3);
        } else {
            panic!("expected Rule construct");
        }
    }

    #[test]
    fn interpret_deftemplate_multiple_slots() {
        let parsed = parse_sexprs(
            "(deftemplate person (slot name) (slot age) (multislot hobbies))",
            file(),
        );
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty());
        assert_eq!(result.constructs.len(), 1);

        if let Construct::Template(template) = &result.constructs[0] {
            assert_eq!(template.slots.len(), 3);
            assert_eq!(template.slots[0].name, "name");
            assert_eq!(template.slots[0].slot_type, SlotType::Single);
            assert_eq!(template.slots[1].name, "age");
            assert_eq!(template.slots[1].slot_type, SlotType::Single);
            assert_eq!(template.slots[2].name, "hobbies");
            assert_eq!(template.slots[2].slot_type, SlotType::Multi);
        } else {
            panic!("expected Template construct");
        }
    }

    #[test]
    fn interpret_deffacts_multiple_facts() {
        let parsed = parse_sexprs(
            "(deffacts startup (person Alice) (person Bob) (setting debug on))",
            file(),
        );
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty());
        assert_eq!(result.constructs.len(), 1);

        if let Construct::Facts(facts) = &result.constructs[0] {
            assert_eq!(facts.facts.len(), 3);
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

    // ========================================================================
    // Pass 003 typed interpretation tests
    // ========================================================================

    #[test]
    fn interpret_template_with_default_value() {
        let parsed = parse_sexprs("(deftemplate person (slot age (default 0)))", file());
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty());
        assert_eq!(result.constructs.len(), 1);

        if let Construct::Template(template) = &result.constructs[0] {
            assert_eq!(template.slots.len(), 1);
            assert_eq!(template.slots[0].name, "age");
            assert!(template.slots[0].default.is_some());
            if let Some(DefaultValue::Value(lit)) = &template.slots[0].default {
                assert!(matches!(lit.value, LiteralKind::Integer(0)));
            } else {
                panic!("expected default value");
            }
        } else {
            panic!("expected Template construct");
        }
    }

    #[test]
    fn interpret_template_with_default_none() {
        let parsed = parse_sexprs("(deftemplate person (slot name (default ?NONE)))", file());
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty());

        if let Construct::Template(template) = &result.constructs[0] {
            assert!(matches!(
                template.slots[0].default,
                Some(DefaultValue::None)
            ));
        } else {
            panic!("expected Template construct");
        }
    }

    #[test]
    fn interpret_template_with_default_derive() {
        let parsed = parse_sexprs("(deftemplate person (slot id (default ?DERIVE)))", file());
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty());

        if let Construct::Template(template) = &result.constructs[0] {
            assert!(matches!(
                template.slots[0].default,
                Some(DefaultValue::Derive)
            ));
        } else {
            panic!("expected Template construct");
        }
    }

    #[test]
    fn interpret_ordered_pattern_with_literals() {
        let parsed = parse_sexprs(
            "(defrule test (person Alice 30) => (printout t ok))",
            file(),
        );
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty());

        if let Construct::Rule(rule) = &result.constructs[0] {
            assert_eq!(rule.patterns.len(), 1);
            if let Pattern::Ordered(ord) = &rule.patterns[0] {
                assert_eq!(ord.relation, "person");
                assert_eq!(ord.constraints.len(), 2);
                assert!(matches!(&ord.constraints[0], Constraint::Literal(_)));
                assert!(matches!(&ord.constraints[1], Constraint::Literal(_)));
            } else {
                panic!("expected ordered pattern");
            }
        } else {
            panic!("expected Rule construct");
        }
    }

    #[test]
    fn interpret_ordered_pattern_with_variables() {
        let parsed = parse_sexprs(
            "(defrule test (person ?name ?age) => (printout t ?name))",
            file(),
        );
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty());

        if let Construct::Rule(rule) = &result.constructs[0] {
            if let Pattern::Ordered(ord) = &rule.patterns[0] {
                assert_eq!(ord.constraints.len(), 2);
                assert!(matches!(&ord.constraints[0], Constraint::Variable(n, _) if n == "name"));
                assert!(matches!(&ord.constraints[1], Constraint::Variable(n, _) if n == "age"));
            } else {
                panic!("expected ordered pattern");
            }
        } else {
            panic!("expected Rule construct");
        }
    }

    #[test]
    fn interpret_ordered_pattern_with_wildcard() {
        let parsed = parse_sexprs(
            "(defrule test (person ? ?age) => (printout t ?age))",
            file(),
        );
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty());

        if let Construct::Rule(rule) = &result.constructs[0] {
            if let Pattern::Ordered(ord) = &rule.patterns[0] {
                assert_eq!(ord.constraints.len(), 2);
                assert!(matches!(&ord.constraints[0], Constraint::Wildcard(_)));
                assert!(matches!(&ord.constraints[1], Constraint::Variable(n, _) if n == "age"));
            } else {
                panic!("expected ordered pattern");
            }
        } else {
            panic!("expected Rule construct");
        }
    }

    #[test]
    fn interpret_template_pattern() {
        let parsed = parse_sexprs(
            "(defrule test (person (name ?n) (age ?a)) => (printout t ?n))",
            file(),
        );
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty());

        if let Construct::Rule(rule) = &result.constructs[0] {
            if let Pattern::Template(tmpl) = &rule.patterns[0] {
                assert_eq!(tmpl.template, "person");
                assert_eq!(tmpl.slot_constraints.len(), 2);
                assert_eq!(tmpl.slot_constraints[0].slot_name, "name");
                assert_eq!(tmpl.slot_constraints[1].slot_name, "age");
            } else {
                panic!("expected template pattern");
            }
        } else {
            panic!("expected Rule construct");
        }
    }

    #[test]
    fn interpret_negation_pattern() {
        let parsed = parse_sexprs(
            "(defrule test (not (blocker ?x)) => (assert (ok ?x)))",
            file(),
        );
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty());

        if let Construct::Rule(rule) = &result.constructs[0] {
            assert_eq!(rule.patterns.len(), 1);
            if let Pattern::Not(inner, _) = &rule.patterns[0] {
                if let Pattern::Ordered(ord) = inner.as_ref() {
                    assert_eq!(ord.relation, "blocker");
                    assert_eq!(ord.constraints.len(), 1);
                } else {
                    panic!("expected ordered inner pattern");
                }
            } else {
                panic!("expected not pattern");
            }
        } else {
            panic!("expected Rule construct");
        }
    }

    #[test]
    fn interpret_negation_pattern_missing_inner_pattern() {
        let parsed = parse_sexprs("(defrule test (not) => (assert (ok)))", file());
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert_eq!(result.errors.len(), 1);
        assert!(result.constructs.is_empty());

        let err = &result.errors[0];
        assert_eq!(err.kind, InterpretErrorKind::MissingElement);
        assert!(err.message.contains("pattern after 'not'"));
        assert_eq!(err.span.start.line, 1);
        assert_eq!(err.span.start.column, 16);
    }

    #[test]
    fn interpret_negation_pattern_rejects_multiple_inner_patterns() {
        let parsed = parse_sexprs(
            "(defrule test (not (blocker) (other)) => (assert (ok)))",
            file(),
        );
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert_eq!(result.errors.len(), 1);
        assert!(result.constructs.is_empty());

        let err = &result.errors[0];
        assert_eq!(err.kind, InterpretErrorKind::InvalidStructure);
        assert!(err.message.contains("'not' conditional element"));
        assert!(err.message.contains("exactly one pattern"));
        assert_eq!(err.span.start.line, 1);
        assert_eq!(err.span.start.column, 30);
    }

    #[test]
    fn interpret_negation_conjunction_pattern() {
        let parsed = parse_sexprs(
            "(defrule test (not (and (blocker ?x) (other ?x))) => (assert (ok ?x)))",
            file(),
        );
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

        if let Construct::Rule(rule) = &result.constructs[0] {
            assert_eq!(rule.patterns.len(), 1);
            if let Pattern::Not(inner, _) = &rule.patterns[0] {
                if let Pattern::And(parts, _) = inner.as_ref() {
                    assert_eq!(parts.len(), 2);
                    assert!(matches!(&parts[0], Pattern::Ordered(_)));
                    assert!(matches!(&parts[1], Pattern::Ordered(_)));
                } else {
                    panic!("expected and inside not");
                }
            } else {
                panic!("expected not pattern");
            }
        } else {
            panic!("expected Rule construct");
        }
    }

    #[test]
    fn interpret_test_pattern() {
        let parsed = parse_sexprs("(defrule test (test (> ?x 10)) => (assert (big)))", file());
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty());

        if let Construct::Rule(rule) = &result.constructs[0] {
            assert_eq!(rule.patterns.len(), 1);
            assert!(matches!(&rule.patterns[0], Pattern::Test(_, _)));
        } else {
            panic!("expected Rule construct");
        }
    }

    #[test]
    fn interpret_exists_pattern() {
        let parsed = parse_sexprs(
            "(defrule test (exists (person ?x)) => (assert (has-person)))",
            file(),
        );
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty());

        if let Construct::Rule(rule) = &result.constructs[0] {
            assert_eq!(rule.patterns.len(), 1);
            if let Pattern::Exists(patterns, _) = &rule.patterns[0] {
                assert_eq!(patterns.len(), 1);
            } else {
                panic!("expected exists pattern");
            }
        } else {
            panic!("expected Rule construct");
        }
    }

    #[test]
    fn interpret_action_with_nested_calls() {
        let parsed = parse_sexprs("(defrule test (x) => (printout t (+ 1 2) crlf))", file());
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty());

        if let Construct::Rule(rule) = &result.constructs[0] {
            assert_eq!(rule.actions.len(), 1);
            let action = &rule.actions[0];
            assert_eq!(action.call.name, "printout");
            assert_eq!(action.call.args.len(), 3);
            assert!(matches!(&action.call.args[1], ActionExpr::FunctionCall(_)));
        } else {
            panic!("expected Rule construct");
        }
    }

    #[test]
    fn interpret_action_fact_slot_access_compacts_var_colon_slot() {
        let parsed = parse_sexprs(
            "(defrule test ?p<-(point) => (bind ?x ?p:x) (printout t (+ ?p:x 1)))",
            file(),
        );
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

        let Construct::Rule(rule) = &result.constructs[0] else {
            panic!("expected Rule construct");
        };
        assert_eq!(rule.actions.len(), 2);

        let bind_call = &rule.actions[0].call;
        assert_eq!(bind_call.name, "bind");
        assert_eq!(bind_call.args.len(), 2);
        assert!(matches!(
            &bind_call.args[1],
            ActionExpr::FunctionCall(FunctionCall { name, args, .. })
                if name == FACT_SLOT_REF_FN
                    && args.len() == 2
                    && matches!(&args[0], ActionExpr::Variable(v, _) if v == "p")
        ));
    }

    #[test]
    fn interpret_deffacts_ordered() {
        let parsed = parse_sexprs(
            "(deffacts startup (person Alice 30) (person Bob 25))",
            file(),
        );
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty());

        if let Construct::Facts(facts) = &result.constructs[0] {
            assert_eq!(facts.facts.len(), 2);
            for fact in &facts.facts {
                if let FactBody::Ordered(ord) = fact {
                    assert_eq!(ord.relation, "person");
                    assert_eq!(ord.values.len(), 2);
                } else {
                    panic!("expected ordered fact");
                }
            }
        } else {
            panic!("expected Facts construct");
        }
    }

    #[test]
    fn interpret_deffacts_template() {
        let parsed = parse_sexprs("(deffacts startup (person (name Alice) (age 30)))", file());
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty());

        if let Construct::Facts(facts) = &result.constructs[0] {
            assert_eq!(facts.facts.len(), 1);
            if let FactBody::Template(tmpl) = &facts.facts[0] {
                assert_eq!(tmpl.template, "person");
                assert_eq!(tmpl.slot_values.len(), 2);
                assert_eq!(tmpl.slot_values[0].name, "name");
                assert_eq!(tmpl.slot_values[1].name, "age");
            } else {
                panic!("expected template fact");
            }
        } else {
            panic!("expected Facts construct");
        }
    }

    #[test]
    fn interpret_comprehensive_clips_example() {
        let source = r#"
            (deftemplate person
                (slot name)
                (slot age (default 0))
                (multislot hobbies))

            (defrule greet
                (person (name ?n) (age ?a))
                =>
                (printout t "Hello " ?n crlf))

            (defrule check-adult
                (person (name ?n) (age ?a))
                (not (minor ?n))
                =>
                (assert (adult ?n)))

            (deffacts initial
                (person (name Alice) (age 30))
                (setting debug on))
        "#;
        let parsed = parse_sexprs(source, file());
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);

        assert!(result.errors.is_empty());
        assert_eq!(result.constructs.len(), 4);

        // Verify template
        if let Construct::Template(tmpl) = &result.constructs[0] {
            assert_eq!(tmpl.name, "person");
            assert_eq!(tmpl.slots.len(), 3);
        } else {
            panic!("expected Template");
        }

        // Verify first rule
        if let Construct::Rule(rule) = &result.constructs[1] {
            assert_eq!(rule.name, "greet");
            assert_eq!(rule.patterns.len(), 1);
            assert_eq!(rule.actions.len(), 1);
        } else {
            panic!("expected Rule");
        }

        // Verify second rule with negation
        if let Construct::Rule(rule) = &result.constructs[2] {
            assert_eq!(rule.name, "check-adult");
            assert_eq!(rule.patterns.len(), 2);
            assert!(matches!(&rule.patterns[1], Pattern::Not(_, _)));
        } else {
            panic!("expected Rule");
        }

        // Verify deffacts
        if let Construct::Facts(facts) = &result.constructs[3] {
            assert_eq!(facts.name, "initial");
            assert_eq!(facts.facts.len(), 2);
        } else {
            panic!("expected Facts");
        }
    }

    #[test]
    fn interpret_error_empty_pattern() {
        let parsed = parse_sexprs("(defrule test () => (b))", file());
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].message.contains("empty pattern"));
    }

    #[test]
    fn interpret_error_empty_action() {
        let parsed = parse_sexprs("(defrule test (a) => ())", file());
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].message.contains("empty function call"));
    }

    #[test]
    fn interpret_field_alias_for_slot() {
        let parsed = parse_sexprs("(deftemplate person (field name))", file());
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(
            result.errors.is_empty(),
            "field should be accepted as alias for slot"
        );
        assert_eq!(result.constructs.len(), 1);
        if let Construct::Template(tmpl) = &result.constructs[0] {
            assert_eq!(tmpl.slots.len(), 1);
            assert_eq!(tmpl.slots[0].name, "name");
            assert_eq!(tmpl.slots[0].slot_type, SlotType::Single);
        } else {
            panic!("expected template construct");
        }
    }

    #[test]
    fn interpret_multifield_alias_for_multislot() {
        let parsed = parse_sexprs("(deftemplate data (multifield values))", file());
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(
            result.errors.is_empty(),
            "multifield should be accepted as alias for multislot"
        );
        assert_eq!(result.constructs.len(), 1);
        if let Construct::Template(tmpl) = &result.constructs[0] {
            assert_eq!(tmpl.slots.len(), 1);
            assert_eq!(tmpl.slots[0].name, "values");
            assert_eq!(tmpl.slots[0].slot_type, SlotType::Multi);
        } else {
            panic!("expected template construct");
        }
    }

    #[test]
    fn interpret_error_invalid_slot_keyword() {
        let parsed = parse_sexprs("(deftemplate person (bogus name))", file());
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].message.contains("slot"));
    }

    #[test]
    fn interpret_multivar_in_pattern() {
        let parsed = parse_sexprs("(defrule test (list $?items) => (printout t ok))", file());
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty());

        if let Construct::Rule(rule) = &result.constructs[0] {
            if let Pattern::Ordered(ord) = &rule.patterns[0] {
                assert_eq!(ord.constraints.len(), 1);
                assert!(
                    matches!(&ord.constraints[0], Constraint::MultiVariable(n, _) if n == "items")
                );
            } else {
                panic!("expected ordered pattern");
            }
        } else {
            panic!("expected Rule construct");
        }
    }

    // -----------------------------------------------------------------------
    // Pattern::Assigned (?var <- pattern) tests
    // -----------------------------------------------------------------------

    #[test]
    fn interpret_assigned_pattern_ordered() {
        let parsed = parse_sexprs(
            "(defrule cleanup ?f <- (temporary ?x) => (retract ?f))",
            file(),
        );
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        assert_eq!(result.constructs.len(), 1);

        if let Construct::Rule(rule) = &result.constructs[0] {
            assert_eq!(rule.patterns.len(), 1);
            if let Pattern::Assigned {
                variable, pattern, ..
            } = &rule.patterns[0]
            {
                assert_eq!(variable, "f");
                assert!(
                    matches!(pattern.as_ref(), Pattern::Ordered(o) if o.relation == "temporary")
                );
            } else {
                panic!("expected Assigned pattern, got {:?}", rule.patterns[0]);
            }
        } else {
            panic!("expected Rule construct");
        }
    }

    #[test]
    fn interpret_assigned_pattern_with_other_patterns() {
        let parsed = parse_sexprs(
            "(defrule test (trigger) ?f <- (item ?x) (other) => (retract ?f))",
            file(),
        );
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

        if let Construct::Rule(rule) = &result.constructs[0] {
            assert_eq!(rule.patterns.len(), 3);
            assert!(matches!(&rule.patterns[0], Pattern::Ordered(o) if o.relation == "trigger"));
            assert!(
                matches!(&rule.patterns[1], Pattern::Assigned { variable, .. } if variable == "f")
            );
            assert!(matches!(&rule.patterns[2], Pattern::Ordered(o) if o.relation == "other"));
        } else {
            panic!("expected Rule construct");
        }
    }

    #[test]
    fn interpret_multiple_assigned_patterns() {
        let parsed = parse_sexprs(
            "(defrule test ?a <- (alpha) ?b <- (beta) => (retract ?a))",
            file(),
        );
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

        if let Construct::Rule(rule) = &result.constructs[0] {
            assert_eq!(rule.patterns.len(), 2);
            assert!(
                matches!(&rule.patterns[0], Pattern::Assigned { variable, .. } if variable == "a")
            );
            assert!(
                matches!(&rule.patterns[1], Pattern::Assigned { variable, .. } if variable == "b")
            );
        } else {
            panic!("expected Rule construct");
        }
    }

    #[test]
    fn interpret_assigned_not_pattern() {
        // ?f <- (not (danger)) — while unusual, should parse correctly
        let parsed = parse_sexprs(
            "(defrule test ?f <- (not (danger)) => (printout t ok))",
            file(),
        );
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

        if let Construct::Rule(rule) = &result.constructs[0] {
            assert_eq!(rule.patterns.len(), 1);
            if let Pattern::Assigned {
                variable, pattern, ..
            } = &rule.patterns[0]
            {
                assert_eq!(variable, "f");
                assert!(matches!(pattern.as_ref(), Pattern::Not(..)));
            } else {
                panic!("expected Assigned pattern");
            }
        } else {
            panic!("expected Rule construct");
        }
    }

    // -----------------------------------------------------------------------
    // Pass 005: deffunction interpretation tests
    // -----------------------------------------------------------------------

    fn interpret_source_inner(source: &str) -> InterpretResult {
        let parsed = parse_sexprs(source, file());
        assert!(
            parsed.errors.is_empty(),
            "parse errors: {:?}",
            parsed.errors
        );
        interpret_constructs(&parsed.exprs, &InterpreterConfig::default())
    }

    #[test]
    fn interpret_deffunction_simple() {
        let result = interpret_source_inner("(deffunction add-one (?x) (+ ?x 1))");
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        assert_eq!(result.constructs.len(), 1);
        match &result.constructs[0] {
            Construct::Function(func) => {
                assert_eq!(func.name, "add-one");
                assert_eq!(func.parameters, vec!["x"]);
                assert!(func.wildcard_parameter.is_none());
                assert_eq!(func.body.len(), 1);
                assert!(func.comment.is_none());
            }
            other => panic!("expected Function, got {other:?}"),
        }
    }

    #[test]
    fn interpret_deffunction_with_comment() {
        let result = interpret_source_inner(r#"(deffunction inc "Increment by 1" (?x) (+ ?x 1))"#);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        match &result.constructs[0] {
            Construct::Function(func) => {
                assert_eq!(func.comment, Some("Increment by 1".to_string()));
                assert_eq!(func.name, "inc");
            }
            other => panic!("expected Function, got {other:?}"),
        }
    }

    #[test]
    fn interpret_deffunction_with_wildcard_only() {
        let result = interpret_source_inner("(deffunction sum-all ($?values) (+ $?values))");
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        match &result.constructs[0] {
            Construct::Function(func) => {
                assert!(func.parameters.is_empty());
                assert_eq!(func.wildcard_parameter, Some("values".to_string()));
                assert_eq!(func.body.len(), 1);
            }
            other => panic!("expected Function, got {other:?}"),
        }
    }

    #[test]
    fn interpret_deffunction_mixed_params() {
        let result = interpret_source_inner(
            "(deffunction fmt (?prefix $?rest) (printout t ?prefix $?rest))",
        );
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        match &result.constructs[0] {
            Construct::Function(func) => {
                assert_eq!(func.parameters, vec!["prefix"]);
                assert_eq!(func.wildcard_parameter, Some("rest".to_string()));
            }
            other => panic!("expected Function, got {other:?}"),
        }
    }

    #[test]
    fn interpret_deffunction_multiple_body_expressions() {
        let result = interpret_source_inner(
            "(deffunction two-steps (?x) (assert (step1)) (assert (step2)))",
        );
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        match &result.constructs[0] {
            Construct::Function(func) => {
                assert_eq!(func.body.len(), 2);
            }
            other => panic!("expected Function, got {other:?}"),
        }
    }

    #[test]
    fn interpret_deffunction_no_params() {
        let result = interpret_source_inner("(deffunction greet () (printout t hello crlf))");
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        match &result.constructs[0] {
            Construct::Function(func) => {
                assert!(func.parameters.is_empty());
                assert!(func.wildcard_parameter.is_none());
                assert_eq!(func.body.len(), 1);
            }
            other => panic!("expected Function, got {other:?}"),
        }
    }

    #[test]
    fn interpret_deffunction_qualified_global_reference() {
        let result = interpret_source_inner("(deffunction get-threshold () ?*CONFIG::threshold*)");
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        match &result.constructs[0] {
            Construct::Function(func) => {
                assert_eq!(func.body.len(), 1);
                assert!(matches!(
                    &func.body[0],
                    ActionExpr::GlobalVariable(name, _) if name == "CONFIG::threshold"
                ));
            }
            other => panic!("expected Function, got {other:?}"),
        }
    }

    #[test]
    fn interpret_deffunction_missing_name_errors() {
        let result = interpret_source_inner("(deffunction)");
        assert!(!result.errors.is_empty());
        assert!(matches!(
            result.errors[0].kind,
            InterpretErrorKind::MissingElement
        ));
    }

    #[test]
    fn interpret_deffunction_missing_params_errors() {
        let result = interpret_source_inner("(deffunction foo)");
        assert!(!result.errors.is_empty());
    }

    #[test]
    fn interpret_deffunction_missing_body_errors() {
        let result = interpret_source_inner("(deffunction foo (?x))");
        assert!(!result.errors.is_empty());
        assert!(matches!(
            result.errors[0].kind,
            InterpretErrorKind::MissingElement
        ));
    }

    #[test]
    fn interpret_deffunction_non_symbol_name_errors() {
        let result = interpret_source_inner("(deffunction 42 (?x) (+ ?x 1))");
        assert!(!result.errors.is_empty());
        assert!(matches!(
            result.errors[0].kind,
            InterpretErrorKind::InvalidStructure
        ));
    }

    #[test]
    fn interpret_deffunction_non_list_params_errors() {
        let result = interpret_source_inner("(deffunction foo ?x (+ ?x 1))");
        assert!(!result.errors.is_empty());
        assert!(matches!(
            result.errors[0].kind,
            InterpretErrorKind::InvalidStructure
        ));
    }

    #[test]
    fn interpret_deffunction_duplicate_wildcard_errors() {
        let result = interpret_source_inner("(deffunction foo ($?a $?b) (+ 1 2))");
        assert!(!result.errors.is_empty());
        assert!(matches!(
            result.errors[0].kind,
            InterpretErrorKind::InvalidStructure
        ));
    }

    #[test]
    fn interpret_deffunction_param_after_wildcard_errors() {
        let result = interpret_source_inner("(deffunction foo ($?a ?b) (+ 1 2))");
        assert!(!result.errors.is_empty());
        assert!(matches!(
            result.errors[0].kind,
            InterpretErrorKind::InvalidStructure
        ));
    }

    // -----------------------------------------------------------------------
    // Pass 005: defglobal interpretation tests
    // -----------------------------------------------------------------------

    #[test]
    fn interpret_defglobal_single() {
        let result = interpret_source_inner("(defglobal ?*threshold* = 50)");
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        assert_eq!(result.constructs.len(), 1);
        match &result.constructs[0] {
            Construct::Global(global) => {
                assert_eq!(global.globals.len(), 1);
                assert_eq!(global.globals[0].name, "threshold");
            }
            other => panic!("expected Global, got {other:?}"),
        }
    }

    #[test]
    fn interpret_defglobal_multiple() {
        let result = interpret_source_inner("(defglobal ?*a* = 1 ?*b* = 2 ?*c* = 3)");
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        match &result.constructs[0] {
            Construct::Global(global) => {
                assert_eq!(global.globals.len(), 3);
                assert_eq!(global.globals[0].name, "a");
                assert_eq!(global.globals[1].name, "b");
                assert_eq!(global.globals[2].name, "c");
            }
            other => panic!("expected Global, got {other:?}"),
        }
    }

    #[test]
    fn interpret_defglobal_float_value() {
        let result = interpret_source_inner("(defglobal ?*pi* = 3.14159)");
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        match &result.constructs[0] {
            Construct::Global(global) => {
                assert_eq!(global.globals.len(), 1);
                assert_eq!(global.globals[0].name, "pi");
                assert!(matches!(
                    &global.globals[0].value,
                    ActionExpr::Literal(lit) if matches!(lit.value, LiteralKind::Float(_))
                ));
            }
            other => panic!("expected Global, got {other:?}"),
        }
    }

    #[test]
    fn interpret_defglobal_expression_value() {
        let result = interpret_source_inner("(defglobal ?*doubled* = (* 2 3))");
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        match &result.constructs[0] {
            Construct::Global(global) => {
                assert!(matches!(
                    &global.globals[0].value,
                    ActionExpr::FunctionCall(_)
                ));
            }
            other => panic!("expected Global, got {other:?}"),
        }
    }

    #[test]
    fn interpret_defglobal_empty_errors() {
        let result = interpret_source_inner("(defglobal)");
        assert!(!result.errors.is_empty());
        assert!(matches!(
            result.errors[0].kind,
            InterpretErrorKind::MissingElement
        ));
    }

    #[test]
    fn interpret_defglobal_missing_equals_errors() {
        let result = interpret_source_inner("(defglobal ?*x* 50)");
        assert!(!result.errors.is_empty());
        assert!(matches!(
            result.errors[0].kind,
            InterpretErrorKind::InvalidStructure
        ));
    }

    #[test]
    fn interpret_defglobal_missing_value_errors() {
        let result = interpret_source_inner("(defglobal ?*x* =)");
        assert!(!result.errors.is_empty());
        assert!(matches!(
            result.errors[0].kind,
            InterpretErrorKind::MissingElement
        ));
    }

    #[test]
    fn interpret_defglobal_non_global_var_errors() {
        // ?x is a SingleVar, not a GlobalVar
        let result = interpret_source_inner("(defglobal ?x = 50)");
        assert!(!result.errors.is_empty());
        assert!(matches!(
            result.errors[0].kind,
            InterpretErrorKind::InvalidStructure
        ));
    }

    // -----------------------------------------------------------------------
    // Pass 007: defmodule interpretation tests
    // -----------------------------------------------------------------------

    #[test]
    fn interpret_defmodule_simple_no_specs() {
        let result = interpret_source_inner("(defmodule MAIN)");
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        assert_eq!(result.constructs.len(), 1);
        let Construct::Module(m) = &result.constructs[0] else {
            panic!("expected Module construct");
        };
        assert_eq!(m.name, "MAIN");
        assert!(m.comment.is_none());
        assert!(m.exports.is_empty());
        assert!(m.imports.is_empty());
    }

    #[test]
    fn interpret_defmodule_with_export_all() {
        let result = interpret_source_inner("(defmodule MAIN (export ?ALL))");
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        assert_eq!(result.constructs.len(), 1);
        let Construct::Module(m) = &result.constructs[0] else {
            panic!("expected Module construct");
        };
        assert_eq!(m.name, "MAIN");
        assert_eq!(m.exports.len(), 1);
        assert!(matches!(m.exports[0], ModuleSpec::All));
    }

    #[test]
    fn interpret_defmodule_with_export_none() {
        let result = interpret_source_inner("(defmodule MAIN (export ?NONE))");
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let Construct::Module(m) = &result.constructs[0] else {
            panic!("expected Module construct");
        };
        assert_eq!(m.exports.len(), 1);
        assert!(matches!(m.exports[0], ModuleSpec::None));
    }

    #[test]
    fn interpret_defmodule_with_specific_exports() {
        let result = interpret_source_inner("(defmodule MAIN (export deftemplate reading sensor))");
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let Construct::Module(m) = &result.constructs[0] else {
            panic!("expected Module construct");
        };
        assert_eq!(m.exports.len(), 1);
        let ModuleSpec::Specific {
            construct_type,
            names,
        } = &m.exports[0]
        else {
            panic!("expected Specific export");
        };
        assert_eq!(construct_type, "deftemplate");
        assert_eq!(names, &["reading", "sensor"]);
    }

    #[test]
    fn interpret_defmodule_with_import() {
        let result = interpret_source_inner("(defmodule SENSOR (import MAIN ?ALL))");
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let Construct::Module(m) = &result.constructs[0] else {
            panic!("expected Module construct");
        };
        assert_eq!(m.name, "SENSOR");
        assert_eq!(m.imports.len(), 1);
        assert_eq!(m.imports[0].module_name, "MAIN");
        assert!(matches!(m.imports[0].spec, ModuleSpec::All));
    }

    #[test]
    fn interpret_defmodule_with_import_and_export() {
        let result = interpret_source_inner(
            "(defmodule SENSOR (import MAIN ?ALL) (export deftemplate reading))",
        );
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let Construct::Module(m) = &result.constructs[0] else {
            panic!("expected Module construct");
        };
        assert_eq!(m.imports.len(), 1);
        assert_eq!(m.exports.len(), 1);
    }

    #[test]
    fn interpret_defmodule_with_comment() {
        let result = interpret_source_inner(r#"(defmodule MAIN "Main module" (export ?ALL))"#);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let Construct::Module(m) = &result.constructs[0] else {
            panic!("expected Module construct");
        };
        assert_eq!(m.comment.as_deref(), Some("Main module"));
        assert_eq!(m.exports.len(), 1);
    }

    #[test]
    fn interpret_defmodule_missing_name_errors() {
        let result = interpret_source_inner("(defmodule)");
        assert!(!result.errors.is_empty());
        assert!(matches!(
            result.errors[0].kind,
            InterpretErrorKind::MissingElement
        ));
    }

    #[test]
    fn interpret_defmodule_invalid_spec_keyword_errors() {
        let result = interpret_source_inner("(defmodule MAIN (reexport ?ALL))");
        assert!(!result.errors.is_empty());
        assert!(matches!(
            result.errors[0].kind,
            InterpretErrorKind::InvalidStructure
        ));
    }

    // -----------------------------------------------------------------------
    // Pass 007: defgeneric interpretation tests
    // -----------------------------------------------------------------------

    #[test]
    fn interpret_defgeneric_simple() {
        let result = interpret_source_inner("(defgeneric display)");
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        assert_eq!(result.constructs.len(), 1);
        let Construct::Generic(g) = &result.constructs[0] else {
            panic!("expected Generic construct");
        };
        assert_eq!(g.name, "display");
        assert!(g.comment.is_none());
    }

    #[test]
    fn interpret_defgeneric_with_comment() {
        let result = interpret_source_inner(r#"(defgeneric display "Display any value")"#);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let Construct::Generic(g) = &result.constructs[0] else {
            panic!("expected Generic construct");
        };
        assert_eq!(g.name, "display");
        assert_eq!(g.comment.as_deref(), Some("Display any value"));
    }

    #[test]
    fn interpret_defgeneric_missing_name_errors() {
        let result = interpret_source_inner("(defgeneric)");
        assert!(!result.errors.is_empty());
        assert!(matches!(
            result.errors[0].kind,
            InterpretErrorKind::MissingElement
        ));
    }

    // -----------------------------------------------------------------------
    // Pass 007: defmethod interpretation tests
    // -----------------------------------------------------------------------

    #[test]
    fn interpret_defmethod_simple_typed_param() {
        let result = interpret_source_inner("(defmethod display ((?x INTEGER)) (printout t ?x))");
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        assert_eq!(result.constructs.len(), 1);
        let Construct::Method(m) = &result.constructs[0] else {
            panic!("expected Method construct");
        };
        assert_eq!(m.name, "display");
        assert!(m.index.is_none());
        assert_eq!(m.parameters.len(), 1);
        assert_eq!(m.parameters[0].name, "x");
        assert_eq!(m.parameters[0].type_restrictions, ["INTEGER"]);
        assert!(m.wildcard_parameter.is_none());
        assert_eq!(m.body.len(), 1);
    }

    #[test]
    fn interpret_defmethod_with_explicit_index() {
        let result = interpret_source_inner("(defmethod display 1 ((?x INTEGER)) (printout t ?x))");
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let Construct::Method(m) = &result.constructs[0] else {
            panic!("expected Method construct");
        };
        assert_eq!(m.index, Some(1));
        assert_eq!(m.parameters.len(), 1);
    }

    #[test]
    fn interpret_defmethod_multiple_params() {
        let result =
            interpret_source_inner("(defmethod display ((?x INTEGER) (?y FLOAT)) (+ ?x ?y))");
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let Construct::Method(m) = &result.constructs[0] else {
            panic!("expected Method construct");
        };
        assert_eq!(m.parameters.len(), 2);
        assert_eq!(m.parameters[0].name, "x");
        assert_eq!(m.parameters[0].type_restrictions, ["INTEGER"]);
        assert_eq!(m.parameters[1].name, "y");
        assert_eq!(m.parameters[1].type_restrictions, ["FLOAT"]);
    }

    #[test]
    fn interpret_defmethod_multi_type_restriction() {
        let result = interpret_source_inner("(defmethod display ((?x INTEGER FLOAT)) ?x)");
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let Construct::Method(m) = &result.constructs[0] else {
            panic!("expected Method construct");
        };
        assert_eq!(m.parameters.len(), 1);
        assert_eq!(m.parameters[0].type_restrictions, ["INTEGER", "FLOAT"]);
    }

    #[test]
    fn interpret_defmethod_untyped_param() {
        let result = interpret_source_inner("(defmethod display ((?x)) ?x)");
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let Construct::Method(m) = &result.constructs[0] else {
            panic!("expected Method construct");
        };
        assert_eq!(m.parameters.len(), 1);
        assert_eq!(m.parameters[0].name, "x");
        assert!(m.parameters[0].type_restrictions.is_empty());
    }

    #[test]
    fn interpret_defmethod_wildcard_param() {
        let result = interpret_source_inner("(defmethod display ((?x) $?rest) ?x)");
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let Construct::Method(m) = &result.constructs[0] else {
            panic!("expected Method construct");
        };
        assert_eq!(m.parameters.len(), 1);
        assert_eq!(m.wildcard_parameter.as_deref(), Some("rest"));
    }

    #[test]
    fn interpret_defmethod_missing_name_errors() {
        let result = interpret_source_inner("(defmethod)");
        assert!(!result.errors.is_empty());
        assert!(matches!(
            result.errors[0].kind,
            InterpretErrorKind::MissingElement
        ));
    }

    #[test]
    fn interpret_defmethod_missing_param_list_errors() {
        let result = interpret_source_inner("(defmethod display)");
        assert!(!result.errors.is_empty());
        assert!(matches!(
            result.errors[0].kind,
            InterpretErrorKind::MissingElement
        ));
    }

    #[test]
    fn interpret_defmethod_missing_body_errors() {
        let result = interpret_source_inner("(defmethod display ((?x INTEGER)))");
        assert!(!result.errors.is_empty());
        assert!(matches!(
            result.errors[0].kind,
            InterpretErrorKind::MissingElement
        ));
    }

    #[test]
    fn interpret_defmethod_empty_param_restriction_errors() {
        let result = interpret_source_inner("(defmethod display (()) ?x)");
        assert!(!result.errors.is_empty());
        assert!(matches!(
            result.errors[0].kind,
            InterpretErrorKind::InvalidStructure
        ));
    }

    // -----------------------------------------------------------------------
    // Pass 010: forall interpretation tests
    // -----------------------------------------------------------------------

    #[test]
    fn interpret_forall_basic() {
        let result = interpret_source_inner(
            r"
            (defrule all-checked
                (forall (item ?id) (checked ?id))
                =>
                (assert (all-complete)))
            ",
        );
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        assert_eq!(result.constructs.len(), 1);
        if let Construct::Rule(rule) = &result.constructs[0] {
            assert_eq!(rule.name, "all-checked");
            assert_eq!(rule.patterns.len(), 1);
            assert!(
                matches!(&rule.patterns[0], Pattern::Forall(pats, _) if pats.len() == 2),
                "expected Forall with 2 sub-patterns, got {:?}",
                rule.patterns[0]
            );
        } else {
            panic!("expected Rule");
        }
    }

    #[test]
    fn interpret_forall_too_few_patterns_errors() {
        let result = interpret_source_inner(
            r"
            (defrule bad
                (forall (item ?id))
                =>
                (assert (done)))
            ",
        );
        assert!(
            !result.errors.is_empty(),
            "expected an error for forall with too few patterns"
        );
        assert!(matches!(
            result.errors[0].kind,
            InterpretErrorKind::MissingElement
        ));
    }

    // -----------------------------------------------------------------------
    // Connective constraint tests
    // -----------------------------------------------------------------------

    #[test]
    fn interpret_ordered_connective_and() {
        // ?x&~red should parse as And([Variable("x"), Not(Literal("red"))])
        let parsed = parse_sexprs("(defrule test (data ?x&~red) => (printout t ok))", file());
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let Construct::Rule(rule) = &result.constructs[0] else {
            panic!("expected Rule");
        };
        // The LHS has one pattern: (data ?x&~red)
        let Pattern::Ordered(op) = &rule.patterns[0] else {
            panic!("expected ordered pattern");
        };
        assert_eq!(op.constraints.len(), 1);
        let Constraint::And(terms, _) = &op.constraints[0] else {
            panic!("expected And constraint, got {:?}", op.constraints[0]);
        };
        assert_eq!(terms.len(), 2);
        assert!(matches!(&terms[0], Constraint::Variable(name, _) if name == "x"));
        let Constraint::Not(inner, _) = &terms[1] else {
            panic!("expected Not constraint");
        };
        assert!(
            matches!(inner.as_ref(), Constraint::Literal(lit) if matches!(&lit.value, LiteralKind::Symbol(s) if s == "red"))
        );
    }

    #[test]
    fn interpret_ordered_connective_or() {
        // a|b should parse as Or([Literal("a"), Literal("b")])
        let parsed = parse_sexprs("(defrule test (data a|b) => (printout t ok))", file());
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let Construct::Rule(rule) = &result.constructs[0] else {
            panic!("expected Rule");
        };
        let Pattern::Ordered(op) = &rule.patterns[0] else {
            panic!("expected ordered pattern");
        };
        assert_eq!(op.constraints.len(), 1);
        let Constraint::Or(terms, _) = &op.constraints[0] else {
            panic!("expected Or constraint, got {:?}", op.constraints[0]);
        };
        assert_eq!(terms.len(), 2);
        assert!(
            matches!(&terms[0], Constraint::Literal(lit) if matches!(&lit.value, LiteralKind::Symbol(s) if s == "a"))
        );
        assert!(
            matches!(&terms[1], Constraint::Literal(lit) if matches!(&lit.value, LiteralKind::Symbol(s) if s == "b"))
        );
    }

    #[test]
    fn interpret_template_connective_constraint() {
        let parsed = parse_sexprs(
            r"
            (deftemplate item (slot color))
            (defrule test (item (color ?c&~red)) => (printout t ok))
            ",
            file(),
        );
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let Construct::Rule(rule) = &result.constructs[1] else {
            panic!("expected Rule as second construct");
        };
        let Pattern::Template(tp) = &rule.patterns[0] else {
            panic!("expected template pattern");
        };
        assert_eq!(tp.slot_constraints.len(), 1);
        let sc = &tp.slot_constraints[0];
        assert_eq!(sc.slot_name, "color");
        let Constraint::And(terms, _) = &sc.constraint else {
            panic!("expected And constraint in slot, got {:?}", sc.constraint);
        };
        assert_eq!(terms.len(), 2);
        assert!(matches!(&terms[0], Constraint::Variable(name, _) if name == "c"));
        assert!(matches!(&terms[1], Constraint::Not(_, _)));
    }

    #[test]
    fn interpret_ordered_predicate_constraint_preserved() {
        let parsed = parse_sexprs(
            "(defrule test (data ?x&:(> ?x 3)) => (printout t ok))",
            file(),
        );
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let Construct::Rule(rule) = &result.constructs[0] else {
            panic!("expected Rule");
        };
        let Pattern::Ordered(op) = &rule.patterns[0] else {
            panic!("expected ordered pattern");
        };
        let Constraint::And(terms, _) = &op.constraints[0] else {
            panic!("expected And constraint, got {:?}", op.constraints[0]);
        };
        assert_eq!(terms.len(), 2);
        assert!(matches!(&terms[0], Constraint::Variable(name, _) if name == "x"));
        assert!(matches!(
            &terms[1],
            Constraint::Predicate(SExpr::List(items, _), _) if items.len() == 3
        ));
    }

    #[test]
    fn interpret_ordered_return_value_constraint_preserved() {
        let parsed = parse_sexprs("(defrule test (data =(+ 1 2)) => (printout t ok))", file());
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let Construct::Rule(rule) = &result.constructs[0] else {
            panic!("expected Rule");
        };
        let Pattern::Ordered(op) = &rule.patterns[0] else {
            panic!("expected ordered pattern");
        };
        assert!(matches!(
            &op.constraints[0],
            Constraint::ReturnValue(SExpr::List(items, _), _) if items.len() == 3
        ));
    }

    #[test]
    fn interpret_negation_constraint() {
        // ~red should parse as Not(Literal("red"))
        let parsed = parse_sexprs("(defrule test (data ~red) => (printout t ok))", file());
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let Construct::Rule(rule) = &result.constructs[0] else {
            panic!("expected Rule");
        };
        let Pattern::Ordered(op) = &rule.patterns[0] else {
            panic!("expected ordered pattern");
        };
        assert_eq!(op.constraints.len(), 1);
        let Constraint::Not(inner, _) = &op.constraints[0] else {
            panic!("expected Not constraint, got {:?}", op.constraints[0]);
        };
        assert!(
            matches!(inner.as_ref(), Constraint::Literal(lit) if matches!(&lit.value, LiteralKind::Symbol(s) if s == "red"))
        );
    }

    // -------------------------------------------------------------------------
    // if/then/else parsing tests
    // -------------------------------------------------------------------------

    #[test]
    fn interpret_if_then_else_action() {
        let parsed = parse_sexprs(
            "(defrule test (data ?x) => (if (> ?x 0) then (assert (positive)) else (assert (negative))))",
            file(),
        );
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let Construct::Rule(rule) = &result.constructs[0] else {
            panic!("expected Rule construct");
        };
        assert_eq!(rule.actions.len(), 1);
        let ActionExpr::If {
            then_actions,
            else_actions,
            ..
        } = &rule.actions[0].call.args[0]
        else {
            panic!(
                "expected If in action args, got {:?}",
                rule.actions[0].call.args[0]
            );
        };
        assert_eq!(then_actions.len(), 1, "then branch should have one action");
        assert_eq!(else_actions.len(), 1, "else branch should have one action");
    }

    #[test]
    fn interpret_if_then_no_else_action() {
        let parsed = parse_sexprs(
            "(defrule test (data ?x) => (if (> ?x 0) then (assert (positive))))",
            file(),
        );
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let Construct::Rule(rule) = &result.constructs[0] else {
            panic!("expected Rule construct");
        };
        assert_eq!(rule.actions.len(), 1);
        let ActionExpr::If {
            then_actions,
            else_actions,
            ..
        } = &rule.actions[0].call.args[0]
        else {
            panic!("expected If in action args");
        };
        assert_eq!(then_actions.len(), 1);
        assert!(else_actions.is_empty(), "else branch should be empty");
    }

    #[test]
    fn interpret_if_then_multiple_actions_in_branch() {
        let parsed = parse_sexprs(
            "(defrule test (data ?x) => (if (> ?x 0) then (assert (p)) (assert (q)) else (assert (r))))",
            file(),
        );
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let Construct::Rule(rule) = &result.constructs[0] else {
            panic!("expected Rule construct");
        };
        let ActionExpr::If {
            then_actions,
            else_actions,
            ..
        } = &rule.actions[0].call.args[0]
        else {
            panic!("expected If in action args");
        };
        assert_eq!(then_actions.len(), 2, "then branch should have two actions");
        assert_eq!(else_actions.len(), 1, "else branch should have one action");
    }

    #[test]
    fn interpret_if_missing_then_keyword_is_error() {
        // `then` keyword is missing — should produce an error
        let parsed = parse_sexprs(
            "(defrule test (data ?x) => (if (> ?x 0) (assert (positive))))",
            file(),
        );
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(
            !result.errors.is_empty(),
            "should have reported a missing `then` error"
        );
    }

    #[test]
    fn interpret_if_in_deffunction_body() {
        let parsed = parse_sexprs(
            "(deffunction classify (?x) (if (> ?x 0) then positive else negative))",
            file(),
        );
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let Construct::Function(func) = &result.constructs[0] else {
            panic!("expected Function construct");
        };
        assert_eq!(func.body.len(), 1);
        assert!(
            matches!(&func.body[0], ActionExpr::If { .. }),
            "expected If in function body"
        );
    }

    // =========================================================================
    // Loop special form tests
    // =========================================================================

    #[test]
    fn interpret_while_action() {
        let parsed = parse_sexprs(
            "(defrule test (data ?x) => (while (> ?x 0) do (printout t ?x)))",
            file(),
        );
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let Construct::Rule(rule) = &result.constructs[0] else {
            panic!("expected Rule construct");
        };
        // Top-level action should be a synthetic FunctionCall wrapping a While.
        assert_eq!(rule.actions.len(), 1);
        assert_eq!(rule.actions[0].call.name, "while");
        assert_eq!(rule.actions[0].call.args.len(), 1);
        assert!(
            matches!(&rule.actions[0].call.args[0], ActionExpr::While { .. }),
            "expected While variant in args[0]"
        );
    }

    #[test]
    fn interpret_while_body_has_correct_actions() {
        let parsed = parse_sexprs(
            "(defrule test => (while (> ?x 1) do (printout t hello) (printout t world)))",
            file(),
        );
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let Construct::Rule(rule) = &result.constructs[0] else {
            panic!("expected Rule construct");
        };
        let ActionExpr::While { body, .. } = &rule.actions[0].call.args[0] else {
            panic!("expected While");
        };
        assert_eq!(body.len(), 2, "while body should have 2 actions");
    }

    #[test]
    fn interpret_loop_for_count_action() {
        let parsed = parse_sexprs(
            "(defrule test => (loop-for-count (?i 1 10) do (printout t ?i)))",
            file(),
        );
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let Construct::Rule(rule) = &result.constructs[0] else {
            panic!("expected Rule construct");
        };
        assert_eq!(rule.actions[0].call.name, "loop-for-count");
        let ActionExpr::LoopForCount { var_name, .. } = &rule.actions[0].call.args[0] else {
            panic!("expected LoopForCount");
        };
        assert_eq!(var_name.as_deref(), Some("i"));
    }

    #[test]
    fn interpret_loop_for_count_two_arg_spec() {
        // `(?var end)` form — start defaults to 1.
        let parsed = parse_sexprs(
            "(defrule test => (loop-for-count (?i 5) do (printout t ?i)))",
            file(),
        );
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let Construct::Rule(rule) = &result.constructs[0] else {
            panic!("expected Rule construct");
        };
        let ActionExpr::LoopForCount {
            var_name, start, ..
        } = &rule.actions[0].call.args[0]
        else {
            panic!("expected LoopForCount");
        };
        assert_eq!(var_name.as_deref(), Some("i"));
        // Start should be the literal integer 1 (default).
        assert!(
            matches!(start.as_ref(), ActionExpr::Literal(lit) if matches!(lit.value, LiteralKind::Integer(1))),
            "expected start = 1"
        );
    }

    #[test]
    fn interpret_loop_for_count_anonymous() {
        // `(end)` form — anonymous counter.
        let parsed = parse_sexprs(
            "(defrule test => (loop-for-count (5) do (printout t hi)))",
            file(),
        );
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let Construct::Rule(rule) = &result.constructs[0] else {
            panic!("expected Rule construct");
        };
        let ActionExpr::LoopForCount { var_name, .. } = &rule.actions[0].call.args[0] else {
            panic!("expected LoopForCount");
        };
        assert!(
            var_name.is_none(),
            "anonymous counter should have no var_name"
        );
    }

    #[test]
    fn interpret_progn_dollar_action() {
        let parsed = parse_sexprs(
            "(defrule test (data $?items) => (progn$ (?item ?items) (printout t ?item)))",
            file(),
        );
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let Construct::Rule(rule) = &result.constructs[0] else {
            panic!("expected Rule construct");
        };
        assert_eq!(rule.actions[0].call.name, "progn$");
        let ActionExpr::Progn { var_name, .. } = &rule.actions[0].call.args[0] else {
            panic!("expected Progn");
        };
        assert_eq!(var_name, "item");
    }

    #[test]
    fn interpret_foreach_action() {
        let parsed = parse_sexprs(
            "(defrule test (data $?items) => (foreach ?item ?items do (printout t ?item)))",
            file(),
        );
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let Construct::Rule(rule) = &result.constructs[0] else {
            panic!("expected Rule construct");
        };
        // foreach is translated to a Progn variant wrapped in a "foreach" call.
        assert_eq!(rule.actions[0].call.name, "foreach");
        let ActionExpr::Progn { var_name, .. } = &rule.actions[0].call.args[0] else {
            panic!("expected Progn");
        };
        assert_eq!(var_name, "item");
    }

    #[test]
    fn interpret_while_in_deffunction_body() {
        let parsed = parse_sexprs(
            "(deffunction count-down (?n) (while (> ?n 0) do (bind ?*n* (- ?n 1))))",
            file(),
        );
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let Construct::Function(func) = &result.constructs[0] else {
            panic!("expected Function construct");
        };
        assert_eq!(func.body.len(), 1);
        assert!(
            matches!(&func.body[0], ActionExpr::While { .. }),
            "expected While in function body"
        );
    }

    #[test]
    fn interpret_while_optional_do() {
        // `do` keyword is optional in while — both forms should parse successfully
        let parsed = parse_sexprs("(defrule test => (while (> ?x 0) (printout t ?x)))", file());
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty(), "while without do should succeed");
    }

    #[test]
    fn interpret_loop_for_count_optional_do() {
        // `do` keyword is optional in loop-for-count — both forms should parse
        let parsed = parse_sexprs(
            "(defrule test => (loop-for-count (?i 1 5) (printout t ?i)))",
            file(),
        );
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(
            result.errors.is_empty(),
            "loop-for-count without do should succeed"
        );
    }

    // -----------------------------------------------------------------------
    // Fact-query macro forms (do-for-fact, any-factp, etc.)
    // -----------------------------------------------------------------------

    #[test]
    fn interpret_do_for_fact_action() {
        let parsed = parse_sexprs(
            "(defrule test => (do-for-fact ((?f data)) TRUE (printout t ?f)))",
            file(),
        );
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let Construct::Rule(rule) = &result.constructs[0] else {
            panic!("expected Rule construct");
        };
        assert_eq!(rule.actions.len(), 1);
        assert_eq!(rule.actions[0].call.name, "do-for-fact");
        // The sole argument is the wrapped QueryAction expression.
        assert_eq!(rule.actions[0].call.args.len(), 1);
        assert!(
            matches!(&rule.actions[0].call.args[0], ActionExpr::QueryAction { name, bindings, body, .. }
                if name == "do-for-fact" && bindings.len() == 1 && body.len() == 1),
            "unexpected QueryAction shape"
        );
    }

    #[test]
    fn interpret_do_for_all_facts_action() {
        let parsed = parse_sexprs(
            r"(defrule test => (do-for-all-facts ((?f data)) (> (fact-slot-value ?f val) 0) (printout t ?f crlf)))",
            file(),
        );
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let Construct::Rule(rule) = &result.constructs[0] else {
            panic!("expected Rule");
        };
        assert_eq!(rule.actions[0].call.name, "do-for-all-facts");
        assert!(
            matches!(&rule.actions[0].call.args[0], ActionExpr::QueryAction { bindings, body, .. }
                if bindings.len() == 1 && body.len() == 1)
        );
    }

    #[test]
    fn interpret_any_factp_action() {
        // any-factp used as a condition inside an if form.
        let parsed = parse_sexprs(
            r"(defrule test => (if (any-factp ((?f data)) TRUE) then (printout t found)))",
            file(),
        );
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    }

    #[test]
    fn interpret_find_all_facts_action() {
        let parsed = parse_sexprs(
            r"(defrule test => (bind ?result (find-all-facts ((?f data)) TRUE)))",
            file(),
        );
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    }

    #[test]
    fn interpret_find_fact_action() {
        let parsed = parse_sexprs(
            r"(defrule test => (bind ?r (find-fact ((?f person)) (eq (fact-slot-value ?f name) Alice))))",
            file(),
        );
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    }

    #[test]
    fn interpret_query_action_multi_binding() {
        // Two binding variables in a single query macro.
        let parsed = parse_sexprs(
            r"(defrule test => (do-for-all-facts ((?a person) (?b address)) TRUE (printout t ?a ?b)))",
            file(),
        );
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let Construct::Rule(rule) = &result.constructs[0] else {
            panic!("expected Rule");
        };
        assert!(
            matches!(&rule.actions[0].call.args[0], ActionExpr::QueryAction { bindings, .. }
                if bindings.len() == 2)
        );
    }

    #[test]
    fn interpret_query_action_missing_binding_list_is_error() {
        let parsed = parse_sexprs("(defrule test => (do-for-fact TRUE))", file());
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(
            !result.errors.is_empty(),
            "should error: missing binding list"
        );
    }

    #[test]
    fn interpret_query_action_bad_binding_var_is_error() {
        // Using a plain symbol instead of ?var in a binding is an error.
        let parsed = parse_sexprs("(defrule test => (do-for-fact ((f data)) TRUE))", file());
        let config = InterpreterConfig::default();
        let result = interpret_constructs(&parsed.exprs, &config);
        assert!(
            !result.errors.is_empty(),
            "should error: bad binding variable"
        );
    }
}
