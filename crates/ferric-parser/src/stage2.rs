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

#[derive(Clone, Debug)]
pub enum ActionExpr {
    Literal(LiteralValue),
    Variable(String, Span),
    GlobalVariable(String, Span),
    FunctionCall(FunctionCall),
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
            "defrule" => match interpret_rule(&list[1..], sexpr.span()) {
                Ok(construct) => result.constructs.push(Construct::Rule(construct)),
                Err(err) => {
                    result.errors.push(err);
                    if config.strict {
                        return result;
                    }
                }
            },
            "deftemplate" => match interpret_template(&list[1..], sexpr.span()) {
                Ok(construct) => result.constructs.push(Construct::Template(construct)),
                Err(err) => {
                    result.errors.push(err);
                    if config.strict {
                        return result;
                    }
                }
            },
            "deffacts" => match interpret_facts(&list[1..], sexpr.span()) {
                Ok(construct) => result.constructs.push(Construct::Facts(construct)),
                Err(err) => {
                    result.errors.push(err);
                    if config.strict {
                        return result;
                    }
                }
            },
            // Known CLIPS keywords that are not yet supported
            "deffunction" | "defglobal" | "defmodule" | "defclass" | "definstances"
            | "defmessage-handler" | "defgeneric" | "defmethod" => {
                result.errors.push(InterpretError {
                    message: format!("{keyword} is not yet supported"),
                    span: list[0].span(),
                    kind: InterpretErrorKind::UnknownConstruct,
                    suggestions: vec![
                        "currently supported: defrule, deftemplate, deffacts".to_string()
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
    let mut comment = None;

    // Check for optional comment (string as second element)
    if idx < elements.len() {
        if let Some(Atom::String(s)) = elements[idx].as_atom() {
            comment = Some(s.clone());
            idx += 1;
        }
    }

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
    let mut comment = None;

    // Check for optional comment (string as second element)
    if idx < elements.len() {
        if let Some(Atom::String(s)) = elements[idx].as_atom() {
            comment = Some(s.clone());
            idx += 1;
        }
    }

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

    // Check for special conditional elements
    let keyword = list[0].as_symbol();
    match keyword {
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
            return Ok(Pattern::And(patterns, expr.span()));
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
            return Ok(Pattern::Not(Box::new(inner_pattern), expr.span()));
        }
        Some("test") => {
            if list.len() < 2 {
                return Err(InterpretError::missing(
                    "expression after 'test'",
                    expr.span(),
                ));
            }
            // Store the test expression as raw S-expr (full compilation in Phase 3)
            return Ok(Pattern::Test(list[1].clone(), expr.span()));
        }
        Some("exists") => {
            let mut patterns = Vec::new();
            for pattern_expr in &list[1..] {
                patterns.push(interpret_pattern(pattern_expr)?);
            }
            return Ok(Pattern::Exists(patterns, expr.span()));
        }
        _ => {}
    }

    // Check if this is a regular pattern (ordered or template)
    // Template patterns have slot-value pairs like: (template (slot-name value) ...)
    // Ordered patterns have fields like: (relation value1 value2 ...)

    let relation = keyword
        .ok_or_else(|| InterpretError::expected("pattern name (symbol)", list[0].span()))?
        .to_string();

    // Determine if this is a template pattern by checking if sub-elements are (name value) lists
    let is_template = list[1..].iter().all(|elem| {
        if let Some(sub_list) = elem.as_list() {
            !sub_list.is_empty() && sub_list[0].as_symbol().is_some()
        } else {
            false
        }
    });

    if is_template && !list[1..].is_empty() {
        // Template pattern
        let mut slot_constraints = Vec::new();
        for slot_expr in &list[1..] {
            let slot_list = slot_expr.as_list().ok_or_else(|| {
                InterpretError::expected("slot constraint (list)", slot_expr.span())
            })?;

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

            // For Phase 2, support single constraint per slot
            let constraint = if slot_list.len() > 1 {
                interpret_constraint(&slot_list[1])?
            } else {
                // Empty slot means wildcard
                Constraint::Wildcard(slot_expr.span())
            };

            slot_constraints.push(SlotConstraint {
                slot_name,
                constraint,
                span: slot_expr.span(),
            });
        }

        Ok(Pattern::Template(TemplatePattern {
            template: relation,
            slot_constraints,
            span: expr.span(),
        }))
    } else {
        // Ordered pattern
        let mut constraints = Vec::new();
        for field_expr in &list[1..] {
            constraints.push(interpret_constraint(field_expr)?);
        }

        Ok(Pattern::Ordered(OrderedPattern {
            relation,
            constraints,
            span: expr.span(),
        }))
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
    let call = interpret_function_call(expr)?;
    Ok(Action { call })
}

/// Interpret a function call expression.
fn interpret_function_call(expr: &SExpr) -> Result<FunctionCall, InterpretError> {
    let list = expr
        .as_list()
        .ok_or_else(|| InterpretError::expected("function call (list)", expr.span()))?;

    if list.is_empty() {
        return Err(InterpretError::invalid("empty function call", expr.span()));
    }

    let name = list[0]
        .as_symbol()
        .ok_or_else(|| InterpretError::expected("function name (symbol)", list[0].span()))?
        .to_string();

    let mut args = Vec::new();
    for arg_expr in &list[1..] {
        args.push(interpret_action_expr(arg_expr)?);
    }

    Ok(FunctionCall {
        name,
        args,
        span: expr.span(),
    })
}

/// Interpret an expression in an action context (RHS).
fn interpret_action_expr(expr: &SExpr) -> Result<ActionExpr, InterpretError> {
    // Check if it's a list (nested function call)
    if let Some(_list) = expr.as_list() {
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
        InterpretError::expected("slot keyword (slot or multislot)", list[0].span())
    })?;

    let (slot_type, name_idx) = match keyword {
        "slot" => (SlotType::Single, 1),
        "multislot" => (SlotType::Multi, 1),
        _ => {
            return Err(InterpretError::invalid(
                "expected 'slot' or 'multislot'",
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
        if name.to_uppercase() == "NONE" {
            return Ok(DefaultValue::None);
        } else if name.to_uppercase() == "DERIVE" {
            return Ok(DefaultValue::Derive);
        }
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

    // Determine if this is a template fact by checking if sub-elements are (name value) lists
    let is_template = list[1..].iter().all(|elem| {
        if let Some(sub_list) = elem.as_list() {
            !sub_list.is_empty() && sub_list[0].as_symbol().is_some()
        } else {
            false
        }
    });

    if is_template && !list[1..].is_empty() {
        // Template fact
        let mut slot_values = Vec::new();
        for slot_expr in &list[1..] {
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
                return Err(InterpretError::missing(
                    "value for slot in fact",
                    slot_expr.span(),
                ));
            }

            let value = interpret_fact_value(&slot_list[1])?;

            slot_values.push(FactSlotValue {
                name: slot_name,
                value,
                span: slot_expr.span(),
            });
        }

        Ok(FactBody::Template(TemplateFactBody {
            template: name,
            slot_values,
            span: expr.span(),
        }))
    } else {
        // Ordered fact
        let mut values = Vec::new();
        for value_expr in &list[1..] {
            values.push(interpret_fact_value(value_expr)?);
        }

        Ok(FactBody::Ordered(OrderedFactBody {
            relation: name,
            values,
            span: expr.span(),
        }))
    }
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
    fn interpret_error_invalid_slot_keyword() {
        let parsed = parse_sexprs("(deftemplate person (field name))", file());
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
}
