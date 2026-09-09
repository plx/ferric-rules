//! Source code loader for CLIPS-compatible syntax.
//!
//! This module provides functionality to load CLIPS source code from strings
//! or files and convert it into engine-level constructs.
//!
//! ## Phase 2 state
//!
//! - Full Stage 2 interpretation for `defrule`, `deftemplate`, `deffacts`.
//! - Rule compilation from Stage 2 AST into rete network.
//! - Pattern validation (nesting depth, unsupported combinations).
//! - `(assert ...)` top-level forms for loading facts into working memory.
//!
//! ## Phase 3 scope
//!
//! - Add support for `deffunction`, `defglobal`, `defmodule`, `defgeneric`,
//!   `defmethod` top-level forms.
//! - `test` CE compilation (currently returns compile error).
//! - Template pattern compilation (currently returns compile error).

use ferric_rules_core::RuleId;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;

// Qualified name utilities: wired into construct loading in passes 003/004.
#[allow(unused_imports)]
use crate::qualified_name::{parse_qualified_name, QualifiedName};

use ferric_rules_core::{
    AlphaEntryType, AtomKey, CompilableCondition, CompilablePattern, CompileResult,
    ConditionCompilationPlan, ConstantTest, ConstantTestType, Fact, FactId, FerricString,
    JoinTestType, Salience, SequenceField, SequencePattern, SequenceSegment, SequenceSource,
    SlotIndex, TemplateFact, Value,
};
use ferric_rules_parser::{
    interpret_constructs, parse_sexprs, ActionExpr, Atom, Constraint, Construct, FactBody,
    FactValue, FileId, FunctionCall, FunctionConstruct, GenericConstruct, GlobalConstruct,
    InterpretError, InterpreterConfig, LiteralKind, MethodConstruct, ModuleConstruct,
    OrderedFactBody, OrderedPattern, ParseError, Pattern, RuleConstruct, SExpr, SlotConstraint,
    SlotType, Span, TemplateConstruct, TemplateFactBody, TemplatePattern,
};

use crate::actions::{CompiledRuleInfo, CompiledTestCondition};
use crate::engine::{Engine, EngineError};
use crate::functions::{get_or_insert_module_entry_with, insert_module_entry, UserFunction};
use crate::templates::RegisteredTemplate;
use crate::tracing_support::{ferric_event, ferric_span};
// GenericRegistry accessed via self.generics (field on Engine)

mod lhs_scope;
mod runtime_constraints;

/// Derived name index, rebuilt from definitions when restoring a snapshot.
pub(crate) type TemplateLocalIndex =
    rustc_hash::FxHashMap<Box<str>, smallvec::SmallVec<[ferric_rules_core::TemplateId; 2]>>;

#[derive(Debug)]
pub(crate) enum TemplateLookupError {
    Unknown,
    NotVisible,
    Ambiguous(smallvec::SmallVec<[crate::modules::ModuleId; 2]>),
}

/// Translated rule data including fact-address variable bindings.
struct TranslatedRule {
    salience: Salience,
    conditions: Vec<CompilableCondition>,
    fact_address_vars: HashMap<String, usize>,
    /// Test conditions referenced by match-time predicate nodes.
    test_conditions: Vec<CompiledTestCondition>,
}

struct PreparedRuleInstallation {
    plan: ConditionCompilationPlan,
    info: Arc<CompiledRuleInfo>,
    module: crate::modules::ModuleId,
}

struct RuleRhsScope<'a> {
    exported: &'a HashSet<String>,
    existential: &'a HashSet<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SimpleComparisonOp {
    Eq,
    Ne,
    Gt,
    Lt,
    Ge,
    Le,
}

impl SimpleComparisonOp {
    fn invert(self) -> Self {
        match self {
            Self::Eq => Self::Eq,
            Self::Ne => Self::Ne,
            Self::Gt => Self::Lt,
            Self::Lt => Self::Gt,
            Self::Ge => Self::Le,
            Self::Le => Self::Ge,
        }
    }

    fn to_join_test(self) -> JoinTestType {
        match self {
            Self::Eq => JoinTestType::Equal,
            Self::Ne => JoinTestType::NotEqual,
            Self::Gt => JoinTestType::GreaterThan,
            Self::Lt => JoinTestType::LessThan,
            Self::Ge => JoinTestType::GreaterOrEqual,
            Self::Le => JoinTestType::LessOrEqual,
        }
    }

    fn to_lex_join_test(self) -> JoinTestType {
        match self {
            Self::Eq => JoinTestType::LexEqual,
            Self::Ne => JoinTestType::LexNotEqual,
            Self::Gt => JoinTestType::LexGreaterThan,
            Self::Lt => JoinTestType::LexLessThan,
            Self::Ge => JoinTestType::LexGreaterOrEqual,
            Self::Le => JoinTestType::LexLessOrEqual,
        }
    }

    fn to_join_test_with_offset(self, offset: i64) -> JoinTestType {
        match self {
            Self::Eq => JoinTestType::EqualOffset(offset),
            Self::Ne => JoinTestType::NotEqualOffset(offset),
            Self::Gt => JoinTestType::GreaterThanOffset(offset),
            Self::Lt => JoinTestType::LessThanOffset(offset),
            Self::Ge => JoinTestType::GreaterOrEqualOffset(offset),
            Self::Le => JoinTestType::LessOrEqualOffset(offset),
        }
    }

    fn to_constant_test(self, key: AtomKey) -> ConstantTestType {
        match self {
            Self::Eq => ConstantTestType::Equal(key),
            Self::Ne => ConstantTestType::NotEqual(key),
            Self::Gt => ConstantTestType::GreaterThan(key),
            Self::Lt => ConstantTestType::LessThan(key),
            Self::Ge => ConstantTestType::GreaterOrEqual(key),
            Self::Le => ConstantTestType::LessOrEqual(key),
        }
    }

    fn to_slot_offset_test(self, other_slot: SlotIndex, offset: i64) -> ConstantTestType {
        match self {
            Self::Eq => ConstantTestType::EqualSlotOffset(other_slot, offset),
            Self::Ne => ConstantTestType::NotEqualSlotOffset(other_slot, offset),
            Self::Gt => ConstantTestType::GreaterThanSlotOffset(other_slot, offset),
            Self::Lt => ConstantTestType::LessThanSlotOffset(other_slot, offset),
            Self::Ge => ConstantTestType::GreaterOrEqualSlotOffset(other_slot, offset),
            Self::Le => ConstantTestType::LessOrEqualSlotOffset(other_slot, offset),
        }
    }
}

#[derive(Clone, Debug)]
enum PredicateOperand {
    Variable(String),
    VariableWithOffset { name: String, offset: i64 },
    Literal(LiteralKind),
}

#[derive(Clone, Debug)]
struct LinearIntegerExpr {
    variable: Option<String>,
    coefficient: i64,
    offset: i64,
}

impl LinearIntegerExpr {
    fn integer(value: i64) -> Self {
        Self {
            variable: None,
            coefficient: 0,
            offset: value,
        }
    }

    fn variable(name: String) -> Self {
        Self {
            variable: Some(name),
            coefficient: 1,
            offset: 0,
        }
    }

    fn add(&self, other: &Self) -> Option<Self> {
        let variable =
            Self::merge_variables(self.variable.as_deref(), other.variable.as_deref()).ok()?;
        let coefficient = self.coefficient.checked_add(other.coefficient)?;
        let offset = self.offset.checked_add(other.offset)?;
        Some(Self::new(variable, coefficient, offset))
    }

    fn sub(&self, other: &Self) -> Option<Self> {
        let variable =
            Self::merge_variables(self.variable.as_deref(), other.variable.as_deref()).ok()?;
        let coefficient = self.coefficient.checked_sub(other.coefficient)?;
        let offset = self.offset.checked_sub(other.offset)?;
        Some(Self::new(variable, coefficient, offset))
    }

    fn negate(self) -> Option<Self> {
        let coefficient = self.coefficient.checked_neg()?;
        let offset = self.offset.checked_neg()?;
        Some(Self::new(self.variable, coefficient, offset))
    }

    fn merge_variables(lhs: Option<&str>, rhs: Option<&str>) -> Result<Option<String>, ()> {
        match (lhs, rhs) {
            (Some(a), Some(b)) if a != b => Err(()),
            (Some(a), _) => Ok(Some(a.to_owned())),
            (_, Some(b)) => Ok(Some(b.to_owned())),
            (None, None) => Ok(None),
        }
    }

    fn new(variable: Option<String>, coefficient: i64, offset: i64) -> Self {
        if coefficient == 0 {
            Self {
                variable: None,
                coefficient,
                offset,
            }
        } else {
            Self {
                variable,
                coefficient,
                offset,
            }
        }
    }
}

/// Errors that can occur during source loading.
#[derive(Debug, Error)]
pub enum LoadError {
    #[error("parse error: {0}")]
    Parse(ParseError),

    #[error("interpret error: {0}")]
    Interpret(InterpretError),

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

    #[error("rule `{rule}` at line {line}, column {column}: {resource} requires at least {required}, exceeding the supported limit of {limit}")]
    ResourceLimit {
        rule: String,
        resource: &'static str,
        required: usize,
        limit: usize,
        line: u32,
        column: u32,
    },

    #[error("pattern validation failed")]
    Validation(Vec<ferric_rules_core::PatternValidationError>),

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
    pub asserted_facts: Vec<crate::FactHandle>,
    /// Rules registered during loading (typed constructs from Stage 2).
    pub rules: Vec<RuleConstruct>,
    /// Templates registered during loading.
    pub templates: Vec<TemplateConstruct>,
    /// Functions parsed during loading (not yet executable; Pass 006 adds execution).
    pub functions: Vec<FunctionConstruct>,
    /// Globals parsed during loading (not yet active; Pass 006 adds execution).
    pub globals: Vec<GlobalConstruct>,
    /// Modules parsed during loading.
    pub modules: Vec<ModuleConstruct>,
    /// Generic function declarations parsed during loading.
    pub generics: Vec<GenericConstruct>,
    /// Method definitions parsed during loading.
    pub methods: Vec<MethodConstruct>,
    /// Warnings/diagnostics (non-fatal).
    pub warnings: Vec<String>,
}

#[path = "loader/queued_input_validation.rs"]
mod queued_input_validation;

impl Engine {
    /// Load CLIPS source code from a string.
    ///
    /// Parses and processes top-level forms:
    /// - `(assert ...)` — assert facts into working memory
    /// - `(defrule ...)` — register rule definitions
    /// - `(deffacts ...)` — register named seed facts, asserted only by `reset`
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
    /// use ferric_rules_runtime::{Engine, EngineConfig};
    ///
    /// let mut engine = Engine::new(EngineConfig::utf8());
    /// let result = engine.load_str("(assert (person John 30))").unwrap();
    /// assert_eq!(result.asserted_facts.len(), 1);
    /// ```
    pub fn load_str(&mut self, source: &str) -> Result<LoadResult, Vec<LoadError>> {
        let diagnostics_start = self.action_diagnostics.len();
        let mut result = self.load_str_inner(source);
        // Scanner notices and other evaluator output must be observable when
        // a load boundary returns, before a later action clears stale events.
        for (channel, bytes) in self.globals.take_printout_events() {
            self.router.write(&channel, &bytes);
        }
        self.drain_evaluator_diagnostics();
        self.globals.take_evaluation_halt();
        self.globals.take_sort_return();
        if let Ok(loaded) = &mut result {
            loaded.warnings.extend(
                self.action_diagnostics[diagnostics_start..]
                    .iter()
                    .map(ToString::to_string),
            );
        }
        result
    }

    #[allow(clippy::too_many_lines)] // Sequential pipeline steps; each section is clearly delineated
    fn load_str_inner(&mut self, source: &str) -> Result<LoadResult, Vec<LoadError>> {
        ferric_span!(info_span, "engine_load_str", len = source.len());
        crate::source_limits::check_source_size(source.len()).map_err(|e| vec![e])?;

        // Parse the source into S-expressions (Stage 1)
        let parse_result = {
            ferric_span!(debug_span, "load_parse");
            parse_sexprs(source, FileId(0))
        };
        ferric_event!(
            debug,
            top_level_forms = parse_result.exprs.len(),
            parse_error_count = parse_result.errors.len(),
            "load_parse_complete"
        );

        // Convert parse errors to LoadError
        if !parse_result.errors.is_empty() {
            let errors = parse_result
                .errors
                .into_iter()
                .map(LoadError::Parse)
                .collect();
            ferric_event!(warn, "engine_load_str_failed_parse");
            return Err(errors);
        }

        let mut result = LoadResult::default();
        let mut errors = Vec::new();

        // Separate assert forms from constructs
        // Assert forms are processed directly for Phase 1 compatibility
        let mut assert_forms = Vec::new();
        let mut construct_forms = Vec::new();

        for expr in parse_result.exprs {
            if let Some(list) = expr.as_list() {
                if !list.is_empty() && list[0].as_symbol() == Some("assert") {
                    assert_forms.push(expr);
                } else if !list.is_empty()
                    && matches!(
                        list[0].as_symbol(),
                        Some(
                            "defrule"
                                | "deftemplate"
                                | "deffacts"
                                | "deffunction"
                                | "defglobal"
                                | "defmodule"
                                | "defgeneric"
                                | "defmethod"
                        )
                    )
                {
                    construct_forms.push(expr);
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
        ferric_event!(
            debug,
            assert_forms = assert_forms.len(),
            construct_forms = construct_forms.len(),
            unsupported_forms = errors.len(),
            warning_count = result.warnings.len(),
            "load_forms_partitioned"
        );

        // Interpret constructs via Stage 2
        if !construct_forms.is_empty() {
            let config = InterpreterConfig::default();
            let interpret_result = {
                ferric_span!(debug_span, "load_interpret");
                interpret_constructs(&construct_forms, &config)
            };

            // Convert interpret errors to LoadError
            if !interpret_result.errors.is_empty() {
                for e in interpret_result.errors {
                    errors.push(LoadError::Interpret(e));
                }
            }

            // Collect constructs by type; deffacts only register reset seeds.
            //
            // Rules are collected with their owning module captured at the
            // time they appear in source so that defmodule statements
            // interleaved with defrule statements are respected.
            let mut deffacts_constructs: Vec<(
                ferric_rules_parser::FactsConstruct,
                crate::modules::ModuleId,
            )> = Vec::new();
            let mut rules_with_module = Vec::new();
            let mut pending_ordered_fact_names = HashSet::new();
            for construct in interpret_result.constructs {
                match construct {
                    Construct::Rule(rule) => {
                        // Determine the owning module for this rule. If the rule
                        // name is module-qualified (e.g. `MAIN::start`), the
                        // declared module takes precedence over the current module
                        // so that rules like `(defrule MAIN::foo ...)` appearing
                        // inside a `(defmodule REPORT ...)` section still belong
                        // to MAIN for focus-aware dispatch.
                        let owning_module = if let Ok(qn) = parse_qualified_name(&rule.name) {
                            if let Some(mod_name) = qn.module_name() {
                                self.module_registry
                                    .get_by_name(mod_name)
                                    .unwrap_or_else(|| self.module_registry.current_module())
                            } else {
                                self.module_registry.current_module()
                            }
                        } else {
                            self.module_registry.current_module()
                        };
                        rules_with_module.push((rule, owning_module));
                    }
                    Construct::Template(template) => {
                        // Register template BEFORE compiling rules so that
                        // rules referencing this template can resolve the ID.
                        let name = Self::template_local_name(&template.name);
                        let pending_ordered_use = rules_with_module.iter().any(|(rule, module)| {
                            self.rule_uses_ordered_name(rule, *module, &name)
                        }) || pending_ordered_fact_names.contains(&name)
                            || assert_forms.iter().any(|expr| {
                                expr.span().start.offset < template.span.start.offset
                                    && expr.as_list().is_some_and(|form| {
                                        form.iter().skip(1).any(|fact| {
                                            fact.as_list()
                                                .and_then(|fields| fields.first())
                                                .and_then(SExpr::as_symbol)
                                                .is_some_and(|raw| {
                                                    Self::ordered_relation_name_is(raw, &name)
                                                })
                                        })
                                    })
                            });
                        let pending_use = self
                            .template_definition_identity(&template)
                            .ok()
                            .and_then(|(_, id)| id)
                            .is_some_and(|id| {
                                rules_with_module.iter().any(|(rule, module)| {
                                    self.rule_uses_template(rule, *module, id)
                                }) || deffacts_constructs.iter().any(|(facts, module)| {
                                    facts.facts.iter().any(|fact| {
                                        let name = match fact {
                                            FactBody::Ordered(fact) => &fact.relation,
                                            FactBody::Template(fact) => &fact.template,
                                        };
                                        self.template_name_is(name, *module, id)
                                    })
                                })
                            });
                        if pending_ordered_use {
                            errors.push(Self::ordered_template_conflict(&template));
                        } else if pending_use {
                            errors.push(Self::template_in_use_error(&template));
                        } else if let Err(e) = self.register_template(&template, &mut result) {
                            errors.push(e);
                        } else {
                            result.templates.push(template);
                        }
                    }
                    Construct::Facts(facts) => {
                        let parsed = parse_qualified_name(&facts.name).map_err(LoadError::Compile);
                        let owning_module = match parsed {
                            Ok(name) => match name.module_name() {
                                Some(module) => {
                                    self.module_registry.get_by_name(module).ok_or_else(|| {
                                        LoadError::Compile(format!(
                                            "unknown module `{module}` for deffacts `{}`",
                                            facts.name
                                        ))
                                    })
                                }
                                None => Ok(self.module_registry.current_module()),
                            },
                            Err(error) => Err(error),
                        };
                        match owning_module {
                            Ok(module) => {
                                for fact in &facts.facts {
                                    if let FactBody::Ordered(fact) = fact {
                                        if self.resolve_template_id(&fact.relation, module).is_err()
                                        {
                                            pending_ordered_fact_names
                                                .insert(Self::template_local_name(&fact.relation));
                                        }
                                    }
                                }
                                deffacts_constructs.push((facts, module));
                            }
                            Err(error) => errors.push(error),
                        }
                    }
                    Construct::Function(func) => {
                        if let Err(error) = func
                            .body
                            .iter()
                            .try_for_each(crate::evaluator::validate_action_depth)
                        {
                            errors.push(Self::compile_error_at(&func.span, &error.to_string()));
                            continue;
                        }
                        let owning_module = self.module_registry.current_module();
                        if let Err(error) = func.body.iter().try_for_each(|expression| {
                            self.validate_queued_input_expression(expression, owning_module)
                        }) {
                            errors.push(error);
                            continue;
                        }
                        // Conflict check: a deffunction cannot share a name with
                        // an existing defgeneric (or vice versa).
                        if self.generics.contains(owning_module, &func.name) {
                            errors.push(Self::construct_conflict_error(
                                "deffunction",
                                "defgeneric",
                                &func.name,
                                &func.span,
                            ));
                            continue;
                        }
                        insert_module_entry(
                            &mut self.function_modules,
                            owning_module,
                            func.name.clone(),
                            owning_module,
                        );
                        // Register in the function environment for runtime use.
                        self.functions.register(
                            owning_module,
                            UserFunction {
                                name: func.name.clone(),
                                parameters: func.parameters.clone(),
                                wildcard_parameter: func.wildcard_parameter.clone(),
                                body: func.body.clone(),
                            },
                        );
                        result.functions.push(func);
                    }
                    Construct::Global(global) => {
                        // Evaluate initial values and store in the global store.
                        if let Err(e) = self.process_global_construct(&global) {
                            errors.push(e);
                        }
                        result.globals.push(global);
                    }
                    Construct::Module(module) => {
                        if let Err(message) = self.module_registry.validate_imports(&module.imports)
                        {
                            errors.push(Self::compile_error_at(&module.span, &message));
                            continue;
                        }
                        // Register the module (or update its exports/imports if it already
                        // exists). Re-defining a module (including MAIN) is allowed in CLIPS
                        // to set up imports and exports; only truly conflicting definitions
                        // (handled elsewhere) are rejected.
                        let module_id = self.module_registry.register(
                            &module.name,
                            module.exports.clone(),
                            module.imports.clone(),
                        );
                        self.module_registry.set_current_module(module_id);
                        result.modules.push(module);
                    }
                    Construct::Generic(generic) => {
                        let owning_module = self.module_registry.current_module();
                        if self.generics.contains(owning_module, &generic.name) {
                            errors.push(Self::duplicate_definition_error(
                                "defgeneric",
                                &generic.name,
                                &generic.span,
                            ));
                        } else if self.functions.contains(owning_module, &generic.name) {
                            // Conflict check: a defgeneric cannot share a name with
                            // an existing deffunction.
                            errors.push(Self::construct_conflict_error(
                                "defgeneric",
                                "deffunction",
                                &generic.name,
                                &generic.span,
                            ));
                        } else {
                            insert_module_entry(
                                &mut self.generic_modules,
                                owning_module,
                                generic.name.clone(),
                                owning_module,
                            );
                            // Register the generic function declaration.
                            self.generics.register_generic(owning_module, &generic.name);
                            result.generics.push(generic);
                        }
                    }
                    Construct::Method(method) => {
                        if let Err(error) = method
                            .body
                            .iter()
                            .try_for_each(crate::evaluator::validate_action_depth)
                        {
                            errors.push(Self::compile_error_at(&method.span, &error.to_string()));
                            continue;
                        }
                        let owning_module = self.module_registry.current_module();
                        if let Err(error) = method.body.iter().try_for_each(|expression| {
                            self.validate_queued_input_expression(expression, owning_module)
                        }) {
                            errors.push(error);
                            continue;
                        }
                        // Conflict check: a defmethod that would auto-create a
                        // generic cannot share a name with an existing deffunction.
                        if !self.generics.contains(owning_module, &method.name)
                            && self.functions.contains(owning_module, &method.name)
                        {
                            errors.push(Self::construct_conflict_error(
                                "defmethod",
                                "deffunction",
                                &method.name,
                                &method.span,
                            ));
                            continue;
                        }
                        if let Some(index) = method.index {
                            if self
                                .generics
                                .has_method_index(owning_module, &method.name, index)
                            {
                                errors.push(Self::duplicate_method_index_error(
                                    &method.name,
                                    index,
                                    &method.span,
                                ));
                                continue;
                            }
                        }
                        // Auto-create the generic module entry if it doesn't exist yet
                        // (a defmethod with no preceding defgeneric auto-creates the generic).
                        let _ = get_or_insert_module_entry_with(
                            &mut self.generic_modules,
                            owning_module,
                            &method.name,
                            || owning_module,
                        );
                        // Register the method in the generic registry.
                        // Extract parameter names and type restrictions from MethodParameter structs.
                        let param_names: Vec<String> =
                            method.parameters.iter().map(|p| p.name.clone()).collect();
                        let type_restrictions: Vec<Vec<String>> = method
                            .parameters
                            .iter()
                            .map(|p| p.type_restrictions.clone())
                            .collect();
                        self.generics.register_method(
                            owning_module,
                            &method.name,
                            method.index,
                            param_names,
                            type_restrictions,
                            method.wildcard_parameter.clone(),
                            method.body.clone(),
                        );
                        result.methods.push(method);
                    }
                }
            }

            // Compile rules so rete has patterns before facts arrive.
            // Templates are already registered at this point.
            // Restore each rule's owning module before compiling so that
            // cross-module template visibility checks use the correct module.
            let saved_module = self.module_registry.current_module();
            let mut expansion_budget = crate::source_limits::LoadBudget::default();
            for (rule, owning_module) in &rules_with_module {
                self.module_registry.set_current_module(*owning_module);
                match self.compile_rule_construct(rule, source, &mut expansion_budget) {
                    Ok(_) => {}
                    Err(e) => errors.push(e),
                }
                result.rules.push(rule.clone());
            }
            self.module_registry.set_current_module(saved_module);

            // Explicit initial-fact patterns use a protected built-in fact.
            // Empty/negative prefixes use the independent RETE root token.
            if let Err(e) = self.ensure_initial_fact() {
                errors.push(e);
            }

            // Register dormant definitions; reset will assert their facts.
            for (facts, owning_module) in &deffacts_constructs {
                self.module_registry.set_current_module(*owning_module);
                if let Err(e) = self.process_deffacts_construct(facts, &mut result) {
                    errors.push(e);
                }
            }
            self.module_registry.set_current_module(saved_module);
        }

        // Process assert forms AFTER rules are compiled so facts flow through rete
        for expr in &assert_forms {
            if let Some(list) = expr.as_list() {
                if let Err(e) = self.process_assert(&list[1..], &mut result) {
                    errors.push(e);
                }
            }
        }

        self.host.prune(&self.fact_base);
        if errors.is_empty() {
            ferric_event!(
                info,
                asserted_facts = result.asserted_facts.len(),
                rules = result.rules.len(),
                templates = result.templates.len(),
                functions = result.functions.len(),
                globals = result.globals.len(),
                modules = result.modules.len(),
                generics = result.generics.len(),
                methods = result.methods.len(),
                warning_count = result.warnings.len(),
                "engine_load_str_complete"
            );
            Ok(result)
        } else {
            ferric_event!(warn, error_count = errors.len(), "engine_load_str_failed");
            Err(errors)
        }
    }

    /// Ensure `(initial-fact)` is present in working memory.
    ///
    /// Explicit `(initial-fact)` patterns match this protected built-in fact.
    /// Empty/negative prefixes use the independent RETE root token. It is
    /// asserted once; subsequent calls are no-ops.
    ///
    /// The `FactId` is stored in `self.initial_fact_id` so that `facts()` can
    /// exclude it from user-visible results.
    fn ensure_initial_fact(&mut self) -> Result<(), LoadError> {
        // Already asserted in a previous load_str call.
        if self.initial_fact_id.is_some() {
            return Ok(());
        }

        let initial_sym = self
            .symbol_table
            .intern_symbol("initial-fact", self.config.string_encoding)
            .map_err(|e| LoadError::Compile(format!("initial-fact symbol: {e}")))?;

        let result = self.assert_fact_internal(Fact::Ordered(ferric_rules_core::OrderedFact {
            relation: initial_sym,
            fields: smallvec::SmallVec::new(),
        }))?;
        self.initial_fact_id = Some(result.fact_id());

        Ok(())
    }

    /// Load CLIPS source code from a file.
    ///
    /// Reads at most the supported source limit plus one byte, then delegates
    /// to `load_str`. Oversized files are rejected before full allocation.
    ///
    /// # Errors
    ///
    /// Returns errors if:
    /// - File cannot be read
    /// - Source parsing or processing fails
    pub fn load_file(&mut self, path: &Path) -> Result<LoadResult, Vec<LoadError>> {
        ferric_span!(info_span, "engine_load_file", path = %path.display());
        let source = crate::source_limits::read_source_file(path).map_err(|e| vec![e])?;
        self.load_str(&source)
    }

    /// Prepare all seed facts before replacing the named definition. Loading
    /// changes metadata only; reset is the sole consumer of these seed facts.
    fn process_deffacts_construct(
        &mut self,
        definition: &ferric_rules_parser::FactsConstruct,
        result: &mut LoadResult,
    ) -> Result<(), LoadError> {
        let name = parse_qualified_name(&definition.name).map_err(LoadError::Compile)?;
        let module = self.module_registry.current_module();
        let name = name.local_name().to_string();
        if module == self.module_registry.main_module_id() && name == "initial-fact" {
            return Err(LoadError::Compile(
                "the built-in initial-fact definition is protected".to_string(),
            ));
        }
        let checkpoint = self.symbol_table.checkpoint();
        let facts = match definition
            .facts
            .iter()
            .map(|body| self.build_fact_body(body, result))
            .collect::<Result<Vec<_>, _>>()
        {
            Ok(facts) => facts,
            Err(error) => {
                self.symbol_table.restore(checkpoint);
                return Err(error);
            }
        };
        self.registered_deffacts
            .retain(|entry| entry.module != module || entry.name != name);
        self.registered_deffacts
            .push(crate::engine::RegisteredDeffacts {
                module,
                name,
                facts,
            });
        Ok(())
    }

    fn build_fact_body(
        &mut self,
        body: &FactBody,
        result: &mut LoadResult,
    ) -> Result<Fact, LoadError> {
        match body {
            FactBody::Ordered(ordered) => self.build_ordered_fact_body(ordered, result),
            FactBody::Template(template) => self.build_template_fact_body(template, result),
        }
    }

    /// Reuse source fact validation without publishing a temporary definition.
    pub(crate) fn load_facts_str(&mut self, contents: &str) -> Result<usize, LoadError> {
        crate::source_limits::check_source_size(contents.len())?;
        let wrapped = format!("(deffacts __loaded_facts__ {contents})");
        let parsed = parse_sexprs(&wrapped, FileId(0));
        if let Some(error) = parsed.errors.into_iter().next() {
            return Err(LoadError::Parse(error));
        }
        let interpreted = interpret_constructs(&parsed.exprs, &InterpreterConfig::default());
        if let Some(error) = interpreted.errors.into_iter().next() {
            return Err(LoadError::Interpret(error));
        }
        let mut count = 0;
        let mut result = LoadResult::default();
        for construct in interpreted.constructs {
            if let Construct::Facts(definition) = construct {
                for body in definition.facts {
                    let fact = self.build_fact_body(&body, &mut result)?;
                    self.assert_fact_internal(fact)?;
                    count += 1;
                }
            }
        }
        Ok(count)
    }

    pub(crate) fn template_ref_parts(raw: &str) -> (Option<&str>, &str) {
        match raw.split_once("::") {
            Some((module, name))
                if !module.is_empty() && !name.is_empty() && !name.contains("::") =>
            {
                (Some(module), name)
            }
            // Keep the parser's malformed-name fallback used by ordered probes.
            _ => (None, raw),
        }
    }

    fn template_local_name(raw: &str) -> String {
        Self::template_ref_parts(raw).1.to_owned()
    }

    #[cfg(any(feature = "serde", debug_assertions, test))]
    pub(crate) fn build_template_local_index(
        definitions: &slotmap::SlotMap<ferric_rules_core::TemplateId, Arc<RegisteredTemplate>>,
    ) -> TemplateLocalIndex {
        let mut index = TemplateLocalIndex::default();
        for (id, definition) in definitions {
            index
                .entry(Self::template_ref_parts(&definition.name).1.into())
                .or_default()
                .push(id);
        }
        index
    }

    pub(crate) fn resolve_template_reference(
        &self,
        raw_name: &str,
        current_module: crate::modules::ModuleId,
    ) -> Result<ferric_rules_core::TemplateId, String> {
        self.resolve_template_id(raw_name, current_module).map_err(|error| {
            let current_module_label = self.module_registry.module_name(current_module).unwrap_or("?");
            match error {
                TemplateLookupError::Unknown => format!("unknown template `{raw_name}`"),
                TemplateLookupError::NotVisible => format!(
                    "template `{raw_name}` is not visible from module `{current_module_label}`"
                ),
                TemplateLookupError::Ambiguous(modules) => {
                    let modules: BTreeSet<_> = modules.iter().map(|module| {
                        self.module_registry.module_name(*module).unwrap_or("?")
                    }).collect();
                    format!(
                        "template `{raw_name}` is ambiguous from module `{current_module_label}` (matches modules: {})",
                        modules.into_iter().collect::<Vec<_>>().join(", ")
                    )
                }
            }
        })
    }

    /// Resolve without allocating diagnostics that ordered-relation probes discard.
    /// Only name candidates are indexed; visibility is always checked live.
    pub(crate) fn resolve_template_id(
        &self,
        raw_name: &str,
        current_module: crate::modules::ModuleId,
    ) -> Result<ferric_rules_core::TemplateId, TemplateLookupError> {
        let (qualified_module_name, wanted_local_name) = Self::template_ref_parts(raw_name);
        let candidates = self
            .template_local_ids
            .get(wanted_local_name)
            .ok_or(TemplateLookupError::Unknown)?;
        let module_for = |id| {
            self.template_modules
                .get(id)
                .copied()
                .unwrap_or_else(|| self.module_registry.main_module_id())
        };
        let choose = |ids: smallvec::SmallVec<[ferric_rules_core::TemplateId; 2]>| {
            if ids.len() == 1 {
                Ok(ids[0])
            } else {
                Err(TemplateLookupError::Ambiguous(
                    ids.into_iter().map(module_for).collect(),
                ))
            }
        };
        if let Some(module_name) = qualified_module_name {
            let target_module = self
                .module_registry
                .get_by_name(module_name)
                .ok_or(TemplateLookupError::Unknown)?;
            let matches: smallvec::SmallVec<_> = candidates
                .iter()
                .copied()
                .filter(|id| module_for(*id) == target_module)
                .collect();
            if matches.is_empty() {
                return Err(TemplateLookupError::Unknown);
            }
            let id = choose(matches)?;
            if !self.module_registry.is_construct_visible(
                current_module,
                target_module,
                "deftemplate",
                wanted_local_name,
            ) {
                return Err(TemplateLookupError::NotVisible);
            }
            return Ok(id);
        }
        let local: smallvec::SmallVec<_> = candidates
            .iter()
            .copied()
            .filter(|id| module_for(*id) == current_module)
            .collect();
        if !local.is_empty() {
            return choose(local);
        }
        let visible: smallvec::SmallVec<_> = candidates
            .iter()
            .copied()
            .filter(|id| {
                self.module_registry.is_construct_visible(
                    current_module,
                    module_for(*id),
                    "deftemplate",
                    wanted_local_name,
                )
            })
            .collect();
        if visible.is_empty() {
            return Err(TemplateLookupError::NotVisible);
        }
        choose(visible)
    }

    /// Process an ordered fact body.
    fn build_ordered_fact_body(
        &mut self,
        ordered: &OrderedFactBody,
        result: &mut LoadResult,
    ) -> Result<Fact, LoadError> {
        let current_module = self.module_registry.current_module();
        if let Ok(template_id) = self.resolve_template_id(&ordered.relation, current_module) {
            if !ordered.values.is_empty() {
                return Err(LoadError::Compile(format!(
                    "template `{}` requires named slot values",
                    ordered.relation
                )));
            }
            let registered = &self.template_defs[template_id];
            registered
                .validate_slots(&registered.defaults)
                .map_err(LoadError::Compile)?;
            let slots = registered.defaults.clone().into_boxed_slice();
            return Ok(Fact::Template(TemplateFact { template_id, slots }));
        }
        let mut fields = Vec::new();
        for fact_value in &ordered.values {
            let value = self
                .fact_value_to_value(fact_value, result)
                .ok_or_else(|| {
                    LoadError::Compile(format!(
                        "invalid value in deffacts relation `{}`",
                        ordered.relation
                    ))
                })?;
            match value {
                Value::Multifield(mf) => fields.extend(mf.as_slice().iter().cloned()),
                Value::Void => {}
                other => fields.push(other),
            }
        }

        Ok(Fact::Ordered(ferric_rules_core::OrderedFact {
            relation: self.compile_symbol(&ordered.relation)?,
            fields: fields.into(),
        }))
    }

    /// Process a template fact body.
    fn build_template_fact_body(
        &mut self,
        template: &TemplateFactBody,
        result: &mut LoadResult,
    ) -> Result<Fact, LoadError> {
        let current_module = self.module_registry.current_module();
        let template_id = match self.resolve_template_reference(&template.template, current_module)
        {
            Ok(template_id) => template_id,
            Err(msg) => {
                if Self::is_ambiguous_empty_template_fact(template) {
                    // Ambiguous parse shape: `(foo (clear))` can mean ordered fact
                    // with one field `clear` rather than template slot syntax.
                    // If no visible template exists, fall back to ordered-fact
                    // interpretation to match CLIPS behavior in drtest10-15.
                    let mut fields = Vec::with_capacity(template.slot_values.len());
                    for slot_val in &template.slot_values {
                        let sym = self
                            .symbol_table
                            .intern_symbol(&slot_val.name, self.config.string_encoding)
                            .map_err(|e| {
                                LoadError::Compile(format!(
                                    "deffacts ordered fallback symbol `{}`: {e}",
                                    slot_val.name
                                ))
                            })?;
                        fields.push(Value::Symbol(sym));
                    }
                    return Ok(Fact::Ordered(ferric_rules_core::OrderedFact {
                        relation: self.compile_symbol(&template.template)?,
                        fields: fields.into(),
                    }));
                }
                return Err(LoadError::Compile(format!("{msg} in deffacts")));
            }
        };

        let registered = self
            .template_defs
            .get(template_id)
            .cloned()
            .ok_or_else(|| {
                LoadError::Compile(format!(
                    "template `{}` not found in registry",
                    template.template
                ))
            })?;

        // Start with defaults.
        let mut slots: Vec<Value> = registered.defaults.clone();

        let mut seen = HashSet::new();
        for slot in &template.slot_values {
            let index = registered.slot_index(&slot.name).ok_or_else(|| {
                LoadError::Compile(format!(
                    "unknown slot `{}` in template `{}`",
                    slot.name, template.template
                ))
            })?;
            if !seen.insert(index) {
                return Err(LoadError::Compile(format!(
                    "duplicate slot `{}` in template `{}`",
                    slot.name, template.template
                )));
            }
            let mut fields = Vec::new();
            for field in &slot.values {
                let value = self.fact_value_to_value(field, result).ok_or_else(|| {
                    LoadError::Compile(format!(
                        "invalid value for slot `{}` in template `{}`",
                        slot.name, template.template
                    ))
                })?;
                match value {
                    Value::Multifield(values) => fields.extend(values.as_slice().iter().cloned()),
                    Value::Void => {}
                    value => fields.push(value),
                }
            }
            slots[index] = match registered.slot_types[index] {
                ferric_rules_parser::SlotType::Single if fields.len() == 1 => fields.pop().unwrap(),
                ferric_rules_parser::SlotType::Single => {
                    return Err(LoadError::Compile(format!(
                        "single-field slot `{}` in template `{}` requires exactly one value",
                        slot.name, template.template
                    )))
                }
                ferric_rules_parser::SlotType::Multi => {
                    Value::Multifield(Box::new(fields.into_iter().collect()))
                }
            };
        }
        registered
            .validate_slots(&slots)
            .map_err(LoadError::Compile)?;
        Ok(Fact::Template(TemplateFact {
            template_id,
            slots: slots.into_boxed_slice(),
        }))
    }

    fn is_ambiguous_empty_template_fact(template: &TemplateFactBody) -> bool {
        !template.slot_values.is_empty()
            && template
                .slot_values
                .iter()
                .all(|slot| slot.values.is_empty())
    }

    fn ordered_template_conflict(template: &TemplateConstruct) -> LoadError {
        Self::compile_error_at(&template.span, &format!(
            "cannot define template `{}` while its ordered relation is in use by facts or constructs", template.name
        ))
    }

    /// Find the owning module and any previous same-name definition.
    fn template_definition_identity(
        &self,
        template: &TemplateConstruct,
    ) -> Result<
        (
            crate::modules::ModuleId,
            Option<ferric_rules_core::TemplateId>,
        ),
        LoadError,
    > {
        let name = parse_qualified_name(&template.name)
            .map_err(|message| Self::compile_error_at(&template.span, &message))?;
        let module = if let Some(module) = name.module_name() {
            self.module_registry.get_by_name(module).ok_or_else(|| {
                Self::compile_error_at(
                    &template.span,
                    &format!("unknown module `{module}` for template `{}`", template.name),
                )
            })?
        } else {
            self.module_registry.current_module()
        };
        let existing = self.template_defs.iter().find_map(|(id, definition)| {
            (self.template_modules.get(id) == Some(&module)
                && Self::template_ref_parts(&definition.name).1 == name.local_name())
            .then_some(id)
        });
        Ok((module, existing))
    }

    fn template_in_use_error(template: &TemplateConstruct) -> LoadError {
        Self::compile_error_at(&template.span, &format!(
            "[CSTRCPSR4] cannot redefine template `{}` while it is in use by facts or constructs", template.name
        ))
    }

    /// Prepare one slot default without installing any template metadata.
    fn template_slot_default(
        &mut self,
        slot_def: &ferric_rules_parser::SlotDefinition,
        result: &mut LoadResult,
    ) -> Result<Value, LoadError> {
        let default_val = match &slot_def.default {
            Some(ferric_rules_parser::DefaultValue::None) => Value::Void,
            Some(ferric_rules_parser::DefaultValue::Value(literal)) => self
                .literal_to_value(&literal.value, literal.span.start.line, result)
                .ok_or_else(|| Self::compile_error_at(&literal.span, "invalid template default"))?,
            Some(ferric_rules_parser::DefaultValue::Values(literals)) => {
                let mut values = Vec::with_capacity(literals.len());
                for literal in literals {
                    values.push(
                        self.literal_to_value(&literal.value, literal.span.start.line, result)
                            .ok_or_else(|| {
                                Self::compile_error_at(&literal.span, "invalid template default")
                            })?,
                    );
                }
                Value::Multifield(Box::new(values.into_iter().collect()))
            }
            None | Some(ferric_rules_parser::DefaultValue::Derive) => {
                use ferric_rules_parser::SlotValueType;
                if slot_def.slot_type == ferric_rules_parser::SlotType::Multi {
                    Value::Multifield(Box::default())
                } else {
                    match slot_def.allowed_types.as_ref().and_then(|types| types.first()) {
                    None | Some(SlotValueType::Symbol) => Value::Symbol(self.compile_symbol("nil")?),
                    Some(SlotValueType::InstanceName) => Value::InstanceName(ferric_rules_core::InstanceName::from_symbol(self.compile_symbol("nil")?)),
                    Some(SlotValueType::String) => Value::String(self.compile_string("")?),
                    Some(SlotValueType::Integer) => Value::Integer(0),
                    Some(SlotValueType::Float) => Value::Float(0.0),
                    Some(SlotValueType::ExternalAddress) => return Err(Self::compile_error_at(&slot_def.span, "an external-address slot requires (default ?NONE); Ferric cannot derive a host-owned token")),
                }
                }
            }
        };
        let default_val = match (slot_def.slot_type, default_val) {
            (ferric_rules_parser::SlotType::Multi, Value::Void) => Value::Void,
            (ferric_rules_parser::SlotType::Multi, Value::Multifield(fields)) => {
                Value::Multifield(fields)
            }
            (ferric_rules_parser::SlotType::Multi, value) => {
                Value::Multifield(Box::new([value].into_iter().collect()))
            }
            (_, value) => value,
        };
        Ok(default_val)
    }

    /// Install a validated new template or replace an unused definition in place.
    fn register_template(
        &mut self,
        template: &TemplateConstruct,
        result: &mut LoadResult,
    ) -> Result<(), LoadError> {
        let (owning_module, existing) = self.template_definition_identity(template)?;
        if existing.is_some_and(|id| self.template_is_in_use(id)) {
            return Err(Self::template_in_use_error(template));
        }
        if existing.is_none() && self.template_ids.contains_key(template.name.as_str()) {
            return Err(Self::compile_error_at(
                &template.span,
                &format!(
                    "template spelling `{}` already belongs to another module; use a module-qualified declaration such as `MODULE::{}` for a distinct template",
                    template.name, Self::template_local_name(&template.name)
                ),
            ));
        }
        let local_name = Self::template_local_name(&template.name);
        if existing.is_none() && self.ordered_identity_is_live(&local_name) {
            return Err(Self::ordered_template_conflict(template));
        }
        let slot_count = template.slots.len();
        let mut slot_names = Vec::with_capacity(slot_count);
        let mut slot_index = HashMap::default();
        slot_index.reserve(slot_count);
        let mut defaults = Vec::with_capacity(slot_count);
        let mut slot_types = Vec::with_capacity(slot_count);
        let mut allowed_types = Vec::with_capacity(slot_count);

        for (i, slot_def) in template.slots.iter().enumerate() {
            slot_names.push(slot_def.name.clone());
            slot_index.insert(slot_def.name.clone(), i);
            slot_types.push(slot_def.slot_type);
            allowed_types.push(slot_def.allowed_types.clone());

            defaults.push(self.template_slot_default(slot_def, result)?);
        }

        let mut registered = RegisteredTemplate {
            name: template.name.clone(),
            slot_names,
            slot_types,
            allowed_types,
            slot_index,
            defaults,
        };
        for (index, value) in registered.defaults.iter().enumerate() {
            if !matches!(value, Value::Void) {
                registered.validate_slot(index, value).map_err(|message| {
                    Self::compile_error_at(&template.slots[index].span, &message)
                })?;
            }
        }
        let template_id = if let Some(id) = existing {
            // The old ID and public spelling remain stable. No fact or compiled
            // construct can observe the new slot layout, and repeated unused
            // definitions do not leave orphaned registry entries behind.
            registered.name.clone_from(&self.template_defs[id].name);
            self.template_defs[id] = Arc::new(registered);
            id
        } else {
            let id = self.template_defs.insert(Arc::new(registered));
            self.template_ids
                .insert(template.name.clone().into_boxed_str(), id);
            self.template_local_ids
                .entry(local_name.into_boxed_str())
                .or_default()
                .push(id);
            id
        };
        self.template_modules.insert(template_id, owning_module);

        Ok(())
    }

    /// Process a `GlobalConstruct`: evaluate each initial value expression and
    /// register it in both the active global store and the snapshot used for reset.
    fn process_global_construct(&mut self, global: &GlobalConstruct) -> Result<(), LoadError> {
        let current_module = self.module_registry.current_module();
        // Parse-time arity is checked across the whole declaration before any
        // initializer side effects or registered global values are published.
        for definition in &global.globals {
            self.validate_queued_input_expression(&definition.value, current_module)?;
        }
        let mut seen_in_construct: HashSet<&str> = HashSet::default();
        for def in &global.globals {
            if !seen_in_construct.insert(def.name.as_str())
                || self.globals.contains(current_module, &def.name)
            {
                return Err(Self::duplicate_definition_error(
                    "defglobal",
                    &def.name,
                    &def.span,
                ));
            }

            // Translate the init-value expression.  This must happen before we
            // construct the EvalContext because from_action_expr also needs
            // &mut symbol_table.
            let runtime_expr = crate::evaluator::from_action_expr(
                &def.value,
                &mut self.symbol_table,
                &self.config,
            )
            .map_err(|e| LoadError::Compile(format!("global `{}` init: {e}", def.name)))?;

            // Evaluate with empty bindings (globals are initialized at load time
            // without any rule context).  The block scope ensures the mutable
            // borrows on symbol_table and globals are released before the
            // subsequent self.globals.set() / self.registered_globals.push().
            let evaluation = {
                let empty_bindings = ferric_rules_core::binding::BindingSet::new();
                let empty_var_map = ferric_rules_core::binding::VarMap::new();
                let mut ctx = crate::evaluator::EvalContext {
                    bindings: &empty_bindings,
                    var_map: &empty_var_map,
                    symbol_table: &mut self.symbol_table,
                    config: &self.config,
                    functions: &self.functions,
                    globals: &mut self.globals,
                    generics: &self.generics,
                    call_depth: 0,
                    expression_depth: 0,
                    current_module: self.module_registry.current_module(),
                    module_registry: &self.module_registry,
                    function_modules: &self.function_modules,
                    global_modules: &self.global_modules,
                    generic_modules: &self.generic_modules,
                    method_chain: None,
                    input_buffer: None,
                    fact_base: None,
                    template_defs: None,
                };
                crate::evaluator::eval(&mut ctx, &runtime_expr)
                    .map_err(|e| LoadError::Compile(format!("global `{}` init: {e}", def.name)))
            };

            // A recovered expression value is usable by RHS consumers, but a
            // failed global initializer must not publish the new global.
            let evaluation_halted = self.globals.take_evaluation_halt();
            self.globals.take_sort_return();
            let value = evaluation?;
            if evaluation_halted {
                return Err(LoadError::Compile(format!(
                    "global `{}` init: evaluation halted",
                    def.name
                )));
            }

            // CLIPS commits named globals incrementally, even within one
            // defglobal group. Publish ownership only after this initializer
            // succeeds, so failed/later names cannot leave phantom metadata.
            insert_module_entry(
                &mut self.global_modules,
                current_module,
                def.name.clone(),
                current_module,
            );
            self.globals.set(current_module, &def.name, value.clone());
            self.registered_globals
                .push((current_module, def.name.clone(), value));
        }
        Ok(())
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
            FactValue::GlobalVariable(name, span) => {
                let runtime_expr = match crate::evaluator::from_action_expr(
                    &ActionExpr::GlobalVariable(name.clone(), *span),
                    &mut self.symbol_table,
                    &self.config,
                ) {
                    Ok(expr) => expr,
                    Err(error) => {
                        Self::warn_with_detail(
                            result,
                            span.start.line,
                            "global variable in deffacts could not be translated, skipping",
                            &error,
                        );
                        return None;
                    }
                };

                let value = {
                    let empty_bindings = ferric_rules_core::binding::BindingSet::new();
                    let empty_var_map = ferric_rules_core::binding::VarMap::new();
                    let mut ctx = crate::evaluator::EvalContext {
                        bindings: &empty_bindings,
                        var_map: &empty_var_map,
                        symbol_table: &mut self.symbol_table,
                        config: &self.config,
                        functions: &self.functions,
                        globals: &mut self.globals,
                        generics: &self.generics,
                        call_depth: 0,
                        expression_depth: 0,
                        current_module: self.module_registry.current_module(),
                        module_registry: &self.module_registry,
                        function_modules: &self.function_modules,
                        global_modules: &self.global_modules,
                        generic_modules: &self.generic_modules,
                        method_chain: None,
                        input_buffer: None,
                        fact_base: None,
                        template_defs: None,
                    };
                    crate::evaluator::eval(&mut ctx, &runtime_expr)
                };

                match value {
                    Ok(value) => Some(value),
                    Err(error) => {
                        Self::warn_with_detail(
                            result,
                            span.start.line,
                            "global variable in deffacts could not be resolved, skipping",
                            &error,
                        );
                        None
                    }
                }
            }
            FactValue::EmptyMultifield(_) => {
                // Empty multislot: `(slot-name)` → empty multifield value.
                // Represented as Void (the default for unset multislots).
                Some(Value::Void)
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
            LiteralKind::InstanceName(s) => {
                self.warned_symbol_value(s, line, result).map(|v| match v {
                    Value::Symbol(symbol) => {
                        Value::InstanceName(ferric_rules_core::InstanceName::from_symbol(symbol))
                    }
                    _ => unreachable!(),
                })
            }
        }
    }

    /// Process an `(assert ...)` form.
    ///
    /// Each sub-list after `assert` is treated as a fact to assert.
    fn process_assert(&mut self, args: &[SExpr], result: &mut LoadResult) -> Result<(), LoadError> {
        for fact_expr in args {
            let fact_id = self.process_assert_fact(fact_expr, result)?;
            result.asserted_facts.push(self.host.export(fact_id));
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

        // Check if this is a known template — if so, parse slot syntax.
        let current_module = self.module_registry.current_module();
        if let Ok(template_id) = self.resolve_template_id(relation, current_module) {
            return self.process_assert_template_fact(
                template_id,
                relation,
                &fact_list[1..],
                result,
            );
        }

        // Ordered fact: remaining elements are field values.
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

        let relation = self
            .symbol_table
            .intern_symbol(relation, self.config.string_encoding)
            .map_err(|error| LoadError::Engine(error.into()))?;
        Ok(self
            .assert_fact_internal(Fact::Ordered(ferric_rules_core::OrderedFact {
                relation,
                fields: fields.into_iter().collect(),
            }))?
            .fact_id())
    }

    /// Process a template fact within an assert form.
    ///
    /// Each remaining element supplies the complete value sequence of one slot.
    fn process_assert_template_fact(
        &mut self,
        template_id: ferric_rules_core::TemplateId,
        template_name: &str,
        slot_exprs: &[SExpr],
        result: &mut LoadResult,
    ) -> Result<FactId, LoadError> {
        let registered = self
            .template_defs
            .get(template_id)
            .cloned()
            .ok_or_else(|| {
                LoadError::Compile(format!("template `{template_name}` not found in registry"))
            })?;

        // Start with defaults.
        let mut slots: Vec<Value> = registered.defaults.clone();
        let mut seen = HashSet::new();

        for slot_expr in slot_exprs {
            let slot_list = slot_expr.as_list().ok_or_else(|| {
                LoadError::InvalidAssert(format!(
                    "expected slot list in template fact `{template_name}`"
                ))
            })?;
            if slot_list.is_empty() {
                return Err(LoadError::InvalidAssert(
                    "empty template slot list".to_string(),
                ));
            }
            let slot_name = slot_list[0].as_symbol().ok_or_else(|| {
                LoadError::InvalidAssert(format!(
                    "expected slot name symbol in template `{template_name}`"
                ))
            })?;
            let slot_idx = registered.slot_index(slot_name).ok_or_else(|| {
                LoadError::Compile(format!(
                    "unknown slot `{slot_name}` in template `{template_name}`"
                ))
            })?;
            if !seen.insert(slot_idx) {
                return Err(LoadError::InvalidAssert(format!(
                    "duplicate slot `{slot_name}` in template `{template_name}`"
                )));
            }

            let mut fields = slot_list[1..]
                .iter()
                .map(|expression| {
                    self.atom_to_value(expression, result).ok_or_else(|| {
                        LoadError::InvalidAssert(format!(
                            "expected literal value for slot `{slot_name}` in template `{template_name}`"
                        ))
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            slots[slot_idx] = match registered.slot_types[slot_idx] {
                SlotType::Single if fields.len() == 1 => fields.pop().unwrap(),
                SlotType::Single => {
                    return Err(LoadError::InvalidAssert(format!(
                        "single-field slot `{slot_name}` requires exactly one value"
                    )));
                }
                SlotType::Multi => Value::Multifield(Box::new(fields.into_iter().collect())),
            };
        }

        registered
            .validate_slots(&slots)
            .map_err(LoadError::Compile)?;

        // Assert as a proper template fact.
        Ok(self
            .assert_fact_internal(Fact::Template(TemplateFact {
                template_id,
                slots: slots.into_boxed_slice(),
            }))?
            .fact_id())
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
            Atom::InstanceName(s) => self.warned_symbol_value(s, line, result).map(|v| match v {
                Value::Symbol(symbol) => {
                    Value::InstanceName(ferric_rules_core::InstanceName::from_symbol(symbol))
                }
                _ => unreachable!(),
            }),
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
    fn compile_rule_construct(
        &mut self,
        rule: &RuleConstruct,
        source: &str,
        expansion_budget: &mut crate::source_limits::LoadBudget,
    ) -> Result<CompileResult, LoadError> {
        Self::reject_logical_conditions(&rule.patterns)?;
        crate::source_limits::check_expansion(rule, expansion_budget)?;
        // Validate patterns first (max nesting depth: 4 to support deeply nested NCCs)
        let validation_errors = validate_rule_patterns(&rule.patterns, 4);
        if !validation_errors.is_empty() {
            return Err(LoadError::Validation(validation_errors));
        }
        // Expansion must not turn a nonbinding constraint reference into a
        // leading field binding in a synthetic rule variant.
        self.validate_rule_lhs_scope(rule)?;

        // Pre-process: distribute or CEs inside NCC/exists contexts.
        // This transforms patterns like (not (and A (or B C))) into
        // (and (not (and A B)) (not (and A C))) which can then be flattened.
        let rule = Self::normalize_nested_or_ces(rule);
        crate::source_limits::check_expansion(&rule, expansion_budget)?;

        // Expand (or ...) CEs via rule duplication: a rule with (or P1 P2) becomes
        // N internal rules, each with one branch substituted. Multiple or CEs produce
        // the Cartesian product.
        let expanded_rules = self.expand_or_patterns(&rule);
        if expanded_rules.is_empty() {
            return Err(LoadError::Compile("empty or-expansion".to_string()));
        }

        // Translation interns symbols, so include that table in the detached
        // planning transaction. No compiler, Rete, rule ID, or metadata state is
        // touched until every expansion is ready to install.
        let symbol_table_checkpoint = self.symbol_table.checkpoint();
        let mut prepared_rules = Vec::with_capacity(expanded_rules.len());
        for variant in &expanded_rules {
            match self.prepare_single_rule(variant, source) {
                Ok(prepared) => prepared_rules.push(prepared),
                Err(error) => {
                    self.symbol_table.restore(symbol_table_checkpoint);
                    return Err(error);
                }
            }
        }

        let maximum_new_nodes = prepared_rules
            .iter()
            .map(|prepared| prepared.plan.maximum_new_beta_nodes())
            .sum();
        if let Err(error) = self.rete.beta.ensure_node_capacity(maximum_new_nodes) {
            self.symbol_table.restore(symbol_table_checkpoint);
            return Err(LoadError::Compile(error.to_string()));
        }

        // Rule identity is its owning module plus local name. Retire all
        // internal disjunction variants only after the replacement is fully
        // prepared, leaving failed reloads and unrelated shared matches intact.
        let local_name = parse_qualified_name(&rule.name)
            .map_err(|error| LoadError::Compile(error.clone()))?
            .local_name()
            .to_string();
        let module = self.module_registry.current_module();
        let replaced: Vec<_> = self
            .rule_info
            .iter()
            .enumerate()
            .filter_map(|(index, info)| {
                let info = info.as_ref()?;
                let id = RuleId(u32::try_from(index).ok()?);
                let same_module =
                    crate::engine::rule_index_get(&self.rule_modules, id) == Some(&module);
                (same_module
                    && parse_qualified_name(&info.name)
                        .is_ok_and(|name| name.local_name() == local_name))
                .then_some(id)
            })
            .collect();
        self.remove_compiled_rules(&replaced);

        // Every operation from this point through installation is infallible.
        // Return the last result because all expansions share source semantics.
        let mut installed = prepared_rules.into_iter();
        let first = installed
            .next()
            .expect("non-empty expansion must produce a prepared rule");
        let mut last_result = self.install_prepared_rule(first);
        for prepared in installed {
            last_result = self.install_prepared_rule(prepared);
        }
        Ok(last_result)
    }

    /// Logical support cannot be approximated by an ordinary conjunction.
    /// Inspect the original tree before normalization can erase its wrapper.
    fn reject_logical_conditions(patterns: &[Pattern]) -> Result<(), LoadError> {
        let mut pending: Vec<_> = patterns.iter().collect();
        while let Some(pattern) = pending.pop() {
            match pattern {
                Pattern::Logical(_, span) => {
                    return Err(Self::unsupported_pattern(
                        "logical",
                        span,
                        "truth maintenance is not implemented; use ordinary stated facts with explicit retraction",
                    ));
                }
                Pattern::Not(inner, _) | Pattern::Assigned { pattern: inner, .. } => {
                    pending.push(inner);
                }
                Pattern::And(children, _)
                | Pattern::Or(children, _)
                | Pattern::Exists(children, _)
                | Pattern::Forall(children, _) => pending.extend(children),
                Pattern::Ordered(_) | Pattern::Template(_) | Pattern::Test(_, _) => {}
            }
        }
        Ok(())
    }

    fn validate_rule_action_callables(
        &self,
        rule: &RuleConstruct,
        current_module: crate::modules::ModuleId,
    ) -> Result<(), LoadError> {
        for action in &rule.actions {
            self.validate_queued_input_call(&action.call, current_module)?;
            self.validate_rule_action_call(&action.call, current_module, &rule.name)?;
        }
        Ok(())
    }

    fn validate_rule_action_call(
        &self,
        call: &FunctionCall,
        current_module: crate::modules::ModuleId,
        rule_name: &str,
    ) -> Result<(), LoadError> {
        match call.name.as_str() {
            "refresh-agenda" => Err(Self::compile_error_at(
                &call.span,
                "refresh-agenda is unsupported: only static salience is supported",
            )),
            // `(assert (relation ...))`: each argument list represents a fact pattern,
            // so the relation name is data, not a callable. For template facts,
            // slot names are also data and only slot values are expressions.
            "assert" => {
                for arg in &call.args {
                    if let ActionExpr::FunctionCall(fact_pattern) = arg {
                        if let Ok(template_id) =
                            self.resolve_template_id(&fact_pattern.name, current_module)
                        {
                            let registered = &self.template_defs[template_id];
                            let slots = registered.slot_overrides(&fact_pattern.args).map_err(
                                |message| Self::compile_error_at(&fact_pattern.span, &message),
                            )?;
                            for (index, default) in registered.defaults.iter().enumerate() {
                                if matches!(default, Value::Void)
                                    && !slots.iter().any(|(slot, _)| *slot == index)
                                {
                                    return Err(Self::compile_error_at(
                                        &fact_pattern.span,
                                        &format!(
                                            "slot `{}` in template `{}` requires a value because of its (default ?NONE) attribute",
                                            registered.slot_names[index], registered.name
                                        ),
                                    ));
                                }
                            }
                            for (_, slot_pair) in slots {
                                for value_expr in &slot_pair.args {
                                    self.validate_action_expr_as_expression(
                                        value_expr,
                                        current_module,
                                        rule_name,
                                    )?;
                                }
                            }
                        } else {
                            for field_expr in &fact_pattern.args {
                                self.validate_action_expr_as_expression(
                                    field_expr,
                                    current_module,
                                    rule_name,
                                )?;
                            }
                        }
                    } else {
                        self.validate_action_expr_as_expression(arg, current_module, rule_name)?;
                    }
                }
                Ok(())
            }
            // `(modify ?f (slot value) ...)` / `(duplicate ?f (slot value) ...)`:
            // slot names are data, but slot values are expressions.
            "modify" | "duplicate" => {
                if let Some(target) = call.args.first() {
                    self.validate_action_expr_as_expression(target, current_module, rule_name)?;
                }
                for slot_override in call.args.iter().skip(1) {
                    if let ActionExpr::FunctionCall(slot_pair) = slot_override {
                        for value_expr in &slot_pair.args {
                            self.validate_action_expr_as_expression(
                                value_expr,
                                current_module,
                                rule_name,
                            )?;
                        }
                    } else {
                        self.validate_action_expr_as_expression(
                            slot_override,
                            current_module,
                            rule_name,
                        )?;
                    }
                }
                Ok(())
            }
            name if Self::is_rule_action_wrapper(name) => {
                for arg in &call.args {
                    self.validate_action_expr_as_action(arg, current_module, rule_name)?;
                }
                Ok(())
            }
            name if Self::is_rule_action_builtin(name) => {
                for arg in &call.args {
                    self.validate_action_expr_as_expression(arg, current_module, rule_name)?;
                }
                Ok(())
            }
            _ => {
                self.validate_expression_callable_name(
                    &call.name,
                    &call.span,
                    current_module,
                    rule_name,
                )?;
                for arg in &call.args {
                    self.validate_action_expr_as_expression(arg, current_module, rule_name)?;
                }
                Ok(())
            }
        }
    }

    fn validate_action_expr_as_expression(
        &self,
        expr: &ActionExpr,
        current_module: crate::modules::ModuleId,
        rule_name: &str,
    ) -> Result<(), LoadError> {
        match expr {
            ActionExpr::Literal(_)
            | ActionExpr::Variable(_, _)
            | ActionExpr::GlobalVariable(_, _) => Ok(()),
            ActionExpr::FunctionCall(call) => {
                self.validate_expression_callable_name(
                    &call.name,
                    &call.span,
                    current_module,
                    rule_name,
                )?;
                for arg in &call.args {
                    self.validate_action_expr_as_expression(arg, current_module, rule_name)?;
                }
                Ok(())
            }
            ActionExpr::If {
                condition,
                then_actions,
                else_actions,
                ..
            } => {
                self.validate_action_expr_as_expression(condition, current_module, rule_name)?;
                for action in then_actions {
                    self.validate_action_expr_as_expression(action, current_module, rule_name)?;
                }
                for action in else_actions {
                    self.validate_action_expr_as_expression(action, current_module, rule_name)?;
                }
                Ok(())
            }
            ActionExpr::While {
                condition, body, ..
            } => {
                self.validate_action_expr_as_expression(condition, current_module, rule_name)?;
                for action in body {
                    self.validate_action_expr_as_expression(action, current_module, rule_name)?;
                }
                Ok(())
            }
            ActionExpr::LoopForCount {
                start, end, body, ..
            } => {
                self.validate_action_expr_as_expression(start, current_module, rule_name)?;
                self.validate_action_expr_as_expression(end, current_module, rule_name)?;
                for action in body {
                    self.validate_action_expr_as_expression(action, current_module, rule_name)?;
                }
                Ok(())
            }
            ActionExpr::Progn {
                list_expr, body, ..
            } => {
                self.validate_action_expr_as_expression(list_expr, current_module, rule_name)?;
                for action in body {
                    self.validate_action_expr_as_expression(action, current_module, rule_name)?;
                }
                Ok(())
            }
            ActionExpr::QueryAction { name, span, .. } => Err(Self::compile_error_at(
                span,
                &format!("{name} in an expression is unsupported; use a rule RHS do-for-* action or the host fact API"),
            )),
            ActionExpr::Switch {
                expr,
                cases,
                default,
                ..
            } => {
                self.validate_action_expr_as_expression(expr, current_module, rule_name)?;
                for (case_expr, actions) in cases {
                    self.validate_action_expr_as_expression(case_expr, current_module, rule_name)?;
                    for action in actions {
                        self.validate_action_expr_as_expression(action, current_module, rule_name)?;
                    }
                }
                if let Some(default_actions) = default {
                    for action in default_actions {
                        self.validate_action_expr_as_expression(action, current_module, rule_name)?;
                    }
                }
                Ok(())
            }
        }
    }

    fn validate_action_expr_as_action(
        &self,
        expr: &ActionExpr,
        current_module: crate::modules::ModuleId,
        rule_name: &str,
    ) -> Result<(), LoadError> {
        match expr {
            ActionExpr::Literal(_)
            | ActionExpr::Variable(_, _)
            | ActionExpr::GlobalVariable(_, _) => Ok(()),
            ActionExpr::FunctionCall(call) => {
                self.validate_rule_action_call(call, current_module, rule_name)
            }
            ActionExpr::If {
                condition,
                then_actions,
                else_actions,
                ..
            } => {
                self.validate_action_expr_as_expression(condition, current_module, rule_name)?;
                for action in then_actions {
                    self.validate_action_expr_as_action(action, current_module, rule_name)?;
                }
                for action in else_actions {
                    self.validate_action_expr_as_action(action, current_module, rule_name)?;
                }
                Ok(())
            }
            ActionExpr::While {
                condition, body, ..
            } => {
                self.validate_action_expr_as_expression(condition, current_module, rule_name)?;
                for action in body {
                    self.validate_action_expr_as_action(action, current_module, rule_name)?;
                }
                Ok(())
            }
            ActionExpr::LoopForCount {
                start, end, body, ..
            } => {
                self.validate_action_expr_as_expression(start, current_module, rule_name)?;
                self.validate_action_expr_as_expression(end, current_module, rule_name)?;
                for action in body {
                    self.validate_action_expr_as_action(action, current_module, rule_name)?;
                }
                Ok(())
            }
            ActionExpr::Progn {
                list_expr, body, ..
            } => {
                self.validate_action_expr_as_expression(list_expr, current_module, rule_name)?;
                for action in body {
                    self.validate_action_expr_as_action(action, current_module, rule_name)?;
                }
                Ok(())
            }
            ActionExpr::QueryAction { query, body, .. } => {
                self.validate_action_expr_as_expression(query, current_module, rule_name)?;
                for action in body {
                    self.validate_action_expr_as_action(action, current_module, rule_name)?;
                }
                Ok(())
            }
            ActionExpr::Switch {
                expr,
                cases,
                default,
                ..
            } => {
                self.validate_action_expr_as_expression(expr, current_module, rule_name)?;
                for (case_expr, actions) in cases {
                    self.validate_action_expr_as_expression(case_expr, current_module, rule_name)?;
                    for action in actions {
                        self.validate_action_expr_as_action(action, current_module, rule_name)?;
                    }
                }
                if let Some(default_actions) = default {
                    for action in default_actions {
                        self.validate_action_expr_as_action(action, current_module, rule_name)?;
                    }
                }
                Ok(())
            }
        }
    }

    fn validate_expression_callable_name(
        &self,
        callable: &str,
        span: &Span,
        current_module: crate::modules::ModuleId,
        rule_name: &str,
    ) -> Result<(), LoadError> {
        if callable == "refresh-agenda" {
            return Err(Self::compile_error_at(
                span,
                "refresh-agenda is unsupported: only static salience is supported",
            ));
        }
        if self.is_declared_expression_callable(callable, current_module) {
            return Ok(());
        }
        Err(Self::missing_function_declaration_error(
            callable, span, rule_name,
        ))
    }

    fn is_declared_expression_callable(
        &self,
        callable: &str,
        _current_module: crate::modules::ModuleId,
    ) -> bool {
        if callable == "__fact_slot_ref" {
            return true;
        }
        if callable == "call-next-method" || crate::evaluator::is_builtin_callable(callable) {
            return true;
        }

        match parse_qualified_name(callable) {
            // Keep module-qualified resolution on the runtime path so existing
            // visibility/module diagnostics remain unchanged.
            Ok(QualifiedName::Qualified { .. }) => true,
            Ok(QualifiedName::Unqualified(name)) => {
                !self.functions.modules_for_name(&name).is_empty()
                    || !self.generics.modules_for_name(&name).is_empty()
            }
            Err(_) => false,
        }
    }

    fn missing_function_declaration_error(
        callable: &str,
        span: &Span,
        rule_name: &str,
    ) -> LoadError {
        LoadError::Compile(format!(
            "[EXPRNPSR3] Missing function declaration for {callable} in rule `{rule_name}` at line {}, column {}",
            span.start.line, span.start.column
        ))
    }

    fn is_rule_action_builtin(name: &str) -> bool {
        matches!(
            name,
            "assert"
                | "retract"
                | "modify"
                | "duplicate"
                | "halt"
                | "reset"
                | "clear"
                | "printout"
                | "println"
                | "focus"
                | "list-focus-stack"
                | "agenda"
                | "rules"
                | "run"
                | "watch"
                | "unwatch"
                | "refresh-agenda"
                | "set-fact-duplication"
                | "get-fact-duplication"
                | "undefrule"
                | "undeffacts"
                | "ppdefrule"
                | "load"
                | "close"
                | "return"
                | "if"
                | "while"
                | "loop-for-count"
                | "switch"
                | "progn$"
                | "foreach"
                | "do-for-fact"
                | "do-for-all-facts"
                | "delayed-do-for-all-facts"
                | "any-factp"
                | "find-fact"
                | "find-all-facts"
        )
    }

    fn is_rule_action_wrapper(name: &str) -> bool {
        matches!(
            name,
            "if" | "while"
                | "loop-for-count"
                | "switch"
                | "progn$"
                | "foreach"
                | "do-for-fact"
                | "do-for-all-facts"
                | "delayed-do-for-all-facts"
                | "any-factp"
                | "find-fact"
                | "find-all-facts"
        )
    }

    fn prepare_single_rule(
        &mut self,
        rule: &RuleConstruct,
        source: &str,
    ) -> Result<PreparedRuleInstallation, LoadError> {
        self.validate_rule_action_callables(rule, self.module_registry.current_module())?;
        self.validate_rule_lhs_scope(rule)?;

        // Translate the LHS first to preserve source-order symbol interning, but
        // do not expose any Rete nodes or consume a rule ID until every fallible
        // RHS translation has also completed.
        let translated = self
            .translate_rule_construct(rule)
            .map_err(|e| LoadError::Compile(format!("{e}")))?;

        let mut runtime_actions = Vec::with_capacity(rule.actions.len());
        for action in &rule.actions {
            let expr = ActionExpr::FunctionCall(action.call.clone());
            let runtime_expr =
                crate::evaluator::from_action_expr(&expr, &mut self.symbol_table, &self.config)
                    .map_err(|error| {
                        LoadError::Compile(format!(
                            "rule `{}` action `{}` at line {}: {error}",
                            rule.name, action.call.name, action.call.span.start.line
                        ))
                    })?;
            runtime_actions.push(Some(runtime_expr));
        }

        let plan = self
            .compiler
            .plan_conditions(translated.salience, translated.conditions)
            .map_err(|e| LoadError::Compile(format!("{e}")))?;

        let source_definition = source
            .get(rule.span.start.offset..rule.span.end.offset)
            .map(str::trim_end)
            .filter(|snippet| !snippet.is_empty())
            .map(ToOwned::to_owned);

        // Store rule info for action execution
        let info = CompiledRuleInfo {
            name: rule.name.clone(),
            source_definition,
            actions: rule.actions.clone(),
            var_map: plan.var_map().clone(),
            fact_address_vars: translated.fact_address_vars,
            salience: Salience::new(rule.salience),
            test_conditions: translated.test_conditions,
            runtime_actions,
        };
        Ok(PreparedRuleInstallation {
            plan,
            info: Arc::new(info),
            module: self.module_registry.current_module(),
        })
    }

    fn install_prepared_rule(&mut self, prepared: PreparedRuleInstallation) -> CompileResult {
        // Reuse retired executable slots so repeated reload/undefine cycles
        // keep metadata bounded by the maximum number of simultaneous rules.
        let rule_id = self
            .rule_info
            .iter()
            .enumerate()
            .skip(1)
            .find_map(|(index, slot)| {
                slot.is_none()
                    .then(|| RuleId(u32::try_from(index).unwrap()))
            })
            .unwrap_or_else(|| self.compiler.allocate_rule_id());

        // Publish executable metadata before network initialization can produce
        // a predicate candidate or terminal activation for this rule.
        crate::engine::rule_index_insert(&mut self.rule_info, rule_id, prepared.info);
        crate::engine::rule_index_insert(&mut self.rule_modules, rule_id, prepared.module);

        let compile_result = self.compiler.install_condition_plan(
            &mut self.rete,
            &self.fact_base,
            rule_id,
            prepared.plan,
        );
        self.drain_pending_predicate_matches();

        compile_result
    }

    /// Recursively flatten a pattern for top-level condition processing.
    /// - `And`: flatten children. Logical CEs are rejected before translation.
    /// - Double negation remains intact so translation can compile it as exists.
    /// - Everything else: push as-is.
    fn flatten_pattern<'a>(pattern: &'a Pattern, out: &mut Vec<&'a Pattern>) {
        match pattern {
            Pattern::And(inner, _) => {
                for sub in inner {
                    Self::flatten_pattern(sub, out);
                }
            }
            _ => out.push(pattern),
        }
    }

    fn collect_pattern_binding_variables(pattern: &Pattern, variables: &mut HashSet<String>) {
        match pattern {
            Pattern::Ordered(ordered) => {
                for constraint in &ordered.constraints {
                    Self::collect_constraint_binding_variables(constraint, variables);
                }
            }
            Pattern::Template(template) => {
                for slot in &template.slot_constraints {
                    for constraint in &slot.constraints {
                        Self::collect_constraint_binding_variables(constraint, variables);
                    }
                }
            }
            Pattern::Assigned {
                variable, pattern, ..
            } => {
                variables.insert(variable.clone());
                Self::collect_pattern_binding_variables(pattern, variables);
            }
            Pattern::And(children, _)
            | Pattern::Logical(children, _)
            | Pattern::Or(children, _)
            | Pattern::Exists(children, _)
            | Pattern::Forall(children, _) => {
                for child in children {
                    Self::collect_pattern_binding_variables(child, variables);
                }
            }
            Pattern::Not(inner, _) => {
                Self::collect_pattern_binding_variables(inner, variables);
            }
            Pattern::Test(_, _) => {}
        }
    }

    fn collect_constraint_binding_variables(
        constraint: &Constraint,
        variables: &mut HashSet<String>,
    ) {
        match constraint {
            Constraint::Variable(name, _) | Constraint::MultiVariable(name, _) => {
                variables.insert(name.clone());
            }
            Constraint::And(parts, _) | Constraint::Or(parts, _) => {
                for part in parts {
                    Self::collect_constraint_binding_variables(part, variables);
                }
            }
            Constraint::Literal(_)
            | Constraint::Wildcard(_)
            | Constraint::MultiWildcard(_)
            | Constraint::Predicate(_, _)
            | Constraint::ReturnValue(_, _)
            | Constraint::Not(_, _) => {}
        }
    }

    fn collect_existential_local_variables(pattern: &Pattern, variables: &mut HashSet<String>) {
        match pattern {
            Pattern::Exists(children, _) => {
                for child in children {
                    Self::collect_pattern_binding_variables(child, variables);
                }
            }
            Pattern::Not(inner, _) => {
                if let Pattern::Not(existential, _) = inner.as_ref() {
                    Self::collect_pattern_binding_variables(existential, variables);
                } else {
                    Self::collect_existential_local_variables(inner, variables);
                }
            }
            Pattern::Assigned { pattern, .. } => {
                Self::collect_existential_local_variables(pattern, variables);
            }
            Pattern::And(children, _)
            | Pattern::Logical(children, _)
            | Pattern::Or(children, _)
            | Pattern::Forall(children, _) => {
                for child in children {
                    Self::collect_existential_local_variables(child, variables);
                }
            }
            Pattern::Ordered(_) | Pattern::Template(_) | Pattern::Test(_, _) => {}
        }
    }

    fn validate_rule_rhs_scope(
        rule: &RuleConstruct,
        existential_locals: &HashSet<String>,
        exported_variables: &HashSet<String>,
    ) -> Result<(), LoadError> {
        let scope = RuleRhsScope {
            exported: exported_variables,
            existential: existential_locals,
        };

        let mut rhs_locals = HashSet::new();
        for action in &rule.actions {
            Self::validate_rule_rhs_call(&rule.name, &action.call, &scope, &mut rhs_locals)?;
        }
        Ok(())
    }

    fn existential_scope_variable_name(name: &str) -> &str {
        name.strip_prefix("$?").unwrap_or(name)
    }

    fn validate_rule_rhs_call(
        rule_name: &str,
        call: &FunctionCall,
        scope: &RuleRhsScope<'_>,
        rhs_locals: &mut HashSet<String>,
    ) -> Result<(), LoadError> {
        if call.name == "bind" {
            if let Some(ActionExpr::Variable(name, _)) = call.args.first() {
                for value in call.args.iter().skip(1) {
                    Self::validate_rule_rhs_expr(rule_name, value, scope, rhs_locals)?;
                }
                rhs_locals.insert(Self::existential_scope_variable_name(name).to_string());
                return Ok(());
            }
        }

        for arg in &call.args {
            Self::validate_rule_rhs_expr(rule_name, arg, scope, rhs_locals)?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_lines)] // Mirrors every structured RHS scope in ActionExpr.
    fn validate_rule_rhs_expr(
        rule_name: &str,
        expr: &ActionExpr,
        scope: &RuleRhsScope<'_>,
        rhs_locals: &mut HashSet<String>,
    ) -> Result<(), LoadError> {
        match expr {
            ActionExpr::Variable(name, span) => {
                let scope_name = Self::existential_scope_variable_name(name);
                if !scope.exported.contains(scope_name) && !rhs_locals.contains(scope_name) {
                    let display_name = if name.starts_with("$?") {
                        name.clone()
                    } else {
                        format!("?{name}")
                    };
                    let reason = if scope.existential.contains(scope_name) {
                        "is not exported by existential conditional element"
                    } else {
                        "is an unbound RHS variable"
                    };
                    return Err(LoadError::Compile(format!(
                        "[PRCCODE3] rule `{rule_name}` variable {display_name} at line {} {reason}",
                        span.start.line
                    )));
                }
                Ok(())
            }
            ActionExpr::FunctionCall(call) => {
                Self::validate_rule_rhs_call(rule_name, call, scope, rhs_locals)
            }
            ActionExpr::If {
                condition,
                then_actions,
                else_actions,
                ..
            } => {
                Self::validate_rule_rhs_expr(rule_name, condition, scope, rhs_locals)?;
                let mut then_locals = rhs_locals.clone();
                for action in then_actions {
                    Self::validate_rule_rhs_expr(rule_name, action, scope, &mut then_locals)?;
                }
                let mut else_locals = rhs_locals.clone();
                for action in else_actions {
                    Self::validate_rule_rhs_expr(rule_name, action, scope, &mut else_locals)?;
                }
                rhs_locals.extend(then_locals);
                rhs_locals.extend(else_locals);
                Ok(())
            }
            ActionExpr::While {
                condition, body, ..
            } => {
                Self::validate_rule_rhs_expr(rule_name, condition, scope, rhs_locals)?;
                let mut body_locals = rhs_locals.clone();
                for action in body {
                    Self::validate_rule_rhs_expr(rule_name, action, scope, &mut body_locals)?;
                }
                rhs_locals.extend(body_locals);
                Ok(())
            }
            ActionExpr::LoopForCount {
                var_name,
                start,
                end,
                body,
                ..
            } => {
                Self::validate_rule_rhs_expr(rule_name, start, scope, rhs_locals)?;
                Self::validate_rule_rhs_expr(rule_name, end, scope, rhs_locals)?;
                let mut body_locals = rhs_locals.clone();
                if let Some(name) = var_name {
                    body_locals.insert(name.clone());
                }
                for action in body {
                    Self::validate_rule_rhs_expr(rule_name, action, scope, &mut body_locals)?;
                }
                if let Some(name) = var_name {
                    body_locals.remove(name);
                }
                rhs_locals.extend(body_locals);
                Ok(())
            }
            ActionExpr::Progn {
                var_name,
                list_expr,
                body,
                ..
            } => {
                Self::validate_rule_rhs_expr(rule_name, list_expr, scope, rhs_locals)?;
                let mut body_locals = rhs_locals.clone();
                body_locals.insert(var_name.clone());
                body_locals.insert(format!("{var_name}-index"));
                for action in body {
                    Self::validate_rule_rhs_expr(rule_name, action, scope, &mut body_locals)?;
                }
                body_locals.remove(var_name);
                body_locals.remove(&format!("{var_name}-index"));
                rhs_locals.extend(body_locals);
                Ok(())
            }
            ActionExpr::QueryAction {
                bindings,
                query,
                body,
                ..
            } => {
                let mut query_locals = rhs_locals.clone();
                query_locals.extend(bindings.iter().map(|(name, _)| name.clone()));
                Self::validate_rule_rhs_expr(rule_name, query, scope, &mut query_locals)?;
                for action in body {
                    Self::validate_rule_rhs_expr(rule_name, action, scope, &mut query_locals)?;
                }
                for (name, _) in bindings {
                    query_locals.remove(name);
                }
                rhs_locals.extend(query_locals);
                Ok(())
            }
            ActionExpr::Switch {
                expr,
                cases,
                default,
                ..
            } => {
                Self::validate_rule_rhs_expr(rule_name, expr, scope, rhs_locals)?;
                for (case_expr, actions) in cases {
                    Self::validate_rule_rhs_expr(rule_name, case_expr, scope, rhs_locals)?;
                    let mut case_locals = rhs_locals.clone();
                    for action in actions {
                        Self::validate_rule_rhs_expr(rule_name, action, scope, &mut case_locals)?;
                    }
                    rhs_locals.extend(case_locals);
                }
                if let Some(actions) = default {
                    let mut default_locals = rhs_locals.clone();
                    for action in actions {
                        Self::validate_rule_rhs_expr(
                            rule_name,
                            action,
                            scope,
                            &mut default_locals,
                        )?;
                    }
                    rhs_locals.extend(default_locals);
                }
                Ok(())
            }
            ActionExpr::Literal(_) | ActionExpr::GlobalVariable(_, _) => Ok(()),
        }
    }

    fn sexpr_references_any_variable(expr: &SExpr, slot_variables: &HashSet<String>) -> bool {
        match expr {
            SExpr::Atom(Atom::SingleVar(name) | Atom::MultiVar(name), _) => {
                slot_variables.contains(name)
            }
            SExpr::Atom(_, _) => false,
            SExpr::List(items, _) => items
                .iter()
                .any(|item| Self::sexpr_references_any_variable(item, slot_variables)),
        }
    }

    /// Normalize nested or CEs by distributing them across enclosing contexts.
    ///
    /// Transforms:
    /// - `not(and(A, or(B, C), D))` → `and(not(and(A, B, D)), not(and(A, C, D)))`
    /// - `exists(or(P1, P2))` → `or(exists(P1), exists(P2))`
    ///
    /// This runs recursively so that or CEs at any nesting depth are resolved.
    fn normalize_nested_or_ces(rule: &RuleConstruct) -> RuleConstruct {
        let patterns = rule
            .patterns
            .iter()
            .flat_map(Self::normalize_pattern)
            .collect();

        RuleConstruct {
            name: rule.name.clone(),
            span: rule.span,
            comment: rule.comment.clone(),
            salience: rule.salience,
            patterns,
            actions: rule.actions.clone(),
        }
    }

    /// Recursively normalize a single pattern, resolving or CEs in nested
    /// contexts. May return multiple patterns if a `Not(And(...Or...))` is
    /// distributed.
    fn normalize_pattern(pattern: &Pattern) -> Vec<Pattern> {
        match pattern {
            Pattern::Not(inner, span) => {
                if let Pattern::And(children, and_span) = inner.as_ref() {
                    // Check if any child is an Or CE
                    let or_idx = children.iter().position(|c| matches!(c, Pattern::Or(..)));
                    if let Some(idx) = or_idx {
                        if let Pattern::Or(branches, _) = &children[idx] {
                            // Distribute: not(and(A, or(B, C), D))
                            // → [not(and(A, B, D)), not(and(A, C, D))]
                            // If a branch is itself an And, flatten it into the parent:
                            // not(and(A, or(and(B, C), D))) → [not(and(A, B, C)), not(and(A, D))]
                            let mut results = Vec::new();
                            for branch in branches {
                                let mut new_children = children.clone();
                                if let Pattern::And(branch_children, _) = branch {
                                    // Flatten: replace the or slot with the And's children
                                    new_children.splice(idx..=idx, branch_children.iter().cloned());
                                } else {
                                    new_children[idx] = branch.clone();
                                }
                                let new_and = Pattern::And(new_children, *and_span);
                                let new_not = Pattern::Not(Box::new(new_and), *span);
                                // Recursively normalize in case there are more or CEs
                                results.extend(Self::normalize_pattern(&new_not));
                            }
                            return results;
                        }
                    }
                    // No or CE found — recurse into children
                    let normalized_children: Vec<Pattern> =
                        children.iter().flat_map(Self::normalize_pattern).collect();
                    vec![Pattern::Not(
                        Box::new(Pattern::And(normalized_children, *and_span)),
                        *span,
                    )]
                } else {
                    // Recurse into the inner pattern
                    let normalized = Self::normalize_pattern(inner);
                    if normalized.len() == 1 {
                        vec![Pattern::Not(
                            Box::new(normalized.into_iter().next().unwrap()),
                            *span,
                        )]
                    } else {
                        // Multiple patterns from inner normalization —
                        // wrap each in Not
                        normalized
                            .into_iter()
                            .map(|p| Pattern::Not(Box::new(p), *span))
                            .collect()
                    }
                }
            }
            Pattern::Exists(children, span) => {
                // Check if any child is an Or CE
                let or_idx = children.iter().position(|c| matches!(c, Pattern::Or(..)));
                if let Some(idx) = or_idx {
                    if let Pattern::Or(branches, _) = &children[idx] {
                        // exists(A, or(B, C), D) → or(exists(A, B, D), exists(A, C, D))
                        // If a branch is an And, flatten: exists(A, or(and(B,C), D))
                        // → or(exists(A, B, C), exists(A, D))
                        let mut or_branches = Vec::new();
                        for branch in branches {
                            let mut new_children = children.clone();
                            if let Pattern::And(branch_children, _) = branch {
                                new_children.splice(idx..=idx, branch_children.iter().cloned());
                            } else {
                                new_children[idx] = branch.clone();
                            }
                            or_branches.push(Pattern::Exists(new_children, *span));
                        }
                        return vec![Pattern::Or(or_branches, *span)];
                    }
                }
                // Recurse into children
                let normalized: Vec<Pattern> =
                    children.iter().flat_map(Self::normalize_pattern).collect();
                vec![Pattern::Exists(normalized, *span)]
            }
            Pattern::And(children, span) => {
                let normalized: Vec<Pattern> =
                    children.iter().flat_map(Self::normalize_pattern).collect();
                vec![Pattern::And(normalized, *span)]
            }
            Pattern::Assigned {
                variable,
                pattern: inner,
                span,
            } => {
                let normalized = Self::normalize_pattern(inner);
                normalized
                    .into_iter()
                    .map(|p| Pattern::Assigned {
                        variable: variable.clone(),
                        pattern: Box::new(p),
                        span: *span,
                    })
                    .collect()
            }
            // All other patterns pass through unchanged
            _ => vec![pattern.clone()],
        }
    }

    /// Expand `Pattern::Or` CEs via rule duplication.
    /// Also expands slot-level `Constraint::Or` disjunctions inside patterns.
    /// Returns a vec of rule variants (1 if no disjunctions, N*M*... for Cartesian product).
    fn expand_or_patterns(&self, rule: &RuleConstruct) -> Vec<RuleConstruct> {
        // First flatten top-level And to expose Or patterns
        let mut flat_patterns: Vec<Pattern> = Vec::new();
        for pattern in &rule.patterns {
            match pattern {
                Pattern::And(inner, _) => {
                    flat_patterns.extend(inner.iter().cloned());
                }
                _ => flat_patterns.push(pattern.clone()),
            }
        }

        // Build Cartesian product of all pattern-level alternatives.
        // Alternatives come from:
        // - top-level `or` CEs
        // - assigned wrappers over top-level `or` CEs
        // - slot-level `|` disjunctions distributed into separate pattern variants
        let mut pattern_options: Vec<Vec<Pattern>> = Vec::new();
        for pattern in &flat_patterns {
            pattern_options.push(self.pattern_disjunction_options(pattern));
        }

        if pattern_options.iter().all(|options| options.len() <= 1) {
            return vec![rule.clone()];
        }

        // Compute Cartesian product
        let mut combinations: Vec<Vec<Pattern>> = vec![vec![]];
        for options in &pattern_options {
            let mut new_combinations = Vec::new();
            for combo in &combinations {
                for option in options {
                    let mut new_combo = combo.clone();
                    new_combo.push(option.clone());
                    new_combinations.push(new_combo);
                }
            }
            combinations = new_combinations;
        }

        // Create rule variants
        combinations
            .into_iter()
            .map(|patterns| RuleConstruct {
                name: rule.name.clone(),
                span: rule.span,
                comment: rule.comment.clone(),
                salience: rule.salience,
                patterns,
                actions: rule.actions.clone(),
            })
            .collect()
    }

    /// Expand a top-level pattern into disjunctive alternatives used for rule duplication.
    fn pattern_disjunction_options(&self, pattern: &Pattern) -> Vec<Pattern> {
        let expanded = self.expand_pattern_constraint_disjunctions(pattern);
        let mut options = Vec::new();
        for variant in expanded {
            match variant {
                Pattern::Or(branches, _) => {
                    options.extend(branches);
                }
                Pattern::Assigned {
                    variable,
                    pattern,
                    span,
                } => {
                    if let Pattern::Or(branches, _) = pattern.as_ref() {
                        options.extend(branches.iter().cloned().map(|branch| Pattern::Assigned {
                            variable: variable.clone(),
                            pattern: Box::new(branch),
                            span,
                        }));
                    } else {
                        options.push(Pattern::Assigned {
                            variable,
                            pattern,
                            span,
                        });
                    }
                }
                other => options.push(other),
            }
        }

        if options.is_empty() {
            vec![pattern.clone()]
        } else {
            options
        }
    }

    /// Recursively expand slot-level `Constraint::Or` disjunctions into pattern variants.
    fn expand_pattern_constraint_disjunctions(&self, pattern: &Pattern) -> Vec<Pattern> {
        if self.pattern_needs_runtime_disjunction(pattern) {
            return vec![pattern.clone()];
        }
        match pattern {
            Pattern::Ordered(ordered) => Self::expand_ordered_pattern_disjunctions(ordered)
                .into_iter()
                .map(Pattern::Ordered)
                .collect(),
            Pattern::Template(template) => Self::expand_template_pattern_disjunctions(template)
                .into_iter()
                .map(Pattern::Template)
                .collect(),
            Pattern::Assigned {
                variable,
                pattern: inner,
                span,
            } => self
                .expand_pattern_constraint_disjunctions(inner)
                .into_iter()
                .map(|p| Pattern::Assigned {
                    variable: variable.clone(),
                    pattern: Box::new(p),
                    span: *span,
                })
                .collect(),
            Pattern::Not(inner, span) => self
                .expand_pattern_constraint_disjunctions(inner)
                .into_iter()
                .map(|p| Pattern::Not(Box::new(p), *span))
                .collect(),
            Pattern::And(children, span) => self
                .expand_child_pattern_product(children)
                .into_iter()
                .map(|combo| Pattern::And(combo, *span))
                .collect(),
            Pattern::Logical(children, span) => self
                .expand_child_pattern_product(children)
                .into_iter()
                .map(|combo| Pattern::Logical(combo, *span))
                .collect(),
            Pattern::Exists(children, span) => self
                .expand_child_pattern_product(children)
                .into_iter()
                .map(|combo| Pattern::Exists(combo, *span))
                .collect(),
            Pattern::Forall(children, span) => self
                .expand_child_pattern_product(children)
                .into_iter()
                .map(|combo| Pattern::Forall(combo, *span))
                .collect(),
            Pattern::Or(children, span) => self
                .expand_child_pattern_product(children)
                .into_iter()
                .map(|combo| Pattern::Or(combo, *span))
                .collect(),
            Pattern::Test(_, _) => vec![pattern.clone()],
        }
    }

    fn expand_ordered_pattern_disjunctions(pattern: &OrderedPattern) -> Vec<OrderedPattern> {
        let per_slot: Vec<Vec<Constraint>> = pattern
            .constraints
            .iter()
            .map(|constraint| {
                Self::expand_constraint_disjunctions(&Self::field_constraint(constraint))
            })
            .collect();
        Self::cartesian_product(&per_slot)
            .into_iter()
            .map(|constraints| OrderedPattern {
                relation: pattern.relation.clone(),
                constraints,
                span: pattern.span,
            })
            .collect()
    }

    fn expand_template_pattern_disjunctions(pattern: &TemplatePattern) -> Vec<TemplatePattern> {
        let per_slot: Vec<Vec<SlotConstraint>> = pattern
            .slot_constraints
            .iter()
            .map(|slot_constraint| {
                let per_field: Vec<_> = slot_constraint
                    .constraints
                    .iter()
                    .map(|constraint| {
                        Self::expand_constraint_disjunctions(&Self::field_constraint(constraint))
                    })
                    .collect();
                Self::cartesian_product(&per_field)
                    .into_iter()
                    .map(|constraints| SlotConstraint {
                        slot_name: slot_constraint.slot_name.clone(),
                        constraints,
                        span: slot_constraint.span,
                    })
                    .collect()
            })
            .collect();

        Self::cartesian_product(&per_slot)
            .into_iter()
            .map(|slot_constraints| TemplatePattern {
                template: pattern.template.clone(),
                slot_constraints,
                span: pattern.span,
            })
            .collect()
    }

    /// Expand a constraint into alternatives by distributing nested `or` inside `and`.
    fn expand_constraint_disjunctions(constraint: &Constraint) -> Vec<Constraint> {
        match constraint {
            Constraint::Or(branches, _) => branches
                .iter()
                .flat_map(Self::expand_constraint_disjunctions)
                .collect(),
            Constraint::And(parts, span) => {
                let per_part: Vec<Vec<Constraint>> = parts
                    .iter()
                    .map(Self::expand_constraint_disjunctions)
                    .collect();
                Self::cartesian_product(&per_part)
                    .into_iter()
                    .map(|parts| {
                        if parts.len() == 1 {
                            parts.into_iter().next().unwrap()
                        } else {
                            Constraint::And(parts, *span)
                        }
                    })
                    .collect()
            }
            _ => vec![constraint.clone()],
        }
    }

    fn expand_child_pattern_product(&self, children: &[Pattern]) -> Vec<Vec<Pattern>> {
        let per_child: Vec<Vec<Pattern>> = children
            .iter()
            .map(|pattern| self.expand_pattern_constraint_disjunctions(pattern))
            .collect();
        Self::cartesian_product(&per_child)
    }

    fn cartesian_product<T: Clone>(choices: &[Vec<T>]) -> Vec<Vec<T>> {
        let mut product: Vec<Vec<T>> = vec![vec![]];
        for options in choices {
            let mut next = Vec::new();
            for combo in &product {
                for option in options {
                    let mut new_combo = combo.clone();
                    new_combo.push(option.clone());
                    next.push(new_combo);
                }
            }
            product = next;
        }
        product
    }

    /// Plan every source condition before publishing any graph nodes.
    fn translate_rule_construct(
        &mut self,
        rule: &RuleConstruct,
    ) -> Result<TranslatedRule, LoadError> {
        let mut plan = runtime_constraints::RuleConstraintPlan::new(rule);
        let mut fact_address_vars = HashMap::new();
        let mut existential_locals = HashSet::new();
        let mut fact_index = 0;
        let mut flat_patterns = Vec::new();
        for pattern in &rule.patterns {
            Self::flatten_pattern(pattern, &mut flat_patterns);
        }
        let mut conditions = Vec::new();
        for pattern in flat_patterns {
            Self::collect_existential_local_variables(pattern, &mut existential_locals);
            self.lower_lhs_condition(pattern, &mut plan, &mut conditions)?;
            if Self::is_positive_fact_pattern(pattern) {
                if let Pattern::Assigned { variable, .. } = pattern {
                    fact_address_vars.insert(variable.clone(), fact_index);
                }
                fact_index += 1;
            }
        }
        Self::validate_rule_rhs_scope(rule, &existential_locals, &plan.available)?;
        Ok(TranslatedRule {
            salience: Salience::new(rule.salience),
            conditions,
            fact_address_vars,
            test_conditions: plan.conditions,
        })
    }

    fn is_positive_fact_pattern(pattern: &Pattern) -> bool {
        match pattern {
            Pattern::Ordered(_) | Pattern::Template(_) => true,
            Pattern::Assigned { pattern, .. } => Self::is_positive_fact_pattern(pattern),
            _ => false,
        }
    }

    /// Translate a single `Pattern` into a `CompilablePattern`.
    #[allow(clippy::too_many_lines)] // Template pattern arm adds lines but is clear as-is
    fn translate_pattern(
        &mut self,
        pattern: &Pattern,
        generated_tests: &mut Vec<crate::evaluator::RuntimeExpr>,
        internal_slot_var_seed: &mut usize,
        in_negated_pattern: bool,
    ) -> Result<CompilablePattern, LoadError> {
        match pattern {
            Pattern::Ordered(ordered) => {
                // The parser cannot distinguish `(template-name)` from an
                // empty ordered pattern without the runtime template registry.
                let current_module = self.module_registry.current_module();
                let entry_type = if let Ok(template_id) =
                    self.resolve_template_id(&ordered.relation, current_module)
                {
                    if !ordered.constraints.is_empty() {
                        return Err(Self::compile_error_at(
                            &ordered.span,
                            &format!(
                                "template `{}` requires named slot constraints",
                                ordered.relation
                            ),
                        ));
                    }
                    AlphaEntryType::Template(template_id)
                } else {
                    let sym = self.compile_symbol(&ordered.relation)?;
                    AlphaEntryType::OrderedRelation(sym)
                };
                let mut constant_tests = Vec::new();
                if matches!(entry_type, AlphaEntryType::OrderedRelation(_)) {
                    // Single-field constraints consume exactly one field, even
                    // when anonymous. Multifield constraints may consume none
                    // or more, so only their fixed neighbors set a lower bound.
                    let min = ordered
                        .constraints
                        .iter()
                        .filter(|constraint| !Self::constraint_is_multifield(constraint))
                        .count();
                    let max = (min == ordered.constraints.len()).then_some(min);
                    constant_tests.push(ConstantTest {
                        slot: SlotIndex::Ordered(0),
                        test_type: ConstantTestType::OrderedFieldCount { min, max },
                    });
                }
                let mut variable_slots = Vec::new();
                let mut negated_variable_slots = Vec::new();
                let mut seen_variable_slots = HashMap::new();
                let mut slot_runtime_vars = HashMap::new();

                for (i, constraint) in ordered.constraints.iter().enumerate() {
                    let slot = SlotIndex::Ordered(i);
                    self.translate_constraint(
                        &Self::field_constraint(constraint),
                        slot,
                        &mut constant_tests,
                        &mut variable_slots,
                        &mut negated_variable_slots,
                        &mut seen_variable_slots,
                        generated_tests,
                        &mut slot_runtime_vars,
                        internal_slot_var_seed,
                        in_negated_pattern,
                    )?;
                }

                let sequence = ordered
                    .constraints
                    .iter()
                    .position(Self::constraint_is_multifield)
                    .map(|first_multifield| {
                        let (prefix_tests, tests): (Vec<_>, Vec<_>) = constant_tests
                            .split_off(1)
                            .into_iter()
                            .partition(|test| {
                                Self::test_uses_ordered_prefix(test, first_multifield)
                            });
                        // Fixed prefix selectors are also physical positions;
                        // retain their alpha filtering before enumerating splits.
                        // The first test is always the raw fact cardinality.
                        constant_tests.extend(prefix_tests);
                        SequencePattern {
                            segments: vec![SequenceSegment {
                                source: SequenceSource::Ordered,
                                fields: ordered
                                    .constraints
                                    .iter()
                                    .map(Self::sequence_field)
                                    .collect(),
                            }],
                            tests,
                        }
                    });

                Ok(CompilablePattern {
                    entry_type,
                    constant_tests,
                    sequence,
                    variable_slots,
                    negated_variable_slots,
                    negated: false,
                    exists: false,
                })
            }
            Pattern::Assigned { pattern, .. } => {
                // Unwrap the assignment and compile the inner pattern
                self.translate_pattern(
                    pattern,
                    generated_tests,
                    internal_slot_var_seed,
                    in_negated_pattern,
                )
            }
            Pattern::Not(inner, _span) => {
                // Unwrap the inner pattern and set negated flag
                let mut compilable =
                    self.translate_pattern(inner, generated_tests, internal_slot_var_seed, true)?;
                compilable.negated = true;
                Ok(compilable)
            }
            Pattern::Exists(patterns, span) => {
                // For single-pattern exists, compile as an exists pattern
                if patterns.len() == 1 {
                    let mut compilable = self.translate_pattern(
                        &patterns[0],
                        generated_tests,
                        internal_slot_var_seed,
                        in_negated_pattern,
                    )?;
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
            Pattern::Test(_, span) => {
                // Safety net: test CEs should be intercepted in translate_rule_construct
                // before reaching translate_pattern.  If we get here something has gone wrong.
                Err(Self::unsupported_pattern(
                    "test",
                    span,
                    "test CE reached translate_pattern unexpectedly (should be handled earlier)",
                ))
            }
            Pattern::Template(template) => {
                let current_module = self.module_registry.current_module();
                let template_id = self
                    .resolve_template_reference(&template.template, current_module)
                    .map_err(|msg| Self::compile_error_at(&template.span, &msg))?;

                let registered =
                    self.template_defs
                        .get(template_id)
                        .cloned()
                        .ok_or_else(|| {
                            Self::compile_error_at(
                                &template.span,
                                &format!("template `{}` not found in registry", template.template),
                            )
                        })?;

                let mut slot_indices = Vec::with_capacity(template.slot_constraints.len());
                let mut seen_slots = HashSet::new();
                for slot_constraint in &template.slot_constraints {
                    let slot_idx = registered.slot_index(&slot_constraint.slot_name).ok_or_else(
                        || {
                            Self::compile_error_at(
                                &slot_constraint.span,
                                &format!(
                                    "unknown slot `{}` in template `{}`",
                                    slot_constraint.slot_name, template.template
                                ),
                            )
                        },
                    )?;
                    if !seen_slots.insert(slot_idx) {
                        return Err(Self::compile_error_at(
                            &slot_constraint.span,
                            &format!("duplicate slot `{}` in template pattern", slot_constraint.slot_name),
                        ));
                    }
                    if registered.slot_types[slot_idx] == SlotType::Single {
                        if slot_constraint.constraints.len() != 1 {
                            return Err(Self::compile_error_at(
                                &slot_constraint.span,
                                &format!("single-field slot `{}` requires exactly one field constraint", slot_constraint.slot_name),
                            ));
                        }
                        let constraint = &slot_constraint.constraints[0];
                        if Self::constraint_is_multifield(constraint)
                            && !matches!(constraint, Constraint::MultiWildcard(_))
                        {
                            return Err(Self::compile_error_at(
                                &slot_constraint.span,
                                &format!("single-field slot `{}` cannot bind a multifield variable", slot_constraint.slot_name),
                            ));
                        }
                    }
                    slot_indices.push(slot_idx);
                }

                let needs_sequence = slot_indices.iter().any(|&index| {
                    registered.slot_types[index] == SlotType::Multi
                });
                let mut constant_tests = Vec::new();
                let mut variable_slots = Vec::new();
                let mut negated_variable_slots = Vec::new();
                let mut seen_variable_slots = HashMap::new();
                let mut slot_runtime_vars = HashMap::new();
                let mut segments = Vec::new();
                let mut scalar_slots = HashMap::new();
                let mut logical_offset = 0;

                // Preserve written slot order: independent multislot splits
                // form a Cartesian product in that order in CLIPS.
                for (slot_constraint, slot_idx) in template.slot_constraints.iter().zip(slot_indices) {
                    let is_multi = registered.slot_types[slot_idx] == SlotType::Multi;
                    if needs_sequence {
                        segments.push(SequenceSegment {
                            source: SequenceSource::TemplateSlot(slot_idx),
                            fields: slot_constraint.constraints.iter().map(|constraint| {
                                if is_multi { Self::sequence_field(constraint) } else { SequenceField::Single }
                            }).collect(),
                        });
                        if !is_multi {
                            scalar_slots.insert(logical_offset, slot_idx);
                        }
                    }
                    for constraint in &slot_constraint.constraints {
                        let slot = SlotIndex::Template(if needs_sequence { logical_offset } else { slot_idx });
                        self.translate_constraint(
                            &Self::field_constraint(constraint),
                            slot,
                            &mut constant_tests,
                            &mut variable_slots,
                            &mut negated_variable_slots,
                            &mut seen_variable_slots,
                            generated_tests,
                            &mut slot_runtime_vars,
                            internal_slot_var_seed,
                            in_negated_pattern,
                        )?;
                        logical_offset += 1;
                    }
                }

                let sequence = needs_sequence.then(|| {
                    let mut tests = Vec::new();
                    let mut alpha_tests = Vec::new();
                    for test in constant_tests.drain(..) {
                        if let Some(physical) = Self::physical_template_test(&test, &scalar_slots) {
                            alpha_tests.push(physical);
                        } else {
                            tests.push(test);
                        }
                    }
                    constant_tests = alpha_tests;
                    SequencePattern { segments, tests }
                });

                Ok(CompilablePattern {
                    entry_type: AlphaEntryType::Template(template_id),
                    constant_tests,
                    sequence,
                    variable_slots,
                    negated_variable_slots,
                    negated: false,
                    exists: false,
                })
            }

            Pattern::Forall(_, span) => Err(Self::unsupported_pattern(
                "forall",
                span,
                "forall CE reached translate_pattern unexpectedly (should be handled in lower_lhs_condition)",
            )),
            Pattern::And(_, span) => Err(Self::unsupported_pattern(
                "and",
                span,
                "and conditional elements are only supported inside (not (and ...))",
            )),
            Pattern::Logical(_, span) => Err(Self::unsupported_pattern(
                "logical",
                span,
                "truth maintenance is not implemented",
            )),
            Pattern::Or(_, span) => Err(Self::unsupported_pattern(
                "or",
                span,
                "or CE reached translate_pattern unexpectedly (should be expanded via rule duplication)",
            )),
        }
    }

    fn sequence_field(constraint: &Constraint) -> SequenceField {
        if Self::constraint_is_multifield(constraint) {
            SequenceField::Multi
        } else {
            SequenceField::Single
        }
    }

    // Tests on scalar sibling slots can filter physical facts before any
    // multislot projection. Both sides of a slot comparison must be scalar.
    fn physical_template_test(
        test: &ConstantTest,
        scalar_slots: &HashMap<usize, usize>,
    ) -> Option<ConstantTest> {
        let physical = |slot| {
            let SlotIndex::Template(index) = slot else {
                return None;
            };
            scalar_slots.get(&index).copied().map(SlotIndex::Template)
        };
        let mut mapped = test.clone();
        mapped.slot = physical(test.slot)?;
        match &mut mapped.test_type {
            ConstantTestType::EqualSlot(other)
            | ConstantTestType::NotEqualSlot(other)
            | ConstantTestType::EqualSlotOffset(other, _)
            | ConstantTestType::NotEqualSlotOffset(other, _)
            | ConstantTestType::GreaterThanSlotOffset(other, _)
            | ConstantTestType::LessThanSlotOffset(other, _)
            | ConstantTestType::GreaterOrEqualSlotOffset(other, _)
            | ConstantTestType::LessOrEqualSlotOffset(other, _) => *other = physical(*other)?,
            ConstantTestType::OrderedFieldCount { .. } => return None,
            _ => {}
        }
        Some(mapped)
    }

    fn test_uses_ordered_prefix(test: &ConstantTest, end: usize) -> bool {
        let in_prefix = |slot| matches!(slot, SlotIndex::Ordered(index) if index < end);
        if !in_prefix(test.slot) {
            return false;
        }
        match test.test_type {
            ConstantTestType::EqualSlot(other)
            | ConstantTestType::NotEqualSlot(other)
            | ConstantTestType::EqualSlotOffset(other, _)
            | ConstantTestType::NotEqualSlotOffset(other, _)
            | ConstantTestType::GreaterThanSlotOffset(other, _)
            | ConstantTestType::LessThanSlotOffset(other, _)
            | ConstantTestType::GreaterOrEqualSlotOffset(other, _)
            | ConstantTestType::LessOrEqualSlotOffset(other, _) => in_prefix(other),
            _ => true,
        }
    }

    fn constraint_is_multifield(constraint: &Constraint) -> bool {
        match constraint {
            Constraint::MultiVariable(_, _) | Constraint::MultiWildcard(_) => true,
            Constraint::And(parts, _) | Constraint::Or(parts, _) => {
                parts.iter().any(Self::constraint_is_multifield)
            }
            Constraint::Not(inner, _) => Self::constraint_is_multifield(inner),
            Constraint::Literal(_)
            | Constraint::Variable(_, _)
            | Constraint::Wildcard(_)
            | Constraint::Predicate(_, _)
            | Constraint::ReturnValue(_, _) => false,
        }
    }

    /// Translate a single `Constraint` into constant tests and/or variable slots.
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    fn translate_constraint(
        &mut self,
        constraint: &Constraint,
        slot: SlotIndex,
        constant_tests: &mut Vec<ConstantTest>,
        variable_slots: &mut Vec<(SlotIndex, ferric_rules_core::Symbol)>,
        negated_variable_slots: &mut Vec<(SlotIndex, ferric_rules_core::Symbol, JoinTestType)>,
        seen_variable_slots: &mut HashMap<ferric_rules_core::Symbol, SlotIndex>,
        generated_tests: &mut Vec<crate::evaluator::RuntimeExpr>,
        slot_runtime_vars: &mut HashMap<SlotIndex, String>,
        internal_slot_var_seed: &mut usize,
        in_negated_pattern: bool,
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
                self.translate_variable_constraint(
                    name,
                    slot,
                    constant_tests,
                    variable_slots,
                    seen_variable_slots,
                )?;
            }
            Constraint::Wildcard(_) | Constraint::MultiWildcard(_) => {
                // No test needed — matches anything
            }
            Constraint::MultiVariable(name, _span) => {
                // Ordered sequence plans project this logical field to a
                // multifield value; template selectors already hold a full slot.
                self.translate_variable_constraint(
                    name,
                    slot,
                    constant_tests,
                    variable_slots,
                    seen_variable_slots,
                )?;
            }
            Constraint::Not(inner, span) => match inner.as_ref() {
                Constraint::Literal(lit) => {
                    // ~literal → NotEqual constant test
                    if let Some(key) = self.literal_to_atom_key(&lit.value)? {
                        constant_tests.push(ConstantTest {
                            slot,
                            test_type: ConstantTestType::NotEqual(key),
                        });
                    }
                }
                Constraint::Variable(name, _) | Constraint::MultiVariable(name, _) => {
                    let sym = self.compile_symbol(name)?;
                    if let Some(previous_slot) = seen_variable_slots.get(&sym).copied() {
                        // `?x&~?x` on the same slot is unsatisfiable, and a distinct
                        // previously-bound slot is the normal inequality case.
                        constant_tests.push(ConstantTest {
                            slot,
                            test_type: ConstantTestType::NotEqualSlot(previous_slot),
                        });
                    } else {
                        // ~?x or ~$?x against a previously-bound variable.
                        negated_variable_slots.push((slot, sym, JoinTestType::NotEqual));
                    }
                }
                Constraint::Wildcard(_) | Constraint::MultiWildcard(_) => {
                    // ~? or ~$? — negated wildcard, effectively a no-op
                    // (matches nothing? or everything?) — treat as accept-all
                }
                _ => {
                    return Err(Self::unsupported_constraint(
                        "not",
                        span,
                        "only negated literals (~<literal>) and negated variables (~?var) are supported",
                    ));
                }
            },
            Constraint::And(constraints, _span) => {
                // Process each sub-constraint against the same slot
                for sub in constraints {
                    self.translate_constraint(
                        sub,
                        slot,
                        constant_tests,
                        variable_slots,
                        negated_variable_slots,
                        seen_variable_slots,
                        generated_tests,
                        slot_runtime_vars,
                        internal_slot_var_seed,
                        in_negated_pattern,
                    )?;
                }
            }
            Constraint::Or(constraints, span) => {
                // Try to compile as an EqualAny alpha test (all-literal case).
                // For each sub-constraint, extract the literal value. If any
                // sub-constraint is not a simple literal, fall back to processing
                // each alternative — binding variables via the first variable branch.
                let mut all_literal = true;
                let mut keys = Vec::with_capacity(constraints.len());
                for sub in constraints {
                    if let Constraint::Literal(lit) = sub {
                        if let Some(key) = self.literal_to_atom_key(&lit.value)? {
                            keys.push(key);
                        } else {
                            all_literal = false;
                            break;
                        }
                    } else {
                        all_literal = false;
                        break;
                    }
                }

                if all_literal && !keys.is_empty() {
                    constant_tests.push(ConstantTest {
                        slot,
                        test_type: ConstantTestType::EqualAny(keys),
                    });
                } else {
                    // Mixed or-constraint with variables: bind the first variable
                    // branch and skip others. This is a simplification — full
                    // semantics would require backtracking.
                    let mut found_var = false;
                    for sub in constraints {
                        match sub {
                            Constraint::Variable(name, _) | Constraint::MultiVariable(name, _)
                                if !found_var =>
                            {
                                self.translate_variable_constraint(
                                    name,
                                    slot,
                                    constant_tests,
                                    variable_slots,
                                    seen_variable_slots,
                                )?;
                                found_var = true;
                            }
                            Constraint::Literal(lit) if !found_var => {
                                if let Some(key) = self.literal_to_atom_key(&lit.value)? {
                                    constant_tests.push(ConstantTest {
                                        slot,
                                        test_type: ConstantTestType::Equal(key),
                                    });
                                    found_var = true;
                                }
                            }
                            _ => {}
                        }
                    }
                    if !found_var && constraints.is_empty() {
                        return Err(Self::unsupported_constraint(
                            "or",
                            span,
                            "or constraints require at least one alternative",
                        ));
                    }
                    // If !found_var but constraints is non-empty, all alternatives
                    // are wildcards/predicates — no alpha-level filtering needed.
                    // Predicate constraints have already been absorbed as wildcards
                    // by the parser, so we just accept any value for this slot.
                }
            }
            Constraint::Predicate(expr, _span) => {
                if self.try_lower_simple_predicate_constraint(
                    expr,
                    slot,
                    constant_tests,
                    variable_slots,
                    negated_variable_slots,
                    seen_variable_slots,
                )? {
                    return Ok(());
                }
                let runtime_expr =
                    crate::evaluator::from_sexpr(expr, &mut self.symbol_table, &self.config)
                        .map_err(|e| {
                            LoadError::Compile(format!("predicate constraint translation: {e}"))
                        })?;
                generated_tests.push(runtime_expr);
            }
            Constraint::ReturnValue(expr, _span) => {
                if in_negated_pattern
                    && self.try_lower_negated_return_value_constraint(
                        expr,
                        slot,
                        constant_tests,
                        variable_slots,
                        negated_variable_slots,
                        seen_variable_slots,
                    )?
                {
                    return Ok(());
                }
                let runtime_expr =
                    crate::evaluator::from_sexpr(expr, &mut self.symbol_table, &self.config)
                        .map_err(|e| {
                            LoadError::Compile(format!("return-value constraint translation: {e}"))
                        })?;
                let slot_var_name = self.ensure_slot_runtime_variable(
                    slot,
                    variable_slots,
                    seen_variable_slots,
                    slot_runtime_vars,
                    internal_slot_var_seed,
                )?;
                generated_tests.push(crate::evaluator::RuntimeExpr::Call {
                    name: "eq".to_string(),
                    args: vec![
                        crate::evaluator::RuntimeExpr::BoundVar {
                            name: slot_var_name,
                            span: None,
                        },
                        runtime_expr,
                    ],
                    span: None,
                });
            }
        }
        Ok(())
    }

    fn try_lower_simple_predicate_constraint(
        &mut self,
        expr: &SExpr,
        slot: SlotIndex,
        constant_tests: &mut Vec<ConstantTest>,
        variable_slots: &mut Vec<(SlotIndex, ferric_rules_core::Symbol)>,
        negated_variable_slots: &mut Vec<(SlotIndex, ferric_rules_core::Symbol, JoinTestType)>,
        seen_variable_slots: &mut HashMap<ferric_rules_core::Symbol, SlotIndex>,
    ) -> Result<bool, LoadError> {
        let Some((mut op, left, right)) = Self::parse_simple_predicate_comparison(expr) else {
            let Some((mut lex_op, lex_left, lex_right)) =
                Self::parse_str_compare_predicate_comparison(expr)
            else {
                return Ok(false);
            };

            let left_is_slot = self
                .operand_slot_offset(&lex_left, slot, seen_variable_slots)?
                .is_some();
            let right_is_slot = self
                .operand_slot_offset(&lex_right, slot, seen_variable_slots)?
                .is_some();

            if left_is_slot == right_is_slot {
                return Ok(false);
            }

            let other_operand = if left_is_slot {
                lex_right
            } else {
                lex_op = lex_op.invert();
                lex_left
            };

            return self.lower_simple_slot_lex_comparison(
                slot,
                lex_op,
                &other_operand,
                negated_variable_slots,
                seen_variable_slots,
            );
        };

        let left_slot_offset = self.operand_slot_offset(&left, slot, seen_variable_slots)?;
        let right_slot_offset = self.operand_slot_offset(&right, slot, seen_variable_slots)?;

        let (slot_offset, other_operand) = match (left_slot_offset, right_slot_offset) {
            (Some(left_offset), Some(right_offset)) => {
                let Some(relative_offset) = right_offset.checked_sub(left_offset) else {
                    return Ok(false);
                };
                let always_true = Self::slot_self_comparison_truthiness(op, relative_offset);
                if !always_true {
                    constant_tests.push(ConstantTest {
                        slot,
                        test_type: ConstantTestType::NotEqualSlot(slot),
                    });
                }
                return Ok(true);
            }
            (Some(slot_offset), None) => (slot_offset, right),
            (None, Some(slot_offset)) => {
                op = op.invert();
                (slot_offset, left)
            }
            (None, None) => return Ok(false),
        };

        self.lower_simple_slot_comparison(
            slot,
            op,
            slot_offset,
            &other_operand,
            constant_tests,
            variable_slots,
            negated_variable_slots,
            seen_variable_slots,
        )
    }

    fn lower_simple_slot_lex_comparison(
        &mut self,
        slot: SlotIndex,
        op: SimpleComparisonOp,
        other_operand: &PredicateOperand,
        negated_variable_slots: &mut Vec<(SlotIndex, ferric_rules_core::Symbol, JoinTestType)>,
        seen_variable_slots: &mut HashMap<ferric_rules_core::Symbol, SlotIndex>,
    ) -> Result<bool, LoadError> {
        let PredicateOperand::Variable(name) = other_operand else {
            return Ok(false);
        };

        let sym = self.compile_symbol(name)?;
        if let Some(other_slot) = seen_variable_slots.get(&sym).copied() {
            // Same-slot str-compare reduces to comparing a value with itself.
            if other_slot == slot {
                return Ok(matches!(
                    op,
                    SimpleComparisonOp::Eq | SimpleComparisonOp::Ge | SimpleComparisonOp::Le
                ));
            }
            // Lexeme slot-vs-slot alpha tests are not represented yet.
            return Ok(false);
        }

        negated_variable_slots.push((slot, sym, op.to_lex_join_test()));
        Ok(true)
    }

    #[allow(clippy::too_many_arguments)]
    fn lower_simple_slot_comparison(
        &mut self,
        slot: SlotIndex,
        op: SimpleComparisonOp,
        slot_offset: i64,
        other_operand: &PredicateOperand,
        constant_tests: &mut Vec<ConstantTest>,
        _variable_slots: &mut Vec<(SlotIndex, ferric_rules_core::Symbol)>,
        negated_variable_slots: &mut Vec<(SlotIndex, ferric_rules_core::Symbol, JoinTestType)>,
        seen_variable_slots: &mut HashMap<ferric_rules_core::Symbol, SlotIndex>,
    ) -> Result<bool, LoadError> {
        let Some(normalized_operand) =
            Self::normalize_operand_for_slot_offset(other_operand, slot_offset)
        else {
            return Ok(false);
        };

        match normalized_operand {
            PredicateOperand::Literal(lit) => {
                let Some(key) = self.literal_to_atom_key(&lit)? else {
                    return Ok(false);
                };
                constant_tests.push(ConstantTest {
                    slot,
                    test_type: op.to_constant_test(key),
                });
                Ok(true)
            }
            PredicateOperand::Variable(name) => self.lower_simple_slot_variable_comparison(
                slot,
                op,
                &name,
                0,
                constant_tests,
                negated_variable_slots,
                seen_variable_slots,
            ),
            PredicateOperand::VariableWithOffset { name, offset } => self
                .lower_simple_slot_variable_comparison(
                    slot,
                    op,
                    &name,
                    offset,
                    constant_tests,
                    negated_variable_slots,
                    seen_variable_slots,
                ),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn lower_simple_slot_variable_comparison(
        &mut self,
        slot: SlotIndex,
        op: SimpleComparisonOp,
        variable_name: &str,
        offset: i64,
        constant_tests: &mut Vec<ConstantTest>,
        negated_variable_slots: &mut Vec<(SlotIndex, ferric_rules_core::Symbol, JoinTestType)>,
        seen_variable_slots: &mut HashMap<ferric_rules_core::Symbol, SlotIndex>,
    ) -> Result<bool, LoadError> {
        let sym = self.compile_symbol(variable_name)?;
        if let Some(other_slot) = seen_variable_slots.get(&sym).copied() {
            if other_slot == slot {
                let always_true = Self::slot_self_comparison_truthiness(op, offset);
                if !always_true {
                    // Unsatisfiable comparison: force pattern mismatch.
                    constant_tests.push(ConstantTest {
                        slot,
                        test_type: ConstantTestType::NotEqualSlot(slot),
                    });
                }
                return Ok(true);
            }

            constant_tests.push(ConstantTest {
                slot,
                test_type: op.to_slot_offset_test(other_slot, offset),
            });
            return Ok(true);
        }

        if offset == 0 {
            negated_variable_slots.push((slot, sym, op.to_join_test()));
        } else {
            negated_variable_slots.push((slot, sym, op.to_join_test_with_offset(offset)));
        }

        Ok(true)
    }

    #[allow(clippy::cast_precision_loss)]
    fn normalize_operand_for_slot_offset(
        operand: &PredicateOperand,
        slot_offset: i64,
    ) -> Option<PredicateOperand> {
        if slot_offset == 0 {
            return Some(operand.clone());
        }

        match operand {
            PredicateOperand::Literal(LiteralKind::Integer(value)) => value
                .checked_sub(slot_offset)
                .map(|adjusted| PredicateOperand::Literal(LiteralKind::Integer(adjusted))),
            PredicateOperand::Literal(LiteralKind::Float(value)) => Some(
                PredicateOperand::Literal(LiteralKind::Float(*value - slot_offset as f64)),
            ),
            PredicateOperand::Literal(_) => None,
            PredicateOperand::Variable(name) => {
                let adjusted = 0_i64.checked_sub(slot_offset)?;
                Some(Self::variable_with_offset_operand(name.clone(), adjusted))
            }
            PredicateOperand::VariableWithOffset { name, offset } => {
                let adjusted = offset.checked_sub(slot_offset)?;
                Some(Self::variable_with_offset_operand(name.clone(), adjusted))
            }
        }
    }

    fn try_lower_negated_return_value_constraint(
        &mut self,
        expr: &SExpr,
        slot: SlotIndex,
        constant_tests: &mut Vec<ConstantTest>,
        _variable_slots: &mut Vec<(SlotIndex, ferric_rules_core::Symbol)>,
        negated_variable_slots: &mut Vec<(SlotIndex, ferric_rules_core::Symbol, JoinTestType)>,
        seen_variable_slots: &mut HashMap<ferric_rules_core::Symbol, SlotIndex>,
    ) -> Result<bool, LoadError> {
        let Some(operand) = Self::parse_predicate_operand(expr) else {
            return Ok(false);
        };

        match operand {
            PredicateOperand::Literal(lit) => {
                let Some(key) = self.literal_to_atom_key(&lit)? else {
                    return Ok(false);
                };
                constant_tests.push(ConstantTest {
                    slot,
                    test_type: ConstantTestType::Equal(key),
                });
                Ok(true)
            }
            PredicateOperand::Variable(name) => {
                let sym = self.compile_symbol(&name)?;
                if let Some(other_slot) = seen_variable_slots.get(&sym).copied() {
                    if other_slot != slot {
                        constant_tests.push(ConstantTest {
                            slot,
                            test_type: ConstantTestType::EqualSlot(other_slot),
                        });
                    }
                } else {
                    negated_variable_slots.push((slot, sym, JoinTestType::Equal));
                }
                Ok(true)
            }
            PredicateOperand::VariableWithOffset { name, offset } => {
                let sym = self.compile_symbol(&name)?;
                if let Some(other_slot) = seen_variable_slots.get(&sym).copied() {
                    if other_slot == slot {
                        if offset != 0 {
                            // Unsatisfiable: ?x = ?x + k where k != 0.
                            constant_tests.push(ConstantTest {
                                slot,
                                test_type: ConstantTestType::NotEqualSlot(slot),
                            });
                        }
                    } else {
                        constant_tests.push(ConstantTest {
                            slot,
                            test_type: ConstantTestType::EqualSlotOffset(other_slot, offset),
                        });
                    }
                } else {
                    negated_variable_slots.push((slot, sym, JoinTestType::EqualOffset(offset)));
                }
                Ok(true)
            }
        }
    }

    fn operand_slot_offset(
        &mut self,
        operand: &PredicateOperand,
        slot: SlotIndex,
        seen_variable_slots: &HashMap<ferric_rules_core::Symbol, SlotIndex>,
    ) -> Result<Option<i64>, LoadError> {
        let (name, offset) = match operand {
            PredicateOperand::Variable(name) => (name, 0),
            PredicateOperand::VariableWithOffset { name, offset } => (name, *offset),
            PredicateOperand::Literal(_) => return Ok(None),
        };
        let sym = self.compile_symbol(name)?;
        Ok(match seen_variable_slots.get(&sym) {
            Some(existing) if *existing == slot => Some(offset),
            _ => None,
        })
    }

    fn slot_self_comparison_truthiness(op: SimpleComparisonOp, offset: i64) -> bool {
        let ordering = match offset.cmp(&0) {
            std::cmp::Ordering::Equal => std::cmp::Ordering::Equal,
            std::cmp::Ordering::Greater => std::cmp::Ordering::Less,
            std::cmp::Ordering::Less => std::cmp::Ordering::Greater,
        };

        match op {
            SimpleComparisonOp::Eq => ordering == std::cmp::Ordering::Equal,
            SimpleComparisonOp::Ne => ordering != std::cmp::Ordering::Equal,
            SimpleComparisonOp::Gt => ordering == std::cmp::Ordering::Greater,
            SimpleComparisonOp::Lt => ordering == std::cmp::Ordering::Less,
            SimpleComparisonOp::Ge => {
                ordering == std::cmp::Ordering::Greater || ordering == std::cmp::Ordering::Equal
            }
            SimpleComparisonOp::Le => {
                ordering == std::cmp::Ordering::Less || ordering == std::cmp::Ordering::Equal
            }
        }
    }

    fn parse_simple_predicate_comparison(
        expr: &SExpr,
    ) -> Option<(SimpleComparisonOp, PredicateOperand, PredicateOperand)> {
        let items = expr.as_list()?;
        if items.len() != 3 {
            return None;
        }
        let op_sym = items[0].as_symbol()?;
        let op = Self::parse_simple_comparison_op(op_sym)?;
        let left = Self::parse_predicate_operand(&items[1])?;
        let right = Self::parse_predicate_operand(&items[2])?;
        Some((op, left, right))
    }

    fn parse_str_compare_predicate_comparison(
        expr: &SExpr,
    ) -> Option<(SimpleComparisonOp, PredicateOperand, PredicateOperand)> {
        let items = expr.as_list()?;
        if items.len() != 3 {
            return None;
        }

        let op_sym = items[0].as_symbol()?;
        let op = Self::parse_simple_comparison_op(op_sym)?;

        if let Some((left, right)) = Self::parse_str_compare_call(&items[1]) {
            if Self::sexpr_is_numeric_zero(&items[2]) {
                return Some((op, left, right));
            }
        }

        if let Some((left, right)) = Self::parse_str_compare_call(&items[2]) {
            if Self::sexpr_is_numeric_zero(&items[1]) {
                return Some((op.invert(), left, right));
            }
        }

        None
    }

    fn parse_str_compare_call(expr: &SExpr) -> Option<(PredicateOperand, PredicateOperand)> {
        let items = expr.as_list()?;
        if items.len() != 3 {
            return None;
        }
        if items[0].as_symbol()? != "str-compare" {
            return None;
        }

        let left = Self::parse_predicate_operand(&items[1])?;
        let right = Self::parse_predicate_operand(&items[2])?;

        // Only plain variables/literals are supported for this lowering.
        match (&left, &right) {
            (PredicateOperand::VariableWithOffset { .. }, _)
            | (_, PredicateOperand::VariableWithOffset { .. }) => None,
            _ => Some((left, right)),
        }
    }

    fn sexpr_is_numeric_zero(expr: &SExpr) -> bool {
        match expr.as_atom() {
            Some(Atom::Integer(n)) => *n == 0,
            Some(Atom::Float(f)) => *f == 0.0,
            _ => false,
        }
    }

    fn parse_simple_comparison_op(symbol: &str) -> Option<SimpleComparisonOp> {
        match symbol {
            "=" | "eq" => Some(SimpleComparisonOp::Eq),
            "!=" | "<>" | "neq" => Some(SimpleComparisonOp::Ne),
            ">" => Some(SimpleComparisonOp::Gt),
            "<" => Some(SimpleComparisonOp::Lt),
            ">=" => Some(SimpleComparisonOp::Ge),
            "<=" => Some(SimpleComparisonOp::Le),
            _ => None,
        }
    }

    fn parse_predicate_operand(expr: &SExpr) -> Option<PredicateOperand> {
        if let Some(atom) = expr.as_atom() {
            return match atom {
                Atom::Integer(n) => Some(PredicateOperand::Literal(LiteralKind::Integer(*n))),
                Atom::Float(f) => Some(PredicateOperand::Literal(LiteralKind::Float(*f))),
                Atom::String(s) => Some(PredicateOperand::Literal(LiteralKind::String(s.clone()))),
                Atom::Symbol(s) => Some(PredicateOperand::Literal(LiteralKind::Symbol(s.clone()))),
                Atom::InstanceName(s) => Some(PredicateOperand::Literal(
                    LiteralKind::InstanceName(s.clone()),
                )),
                Atom::SingleVar(name) | Atom::MultiVar(name) => {
                    Some(PredicateOperand::Variable(name.clone()))
                }
                Atom::GlobalVar(_) | Atom::Connective(_) => None,
            };
        }

        let linear = Self::parse_linear_integer_expression(expr)?;
        match linear.coefficient {
            0 => Some(PredicateOperand::Literal(LiteralKind::Integer(
                linear.offset,
            ))),
            1 => Some(Self::variable_with_offset_operand(
                linear.variable?,
                linear.offset,
            )),
            _ => None,
        }
    }

    fn parse_linear_integer_expression(expr: &SExpr) -> Option<LinearIntegerExpr> {
        if let Some(atom) = expr.as_atom() {
            return match atom {
                Atom::Integer(value) => Some(LinearIntegerExpr::integer(*value)),
                Atom::SingleVar(name) | Atom::MultiVar(name) => {
                    Some(LinearIntegerExpr::variable(name.clone()))
                }
                _ => None,
            };
        }

        let items = expr.as_list()?;
        if items.len() < 2 {
            return None;
        }

        match items[0].as_symbol()? {
            "+" => {
                let mut terms = items.iter().skip(1);
                let first = terms.next()?;
                let mut acc = Self::parse_linear_integer_expression(first)?;
                for term in terms {
                    let rhs = Self::parse_linear_integer_expression(term)?;
                    acc = acc.add(&rhs)?;
                }
                Some(acc)
            }
            "-" => {
                let mut terms = items.iter().skip(1);
                let first = terms.next()?;
                let mut acc = Self::parse_linear_integer_expression(first)?;
                if items.len() == 2 {
                    return acc.negate();
                }

                for term in terms {
                    let rhs = Self::parse_linear_integer_expression(term)?;
                    acc = acc.sub(&rhs)?;
                }
                Some(acc)
            }
            _ => None,
        }
    }

    fn variable_with_offset_operand(name: String, offset: i64) -> PredicateOperand {
        if offset == 0 {
            PredicateOperand::Variable(name)
        } else {
            PredicateOperand::VariableWithOffset { name, offset }
        }
    }

    fn translate_variable_constraint(
        &mut self,
        name: &str,
        slot: SlotIndex,
        constant_tests: &mut Vec<ConstantTest>,
        variable_slots: &mut Vec<(SlotIndex, ferric_rules_core::Symbol)>,
        seen_variable_slots: &mut HashMap<ferric_rules_core::Symbol, SlotIndex>,
    ) -> Result<(), LoadError> {
        let sym = self.compile_symbol(name)?;
        if let Some(previous_slot) = seen_variable_slots.get(&sym).copied() {
            if previous_slot != slot {
                constant_tests.push(ConstantTest {
                    slot,
                    test_type: ConstantTestType::EqualSlot(previous_slot),
                });
            }
        } else {
            seen_variable_slots.insert(sym, slot);
            variable_slots.push((slot, sym));
        }
        Ok(())
    }

    fn ensure_slot_runtime_variable(
        &mut self,
        slot: SlotIndex,
        variable_slots: &mut Vec<(SlotIndex, ferric_rules_core::Symbol)>,
        seen_variable_slots: &mut HashMap<ferric_rules_core::Symbol, SlotIndex>,
        slot_runtime_vars: &mut HashMap<SlotIndex, String>,
        internal_slot_var_seed: &mut usize,
    ) -> Result<String, LoadError> {
        if let Some(existing) = slot_runtime_vars.get(&slot) {
            return Ok(existing.clone());
        }

        loop {
            let candidate = match slot {
                SlotIndex::Ordered(index) => {
                    format!("__ferric_slot_ord_{index}_{internal_slot_var_seed}")
                }
                SlotIndex::Template(index) => {
                    format!("__ferric_slot_tpl_{index}_{internal_slot_var_seed}")
                }
            };
            *internal_slot_var_seed += 1;

            let sym = self.compile_symbol(&candidate)?;
            if seen_variable_slots.contains_key(&sym) {
                continue;
            }

            seen_variable_slots.insert(sym, slot);
            variable_slots.push((slot, sym));
            slot_runtime_vars.insert(slot, candidate.clone());
            return Ok(candidate);
        }
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
            LiteralKind::InstanceName(s) => Ok(Some(AtomKey::InstanceName(
                ferric_rules_core::InstanceName::from_symbol(self.compile_symbol(s)?),
            ))),
            LiteralKind::String(s) => {
                let fs = self.compile_string(s)?;
                Ok(Some(AtomKey::String(fs)))
            }
        }
    }

    fn compile_encoding_error(error: impl std::fmt::Display) -> LoadError {
        LoadError::Compile(format!("encoding error: {error}"))
    }

    fn compile_symbol(&mut self, symbol: &str) -> Result<ferric_rules_core::Symbol, LoadError> {
        self.symbol_table
            .intern_symbol(symbol, self.config.string_encoding)
            .map_err(Self::compile_encoding_error)
    }

    fn compile_string(&self, value: &str) -> Result<FerricString, LoadError> {
        FerricString::new(value, self.config.string_encoding).map_err(Self::compile_encoding_error)
    }

    fn duplicate_definition_error(
        construct: &str,
        name: &str,
        span: &ferric_rules_parser::Span,
    ) -> LoadError {
        LoadError::Compile(format!(
            "duplicate {construct} `{name}` at line {}, column {}",
            span.start.line, span.start.column
        ))
    }

    fn construct_conflict_error(
        new_construct: &str,
        existing_construct: &str,
        name: &str,
        span: &ferric_rules_parser::Span,
    ) -> LoadError {
        LoadError::Compile(format!(
            "cannot define {new_construct} `{name}`: a {existing_construct} with the same name already exists at line {}, column {}",
            span.start.line, span.start.column
        ))
    }

    fn duplicate_method_index_error(
        generic_name: &str,
        index: i32,
        span: &ferric_rules_parser::Span,
    ) -> LoadError {
        LoadError::Compile(format!(
            "duplicate defmethod index {index} for `{generic_name}` at line {}, column {}",
            span.start.line, span.start.column
        ))
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

    fn compile_error_at(span: &ferric_rules_parser::Span, detail: &str) -> LoadError {
        LoadError::Compile(format!(
            "{detail} at line {}, column {}",
            span.start.line, span.start.column
        ))
    }

    fn unsupported_pattern(
        kind: &str,
        span: &ferric_rules_parser::Span,
        detail: &str,
    ) -> LoadError {
        Self::unsupported_compile_form("pattern", kind, span, detail)
    }

    fn unsupported_constraint(
        kind: &str,
        span: &ferric_rules_parser::Span,
        detail: &str,
    ) -> LoadError {
        Self::unsupported_compile_form("constraint", kind, span, detail)
    }

    fn unsupported_compile_form(
        category: &str,
        kind: &str,
        span: &ferric_rules_parser::Span,
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
) -> Vec<ferric_rules_core::PatternValidationError> {
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
    errors: &mut Vec<ferric_rules_core::PatternValidationError>,
) {
    match pattern {
        Pattern::Not(inner, span) => {
            let new_depth = depth + 1;
            if new_depth > max_depth {
                push_nesting_depth_error(
                    errors,
                    span,
                    new_depth,
                    max_depth,
                    ferric_rules_core::ValidationStage::ReteCompilation,
                );
            }
            // Continue validating the inner pattern regardless of depth violation
            validate_pattern_recursive(inner, new_depth, max_depth, errors);
        }

        Pattern::Exists(inner_patterns, span) => {
            let new_depth = depth + 1;
            if new_depth > max_depth {
                push_nesting_depth_error(
                    errors,
                    span,
                    new_depth,
                    max_depth,
                    ferric_rules_core::ValidationStage::ReteCompilation,
                );
            }

            // Check for unsupported combination: single-pattern exists containing not.
            // Multi-pattern exists groups compile as a tuple subnetwork, where
            // mixed branches like `(exists A (not B))` are supported.
            let enforce_exists_not_guard = inner_patterns.len() == 1;
            for inner in inner_patterns {
                if enforce_exists_not_guard && matches!(inner, Pattern::Not(..)) {
                    let kind = ferric_rules_core::PatternViolation::UnsupportedNestingCombination {
                        description: "exists containing not is not supported".to_string(),
                    };
                    let location = Some(span_to_source_location(span));
                    let error = ferric_rules_core::PatternValidationError::new(
                        kind,
                        location,
                        ferric_rules_core::ValidationStage::ReteCompilation,
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

        Pattern::And(inner_patterns, _)
        | Pattern::Logical(inner_patterns, _)
        | Pattern::Or(inner_patterns, _) => {
            for inner in inner_patterns {
                validate_pattern_recursive(inner, depth, max_depth, errors);
            }
        }

        Pattern::Forall(sub_patterns, span) => {
            let new_depth = depth + 1;
            if new_depth > max_depth {
                push_nesting_depth_error(
                    errors,
                    span,
                    new_depth,
                    max_depth,
                    ferric_rules_core::ValidationStage::AstInterpretation,
                );
            }
            for sub in sub_patterns {
                validate_pattern_recursive(sub, new_depth, max_depth, errors);
            }
        }

        Pattern::Ordered(..) | Pattern::Template(..) | Pattern::Test(..) => {
            // Leaf patterns - nothing to validate at this level
        }
    }
}

fn push_nesting_depth_error(
    errors: &mut Vec<ferric_rules_core::PatternValidationError>,
    span: &ferric_rules_parser::Span,
    depth: usize,
    max: usize,
    stage: ferric_rules_core::ValidationStage,
) {
    let error = ferric_rules_core::PatternValidationError::new(
        ferric_rules_core::PatternViolation::NestingTooDeep { depth, max },
        Some(span_to_source_location(span)),
        stage,
    );
    errors.push(error);
}

/// Convert a parser `Span` to a core `SourceLocation`.
fn span_to_source_location(span: &ferric_rules_parser::Span) -> ferric_rules_core::SourceLocation {
    ferric_rules_core::SourceLocation::new(
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
    use crate::test_helpers::{
        find_facts_by_relation, load_err, load_ok, new_utf8_engine, run_to_completion,
    };
    use ferric_rules_core::{Fact, Value};
    use std::collections::HashMap;

    #[test]
    fn template_resolution_preserves_visibility_diagnostics_and_local_preference() {
        let mut engine = Engine::with_rules(
            "(defmodule Z (export ?ALL)) (deftemplate Z::item (slot z))
             (defmodule A (export ?ALL)) (deftemplate A::item (slot a))
             (defmodule BOTH (import Z ?ALL) (import A ?ALL))
             (defmodule ONE (import A ?ALL))
             (defmodule NONE)",
        )
        .unwrap();
        let a = engine.module_registry.get_by_name("A").unwrap();
        let both = engine.module_registry.get_by_name("BOTH").unwrap();
        let one = engine.module_registry.get_by_name("ONE").unwrap();
        let none = engine.module_registry.get_by_name("NONE").unwrap();
        let a_item = engine.resolve_template_reference("item", a).unwrap();
        assert_eq!(engine.resolve_template_reference("item", one), Ok(a_item));
        assert_eq!(
            engine.resolve_template_reference("A::item", both),
            Ok(a_item)
        );
        assert_eq!(
            engine.resolve_template_reference("item", both).unwrap_err(),
            "template `item` is ambiguous from module `BOTH` (matches modules: A, Z)"
        );
        assert_eq!(
            engine.resolve_template_reference("item", none).unwrap_err(),
            "template `item` is not visible from module `NONE`"
        );
        assert_eq!(
            engine
                .resolve_template_reference("A::item", none)
                .unwrap_err(),
            "template `A::item` is not visible from module `NONE`"
        );
        for name in [
            "missing",
            "MISSING::item",
            "A::missing",
            "A::",
            "::item",
            "A::B::item",
        ] {
            assert_eq!(
                engine.resolve_template_reference(name, both).unwrap_err(),
                format!("unknown template `{name}`")
            );
        }
        engine.load_str("(defmodule NONE (import A ?ALL))").unwrap();
        assert_eq!(engine.resolve_template_reference("item", none), Ok(a_item));
        engine.load_str("(defmodule NONE)").unwrap();
        assert_eq!(
            engine.resolve_template_reference("item", none).unwrap_err(),
            "template `item` is not visible from module `NONE`"
        );
        engine
            .load_str("(deftemplate BOTH::item (slot local))")
            .unwrap();
        let local = engine.resolve_template_reference("item", both).unwrap();
        assert_ne!(local, a_item);
        engine
            .load_str("(deftemplate BOTH::item (slot replacement))")
            .unwrap();
        assert_eq!(engine.resolve_template_reference("item", both), Ok(local));
        engine.reset().unwrap();
        assert_eq!(engine.resolve_template_reference("item", both), Ok(local));
        engine.clear();
        let main = engine.module_registry.main_module_id();
        assert!(engine.resolve_template_reference("item", main).is_err());
        engine.load_str("(deftemplate item (slot fresh))").unwrap();
        assert!(engine.resolve_template_reference("item", main).is_ok());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn restored_template_resolution_rebuilds_candidates_in_every_format() {
        use crate::serialization::SerializationFormat;
        let engine = Engine::with_rules(
            "(defmodule A (export ?ALL)) (deftemplate A::item (slot a))
             (defmodule B (export ?ALL)) (deftemplate B::item (slot b))
             (defmodule APP (import A ?ALL) (import B ?ALL))",
        )
        .unwrap();
        for &format in SerializationFormat::ALL {
            let mut restored =
                Engine::deserialize(&engine.serialize(format).unwrap(), format).unwrap();
            for module in ["A", "B", "APP", "MAIN"] {
                let module = restored.module_registry.get_by_name(module).unwrap();
                for name in ["item", "A::item", "B::item", "missing"] {
                    assert_eq!(
                        restored.resolve_template_reference(name, module),
                        engine.resolve_template_reference(name, module)
                    );
                }
            }
            restored
                .load_str("(deftemplate APP::item (slot c))")
                .unwrap();
            let app = restored.module_registry.get_by_name("APP").unwrap();
            assert!(restored.resolve_template_reference("item", app).is_ok());
        }
    }

    fn test_span(line: u32, column: u32) -> ferric_rules_parser::Span {
        let pos = ferric_rules_parser::Position {
            offset: 0,
            line,
            column,
        };
        ferric_rules_parser::Span::new(pos, pos, FileId(0))
    }

    fn parser_depth_nested_list_source(depth: usize) -> String {
        let mut source = String::with_capacity(depth.saturating_mul(2).saturating_add(1));
        source.extend(std::iter::repeat('(').take(depth));
        source.push('x');
        source.extend(std::iter::repeat(')').take(depth));
        source
    }

    fn parser_depth_action_rule(action_depth: usize) -> String {
        let mut source = String::from("(defrule depth-action (trigger) => ");
        for _ in 0..action_depth {
            source.push_str("(+ 1 ");
        }
        source.push('1');
        source.extend(std::iter::repeat(')').take(action_depth));
        source.push(')');
        source
    }

    fn parser_depth_conditional_rule(not_depth: usize) -> String {
        let mut source = String::from("(defrule depth-ce ");
        for _ in 0..not_depth {
            source.push_str("(not ");
        }
        source.push_str("(leaf)");
        source.extend(std::iter::repeat(')').take(not_depth));
        source.push_str(" => (assert (ok)))");
        source
    }

    #[test]
    fn action_translation_accepts_its_bound_and_rejects_deeper_parser_valid_input() {
        let action_depth = 15;
        let mut engine = new_utf8_engine();

        let result = engine.load_str(&parser_depth_action_rule(action_depth));

        assert!(
            result.is_ok(),
            "an action within the translation bound must load: {result:?}"
        );
        assert_eq!(engine.rules().len(), 1);
        let errors = engine
            .load_str(&parser_depth_action_rule(
                ferric_rules_parser::MAX_SEXPR_NESTING_DEPTH - 1,
            ))
            .unwrap_err();
        assert!(errors
            .iter()
            .any(|error| error.to_string().contains("expression nesting limit")));
        // Failed replacement preserves the earlier, executable rule.
        engine.assert_ordered("trigger", ()).unwrap();
        assert_eq!(
            engine.run(crate::RunLimit::Unlimited).unwrap().rules_fired,
            1
        );
    }

    #[test]
    fn parser_depth_rejects_maximum_plus_one_actions_and_conditional_elements() {
        let rejected_action =
            parser_depth_action_rule(ferric_rules_parser::MAX_SEXPR_NESTING_DEPTH);
        let rejected_ce =
            parser_depth_conditional_rule(ferric_rules_parser::MAX_SEXPR_NESTING_DEPTH);

        for source in [rejected_action, rejected_ce] {
            let mut engine = new_utf8_engine();
            let errors = engine
                .load_str(&source)
                .expect_err("maximum plus one must be rejected");
            assert_eq!(errors.len(), 1, "diagnostics must remain bounded");
            assert!(matches!(
                &errors[0],
                LoadError::Parse(error)
                    if error.kind == ferric_rules_parser::ParseErrorKind::NestingDepthExceeded
                        && error.message
                            == format!(
                                "S-expression nesting depth {} exceeds maximum of {}",
                                ferric_rules_parser::MAX_SEXPR_NESTING_DEPTH + 1,
                                ferric_rules_parser::MAX_SEXPR_NESTING_DEPTH
                            )
            ));
        }
    }

    #[test]
    fn parser_depth_extreme_is_bounded_in_small_stack_subprocess() {
        const CHILD_ENV: &str = "FERRIC_ROBUST_001_RUNTIME_SMALL_STACK_CHILD";
        const TEST_NAME: &str =
            "loader::tests::parser_depth_extreme_is_bounded_in_small_stack_subprocess";

        if std::env::var_os(CHILD_ENV).is_some() {
            std::thread::Builder::new()
                .name("fr-robust-001-runtime-stack".to_string())
                .stack_size(64 * 1024)
                .spawn(|| {
                    let mut engine = new_utf8_engine();
                    let errors = engine
                        .load_str(&parser_depth_nested_list_source(50_000))
                        .expect_err("extreme nesting must be rejected");
                    assert_eq!(errors.len(), 1);
                    assert!(matches!(
                        &errors[0],
                        LoadError::Parse(error)
                            if error.kind
                                == ferric_rules_parser::ParseErrorKind::NestingDepthExceeded
                    ));
                })
                .expect("small-stack runtime thread must start")
                .join()
                .expect("small-stack runtime load must not panic");
            return;
        }

        let status =
            std::process::Command::new(std::env::current_exe().expect("current test executable"))
                .args(["--exact", TEST_NAME, "--nocapture"])
                .env(CHILD_ENV, "1")
                .status()
                .expect("isolated runtime test subprocess must start");

        assert!(
            status.success(),
            "extreme runtime input must not abort the isolated subprocess: {status}"
        );
    }

    #[test]
    fn load_empty_string_returns_empty_result() {
        let mut engine = new_utf8_engine();
        let result = load_ok(&mut engine, "");
        assert!(result.asserted_facts.is_empty());
        assert!(result.rules.is_empty());
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn load_single_assert_form() {
        let mut engine = new_utf8_engine();
        let result = load_ok(&mut engine, "(assert (person John 30))");

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
        let mut engine = new_utf8_engine();
        let source = r"
            (assert (person Alice 25))
            (assert (person Bob 30))
            (assert (person Carol 35))
        ";
        let result = load_ok(&mut engine, source);

        assert_eq!(result.asserted_facts.len(), 3);
        assert!(result.rules.is_empty());
    }

    #[test]
    fn load_assert_with_multiple_facts() {
        let mut engine = new_utf8_engine();
        let result = engine
            .load_str("(assert (person Alice) (person Bob))")
            .unwrap();

        assert_eq!(result.asserted_facts.len(), 2);
    }

    #[test]
    fn load_assert_with_various_value_types() {
        let mut engine = new_utf8_engine();
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
            assert!(
                matches!(&ordered.fields[2], Value::String(s) if s.as_str().unwrap() == "hello")
            );
            assert!(matches!(&ordered.fields[3], Value::Symbol(_)));
        } else {
            panic!("expected ordered fact");
        }
    }

    #[test]
    fn load_simple_defrule() {
        let mut engine = new_utf8_engine();
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
    fn load_rule_with_test_pattern_compiles_successfully() {
        let mut engine = new_utf8_engine();
        let result = engine
            .load_str("(defrule t (value ?x) (test (> ?x 0)) => (assert (ok)))")
            .unwrap();
        assert_eq!(result.rules.len(), 1);
    }

    #[test]
    fn load_empty_lhs_rule_compiles_with_implicit_initial_fact() {
        let mut engine = new_utf8_engine();
        let result = engine
            .load_str("(defrule empty-rule => (assert (fired)))")
            .unwrap();
        assert_eq!(result.rules.len(), 1);
    }

    #[test]
    fn load_test_only_rule_compiles_with_implicit_initial_fact() {
        let mut engine = new_utf8_engine();
        let result = engine
            .load_str("(defrule test-only (test (> 5 3)) => (assert (ok)))")
            .unwrap();
        assert_eq!(result.rules.len(), 1);
    }

    #[test]
    fn empty_lhs_rule_fires_after_reset() {
        let mut engine = new_utf8_engine();
        engine
            .load_str("(defrule empty-rule => (assert (fired)))")
            .unwrap();
        engine.reset().unwrap();
        let result = engine.run(crate::execution::RunLimit::Unlimited).unwrap();
        assert!(
            result.rules_fired > 0,
            "empty-LHS rule should fire after reset, fired: {}",
            result.rules_fired
        );
    }

    #[test]
    fn load_rule_with_toplevel_and_ce_compiles() {
        let mut engine = new_utf8_engine();
        let result = engine
            .load_str("(defrule test (and (data ?x) (info ?y)) => (assert (combined ?x ?y)))")
            .unwrap();
        assert_eq!(result.rules.len(), 1);
    }

    #[test]
    fn load_rule_with_and_containing_test_compiles() {
        let mut engine = new_utf8_engine();
        let result = engine
            .load_str("(defrule test (and (value ?x) (test (> ?x 0))) => (assert (ok)))")
            .unwrap();
        assert_eq!(result.rules.len(), 1);
    }

    #[test]
    fn load_rule_with_template_pattern_compiles_with_defined_template() {
        let mut engine = new_utf8_engine();
        let result = engine
            .load_str(
                r"
                (deftemplate person (slot name))
                (defrule t (person (name Alice)) => (assert (ok)))
            ",
            )
            .unwrap();
        assert_eq!(result.rules.len(), 1);
    }

    #[test]
    fn load_rule_with_undefined_template_pattern_returns_error() {
        let mut engine = new_utf8_engine();
        let errors = engine
            .load_str("(defrule t (person (name Alice)) => (assert (ok)))")
            .unwrap_err();

        assert_eq!(
            errors.len(),
            1,
            "expected exactly one error, got {errors:?}"
        );
        match &errors[0] {
            LoadError::Compile(msg) => assert!(
                msg.contains("unknown template"),
                "expected 'unknown template' in error message, got: `{msg}`"
            ),
            other => panic!("expected compile error, got {other:?}"),
        }
    }

    #[test]
    fn load_rule_with_multi_pattern_exists_compiles() {
        let mut engine = new_utf8_engine();
        let result = engine.load_str("(defrule t (exists (a) (b)) => (assert (ok)))");

        assert!(
            result.is_ok(),
            "multi-pattern exists should compile: {result:?}"
        );
    }

    #[test]
    fn load_rule_with_template_assert_slot_names_not_treated_as_callables() {
        let mut engine = new_utf8_engine();
        let result = engine.load_str(
            r"
            (deftemplate example (slot value))
            (defrule t
              =>
              (assert (example (value (eq 1 1)))))
            ",
        );

        assert!(
            result.is_ok(),
            "template assert slot names should not require function declarations: {result:?}"
        );
        engine.reset().unwrap();
        let run = engine.run(crate::RunLimit::Unlimited).unwrap();
        assert_eq!(run.rules_fired, 1);
        assert!(engine.action_diagnostics().is_empty());
        let (fact_id, _) = engine
            .facts()
            .unwrap()
            .find(|(_, fact)| matches!(fact, Fact::Template(_)))
            .expect("RHS assert must create a template fact");
        let Value::Symbol(value) = engine.get_fact_slot_by_name(fact_id, "value").unwrap() else {
            panic!("eq must produce a symbol value");
        };
        assert_eq!(engine.resolve_core_symbol(*value), Some("TRUE"));
    }

    #[test]
    fn load_rule_with_ordered_assert_still_validates_function_calls() {
        let mut engine = new_utf8_engine();
        let errors = engine
            .load_str(
                r"
                (defrule t
                  =>
                  (assert (foo (nonexistent-fn))))
                ",
            )
            .unwrap_err();

        let has_missing_decl = errors.iter().any(
            |e| matches!(e, LoadError::Compile(msg) if msg.contains("[EXPRNPSR3]") && msg.contains("nonexistent-fn")),
        );
        assert!(
            has_missing_decl,
            "expected missing function declaration error, got: {errors:?}"
        );
    }

    #[test]
    fn load_rule_with_nested_multi_pattern_exists_compiles() {
        let mut engine = new_utf8_engine();
        let result =
            engine.load_str("(defrule t (exists (b) (exists (h) (i) (j)) (k)) => (assert (ok)))");

        assert!(
            result.is_ok(),
            "nested multi-pattern exists should compile: {result:?}"
        );
    }

    #[test]
    fn load_rule_with_multi_pattern_exists_including_not_compiles() {
        let mut engine = new_utf8_engine();
        let result = engine.load_str(
            r"
            (defrule t
              (a)
              (exists
                (b)
                (not (and (c) (d))))
              =>)
            ",
        );

        assert!(
            result.is_ok(),
            "multi-pattern exists including not should compile: {result:?}"
        );
    }

    #[test]
    fn load_rule_with_distributed_or_and_nested_exists_compiles() {
        let mut engine = new_utf8_engine();
        let result = engine.load_str(
            r"
            (defrule t
              (exists
                (or
                  (and
                    (exists (a) (b) (c))
                    (test (eq 1 1)))
                  (and
                    (exists (d) (e) (f)))))
              =>)
            ",
        );

        assert!(
            result.is_ok(),
            "exists(or(...)) with nested multi-pattern exists should compile: {result:?}"
        );
    }

    #[test]
    fn load_rule_with_not_and_compiles() {
        let mut engine = new_utf8_engine();
        let result = engine.load_str(
            "(defrule t (item ?x) (not (and (block ?x) (reason ?x))) => (assert (ok ?x)))",
        );

        assert!(result.is_ok(), "not(and ...) should compile in Phase 2");
    }

    #[test]
    fn load_rule_with_multivariable_constraint_compiles() {
        let mut engine = new_utf8_engine();
        let result = engine.load_str("(defrule t (items $?values) => (assert (ok)))");

        assert!(
            result.is_ok(),
            "$?var in slot constraint should compile: {result:?}"
        );
    }

    #[test]
    fn load_rule_with_connected_slot_variables_compiles() {
        let mut engine = new_utf8_engine();
        let result = engine.load_str("(defrule t (seed ?x) ?f <- (x ?y&?x) => (retract ?f))");

        assert!(
            result.is_ok(),
            "connected variables in one slot should compile: {result:?}"
        );
    }

    #[test]
    fn load_rule_with_intra_pattern_slot_variable_reuse_compiles() {
        let mut engine = new_utf8_engine();
        let result = engine.load_str(
            r"
            (deftemplate foo (slot x) (slot y))
            (defrule t (foo (x ?x) (y ?x)) => (assert (ok ?x)))
            ",
        );

        assert!(
            result.is_ok(),
            "same variable across slots in one pattern should compile: {result:?}"
        );
    }

    #[test]
    fn intra_pattern_slot_variable_reuse_enforces_equality() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (deftemplate foo (slot x) (slot y))
            (deffacts startup
                (foo (x 1) (y 1))
                (foo (x 1) (y 2)))
            (defrule t
                (foo (x ?x) (y ?x))
                =>
                (assert (matched ?x)))
            ",
        );
        engine.reset().unwrap();

        let run = run_to_completion(&mut engine);
        assert_eq!(
            run.rules_fired, 1,
            "only the (x==y) fact should satisfy the pattern"
        );

        let matched = find_facts_by_relation(&engine, "matched");
        assert_eq!(matched.len(), 1, "expected exactly one matched fact");
    }

    #[test]
    fn load_or_constraints_with_reused_variables_across_slots_compiles() {
        let mut engine = new_utf8_engine();
        let result = engine.load_str(
            r"
            (deftemplate mnj (slot x) (slot y))
            (defrule t
              (seed ?x ?y)
              (mnj (x ?x | ?y) (y ?x | ?y))
              =>)
            ",
        );

        assert!(
            result.is_ok(),
            "reused vars inside mixed or-constraints should compile: {result:?}"
        );
    }

    #[test]
    fn or_constraint_distributes_mixed_branches_and_preserves_bindings() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (deffacts startup
              (v 2)
              (v 3))
            (defrule branchy
              (v ?x&2|?x&~2)
              =>
              (assert (hit ?x)))
            ",
        );
        engine.reset().unwrap();

        let run = run_to_completion(&mut engine);
        assert_eq!(run.rules_fired, 2);

        let hits = find_facts_by_relation(&engine, "hit");
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn predicate_constraint_filters_positive_pattern_matches() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (deffacts startup
              (data 1)
              (data 3)
              (data 5))
            (defrule gt-two
              (data ?x&:(> ?x 2))
              =>
              (assert (gt2 ?x)))
            ",
        );
        engine.reset().unwrap();

        let rule_info = engine
            .rule_info
            .iter()
            .flatten()
            .find(|info| info.name == "gt-two")
            .expect("compiled rule metadata");
        assert!(
            rule_info.test_conditions.is_empty(),
            "simple slot comparisons should lower into alpha tests"
        );

        let run = run_to_completion(&mut engine);
        assert_eq!(run.rules_fired, 2);
        assert_eq!(find_facts_by_relation(&engine, "gt2").len(), 2);
    }

    #[test]
    fn return_value_constraint_filters_positive_pattern_matches() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (deffacts startup
              (pair 3 4)
              (pair 3 5))
            (defrule plus-one
              (pair ?x =(+ ?x 1))
              =>
              (assert (ok ?x)))
            ",
        );
        engine.reset().unwrap();

        let run = run_to_completion(&mut engine);
        assert_eq!(run.rules_fired, 1);
        assert_eq!(find_facts_by_relation(&engine, "ok").len(), 1);
    }

    #[test]
    fn predicate_constraint_filters_template_slot_matches() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (deftemplate zc8 (slot x) (slot y))
            (deffacts startup
              (zc8 (x a) (y 5))
              (zc8 (x a) (y -1)))
            (defrule positive-y
              (zc8 (x ?x) (y ?y&:(> ?y 0)))
              =>
              (assert (pos ?y)))
            ",
        );
        engine.reset().unwrap();

        let run = run_to_completion(&mut engine);
        assert_eq!(run.rules_fired, 1);
        assert_eq!(find_facts_by_relation(&engine, "pos").len(), 1);
    }

    #[test]
    fn return_value_constraint_filters_template_slot_matches() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (deftemplate pair (slot a) (slot b))
            (deffacts startup
              (pair (a 2) (b 3))
              (pair (a 2) (b 4)))
            (defrule plus-one
              (pair (a ?a) (b =(+ ?a 1)))
              =>
              (assert (tpl-ok ?a)))
            ",
        );
        engine.reset().unwrap();

        let run = run_to_completion(&mut engine);
        assert_eq!(run.rules_fired, 1);
        assert_eq!(find_facts_by_relation(&engine, "tpl-ok").len(), 1);
    }

    #[test]
    fn negated_predicate_constraint_filters_with_outer_variable_comparison() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (deffacts startup
              (anchor 2)
              (anchor 5)
              (data 1)
              (data 3))
            (defrule no-greater
              (anchor ?min)
              (not (data ?x&:(> ?x ?min)))
              =>
              (assert (safe ?min)))
            ",
        );
        engine.reset().unwrap();

        let run = run_to_completion(&mut engine);
        assert_eq!(run.rules_fired, 1);
        assert_eq!(find_facts_by_relation(&engine, "safe").len(), 1);
    }

    #[test]
    fn negated_predicate_constraint_filters_with_offset_expression() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (deffacts startup
              (anchor 1)
              (anchor 3)
              (data 2)
              (data 4))
            (defrule no-far-greater
              (anchor ?min)
              (not (data ?x&:(> ?x (+ ?min 1))))
              =>
              (assert (safe-offset ?min)))
            ",
        );
        engine.reset().unwrap();

        let run = run_to_completion(&mut engine);
        assert_eq!(run.rules_fired, 1);
        assert_eq!(find_facts_by_relation(&engine, "safe-offset").len(), 1);
    }

    #[test]
    fn negated_predicate_constraint_filters_with_slot_side_offset_expression() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (deffacts startup
              (anchor 5)
              (anchor 8)
              (data 4)
              (data 7))
            (defrule no-greater-after-bump
              (anchor ?min)
              (not (data ?x&:(> (+ ?x 1) ?min)))
              =>
              (assert (safe-bump ?min)))
            ",
        );
        engine.reset().unwrap();

        let run = run_to_completion(&mut engine);
        assert_eq!(run.rules_fired, 1);
        assert_eq!(find_facts_by_relation(&engine, "safe-bump").len(), 1);
    }

    #[test]
    fn negated_predicate_constraint_filters_with_nested_linear_expression() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (deffacts startup
              (anchor 2)
              (anchor 4)
              (data 3))
            (defrule no-nested-greater
              (anchor ?min)
              (not (data ?x&:(> (+ (+ ?x 2) 1) (+ ?min 3))))
              =>
              (assert (safe-nested ?min)))
            ",
        );
        engine.reset().unwrap();

        let run = run_to_completion(&mut engine);
        assert_eq!(run.rules_fired, 1);
        assert_eq!(find_facts_by_relation(&engine, "safe-nested").len(), 1);
    }

    #[test]
    fn negated_predicate_constraint_filters_with_str_compare_expression() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r#"
            (deffacts startup
              (answer id1 "apple")
              (answer id2 "banana")
              (answer id3 "carrot"))
            (defrule lex-min
              (answer ? ?a)
              (not (answer ? ?b&:(> (str-compare ?a ?b) 0)))
              =>
              (assert (min-answer ?a)))
            "#,
        );
        engine.reset().unwrap();

        let run = run_to_completion(&mut engine);
        assert_eq!(run.rules_fired, 1);
        assert_eq!(find_facts_by_relation(&engine, "min-answer").len(), 1);
    }

    #[test]
    fn negated_return_value_constraint_filters_with_simple_variable_expression() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (deffacts startup
              (target 3)
              (target 4)
              (pair 2)
              (pair 3))
            (defrule missing-pair
              (target ?x)
              (not (pair =?x))
              =>
              (assert (missing ?x)))
            ",
        );
        engine.reset().unwrap();

        let run = run_to_completion(&mut engine);
        assert_eq!(run.rules_fired, 1);
        assert_eq!(find_facts_by_relation(&engine, "missing").len(), 1);
    }

    #[test]
    fn negated_predicate_constraint_matches_non_linear_expression() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (deffacts initial (anchor 2) (anchor 5) (data 3))
            (defrule no-square-greater
                (anchor ?min)
                (not (data ?x&:(> (* ?x ?x) (* ?min ?min))))
                => (assert (safe-square ?min)))",
        );
        engine.reset().unwrap();
        assert_eq!(engine.agenda_len(), 1);
        assert_eq!(run_to_completion(&mut engine).rules_fired, 1);
        assert_eq!(find_facts_by_relation(&engine, "safe-square").len(), 1);
    }

    #[test]
    fn negated_return_value_constraint_filters_with_offset_expression() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (deffacts startup
              (target 1)
              (target 2)
              (target 3)
              (pair 1 2)
              (pair 2 3))
            (defrule missing-offset
              (target ?x)
              (not (pair ?x =(+ ?x 1)))
              =>
              (assert (missing-offset ?x)))
            ",
        );
        engine.reset().unwrap();

        let run = run_to_completion(&mut engine);
        assert_eq!(run.rules_fired, 1);
        assert_eq!(find_facts_by_relation(&engine, "missing-offset").len(), 1);
    }

    #[test]
    fn negated_return_value_constraint_filters_with_nested_linear_expression() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (deffacts startup
              (target 1)
              (target 2)
              (target 3)
              (pair 3)
              (pair 4))
            (defrule missing-nested-offset
              (target ?x)
              (not (pair =(+ (+ ?x 1) 1)))
              =>
              (assert (missing-nested ?x)))
            ",
        );
        engine.reset().unwrap();

        let run = run_to_completion(&mut engine);
        assert_eq!(run.rules_fired, 1);
        assert_eq!(find_facts_by_relation(&engine, "missing-nested").len(), 1);
    }

    #[test]
    fn negated_return_value_constraint_matches_non_linear_expression() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule no-self-square
                (not (pair ?x&=(* ?x ?x)))
                => (assert (safe-return)))",
        );
        engine.reset().unwrap();
        engine.assert_ordered("pair", vec![2_i64]).unwrap();
        assert_eq!(engine.agenda_len(), 1);
        let blocker = engine.assert_ordered("pair", vec![1_i64]).unwrap();
        assert_eq!(engine.agenda_len(), 0);
        engine.retract(blocker).unwrap();
        assert_eq!(engine.agenda_len(), 1);
        assert_eq!(run_to_completion(&mut engine).rules_fired, 1);
    }

    #[test]
    fn negated_predicate_constraint_rejects_unbound_variables() {
        let mut engine = new_utf8_engine();
        let errors = engine
            .load_str("(defrule t (not (data b&:(> ?x ?y))) => (assert (ok)))")
            .unwrap_err();

        assert!(
            errors.iter().any(|e| matches!(
                e,
                LoadError::Compile(msg) if msg.contains("unbound LHS variable")
            )),
            "expected an unbound LHS diagnostic, got: {errors:?}"
        );
    }

    #[test]
    fn negated_return_value_constraint_rejects_unbound_variables() {
        let mut engine = new_utf8_engine();
        let errors = engine
            .load_str("(defrule t (not (pair =(* ?x 2))) => (assert (ok)))")
            .unwrap_err();

        assert!(
            errors.iter().any(|e| matches!(
                e,
                LoadError::Compile(msg) if msg.contains("unbound LHS variable")
            )),
            "expected an unbound LHS diagnostic, got: {errors:?}"
        );
    }

    #[test]
    fn translate_empty_or_constraint_returns_compile_error() {
        let mut engine = new_utf8_engine();
        let mut constant_tests = Vec::new();
        let mut variable_slots = Vec::new();
        let mut negated_variable_slots = Vec::new();
        let mut seen_variable_slots = HashMap::new();
        let mut generated_tests = Vec::new();
        let mut slot_runtime_vars = HashMap::new();
        let mut internal_slot_var_seed = 0usize;
        let span = test_span(9, 4);
        let constraint = Constraint::Or(Vec::new(), span);

        let error = engine
            .translate_constraint(
                &constraint,
                SlotIndex::Ordered(0),
                &mut constant_tests,
                &mut variable_slots,
                &mut negated_variable_slots,
                &mut seen_variable_slots,
                &mut generated_tests,
                &mut slot_runtime_vars,
                &mut internal_slot_var_seed,
                false,
            )
            .unwrap_err();

        match error {
            LoadError::Compile(message) => {
                assert!(
                    message.contains("or"),
                    "expected 'or' in error message, got: `{message}`"
                );
                assert!(message.contains("line 9, column 4"));
            }
            other => panic!("expected compile error, got {other:?}"),
        }
    }

    #[test]
    fn translate_negated_variable_constraint_produces_negated_slot() {
        let mut engine = new_utf8_engine();
        let mut constant_tests = Vec::new();
        let mut variable_slots = Vec::new();
        let mut negated_variable_slots = Vec::new();
        let mut seen_variable_slots = HashMap::new();
        let mut generated_tests = Vec::new();
        let mut slot_runtime_vars = HashMap::new();
        let mut internal_slot_var_seed = 0usize;
        let outer_span = test_span(7, 2);
        let inner_span = test_span(7, 3);
        let constraint = Constraint::Not(
            Box::new(Constraint::Variable("x".to_string(), inner_span)),
            outer_span,
        );

        engine
            .translate_constraint(
                &constraint,
                SlotIndex::Ordered(0),
                &mut constant_tests,
                &mut variable_slots,
                &mut negated_variable_slots,
                &mut seen_variable_slots,
                &mut generated_tests,
                &mut slot_runtime_vars,
                &mut internal_slot_var_seed,
                false,
            )
            .unwrap();

        assert_eq!(negated_variable_slots.len(), 1);
        assert_eq!(negated_variable_slots[0].0, SlotIndex::Ordered(0));
        assert_eq!(negated_variable_slots[0].2, JoinTestType::NotEqual);
    }

    #[test]
    fn translate_negated_literal_constraint_still_compiles() {
        let mut engine = new_utf8_engine();
        let mut constant_tests = Vec::new();
        let mut variable_slots = Vec::new();
        let mut negated_variable_slots = Vec::new();
        let mut seen_variable_slots = HashMap::new();
        let mut generated_tests = Vec::new();
        let mut slot_runtime_vars = HashMap::new();
        let mut internal_slot_var_seed = 0usize;
        let span = test_span(3, 8);
        let literal = ferric_rules_parser::LiteralValue {
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
                &mut negated_variable_slots,
                &mut seen_variable_slots,
                &mut generated_tests,
                &mut slot_runtime_vars,
                &mut internal_slot_var_seed,
                false,
            )
            .unwrap();

        assert_eq!(constant_tests.len(), 1);
        assert!(matches!(
            constant_tests[0].test_type,
            ConstantTestType::NotEqual(AtomKey::Integer(42))
        ));
    }

    #[test]
    fn translate_reused_variable_across_slots_generates_slot_equality_test() {
        let mut engine = new_utf8_engine();
        let mut constant_tests = Vec::new();
        let mut variable_slots = Vec::new();
        let mut negated_variable_slots = Vec::new();
        let mut seen_variable_slots = HashMap::new();
        let mut generated_tests = Vec::new();
        let mut slot_runtime_vars = HashMap::new();
        let mut internal_slot_var_seed = 0usize;
        let span = test_span(12, 7);

        let first = Constraint::Variable("x".to_string(), span);
        let second = Constraint::Variable("x".to_string(), span);

        engine
            .translate_constraint(
                &first,
                SlotIndex::Ordered(0),
                &mut constant_tests,
                &mut variable_slots,
                &mut negated_variable_slots,
                &mut seen_variable_slots,
                &mut generated_tests,
                &mut slot_runtime_vars,
                &mut internal_slot_var_seed,
                false,
            )
            .unwrap();
        engine
            .translate_constraint(
                &second,
                SlotIndex::Ordered(1),
                &mut constant_tests,
                &mut variable_slots,
                &mut negated_variable_slots,
                &mut seen_variable_slots,
                &mut generated_tests,
                &mut slot_runtime_vars,
                &mut internal_slot_var_seed,
                false,
            )
            .unwrap();

        assert_eq!(variable_slots.len(), 1, "only the first slot binds ?x");
        assert_eq!(constant_tests.len(), 1);
        assert_eq!(constant_tests[0].slot, SlotIndex::Ordered(1));
        assert!(matches!(
            constant_tests[0].test_type,
            ConstantTestType::EqualSlot(SlotIndex::Ordered(0))
        ));
    }

    #[test]
    fn load_defrule_with_multiple_patterns() {
        let mut engine = new_utf8_engine();
        let source = r"
            (defrule match-pair
                (person ?x)
                (person ?y)
                =>
                (assert (pair ?x ?y)))
        ";
        let result = load_ok(&mut engine, source);

        assert_eq!(result.rules.len(), 1);
        let rule = &result.rules[0];
        assert_eq!(rule.name, "match-pair");
        assert_eq!(rule.patterns.len(), 2);
        assert_eq!(rule.actions.len(), 1);
    }

    #[test]
    fn load_mixed_assert_and_defrule() {
        let mut engine = new_utf8_engine();
        let source = r#"
            (assert (person Alice))
            (defrule greet (person ?x) => (printout t "Hello " ?x crlf))
            (assert (person Bob))
        "#;
        let result = load_ok(&mut engine, source);

        assert_eq!(result.asserted_facts.len(), 2);
        assert_eq!(result.rules.len(), 1);
    }

    #[test]
    fn load_deftemplate() {
        let mut engine = new_utf8_engine();
        let result = load_ok(&mut engine, "(deftemplate person (slot name))");

        assert_eq!(result.templates.len(), 1);
        assert_eq!(result.templates[0].name, "person");
        assert_eq!(result.templates[0].slots.len(), 1);
        assert_eq!(result.templates[0].slots[0].name, "name");
    }

    #[test]
    fn load_unsupported_form_returns_error() {
        // defclass is not yet supported; verify the UnsupportedForm error fires
        let mut engine = new_utf8_engine();
        let errors = engine
            .load_str("(defclass Sensor (is-a USER))")
            .unwrap_err();

        assert_eq!(errors.len(), 1);
        match &errors[0] {
            LoadError::UnsupportedForm { name, .. } => {
                assert_eq!(name, "defclass");
            }
            _ => panic!("expected UnsupportedForm error"),
        }
    }

    #[test]
    fn load_deffunction_succeeds() {
        let mut engine = new_utf8_engine();
        let result = engine
            .load_str("(deffunction add-one (?x) (+ ?x 1))")
            .unwrap();

        assert_eq!(result.functions.len(), 1);
        assert_eq!(result.functions[0].name, "add-one");
        assert!(result.rules.is_empty());
        assert!(result.asserted_facts.is_empty());
    }

    #[test]
    fn failed_global_initializers_cannot_leave_phantom_persisted_metadata() {
        let mut engine = Engine::new(EngineConfig::default());
        let errors = engine
            .load_str(include_str!(
                "../tests/fixtures/global_incremental_failure.clp"
            ))
            .unwrap_err();
        assert_eq!(errors.len(), 1);
        // Snapshot restoration and module visibility must describe the same set
        // of globals as the active value store, even after a partial load.
        for (module, names) in &engine.global_modules {
            for (name, owner) in names {
                assert_eq!(module, owner);
                assert!(
                    engine.globals.contains(*module, name),
                    "phantom global {module:?}::{name}"
                );
            }
        }
        for (module, name, _) in &engine.registered_globals {
            assert!(engine.globals.contains(*module, name));
            assert_eq!(
                engine
                    .global_modules
                    .get(module)
                    .and_then(|names| names.get(name.as_str())),
                Some(module)
            );
        }
    }

    #[test]
    fn load_defglobal_succeeds() {
        let mut engine = new_utf8_engine();
        let result = load_ok(&mut engine, "(defglobal ?*threshold* = 50)");

        assert_eq!(result.globals.len(), 1);
        assert_eq!(result.globals[0].globals.len(), 1);
        assert_eq!(result.globals[0].globals[0].name, "threshold");
        assert!(result.rules.is_empty());
        assert!(result.asserted_facts.is_empty());
    }

    #[test]
    fn load_deffunction_with_comment_succeeds() {
        let mut engine = new_utf8_engine();
        let result = engine
            .load_str(r#"(deffunction inc "Increment" (?x) (+ ?x 1))"#)
            .unwrap();

        assert_eq!(result.functions.len(), 1);
        assert_eq!(result.functions[0].comment, Some("Increment".to_string()));
    }

    #[test]
    fn load_defglobal_multiple_globals_succeeds() {
        let mut engine = new_utf8_engine();
        let result = engine
            .load_str("(defglobal ?*pi* = 3.14159 ?*e* = 2.71828)")
            .unwrap();

        assert_eq!(result.globals.len(), 1);
        assert_eq!(result.globals[0].globals.len(), 2);
        assert_eq!(result.globals[0].globals[0].name, "pi");
        assert_eq!(result.globals[0].globals[1].name, "e");
    }

    #[test]
    fn load_empty_top_level_list_returns_error_not_panic() {
        let mut engine = new_utf8_engine();
        let errors = load_err(&mut engine, "()");

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
        let mut engine = new_utf8_engine();
        let errors = load_err(&mut engine, "(assert ())");

        assert_eq!(errors.len(), 1);
        assert!(matches!(errors[0], LoadError::InvalidAssert(_)));
    }

    #[test]
    fn load_invalid_assert_non_symbol_relation() {
        let mut engine = new_utf8_engine();
        let errors = load_err(&mut engine, "(assert (42 value))");

        assert_eq!(errors.len(), 1);
        assert!(matches!(errors[0], LoadError::InvalidAssert(_)));
    }

    #[test]
    fn load_invalid_defrule_missing_name() {
        let mut engine = new_utf8_engine();
        let errors = load_err(&mut engine, "(defrule)");

        assert_eq!(errors.len(), 1);
        assert!(matches!(errors[0], LoadError::Interpret(_)));
    }

    #[test]
    fn load_invalid_defrule_missing_arrow() {
        let mut engine = new_utf8_engine();
        let errors = engine
            .load_str("(defrule test (person ?x) (printout t ?x))")
            .unwrap_err();

        assert_eq!(errors.len(), 1);
        assert!(matches!(errors[0], LoadError::Interpret(_)));
    }

    #[test]
    fn load_invalid_defrule_non_symbol_name() {
        let mut engine = new_utf8_engine();
        let errors = engine
            .load_str("(defrule 123 (person ?x) => (printout t ?x))")
            .unwrap_err();

        assert_eq!(errors.len(), 1);
        assert!(matches!(errors[0], LoadError::Interpret(_)));
    }

    #[test]
    fn load_invalid_defrule_not_with_multiple_patterns() {
        let mut engine = new_utf8_engine();
        let errors = engine
            .load_str("(defrule test (not (a) (b)) => (printout t ok))")
            .unwrap_err();

        assert_eq!(errors.len(), 1);
        match &errors[0] {
            LoadError::Interpret(error) => {
                let message = error.to_string();
                assert!(message.contains("expected exactly one pattern"));
                assert!(message.contains("line 1, column "));
            }
            other => panic!("expected interpret error, got {other:?}"),
        }
    }

    #[test]
    fn load_parse_error() {
        let mut engine = new_utf8_engine();
        let errors = load_err(&mut engine, "(assert (person)");

        assert_eq!(errors.len(), 1);
        assert!(matches!(errors[0], LoadError::Parse(_)));
    }

    #[test]
    fn load_recovers_after_malformed_deffunction_and_runs_later_constructs() {
        let mut engine = new_utf8_engine();
        let source = r"
            (deffunction foo ())
            (deffunction bar () 42)
            (defrule test (go) => (printout t (bar) crlf))
            (deffacts startup (go))
        ";
        let errors = load_err(&mut engine, source);
        let joined = errors
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ");
        assert!(
            joined.contains("deffunction requires at least one body expression"),
            "expected malformed deffunction diagnostic, got: {joined}"
        );

        engine.reset().expect("reset");
        run_to_completion(&mut engine);
        let output = engine
            .get_output("t")
            .unwrap()
            .unwrap_or("")
            .trim()
            .to_string();
        assert_eq!(output, "42");
    }

    #[test]
    fn load_recovers_after_bad_rule_and_runs_other_rules() {
        let mut engine = new_utf8_engine();
        let source = r"
            (deftemplate a (slot one) (slot two))
            (defrule bad  (a (three 3)) => (printout t BAD crlf))
            (defrule good (a (one 1))  => (printout t GOOD crlf))
            (deffacts startup (a (one 1) (two ok)))
        ";
        let errors = load_err(&mut engine, source);
        let joined = errors
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ");
        assert!(
            joined.contains("unknown slot `three`"),
            "expected unknown-slot diagnostic, got: {joined}"
        );

        engine.reset().expect("reset");
        run_to_completion(&mut engine);
        let output = engine.get_output("t").unwrap().unwrap_or("");
        assert!(
            output.contains("GOOD"),
            "expected valid rule to run after recovery, got: {output:?}"
        );
        assert!(
            !output.contains("BAD"),
            "bad rule should not compile/fire after unknown-slot error, got: {output:?}"
        );
    }

    #[test]
    fn load_deffacts() {
        let mut engine = new_utf8_engine();
        let source = r"
            (deffacts startup
                (person Alice)
                (person Bob))
        ";
        let result = load_ok(&mut engine, source);
        engine.reset().unwrap();

        assert!(result.asserted_facts.is_empty());
        assert!(result.rules.is_empty());
    }

    #[test]
    fn load_deffacts_ambiguous_empty_slot_form_falls_back_to_ordered_fact() {
        let mut engine = new_utf8_engine();
        let result = load_ok(
            &mut engine,
            r"
            (deffacts startup
                (foo bar)
                (foo (clear)))
            ",
        );
        engine.reset().unwrap();

        assert!(result.asserted_facts.is_empty());

        let fact_id = engine.find_facts("foo").unwrap()[1].0;
        let entry = engine
            .fact_base
            .get(engine.host.resolve(fact_id).unwrap())
            .expect("asserted fact should exist");
        match &entry.fact {
            ferric_rules_core::Fact::Ordered(ordered) => {
                let relation = engine
                    .resolve_core_symbol(ordered.relation)
                    .expect("relation symbol should resolve");
                assert_eq!(relation, "foo");
                assert_eq!(ordered.fields.len(), 1);
                let Value::Symbol(field_sym) = ordered.fields[0] else {
                    panic!("expected symbol field, got {:?}", ordered.fields[0]);
                };
                let field = engine
                    .resolve_core_symbol(field_sym)
                    .expect("field symbol should resolve");
                assert_eq!(field, "clear");
            }
            Fact::Template(template) => {
                panic!("expected ordered fact fallback, got template fact {template:?}")
            }
        }
    }

    #[test]
    fn load_deffacts_unknown_template_with_explicit_slot_value_still_errors() {
        let mut engine = new_utf8_engine();
        let errors = load_err(&mut engine, "(deffacts startup (ghost (slot1 value)))");

        assert!(
            errors
                .iter()
                .any(|e| matches!(e, LoadError::Compile(msg) if msg.contains("unknown template"))),
            "expected unknown-template error, got: {errors:?}"
        );
    }

    #[test]
    fn load_nested_fact_produces_warning() {
        let mut engine = new_utf8_engine();
        let source = r#"(assert (person (name "John") (age 30)))"#;
        let result = load_ok(&mut engine, source);

        // The nested lists will be skipped with warnings
        assert_eq!(result.asserted_facts.len(), 1);
        assert!(!result.warnings.is_empty());
    }

    #[test]
    fn load_encoding_error_produces_warning() {
        let mut engine = Engine::new(EngineConfig::ascii());
        let source = "(assert (person \"héllo\"))";
        let result = load_ok(&mut engine, source);

        // The invalid string should produce a warning and be skipped
        assert_eq!(result.asserted_facts.len(), 1);
        assert!(!result.warnings.is_empty());
    }

    #[test]
    fn load_file_reads_from_disk() {
        use std::io::Write;
        let mut temp = tempfile::NamedTempFile::new().unwrap();
        write!(temp, "(assert (test 123))").unwrap();

        let mut engine = new_utf8_engine();
        let result = engine.load_file(temp.path()).unwrap();

        assert_eq!(result.asserted_facts.len(), 1);
    }

    #[test]
    fn load_nonexistent_file_returns_error() {
        let mut engine = new_utf8_engine();
        let errors = engine
            .load_file(Path::new("/nonexistent/path"))
            .unwrap_err();

        assert_eq!(errors.len(), 1);
        assert!(matches!(errors[0], LoadError::Io(_)));
    }

    // -----------------------------------------------------------------------
    // Pass 007: defmodule / defgeneric / defmethod loader tests
    // -----------------------------------------------------------------------

    #[test]
    fn load_defmodule_succeeds() {
        let mut engine = new_utf8_engine();
        let result = engine
            .load_str("(defmodule SENSOR (export ?ALL))")
            .expect("load should succeed");
        assert_eq!(result.modules.len(), 1);
        assert_eq!(result.modules[0].name, "SENSOR");
    }

    #[test]
    fn load_defgeneric_succeeds() {
        let mut engine = new_utf8_engine();
        let result = engine
            .load_str("(defgeneric display)")
            .expect("load should succeed");
        assert_eq!(result.generics.len(), 1);
        assert_eq!(result.generics[0].name, "display");
    }

    #[test]
    fn load_defmethod_succeeds() {
        let mut engine = new_utf8_engine();
        let result = engine
            .load_str("(defmethod display ((?x INTEGER)) ?x)")
            .expect("load should succeed");
        assert_eq!(result.methods.len(), 1);
        assert_eq!(result.methods[0].name, "display");
    }

    #[test]
    fn load_defmethod_with_index_succeeds() {
        let mut engine = new_utf8_engine();
        let result = engine
            .load_str("(defmethod display 1 ((?x)) ?x)")
            .expect("load should succeed");
        assert_eq!(result.methods.len(), 1);
        assert_eq!(result.methods[0].index, Some(1));
    }

    #[test]
    fn duplicate_defglobal_reports_source_location() {
        let mut engine = new_utf8_engine();
        let errors = engine
            .load_str(
                r"
            (defglobal ?*count* = 1)
            (defglobal ?*count* = 2)
        ",
            )
            .unwrap_err();

        let has_duplicate_error = errors.iter().any(|e| {
            matches!(e, LoadError::Compile(msg) if msg.contains("duplicate defglobal `count`") && msg.contains("line"))
        });
        assert!(
            has_duplicate_error,
            "expected duplicate defglobal error with location, got: {errors:?}"
        );
    }

    #[test]
    fn duplicate_defmodule_is_allowed_as_update() {
        // Re-defining a module updates its import/export specs rather than erroring.
        // This follows CLIPS semantics where (defmodule MAIN (import X ...)) is a
        // standard way to set up module visibility.
        let mut engine = new_utf8_engine();
        let result = engine.load_str(
            r"
            (defmodule SENSOR)
            (defmodule SENSOR (export ?ALL))
        ",
        );
        assert!(
            result.is_ok(),
            "re-defining a module should succeed, got: {result:?}"
        );
        // Verify the module's exports were updated to the last definition.
        let sensor_id = engine
            .module_registry
            .get_by_name("SENSOR")
            .expect("SENSOR should be registered");
        let sensor = engine
            .module_registry
            .get(sensor_id)
            .expect("SENSOR module should be found");
        assert!(
            matches!(sensor.exports[0], ferric_rules_parser::ModuleSpec::All),
            "expected SENSOR to export ?ALL after re-definition"
        );
    }

    // -----------------------------------------------------------------------
    // Pass 005: deffunction/defgeneric conflict diagnostics
    // -----------------------------------------------------------------------

    #[test]
    fn deffunction_then_defgeneric_same_name_errors() {
        let mut engine = new_utf8_engine();
        let errors = engine
            .load_str(
                r"
            (deffunction display (?x) ?x)
            (defgeneric display)
        ",
            )
            .unwrap_err();

        let has_conflict = errors.iter().any(|e| {
            matches!(e, LoadError::Compile(msg)
                if msg.contains("cannot define defgeneric `display`")
                    && msg.contains("deffunction with the same name"))
        });
        assert!(
            has_conflict,
            "expected defgeneric/deffunction conflict error, got: {errors:?}"
        );
    }

    #[test]
    fn defgeneric_then_deffunction_same_name_errors() {
        let mut engine = new_utf8_engine();
        let errors = engine
            .load_str(
                r"
            (defgeneric display)
            (deffunction display (?x) ?x)
        ",
            )
            .unwrap_err();

        let has_conflict = errors.iter().any(|e| {
            matches!(e, LoadError::Compile(msg)
                if msg.contains("cannot define deffunction `display`")
                    && msg.contains("defgeneric with the same name"))
        });
        assert!(
            has_conflict,
            "expected deffunction/defgeneric conflict error, got: {errors:?}"
        );
    }

    #[test]
    fn defmethod_autocreate_conflicts_with_deffunction() {
        let mut engine = new_utf8_engine();
        let errors = engine
            .load_str(
                r"
            (deffunction display (?x) ?x)
            (defmethod display ((?x INTEGER)) ?x)
        ",
            )
            .unwrap_err();

        let has_conflict = errors.iter().any(|e| {
            matches!(e, LoadError::Compile(msg)
                if msg.contains("cannot define defmethod `display`")
                    && msg.contains("deffunction with the same name"))
        });
        assert!(
            has_conflict,
            "expected defmethod/deffunction conflict error, got: {errors:?}"
        );
    }

    #[test]
    fn deffunction_and_defgeneric_different_names_ok() {
        let mut engine = new_utf8_engine();
        let result = engine
            .load_str(
                r"
            (deffunction add-one (?x) (+ ?x 1))
            (defgeneric display)
            (defmethod display ((?x INTEGER)) ?x)
        ",
            )
            .expect("different names should not conflict");

        assert_eq!(result.functions.len(), 1);
        assert_eq!(result.generics.len(), 1);
        assert_eq!(result.methods.len(), 1);
    }

    #[test]
    fn defmethod_with_existing_generic_not_conflicting_with_deffunction() {
        // If a defgeneric already exists, a defmethod for that generic should
        // succeed even if a deffunction with a different name exists.
        let mut engine = new_utf8_engine();
        let result = engine
            .load_str(
                r"
            (deffunction helper (?x) ?x)
            (defgeneric display)
            (defmethod display ((?x INTEGER)) ?x)
        ",
            )
            .expect("defmethod for existing generic should succeed");

        assert_eq!(result.functions.len(), 1);
        assert_eq!(result.generics.len(), 1);
        assert_eq!(result.methods.len(), 1);
    }

    #[test]
    fn duplicate_defgeneric_reports_source_location() {
        let mut engine = new_utf8_engine();
        let errors = engine
            .load_str(
                r"
            (defgeneric display)
            (defgeneric display)
        ",
            )
            .unwrap_err();

        let has_duplicate_error = errors.iter().any(|e| {
            matches!(e, LoadError::Compile(msg) if msg.contains("duplicate defgeneric `display`") && msg.contains("line"))
        });
        assert!(
            has_duplicate_error,
            "expected duplicate defgeneric error with location, got: {errors:?}"
        );
    }

    #[test]
    fn duplicate_defmethod_explicit_index_reports_source_location() {
        let mut engine = new_utf8_engine();
        let errors = engine
            .load_str(
                r"
            (defgeneric describe)
            (defmethod describe 1 ((?x INTEGER)) ?x)
            (defmethod describe 1 ((?x FLOAT)) ?x)
        ",
            )
            .unwrap_err();

        let has_duplicate_error = errors.iter().any(|e| {
            matches!(e, LoadError::Compile(msg) if msg.contains("duplicate defmethod index 1 for `describe`") && msg.contains("line"))
        });
        assert!(
            has_duplicate_error,
            "expected duplicate defmethod index error with location, got: {errors:?}"
        );
    }
}

#[cfg(test)]
mod proptests {
    use super::{parse_qualified_name, Engine};
    use crate::test_helpers::new_utf8_engine;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn borrowed_template_names_match_owned_parser(raw in any::<String>()) {
            let parsed = parse_qualified_name(&raw);
            let expected = match &parsed {
                Ok(name) => (name.module_name(), name.local_name()),
                Err(_) => (None, raw.as_str()),
            };
            prop_assert_eq!(Engine::template_ref_parts(&raw), expected);
        }

        #[test]
        fn borrowed_template_names_match_parser_at_colon_boundaries(raw in "[a-z:]{0,30}") {
            let parsed = parse_qualified_name(&raw);
            let expected = match &parsed {
                Ok(name) => (name.module_name(), name.local_name()),
                Err(_) => (None, raw.as_str()),
            };
            prop_assert_eq!(Engine::template_ref_parts(&raw), expected);
        }

        /// Any valid assert form should produce at least one fact.
        #[test]
        fn valid_assert_produces_facts(
            relation in "[a-z][a-z0-9]{0,10}",
            values in prop::collection::vec(0i64..=100, 0..5)
        ) {
            let mut engine = new_utf8_engine();
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
            let mut engine = new_utf8_engine();
            let source = format!("(defrule {name} (item ?x) => (assert (result ?x)))");

            if let Ok(result) = engine.load_str(&source) {
                prop_assert_eq!(result.rules.len(), 1);
                prop_assert_eq!(&result.rules[0].name, &name);
            }
        }

        /// The loader should never panic on arbitrary input.
        #[test]
        fn loader_never_panics(source in "[\\x20-\\x7e]{0,200}") {
            let mut engine = new_utf8_engine();
            let _ = engine.load_str(&source);
        }
    }
}

#[cfg(test)]
mod sequence_runtime_union_tests {
    use super::*;
    use ferric_rules_core::RuntimeConditionRole;

    fn setup(source: &str) -> Engine {
        let mut engine = Engine::new(crate::EngineConfig::utf8());
        engine.load_str(source).unwrap();
        engine.reset().unwrap();
        engine
    }

    #[test]
    fn loader_union_positive_sequence_predicates_receive_projected_bindings() {
        for pattern in [
            "(row $?before ?value&:(> (length$ $?before) 0) $?after)",
            "(packet (values $?before ?value&:(> (length$ $?before) 0) $?after))",
        ] {
            let mut engine = setup(&format!(
                "(deftemplate packet (multislot values))
                 (defglobal ?*prefix* = pending ?*picked* = 0)
                 (deffacts input (row 1 2) (packet (values 1 2)))
                 (defrule choose {pattern} =>
                   (bind ?*prefix* $?before) (bind ?*picked* ?value))"
            ));
            let uses = engine.rete.snapshot_runtime_condition_uses();
            assert_eq!(uses.len(), 1);
            assert_eq!(uses[0].role, RuntimeConditionRole::PositiveJoin);
            assert_eq!(
                engine.run(crate::RunLimit::Unlimited).unwrap().rules_fired,
                1
            );
            assert!(matches!(
                engine.get_global("picked"),
                Some(Value::Integer(2))
            ));
            let Some(Value::Multifield(prefix)) = engine.get_global("prefix") else {
                panic!("projected prefix must remain a MULTIFIELD");
            };
            assert!(matches!(prefix.as_slice(), [Value::Integer(1)]));
            assert!(engine.action_diagnostics().is_empty());
        }
    }

    #[test]
    fn loader_union_sequence_field_or_expands_with_leading_binding_preserved() {
        let mut engine = setup(
            "(deftemplate packet (multislot values))
             (deffunction one (?x) (eq ?x 1))
             (deffacts input (packet (values 1 2)))
             (defrule choose
               (packet (values $?before ?x&:(one ?x)|2 $?after))
               => (printout t ?x crlf))",
        );
        assert_eq!(engine.rete.snapshot_rule_ids().count(), 2);
        let uses = engine.rete.snapshot_runtime_condition_uses();
        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].role, RuntimeConditionRole::PositiveJoin);
        assert_eq!(
            engine.run(crate::RunLimit::Unlimited).unwrap().rules_fired,
            2
        );
        let mut lines: Vec<_> = engine.get_output("t").unwrap().unwrap().lines().collect();
        lines.sort_unstable();
        assert_eq!(lines, ["1", "2"]);
        assert!(engine.action_diagnostics().is_empty());
    }

    #[test]
    fn loader_union_whole_multislot_callback_remains_physical_and_early() {
        let mut engine = setup(
            "(deftemplate packet (slot tag) (multislot values))
             (defglobal ?*observed* = 0)
             (deffunction remember (?size)
               (bind ?*observed* ?size) TRUE)
             (deffacts input (packet (tag ready) (values 4 5)))
             (defrule choose (anchor)
               (packet (values $?all&:(remember (length$ $?all))) (tag ?tag))
               => (printout t ?tag crlf))",
        );
        assert!(matches!(
            engine.get_global("observed"),
            Some(Value::Integer(2))
        ));
        let uses = engine.rete.snapshot_runtime_condition_uses();
        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].role, RuntimeConditionRole::PatternFilter);
        assert!(uses[0]
            .bindings
            .iter()
            .any(|(slot, _)| *slot == SlotIndex::Template(1)));
        assert_eq!(
            engine.run(crate::RunLimit::Unlimited).unwrap().rules_fired,
            0
        );
        engine.load_str("(assert (anchor))").unwrap();
        assert_eq!(
            engine.run(crate::RunLimit::Unlimited).unwrap().rules_fired,
            1
        );
        assert_eq!(engine.get_output("t").unwrap(), Some("ready\n"));
        assert!(engine.action_diagnostics().is_empty());
    }

    #[test]
    fn loader_union_rejects_negative_sequence_callbacks_and_forward_field_references() {
        let mut engine = setup("(deftemplate packet (multislot values))");
        let before = engine.rete.snapshot_rule_ids().count();
        let errors = engine
            .load_str(
                "(defrule bad
               (not (packet (values $?before ?value&:(> (length$ $?before) 0) $?after)))
               => (assert (bad-fired)))",
            )
            .unwrap_err();
        assert!(errors.iter().any(|error| error
            .to_string()
            .contains("runtime constraints require one physical whole-slot field")));
        assert_eq!(engine.rete.snapshot_rule_ids().count(), before);
        let errors = engine
            .load_str(
                "(defrule unbound
               (packet (values ?first&:(eq ?later 1) ?later))
               => (assert (bad-fired)))",
            )
            .unwrap_err();
        assert!(errors
            .iter()
            .any(|error| error.to_string().contains("unbound LHS variable")));
        assert_eq!(engine.rete.snapshot_rule_ids().count(), before);
    }
}
