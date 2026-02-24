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

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::rc::Rc;
use thiserror::Error;

// Qualified name utilities: wired into construct loading in passes 003/004.
#[allow(unused_imports)]
use crate::qualified_name::{parse_qualified_name, QualifiedName};

use ferric_core::{
    AlphaEntryType, AtomKey, CompilableCondition, CompilablePattern, CompileResult, ConstantTest,
    ConstantTestType, FactId, FerricString, RuleId, Salience, SlotIndex, Value,
};
use ferric_parser::{
    interpret_constructs, parse_sexprs, ActionExpr, Atom, Constraint, Construct, FactBody,
    FactValue, FileId, FunctionConstruct, GenericConstruct, GlobalConstruct, InterpretError,
    InterpreterConfig, LiteralKind, MethodConstruct, ModuleConstruct, OrderedFactBody, ParseError,
    Pattern, RuleConstruct, SExpr, TemplateConstruct, TemplateFactBody,
};

use crate::actions::CompiledRuleInfo;
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
    /// Test CE expressions (not compiled into Rete; evaluated at firing time).
    test_conditions: Vec<crate::evaluator::RuntimeExpr>,
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
                match self.compile_rule_construct(rule) {
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
        let template_id = self
            .template_ids
            .get(&template.template)
            .copied()
            .ok_or_else(|| {
                LoadError::Compile(format!(
                    "unknown template `{}` in deffacts",
                    template.template
                ))
            })?;

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
            FactValue::GlobalVariable(_name, span) => {
                Self::warn_at_line(
                    result,
                    span.start.line,
                    "global variables in deffacts not supported, skipping",
                );
                None
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
    fn compile_rule_construct(&mut self, rule: &RuleConstruct) -> Result<CompileResult, LoadError> {
        // Validate patterns first (max nesting depth: 2)
        let validation_errors = validate_rule_patterns(&rule.patterns, 2);
        if !validation_errors.is_empty() {
            return Err(LoadError::Validation(validation_errors));
        }

        // Expand (or ...) CEs via rule duplication: a rule with (or P1 P2) becomes
        // N internal rules, each with one branch substituted. Multiple or CEs produce
        // the Cartesian product.
        let expanded_rules = Self::expand_or_patterns(rule);
        let mut last_result = None;

        for variant in &expanded_rules {
            let result = self.compile_single_rule(variant)?;
            last_result = Some(result);
        }

        // Return the last compile result (all variants share the same name/semantics)
        last_result.ok_or_else(|| LoadError::Compile("empty or-expansion".to_string()))
    }

    fn compile_single_rule(&mut self, rule: &RuleConstruct) -> Result<CompileResult, LoadError> {
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

        let runtime_actions = rule
            .actions
            .iter()
            .map(|action| {
                let expr = ActionExpr::FunctionCall(action.call.clone());
                crate::evaluator::from_action_expr(&expr, &mut self.symbol_table, &self.config).ok()
            })
            .collect();

        // Store rule info for action execution
        let info = CompiledRuleInfo {
            name: rule.name.clone(),
            actions: rule.actions.clone(),
            var_map: compile_result.var_map.clone(),
            fact_address_vars: translated.fact_address_vars,
            salience: Salience::new(rule.salience),
            test_conditions: translated.test_conditions,
            runtime_actions,
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
        test_conditions: &mut Vec<crate::evaluator::RuntimeExpr>,
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
                        .map_err(|e| {
                            LoadError::Compile(format!("test CE translation: {e}"))
                        })?;
                        let negated = crate::evaluator::RuntimeExpr::Call {
                            name: "not".to_string(),
                            args: vec![inner_expr],
                            span: None,
                        };
                        test_conditions.push(negated);
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
                        test_conditions.push(negated);
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
                    let expr = crate::evaluator::from_sexpr(
                        sexpr,
                        &mut self.symbol_table,
                        &self.config,
                    )
                    .map_err(|e| {
                        LoadError::Compile(format!("test CE translation: {e}"))
                    })?;
                    test_conditions.push(expr);
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
                if sub_patterns.len() == 2
                    && matches!(&sub_patterns[1], Pattern::Test(..)) =>
            {
                // The test CE is the then-clause; just add it as a test condition.
                // The condition (P) still needs to be checked, but since forall
                // with a constant test is either always-true or always-false,
                // adding the test as a rule-level condition is semantically correct.
                if let Pattern::Test(sexpr, _) = &sub_patterns[1] {
                    let expr = crate::evaluator::from_sexpr(
                        sexpr,
                        &mut self.symbol_table,
                        &self.config,
                    )
                    .map_err(|e| {
                        LoadError::Compile(format!("test CE translation: {e}"))
                    })?;
                    test_conditions.push(expr);
                }
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// Expand `Pattern::Or` CEs via rule duplication.
    /// Returns a vec of rule variants (1 if no `or` CEs, N*M*... for Cartesian product).
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

        // Check if any top-level pattern is an Or (or assigned wrapping an Or)
        let has_or = flat_patterns.iter().any(|p| {
            matches!(p, Pattern::Or(..))
                || matches!(p, Pattern::Assigned { pattern, .. } if matches!(pattern.as_ref(), Pattern::Or(..)))
        });

        if !has_or {
            return vec![rule.clone()];
        }

        // Build Cartesian product of all or-branches
        let mut pattern_options: Vec<Vec<Pattern>> = Vec::new();
        for pattern in &flat_patterns {
            match pattern {
                Pattern::Or(branches, _) => {
                    pattern_options.push(branches.clone());
                }
                Pattern::Assigned {
                    variable,
                    pattern: inner,
                    span,
                } if matches!(inner.as_ref(), Pattern::Or(..)) => {
                    // ?var <- (or P1 P2) → expand to [?var <- P1, ?var <- P2]
                    if let Pattern::Or(branches, _) = inner.as_ref() {
                        let assigned_branches: Vec<Pattern> = branches
                            .iter()
                            .map(|b| Pattern::Assigned {
                                variable: variable.clone(),
                                pattern: Box::new(b.clone()),
                                span: *span,
                            })
                            .collect();
                        pattern_options.push(assigned_branches);
                    }
                }
                other => {
                    pattern_options.push(vec![other.clone()]);
                }
            }
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

    /// Translate a `RuleConstruct` (parser types) into a `CompilableRule` (core types).
    fn translate_rule_construct(
        &mut self,
        rule: &RuleConstruct,
    ) -> Result<TranslatedRule, LoadError> {
        let rule_id = self.compiler.allocate_rule_id();
        let mut conditions = Vec::new();
        let mut fact_address_vars = HashMap::new();
        let mut test_conditions = Vec::new();
        let mut fact_index = 0usize;

        // Flatten top-level Pattern::And and Pattern::Logical into their children.
        // CLIPS treats (and ...) as a grouping CE equivalent to listing sub-patterns directly.
        // (logical ...) is a truth-maintenance wrapper; we strip it (no TMS yet) and treat
        // children as top-level conditions.
        // Also flatten (not (not X)) → X (double negation = exists, approximated as positive).
        let mut flat_patterns: Vec<&Pattern> = Vec::new();
        for pattern in &rule.patterns {
            Self::flatten_pattern(pattern, &mut flat_patterns);
        }

        for pattern in &flat_patterns {
            // Test CEs are handled separately: they do not generate alpha/beta
            // nodes in the Rete network, and they do not consume a fact index.
            // Instead they are collected and evaluated at rule-firing time.
            if let Pattern::Test(sexpr, _span) = pattern {
                let runtime_expr =
                    crate::evaluator::from_sexpr(sexpr, &mut self.symbol_table, &self.config)
                        .map_err(|e| LoadError::Compile(format!("test CE translation: {e}")))?;
                test_conditions.push(runtime_expr);
                continue;
            }

            // Handle test CEs inside negation and NCC contexts by extracting
            // them as rule-level test conditions.
            if self.try_extract_nested_test_ce(pattern, &mut test_conditions)? {
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
                        let mut subpatterns = Vec::with_capacity(inner_patterns.len());
                        for sub in inner_patterns {
                            let translated = self.translate_pattern(sub)?;
                            subpatterns.push(translated);
                        }
                        Ok(CompilableCondition::Ncc(subpatterns))
                    }
                    Pattern::Not(doubly_inner, _) => {
                        // (not (not X)) ≡ (exists X) in CLIPS.
                        // Strip double negation and translate as a positive condition.
                        self.translate_condition(doubly_inner)
                    }
                    _ => {
                        let mut compilable = self.translate_pattern(inner)?;
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
                let condition = self.translate_pattern(&sub_patterns[0])?;
                // Compile then-clause (Q) as negated pattern.
                let mut then_clause = self.translate_pattern(&sub_patterns[1])?;
                then_clause.negated = true;

                Ok(CompilableCondition::Ncc(vec![condition, then_clause]))
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
            _ => Ok(CompilableCondition::Pattern(
                self.translate_pattern(pattern)?,
            )),
        }
    }

    /// Translate a single `Pattern` into a `CompilablePattern`.
    #[allow(clippy::too_many_lines)] // Template pattern arm adds lines but is clear as-is
    fn translate_pattern(&mut self, pattern: &Pattern) -> Result<CompilablePattern, LoadError> {
        match pattern {
            Pattern::Ordered(ordered) => {
                let sym = self.compile_symbol(&ordered.relation)?;
                let entry_type = AlphaEntryType::OrderedRelation(sym);
                let mut constant_tests = Vec::new();
                let mut variable_slots = Vec::new();
                let mut negated_variable_slots = Vec::new();

                for (i, constraint) in ordered.constraints.iter().enumerate() {
                    let slot = SlotIndex::Ordered(i);
                    self.translate_constraint(
                        constraint,
                        slot,
                        &mut constant_tests,
                        &mut variable_slots,
                        &mut negated_variable_slots,
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
                let template_id = self
                    .template_ids
                    .get(&template.template)
                    .copied()
                    .ok_or_else(|| {
                        Self::unsupported_pattern(
                            "template",
                            &template.span,
                            &format!("unknown template `{}`", template.template),
                        )
                    })?;

                let registered =
                    self.template_defs
                        .get(&template_id)
                        .cloned()
                        .ok_or_else(|| {
                            Self::unsupported_pattern(
                                "template",
                                &template.span,
                                &format!("template `{}` not found in registry", template.template),
                            )
                        })?;

                // Check module visibility for cross-module template references.
                let current_module = self.module_registry.current_module();
                let template_module = self
                    .template_modules
                    .get(&template_id)
                    .copied()
                    .unwrap_or_else(|| self.module_registry.main_module_id());

                if !self.module_registry.is_construct_visible(
                    current_module,
                    template_module,
                    "deftemplate",
                    &template.template,
                ) {
                    return Err(Self::unsupported_pattern(
                        "template",
                        &template.span,
                        &format!(
                            "template `{}` is not visible from module `{}`",
                            template.template,
                            self.module_registry
                                .module_name(current_module)
                                .unwrap_or("?")
                        ),
                    ));
                }

                let entry_type = AlphaEntryType::Template(template_id);
                let mut constant_tests = Vec::new();
                let mut variable_slots = Vec::new();
                let mut negated_variable_slots = Vec::new();

                for slot_constraint in &template.slot_constraints {
                    let slot_idx = registered
                        .slot_index
                        .get(&slot_constraint.slot_name)
                        .copied()
                        .ok_or_else(|| {
                            Self::unsupported_pattern(
                                "template",
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
    #[allow(clippy::too_many_lines)]
    fn translate_constraint(
        &mut self,
        constraint: &Constraint,
        slot: SlotIndex,
        constant_tests: &mut Vec<ConstantTest>,
        variable_slots: &mut Vec<(SlotIndex, ferric_core::Symbol)>,
        negated_variable_slots: &mut Vec<(SlotIndex, ferric_core::Symbol)>,
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
            Constraint::MultiVariable(name, _span) => {
                // Treat $?var the same as ?var: bind to the value at this slot position.
                // For template slots this is semantically correct (binds to full slot value).
                // For ordered patterns this is an approximation — true CLIPS multi-field
                // matching (spanning multiple positions) is not yet implemented.
                let sym = self.compile_symbol(name)?;
                variable_slots.push((slot, sym));
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
                    // ~?x or ~$?x → NotEqual beta join test
                    let sym = self.compile_symbol(name)?;
                    negated_variable_slots.push((slot, sym));
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
                            Constraint::Variable(name, _)
                            | Constraint::MultiVariable(name, _)
                                if !found_var =>
                            {
                                let sym = self.compile_symbol(name)?;
                                variable_slots.push((slot, sym));
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
    use crate::test_helpers::{load_err, load_ok, new_utf8_engine};
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
    fn load_rule_with_multi_pattern_exists_returns_compile_error() {
        let mut engine = new_utf8_engine();
        let errors = engine
            .load_str("(defrule t (exists (a) (b)) => (assert (ok)))")
            .unwrap_err();

        assert_single_compile_error_contains(&errors, "unsupported pattern form `exists`");
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
    fn translate_empty_or_constraint_returns_compile_error() {
        let mut engine = new_utf8_engine();
        let mut constant_tests = Vec::new();
        let mut variable_slots = Vec::new();
        let mut negated_variable_slots = Vec::new();
        let span = test_span(9, 4);
        let constraint = Constraint::Or(Vec::new(), span);

        let error = engine
            .translate_constraint(
                &constraint,
                SlotIndex::Ordered(0),
                &mut constant_tests,
                &mut variable_slots,
                &mut negated_variable_slots,
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
            )
            .unwrap();

        assert_eq!(negated_variable_slots.len(), 1);
        assert_eq!(negated_variable_slots[0].0, SlotIndex::Ordered(0));
    }

    #[test]
    fn translate_negated_literal_constraint_still_compiles() {
        let mut engine = new_utf8_engine();
        let mut constant_tests = Vec::new();
        let mut variable_slots = Vec::new();
        let mut negated_variable_slots = Vec::new();
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
