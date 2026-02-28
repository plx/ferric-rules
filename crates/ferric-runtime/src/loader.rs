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

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::rc::Rc;
use thiserror::Error;

// Qualified name utilities: wired into construct loading in passes 003/004.
#[allow(unused_imports)]
use crate::qualified_name::{parse_qualified_name, QualifiedName};

use ferric_core::{
    AlphaEntryType, AtomKey, CompilableCondition, CompilablePattern, CompileResult, ConstantTest,
    ConstantTestType, FactId, FerricString, JoinTestType, RuleId, Salience, SlotIndex, Value,
};
use ferric_parser::{
    interpret_constructs, parse_sexprs, ActionExpr, Atom, Constraint, Construct, FactBody,
    FactValue, FileId, FunctionCall, FunctionConstruct, GenericConstruct, GlobalConstruct,
    InterpretError, InterpreterConfig, LiteralKind, MethodConstruct, ModuleConstruct,
    OrderedFactBody, OrderedPattern, ParseError, Pattern, RuleConstruct, SExpr, SlotConstraint,
    Span, TemplateConstruct, TemplateFactBody, TemplatePattern,
};

use crate::actions::{
    CompiledRuleInfo, CompiledTestCondition, MultifieldTailBindingHint, NegatedPatternRuntimeCheck,
};
use crate::engine::{Engine, EngineError};
use crate::functions::UserFunction;
use crate::templates::RegisteredTemplate;
// GenericRegistry accessed via self.generics (field on Engine)

/// Translated rule data including fact-address variable bindings.
struct TranslatedRule {
    rule_id: RuleId,
    salience: Salience,
    conditions: Vec<CompilableCondition>,
    fact_address_vars: HashMap<String, usize>,
    /// Test conditions (not compiled into Rete; evaluated at firing time).
    test_conditions: Vec<CompiledTestCondition>,
    /// Action-time hints for trailing ordered multifield captures.
    multifield_tail_bindings: Vec<MultifieldTailBindingHint>,
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
    #[allow(clippy::too_many_lines)] // Sequential pipeline steps; each section is clearly delineated
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
                .map(LoadError::Parse)
                .collect();
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

        // Interpret constructs via Stage 2
        if !construct_forms.is_empty() {
            let config = InterpreterConfig::default();
            let interpret_result = interpret_constructs(&construct_forms, &config);

            // Convert interpret errors to LoadError
            if !interpret_result.errors.is_empty() {
                for e in interpret_result.errors {
                    errors.push(LoadError::Interpret(e));
                }
            }

            // Collect constructs by type (don't assert deffacts yet).
            //
            // Rules are collected with their owning module captured at the
            // time they appear in source so that defmodule statements
            // interleaved with defrule statements are respected.
            let mut deffacts_constructs = Vec::new();
            let mut rules_with_module = Vec::new();
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
                        if let Err(e) = self.register_template(&template, &mut result) {
                            errors.push(e);
                        }
                        result.templates.push(template);
                    }
                    Construct::Facts(facts) => {
                        deffacts_constructs.push(facts);
                    }
                    Construct::Function(func) => {
                        let owning_module = self.module_registry.current_module();
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
                        self.function_modules
                            .insert((owning_module, func.name.clone()), owning_module);
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
                        // Record the owning module for each global defined in this construct.
                        let owning_module = self.module_registry.current_module();
                        for def in &global.globals {
                            self.global_modules
                                .insert((owning_module, def.name.clone()), owning_module);
                        }
                        // Evaluate initial values and store in the global store.
                        if let Err(e) = self.process_global_construct(&global) {
                            errors.push(e);
                        }
                        result.globals.push(global);
                    }
                    Construct::Module(module) => {
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
                            self.generic_modules
                                .insert((owning_module, generic.name.clone()), owning_module);
                            // Register the generic function declaration.
                            self.generics.register_generic(owning_module, &generic.name);
                            result.generics.push(generic);
                        }
                    }
                    Construct::Method(method) => {
                        let owning_module = self.module_registry.current_module();
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
                        self.generic_modules
                            .entry((owning_module, method.name.clone()))
                            .or_insert(owning_module);
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
            for (rule, owning_module) in &rules_with_module {
                self.module_registry.set_current_module(*owning_module);
                match self.compile_rule_construct(rule, source) {
                    Ok(_) => {}
                    Err(e) => errors.push(e),
                }
                result.rules.push(rule.clone());
            }
            self.module_registry.set_current_module(saved_module);

            // Ensure (initial-fact) is present AFTER rules are compiled but BEFORE
            // deffacts are asserted.  This mirrors CLIPS' built-in (initial-fact)
            // mechanism: it provides the root token required by top-level NCC/forall
            // subnetworks so that items asserted through deffacts are properly evaluated.
            // Asserted only once; subsequent load_str calls skip if already present.
            if let Err(e) = self.ensure_initial_fact() {
                errors.push(e);
            }

            // Now process deffacts (facts will flow through compiled rete via assert_ordered).
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

    /// Ensure `(initial-fact)` is present in working memory.
    ///
    /// `(initial-fact)` provides the root token for top-level NCC/negation/forall CEs,
    /// mirroring CLIPS' built-in `(initial-fact)` mechanism.  It is asserted once;
    /// subsequent calls are no-ops if it is already present.
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

        let fid = self
            .fact_base
            .assert_ordered(initial_sym, smallvec::SmallVec::new());
        let stored = self.fact_base.get(fid).unwrap().fact.clone();
        self.rete.assert_fact(fid, &stored, &self.fact_base);
        self.initial_fact_id = Some(fid);

        Ok(())
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

    fn template_local_name(raw: &str) -> String {
        match parse_qualified_name(raw) {
            Ok(parsed) => parsed.local_name().to_string(),
            Err(_) => raw.to_string(),
        }
    }

    fn template_ref_parts(raw: &str) -> (Option<String>, String) {
        match parse_qualified_name(raw) {
            Ok(QualifiedName::Qualified { module, name }) => (Some(module), name),
            Ok(QualifiedName::Unqualified(name)) => (None, name),
            Err(_) => (None, raw.to_string()),
        }
    }

    fn module_label_for_error(&self, module_id: crate::modules::ModuleId) -> String {
        self.module_registry
            .module_name(module_id)
            .unwrap_or("?")
            .to_string()
    }

    fn resolve_template_reference(
        &self,
        raw_name: &str,
        current_module: crate::modules::ModuleId,
    ) -> Result<ferric_core::TemplateId, String> {
        let (qualified_module_name, wanted_local_name) = Self::template_ref_parts(raw_name);
        let current_module_label = self.module_label_for_error(current_module);

        let mut candidates: Vec<(ferric_core::TemplateId, crate::modules::ModuleId)> = Vec::new();
        for (&template_id, registered) in &self.template_defs {
            let template_module = self
                .template_modules
                .get(&template_id)
                .copied()
                .unwrap_or_else(|| self.module_registry.main_module_id());
            let local_name = Self::template_local_name(&registered.name);

            if local_name == wanted_local_name {
                candidates.push((template_id, template_module));
            }
        }

        let choose_or_ambiguous = |list: &[(
            ferric_core::TemplateId,
            crate::modules::ModuleId,
        )]| {
            if list.len() == 1 {
                return Ok(list[0].0);
            }

            let mut modules = BTreeSet::new();
            for (_, module_id) in list {
                modules.insert(self.module_label_for_error(*module_id));
            }
            Err(format!(
                "template `{raw_name}` is ambiguous from module `{current_module_label}` (matches modules: {})",
                modules.into_iter().collect::<Vec<_>>().join(", ")
            ))
        };

        if let Some(module_name) = qualified_module_name {
            let Some(target_module) = self.module_registry.get_by_name(&module_name) else {
                return Err(format!("unknown template `{raw_name}`"));
            };

            let module_matches: Vec<_> = candidates
                .into_iter()
                .filter(|(_, module_id)| *module_id == target_module)
                .collect();

            if module_matches.is_empty() {
                return Err(format!("unknown template `{raw_name}`"));
            }

            let template_id = choose_or_ambiguous(&module_matches)?;
            if !self.module_registry.is_construct_visible(
                current_module,
                target_module,
                "deftemplate",
                &wanted_local_name,
            ) {
                return Err(format!(
                    "template `{raw_name}` is not visible from module `{current_module_label}`"
                ));
            }

            return Ok(template_id);
        }

        if candidates.is_empty() {
            return Err(format!("unknown template `{raw_name}`"));
        }

        // Prefer local module definitions over imported ones.
        let same_module: Vec<_> = candidates
            .iter()
            .copied()
            .filter(|(_, module_id)| *module_id == current_module)
            .collect();

        if !same_module.is_empty() {
            return choose_or_ambiguous(&same_module);
        }

        let visible: Vec<_> = candidates
            .into_iter()
            .filter(|(_, module_id)| {
                self.module_registry.is_construct_visible(
                    current_module,
                    *module_id,
                    "deftemplate",
                    &wanted_local_name,
                )
            })
            .collect();

        if visible.is_empty() {
            return Err(format!(
                "template `{raw_name}` is not visible from module `{current_module_label}`"
            ));
        }

        choose_or_ambiguous(&visible)
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
                match value {
                    // CLIPS splices multifield values into ordered facts.
                    Value::Multifield(mf) => fields.extend(mf.as_slice().iter().cloned()),
                    other => fields.push(other),
                }
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
                    return self
                        .assert_ordered(&template.template, fields)
                        .map_err(LoadError::Engine);
                }
                return Err(LoadError::Compile(format!("{msg} in deffacts")));
            }
        };

        let registered = self
            .template_defs
            .get(&template_id)
            .cloned()
            .ok_or_else(|| {
                LoadError::Compile(format!(
                    "template `{}` not found in registry",
                    template.template
                ))
            })?;

        // Start with defaults.
        let mut slots: Vec<Value> = registered.defaults.clone();

        // Apply slot values from the deffacts body.
        for slot_val in &template.slot_values {
            let slot_idx = registered
                .slot_index
                .get(&slot_val.name)
                .copied()
                .ok_or_else(|| {
                    LoadError::Compile(format!(
                        "unknown slot `{}` in template `{}`",
                        slot_val.name, template.template
                    ))
                })?;

            if let Some(value) = self.fact_value_to_value(&slot_val.value, result) {
                slots[slot_idx] = value;
            }
        }

        // Assert as a proper template fact.
        let fact_id = self
            .fact_base
            .assert_template(template_id, slots.into_boxed_slice());

        // Propagate through rete.
        let fact = self
            .fact_base
            .get(fact_id)
            .expect("just-asserted fact must exist")
            .fact
            .clone();
        self.rete.assert_fact(fact_id, &fact, &self.fact_base);

        Ok(fact_id)
    }

    fn is_ambiguous_empty_template_fact(template: &TemplateFactBody) -> bool {
        !template.slot_values.is_empty()
            && template
                .slot_values
                .iter()
                .all(|slot| matches!(slot.value, FactValue::EmptyMultifield(_)))
    }

    /// Register a `TemplateConstruct` in the engine's template registry.
    ///
    /// Allocates a fresh `TemplateId`, builds slot metadata, and stores both
    /// the name→id mapping and the `RegisteredTemplate`.
    #[allow(clippy::unnecessary_wraps)] // Result return kept for future error paths
    fn register_template(
        &mut self,
        template: &TemplateConstruct,
        result: &mut LoadResult,
    ) -> Result<(), LoadError> {
        // Allocate a new TemplateId.
        let template_id = self.template_id_alloc.insert(());

        let slot_count = template.slots.len();
        let mut slot_names = Vec::with_capacity(slot_count);
        let mut slot_index = HashMap::with_capacity(slot_count);
        let mut defaults = Vec::with_capacity(slot_count);

        for (i, slot_def) in template.slots.iter().enumerate() {
            slot_names.push(slot_def.name.clone());
            slot_index.insert(slot_def.name.clone(), i);

            let default_val = match &slot_def.default {
                Some(ferric_parser::DefaultValue::Value(lit)) => self
                    .literal_to_value(&lit.value, lit.span.start.line, result)
                    .unwrap_or(Value::Void),
                // ?NONE, ?DERIVE, or no default: use Void as placeholder.
                _ => Value::Void,
            };
            defaults.push(default_val);
        }

        let registered = RegisteredTemplate {
            name: template.name.clone(),
            slot_names,
            slot_index,
            defaults,
        };

        self.template_ids.insert(template.name.clone(), template_id);
        self.template_defs.insert(template_id, registered);
        self.template_modules
            .insert(template_id, self.module_registry.current_module());

        Ok(())
    }

    /// Process a `GlobalConstruct`: evaluate each initial value expression and
    /// register it in both the active global store and the snapshot used for reset.
    fn process_global_construct(&mut self, global: &GlobalConstruct) -> Result<(), LoadError> {
        let current_module = self.module_registry.current_module();
        let mut seen_in_construct: HashSet<&str> = HashSet::new();
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
            let value = {
                let empty_bindings = ferric_core::binding::BindingSet::new();
                let empty_var_map = ferric_core::binding::VarMap::new();
                let mut ctx = crate::evaluator::EvalContext {
                    bindings: &empty_bindings,
                    var_map: &empty_var_map,
                    symbol_table: &mut self.symbol_table,
                    config: &self.config,
                    functions: &self.functions,
                    globals: &mut self.globals,
                    generics: &self.generics,
                    call_depth: 0,
                    current_module: self.module_registry.current_module(),
                    module_registry: &self.module_registry,
                    function_modules: &self.function_modules,
                    global_modules: &self.global_modules,
                    generic_modules: &self.generic_modules,
                    method_chain: None,
                    input_buffer: None,
                };
                crate::evaluator::eval(&mut ctx, &runtime_expr)
                    .map_err(|e| LoadError::Compile(format!("global `{}` init: {e}", def.name)))?
            };

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
                    let empty_bindings = ferric_core::binding::BindingSet::new();
                    let empty_var_map = ferric_core::binding::VarMap::new();
                    let mut ctx = crate::evaluator::EvalContext {
                        bindings: &empty_bindings,
                        var_map: &empty_var_map,
                        symbol_table: &mut self.symbol_table,
                        config: &self.config,
                        functions: &self.functions,
                        globals: &mut self.globals,
                        generics: &self.generics,
                        call_depth: 0,
                        current_module: self.module_registry.current_module(),
                        module_registry: &self.module_registry,
                        function_modules: &self.function_modules,
                        global_modules: &self.global_modules,
                        generic_modules: &self.generic_modules,
                        method_chain: None,
                        input_buffer: None,
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
    fn compile_rule_construct(
        &mut self,
        rule: &RuleConstruct,
        source: &str,
    ) -> Result<CompileResult, LoadError> {
        // Validate patterns first (max nesting depth: 4 to support deeply nested NCCs)
        let validation_errors = validate_rule_patterns(&rule.patterns, 4);
        if !validation_errors.is_empty() {
            return Err(LoadError::Validation(validation_errors));
        }

        // Pre-process: distribute or CEs inside NCC/exists contexts.
        // This transforms patterns like (not (and A (or B C))) into
        // (and (not (and A B)) (not (and A C))) which can then be flattened.
        let rule = Self::normalize_nested_or_ces(rule);

        // Expand (or ...) CEs via rule duplication: a rule with (or P1 P2) becomes
        // N internal rules, each with one branch substituted. Multiple or CEs produce
        // the Cartesian product.
        let expanded_rules = Self::expand_or_patterns(&rule);
        let mut last_result = None;

        for variant in &expanded_rules {
            let result = self.compile_single_rule(variant, source)?;
            last_result = Some(result);
        }

        // Return the last compile result (all variants share the same name/semantics)
        last_result.ok_or_else(|| LoadError::Compile("empty or-expansion".to_string()))
    }

    fn validate_rule_action_callables(
        &self,
        rule: &RuleConstruct,
        current_module: crate::modules::ModuleId,
    ) -> Result<(), LoadError> {
        for action in &rule.actions {
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
            // `(assert (relation ...))`: each argument list represents a fact pattern,
            // so the relation name is data, not a callable. For template facts,
            // slot names are also data and only slot values are expressions.
            "assert" => {
                for arg in &call.args {
                    if let ActionExpr::FunctionCall(fact_pattern) = arg {
                        if self
                            .resolve_template_reference(&fact_pattern.name, current_module)
                            .is_ok()
                        {
                            for slot_expr in &fact_pattern.args {
                                if let ActionExpr::FunctionCall(slot_pair) = slot_expr {
                                    for value_expr in &slot_pair.args {
                                        self.validate_action_expr_as_expression(
                                            value_expr,
                                            current_module,
                                            rule_name,
                                        )?;
                                    }
                                } else {
                                    self.validate_action_expr_as_expression(
                                        slot_expr,
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
            ActionExpr::QueryAction { query, body, .. } => {
                self.validate_action_expr_as_expression(query, current_module, rule_name)?;
                for action in body {
                    self.validate_action_expr_as_expression(action, current_module, rule_name)?;
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
                | "undefrule"
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

    fn compile_single_rule(
        &mut self,
        rule: &RuleConstruct,
        source: &str,
    ) -> Result<CompileResult, LoadError> {
        self.validate_rule_action_callables(rule, self.module_registry.current_module())?;

        let translated = self
            .translate_rule_construct(rule)
            .map_err(|e| LoadError::Compile(format!("{e}")))?;

        let compile_result = self
            .compiler
            .compile_conditions(
                &mut self.rete,
                &self.fact_base,
                translated.rule_id,
                translated.salience,
                &translated.conditions,
            )
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
            var_map: compile_result.var_map.clone(),
            fact_address_vars: translated.fact_address_vars,
            salience: Salience::new(rule.salience),
            test_conditions: translated.test_conditions,
            runtime_actions,
            multifield_tail_bindings: translated.multifield_tail_bindings,
        };
        self.rule_info.insert(compile_result.rule_id, Rc::new(info));
        self.rule_modules.insert(
            compile_result.rule_id,
            self.module_registry.current_module(),
        );

        Ok(compile_result)
    }

    /// Recursively flatten a pattern for top-level condition processing.
    /// - `And`/`Logical`: flatten children.
    /// - `Not(Not(X))`: strip double negation (exists ≈ positive), then flatten X.
    /// - Everything else: push as-is.
    fn flatten_pattern<'a>(pattern: &'a Pattern, out: &mut Vec<&'a Pattern>) {
        match pattern {
            Pattern::And(inner, _) | Pattern::Logical(inner, _) => {
                for sub in inner {
                    Self::flatten_pattern(sub, out);
                }
            }
            Pattern::Not(inner, _) => {
                if let Pattern::Not(doubly_inner, _) = inner.as_ref() {
                    // (not (not X)) ≡ (exists X): strip double negation
                    Self::flatten_pattern(doubly_inner, out);
                } else {
                    out.push(pattern);
                }
            }
            _ => out.push(pattern),
        }
    }

    /// Desugar multi-pattern exists into individual patterns.
    ///
    /// `exists(P1, P2, ..., Pn)` is desugared to `P1, P2, ..., Pn-1, exists(Pn)`.
    /// The inner patterns become regular joins and only the last is an exists join.
    /// Single-element `And` wrappers inside exists are flattened: `And([P])` → `P`.
    fn desugar_multi_pattern_exists(patterns: &[&Pattern]) -> Vec<Pattern> {
        let mut result = Vec::new();
        for pattern in patterns {
            if let Pattern::Exists(sub_patterns, span) = pattern {
                if sub_patterns.len() > 1 {
                    // Flatten single-element Ands within the exists sub-patterns
                    let flattened: Vec<Pattern> = sub_patterns
                        .iter()
                        .map(|p| {
                            if let Pattern::And(children, _) = p {
                                if children.len() == 1 {
                                    return children[0].clone();
                                }
                            }
                            p.clone()
                        })
                        .collect();

                    // Choose a non-negated anchor for the existential join when
                    // possible; this avoids constructing `exists(not(...))`
                    // artifacts for mixed forms like:
                    //   (exists A B (not ...))
                    // which are better approximated as:
                    //   A (not ...) (exists B)
                    let anchor_idx = flattened
                        .iter()
                        .rposition(|p| !matches!(p, Pattern::Not(..)))
                        .unwrap_or(flattened.len() - 1);

                    for (idx, p) in flattened.iter().enumerate() {
                        if idx != anchor_idx {
                            result.push(p.clone());
                        }
                    }
                    // Wrap the anchor in exists
                    let anchor = flattened[anchor_idx].clone();
                    result.push(Pattern::Exists(vec![anchor], *span));
                    continue;
                }

                if let Some(Pattern::Exists(inner, inner_span)) = sub_patterns.first() {
                    // Collapse redundant nesting so fixed-point passes can desugar
                    // cases like (exists (exists A B C)).
                    result.push(Pattern::Exists(inner.clone(), *inner_span));
                    continue;
                }
            }
            result.push((*pattern).clone());
        }
        result
    }

    fn has_top_level_multi_pattern_exists(patterns: &[Pattern]) -> bool {
        patterns
            .iter()
            .any(|pattern| matches!(pattern, Pattern::Exists(sub, _) if sub.len() > 1))
    }

    /// Try to extract test CEs from nested contexts (negation, NCC, exists)
    /// and add them as rule-level test conditions.
    ///
    /// Returns `true` if the pattern was fully handled (caller should skip it).
    /// Returns `false` if the pattern should be processed normally.
    ///
    /// Handles these cases:
    /// - `(not (test expr))` → adds `(not expr)` to `test_conditions`
    /// - `(not (and (test ...) ...))` where ALL children are test CEs → adds
    ///   negated conjunction to `test_conditions`
    /// - `(exists (test expr))` → adds `expr` to `test_conditions`
    fn try_extract_nested_test_ce(
        &mut self,
        pattern: &Pattern,
        test_conditions: &mut Vec<CompiledTestCondition>,
    ) -> Result<bool, LoadError> {
        match pattern {
            Pattern::Not(inner, _) => {
                match inner.as_ref() {
                    // (not (test expr)) → rule fires when expr is false
                    Pattern::Test(sexpr, _) => {
                        let inner_expr = crate::evaluator::from_sexpr(
                            sexpr,
                            &mut self.symbol_table,
                            &self.config,
                        )
                        .map_err(|e| LoadError::Compile(format!("test CE translation: {e}")))?;
                        let negated = crate::evaluator::RuntimeExpr::Call {
                            name: "not".to_string(),
                            args: vec![inner_expr],
                            span: None,
                        };
                        test_conditions.push(CompiledTestCondition::Expr(negated));
                        Ok(true)
                    }
                    // (not (and (test1) (test2) ...)) where ALL are test CEs
                    Pattern::And(inner_patterns, _)
                        if inner_patterns
                            .iter()
                            .all(|p| matches!(p, Pattern::Test(..))) =>
                    {
                        let mut test_exprs = Vec::with_capacity(inner_patterns.len());
                        for sub in inner_patterns {
                            if let Pattern::Test(sexpr, _) = sub {
                                let expr = crate::evaluator::from_sexpr(
                                    sexpr,
                                    &mut self.symbol_table,
                                    &self.config,
                                )
                                .map_err(|e| {
                                    LoadError::Compile(format!("test CE translation: {e}"))
                                })?;
                                test_exprs.push(expr);
                            }
                        }
                        // Negate the conjunction: not(and(t1, t2, ...))
                        let conjunction = if test_exprs.len() == 1 {
                            test_exprs.into_iter().next().unwrap()
                        } else {
                            crate::evaluator::RuntimeExpr::Call {
                                name: "and".to_string(),
                                args: test_exprs,
                                span: None,
                            }
                        };
                        let negated = crate::evaluator::RuntimeExpr::Call {
                            name: "not".to_string(),
                            args: vec![conjunction],
                            span: None,
                        };
                        test_conditions.push(CompiledTestCondition::Expr(negated));
                        Ok(true)
                    }
                    _ => Ok(false),
                }
            }
            // (exists (test expr)) → rule fires when expr is true
            Pattern::Exists(patterns, _)
                if patterns.len() == 1 && matches!(&patterns[0], Pattern::Test(..)) =>
            {
                if let Pattern::Test(sexpr, _) = &patterns[0] {
                    let expr =
                        crate::evaluator::from_sexpr(sexpr, &mut self.symbol_table, &self.config)
                            .map_err(|e| LoadError::Compile(format!("test CE translation: {e}")))?;
                    test_conditions.push(CompiledTestCondition::Expr(expr));
                }
                Ok(true)
            }
            // (forall (P) (test expr)) where test has no P-local variables:
            // desugar to just the test as a rule-level condition.
            // The forall semantics "for all P, expr holds" reduce to "expr holds"
            // when the test doesn't reference P-local variables.  The condition P
            // is still checked by the NCC that forall desugars into, but if the
            // then-clause is a pure test with no pattern dependencies, we handle it
            // as a rule-level test condition to avoid the NCC needing test support.
            Pattern::Forall(sub_patterns, _)
                if sub_patterns.len() == 2 && matches!(&sub_patterns[1], Pattern::Test(..)) =>
            {
                // The test CE is the then-clause; just add it as a test condition.
                // The condition (P) still needs to be checked, but since forall
                // with a constant test is either always-true or always-false,
                // adding the test as a rule-level condition is semantically correct.
                if let Pattern::Test(sexpr, _) = &sub_patterns[1] {
                    let expr =
                        crate::evaluator::from_sexpr(sexpr, &mut self.symbol_table, &self.config)
                            .map_err(|e| LoadError::Compile(format!("test CE translation: {e}")))?;
                    test_conditions.push(CompiledTestCondition::Expr(expr));
                }
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// Build a runtime negated-pattern check for complex ordered constraints.
    ///
    /// This is a fallback for negated ordered patterns where predicate/return
    /// expressions are too rich to lower into join/alpha tests.
    fn try_build_negated_runtime_check(pattern: &Pattern) -> Option<NegatedPatternRuntimeCheck> {
        match pattern {
            Pattern::Assigned { pattern, .. } => Self::try_build_negated_runtime_check(pattern),
            Pattern::Not(inner, _) => {
                let Pattern::Ordered(ordered) = inner.as_ref() else {
                    return None;
                };

                if !Self::ordered_pattern_has_complex_negated_expression(ordered) {
                    return None;
                }

                Some(NegatedPatternRuntimeCheck {
                    relation: ordered.relation.clone(),
                    constraints: ordered.constraints.clone(),
                })
            }
            _ => None,
        }
    }

    fn ordered_pattern_has_complex_negated_expression(pattern: &OrderedPattern) -> bool {
        pattern.constraints.iter().any(|constraint| {
            let mut slot_variables = HashSet::new();
            Self::collect_slot_constraint_variables(constraint, &mut slot_variables);
            Self::constraint_has_complex_negated_expression(constraint, &slot_variables)
        })
    }

    fn collect_slot_constraint_variables(
        constraint: &Constraint,
        slot_variables: &mut HashSet<String>,
    ) {
        match constraint {
            Constraint::Variable(name, _) | Constraint::MultiVariable(name, _) => {
                slot_variables.insert(name.clone());
            }
            Constraint::And(parts, _) | Constraint::Or(parts, _) => {
                for part in parts {
                    Self::collect_slot_constraint_variables(part, slot_variables);
                }
            }
            // `~?x` does not bind a slot-local variable; keep this strict so we
            // don't mask unsupported unbound-expression diagnostics.
            Constraint::Not(_, _)
            | Constraint::Literal(_)
            | Constraint::Wildcard(_)
            | Constraint::MultiWildcard(_)
            | Constraint::Predicate(_, _)
            | Constraint::ReturnValue(_, _) => {}
        }
    }

    fn constraint_has_complex_negated_expression(
        constraint: &Constraint,
        slot_variables: &HashSet<String>,
    ) -> bool {
        match constraint {
            Constraint::Predicate(expr, _) => {
                !slot_variables.is_empty()
                    && Self::sexpr_references_any_variable(expr, slot_variables)
                    && !Self::is_potentially_lowerable_negated_predicate_expr(expr)
            }
            Constraint::ReturnValue(expr, _) => {
                !slot_variables.is_empty()
                    && Self::sexpr_references_any_variable(expr, slot_variables)
                    && !Self::is_potentially_lowerable_negated_return_value_expr(expr)
            }
            Constraint::And(parts, _) | Constraint::Or(parts, _) => parts
                .iter()
                .any(|part| Self::constraint_has_complex_negated_expression(part, slot_variables)),
            Constraint::Literal(_)
            | Constraint::Variable(_, _)
            | Constraint::MultiVariable(_, _)
            | Constraint::Wildcard(_)
            | Constraint::MultiWildcard(_)
            | Constraint::Not(_, _) => false,
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

    fn is_potentially_lowerable_negated_predicate_expr(expr: &SExpr) -> bool {
        Self::parse_simple_predicate_comparison(expr).is_some()
            || Self::parse_str_compare_predicate_comparison(expr).is_some()
    }

    fn is_potentially_lowerable_negated_return_value_expr(expr: &SExpr) -> bool {
        Self::parse_predicate_operand(expr).is_some()
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
    fn expand_or_patterns(rule: &RuleConstruct) -> Vec<RuleConstruct> {
        // First flatten top-level And/Logical to expose Or patterns
        let mut flat_patterns: Vec<Pattern> = Vec::new();
        for pattern in &rule.patterns {
            match pattern {
                Pattern::And(inner, _) | Pattern::Logical(inner, _) => {
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
            pattern_options.push(Self::pattern_disjunction_options(pattern));
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
    fn pattern_disjunction_options(pattern: &Pattern) -> Vec<Pattern> {
        let expanded = Self::expand_pattern_constraint_disjunctions(pattern);
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
    fn expand_pattern_constraint_disjunctions(pattern: &Pattern) -> Vec<Pattern> {
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
            } => Self::expand_pattern_constraint_disjunctions(inner)
                .into_iter()
                .map(|p| Pattern::Assigned {
                    variable: variable.clone(),
                    pattern: Box::new(p),
                    span: *span,
                })
                .collect(),
            Pattern::Not(inner, span) => Self::expand_pattern_constraint_disjunctions(inner)
                .into_iter()
                .map(|p| Pattern::Not(Box::new(p), *span))
                .collect(),
            Pattern::And(children, span) => Self::expand_child_pattern_product(children)
                .into_iter()
                .map(|combo| Pattern::And(combo, *span))
                .collect(),
            Pattern::Logical(children, span) => Self::expand_child_pattern_product(children)
                .into_iter()
                .map(|combo| Pattern::Logical(combo, *span))
                .collect(),
            Pattern::Exists(children, span) => Self::expand_child_pattern_product(children)
                .into_iter()
                .map(|combo| Pattern::Exists(combo, *span))
                .collect(),
            Pattern::Forall(children, span) => Self::expand_child_pattern_product(children)
                .into_iter()
                .map(|combo| Pattern::Forall(combo, *span))
                .collect(),
            Pattern::Or(children, span) => Self::expand_child_pattern_product(children)
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
            .map(Self::expand_constraint_disjunctions)
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
                Self::expand_constraint_disjunctions(&slot_constraint.constraint)
                    .into_iter()
                    .map(|constraint| SlotConstraint {
                        slot_name: slot_constraint.slot_name.clone(),
                        constraint,
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

    fn expand_child_pattern_product(children: &[Pattern]) -> Vec<Vec<Pattern>> {
        let per_child: Vec<Vec<Pattern>> = children
            .iter()
            .map(Self::expand_pattern_constraint_disjunctions)
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

    /// Translate a `RuleConstruct` (parser types) into a `CompilableRule` (core types).
    fn translate_rule_construct(
        &mut self,
        rule: &RuleConstruct,
    ) -> Result<TranslatedRule, LoadError> {
        let rule_id = self.compiler.allocate_rule_id();
        let mut conditions = Vec::new();
        let mut fact_address_vars = HashMap::new();
        let mut test_conditions: Vec<CompiledTestCondition> = Vec::new();
        let mut multifield_tail_bindings = Vec::new();
        let mut fact_index = 0usize;
        let mut internal_slot_var_seed = 0usize;

        // Flatten top-level Pattern::And and Pattern::Logical into their children.
        // CLIPS treats (and ...) as a grouping CE equivalent to listing sub-patterns directly.
        // (logical ...) is a truth-maintenance wrapper; we strip it (no TMS yet) and treat
        // children as top-level conditions.
        // Also flatten (not (not X)) → X (double negation = exists, approximated as positive).
        let mut flat_patterns: Vec<&Pattern> = Vec::new();
        for pattern in &rule.patterns {
            Self::flatten_pattern(pattern, &mut flat_patterns);
        }

        // Desugar multi-pattern exists into individual patterns:
        // exists(P1, P2, ..., Pn) → P1, P2, ..., Pn-1, exists(Pn)
        // This makes the inner patterns regular joins and only the last is exists,
        // approximating CLIPS' "at most one activation" semantics.
        let mut desugared_patterns: Vec<Pattern> =
            Self::desugar_multi_pattern_exists(&flat_patterns);
        // Nested exists normalizations can leave multi-pattern `exists` CEs as
        // newly emitted top-level patterns. Re-run the pass to fixed-point.
        for _ in 0..32 {
            if !Self::has_top_level_multi_pattern_exists(&desugared_patterns) {
                break;
            }
            let refs: Vec<&Pattern> = desugared_patterns.iter().collect();
            desugared_patterns = Self::desugar_multi_pattern_exists(&refs);
        }
        let pattern_refs: Vec<&Pattern> = desugared_patterns.iter().collect();

        for pattern in &pattern_refs {
            // Test CEs are handled separately: they do not generate alpha/beta
            // nodes in the Rete network, and they do not consume a fact index.
            // Instead they are collected and evaluated at rule-firing time.
            if let Pattern::Test(sexpr, _span) = pattern {
                let runtime_expr =
                    crate::evaluator::from_sexpr(sexpr, &mut self.symbol_table, &self.config)
                        .map_err(|e| LoadError::Compile(format!("test CE translation: {e}")))?;
                test_conditions.push(CompiledTestCondition::Expr(runtime_expr));
                continue;
            }

            // Handle test CEs inside negation and NCC contexts by extracting
            // them as rule-level test conditions.
            if self.try_extract_nested_test_ce(pattern, &mut test_conditions)? {
                continue;
            }

            // Fallback path for complex negated ordered constraints that cannot
            // be lowered to join/alpha tests in the negative network.
            if let Some(runtime_check) = Self::try_build_negated_runtime_check(pattern) {
                test_conditions.push(CompiledTestCondition::NegatedPatternRuntimeCheck(
                    runtime_check,
                ));
                continue;
            }

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

            let mut generated_tests = Vec::new();
            let condition = self.translate_condition(
                pattern,
                &mut generated_tests,
                &mut internal_slot_var_seed,
            )?;
            test_conditions.extend(generated_tests.into_iter().map(CompiledTestCondition::Expr));
            if let Some(name) = var_name {
                if !is_negated && Self::condition_has_fact_address(&condition) {
                    fact_address_vars.insert(name, fact_index);
                }
            }
            if Self::condition_has_fact_address(&condition) {
                Self::collect_multifield_tail_bindings(
                    pattern,
                    fact_index,
                    &mut multifield_tail_bindings,
                );
                fact_index += 1;
            }
            conditions.push(condition);
        }

        // If the condition list is empty (empty-LHS rule or test-only rule), or if the
        // first condition is an NCC (e.g., forall or not/and as the leading CE), inject
        // an implicit (initial-fact) join as the first condition, mirroring CLIPS' built-in
        // (initial-fact) mechanism. Empty-LHS rules implicitly match (initial-fact) after
        // (reset) in CLIPS.
        if conditions.is_empty() || matches!(conditions.first(), Some(CompilableCondition::Ncc(_)))
        {
            let initial_sym = self
                .symbol_table
                .intern_symbol("initial-fact", self.config.string_encoding)
                .map_err(|e| LoadError::Compile(format!("initial-fact symbol: {e}")))?;
            let initial_pattern = CompilablePattern {
                entry_type: AlphaEntryType::OrderedRelation(initial_sym),
                constant_tests: Vec::new(),
                variable_slots: Vec::new(),
                negated_variable_slots: Vec::new(),
                negated: false,
                exists: false,
            };
            conditions.insert(0, CompilableCondition::Pattern(initial_pattern));
        }

        Ok(TranslatedRule {
            rule_id,
            salience: Salience::new(rule.salience),
            conditions,
            fact_address_vars,
            test_conditions,
            multifield_tail_bindings,
        })
    }

    fn condition_has_fact_address(condition: &CompilableCondition) -> bool {
        match condition {
            CompilableCondition::Pattern(pattern) => !pattern.negated && !pattern.exists,
            CompilableCondition::Ncc(_) => false,
        }
    }

    fn collect_multifield_tail_bindings(
        pattern: &Pattern,
        fact_index: usize,
        out: &mut Vec<MultifieldTailBindingHint>,
    ) {
        match pattern {
            Pattern::Assigned { pattern, .. } => {
                Self::collect_multifield_tail_bindings(pattern, fact_index, out);
            }
            Pattern::Ordered(ordered) => {
                let Some((tail_index, Constraint::MultiVariable(name, _))) =
                    ordered.constraints.iter().enumerate().next_back()
                else {
                    return;
                };

                out.push(MultifieldTailBindingHint {
                    name: name.clone(),
                    fact_index,
                    start_slot: tail_index,
                });
            }
            _ => {}
        }
    }

    #[allow(clippy::too_many_lines)]
    fn translate_condition(
        &mut self,
        pattern: &Pattern,
        generated_tests: &mut Vec<crate::evaluator::RuntimeExpr>,
        internal_slot_var_seed: &mut usize,
    ) -> Result<CompilableCondition, LoadError> {
        match pattern {
            Pattern::Assigned { pattern, .. } => {
                self.translate_condition(pattern, generated_tests, internal_slot_var_seed)
            }
            Pattern::Not(inner, span) => {
                match inner.as_ref() {
                    Pattern::And(inner_patterns, _) => {
                        // (not (and P1 P2 ...)) → NCC
                        if inner_patterns.is_empty() {
                            return Err(Self::unsupported_pattern(
                                "not/and",
                                span,
                                "not(and ...) requires at least one inner pattern",
                            ));
                        }
                        let mut subconditions = Vec::with_capacity(inner_patterns.len());
                        for sub in inner_patterns {
                            let condition = self.translate_condition(
                                sub,
                                generated_tests,
                                internal_slot_var_seed,
                            )?;
                            subconditions.push(condition);
                        }
                        Ok(CompilableCondition::Ncc(subconditions))
                    }
                    Pattern::Not(doubly_inner, _) => {
                        // (not (not X)) ≡ (exists X) in CLIPS.
                        // Strip double negation and translate as a positive condition.
                        self.translate_condition(
                            doubly_inner,
                            generated_tests,
                            internal_slot_var_seed,
                        )
                    }
                    _ => {
                        let mut compilable = self.translate_pattern(
                            inner,
                            generated_tests,
                            internal_slot_var_seed,
                            true,
                        )?;
                        compilable.negated = true;
                        Ok(CompilableCondition::Pattern(compilable))
                    }
                }
            }
            Pattern::Forall(sub_patterns, span) => {
                // Phase 3 restriction: exactly 2 sub-patterns (condition + then-clause).
                if sub_patterns.len() != 2 {
                    return Err(Self::unsupported_pattern(
                        "forall",
                        span,
                        &format!(
                            "Phase 3 forall supports exactly one condition and one then-clause, got {} sub-patterns",
                            sub_patterns.len()
                        ),
                    ));
                }

                // Validate sub-patterns are simple (no nested CEs).
                for sub in sub_patterns {
                    match sub {
                        Pattern::Ordered(_) | Pattern::Template(_) => {}
                        Pattern::Forall(_, inner_span) => {
                            return Err(Self::unsupported_pattern(
                                "forall",
                                inner_span,
                                "nested forall is not supported",
                            ));
                        }
                        _ => {
                            return Err(Self::unsupported_pattern(
                                "forall",
                                span,
                                "forall sub-patterns must be simple fact patterns (ordered or template)",
                            ));
                        }
                    }
                }

                // Desugar forall(P, Q) → NCC([P, neg(Q)]).
                // Compile condition (P) as positive pattern.
                let condition = self.translate_pattern(
                    &sub_patterns[0],
                    generated_tests,
                    internal_slot_var_seed,
                    false,
                )?;
                // Compile then-clause (Q) as negated pattern.
                let mut then_clause = self.translate_pattern(
                    &sub_patterns[1],
                    generated_tests,
                    internal_slot_var_seed,
                    true,
                )?;
                then_clause.negated = true;

                Ok(CompilableCondition::Ncc(vec![
                    CompilableCondition::Pattern(condition),
                    CompilableCondition::Pattern(then_clause),
                ]))
            }
            Pattern::And(_, span) => Err(Self::unsupported_pattern(
                "and",
                span,
                "standalone and conditional elements are not supported; use (not (and ...))",
            )),
            Pattern::Logical(_, span) => Err(Self::unsupported_pattern(
                "logical",
                span,
                "logical CE is only supported at top-level of rule LHS",
            )),
            Pattern::Or(_, span) => Err(Self::unsupported_pattern(
                "or",
                span,
                "or CE should have been expanded via rule duplication before reaching translate_condition",
            )),
            _ => Ok(CompilableCondition::Pattern(self.translate_pattern(
                pattern,
                generated_tests,
                internal_slot_var_seed,
                false,
            )?)),
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
                let sym = self.compile_symbol(&ordered.relation)?;
                let entry_type = AlphaEntryType::OrderedRelation(sym);
                let mut constant_tests = Vec::new();
                let mut variable_slots = Vec::new();
                let mut negated_variable_slots = Vec::new();
                let mut seen_variable_slots = HashMap::new();
                let mut slot_runtime_vars = HashMap::new();

                for (i, constraint) in ordered.constraints.iter().enumerate() {
                    let slot = SlotIndex::Ordered(i);
                    self.translate_constraint(
                        constraint,
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

                Ok(CompilablePattern {
                    entry_type,
                    constant_tests,
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
                        .get(&template_id)
                        .cloned()
                        .ok_or_else(|| {
                            Self::compile_error_at(
                                &template.span,
                                &format!("template `{}` not found in registry", template.template),
                            )
                        })?;

                let entry_type = AlphaEntryType::Template(template_id);
                let mut constant_tests = Vec::new();
                let mut variable_slots = Vec::new();
                let mut negated_variable_slots = Vec::new();
                let mut seen_variable_slots = HashMap::new();
                let mut slot_runtime_vars = HashMap::new();

                for slot_constraint in &template.slot_constraints {
                    let slot_idx = registered
                        .slot_index
                        .get(&slot_constraint.slot_name)
                        .copied()
                        .ok_or_else(|| {
                            Self::compile_error_at(
                                &slot_constraint.span,
                                &format!(
                                    "unknown slot `{}` in template `{}`",
                                    slot_constraint.slot_name, template.template
                                ),
                            )
                        })?;

                    let slot = SlotIndex::Template(slot_idx);
                    self.translate_constraint(
                        &slot_constraint.constraint,
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

                Ok(CompilablePattern {
                    entry_type,
                    constant_tests,
                    variable_slots,
                    negated_variable_slots,
                    negated: false,
                    exists: false,
                })
            }
            Pattern::Forall(_, span) => Err(Self::unsupported_pattern(
                "forall",
                span,
                "forall CE reached translate_pattern unexpectedly (should be handled in translate_condition)",
            )),
            Pattern::And(_, span) => Err(Self::unsupported_pattern(
                "and",
                span,
                "and conditional elements are only supported inside (not (and ...))",
            )),
            Pattern::Logical(_, span) => Err(Self::unsupported_pattern(
                "logical",
                span,
                "logical CE reached translate_pattern unexpectedly (should be flattened at top level)",
            )),
            Pattern::Or(_, span) => Err(Self::unsupported_pattern(
                "or",
                span,
                "or CE reached translate_pattern unexpectedly (should be expanded via rule duplication)",
            )),
        }
    }

    /// Translate a single `Constraint` into constant tests and/or variable slots.
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    fn translate_constraint(
        &mut self,
        constraint: &Constraint,
        slot: SlotIndex,
        constant_tests: &mut Vec<ConstantTest>,
        variable_slots: &mut Vec<(SlotIndex, ferric_core::Symbol)>,
        negated_variable_slots: &mut Vec<(SlotIndex, ferric_core::Symbol, JoinTestType)>,
        seen_variable_slots: &mut HashMap<ferric_core::Symbol, SlotIndex>,
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
                // Treat $?var the same as ?var: bind to the value at this slot position.
                // For template slots this is semantically correct (binds to full slot value).
                // For ordered patterns this is an approximation — true CLIPS multi-field
                // matching (spanning multiple positions) is not yet implemented.
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
            Constraint::Predicate(expr, span) => {
                if in_negated_pattern {
                    if self.try_lower_negated_predicate_constraint(
                        expr,
                        slot,
                        constant_tests,
                        variable_slots,
                        negated_variable_slots,
                        seen_variable_slots,
                    )? {
                        return Ok(());
                    }
                    return Err(Self::unsupported_constraint(
                        ":",
                        span,
                        "predicate constraints inside negated patterns currently require a simple binary comparison involving the current slot variable",
                    ));
                }
                let runtime_expr =
                    crate::evaluator::from_sexpr(expr, &mut self.symbol_table, &self.config)
                        .map_err(|e| {
                            LoadError::Compile(format!("predicate constraint translation: {e}"))
                        })?;
                generated_tests.push(runtime_expr);
            }
            Constraint::ReturnValue(expr, span) => {
                if in_negated_pattern {
                    if self.try_lower_negated_return_value_constraint(
                        expr,
                        slot,
                        constant_tests,
                        variable_slots,
                        negated_variable_slots,
                        seen_variable_slots,
                    )? {
                        return Ok(());
                    }
                    return Err(Self::unsupported_constraint(
                        "=",
                        span,
                        "return-value constraints inside negated patterns currently require a simple literal/variable expression or a linear (+/- var integer) form",
                    ));
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

    fn try_lower_negated_predicate_constraint(
        &mut self,
        expr: &SExpr,
        slot: SlotIndex,
        constant_tests: &mut Vec<ConstantTest>,
        variable_slots: &mut Vec<(SlotIndex, ferric_core::Symbol)>,
        negated_variable_slots: &mut Vec<(SlotIndex, ferric_core::Symbol, JoinTestType)>,
        seen_variable_slots: &mut HashMap<ferric_core::Symbol, SlotIndex>,
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
        negated_variable_slots: &mut Vec<(SlotIndex, ferric_core::Symbol, JoinTestType)>,
        seen_variable_slots: &mut HashMap<ferric_core::Symbol, SlotIndex>,
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
        _variable_slots: &mut Vec<(SlotIndex, ferric_core::Symbol)>,
        negated_variable_slots: &mut Vec<(SlotIndex, ferric_core::Symbol, JoinTestType)>,
        seen_variable_slots: &mut HashMap<ferric_core::Symbol, SlotIndex>,
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
        negated_variable_slots: &mut Vec<(SlotIndex, ferric_core::Symbol, JoinTestType)>,
        seen_variable_slots: &mut HashMap<ferric_core::Symbol, SlotIndex>,
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
        _variable_slots: &mut Vec<(SlotIndex, ferric_core::Symbol)>,
        negated_variable_slots: &mut Vec<(SlotIndex, ferric_core::Symbol, JoinTestType)>,
        seen_variable_slots: &mut HashMap<ferric_core::Symbol, SlotIndex>,
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
        seen_variable_slots: &HashMap<ferric_core::Symbol, SlotIndex>,
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
        variable_slots: &mut Vec<(SlotIndex, ferric_core::Symbol)>,
        seen_variable_slots: &mut HashMap<ferric_core::Symbol, SlotIndex>,
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
        variable_slots: &mut Vec<(SlotIndex, ferric_core::Symbol)>,
        seen_variable_slots: &mut HashMap<ferric_core::Symbol, SlotIndex>,
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

    fn duplicate_definition_error(
        construct: &str,
        name: &str,
        span: &ferric_parser::Span,
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
        span: &ferric_parser::Span,
    ) -> LoadError {
        LoadError::Compile(format!(
            "cannot define {new_construct} `{name}`: a {existing_construct} with the same name already exists at line {}, column {}",
            span.start.line, span.start.column
        ))
    }

    fn duplicate_method_index_error(
        generic_name: &str,
        index: i32,
        span: &ferric_parser::Span,
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

    fn compile_error_at(span: &ferric_parser::Span, detail: &str) -> LoadError {
        LoadError::Compile(format!(
            "{detail} at line {}, column {}",
            span.start.line, span.start.column
        ))
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
                push_nesting_depth_error(
                    errors,
                    span,
                    new_depth,
                    max_depth,
                    ferric_core::ValidationStage::ReteCompilation,
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
                    ferric_core::ValidationStage::ReteCompilation,
                );
            }

            // Check for unsupported combination: single-pattern exists containing not.
            // Multi-pattern exists forms are desugared later, and mixed branches
            // like `(exists A (not B))` are handled by that pass.
            let enforce_exists_not_guard = inner_patterns.len() == 1;
            for inner in inner_patterns {
                if enforce_exists_not_guard && matches!(inner, Pattern::Not(..)) {
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
                    ferric_core::ValidationStage::AstInterpretation,
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
    errors: &mut Vec<ferric_core::PatternValidationError>,
    span: &ferric_parser::Span,
    depth: usize,
    max: usize,
    stage: ferric_core::ValidationStage,
) {
    let error = ferric_core::PatternValidationError::new(
        ferric_core::PatternViolation::NestingTooDeep { depth, max },
        Some(span_to_source_location(span)),
        stage,
    );
    errors.push(error);
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
    use crate::test_helpers::{
        find_facts_by_relation, load_err, load_ok, new_utf8_engine, run_to_completion,
    };
    use ferric_core::{Fact, Value};
    use std::collections::HashMap;

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
            assert!(matches!(&ordered.fields[2], Value::String(s) if s.as_str() == "hello"));
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
        let result = engine.load_str("(defrule t ?f <- (x ?y&?x) => (retract ?f))");

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

        let run = run_to_completion(&mut engine);
        assert_eq!(run.rules_fired, 1);
        assert_eq!(find_facts_by_relation(&engine, "missing").len(), 1);
    }

    #[test]
    fn negated_predicate_constraint_falls_back_for_non_linear_expression() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (deffacts startup
              (anchor 1)
              (anchor 3)
              (data 1)
              (data 2))
            (defrule no-square-greater
              (anchor ?min)
              (not (data ?x&:(> (* ?x ?x) (* ?min ?min))))
              =>
              (assert (safe-square ?min)))
            ",
        );

        let run = run_to_completion(&mut engine);
        assert_eq!(run.rules_fired, 1);
        let safe = find_facts_by_relation(&engine, "safe-square");
        assert_eq!(safe.len(), 1);
        let Some(entry) = engine.fact_base.get(safe[0]) else {
            panic!("safe-square fact should exist");
        };
        let Fact::Ordered(ordered) = &entry.fact else {
            panic!("safe-square should be an ordered fact");
        };
        assert_eq!(ordered.fields.len(), 1);
        assert!(matches!(ordered.fields[0], Value::Integer(3)));
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

        let run = run_to_completion(&mut engine);
        assert_eq!(run.rules_fired, 1);
        assert_eq!(find_facts_by_relation(&engine, "missing-nested").len(), 1);
    }

    #[test]
    fn negated_return_value_constraint_falls_back_for_non_linear_expression() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (deffacts startup
              (pair 2)
              (pair 3))
            (defrule no-self-square
              (not (pair ?x&=(* ?x ?x)))
              =>
              (assert (safe-return)))
            ",
        );

        let run = run_to_completion(&mut engine);
        assert_eq!(run.rules_fired, 1);
        assert_eq!(find_facts_by_relation(&engine, "safe-return").len(), 1);
    }

    #[test]
    fn negated_predicate_constraint_still_reports_unsupported_when_slot_variable_not_involved() {
        let mut engine = new_utf8_engine();
        let errors = engine
            .load_str("(defrule t (not (data b&:(> ?x ?y))) => (assert (ok)))")
            .unwrap_err();

        assert!(
            errors.iter().any(|e| matches!(
                e,
                LoadError::Compile(msg) if msg.contains("predicate constraints inside negated patterns currently require")
            )),
            "expected explicit unsupported diagnostic, got: {errors:?}"
        );
    }

    #[test]
    fn negated_return_value_constraint_still_reports_unsupported_when_slot_variable_not_involved() {
        let mut engine = new_utf8_engine();
        let errors = engine
            .load_str("(defrule t (not (pair =(* ?x 2))) => (assert (ok)))")
            .unwrap_err();

        assert!(
            errors.iter().any(|e| matches!(
                e,
                LoadError::Compile(msg) if msg.contains("return-value constraints inside negated patterns currently require")
            )),
            "expected explicit unsupported diagnostic, got: {errors:?}"
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
        let output = engine.get_output("t").unwrap_or("").trim().to_string();
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
        let output = engine.get_output("t").unwrap_or("");
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

        assert_eq!(result.asserted_facts.len(), 2);
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

        assert_eq!(result.asserted_facts.len(), 2);

        let fact_id = result.asserted_facts[1];
        let entry = engine
            .fact_base
            .get(fact_id)
            .expect("asserted fact should exist");
        match &entry.fact {
            ferric_core::Fact::Ordered(ordered) => {
                let relation = engine
                    .resolve_symbol(ordered.relation)
                    .expect("relation symbol should resolve");
                assert_eq!(relation, "foo");
                assert_eq!(ordered.fields.len(), 1);
                let Value::Symbol(field_sym) = ordered.fields[0] else {
                    panic!("expected symbol field, got {:?}", ordered.fields[0]);
                };
                let field = engine
                    .resolve_symbol(field_sym)
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
            matches!(sensor.exports[0], ferric_parser::ModuleSpec::All),
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
    use crate::test_helpers::new_utf8_engine;
    use proptest::prelude::*;

    proptest! {
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
