//! Source code loader for CLIPS-compatible syntax.
//!
//! This module provides functionality to load CLIPS source code from strings
//! or files and convert it into engine-level constructs: Stage 2
//! interpretation of every supported construct, pattern validation, rule
//! compilation into the Rete network, and top-level `(assert ...)` forms.

use ferric_rules_core::RuleId;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;

use crate::qualified_name::{parse_qualified_name, QualifiedName};

use ferric_rules_core::{
    AlphaEntryType, AtomKey, CompilableCondition, CompilablePattern, CompileResult,
    ConditionCompilationPlan, ConstantTest, ConstantTestType, FerricString, InstanceName,
    JoinTestType, Salience, SequenceField, SequencePattern, SequenceSegment, SequenceSource,
    SlotIndex, Value,
};
use ferric_rules_parser::{
    interpret_constructs, parse_sexprs, ActionExpr, Atom, Constraint, Construct, FactBody, FileId,
    FunctionCall, FunctionConstruct, GenericConstruct, GlobalConstruct, InterpretError,
    InterpreterConfig, LiteralKind, MethodConstruct, ModuleConstruct, OrderedPattern, ParseError,
    Pattern, RuleConstruct, SExpr, SlotType, Span, TemplateConstruct,
};

use crate::actions::{CompiledRuleInfo, CompiledTestCondition};
use crate::engine::{Engine, EngineError};
use crate::functions::{insert_module_entry, GenericFunction, UserFunction};
use crate::templates::RegisteredTemplate;
use crate::tracing_support::{ferric_event, ferric_span};
// GenericRegistry accessed via self.generics (field on Engine)

/// Derived name index, rebuilt from definitions when restoring a snapshot.
pub(crate) type TemplateLocalIndex =
    rustc_hash::FxHashMap<Box<str>, smallvec::SmallVec<[ferric_rules_core::TemplateId; 2]>>;

#[derive(Debug)]
pub(crate) enum TemplateLookupError {
    Unknown,
    NotVisible,
    Ambiguous(smallvec::SmallVec<[crate::modules::ModuleId; 2]>),
}

/// Borrowed template lookup state shared by loading and expression queries.
/// Visibility is evaluated against the live module registry on every lookup.
#[derive(Clone, Copy)]
pub(crate) struct TemplateResolver<'a> {
    pub(crate) template_local_ids: &'a TemplateLocalIndex,
    pub(crate) template_modules:
        &'a slotmap::SecondaryMap<ferric_rules_core::TemplateId, crate::modules::ModuleId>,
    pub(crate) module_registry: &'a crate::modules::ModuleRegistry,
}

impl TemplateResolver<'_> {
    /// CLIPS query restrictions use visible unqualified deftemplate names.
    pub(crate) fn resolve_query_reference(
        &self,
        raw_name: &str,
        current_module: crate::modules::ModuleId,
    ) -> Result<ferric_rules_core::TemplateId, String> {
        if raw_name.contains("::") {
            return Err(format!(
                "qualified template `{raw_name}` is unsupported in fact queries"
            ));
        }
        self.resolve_reference(raw_name, current_module)
    }

    pub(crate) fn resolve_reference(
        &self,
        raw_name: &str,
        current_module: crate::modules::ModuleId,
    ) -> Result<ferric_rules_core::TemplateId, String> {
        self.resolve_id(raw_name, current_module).map_err(|error| {
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
    pub(crate) fn resolve_id(
        &self,
        raw_name: &str,
        current_module: crate::modules::ModuleId,
    ) -> Result<ferric_rules_core::TemplateId, TemplateLookupError> {
        let (qualified_module_name, wanted_local_name) = Engine::template_ref_parts(raw_name);
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
}

/// Resolve predicate call names before candidate iteration, including empty
/// queries. A callable being defined can refer to itself before registration.
pub(crate) fn validate_query_callable(
    raw_name: &str,
    functions: &crate::functions::FunctionEnv,
    generics: &crate::functions::GenericRegistry,
    modules: &crate::modules::ModuleRegistry,
    current_module: crate::modules::ModuleId,
    self_name: Option<&str>,
) -> Result<(), String> {
    if self_name == Some(raw_name)
        || raw_name == "call-next-method"
        || crate::evaluator::is_builtin_callable(raw_name)
    {
        return Ok(());
    }
    let unknown =
        || format!("[EXPRNPSR3] query predicate callable `{raw_name}` is not declared or visible");
    let qualified = parse_qualified_name(raw_name).map_err(|_| unknown())?;
    let visible =
        |owner, name: &str, kind| modules.is_construct_visible(current_module, owner, kind, name);
    let (name, requested_module) = match &qualified {
        QualifiedName::Qualified { module, name } => (
            name.as_str(),
            Some(modules.get_by_name(module).ok_or_else(unknown)?),
        ),
        QualifiedName::Unqualified(name) => (name.as_str(), None),
    };
    if let Some(owner) = requested_module {
        if (functions.contains(owner, name) && visible(owner, name, "deffunction"))
            || (generics.contains(owner, name) && visible(owner, name, "defgeneric"))
        {
            return Ok(());
        }
        return Err(unknown());
    }
    if functions.contains(current_module, name) || generics.contains(current_module, name) {
        return Ok(());
    }
    let mut owners: Vec<_> = functions
        .modules_for_name(name)
        .into_iter()
        .filter(|owner| visible(*owner, name, "deffunction"))
        .chain(
            generics
                .modules_for_name(name)
                .into_iter()
                .filter(|owner| visible(*owner, name, "defgeneric")),
        )
        .collect();
    owners.sort_by_key(|owner| owner.0);
    owners.dedup();
    match owners.len() {
        1 => Ok(()),
        0 => Err(unknown()),
        _ => Err(format!(
            "[EXPRNPSR3] query predicate callable `{raw_name}` is ambiguous"
        )),
    }
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

/// A field disjunction compiled as one alpha `Any` test, kept so the pattern
/// can move it to a match-time predicate if its alpha path is over budget.
struct AlphaDisjunction {
    /// Position of the `Any` test in the pattern's constant tests.
    index: usize,
    slot: SlotIndex,
    constraint: Constraint,
    /// Where its predicate goes among the generated tests, in field order.
    generated_at: usize,
}

struct RuleRhsScope<'a> {
    engine: &'a Engine,
    module: crate::modules::ModuleId,
    exported: &'a HashSet<String>,
    existential: &'a HashSet<String>,
    allow_local_reads: bool,
}

/// Definitions are provisionally visible during loading to support forward
/// references. Invalid definitions are retired before rules or facts use them.
#[derive(Default)]
struct PendingCallables {
    originals: HashMap<(crate::modules::ModuleId, String), OriginalCallables>,
    definitions: Vec<PendingCallable>,
}

struct OriginalCallables {
    function: Option<UserFunction>,
    generic: Option<GenericFunction>,
}

struct PendingCallable {
    module: crate::modules::ModuleId,
    definition: CallableDefinition,
    valid: bool,
}

enum CallableDefinition {
    Function(Box<FunctionConstruct>),
    Generic(GenericConstruct),
    Method(Box<MethodConstruct>),
}

impl CallableDefinition {
    fn name(&self) -> &str {
        match self {
            Self::Function(function) => &function.name,
            Self::Generic(generic) => &generic.name,
            Self::Method(method) => &method.name,
        }
    }

    fn referenced_callables(&self, is_template: &dyn Fn(&str) -> bool) -> HashSet<&str> {
        let mut expressions: Vec<_> = match self {
            Self::Function(function) => function.body.iter().collect(),
            Self::Generic(_) => Vec::new(),
            Self::Method(method) => method
                .body
                .iter()
                .chain(
                    method
                        .parameters
                        .iter()
                        .filter_map(|parameter| parameter.query.as_ref()),
                )
                .chain(method.wildcard_query.as_ref())
                .collect(),
        };
        let mut names = HashSet::new();
        while let Some(expression) = expressions.pop() {
            if let ActionExpr::FunctionCall(call) = expression {
                names.insert(call.name.as_str());
                match call.name.as_str() {
                    "assert" => {
                        for argument in &call.args {
                            if let ActionExpr::FunctionCall(fact) = argument {
                                if is_template(&fact.name) {
                                    Self::push_slot_values(&fact.args, &mut expressions);
                                } else {
                                    expressions.extend(&fact.args);
                                }
                            } else {
                                expressions.push(argument);
                            }
                        }
                        continue;
                    }
                    "modify" | "duplicate" => {
                        if let Some((target, slots)) = call.args.split_first() {
                            expressions.push(target);
                            Self::push_slot_values(slots, &mut expressions);
                        }
                        continue;
                    }
                    _ => {}
                }
            }
            expression.push_children(&mut expressions);
        }
        names
    }

    fn push_slot_values<'a>(slots: &'a [ActionExpr], expressions: &mut Vec<&'a ActionExpr>) {
        for slot in slots {
            if let ActionExpr::FunctionCall(slot) = slot {
                expressions.extend(&slot.args);
            } else {
                expressions.push(slot);
            }
        }
    }
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

    #[error("pattern validation failed: {}", .0.iter().map(ToString::to_string).collect::<Vec<_>>().join("; "))]
    Validation(Vec<ferric_rules_core::PatternValidationError>),

    #[error("engine error: {0}")]
    Engine(#[from] EngineError),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// A minimal rule definition stored at S-expression level.
///
/// Captures the raw S-expression structure without Stage 2 interpretation.
/// The loader itself compiles typed Stage 2 constructs; this type is retained
/// only as part of the public API.
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
    /// Functions parsed during loading.
    pub functions: Vec<FunctionConstruct>,
    /// Globals parsed during loading.
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
    fn stage_callable(
        &mut self,
        pending: &mut PendingCallables,
        module: crate::modules::ModuleId,
        definition: CallableDefinition,
    ) {
        let name = definition.name().to_owned();
        pending
            .originals
            .entry((module, name.clone()))
            .or_insert_with(|| OriginalCallables {
                function: self.functions.get(module, &name).cloned(),
                generic: self.generics.get(module, &name).cloned(),
            });
        // Preserve source-order visibility for immediate global initializers.
        // A conflicting candidate stays dormant until final callable validation.
        let _ = self.install_callable(module, &definition);
        pending.definitions.push(PendingCallable {
            module,
            definition,
            valid: true,
        });
    }

    fn install_callable(
        &mut self,
        module: crate::modules::ModuleId,
        definition: &CallableDefinition,
    ) -> Result<(), LoadError> {
        match definition {
            CallableDefinition::Function(function) => {
                if self.generics.contains(module, &function.name) {
                    return Err(Self::construct_conflict_error(
                        "deffunction",
                        "defgeneric",
                        &function.name,
                        &function.span,
                    ));
                }
                self.publish_function(module, function);
            }
            CallableDefinition::Generic(generic) => {
                if self.functions.contains(module, &generic.name) {
                    return Err(Self::construct_conflict_error(
                        "defgeneric",
                        "deffunction",
                        &generic.name,
                        &generic.span,
                    ));
                }
                if self.generics.contains(module, &generic.name) {
                    return Err(Self::duplicate_definition_error(
                        "defgeneric",
                        &generic.name,
                        &generic.span,
                    ));
                }
                insert_module_entry(
                    &mut self.generic_modules,
                    module,
                    generic.name.clone(),
                    module,
                );
                self.generics.register_generic(module, &generic.name);
            }
            CallableDefinition::Method(method) => {
                if self.functions.contains(module, &method.name) {
                    return Err(Self::construct_conflict_error(
                        "defmethod",
                        "deffunction",
                        &method.name,
                        &method.span,
                    ));
                }
                if let Some(index) = method.index {
                    if self.generics.has_method_index(module, &method.name, index) {
                        return Err(Self::duplicate_method_index_error(
                            &method.name,
                            index,
                            &method.span,
                        ));
                    }
                }
                insert_module_entry(
                    &mut self.generic_modules,
                    module,
                    method.name.clone(),
                    module,
                );
                self.generics.register_restricted_method(
                    module,
                    &method.name,
                    method.index,
                    method.parameters.iter().map(|p| p.name.clone()).collect(),
                    method
                        .parameters
                        .iter()
                        .map(|p| p.type_restrictions.clone())
                        .collect(),
                    method.parameters.iter().map(|p| p.query.clone()).collect(),
                    method.wildcard_parameter.clone(),
                    method.wildcard_type_restrictions.clone(),
                    method.wildcard_query.clone(),
                    method.body.clone(),
                );
            }
        }
        Ok(())
    }

    fn publish_function(&mut self, module: crate::modules::ModuleId, function: &FunctionConstruct) {
        insert_module_entry(
            &mut self.function_modules,
            module,
            function.name.clone(),
            module,
        );
        self.functions.register(
            module,
            UserFunction {
                name: function.name.clone(),
                parameters: function.parameters.clone(),
                wildcard_parameter: function.wildcard_parameter.clone(),
                body: function.body.clone(),
            },
        );
    }

    fn restore_original_callables(&mut self, pending: &PendingCallables) {
        for ((module, name), original) in &pending.originals {
            if let Some(entries) = self.functions.functions.get_mut(module) {
                entries.remove(name.as_str());
            }
            if let Some(entries) = self.function_modules.get_mut(module) {
                entries.remove(name.as_str());
            }
            if let Some(entries) = self.generics.generics.get_mut(module) {
                entries.remove(name.as_str());
            }
            if let Some(entries) = self.generic_modules.get_mut(module) {
                entries.remove(name.as_str());
            }
            if let Some(function) = &original.function {
                self.functions.register(*module, function.clone());
                insert_module_entry(&mut self.function_modules, *module, name.clone(), *module);
            }
            if let Some(generic) = &original.generic {
                insert_module_entry(
                    &mut self.generics.generics,
                    *module,
                    name.clone(),
                    generic.clone(),
                );
                insert_module_entry(&mut self.generic_modules, *module, name.clone(), *module);
            }
        }
    }

    fn original_blocks_candidate(pending: &PendingCallables, candidate: &PendingCallable) -> bool {
        let original =
            &pending.originals[&(candidate.module, candidate.definition.name().to_owned())];
        match &candidate.definition {
            CallableDefinition::Function(_) => original.generic.is_some(),
            CallableDefinition::Generic(_) => {
                original.function.is_some() || original.generic.is_some()
            }
            CallableDefinition::Method(method) => {
                original.function.is_some()
                    || original.generic.as_ref().is_some_and(|generic| {
                        method.index.is_some_and(|index| {
                            generic.methods.iter().any(|method| method.index == index)
                        })
                    })
            }
        }
    }

    fn publish_pending_names(&mut self, pending: &PendingCallables) {
        self.restore_original_callables(pending);
        for candidate in &pending.definitions {
            if !candidate.valid || Self::original_blocks_candidate(pending, candidate) {
                continue;
            }
            match &candidate.definition {
                CallableDefinition::Function(function) => {
                    self.publish_function(candidate.module, function);
                }
                definition => {
                    let name = definition.name();
                    insert_module_entry(
                        &mut self.generic_modules,
                        candidate.module,
                        name.to_owned(),
                        candidate.module,
                    );
                    self.generics.register_generic(candidate.module, name);
                }
            }
        }
    }

    fn reject_invalid_candidates(
        &self,
        pending: &mut PendingCallables,
        selected: Option<&HashSet<usize>>,
        errors: &mut Vec<LoadError>,
    ) -> bool {
        let mut rejected = Vec::new();
        for (index, candidate) in pending.definitions.iter().enumerate() {
            if !candidate.valid || selected.is_some_and(|selected| !selected.contains(&index)) {
                continue;
            }
            let result = match &candidate.definition {
                CallableDefinition::Function(function) => self.validate_callable_body(
                    &function.body,
                    candidate.module,
                    "deffunction",
                    &function.name,
                    selected.is_some(),
                ),
                CallableDefinition::Method(method) => {
                    self.validate_method_body(method, candidate.module, selected.is_some())
                }
                CallableDefinition::Generic(_) => Ok(()),
            };
            if let Err(error) = result {
                rejected.push((index, error));
            }
        }
        let changed = !rejected.is_empty();
        let settled = selected.map(|_| self.settled_callable_failures(pending, &rejected));
        for (index, error) in rejected {
            if settled
                .as_ref()
                .map_or(true, |settled| settled.contains(&index))
            {
                pending.definitions[index].valid = false;
                errors.push(error);
            }
        }
        changed
    }

    fn settled_callable_failures(
        &self,
        pending: &PendingCallables,
        rejected: &[(usize, LoadError)],
    ) -> HashSet<usize> {
        let dependencies: Vec<Vec<usize>> = rejected
            .iter()
            .map(|(index, _)| {
                let caller = &pending.definitions[*index];
                let names = caller.definition.referenced_callables(&|name| {
                    self.resolve_template_id(name, caller.module).is_ok()
                });
                rejected
                    .iter()
                    .enumerate()
                    .filter_map(|(position, (other, _))| {
                        let dependency = &pending.definitions[*other];
                        if caller.module == dependency.module
                            && caller.definition.name() == dependency.definition.name()
                        {
                            return None;
                        }
                        names
                            .iter()
                            .any(|name| {
                                self.references_callable_candidate(name, caller.module, dependency)
                            })
                            .then_some(position)
                    })
                    .collect()
            })
            .collect();
        // Failures in a callee may uncover an opposite-kind replacement that
        // repairs its callers. Settle only terminal dependency components;
        // mutually dependent failures retire together without dropping callers.
        let reachable: Vec<HashSet<usize>> = (0..rejected.len())
            .map(|index| {
                let mut visited = HashSet::new();
                let mut queue = vec![index];
                while let Some(next) = queue.pop() {
                    if visited.insert(next) {
                        queue.extend(dependencies[next].iter().copied());
                    }
                }
                visited
            })
            .collect();
        rejected
            .iter()
            .enumerate()
            .filter_map(|(position, (index, _))| {
                reachable[position]
                    .iter()
                    .all(|other| reachable[*other].contains(&position))
                    .then_some(*index)
            })
            .collect()
    }

    fn references_callable_candidate(
        &self,
        raw_name: &str,
        current_module: crate::modules::ModuleId,
        candidate: &PendingCallable,
    ) -> bool {
        match parse_qualified_name(raw_name) {
            Ok(QualifiedName::Qualified { module, name }) => {
                name == candidate.definition.name()
                    && self.module_registry.get_by_name(&module) == Some(candidate.module)
            }
            Ok(QualifiedName::Unqualified(name)) => {
                name == candidate.definition.name()
                    && ["deffunction", "defgeneric"].iter().any(|kind| {
                        self.module_registry.is_construct_visible(
                            current_module,
                            candidate.module,
                            kind,
                            &name,
                        )
                    })
            }
            Err(_) => false,
        }
    }

    fn validate_pending_callables(
        &mut self,
        pending: &mut PendingCallables,
        errors: &mut Vec<LoadError>,
    ) {
        loop {
            // Forward references see every viable candidate name. Do not remove
            // a caller while a later opposite-kind definition can supply its callee.
            loop {
                self.publish_pending_names(pending);
                if !self.reject_invalid_candidates(pending, None, errors) {
                    break;
                }
            }
            self.restore_original_callables(pending);
            let mut selected = HashSet::new();
            let mut conflicts = Vec::new();
            for (index, candidate) in pending.definitions.iter().enumerate() {
                if !candidate.valid {
                    continue;
                }
                match self.install_callable(candidate.module, &candidate.definition) {
                    Ok(()) => {
                        selected.insert(index);
                    }
                    Err(error) => conflicts.push(error),
                }
            }
            // Selecting a kind can change visibility through kind-specific imports.
            // Keep unselected candidates available until this choice is stable.
            if !self.reject_invalid_candidates(pending, Some(&selected), errors) {
                errors.extend(conflicts);
                break;
            }
        }
    }

    fn validate_method_body(
        &self,
        method: &MethodConstruct,
        module: crate::modules::ModuleId,
        validate_visibility: bool,
    ) -> Result<(), LoadError> {
        self.validate_callable_body(
            &method.body,
            module,
            "defmethod",
            &method.name,
            validate_visibility,
        )?;
        let parameters = method
            .parameters
            .iter()
            .map(|parameter| parameter.name.clone())
            .chain(method.wildcard_parameter.iter().cloned())
            .collect();
        self.validate_method_queries_with_visibility(
            &parameters,
            method
                .parameters
                .iter()
                .filter_map(|parameter| parameter.query.as_ref())
                .chain(method.wildcard_query.as_ref()),
            module,
            &method.name,
            validate_visibility,
        )
    }

    #[cfg(feature = "serde")]
    pub(crate) fn validate_method_queries<'a>(
        &self,
        parameters: &HashSet<String>,
        queries: impl IntoIterator<Item = &'a ActionExpr>,
        module: crate::modules::ModuleId,
        name: &str,
    ) -> Result<(), LoadError> {
        self.validate_method_queries_with_visibility(parameters, queries, module, name, true)
    }

    fn validate_method_queries_with_visibility<'a>(
        &self,
        parameters: &HashSet<String>,
        queries: impl IntoIterator<Item = &'a ActionExpr>,
        module: crate::modules::ModuleId,
        name: &str,
        validate_visibility: bool,
    ) -> Result<(), LoadError> {
        let scope = RuleRhsScope {
            engine: self,
            module,
            exported: parameters,
            existential: &HashSet::new(),
            allow_local_reads: true,
        };
        let context = format!("defmethod `{name}`");
        for query in queries {
            crate::evaluator::validate_action_depth(query)
                .map_err(|error| LoadError::Compile(error.to_string()))?;
            crate::callable_validation::validate_breaks_with_templates(
                std::slice::from_ref(query),
                &|name| self.resolve_template_id(name, module).is_ok(),
            )
            .map_err(|(span, message)| Self::compile_error_at(&span, &message))?;
            crate::callable_validation::validate_iterator_binds_with_templates(
                std::slice::from_ref(query),
                &|name| self.resolve_template_id(name, module).is_ok(),
            )
            .map_err(|(span, message)| Self::compile_error_at(&span, &message))?;
            let mut pending = vec![query];
            while let Some(expression) = pending.pop() {
                if let ActionExpr::FunctionCall(call) = expression {
                    if call.name == "bind"
                        && matches!(call.args.first(), Some(ActionExpr::Variable(..)))
                    {
                        return Err(Self::compile_error_at(
                            &call.span,
                            "[GENRCPSR12] Binds are not allowed in query expressions.",
                        ));
                    }
                    // A query runs outside the method body, so CLIPS rejects
                    // `return` anywhere in it.
                    if call.name == "return" {
                        return Err(Self::compile_error_at(
                            &call.span,
                            "[PRCDRPSR2] The return function is not valid in this context.",
                        ));
                    }
                }
                if let ActionExpr::FunctionCall(call) = expression {
                    pending.extend(crate::effects::evaluated_arguments(self, module, call));
                } else {
                    expression.push_children(&mut pending);
                }
            }
            Self::validate_rule_rhs_expr(&context, query, &scope, &mut HashSet::new())?;
            if validate_visibility {
                self.validate_expression_query_declarations(query, module, Some(name))?;
            } else {
                self.validate_expression_query_structure(query, module)?;
            }
            self.validate_action_expr_as_expression(query, module, &context, &HashSet::new())?;
        }
        Ok(())
    }

    fn validate_callable_body(
        &self,
        body: &[ActionExpr],
        module: crate::modules::ModuleId,
        construct: &str,
        name: &str,
        validate_visibility: bool,
    ) -> Result<(), LoadError> {
        let context = format!("{construct} `{name}`");
        crate::callable_validation::validate_breaks_with_templates(body, &|name| {
            self.resolve_template_id(name, module).is_ok()
        })
        .map_err(|(span, message)| Self::compile_error_at(&span, &message))?;
        for expression in body {
            self.validate_action_expr_as_action(expression, module, &context, &HashSet::new())?;
            if validate_visibility {
                self.validate_expression_query_declarations(expression, module, Some(name))?;
            }
        }
        Ok(())
    }

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
    /// - Engine operations fail (e.g., encoding errors)
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
        let previous_depth = self.source_load_depth;
        self.source_load_depth += 1;
        let result = self.load_str_inner(source);
        self.source_load_depth = previous_depth;
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
        // Source offsets at which the current module changes. Each top-level
        // assertion is prepared and evaluated in the module current at its
        // position, as CLIPS does when it executes the form in place.
        let mut module_transitions = vec![(0, self.module_registry.current_module())];

        // Separate top-level (assert ...) forms from constructs; asserts are
        // processed directly after the constructs load.
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
            let mut callable_load = PendingCallables::default();
            let mut declaration_asserts = assert_forms.iter().peekable();
            for construct in interpret_result.constructs {
                let offset = match &construct {
                    Construct::Rule(value) => value.span.start.offset,
                    Construct::Template(value) => value.span.start.offset,
                    Construct::Facts(value) => value.span.start.offset,
                    Construct::Function(value) => value.span.start.offset,
                    Construct::Global(value) => value.span.start.offset,
                    Construct::Module(value) => value.span.start.offset,
                    Construct::Generic(value) => value.span.start.offset,
                    Construct::Method(value) => value.span.start.offset,
                };
                while declaration_asserts
                    .peek()
                    .is_some_and(|expr| expr.span().start.offset < offset)
                {
                    if let Ok(expression) = ferric_rules_parser::interpret_action_expr(
                        declaration_asserts.next().expect("peeked assertion"),
                    ) {
                        self.declare_expression_templates(
                            &expression,
                            self.module_registry.current_module(),
                        );
                    }
                }
                match construct {
                    Construct::Rule(mut rule) => {
                        // Determine the owning module for this rule. If the rule
                        // name is module-qualified (e.g. `MAIN::start`), the
                        // declared module takes precedence over the current module
                        // so that rules like `(defrule MAIN::foo ...)` appearing
                        // inside a `(defmodule REPORT ...)` section still belong
                        // to MAIN for focus-aware dispatch.
                        let qualified = match parse_qualified_name(&rule.name) {
                            Ok(name) => name,
                            Err(error) => {
                                errors.push(Self::compile_error_at(&rule.span, &error));
                                continue;
                            }
                        };
                        let owning_module = if let Some(name) = qualified.module_name() {
                            let Some(module) = self.module_registry.get_by_name(name) else {
                                errors.push(Self::compile_error_at(
                                    &rule.span,
                                    &format!("unknown module `{name}` for rule `{}`", rule.name),
                                ));
                                continue;
                            };
                            module
                        } else {
                            self.module_registry.current_module()
                        };
                        if let Err(error) = self.evaluate_rule_salience(&mut rule, owning_module) {
                            errors.push(error);
                            continue;
                        }
                        self.declare_rule_templates(&rule, owning_module);
                        // Query restrictions must exist where the rule is written;
                        // later declarations must not make an invalid query loadable.
                        if let Err(error) = rule.actions.iter().try_for_each(|action| {
                            crate::effects::evaluated_arguments(self, owning_module, &action.call)
                                .into_iter()
                                .try_for_each(|expr| {
                                    self.validate_expression_query_declarations(
                                        expr,
                                        owning_module,
                                        None,
                                    )
                                })
                        }) {
                            errors.push(error);
                            continue;
                        }
                        rules_with_module.push((rule, owning_module));
                    }
                    Construct::Template(template) => {
                        for slot in &template.slots {
                            if let Some(
                                ferric_rules_parser::DefaultValue::Expressions(expressions)
                                | ferric_rules_parser::DefaultValue::Dynamic(expressions),
                            ) = &slot.default
                            {
                                for expression in expressions {
                                    self.declare_expression_templates(
                                        expression,
                                        self.module_registry.current_module(),
                                    );
                                }
                            }
                        }
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
                        let pending_use =
                            self.template_definition_identity(&template)
                                .ok()
                                .and_then(|(_, id)| id)
                                .is_some_and(|id| {
                                    rules_with_module.iter().any(|(rule, module)| {
                                        self.rule_uses_template(rule, *module, id)
                                    }) || deffacts_constructs.iter().any(|(facts, module)| {
                                        facts.facts.iter().any(|fact| {
                                            self.fact_body_uses_template(fact, *module, id)
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
                                    let relation = match fact {
                                        FactBody::Ordered(fact) => &fact.relation,
                                        FactBody::Template(fact) => &fact.template,
                                    };
                                    self.declare_fact_templates(fact, module);
                                    if self.resolve_template_id(relation, module).is_err() {
                                        pending_ordered_fact_names
                                            .insert(Self::template_local_name(relation));
                                    }
                                }
                                deffacts_constructs.push((facts, module));
                            }
                            Err(error) => errors.push(error),
                        }
                    }
                    Construct::Function(func) => {
                        let declaration_module = self.module_registry.current_module();
                        for expression in &func.body {
                            self.declare_expression_templates(expression, declaration_module);
                        }
                        if let Err(error) = func
                            .body
                            .iter()
                            .try_for_each(crate::evaluator::validate_action_depth)
                        {
                            errors.push(Self::compile_error_at(&func.span, &error.to_string()));
                            continue;
                        }
                        let owning_module = self.module_registry.current_module();
                        if let Err(error) = func.body.iter().try_for_each(|expr| {
                            self.validate_expression_query_structure(expr, owning_module)
                        }) {
                            errors.push(error);
                            continue;
                        }
                        let ordinary = func
                            .parameters
                            .iter()
                            .map(String::as_str)
                            .chain(func.wildcard_parameter.as_deref())
                            .map(|name| Self::existential_scope_variable_name(name).to_owned())
                            .collect();
                        if let Err((span, message)) = crate::query_validation::validate_query_scopes(
                            &func.body,
                            ordinary,
                            &HashSet::new(),
                            self,
                            owning_module,
                        ) {
                            errors.push(Self::compile_error_at(&span, &message));
                            continue;
                        }
                        if let Err((span, message)) =
                            crate::callable_validation::validate_iterator_binds_with_templates(
                                &func.body,
                                &|name| self.resolve_template_id(name, owning_module).is_ok(),
                            )
                        {
                            errors.push(Self::compile_error_at(&span, &message));
                            continue;
                        }
                        self.stage_callable(
                            &mut callable_load,
                            owning_module,
                            CallableDefinition::Function(Box::new(func.clone())),
                        );
                        result.functions.push(func);
                    }
                    Construct::Global(global) => {
                        for definition in &global.globals {
                            self.declare_expression_templates(
                                &definition.value,
                                self.module_registry.current_module(),
                            );
                        }
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
                        module_transitions.push((module.span.start.offset, module_id));
                        result.modules.push(module);
                    }
                    Construct::Generic(generic) => {
                        let owning_module = self.module_registry.current_module();
                        self.stage_callable(
                            &mut callable_load,
                            owning_module,
                            CallableDefinition::Generic(generic.clone()),
                        );
                        result.generics.push(generic);
                    }
                    Construct::Method(method) => {
                        let declaration_module = self.module_registry.current_module();
                        for expression in &method.body {
                            self.declare_expression_templates(expression, declaration_module);
                        }
                        if let Err(error) = method
                            .body
                            .iter()
                            .try_for_each(crate::evaluator::validate_action_depth)
                        {
                            errors.push(Self::compile_error_at(&method.span, &error.to_string()));
                            continue;
                        }
                        let owning_module = self.module_registry.current_module();
                        if let Err(error) = method
                            .body
                            .iter()
                            .chain(
                                method
                                    .parameters
                                    .iter()
                                    .filter_map(|parameter| parameter.query.as_ref()),
                            )
                            .chain(method.wildcard_query.as_ref())
                            .try_for_each(|expr| {
                                self.validate_expression_query_structure(expr, owning_module)
                            })
                        {
                            errors.push(error);
                            continue;
                        }
                        let ordinary = method
                            .parameters
                            .iter()
                            .map(|parameter| parameter.name.as_str())
                            .chain(method.wildcard_parameter.as_deref())
                            .map(|name| Self::existential_scope_variable_name(name).to_owned())
                            .collect();
                        if let Err((span, message)) = crate::query_validation::validate_query_scopes(
                            &method.body,
                            ordinary,
                            &HashSet::new(),
                            self,
                            owning_module,
                        ) {
                            errors.push(Self::compile_error_at(&span, &message));
                            continue;
                        }
                        if let Err((span, message)) =
                            crate::callable_validation::validate_iterator_binds_with_templates(
                                &method.body,
                                &|name| self.resolve_template_id(name, owning_module).is_ok(),
                            )
                        {
                            errors.push(Self::compile_error_at(&span, &message));
                            continue;
                        }
                        self.stage_callable(
                            &mut callable_load,
                            owning_module,
                            CallableDefinition::Method(Box::new(method.clone())),
                        );
                        result.methods.push(method);
                    }
                }
            }

            for expr in declaration_asserts {
                if let Ok(expression) = ferric_rules_parser::interpret_action_expr(expr) {
                    self.declare_expression_templates(
                        &expression,
                        self.module_registry.current_module(),
                    );
                }
            }
            self.validate_pending_callables(&mut callable_load, &mut errors);

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
                errors.push(e.into());
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

        // Construct parsing is complete. A top-level assertion may now build a
        // new construct through a field expression without reentering a loader
        // that still owns provisional construct definitions.
        self.source_load_depth -= 1;
        // Process assert forms AFTER rules are compiled so facts flow through rete.
        // The last defmodule in the source stays current afterwards.
        let loaded_module = self.module_registry.current_module();
        for expr in &assert_forms {
            if let Some(list) = expr.as_list() {
                let offset = expr.span().start.offset;
                let module = module_transitions
                    .iter()
                    .rev()
                    .find(|(start, _)| *start <= offset)
                    .map_or(loaded_module, |(_, module)| *module);
                self.module_registry.set_current_module(module);
                if let Err(e) = self.process_assert(&list[1..], &mut result) {
                    errors.push(e);
                }
            }
        }
        self.module_registry.set_current_module(loaded_module);

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
        _result: &mut LoadResult,
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
            .map(|body| self.prepare_fact_body(body, false))
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
        for construct in interpreted.constructs {
            if let Construct::Facts(definition) = construct {
                for body in definition.facts {
                    let prepared = self.prepare_fact_body(&body, true)?;
                    self.with_active_fact(prepared.identity(), |engine| {
                        let module = engine.module_registry.current_module();
                        let fact = engine
                            .evaluate_prepared_fact(&prepared, module)
                            .map_err(LoadError::Compile)?;
                        engine.assert_fact_internal(fact).map_err(LoadError::from)
                    })?;
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

    fn template_resolver(&self) -> TemplateResolver<'_> {
        TemplateResolver {
            template_local_ids: &self.template_local_ids,
            template_modules: &self.template_modules,
            module_registry: &self.module_registry,
        }
    }

    pub(crate) fn resolve_template_reference(
        &self,
        raw_name: &str,
        current_module: crate::modules::ModuleId,
    ) -> Result<ferric_rules_core::TemplateId, String> {
        self.template_resolver()
            .resolve_reference(raw_name, current_module)
    }

    /// Resolve without allocating diagnostics that ordered-relation probes discard.
    /// Only name candidates are indexed; visibility is always checked live.
    pub(crate) fn resolve_template_id(
        &self,
        raw_name: &str,
        current_module: crate::modules::ModuleId,
    ) -> Result<ferric_rules_core::TemplateId, TemplateLookupError> {
        self.template_resolver()
            .resolve_id(raw_name, current_module)
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

    /// Prepare defaults without installing a partially valid template.
    ///
    /// `redefining` is the definition this construct replaces. CLIPS removes
    /// it before parsing the new body, so it rejects a slot-syntax assertion
    /// or a fact query of it. Ferric rejects any reference to it, including an
    /// ordered-form `(assert (item))` for which CLIPS instead creates a second,
    /// implied template (a Ferric-only rejection). The check runs before this
    /// slot's default is evaluated, leaving the previous definition installed
    /// and redefinable.
    fn template_slot_default(
        &mut self,
        slot: &ferric_rules_parser::SlotDefinition,
        constraints: &crate::slot_constraints::RuntimeSlotConstraints,
        module: crate::modules::ModuleId,
        redefining: Option<ferric_rules_core::TemplateId>,
        result: &mut LoadResult,
    ) -> Result<(Value, Option<crate::templates::DynamicSlotDefault>), LoadError> {
        use ferric_rules_parser::DefaultValue;
        let reject_self_reference = |engine: &Self, compiled: &[crate::evaluator::RuntimeExpr]| {
            if redefining
                .is_some_and(|id| engine.runtime_expressions_use_template(compiled, module, id))
            {
                return Err(Self::compile_error_at(
                    &slot.span,
                    &format!(
                        "default for slot `{}` refers to its own template while that template is being redefined",
                        slot.name
                    ),
                ));
            }
            Ok(())
        };
        let value = match &slot.default {
            Some(DefaultValue::None) => Value::Void,
            Some(DefaultValue::Value(literal)) => self
                .literal_to_value(&literal.value, literal.span.start.line, result)
                .ok_or_else(|| Self::compile_error_at(&literal.span, "invalid template default"))?,
            Some(DefaultValue::Values(literals)) => {
                let values = literals
                    .iter()
                    .map(|literal| {
                        self.literal_to_value(&literal.value, literal.span.start.line, result)
                            .ok_or_else(|| {
                                Self::compile_error_at(&literal.span, "invalid template default")
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Value::Multifield(Box::new(values.into_iter().collect()))
            }
            Some(DefaultValue::Expressions(expressions)) => {
                let compiled = self.prepare_default_expressions(expressions, module)?;
                reject_self_reference(self, &compiled)?;
                self.evaluate_static_default(slot.slot_type, &slot.name, &compiled, module)
                    .map_err(|error| Self::compile_error_at(&slot.span, &error))?
            }
            Some(DefaultValue::Dynamic(expressions)) => {
                let expressions = self.prepare_default_expressions(expressions, module)?;
                reject_self_reference(self, &expressions)?;
                return Ok((
                    Value::Void,
                    Some(crate::templates::DynamicSlotDefault {
                        module,
                        expressions,
                    }),
                ));
            }
            None | Some(DefaultValue::Derive) => crate::slot_constraints::derive_default(
                slot.slot_type,
                slot.allowed_types.as_deref(),
                constraints,
                &mut self.symbol_table,
                self.config.string_encoding,
            )
            .map_err(|error| Self::compile_error_at(&slot.span, &error))?,
        };
        let value = match (slot.slot_type, value) {
            (ferric_rules_parser::SlotType::Multi, Value::Void) => Value::Void,
            (ferric_rules_parser::SlotType::Multi, value @ Value::Multifield(_)) => value,
            (ferric_rules_parser::SlotType::Multi, value) => {
                Value::Multifield(Box::new(std::iter::once(value).collect()))
            }
            (_, value) => value,
        };
        Ok((value, None))
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
        let mut slot_index = HashMap::default();
        slot_index.reserve(slot_count);
        let mut registered = RegisteredTemplate {
            name: template.name.clone(),
            slot_names: Vec::with_capacity(slot_count),
            slot_types: Vec::with_capacity(slot_count),
            allowed_types: Vec::with_capacity(slot_count),
            slot_index,
            defaults: Vec::with_capacity(slot_count),
            constraints: Vec::with_capacity(slot_count),
            dynamic_defaults: Vec::with_capacity(slot_count),
        };
        for (index, slot) in template.slots.iter().enumerate() {
            let constraints = crate::slot_constraints::compile_constraints(
                &slot.constraints,
                &mut self.symbol_table,
                self.config.string_encoding,
            )
            .map_err(|error| Self::compile_error_at(&slot.span, &error))?;
            constraints
                .validate_metadata(
                    slot.allowed_types.as_deref(),
                    slot.slot_type,
                    &self.symbol_table,
                )
                .map_err(|error| Self::compile_error_at(&slot.span, &error))?;
            let (value, dynamic) =
                self.template_slot_default(slot, &constraints, owning_module, existing, result)?;
            // An ordered-form assertion of a new template's own name would
            // stop working once the template is installed. CLIPS 6.30 instead
            // creates a second, implied template, which Ferric does not model.
            if existing.is_none()
                && dynamic.as_ref().is_some_and(|default| {
                    self.dynamic_default_uses_ordered_name(default, &local_name)
                })
            {
                return Err(Self::compile_error_at(
                    &slot.span,
                    &format!(
                        "default for slot `{}` uses its own template `{}` as an ordered relation",
                        slot.name, template.name
                    ),
                ));
            }
            registered.slot_names.push(slot.name.clone());
            registered.slot_index.insert(slot.name.clone(), index);
            registered.slot_types.push(slot.slot_type);
            registered.allowed_types.push(slot.allowed_types.clone());
            registered.defaults.push(value);
            registered.dynamic_defaults.push(dynamic);
            registered.constraints.push(constraints);

            // A failing slot stops definition-time evaluation immediately;
            // later defaults must not produce effects after that error.
            if let Some(ferric_rules_parser::DefaultValue::Dynamic(expressions)) = &slot.default {
                registered
                    .validate_literal_expressions(index, expressions, &self.symbol_table)
                    .map_err(|error| Self::compile_error_at(&slot.span, &error))?;
            } else if !matches!(registered.defaults[index], Value::Void) {
                registered
                    .validate_slot(index, &registered.defaults[index])
                    .map_err(|message| Self::compile_error_at(&slot.span, &message))?;
            }
        }
        // Definition-time expressions may assert facts. Recheck consumers after
        // evaluating defaults so an effect cannot invalidate a live fact layout.
        if existing.is_some_and(|id| self.template_is_in_use(id)) {
            return Err(Self::template_in_use_error(template));
        }
        if existing.is_none() && self.ordered_identity_is_live(&local_name) {
            return Err(Self::ordered_template_conflict(template));
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
        self.declare_explicit_template(&template.name, owning_module);

        Ok(())
    }

    /// Resolve salience at its source declaration, before later constructs exist.
    fn evaluate_rule_salience(
        &mut self,
        rule: &mut RuleConstruct,
        module: crate::modules::ModuleId,
    ) -> Result<(), LoadError> {
        let Some(expression) = rule.salience_expression.as_ref() else {
            return Ok(());
        };
        self.declare_expression_templates(expression, module);
        let runtime = self.prepare_salience_expression(expression, module)?;
        let bindings = ferric_rules_core::binding::BindingSet::new();
        let var_map = ferric_rules_core::binding::VarMap::new();
        let mut locals = crate::evaluator::CallableLocals::default();
        // A `build` at depth continues its caller's evaluation budget.
        let (call_depth, expression_depth) = self.eval_depth_floor;
        let value = {
            let mut context = crate::evaluator::EvalContext {
                engine: self,
                current_module: module,
                global_module: None,
                bindings: &bindings,
                var_map: &var_map,
                callable_locals: Some(&mut locals),
                call_depth,
                expression_depth,
                method_chain: None,
                compact_fact_bindings: None,
                allow_engine_effects: true,
            };
            crate::evaluator::eval(&mut context, &runtime)
        };
        for (channel, text) in self.globals.take_printout_events() {
            self.router.write(&channel, &text);
        }
        let value = value.map_err(|error| {
            Self::compile_error_at(
                &rule.span,
                &format!("rule `{}` salience: {error}", rule.name),
            )
        })?;
        let Value::Integer(value) = value else {
            return Err(Self::compile_error_at(
                &rule.span,
                "[PRNTUTIL10] Salience must evaluate to an integer.",
            ));
        };
        if !(-10_000..=10_000).contains(&value) {
            return Err(Self::compile_error_at(
                &rule.span,
                "[PRNTUTIL9] Salience must be in the range -10000 to 10000.",
            ));
        }
        rule.salience = i32::try_from(value).expect("salience range fits i32");
        Ok(())
    }

    /// Process a `GlobalConstruct`: evaluate each initial value expression and
    /// register it in both the active global store and the snapshot used for reset.
    fn process_global_construct(&mut self, global: &GlobalConstruct) -> Result<(), LoadError> {
        let current_module = self.module_registry.current_module();
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

            crate::callable_validation::validate_breaks_with_templates(
                std::slice::from_ref(&def.value),
                &|name| self.resolve_template_id(name, current_module).is_ok(),
            )
            .map_err(|(span, message)| Self::compile_error_at(&span, &message))?;
            self.validate_expression_query_declarations(&def.value, current_module, None)?;

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
                let empty_bindings = ferric_rules_core::binding::BindingSet::new();
                let empty_var_map = ferric_rules_core::binding::VarMap::new();
                let (call_depth, expression_depth) = self.eval_depth_floor;
                let mut ctx = crate::evaluator::EvalContext {
                    global_module: None,
                    current_module: self.module_registry.current_module(),
                    engine: self,
                    bindings: &empty_bindings,
                    var_map: &empty_var_map,
                    callable_locals: None,
                    call_depth,
                    expression_depth,
                    method_chain: None,
                    compact_fact_bindings: None,
                    allow_engine_effects: true,
                };
                crate::evaluator::eval(&mut ctx, &runtime_expr)
                    .map_err(|e| LoadError::Compile(format!("global `{}` init: {e}", def.name)))?
            };

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
            LiteralKind::InstanceName(s) => self.warned_instance_name_value(s, line, result),
        }
    }

    /// Process an `(assert ...)` form.
    ///
    /// Like CLIPS, every fact is parsed before any is evaluated, so a static
    /// error in a later fact asserts nothing. Evaluation errors stop the
    /// command and keep the facts already asserted.
    fn process_assert(&mut self, args: &[SExpr], result: &mut LoadResult) -> Result<(), LoadError> {
        if args.is_empty() {
            return Err(LoadError::InvalidAssert(
                "assert requires at least one fact".to_owned(),
            ));
        }
        let prepared = args
            .iter()
            .map(|fact_expr| self.prepare_assertion(fact_expr))
            .collect::<Result<Vec<_>, _>>()?;
        let module = self.module_registry.current_module();
        // Like CLIPS's parsed command, the whole assertion keeps every fact's
        // template or relation, and whatever its fields name, in use until
        // the last fact is published: a `build` in an earlier fact cannot
        // redefine a later one (CSTRCPSR4).
        let identities: Vec<_> = prepared
            .iter()
            .map(crate::fact_initializer::PreparedFact::identity)
            .collect();
        let expressions: Vec<_> = prepared
            .iter()
            .flat_map(crate::fact_initializer::PreparedFact::expressions)
            .cloned()
            .map(std::sync::Arc::new)
            .collect();
        self.with_active_expressions(module, expressions, |engine| {
            engine.with_active_facts(identities, |engine| {
                let mut locals = crate::evaluator::CallableLocals::default();
                for fact in &prepared {
                    // Each fact is evaluated completely before it is published.
                    let fact = engine
                        .evaluate_prepared_fact_with_locals(fact, module, &mut locals)
                        .map_err(LoadError::InvalidAssert)?;
                    let fact_id = engine.assert_fact_internal(fact)?.fact_id();
                    result.asserted_facts.push(engine.host.export(fact_id));
                }
                Ok(())
            })
        })
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

    fn warned_instance_name_value(
        &mut self,
        name: &str,
        line: u32,
        result: &mut LoadResult,
    ) -> Option<Value> {
        match self.warned_symbol_value(name, line, result)? {
            Value::Symbol(symbol) => Some(Value::InstanceName(InstanceName::from_symbol(symbol))),
            _ => None,
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

        // Pre-process: distribute or CEs inside NCC/exists contexts.
        // This transforms patterns like (not (and A (or B C))) into
        // (and (not (and A B)) (not (and A C))) which can then be flattened.
        let rule = Self::normalize_nested_or_ces(rule);
        crate::source_limits::check_expansion(&rule, expansion_budget)?;

        // Expand (or ...) CEs via rule duplication: a rule with (or P1 P2) becomes
        // N internal rules, each with one branch substituted. Multiple or CEs produce
        // the Cartesian product.
        let expanded_rules = Self::expand_or_patterns(&rule);
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
        self.rule_declarations.push((module, local_name));
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
        crate::callable_validation::validate_action_breaks_with_templates(&rule.actions, &|name| {
            self.resolve_template_id(name, current_module).is_ok()
        })
        .map_err(|(span, message)| Self::compile_error_at(&span, &message))?;
        self.validate_pattern_builtin_calls(&rule.patterns, current_module)?;
        let context = format!("rule `{}`", rule.name);
        for action in &rule.actions {
            self.validate_rule_action_call(
                &action.call,
                current_module,
                &context,
                &HashSet::new(),
            )?;
        }
        Ok(())
    }

    fn validate_pattern_builtin_calls(
        &self,
        patterns: &[Pattern],
        module: crate::modules::ModuleId,
    ) -> Result<(), LoadError> {
        let mut pending: Vec<_> = patterns.iter().collect();
        let mut constraints = Vec::new();
        let mut expressions = Vec::new();
        while let Some(pattern) = pending.pop() {
            match pattern {
                Pattern::Ordered(pattern) => constraints.extend(&pattern.constraints),
                Pattern::Template(pattern) => {
                    constraints.extend(
                        pattern
                            .slot_constraints
                            .iter()
                            .flat_map(|slot| &slot.constraints),
                    );
                }
                Pattern::Test(expression, _) => expressions.push(expression),
                Pattern::Not(inner, _) | Pattern::Assigned { pattern: inner, .. } => {
                    pending.push(inner);
                }
                Pattern::And(children, _)
                | Pattern::Or(children, _)
                | Pattern::Exists(children, _)
                | Pattern::Forall(children, _)
                | Pattern::Logical(children, _) => pending.extend(children),
            }
        }
        while let Some(constraint) = constraints.pop() {
            match constraint {
                Constraint::Predicate(expression, _) | Constraint::ReturnValue(expression, _) => {
                    expressions.push(expression);
                }
                Constraint::Not(inner, _) => constraints.push(inner),
                Constraint::And(children, _) | Constraint::Or(children, _) => {
                    constraints.extend(children);
                }
                _ => {}
            }
        }
        for expression in expressions {
            let expression = ferric_rules_parser::interpret_action_expr(expression)
                .map_err(LoadError::Interpret)?;
            self.validate_sequence_expansion(&expression, module, true, false)?;
            let mut pending = vec![&expression];
            while let Some(expression) = pending.pop() {
                if let ActionExpr::FunctionCall(call) = expression {
                    crate::builtin_validation::validate_call(call)
                        .map_err(|message| Self::compile_error_at(&call.span, &message))?;
                    pending.extend(crate::effects::evaluated_arguments(self, module, call));
                } else {
                    expression.push_children(&mut pending);
                }
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_lines)] // Keeps action-specific argument roles in one dispatch.
    fn validate_rule_action_call(
        &self,
        call: &FunctionCall,
        current_module: crate::modules::ModuleId,
        context: &str,
        query_members: &HashSet<String>,
    ) -> Result<(), LoadError> {
        if call.name == "expand$" {
            return Err(Self::compile_error_at(
                &call.span,
                "[EXPRNPSR4] sequence expansion is not valid as a body action",
            ));
        }
        let expansion_allowed = Self::allows_sequence_arguments(&call.name);
        for argument in crate::effects::evaluated_arguments(self, current_module, call) {
            self.validate_sequence_expansion(
                argument,
                current_module,
                expansion_allowed,
                Self::is_rule_action_wrapper(&call.name),
            )?;
        }
        crate::builtin_validation::validate_call(call)
            .map_err(|message| Self::compile_error_at(&call.span, &message))?;
        Self::validate_query_member_rebinding(call, query_members)?;
        match call.name.as_str() {
            "refresh-agenda" => Err(Self::compile_error_at(
                &call.span,
                "refresh-agenda is unsupported: only definition-time salience is supported",
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
                            let slots = registered
                                .slot_overrides(&fact_pattern.args, &self.symbol_table)
                                .map_err(|message| {
                                    Self::compile_error_at(&fact_pattern.span, &message)
                                })?;
                            for index in 0..registered.defaults.len() {
                                if registered.requires_value(index)
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
                                        context,
                                        query_members,
                                    )?;
                                }
                            }
                        } else {
                            for field_expr in &fact_pattern.args {
                                self.validate_action_expr_as_expression(
                                    field_expr,
                                    current_module,
                                    context,
                                    query_members,
                                )?;
                            }
                        }
                    } else {
                        self.validate_action_expr_as_expression(
                            arg,
                            current_module,
                            context,
                            query_members,
                        )?;
                    }
                }
                Ok(())
            }
            // `(modify ?f (slot value) ...)` / `(duplicate ?f (slot value) ...)`:
            // slot names are data, but slot values are expressions.
            "modify" | "duplicate" => {
                if let Some(target) = call.args.first() {
                    self.validate_action_expr_as_expression(
                        target,
                        current_module,
                        context,
                        query_members,
                    )?;
                }
                for slot_override in call.args.iter().skip(1) {
                    if let ActionExpr::FunctionCall(slot_pair) = slot_override {
                        for value_expr in &slot_pair.args {
                            self.validate_action_expr_as_expression(
                                value_expr,
                                current_module,
                                context,
                                query_members,
                            )?;
                        }
                    } else {
                        self.validate_action_expr_as_expression(
                            slot_override,
                            current_module,
                            context,
                            query_members,
                        )?;
                    }
                }
                Ok(())
            }
            name if Self::is_rule_action_wrapper(name) => {
                for arg in &call.args {
                    self.validate_action_expr_as_action(
                        arg,
                        current_module,
                        context,
                        query_members,
                    )?;
                }
                Ok(())
            }
            name if Self::is_rule_action_builtin(name) => {
                for arg in &call.args {
                    self.validate_action_expr_as_expression(
                        arg,
                        current_module,
                        context,
                        query_members,
                    )?;
                }
                Ok(())
            }
            _ => {
                self.validate_expression_callable_name(
                    &call.name,
                    &call.span,
                    current_module,
                    context,
                )?;
                for arg in &call.args {
                    self.validate_action_expr_as_expression(
                        arg,
                        current_module,
                        context,
                        query_members,
                    )?;
                }
                Ok(())
            }
        }
    }

    #[allow(clippy::too_many_lines)] // Mirrors every structured expression and its query scope.
    pub(crate) fn validate_action_expr_as_expression(
        &self,
        expr: &ActionExpr,
        current_module: crate::modules::ModuleId,
        context: &str,
        query_members: &HashSet<String>,
    ) -> Result<(), LoadError> {
        match expr {
            ActionExpr::Literal(_)
            | ActionExpr::Variable(_, _)
            | ActionExpr::GlobalVariable(_, _) => Ok(()),
            ActionExpr::FunctionCall(call) => {
                if crate::effects::is_effect(&call.name) {
                    return self.validate_rule_action_call(
                        call,
                        current_module,
                        context,
                        query_members,
                    );
                }
                Self::validate_query_member_rebinding(call, query_members)?;
                self.validate_expression_callable_name(
                    &call.name,
                    &call.span,
                    current_module,
                    context,
                )?;
                crate::builtin_validation::validate_call(call)
                    .map_err(|message| Self::compile_error_at(&call.span, &message))?;
                for arg in &call.args {
                    self.validate_action_expr_as_expression(
                        arg,
                        current_module,
                        context,
                        query_members,
                    )?;
                }
                Ok(())
            }
            ActionExpr::If {
                condition,
                then_actions,
                else_actions,
                ..
            } => {
                self.validate_action_expr_as_expression(
                    condition,
                    current_module,
                    context,
                    query_members,
                )?;
                for action in then_actions {
                    self.validate_action_expr_as_expression(
                        action,
                        current_module,
                        context,
                        query_members,
                    )?;
                }
                for action in else_actions {
                    self.validate_action_expr_as_expression(
                        action,
                        current_module,
                        context,
                        query_members,
                    )?;
                }
                Ok(())
            }
            ActionExpr::While {
                condition, body, ..
            } => {
                self.validate_action_expr_as_expression(
                    condition,
                    current_module,
                    context,
                    query_members,
                )?;
                for action in body {
                    self.validate_action_expr_as_expression(
                        action,
                        current_module,
                        context,
                        query_members,
                    )?;
                }
                Ok(())
            }
            ActionExpr::LoopForCount {
                start, end, body, ..
            } => {
                self.validate_action_expr_as_expression(
                    start,
                    current_module,
                    context,
                    query_members,
                )?;
                self.validate_action_expr_as_expression(
                    end,
                    current_module,
                    context,
                    query_members,
                )?;
                for action in body {
                    self.validate_action_expr_as_expression(
                        action,
                        current_module,
                        context,
                        query_members,
                    )?;
                }
                Ok(())
            }
            ActionExpr::Progn {
                list_expr, body, ..
            } => {
                self.validate_action_expr_as_expression(
                    list_expr,
                    current_module,
                    context,
                    query_members,
                )?;
                for action in body {
                    self.validate_action_expr_as_expression(
                        action,
                        current_module,
                        context,
                        query_members,
                    )?;
                }
                Ok(())
            }
            ActionExpr::QueryAction {
                name,
                bindings,
                query,
                body,
                span,
            } => {
                self.validate_query_declaration(name, bindings, body, span, current_module)?;
                self.validate_query_predicate_bindings(query, current_module)?;
                let mut nested_members = query_members.clone();
                nested_members.extend(bindings.iter().map(|(name, _)| name.clone()));
                self.validate_action_expr_as_expression(
                    query,
                    current_module,
                    context,
                    &nested_members,
                )?;
                for action in body {
                    self.validate_action_expr_as_expression(
                        action,
                        current_module,
                        context,
                        &nested_members,
                    )?;
                }
                Ok(())
            }
            ActionExpr::Switch {
                expr,
                cases,
                default,
                ..
            } => {
                self.validate_action_expr_as_expression(
                    expr,
                    current_module,
                    context,
                    query_members,
                )?;
                for (case_expr, actions) in cases {
                    self.validate_action_expr_as_expression(
                        case_expr,
                        current_module,
                        context,
                        query_members,
                    )?;
                    for action in actions {
                        self.validate_action_expr_as_expression(
                            action,
                            current_module,
                            context,
                            query_members,
                        )?;
                    }
                }
                if let Some(default_actions) = default {
                    for action in default_actions {
                        self.validate_action_expr_as_expression(
                            action,
                            current_module,
                            context,
                            query_members,
                        )?;
                    }
                }
                Ok(())
            }
        }
    }

    #[allow(clippy::too_many_lines)] // Mirrors every structured RHS scope in ActionExpr.
    fn validate_action_expr_as_action(
        &self,
        expr: &ActionExpr,
        current_module: crate::modules::ModuleId,
        context: &str,
        query_members: &HashSet<String>,
    ) -> Result<(), LoadError> {
        match expr {
            ActionExpr::Literal(_)
            | ActionExpr::Variable(_, _)
            | ActionExpr::GlobalVariable(_, _) => Ok(()),
            ActionExpr::FunctionCall(call) => {
                self.validate_rule_action_call(call, current_module, context, query_members)
            }
            ActionExpr::If {
                condition,
                then_actions,
                else_actions,
                ..
            } => {
                self.validate_action_expr_as_expression(
                    condition,
                    current_module,
                    context,
                    query_members,
                )?;
                for action in then_actions {
                    self.validate_action_expr_as_action(
                        action,
                        current_module,
                        context,
                        query_members,
                    )?;
                }
                for action in else_actions {
                    self.validate_action_expr_as_action(
                        action,
                        current_module,
                        context,
                        query_members,
                    )?;
                }
                Ok(())
            }
            ActionExpr::While {
                condition, body, ..
            } => {
                self.validate_action_expr_as_expression(
                    condition,
                    current_module,
                    context,
                    query_members,
                )?;
                for action in body {
                    self.validate_action_expr_as_action(
                        action,
                        current_module,
                        context,
                        query_members,
                    )?;
                }
                Ok(())
            }
            ActionExpr::LoopForCount {
                start, end, body, ..
            } => {
                self.validate_action_expr_as_expression(
                    start,
                    current_module,
                    context,
                    query_members,
                )?;
                self.validate_action_expr_as_expression(
                    end,
                    current_module,
                    context,
                    query_members,
                )?;
                for action in body {
                    self.validate_action_expr_as_action(
                        action,
                        current_module,
                        context,
                        query_members,
                    )?;
                }
                Ok(())
            }
            ActionExpr::Progn {
                list_expr, body, ..
            } => {
                self.validate_action_expr_as_expression(
                    list_expr,
                    current_module,
                    context,
                    query_members,
                )?;
                for action in body {
                    self.validate_action_expr_as_action(
                        action,
                        current_module,
                        context,
                        query_members,
                    )?;
                }
                Ok(())
            }
            ActionExpr::QueryAction {
                name,
                bindings,
                query,
                body,
                span,
            } => {
                if Self::is_result_query(name) {
                    return self.validate_action_expr_as_expression(
                        expr,
                        current_module,
                        context,
                        query_members,
                    );
                }
                self.validate_query_declaration(name, bindings, body, span, current_module)?;
                self.validate_query_predicate_bindings(query, current_module)?;
                let mut nested_members = query_members.clone();
                nested_members.extend(bindings.iter().map(|(name, _)| name.clone()));
                self.validate_action_expr_as_expression(
                    query,
                    current_module,
                    context,
                    &nested_members,
                )?;
                for action in body {
                    self.validate_action_expr_as_action(
                        action,
                        current_module,
                        context,
                        &nested_members,
                    )?;
                }
                Ok(())
            }
            ActionExpr::Switch {
                expr,
                cases,
                default,
                ..
            } => {
                self.validate_action_expr_as_expression(
                    expr,
                    current_module,
                    context,
                    query_members,
                )?;
                for (case_expr, actions) in cases {
                    self.validate_action_expr_as_expression(
                        case_expr,
                        current_module,
                        context,
                        query_members,
                    )?;
                    for action in actions {
                        self.validate_action_expr_as_action(
                            action,
                            current_module,
                            context,
                            query_members,
                        )?;
                    }
                }
                if let Some(default_actions) = default {
                    for action in default_actions {
                        self.validate_action_expr_as_action(
                            action,
                            current_module,
                            context,
                            query_members,
                        )?;
                    }
                }
                Ok(())
            }
        }
    }

    fn is_result_query(name: &str) -> bool {
        matches!(name, "any-factp" | "find-fact" | "find-all-facts")
    }

    fn validate_query_declaration(
        &self,
        name: &str,
        bindings: &[(String, String)],
        body: &[ActionExpr],
        span: &Span,
        current_module: crate::modules::ModuleId,
    ) -> Result<(), LoadError> {
        if bindings.is_empty() {
            return Err(Self::compile_error_at(
                span,
                "fact queries require at least one member",
            ));
        }
        if Self::is_result_query(name) && !body.is_empty() {
            return Err(Self::compile_error_at(
                span,
                "result fact queries cannot have body actions",
            ));
        }
        let mut names = HashSet::new();
        for (member, template) in bindings {
            if member.is_empty() || !names.insert(member) {
                return Err(Self::compile_error_at(
                    span,
                    "fact queries require distinct named single-field members",
                ));
            }
            self.template_resolver()
                .resolve_query_reference(template, current_module)
                .map_err(|message| Self::compile_error_at(span, &message))?;
        }
        Ok(())
    }

    /// Validate query declarations inside callable/global expressions without
    /// changing the existing declaration policy for ordinary function calls.
    pub(crate) fn validate_expression_query_declarations(
        &self,
        expr: &ActionExpr,
        current_module: crate::modules::ModuleId,
        self_name: Option<&str>,
    ) -> Result<(), LoadError> {
        self.validate_expression_query_structure(expr, current_module)?;
        let mut pending = vec![expr];
        while let Some(expr) = pending.pop() {
            if let ActionExpr::QueryAction { query, .. } = expr {
                self.validate_query_predicate_callables(query, current_module, self_name)?;
            }
            if let ActionExpr::FunctionCall(call) = expr {
                pending.extend(crate::effects::evaluated_arguments(
                    self,
                    current_module,
                    call,
                ));
            } else {
                expr.push_children(&mut pending);
            }
        }
        Ok(())
    }

    fn allows_sequence_arguments(name: &str) -> bool {
        !matches!(
            name,
            "progn" | "return" | "expand$" | "assert" | "modify" | "duplicate"
        )
    }

    /// Explicit sequence operators are arguments of ordinary calls, never
    /// body actions, control-form operands, or fact/slot syntax fields.
    fn validate_sequence_expansion(
        &self,
        expression: &ActionExpr,
        module: crate::modules::ModuleId,
        root_allowed: bool,
        root_return_allowed: bool,
    ) -> Result<(), LoadError> {
        let mut pending = vec![(expression, root_allowed, root_return_allowed)];
        while let Some((expression, allowed, return_allowed)) = pending.pop() {
            if let ActionExpr::FunctionCall(call) = expression {
                if call.name == "expand$" && !allowed {
                    return Err(Self::compile_error_at(
                        &call.span,
                        "[EXPRNPSR4] sequence operator is not valid in this argument position",
                    ));
                }
                if call.name == "return" && !return_allowed {
                    return Err(Self::compile_error_at(
                        &call.span,
                        "[PRCDRPSR2] return is not valid inside an argument expression",
                    ));
                }
                let children_allowed = Self::allows_sequence_arguments(&call.name);
                let children_return_allowed = call.name == "progn" && return_allowed;
                pending.extend(
                    crate::effects::evaluated_arguments(self, module, call)
                        .into_iter()
                        .map(|child| (child, children_allowed, children_return_allowed)),
                );
            } else {
                let mut body = Vec::new();
                let mut operands: Vec<&ActionExpr> = Vec::new();
                // Conditions, bounds, selectors, and query predicates are
                // expression operands rather than procedural body positions.
                match expression {
                    ActionExpr::If {
                        condition,
                        then_actions,
                        else_actions,
                        ..
                    } => {
                        operands.push(condition);
                        body.extend(then_actions.iter().chain(else_actions));
                    }
                    ActionExpr::While {
                        condition,
                        body: actions,
                        ..
                    } => {
                        operands.push(condition);
                        body.extend(actions);
                    }
                    ActionExpr::LoopForCount {
                        start,
                        end,
                        body: actions,
                        ..
                    } => {
                        operands.extend([start.as_ref(), end.as_ref()]);
                        body.extend(actions);
                    }
                    ActionExpr::Progn {
                        list_expr,
                        body: actions,
                        ..
                    } => {
                        operands.push(list_expr);
                        body.extend(actions);
                    }
                    ActionExpr::QueryAction {
                        query,
                        body: actions,
                        ..
                    } => {
                        operands.push(query);
                        body.extend(actions);
                    }
                    ActionExpr::Switch {
                        expr,
                        cases,
                        default,
                        ..
                    } => {
                        operands.push(expr);
                        for (selector, actions) in cases {
                            operands.push(selector);
                            body.extend(actions);
                        }
                        body.extend(default.iter().flatten());
                    }
                    _ => {}
                }
                pending.extend(body.into_iter().map(|child| (child, false, return_allowed)));
                pending.extend(operands.into_iter().map(|child| (child, false, false)));
            }
        }
        Ok(())
    }

    // Templates must exist at the definition site. Callable names in staged
    // definitions are checked after recovery settles their kind and visibility.
    fn validate_expression_query_structure(
        &self,
        expr: &ActionExpr,
        current_module: crate::modules::ModuleId,
    ) -> Result<(), LoadError> {
        self.validate_sequence_expansion(expr, current_module, true, true)?;
        let mut pending = vec![expr];
        while let Some(expr) = pending.pop() {
            if let ActionExpr::QueryAction {
                name,
                bindings,
                query,
                body,
                span,
            } = expr
            {
                self.validate_query_declaration(name, bindings, body, span, current_module)?;
                self.validate_query_predicate_bindings(query, current_module)?;
            }
            if let ActionExpr::FunctionCall(call) = expr {
                crate::builtin_validation::validate_call(call)
                    .map_err(|message| Self::compile_error_at(&call.span, &message))?;
                pending.extend(crate::effects::evaluated_arguments(
                    self,
                    current_module,
                    call,
                ));
            } else {
                expr.push_children(&mut pending);
            }
        }
        Ok(())
    }

    fn validate_query_predicate_callables(
        &self,
        expr: &ActionExpr,
        current_module: crate::modules::ModuleId,
        self_name: Option<&str>,
    ) -> Result<(), LoadError> {
        let mut pending = vec![expr];
        while let Some(expr) = pending.pop() {
            if let ActionExpr::FunctionCall(call) = expr {
                validate_query_callable(
                    &call.name,
                    &self.functions,
                    &self.generics,
                    &self.module_registry,
                    current_module,
                    self_name,
                )
                .map_err(|message| Self::compile_error_at(&call.span, &message))?;
            }
            if let ActionExpr::FunctionCall(call) = expr {
                pending.extend(crate::effects::evaluated_arguments(
                    self,
                    current_module,
                    call,
                ));
            } else {
                expr.push_children(&mut pending);
            }
        }
        Ok(())
    }

    /// CLIPS disallows local bind syntax anywhere in a query predicate; a
    /// called function's body belongs to its own scope and is not inspected.
    fn validate_query_predicate_bindings(
        &self,
        expr: &ActionExpr,
        current_module: crate::modules::ModuleId,
    ) -> Result<(), LoadError> {
        let mut pending = vec![expr];
        while let Some(expr) = pending.pop() {
            if let ActionExpr::FunctionCall(call) = expr {
                if call.name == "bind"
                    && matches!(call.args.first(), Some(ActionExpr::Variable(..)))
                {
                    return Err(Self::compile_error_at(
                        &call.span,
                        "[FACTQPSR2] local bind is not allowed in a fact-query predicate",
                    ));
                }
            }
            if let ActionExpr::FunctionCall(call) = expr {
                pending.extend(crate::effects::evaluated_arguments(
                    self,
                    current_module,
                    call,
                ));
            } else {
                expr.push_children(&mut pending);
            }
        }
        Ok(())
    }

    fn validate_query_member_rebinding(
        call: &FunctionCall,
        query_members: &HashSet<String>,
    ) -> Result<(), LoadError> {
        if call.name == "bind" {
            if let Some(ActionExpr::Variable(name, span)) = call.args.first() {
                if query_members.contains(Self::existential_scope_variable_name(name)) {
                    return Err(Self::compile_error_at(
                        span,
                        &format!("[FACTQPSR3] cannot rebind query member ?{name}"),
                    ));
                }
            }
        }
        Ok(())
    }

    fn validate_expression_callable_name(
        &self,
        callable: &str,
        span: &Span,
        current_module: crate::modules::ModuleId,
        context: &str,
    ) -> Result<(), LoadError> {
        if callable == "refresh-agenda" {
            return Err(Self::compile_error_at(
                span,
                "refresh-agenda is unsupported: only definition-time salience is supported",
            ));
        }
        if self.is_declared_expression_callable(callable, current_module) {
            return Ok(());
        }
        Err(Self::missing_function_declaration_error(
            callable, span, context,
        ))
    }

    fn is_declared_expression_callable(
        &self,
        callable: &str,
        current_module: crate::modules::ModuleId,
    ) -> bool {
        if callable == "__fact_slot_ref" {
            return true;
        }
        if callable == "call-next-method" || crate::evaluator::is_builtin_callable(callable) {
            return true;
        }

        match parse_qualified_name(callable) {
            Ok(QualifiedName::Qualified { module, name }) => self
                .module_registry
                .get_by_name(&module)
                .is_some_and(|owner| {
                    self.functions.contains(owner, &name) || self.generics.contains(owner, &name)
                }),
            Ok(QualifiedName::Unqualified(name)) => {
                self.functions.modules_for_name(&name).iter().any(|owner| {
                    self.module_registry.is_construct_visible(
                        current_module,
                        *owner,
                        "deffunction",
                        &name,
                    )
                }) || self.generics.modules_for_name(&name).iter().any(|owner| {
                    self.module_registry.is_construct_visible(
                        current_module,
                        *owner,
                        "defgeneric",
                        &name,
                    )
                })
            }
            Err(_) => false,
        }
    }

    fn missing_function_declaration_error(callable: &str, span: &Span, context: &str) -> LoadError {
        LoadError::Compile(format!(
            "[EXPRNPSR3] Missing function declaration for {callable} in {context} at line {}, column {}",
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
                | "break"
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
                | "progn"
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

        // Pattern fact addresses get variable slots after the match variables,
        // so each activation can bind them into its own token copy.
        let mut var_map = plan.var_map().clone();
        let mut address_names: Vec<&String> = translated.fact_address_vars.keys().collect();
        address_names.sort_unstable();
        for name in address_names {
            let symbol = self
                .symbol_table
                .intern_symbol(name, self.config.string_encoding)
                .map_err(|e| LoadError::Compile(format!("{e}")))?;
            var_map
                .get_or_create(symbol)
                .map_err(|e| LoadError::Compile(format!("{e}")))?;
        }

        // Store rule info for action execution
        let info = CompiledRuleInfo {
            name: rule.name.clone(),
            source_definition,
            actions: rule.actions.clone(),
            var_map,
            fact_address_vars: translated.fact_address_vars,
            salience: Salience::new(rule.salience),
            auto_focus: rule.auto_focus,
            test_conditions: translated.test_conditions,
            runtime_actions,
            activation_layout: std::sync::OnceLock::new(),
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

        self.rete
            .set_rule_auto_focus(rule_id, prepared.info.auto_focus);

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
        self.drain_network_events();

        compile_result
    }

    fn push_test_predicate(
        conditions: &mut Vec<CompilableCondition>,
        test_conditions: &mut Vec<CompiledTestCondition>,
        test_condition: CompiledTestCondition,
    ) -> Result<(), LoadError> {
        let condition_index = u32::try_from(test_conditions.len())
            .map_err(|_| LoadError::Compile("too many test CEs in one rule".to_string()))?;
        test_conditions.push(test_condition);
        conditions.push(CompilableCondition::Predicate { condition_index });
        Ok(())
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
            Constraint::And(parts, _) => {
                for part in parts {
                    Self::collect_constraint_binding_variables(part, variables);
                }
            }
            Constraint::Or(_, _)
            | Constraint::Literal(_)
            | Constraint::Wildcard(_)
            | Constraint::MultiWildcard(_)
            | Constraint::Predicate(_, _)
            | Constraint::ReturnValue(_, _)
            | Constraint::Not(_, _) => {}
        }
    }

    /// Alternatives test existing variables; they cannot introduce bindings.
    fn validate_disjunction_bindings(
        pattern: &Pattern,
        bound: &mut HashSet<String>,
    ) -> Result<(), LoadError> {
        match pattern {
            Pattern::Ordered(ordered) => {
                for constraint in &ordered.constraints {
                    Self::validate_constraint_disjunction_bindings(constraint, bound, false)?;
                }
            }
            Pattern::Template(template) => {
                for constraint in template
                    .slot_constraints
                    .iter()
                    .flat_map(|slot| &slot.constraints)
                {
                    Self::validate_constraint_disjunction_bindings(constraint, bound, false)?;
                }
            }
            Pattern::Assigned {
                variable, pattern, ..
            } => {
                bound.insert(variable.clone());
                Self::validate_disjunction_bindings(pattern, bound)?;
            }
            Pattern::And(children, _) | Pattern::Logical(children, _) => {
                for child in children {
                    Self::validate_disjunction_bindings(child, bound)?;
                }
            }
            Pattern::Not(inner, _) => {
                Self::validate_disjunction_bindings(inner, &mut bound.clone())?;
            }
            Pattern::Exists(children, _) | Pattern::Forall(children, _) => {
                let mut local = bound.clone();
                for child in children {
                    Self::validate_disjunction_bindings(child, &mut local)?;
                }
            }
            Pattern::Or(children, _) => {
                for child in children {
                    Self::validate_disjunction_bindings(child, &mut bound.clone())?;
                }
            }
            Pattern::Test(..) => {}
        }
        Ok(())
    }

    /// CLIPS rejects `break` in test CEs and in `:`/`=` constraints with
    /// PRCDRPSR2; no loop surrounds an LHS expression.
    fn validate_lhs_breaks(pattern: &Pattern) -> Result<(), LoadError> {
        let constraints: Vec<&Constraint> = match pattern {
            Pattern::Ordered(ordered) => ordered.constraints.iter().collect(),
            Pattern::Template(template) => template
                .slot_constraints
                .iter()
                .flat_map(|slot| &slot.constraints)
                .collect(),
            Pattern::Assigned { pattern, .. } => return Self::validate_lhs_breaks(pattern),
            Pattern::Not(inner, _) => return Self::validate_lhs_breaks(inner),
            Pattern::And(children, _)
            | Pattern::Logical(children, _)
            | Pattern::Exists(children, _)
            | Pattern::Forall(children, _)
            | Pattern::Or(children, _) => {
                return children.iter().try_for_each(Self::validate_lhs_breaks);
            }
            Pattern::Test(expression, _) => {
                return Self::validate_lhs_expression_breaks(expression)
            }
        };
        constraints
            .into_iter()
            .try_for_each(Self::validate_constraint_breaks)
    }

    fn validate_constraint_breaks(constraint: &Constraint) -> Result<(), LoadError> {
        match constraint {
            Constraint::Predicate(expression, _) | Constraint::ReturnValue(expression, _) => {
                Self::validate_lhs_expression_breaks(expression)
            }
            Constraint::Not(inner, _) => Self::validate_constraint_breaks(inner),
            Constraint::And(parts, _) | Constraint::Or(parts, _) => {
                parts.iter().try_for_each(Self::validate_constraint_breaks)
            }
            _ => Ok(()),
        }
    }

    fn validate_lhs_expression_breaks(expression: &SExpr) -> Result<(), LoadError> {
        // Expressions the action interpreter cannot read keep their existing
        // translation diagnostics.
        let Ok(expression) = ferric_rules_parser::interpret_action_expr(expression) else {
            return Ok(());
        };
        // No loop body is in scope here, so every `break` is invalid whether a
        // fact head is read as a template or as an ordered relation.
        crate::callable_validation::validate_breaks_with_templates(
            std::slice::from_ref(&expression),
            &|_| false,
        )
        .map_err(|(span, message)| Self::compile_error_at(&span, &message))
    }

    fn validate_constraint_disjunction_bindings(
        constraint: &Constraint,
        bound: &mut HashSet<String>,
        alternative: bool,
    ) -> Result<(), LoadError> {
        match constraint {
            Constraint::Variable(name, span) | Constraint::MultiVariable(name, span) => {
                if alternative && !bound.contains(name) {
                    let message = format!(
                        "variable ?{name} in a field disjunction is referenced before being bound"
                    );
                    return Err(Self::compile_error_at(span, &message));
                }
                if !alternative {
                    bound.insert(name.clone());
                }
            }
            Constraint::And(parts, _) => {
                for part in parts {
                    Self::validate_constraint_disjunction_bindings(part, bound, alternative)?;
                }
            }
            Constraint::Or(parts, _) => {
                for part in parts {
                    Self::validate_constraint_disjunction_bindings(part, bound, true)?;
                }
            }
            Constraint::Not(inner, _) if alternative => {
                Self::validate_constraint_disjunction_bindings(inner, bound, true)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn collect_existential_local_variables(pattern: &Pattern, variables: &mut HashSet<String>) {
        match pattern {
            Pattern::Exists(children, _) | Pattern::Forall(children, _) => {
                for child in children {
                    Self::collect_pattern_binding_variables(child, variables);
                }
            }
            // Every variable first bound beneath a negation is local to it,
            // whatever the negated body is. Normalization rewrites `exists`
            // over `or` as a negated conjunction of negations, so the local
            // scope must not depend on a direct double negation.
            Pattern::Not(inner, _) => {
                Self::collect_pattern_binding_variables(inner, variables);
            }
            Pattern::Assigned { pattern, .. } => {
                Self::collect_existential_local_variables(pattern, variables);
            }
            Pattern::And(children, _)
            | Pattern::Logical(children, _)
            | Pattern::Or(children, _) => {
                for child in children {
                    Self::collect_existential_local_variables(child, variables);
                }
            }
            Pattern::Ordered(_) | Pattern::Template(_) | Pattern::Test(_, _) => {}
        }
    }

    /// A universal test can read the outer tuple and its own antecedent's
    /// bindings. Checking this before compilation avoids a dormant unbound
    /// variable becoming a silent match failure when the first tuple arrives.
    fn validate_forall_test_scope(
        &self,
        rule_name: &str,
        pattern: &Pattern,
        exported_variables: &HashSet<String>,
    ) -> Result<(), LoadError> {
        let Pattern::Forall(children, _) = pattern else {
            return Ok(());
        };
        let [antecedent, consequent] = children.as_slice() else {
            return Ok(());
        };
        let Some(expression) = Self::test_only_pattern_expression(consequent) else {
            return Ok(());
        };
        let expression = ferric_rules_parser::interpret_action_expr(&expression)
            .map_err(LoadError::Interpret)?;
        let mut available = exported_variables.clone();
        Self::collect_pattern_binding_variables(antecedent, &mut available);
        let scope = RuleRhsScope {
            engine: self,
            module: self.module_registry.current_module(),
            exported: &available,
            existential: &HashSet::new(),
            allow_local_reads: false,
        };
        Self::validate_rule_rhs_expr(rule_name, &expression, &scope, &mut HashSet::new()).map_err(
            |error| match error {
                LoadError::Compile(message) => LoadError::Compile(
                    message.replace("unbound RHS variable", "unbound variable in forall test"),
                ),
                error => error,
            },
        )
    }

    fn first_restricted_sexpr_variable(
        expr: &SExpr,
        restricted: &HashSet<String>,
    ) -> Option<String> {
        match expr {
            SExpr::Atom(Atom::SingleVar(name) | Atom::MultiVar(name), _) => {
                restricted.contains(name).then(|| name.clone())
            }
            SExpr::Atom(_, _) => None,
            SExpr::List(items, _) => items
                .iter()
                .find_map(|item| Self::first_restricted_sexpr_variable(item, restricted)),
        }
    }

    fn validate_existential_test_scope(
        rule_name: &str,
        expr: &SExpr,
        existential_locals: &HashSet<String>,
        exported_variables: &HashSet<String>,
    ) -> Result<(), LoadError> {
        let restricted: HashSet<String> = existential_locals
            .difference(exported_variables)
            .cloned()
            .collect();
        if let Some(variable) = Self::first_restricted_sexpr_variable(expr, &restricted) {
            return Err(Self::compile_error_at(
                &expr.span(),
                &format!(
                    "rule `{rule_name}` variable ?{variable} is not exported by existential or negated conditional element"
                ),
            ));
        }
        Ok(())
    }

    fn validate_rule_rhs_scope(
        &self,
        rule: &RuleConstruct,
        existential_locals: &HashSet<String>,
        exported_variables: &HashSet<String>,
    ) -> Result<(), LoadError> {
        let scope = RuleRhsScope {
            engine: self,
            module: self.module_registry.current_module(),
            exported: exported_variables,
            existential: existential_locals,
            allow_local_reads: true,
        };

        let context = format!("rule `{}`", rule.name);
        let mut rhs_locals = HashSet::new();
        for action in &rule.actions {
            Self::validate_rule_rhs_call(&context, &action.call, &scope, &mut rhs_locals)?;
        }
        Ok(())
    }

    /// Dormant fact initializers may bind locals but never read them; only
    /// iteration variables are visible inside their loop bodies.
    pub(crate) fn validate_fact_initializer_bindings(
        &self,
        expression: &ActionExpr,
        module: crate::modules::ModuleId,
    ) -> Result<(), LoadError> {
        let empty = HashSet::new();
        let scope = RuleRhsScope {
            engine: self,
            module,
            exported: &empty,
            existential: &empty,
            allow_local_reads: false,
        };
        Self::validate_rule_rhs_expr("fact initializer", expression, &scope, &mut HashSet::new())
    }

    fn existential_scope_variable_name(name: &str) -> &str {
        name.strip_prefix("$?").unwrap_or(name)
    }

    fn validate_rule_rhs_call(
        context: &str,
        call: &FunctionCall,
        scope: &RuleRhsScope<'_>,
        rhs_locals: &mut HashSet<String>,
    ) -> Result<(), LoadError> {
        if call.name == "__fact_slot_ref" {
            if let [ActionExpr::Variable(member, _), ActionExpr::Literal(slot)] =
                call.args.as_slice()
            {
                if let LiteralKind::Symbol(slot) = &slot.value {
                    let ordinary_name = format!("{member}:{slot}");
                    if rhs_locals.contains(&ordinary_name)
                        || scope.exported.contains(&ordinary_name)
                    {
                        return Ok(());
                    }
                }
            }
        }
        if call.name == "bind" {
            if let Some(ActionExpr::Variable(name, _)) = call.args.first() {
                for value in call.args.iter().skip(1) {
                    Self::validate_rule_rhs_expr(context, value, scope, rhs_locals)?;
                }
                if scope.allow_local_reads {
                    rhs_locals.insert(Self::existential_scope_variable_name(name).to_string());
                }
                return Ok(());
            }
        }

        for arg in crate::effects::evaluated_arguments(scope.engine, scope.module, call) {
            Self::validate_rule_rhs_expr(context, arg, scope, rhs_locals)?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_lines)] // Mirrors every structured RHS scope in ActionExpr.
    fn validate_rule_rhs_expr(
        context: &str,
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
                        "is not exported by existential or negated conditional element"
                    } else {
                        "is an unbound RHS variable"
                    };
                    return Err(LoadError::Compile(format!(
                        "[PRCCODE3] {context} variable {display_name} at line {} {reason}",
                        span.start.line
                    )));
                }
                Ok(())
            }
            ActionExpr::FunctionCall(call) => {
                Self::validate_rule_rhs_call(context, call, scope, rhs_locals)
            }
            ActionExpr::If {
                condition,
                then_actions,
                else_actions,
                ..
            } => {
                Self::validate_rule_rhs_expr(context, condition, scope, rhs_locals)?;
                let mut then_locals = rhs_locals.clone();
                for action in then_actions {
                    Self::validate_rule_rhs_expr(context, action, scope, &mut then_locals)?;
                }
                let mut else_locals = rhs_locals.clone();
                for action in else_actions {
                    Self::validate_rule_rhs_expr(context, action, scope, &mut else_locals)?;
                }
                rhs_locals.extend(then_locals);
                rhs_locals.extend(else_locals);
                Ok(())
            }
            ActionExpr::While {
                condition, body, ..
            } => {
                Self::validate_rule_rhs_expr(context, condition, scope, rhs_locals)?;
                let mut body_locals = rhs_locals.clone();
                for action in body {
                    Self::validate_rule_rhs_expr(context, action, scope, &mut body_locals)?;
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
                Self::validate_rule_rhs_expr(context, start, scope, rhs_locals)?;
                Self::validate_rule_rhs_expr(context, end, scope, rhs_locals)?;
                let mut body_locals = rhs_locals.clone();
                if let Some(name) = var_name {
                    body_locals.insert(name.clone());
                }
                for action in body {
                    Self::validate_rule_rhs_expr(context, action, scope, &mut body_locals)?;
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
                Self::validate_rule_rhs_expr(context, list_expr, scope, rhs_locals)?;
                let mut body_locals = rhs_locals.clone();
                body_locals.insert(var_name.clone());
                body_locals.insert(format!("{var_name}-index"));
                for action in body {
                    Self::validate_rule_rhs_expr(context, action, scope, &mut body_locals)?;
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
                Self::validate_rule_rhs_expr(context, query, scope, &mut query_locals)?;
                for action in body {
                    Self::validate_rule_rhs_expr(context, action, scope, &mut query_locals)?;
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
                Self::validate_rule_rhs_expr(context, expr, scope, rhs_locals)?;
                for (case_expr, actions) in cases {
                    Self::validate_rule_rhs_expr(context, case_expr, scope, rhs_locals)?;
                    let mut case_locals = rhs_locals.clone();
                    for action in actions {
                        Self::validate_rule_rhs_expr(context, action, scope, &mut case_locals)?;
                    }
                    rhs_locals.extend(case_locals);
                }
                if let Some(actions) = default {
                    let mut default_locals = rhs_locals.clone();
                    for action in actions {
                        Self::validate_rule_rhs_expr(context, action, scope, &mut default_locals)?;
                    }
                    rhs_locals.extend(default_locals);
                }
                Ok(())
            }
            ActionExpr::Literal(_) | ActionExpr::GlobalVariable(_, _) => Ok(()),
        }
    }

    /// Whether a negated ordered pattern contains an expression that cannot
    /// currently be represented by the negative network.
    fn has_complex_negated_expression(pattern: &Pattern) -> bool {
        match pattern {
            Pattern::Assigned { pattern, .. } => Self::has_complex_negated_expression(pattern),
            Pattern::Not(inner, _) => {
                let Pattern::Ordered(ordered) = inner.as_ref() else {
                    return false;
                };

                Self::ordered_pattern_has_complex_negated_expression(ordered)
            }
            _ => false,
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

    /// Normalize conditional elements so that `or` survives only at rule level.
    ///
    /// Works bottom-up: every child is normalized before its parent is rebuilt.
    /// - `and` groups are spliced into their enclosing conjunction.
    /// - `or` branches that are themselves `or` groups are spliced in source
    ///   order, and a branch conjunction containing `or` is distributed into
    ///   several branches, so no branch contains an `or`.
    /// - `not(and(A, or(B, C)))` → `not(and(A, B))`, `not(and(A, C))`
    ///   (De Morgan, including `not(or(B, C))` → `not(B)`, `not(C)`).
    /// - `not(exists(A, B ...))` → `not(and(A, B ...))`, and
    ///   `not(not(and(A, B ...)))` → `exists(A, B ...)`.
    /// - `exists(A, or(B, C))` → `not(and(not(and(A, B)), not(and(A, C))))`:
    ///   `exists` is one boolean condition, so it is rewritten as its double
    ///   negation instead of being split into separately firing rule variants.
    fn normalize_nested_or_ces(rule: &RuleConstruct) -> RuleConstruct {
        let patterns = Self::normalize_conjunction(&rule.patterns);

        RuleConstruct {
            name: rule.name.clone(),
            span: rule.span,
            comment: rule.comment.clone(),
            salience: rule.salience,
            salience_expression: rule.salience_expression.clone(),
            auto_focus: rule.auto_focus,
            patterns,
            actions: rule.actions.clone(),
        }
    }

    /// Normalize a sequence of conjuncts into one flat conjunction. The only
    /// `or` CEs in the result are direct members, and their branches contain
    /// no `or`.
    fn normalize_conjunction(patterns: &[Pattern]) -> Vec<Pattern> {
        let mut conjunction = Vec::with_capacity(patterns.len());
        for pattern in patterns {
            for normalized in Self::normalize_pattern(pattern) {
                Self::push_conjunct(&mut conjunction, normalized);
            }
        }
        conjunction
    }

    /// Append a pattern to a conjunction, splicing nested `and` groups.
    fn push_conjunct(conjunction: &mut Vec<Pattern>, pattern: Pattern) {
        match pattern {
            Pattern::And(children, _) => {
                for child in children {
                    Self::push_conjunct(conjunction, child);
                }
            }
            pattern => conjunction.push(pattern),
        }
    }

    /// Distribute a normalized conjunction over its `or` members. Alternatives
    /// keep source order, with the first `or` varying slowest.
    fn conjunction_disjuncts(conjunction: Vec<Pattern>) -> Vec<Vec<Pattern>> {
        let mut disjuncts = vec![Vec::with_capacity(conjunction.len())];
        for pattern in conjunction {
            match pattern {
                Pattern::Or(branches, _) if !branches.is_empty() => {
                    let mut next = Vec::with_capacity(disjuncts.len() * branches.len());
                    for prefix in &disjuncts {
                        for branch in &branches {
                            let mut disjunct = prefix.clone();
                            Self::push_conjunct(&mut disjunct, branch.clone());
                            next.push(disjunct);
                        }
                    }
                    disjuncts = next;
                }
                pattern => {
                    for disjunct in &mut disjuncts {
                        disjunct.push(pattern.clone());
                    }
                }
            }
        }
        disjuncts
    }

    fn conjunction_pattern(mut conjunction: Vec<Pattern>, span: Span) -> Pattern {
        if conjunction.len() == 1 {
            conjunction.pop().expect("one conjunct")
        } else {
            Pattern::And(conjunction, span)
        }
    }

    /// Negate a normalized conjunction. An `or` member is distributed first,
    /// so the result is a conjunction of negations that contains no `or`.
    fn negate_conjunction(conjunction: Vec<Pattern>, inner_span: Span, span: Span) -> Vec<Pattern> {
        if !conjunction
            .iter()
            .any(|pattern| matches!(pattern, Pattern::Or(..)))
        {
            return vec![Self::negated_pattern(
                Self::conjunction_pattern(conjunction, inner_span),
                span,
            )];
        }
        Self::conjunction_disjuncts(conjunction)
            .into_iter()
            .map(|disjunct| {
                Self::negated_pattern(Self::conjunction_pattern(disjunct, inner_span), span)
            })
            .collect()
    }

    /// Build `(not inner)` from a normalized, `or`-free operand.
    fn negated_pattern(inner: Pattern, span: Span) -> Pattern {
        // `(not (exists X ...))` is `(not (and X ...))`, through any number
        // of directly nested `exists`.
        let mut inner = inner;
        while let Pattern::Exists(children, exists_span) = inner {
            if children.is_empty() {
                inner = Pattern::Exists(children, exists_span);
                break;
            }
            inner = Self::conjunction_pattern(children, exists_span);
        }
        // A doubly negated conjunction is the existential tuple condition.
        if let Pattern::Not(negated, _) = &inner {
            if let Pattern::And(children, _) = negated.as_ref() {
                return Pattern::Exists(children.clone(), span);
            }
        }
        let negated = Pattern::Not(Box::new(inner), span);
        match Self::test_only_pattern_expression(&negated) {
            Some(expression) => Pattern::Test(expression, span),
            None => negated,
        }
    }

    /// Recursively normalize a single pattern into a conjunction of patterns.
    fn normalize_pattern(pattern: &Pattern) -> Vec<Pattern> {
        // Pure tests beneath a quantifier have no fact tuple to retain. Collapse
        // them to a predicate while preserving ordinary OR-CE multiplicity.
        if let Pattern::Not(_, span) | Pattern::Exists(_, span) = pattern {
            if let Some(expression) = Self::test_only_pattern_expression(pattern) {
                return vec![Pattern::Test(expression, *span)];
            }
        }
        match pattern {
            Pattern::And(children, _) => Self::normalize_conjunction(children),
            Pattern::Or(children, span) => {
                let mut branches = Vec::with_capacity(children.len());
                for child in children {
                    let child_span = *pattern_source_span(child);
                    let conjunction = Self::normalize_conjunction(std::slice::from_ref(child));
                    // Splitting one branch into a conjunction must not turn
                    // those conjuncts into additional OR branches; only its
                    // own `or` members multiply it.
                    branches.extend(
                        Self::conjunction_disjuncts(conjunction)
                            .into_iter()
                            .map(|disjunct| Self::conjunction_pattern(disjunct, child_span)),
                    );
                }
                if branches.len() == 1 {
                    let mut conjunction = Vec::new();
                    Self::push_conjunct(&mut conjunction, branches.pop().expect("one branch"));
                    return conjunction;
                }
                vec![Pattern::Or(branches, *span)]
            }
            Pattern::Not(inner, span) => {
                let inner_span = *pattern_source_span(inner);
                let conjunction = match inner.as_ref() {
                    // `(not (exists X ...))` is `(not (and X ...))`.
                    Pattern::Exists(children, _) => Self::normalize_conjunction(children),
                    inner => Self::normalize_conjunction(std::slice::from_ref(inner)),
                };
                Self::negate_conjunction(conjunction, inner_span, *span)
            }
            Pattern::Exists(children, span) => {
                let body = Self::normalize_conjunction(children);
                if body
                    .iter()
                    .any(|pattern| matches!(pattern, Pattern::Or(..)))
                {
                    // CLIPS treats `exists` as `(not (not (and ...)))`: one
                    // condition however many disjuncts hold. Distributing the
                    // `or` into rule variants would fire once per true branch.
                    let negated = Self::negate_conjunction(body, *span, *span);
                    return Self::negate_conjunction(negated, *span, *span);
                }
                vec![Pattern::Exists(body, *span)]
            }
            Pattern::Assigned {
                variable,
                pattern: inner,
                span,
            } => Self::normalize_pattern(inner)
                .into_iter()
                .map(|p| Pattern::Assigned {
                    variable: variable.clone(),
                    pattern: Box::new(p),
                    span: *span,
                })
                .collect(),
            // All other patterns pass through unchanged
            _ => vec![pattern.clone()],
        }
    }

    /// Turn a fact-free quantified CE into its boolean test. This helper is
    /// deliberately not applied to ordinary positive OR CEs, whose disjuncts
    /// retain separate activations even when several branches are true.
    fn test_only_pattern_expression(pattern: &Pattern) -> Option<SExpr> {
        let (name, arguments, span) = match pattern {
            Pattern::Test(expression, _) => return Some(expression.clone()),
            Pattern::Not(inner, span) => (
                "not",
                vec![Self::test_only_pattern_expression(inner)?],
                *span,
            ),
            Pattern::And(children, span) | Pattern::Exists(children, span) => (
                "and",
                children
                    .iter()
                    .map(Self::test_only_pattern_expression)
                    .collect::<Option<Vec<_>>>()?,
                *span,
            ),
            Pattern::Or(children, span) => (
                "or",
                children
                    .iter()
                    .map(Self::test_only_pattern_expression)
                    .collect::<Option<Vec<_>>>()?,
                *span,
            ),
            _ => return None,
        };
        // An empty group has no condition to test. The parser rejects it;
        // leave it to the ordinary CE paths rather than synthesize `(and)`.
        if arguments.is_empty() {
            return None;
        }
        if name != "not" && arguments.len() == 1 {
            return arguments.into_iter().next();
        }
        let mut call = Vec::with_capacity(arguments.len() + 1);
        call.push(SExpr::Atom(Atom::Symbol(name.to_owned()), span));
        call.extend(arguments);
        Some(SExpr::List(call, span))
    }

    /// Expand `Pattern::Or` CEs via rule duplication.
    /// Returns a vec of rule variants (1 if no disjunctions, N*M*... for Cartesian product).
    ///
    /// Runs to a fixpoint: top-level `and` groups are flattened and each
    /// rule-level `or` (possibly under a fact assignment) is replaced by its
    /// branches until none remains. Variants keep source order, with the first
    /// `or` varying slowest.
    fn expand_or_patterns(rule: &RuleConstruct) -> Vec<RuleConstruct> {
        // Without any `or` CE, every pattern has exactly one
        // alternative; skip building (and cloning) the single-variant product.
        if !rule.patterns.iter().any(Self::pattern_has_disjunction) {
            return vec![rule.clone()];
        }

        let mut flat_patterns = Vec::with_capacity(rule.patterns.len());
        for pattern in &rule.patterns {
            Self::push_conjunct(&mut flat_patterns, pattern.clone());
        }
        let mut combinations = Vec::new();
        Self::expand_disjunction_variants(flat_patterns, &mut combinations);
        if combinations.len() <= 1 {
            return vec![rule.clone()];
        }

        // Create rule variants
        combinations
            .into_iter()
            .map(|patterns| RuleConstruct {
                name: rule.name.clone(),
                span: rule.span,
                comment: rule.comment.clone(),
                salience: rule.salience,
                salience_expression: rule.salience_expression.clone(),
                auto_focus: rule.auto_focus,
                patterns,
                actions: rule.actions.clone(),
            })
            .collect()
    }

    /// Replace the first rule-level `or` with each branch, then expand the
    /// rest of each variant the same way.
    fn expand_disjunction_variants(patterns: Vec<Pattern>, out: &mut Vec<Vec<Pattern>>) {
        let Some(index) = patterns.iter().position(Self::is_rule_disjunction) else {
            out.push(patterns);
            return;
        };
        for option in Self::pattern_disjunction_options(&patterns[index]) {
            let mut variant = Vec::with_capacity(patterns.len());
            variant.extend(patterns[..index].iter().cloned());
            Self::push_conjunct(&mut variant, option);
            variant.extend(patterns[index + 1..].iter().cloned());
            Self::expand_disjunction_variants(variant, out);
        }
    }

    /// Whether a pattern contains an `or` CE. Field disjunctions stay in one test.
    fn pattern_has_disjunction(pattern: &Pattern) -> bool {
        match pattern {
            Pattern::Or(..) => true,
            Pattern::Test(..) | Pattern::Ordered(_) | Pattern::Template(_) => false,
            Pattern::Not(inner, _) | Pattern::Assigned { pattern: inner, .. } => {
                Self::pattern_has_disjunction(inner)
            }
            Pattern::And(children, _)
            | Pattern::Exists(children, _)
            | Pattern::Forall(children, _)
            | Pattern::Logical(children, _) => children.iter().any(Self::pattern_has_disjunction),
        }
    }

    /// Whether a rule-level pattern is an `or` CE, possibly under an assignment.
    fn is_rule_disjunction(pattern: &Pattern) -> bool {
        match pattern {
            Pattern::Or(branches, _) => !branches.is_empty(),
            Pattern::Assigned { pattern, .. } => Self::is_rule_disjunction(pattern),
            _ => false,
        }
    }

    /// Expand only a top-level `or` CE (possibly wrapped in a fact assignment).
    fn pattern_disjunction_options(pattern: &Pattern) -> Vec<Pattern> {
        match pattern {
            Pattern::Or(branches, _) if !branches.is_empty() => branches.clone(),
            Pattern::Assigned {
                variable,
                pattern: inner,
                span,
            } => Self::pattern_disjunction_options(inner)
                .into_iter()
                .map(|branch| Pattern::Assigned {
                    variable: variable.clone(),
                    pattern: Box::new(branch),
                    span: *span,
                })
                .collect(),
            _ => vec![pattern.clone()],
        }
    }

    /// Translate a `RuleConstruct` (parser types) into a `CompilableRule` (core types).
    #[allow(clippy::too_many_lines)] // Preserves source-order CE translation in one pass.
    fn translate_rule_construct(
        &mut self,
        rule: &RuleConstruct,
    ) -> Result<TranslatedRule, LoadError> {
        let mut bound = HashSet::new();
        for pattern in &rule.patterns {
            Self::validate_disjunction_bindings(pattern, &mut bound)?;
            Self::validate_lhs_breaks(pattern)?;
        }
        let mut conditions = Vec::new();
        let mut fact_address_vars = HashMap::new();
        let mut test_conditions: Vec<CompiledTestCondition> = Vec::new();
        let mut fact_index = 0usize;
        let mut internal_slot_var_seed = 0usize;
        let mut exported_variables = HashSet::new();
        let mut existential_locals = HashSet::new();

        // Flatten ordinary conjunctions. Logical CEs have already been rejected.
        // CLIPS treats (and ...) as a grouping CE equivalent to listing sub-patterns directly.
        // Double negation stays intact and is translated through an exists node.
        let mut flat_patterns: Vec<&Pattern> = Vec::new();
        for pattern in &rule.patterns {
            Self::flatten_pattern(pattern, &mut flat_patterns);
        }

        for pattern in &flat_patterns {
            // Test CEs do not consume a fact index. Their expressions are
            // retained in rule metadata and referenced by predicate nodes at
            // their source position in the beta network.
            if let Pattern::Test(sexpr, _span) = pattern {
                Self::validate_existential_test_scope(
                    &rule.name,
                    sexpr,
                    &existential_locals,
                    &exported_variables,
                )?;
                let runtime_expr =
                    crate::evaluator::from_sexpr(sexpr, &mut self.symbol_table, &self.config)
                        .map_err(|e| LoadError::Compile(format!("test CE translation: {e}")))?;
                Self::push_test_predicate(
                    &mut conditions,
                    &mut test_conditions,
                    CompiledTestCondition::Expr(runtime_expr),
                )?;
                continue;
            }

            Self::collect_existential_local_variables(pattern, &mut existential_locals);
            self.validate_forall_test_scope(&rule.name, pattern, &exported_variables)?;

            // Fallback path for complex negated ordered constraints that cannot
            // be lowered to join/alpha tests cannot remain a firing-time check:
            // it would expose an invalid terminal activation and could not react
            // to right-side assertion/retraction. Reject it until it has a real
            // negative-network representation.
            if Self::has_complex_negated_expression(pattern) {
                return Err(Self::unsupported_pattern(
                    "not",
                    pattern_source_span(pattern),
                    "complex constraints inside negated patterns are not supported at match time",
                ));
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
            let mut embedded_generated_tests = HashSet::new();
            let condition = self.translate_condition(
                pattern,
                &mut generated_tests,
                &mut internal_slot_var_seed,
                test_conditions.len(),
                &mut embedded_generated_tests,
            )?;
            // Quantified translation embeds every test it generates in its own
            // subnetwork; only a plain positive pattern leaves tests to follow it.
            debug_assert!(
                matches!(
                    &condition,
                    CompilableCondition::Pattern(compilable)
                        if !compilable.negated && !compilable.exists
                ) || generated_tests.len() == embedded_generated_tests.len()
            );
            if let Some(name) = var_name {
                if !is_negated && Self::condition_has_fact_address(&condition) {
                    fact_address_vars.insert(name, fact_index);
                }
            }
            if Self::condition_has_fact_address(&condition) {
                Self::collect_pattern_binding_variables(pattern, &mut exported_variables);
                fact_index += 1;
            }
            conditions.push(condition);
            for (generated_index, generated_test) in generated_tests.into_iter().enumerate() {
                if embedded_generated_tests.contains(&generated_index) {
                    test_conditions.push(CompiledTestCondition::Expr(generated_test));
                } else {
                    Self::push_test_predicate(
                        &mut conditions,
                        &mut test_conditions,
                        CompiledTestCondition::Expr(generated_test),
                    )?;
                }
            }
        }

        // Empty conjunctions attach directly to the existing non-fact root.
        // The visible initial-fact is separate state, not support for this match.

        self.validate_rule_rhs_scope(rule, &existential_locals, &exported_variables)?;
        let mut query_ordinary = exported_variables.clone();
        for action in &rule.actions {
            if action.call.name == "bind" {
                if let Some(ActionExpr::Variable(name, _)) = action.call.args.first() {
                    query_ordinary.insert(Self::existential_scope_variable_name(name).to_owned());
                }
            }
        }
        crate::query_validation::validate_query_scopes(
            rule.actions.iter().flat_map(|action| {
                crate::effects::evaluated_arguments(
                    self,
                    self.module_registry.current_module(),
                    &action.call,
                )
            }),
            query_ordinary,
            &fact_address_vars.keys().cloned().collect(),
            self,
            self.module_registry.current_module(),
        )
        .map_err(|(span, message)| Self::compile_error_at(&span, &message))?;

        Ok(TranslatedRule {
            salience: Salience::new(rule.salience),
            conditions,
            fact_address_vars,
            test_conditions,
        })
    }

    fn condition_has_fact_address(condition: &CompilableCondition) -> bool {
        match condition {
            CompilableCondition::Pattern(pattern) => !pattern.negated && !pattern.exists,
            CompilableCondition::Predicate { .. } | CompilableCondition::Ncc(_) => false,
        }
    }

    #[allow(clippy::too_many_lines)]
    fn translate_condition(
        &mut self,
        pattern: &Pattern,
        generated_tests: &mut Vec<crate::evaluator::RuntimeExpr>,
        internal_slot_var_seed: &mut usize,
        test_condition_base: usize,
        embedded_generated_tests: &mut HashSet<usize>,
    ) -> Result<CompilableCondition, LoadError> {
        match pattern {
            Pattern::Assigned { pattern, .. } => self.translate_condition(
                pattern,
                generated_tests,
                internal_slot_var_seed,
                test_condition_base,
                embedded_generated_tests,
            ),
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
                            subconditions.extend(self.translate_subcondition(
                                sub,
                                generated_tests,
                                internal_slot_var_seed,
                                test_condition_base,
                                embedded_generated_tests,
                            )?);
                        }
                        Ok(CompilableCondition::Ncc(subconditions))
                    }
                    Pattern::Not(doubly_inner, _) => {
                        // (not (not X)) ≡ (exists X) in CLIPS.
                        // Preserve further nested negation parity, but compile
                        // an ordinary doubly-negated fact pattern through the
                        // support-counted exists node.
                        if matches!(doubly_inner.as_ref(), Pattern::Not(..)) {
                            self.translate_condition(
                                doubly_inner,
                                generated_tests,
                                internal_slot_var_seed,
                                test_condition_base,
                                embedded_generated_tests,
                            )
                        } else {
                            let mut conditions = self.translate_quantified_pattern(
                                doubly_inner,
                                false,
                                true,
                                generated_tests,
                                internal_slot_var_seed,
                                test_condition_base,
                                embedded_generated_tests,
                            )?;
                            Ok(conditions.remove(0))
                        }
                    }
                    _ => {
                        let mut conditions = self.translate_quantified_pattern(
                            inner,
                            true,
                            false,
                            generated_tests,
                            internal_slot_var_seed,
                            test_condition_base,
                            embedded_generated_tests,
                        )?;
                        Ok(conditions.remove(0))
                    }
                }
            }
            Pattern::Exists(sub_patterns, span) => {
                if sub_patterns.is_empty() {
                    return Err(Self::unsupported_pattern(
                        "exists",
                        span,
                        "exists requires at least one inner pattern",
                    ));
                }

                if sub_patterns.len() == 1
                    && matches!(
                        &sub_patterns[0],
                        Pattern::Ordered(_) | Pattern::Template(_) | Pattern::Assigned { .. }
                    )
                {
                    let mut conditions = self.translate_quantified_pattern(
                        &sub_patterns[0],
                        false,
                        true,
                        generated_tests,
                        internal_slot_var_seed,
                        test_condition_base,
                        embedded_generated_tests,
                    )?;
                    return Ok(conditions.remove(0));
                }

                let mut tuple_conditions = Vec::new();
                for sub_pattern in sub_patterns {
                    match sub_pattern {
                        Pattern::And(children, _) | Pattern::Logical(children, _) => {
                            for child in children {
                                tuple_conditions.extend(self.translate_subcondition(
                                    child,
                                    generated_tests,
                                    internal_slot_var_seed,
                                    test_condition_base,
                                    embedded_generated_tests,
                                )?);
                            }
                        }
                        _ => tuple_conditions.extend(self.translate_subcondition(
                            sub_pattern,
                            generated_tests,
                            internal_slot_var_seed,
                            test_condition_base,
                            embedded_generated_tests,
                        )?),
                    }
                }

                // An NCC emits one pass-through only while its tuple subnetwork
                // has no complete results. Watching that NCC with a second NCC
                // complements the condition: the outer token propagates exactly
                // while one or more complete tuples exist. Both NCC memories are
                // keyed by their owner token, so tuple support stays isolated per
                // outer match and follows only zero/nonzero transitions.
                Ok(CompilableCondition::Ncc(vec![CompilableCondition::Ncc(
                    tuple_conditions,
                )]))
            }
            Pattern::Test(sexpr, _) => {
                let condition_index = test_condition_base
                    .checked_add(generated_tests.len())
                    .and_then(|index| u32::try_from(index).ok())
                    .ok_or_else(|| {
                        LoadError::Compile("too many test CEs in one rule".to_string())
                    })?;
                let runtime_expr =
                    crate::evaluator::from_sexpr(sexpr, &mut self.symbol_table, &self.config)
                        .map_err(|e| LoadError::Compile(format!("test CE translation: {e}")))?;
                embedded_generated_tests.insert(generated_tests.len());
                generated_tests.push(runtime_expr);
                Ok(CompilableCondition::Predicate { condition_index })
            }
            Pattern::Forall(sub_patterns, span) => {
                // forall takes exactly 2 sub-patterns (condition + then-clause).
                if sub_patterns.len() != 2 {
                    return Err(Self::unsupported_pattern(
                        "forall",
                        span,
                        &format!(
                            "forall supports exactly one condition and one then-clause, got {} sub-patterns",
                            sub_patterns.len()
                        ),
                    ));
                }

                let consequent_test = Self::test_only_pattern_expression(&sub_patterns[1]);
                // The antecedent and fact consequent remain single patterns;
                // a fact-free consequent is evaluated for each antecedent tuple.
                for (index, sub) in sub_patterns.iter().enumerate() {
                    if index == 1 && consequent_test.is_some() {
                        continue;
                    }
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

                // forall(P, Q) holds while no P lacks a matching Q.
                let mut conditions = self.translate_quantified_pattern(
                    &sub_patterns[0],
                    false,
                    false,
                    generated_tests,
                    internal_slot_var_seed,
                    test_condition_base,
                    embedded_generated_tests,
                )?;
                if let Some(expression) = consequent_test {
                    let expression_ast = ferric_rules_parser::interpret_action_expr(&expression)
                        .map_err(LoadError::Interpret)?;
                    let test = crate::evaluator::from_action_expr(
                        &expression_ast,
                        &mut self.symbol_table,
                        &self.config,
                    )
                    .map_err(|error| Self::compile_error_at(span, &error.to_string()))?;
                    let local_index = generated_tests.len();
                    let condition_index = test_condition_base
                        .checked_add(local_index)
                        .and_then(|index| u32::try_from(index).ok())
                        .ok_or_else(|| {
                            Self::compile_error_at(span, "too many test CEs in one rule")
                        })?;
                    generated_tests.push(crate::evaluator::RuntimeExpr::Call {
                        name: "not".to_owned(),
                        args: vec![test],
                        span: Some(crate::evaluator::SourceSpan {
                            line: expression.span().start.line,
                            column: expression.span().start.column,
                        }),
                    });
                    embedded_generated_tests.insert(local_index);
                    conditions.push(CompilableCondition::Predicate { condition_index });
                } else {
                    conditions.extend(self.translate_quantified_pattern(
                        &sub_patterns[1],
                        true,
                        false,
                        generated_tests,
                        internal_slot_var_seed,
                        test_condition_base,
                        embedded_generated_tests,
                    )?);
                }
                Ok(CompilableCondition::Ncc(conditions))
            }
            Pattern::And(_, span) => Err(Self::unsupported_pattern(
                "and",
                span,
                "internal invariant violated: and groups are flattened during normalization",
            )),
            Pattern::Logical(_, span) => Err(Self::unsupported_pattern(
                "logical",
                span,
                "truth maintenance is not implemented",
            )),
            Pattern::Or(_, span) => Err(Self::unsupported_pattern(
                "or",
                span,
                "internal invariant violated: or CEs are expanded into rule variants during normalization",
            )),
            _ => Ok(CompilableCondition::Pattern(self.translate_pattern(
                pattern,
                generated_tests,
                &mut 0,
                internal_slot_var_seed,
                false,
            )?)),
        }
    }

    /// Append predicates to their positive child before an enclosing NCC decides
    /// whether the tuple exists. Nested quantified children embed their own tests.
    fn translate_subcondition(
        &mut self,
        pattern: &Pattern,
        generated_tests: &mut Vec<crate::evaluator::RuntimeExpr>,
        internal_slot_var_seed: &mut usize,
        test_condition_base: usize,
        embedded_generated_tests: &mut HashSet<usize>,
    ) -> Result<Vec<CompilableCondition>, LoadError> {
        let first_test = generated_tests.len();
        let condition = match self.translate_condition(
            pattern,
            generated_tests,
            internal_slot_var_seed,
            test_condition_base,
            embedded_generated_tests,
        )? {
            // NCC subnetworks have no support-counted exists node. Express an
            // existential member as the complement of its negation instead.
            CompilableCondition::Pattern(mut compiled) if compiled.exists => {
                compiled.exists = false;
                CompilableCondition::Ncc(vec![CompilableCondition::Ncc(vec![
                    CompilableCondition::Pattern(compiled),
                ])])
            }
            condition => condition,
        };
        let mut conditions = vec![condition];
        for index in first_test..generated_tests.len() {
            if embedded_generated_tests.insert(index) {
                let condition_index = test_condition_base
                    .checked_add(index)
                    .and_then(|index| u32::try_from(index).ok())
                    .ok_or_else(|| {
                        LoadError::Compile("too many test CEs in one rule".to_owned())
                    })?;
                conditions.push(CompilableCondition::Predicate { condition_index });
            }
        }
        Ok(conditions)
    }

    /// Keep generated field predicates within the quantified match. Moving a
    /// predicate after a not/exists node would test unbound local variables and
    /// miss changes to the supporting facts.
    #[allow(clippy::too_many_arguments)]
    fn translate_quantified_pattern(
        &mut self,
        pattern: &Pattern,
        negated: bool,
        exists: bool,
        generated_tests: &mut Vec<crate::evaluator::RuntimeExpr>,
        internal_slot_var_seed: &mut usize,
        test_condition_base: usize,
        embedded_generated_tests: &mut HashSet<usize>,
    ) -> Result<Vec<CompilableCondition>, LoadError> {
        let first_test = generated_tests.len();
        let mut disjunction_tests = 0;
        let mut compiled = self.translate_pattern(
            pattern,
            generated_tests,
            &mut disjunction_tests,
            internal_slot_var_seed,
            negated,
        )?;
        if first_test == generated_tests.len() {
            compiled.negated = negated;
            compiled.exists = exists;
            return Ok(vec![CompilableCondition::Pattern(compiled)]);
        }
        // Preserve the explicitly unsupported general existential-expression
        // boundary; this lowering is for connected field disjunctions, so any
        // other generated test keeps the pattern unsupported.
        if exists && generated_tests.len() - first_test != disjunction_tests {
            return Err(Self::unsupported_pattern(
                "exists",
                pattern_source_span(pattern),
                "complex constraints inside existential patterns are not supported at match time",
            ));
        }
        // The positive child carries the predicates; the wrappers below come
        // only from this call's quantifier, not from an inner `exists` that
        // `translate_pattern` reports on the pattern itself.
        compiled.negated = false;
        compiled.exists = false;
        let mut conditions = vec![CompilableCondition::Pattern(compiled)];
        for index in first_test..generated_tests.len() {
            let condition_index = test_condition_base
                .checked_add(index)
                .and_then(|index| u32::try_from(index).ok())
                .ok_or_else(|| LoadError::Compile("too many test CEs in one rule".to_owned()))?;
            embedded_generated_tests.insert(index);
            conditions.push(CompilableCondition::Predicate { condition_index });
        }
        if negated {
            conditions = vec![CompilableCondition::Ncc(conditions)];
        }
        if exists {
            conditions = vec![CompilableCondition::Ncc(vec![CompilableCondition::Ncc(
                conditions,
            )])];
        }
        Ok(conditions)
    }

    /// Translate a single `Pattern` into a `CompilablePattern`.
    #[allow(clippy::too_many_lines)] // Template pattern arm adds lines but is clear as-is
    fn translate_pattern(
        &mut self,
        pattern: &Pattern,
        generated_tests: &mut Vec<crate::evaluator::RuntimeExpr>,
        disjunction_tests: &mut usize,
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
                let mut variable_slots = Vec::new();
                let mut negated_variable_slots = Vec::new();
                let mut seen_variable_slots = HashMap::new();
                let mut slot_runtime_vars = HashMap::new();
                let mut alpha_disjunctions = Vec::new();

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
                        disjunction_tests,
                        &mut alpha_disjunctions,
                        &mut slot_runtime_vars,
                        internal_slot_var_seed,
                        in_negated_pattern,
                    )?;
                }
                self.fit_alpha_disjunctions(
                    alpha_disjunctions,
                    &mut constant_tests,
                    &mut variable_slots,
                    &mut seen_variable_slots,
                    generated_tests,
                    disjunction_tests,
                    &mut slot_runtime_vars,
                    internal_slot_var_seed,
                )?;

                let sequence = ordered
                    .constraints
                    .iter()
                    .any(Self::constraint_is_multifield)
                    .then(|| {
                        let tests = std::mem::take(&mut constant_tests);
                        let segments = vec![SequenceSegment {
                            source: SequenceSource::Ordered,
                            fields: ordered
                                .constraints
                                .iter()
                                .map(Self::sequence_field)
                                .collect(),
                        }];
                        Self::sequence_pattern(segments, tests, &mut constant_tests)
                    });
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
                    // The count goes last: alpha paths share no prefixes, so a
                    // leading count test would run once per pattern of the
                    // relation. Value tests already fail on a missing field.
                    if min > 0 || max.is_some() {
                        constant_tests.push(ConstantTest {
                            slot: SlotIndex::Ordered(0),
                            test_type: ConstantTestType::OrderedFieldCount { min, max },
                        });
                    }
                }

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
                    disjunction_tests,
                    internal_slot_var_seed,
                    in_negated_pattern,
                )
            }
            Pattern::Not(inner, _span) => {
                // Unwrap the inner pattern and set negated flag
                let mut compilable = self.translate_pattern(
                    inner,
                    generated_tests,
                    disjunction_tests,
                    internal_slot_var_seed,
                    true,
                )?;
                compilable.negated = true;
                Ok(compilable)
            }
            Pattern::Exists(patterns, span) => {
                // For single-pattern exists, compile as an exists pattern
                if patterns.len() == 1 {
                    let mut compilable = self.translate_pattern(
                        &patterns[0],
                        generated_tests,
                        disjunction_tests,
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
                    "test cannot be used in this nested pattern position",
                ))
            }
            Pattern::Template(template) => {
                let current_module = self.module_registry.current_module();
                let template_id = self
                    .resolve_template_reference(&template.template, current_module)
                    .map_err(|msg| Self::compile_error_at(&template.span, &msg))?;

                let registered = self
                    .template_defs
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
                    let slot_idx = registered
                        .slot_index(&slot_constraint.slot_name)
                        .ok_or_else(|| {
                            Self::compile_error_at(
                                &slot_constraint.span,
                                &format!(
                                    "unknown slot `{}` in template `{}`",
                                    slot_constraint.slot_name, template.template
                                ),
                            )
                        })?;
                    if !seen_slots.insert(slot_idx) {
                        return Err(Self::compile_error_at(
                            &slot_constraint.span,
                            &format!(
                                "duplicate slot `{}` in template pattern",
                                slot_constraint.slot_name
                            ),
                        ));
                    }
                    if registered.slot_types[slot_idx] == SlotType::Single {
                        if slot_constraint.constraints.len() != 1 {
                            return Err(Self::compile_error_at(
                                &slot_constraint.span,
                                &format!(
                                    "single-field slot `{}` requires exactly one field constraint",
                                    slot_constraint.slot_name
                                ),
                            ));
                        }
                        let constraint = &slot_constraint.constraints[0];
                        if Self::constraint_is_multifield(constraint)
                            && !matches!(constraint, Constraint::MultiWildcard(_))
                        {
                            return Err(Self::compile_error_at(
                                &slot_constraint.span,
                                &format!(
                                    "single-field slot `{}` cannot bind a multifield variable",
                                    slot_constraint.slot_name
                                ),
                            ));
                        }
                    }
                    for constraint in &slot_constraint.constraints {
                        self.validate_template_constraint(&registered, slot_idx, constraint)?;
                    }
                    // CLIPS 6.30 loads an empty multislot restriction such as
                    // `(values)` whatever its cardinality; the runtime check
                    // then keeps it from matching a valid fact.
                    if registered.slot_types[slot_idx] == SlotType::Multi
                        && !slot_constraint.constraints.is_empty()
                        && !slot_constraint
                            .constraints
                            .iter()
                            .any(Self::constraint_is_multifield)
                    {
                        registered.constraints[slot_idx]
                            .validate_cardinality(slot_constraint.constraints.len())
                            .map_err(|reason| {
                                Self::compile_error_at(
                                    &slot_constraint.span,
                                    &format!(
                                        "[CSTRNCHK1] {reason} for slot `{}` in template `{}`",
                                        slot_constraint.slot_name, template.template
                                    ),
                                )
                            })?;
                    }
                    slot_indices.push(slot_idx);
                }

                // A multislot constrained by one multifield term captures the
                // whole stored multifield, so the physical slot already is the
                // logical field. Keep such patterns physical (and indexable);
                // only real positional constraints need a sequence projection.
                let needs_sequence = template.slot_constraints.iter().zip(&slot_indices).any(
                    |(slot_constraint, &index)| {
                        registered.slot_types[index] == SlotType::Multi
                            && !matches!(
                                slot_constraint.constraints.as_slice(),
                                [constraint] if Self::constraint_is_multifield(constraint)
                            )
                    },
                );
                let mut constant_tests = Vec::new();
                let mut variable_slots = Vec::new();
                let mut negated_variable_slots = Vec::new();
                let mut seen_variable_slots = HashMap::new();
                let mut slot_runtime_vars = HashMap::new();
                let mut alpha_disjunctions = Vec::new();
                let mut segments = Vec::new();
                let mut logical_offset = 0;

                // Preserve written slot order: independent multislot splits
                // form a Cartesian product in that order in CLIPS.
                for (slot_constraint, slot_idx) in
                    template.slot_constraints.iter().zip(slot_indices)
                {
                    let is_multi = registered.slot_types[slot_idx] == SlotType::Multi;
                    if needs_sequence {
                        segments.push(if is_multi {
                            SequenceSegment {
                                source: SequenceSource::TemplateSlot(slot_idx),
                                fields: slot_constraint
                                    .constraints
                                    .iter()
                                    .map(Self::sequence_field)
                                    .collect(),
                            }
                        } else {
                            SequenceSegment {
                                source: SequenceSource::TemplateScalar(slot_idx),
                                fields: vec![SequenceField::Single],
                            }
                        });
                    }
                    for constraint in &slot_constraint.constraints {
                        let slot = SlotIndex::Template(if needs_sequence {
                            logical_offset
                        } else {
                            slot_idx
                        });
                        self.translate_constraint(
                            constraint,
                            slot,
                            &mut constant_tests,
                            &mut variable_slots,
                            &mut negated_variable_slots,
                            &mut seen_variable_slots,
                            generated_tests,
                            disjunction_tests,
                            &mut alpha_disjunctions,
                            &mut slot_runtime_vars,
                            internal_slot_var_seed,
                            in_negated_pattern,
                        )?;
                        logical_offset += 1;
                    }
                }
                self.fit_alpha_disjunctions(
                    alpha_disjunctions,
                    &mut constant_tests,
                    &mut variable_slots,
                    &mut seen_variable_slots,
                    generated_tests,
                    disjunction_tests,
                    &mut slot_runtime_vars,
                    internal_slot_var_seed,
                )?;

                let sequence = needs_sequence.then(|| {
                    let tests = std::mem::take(&mut constant_tests);
                    Self::sequence_pattern(segments, tests, &mut constant_tests)
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
                "forall is not supported in this nested pattern position",
            )),
            Pattern::And(_, span) => Err(Self::unsupported_pattern(
                "and",
                span,
                "internal invariant violated: and groups are flattened during normalization",
            )),
            Pattern::Logical(_, span) => Err(Self::unsupported_pattern(
                "logical",
                span,
                "truth maintenance is not implemented",
            )),
            Pattern::Or(_, span) => Err(Self::unsupported_pattern(
                "or",
                span,
                "internal invariant violated: or CEs are expanded into rule variants during normalization",
            )),
        }
    }

    /// Literal constraints must be valid even when negated or part of an OR.
    /// A predicate or return-value expression is checked when its value exists.
    fn validate_template_constraint(
        &self,
        template: &crate::templates::RegisteredTemplate,
        slot_index: usize,
        constraint: &Constraint,
    ) -> Result<(), LoadError> {
        match constraint {
            Constraint::Literal(literal) => template
                .validate_literal(slot_index, &literal.value, &self.symbol_table)
                .map_err(|reason| {
                    Self::compile_error_at(&literal.span, &format!("[CSTRNCHK1] {reason}"))
                }),
            Constraint::Not(inner, _) => {
                self.validate_template_constraint(template, slot_index, inner)
            }
            Constraint::And(parts, _) | Constraint::Or(parts, _) => {
                for part in parts {
                    self.validate_template_constraint(template, slot_index, part)?;
                }
                Ok(())
            }
            Constraint::Variable(..)
            | Constraint::MultiVariable(..)
            | Constraint::Wildcard(_)
            | Constraint::MultiWildcard(_)
            | Constraint::Predicate(..)
            | Constraint::ReturnValue(..) => Ok(()),
        }
    }

    fn sequence_field(constraint: &Constraint) -> SequenceField {
        if Self::constraint_is_multifield(constraint) {
            SequenceField::Multi
        } else {
            SequenceField::Single
        }
    }

    /// Build a sequence plan from logical constant tests. Tests that read only
    /// split-independent fields (an ordered prefix or scalar template slots)
    /// move to physical alpha selectors and filter facts before enumeration.
    fn sequence_pattern(
        segments: Vec<SequenceSegment>,
        tests: Vec<ConstantTest>,
        alpha_tests: &mut Vec<ConstantTest>,
    ) -> SequencePattern {
        let mut sequence = SequencePattern {
            segments,
            tests: Vec::new(),
        };
        for test in tests {
            match sequence.physical_test(&test) {
                Some(physical) => alpha_tests.push(physical),
                None => sequence.tests.push(test),
            }
        }
        sequence
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
        disjunction_tests: &mut usize,
        alpha_disjunctions: &mut Vec<AlphaDisjunction>,
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
                        disjunction_tests,
                        alpha_disjunctions,
                        slot_runtime_vars,
                        internal_slot_var_seed,
                        in_negated_pattern,
                    )?;
                }
            }
            Constraint::Or(constraints, span) => {
                if constraints.is_empty() {
                    return Err(Self::unsupported_constraint(
                        "or",
                        span,
                        "or constraints require at least one alternative",
                    ));
                }
                // Literal alternatives use the existing compact alpha test.
                let mut keys = Vec::with_capacity(constraints.len());
                for sub in constraints {
                    let Constraint::Literal(lit) = sub else { break };
                    let Some(key) = self.literal_to_atom_key(&lit.value)? else {
                        break;
                    };
                    keys.push(key);
                }
                if keys.len() == constraints.len() {
                    constant_tests.push(ConstantTest {
                        slot,
                        test_type: ConstantTestType::EqualAny(keys),
                    });
                    return Ok(());
                }

                // Preserve each alternative as a conjunction, without leaking
                // its bindings or comparisons into the other alternatives.
                // Constant and same-fact comparisons can run in alpha memory,
                // including inside not/exists and sequence plans. Every
                // alternative is translated, so each one is held to the same
                // restrictions as a standalone constraint in this position,
                // whatever its order.
                let mut alternatives = Vec::with_capacity(constraints.len());
                let mut alpha_only = true;
                for sub in constraints {
                    let mut tests = Vec::new();
                    let mut vars = variable_slots.clone();
                    let mut joins = Vec::new();
                    let mut seen = seen_variable_slots.clone();
                    let mut predicates = Vec::new();
                    let mut runtime_vars = slot_runtime_vars.clone();
                    self.translate_constraint(
                        sub,
                        slot,
                        &mut tests,
                        &mut vars,
                        &mut joins,
                        &mut seen,
                        &mut predicates,
                        &mut 0,
                        &mut Vec::new(),
                        &mut runtime_vars,
                        internal_slot_var_seed,
                        in_negated_pattern,
                    )?;
                    if vars != *variable_slots || !joins.is_empty() || !predicates.is_empty() {
                        alpha_only = false;
                    } else if alpha_only {
                        alternatives.push(tests);
                    }
                }
                let candidate = ConstantTest {
                    slot,
                    test_type: ConstantTestType::Any(alternatives),
                };
                // A field too wide for the alpha path budget is evaluated as
                // one match-time predicate instead of failing to load. Later
                // fields can still exceed the budget, so the pattern rechecks
                // it once all of its fields are translated.
                let fits_alpha_budget = ferric_rules_core::alpha::constant_test_count(
                    constant_tests,
                )
                .saturating_add(ferric_rules_core::alpha::constant_test_count(
                    std::slice::from_ref(&candidate),
                )) <= ferric_rules_core::compiler::MAX_ALPHA_TESTS;
                if alpha_only && fits_alpha_budget {
                    alpha_disjunctions.push(AlphaDisjunction {
                        index: constant_tests.len(),
                        slot,
                        constraint: constraint.clone(),
                        generated_at: generated_tests.len(),
                    });
                    constant_tests.push(candidate);
                } else {
                    let slot_var = self.ensure_slot_runtime_variable(
                        slot,
                        variable_slots,
                        seen_variable_slots,
                        slot_runtime_vars,
                        internal_slot_var_seed,
                    )?;
                    generated_tests.push(self.constraint_runtime_expr(constraint, &slot_var)?);
                    *disjunction_tests += 1;
                }
            }
            Constraint::Predicate(expr, span) => {
                if in_negated_pattern {
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
                    return Err(Self::unsupported_constraint(
                        ":",
                        span,
                        "predicate constraints inside negated patterns currently require a simple binary comparison involving the current slot variable",
                    ));
                }
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

    /// Decide the alpha budget once the whole pattern is translated: while its
    /// constant tests exceed `MAX_ALPHA_TESTS`, the widest alpha disjunctions
    /// become match-time predicates. An alpha-only disjunction binds nothing,
    /// so the swap leaves the pattern's bindings unchanged.
    #[allow(clippy::too_many_arguments)]
    fn fit_alpha_disjunctions(
        &mut self,
        mut disjunctions: Vec<AlphaDisjunction>,
        constant_tests: &mut Vec<ConstantTest>,
        variable_slots: &mut Vec<(SlotIndex, ferric_rules_core::Symbol)>,
        seen_variable_slots: &mut HashMap<ferric_rules_core::Symbol, SlotIndex>,
        generated_tests: &mut Vec<crate::evaluator::RuntimeExpr>,
        disjunction_tests: &mut usize,
        slot_runtime_vars: &mut HashMap<SlotIndex, String>,
        internal_slot_var_seed: &mut usize,
    ) -> Result<(), LoadError> {
        use ferric_rules_core::alpha::constant_test_count;
        use std::cmp::Reverse;
        // Sequence plans split these tests between alpha selectors and
        // sequence tests without changing the total, and the field-count
        // test is added later and not counted, so this total is the budget.
        let mut count = constant_test_count(constant_tests);
        if count <= ferric_rules_core::compiler::MAX_ALPHA_TESTS {
            return Ok(());
        }
        let size = |disjunction: &AlphaDisjunction| {
            constant_test_count(std::slice::from_ref(&constant_tests[disjunction.index]))
        };
        disjunctions.sort_by_cached_key(|disjunction| Reverse(size(disjunction)));
        let mut fallbacks = Vec::new();
        for disjunction in disjunctions {
            if count <= ferric_rules_core::compiler::MAX_ALPHA_TESTS {
                break;
            }
            count -= size(&disjunction);
            fallbacks.push(disjunction);
        }
        // Remove from the back so the recorded positions stay valid.
        fallbacks.sort_by_key(|disjunction| Reverse(disjunction.index));
        for disjunction in &fallbacks {
            constant_tests.remove(disjunction.index);
        }
        // Insert later fields first so the predicates keep field order.
        fallbacks.sort_by_key(|disjunction| Reverse((disjunction.generated_at, disjunction.index)));
        for disjunction in fallbacks {
            let slot_var = self.ensure_slot_runtime_variable(
                disjunction.slot,
                variable_slots,
                seen_variable_slots,
                slot_runtime_vars,
                internal_slot_var_seed,
            )?;
            let test = self.constraint_runtime_expr(&disjunction.constraint, &slot_var)?;
            generated_tests.insert(disjunction.generated_at, test);
            *disjunction_tests += 1;
        }
        Ok(())
    }

    /// A connected constraint is a Boolean expression over one field. Variables
    /// within alternatives reference existing bindings; only the leading binding
    /// (outside the disjunction) introduces a variable.
    fn constraint_runtime_expr(
        &mut self,
        constraint: &Constraint,
        slot_var: &str,
    ) -> Result<crate::evaluator::RuntimeExpr, LoadError> {
        use crate::evaluator::RuntimeExpr;
        let call = |name: &str, args| RuntimeExpr::Call {
            name: name.to_owned(),
            args,
            span: None,
        };
        let field = || RuntimeExpr::BoundVar {
            name: slot_var.to_owned(),
            span: None,
        };
        Ok(match constraint {
            Constraint::Literal(literal) => {
                let value = crate::evaluator::from_action_expr(
                    &ActionExpr::Literal(literal.clone()),
                    &mut self.symbol_table,
                    &self.config,
                )
                .map_err(|error| LoadError::Compile(format!("field constraint: {error}")))?;
                call("eq", vec![field(), value])
            }
            Constraint::Variable(name, _) | Constraint::MultiVariable(name, _) => call(
                "eq",
                vec![
                    field(),
                    RuntimeExpr::BoundVar {
                        name: name.clone(),
                        span: None,
                    },
                ],
            ),
            Constraint::Wildcard(_) | Constraint::MultiWildcard(_) => {
                RuntimeExpr::Literal(Value::Symbol(self.compile_symbol("TRUE")?))
            }
            Constraint::Not(inner, _) => {
                call("not", vec![self.constraint_runtime_expr(inner, slot_var)?])
            }
            Constraint::And(parts, _) | Constraint::Or(parts, _) => {
                let args = parts
                    .iter()
                    .map(|part| self.constraint_runtime_expr(part, slot_var))
                    .collect::<Result<Vec<_>, _>>()?;
                call(
                    if matches!(constraint, Constraint::And(..)) {
                        "and"
                    } else {
                        "or"
                    },
                    args,
                )
            }
            Constraint::Predicate(expr, _) | Constraint::ReturnValue(expr, _) => {
                let value =
                    crate::evaluator::from_sexpr(expr, &mut self.symbol_table, &self.config)
                        .map_err(|error| {
                            LoadError::Compile(format!("field constraint: {error}"))
                        })?;
                if matches!(constraint, Constraint::ReturnValue(..)) {
                    call("eq", vec![field(), value])
                } else {
                    value
                }
            }
        })
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
            LiteralKind::InstanceName(s) => {
                let sym = self.compile_symbol(s)?;
                Ok(Some(AtomKey::InstanceName(InstanceName::from_symbol(sym))))
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

/// Validate source conditional elements before normalization and Rete compilation.
///
/// `not`, `exists`, and `forall` each consume one nesting level. Grouping CEs
/// and fact-address assignments are transparent. Retained nesting/operand limits
/// report the source construct that violates them, before lowering loses spans.
fn validate_rule_patterns(
    patterns: &[Pattern],
    max_nesting_depth: usize,
) -> Vec<ferric_rules_core::PatternValidationError> {
    let mut errors = Vec::new();
    for pattern in patterns {
        validate_pattern_recursive(pattern, 0, max_nesting_depth, false, false, &mut errors);
    }
    errors
}

fn validate_pattern_recursive(
    pattern: &Pattern,
    depth: usize,
    max_depth: usize,
    inside_forall: bool,
    inside_not_or_exists: bool,
    errors: &mut Vec<ferric_rules_core::PatternValidationError>,
) {
    let quantified = matches!(
        pattern,
        Pattern::Not(..) | Pattern::Exists(..) | Pattern::Forall(..)
    );
    let child_depth = depth + usize::from(quantified);
    if quantified && child_depth > max_depth {
        push_nesting_depth_error(
            errors,
            pattern_source_span(pattern),
            child_depth,
            max_depth,
            ferric_rules_core::ValidationStage::ReteCompilation,
        );
    }
    match pattern {
        Pattern::Not(inner, _) => {
            validate_pattern_recursive(inner, child_depth, max_depth, inside_forall, true, errors);
        }
        Pattern::Exists(children, _) => {
            for child in children {
                validate_pattern_recursive(
                    child,
                    child_depth,
                    max_depth,
                    inside_forall,
                    true,
                    errors,
                );
            }
        }
        Pattern::Forall(children, span) => {
            if inside_forall {
                push_pattern_restriction(
                    errors,
                    span,
                    ferric_rules_core::PatternViolation::NestedForall,
                );
            }
            if inside_not_or_exists {
                push_pattern_restriction(
                    errors,
                    span,
                    ferric_rules_core::PatternViolation::UnsupportedNestingCombination {
                        description: "forall inside not or exists is not supported; place forall in a positive rule condition".to_owned(),
                    },
                );
            }
            validate_forall_operands(children, span, errors);
            for child in children {
                validate_pattern_recursive(
                    child,
                    child_depth,
                    max_depth,
                    true,
                    inside_not_or_exists,
                    errors,
                );
            }
        }
        Pattern::Assigned { pattern: inner, .. } => {
            validate_pattern_recursive(
                inner,
                depth,
                max_depth,
                inside_forall,
                inside_not_or_exists,
                errors,
            );
        }
        Pattern::And(children, _) | Pattern::Logical(children, _) | Pattern::Or(children, _) => {
            for child in children {
                validate_pattern_recursive(
                    child,
                    depth,
                    max_depth,
                    inside_forall,
                    inside_not_or_exists,
                    errors,
                );
            }
        }
        Pattern::Ordered(..) | Pattern::Template(..) | Pattern::Test(..) => {}
    }
}

fn validate_forall_operands(
    children: &[Pattern],
    span: &ferric_rules_parser::Span,
    errors: &mut Vec<ferric_rules_core::PatternValidationError>,
) {
    if children.len() != 2 {
        push_pattern_restriction(
            errors,
            span,
            ferric_rules_core::PatternViolation::UnsupportedNestingCombination {
                description: format!("forall requires exactly one fact condition and one fact-or-test then-clause; received {} conditional elements", children.len()),
            },
        );
        return;
    }
    if !matches!(
        children[0],
        Pattern::Ordered(..) | Pattern::Template(..) | Pattern::Forall(..)
    ) {
        push_pattern_restriction(
            errors,
            pattern_source_span(&children[0]),
            ferric_rules_core::PatternViolation::ForallConditionNotSinglePattern,
        );
    }
    if !matches!(
        children[1],
        Pattern::Ordered(..) | Pattern::Template(..) | Pattern::Forall(..)
    ) && !is_pure_test_condition(&children[1])
    {
        push_pattern_restriction(
            errors,
            pattern_source_span(&children[1]),
            ferric_rules_core::PatternViolation::UnsupportedNestingCombination {
                description: "forall then-clause must be a single fact pattern or a test-only condition; fact conditions inside not, or, and, or exists are not supported here".to_owned(),
            },
        );
    }
}

/// Test-only quantified trees lower to a predicate. Logical CEs never qualify:
/// they remain unsupported and must not disappear during normalization.
/// Derived from the lowering itself so validation and normalization agree.
fn is_pure_test_condition(pattern: &Pattern) -> bool {
    Engine::test_only_pattern_expression(pattern).is_some()
}

fn push_pattern_restriction(
    errors: &mut Vec<ferric_rules_core::PatternValidationError>,
    span: &ferric_rules_parser::Span,
    kind: ferric_rules_core::PatternViolation,
) {
    errors.push(ferric_rules_core::PatternValidationError::new(
        kind,
        Some(span_to_source_location(span)),
        ferric_rules_core::ValidationStage::ReteCompilation,
    ));
}

fn pattern_source_span(pattern: &Pattern) -> &ferric_rules_parser::Span {
    match pattern {
        Pattern::Ordered(pattern) => &pattern.span,
        Pattern::Template(pattern) => &pattern.span,
        Pattern::Assigned { span, .. }
        | Pattern::Not(_, span)
        | Pattern::Exists(_, span)
        | Pattern::Forall(_, span)
        | Pattern::Test(_, span)
        | Pattern::And(_, span)
        | Pattern::Or(_, span)
        | Pattern::Logical(_, span) => span,
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
            (deffacts seed (a) (b) (c) (d) (e) (f))
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
        // Both disjuncts hold, but exists is one condition (CLIPS 6.30
        // crashes on this rule, so there is no reference behavior to pin).
        engine.reset().unwrap();
        assert_eq!(engine.agenda_len(), 1);
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
    fn field_alternatives_require_previously_bound_variables() {
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
            result.as_ref().is_err_and(|errors| errors
                .iter()
                .any(|error| { error.to_string().contains("referenced before being bound") })),
            "alternatives cannot introduce variables: {result:?}"
        );
    }

    #[test]
    fn or_constraint_preserves_bindings_without_rule_duplication() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (deffacts startup
              (v 2)
              (v 3))
            (defrule branchy
              (v ?x&2|?x&:(> ?x 1))
              =>
              (assert (hit ?x)))
            ",
        );
        assert_eq!(engine.rules().len(), 1);
        engine.reset().unwrap();

        // (v 2) satisfies both alternatives; a duplicated rule would fire
        // for it twice.
        let run = run_to_completion(&mut engine);
        assert_eq!(run.rules_fired, 2);

        let mut hits: Vec<_> = find_facts_by_relation(&engine, "hit")
            .into_iter()
            .map(|handle| match engine.get_fact(handle).unwrap().unwrap() {
                Fact::Ordered(ordered) => match ordered.fields.as_slice() {
                    [Value::Integer(value)] => *value,
                    fields => panic!("unexpected hit fields {fields:?}"),
                },
                Fact::Template(_) => panic!("expected ordered fact"),
            })
            .collect();
        hits.sort_unstable();
        assert_eq!(hits, [2, 3]);
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
    fn negated_predicate_constraint_rejects_non_linear_expression() {
        let mut engine = new_utf8_engine();
        let errors = engine
            .load_str(
                r"
            (defrule no-square-greater
              (anchor ?min)
              (not (data ?x&:(> (* ?x ?x) (* ?min ?min))))
              =>
              (assert (safe-square ?min)))
            ",
            )
            .expect_err("non-linear negated predicate must be rejected");

        assert!(errors.iter().any(|error| matches!(
            error,
            LoadError::Compile(message)
                if message.contains("complex constraints inside negated patterns")
        )));
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
    fn negated_return_value_constraint_rejects_non_linear_expression() {
        let mut engine = new_utf8_engine();
        let errors = engine
            .load_str(
                r"
            (defrule no-self-square
              (not (pair ?x&=(* ?x ?x)))
              =>
              (assert (safe-return)))
            ",
            )
            .expect_err("non-linear negated return value must be rejected");

        assert!(errors.iter().any(|error| matches!(
            error,
            LoadError::Compile(message)
                if message.contains("complex constraints inside negated patterns")
        )));
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
                &mut 0,
                &mut Vec::new(),
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
                &mut 0,
                &mut Vec::new(),
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
                &mut 0,
                &mut Vec::new(),
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
                &mut 0,
                &mut Vec::new(),
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
                &mut 0,
                &mut Vec::new(),
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
    fn expression_queries_in_global_initializers_share_live_engine_context() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (deftemplate item (slot value))
            (deffacts seed (item (value 30)))
            (deffunction query () (find-fact ((?f item)) TRUE))
            (defglobal ?*kept* = 7)
        ",
        );
        engine.reset().unwrap();
        for (index, initializer) in [
            "(any-factp ((?f item)) TRUE)",
            "(find-fact ((?f item)) TRUE)",
            "(find-all-facts ((?f item)) TRUE)",
            "(query)",
            "(do-for-fact ((?f item)) TRUE ?f:value)",
        ]
        .iter()
        .enumerate()
        {
            let name = format!("captured-{index}");
            engine
                .load_str(&format!("(defglobal ?*{name}* = {initializer})"))
                .unwrap();
            let value = engine.get_global(&name).unwrap();
            match index {
                0 => assert!(
                    matches!(value, Value::Symbol(symbol) if engine.resolve_core_symbol(*symbol) == Some("TRUE"))
                ),
                4 => assert!(matches!(value, Value::Integer(30))),
                _ => assert!(
                    matches!(value, Value::Multifield(fields) if fields.len() == 1 && matches!(&fields[0], Value::FactAddress(_)))
                ),
            }
        }
        engine.reset().unwrap();
        assert!(matches!(engine.get_global("kept"), Some(Value::Integer(7))));
        assert!(matches!(
            engine.get_global("captured-4"),
            Some(Value::Integer(30))
        ));
    }

    #[test]
    fn invalid_expression_query_does_not_replace_callable_or_create_generic() {
        let mut engine = new_utf8_engine();
        load_ok(&mut engine, "(deffunction keep () 7)");
        let main = engine.module_registry.main_module_id();
        for source in [
            "(deffunction keep () (any-factp ((?f missing)) TRUE))",
            "(defmethod absent ((?x INTEGER)) (find-fact ((?f missing)) TRUE))",
        ] {
            let errors = engine.load_str(source).unwrap_err();
            assert!(errors
                .iter()
                .any(|error| error.to_string().contains("unknown template")));
        }
        assert!(engine.functions.contains(main, "keep"));
        assert!(!engine.generics.contains(main, "absent"));
        assert!(!engine
            .generic_modules
            .get(&main)
            .is_some_and(|names| names.contains_key("absent")));
        load_ok(&mut engine, "(defrule invoke => (printout t (keep) crlf))");
        engine.reset().unwrap();
        engine.run(crate::RunLimit::Unlimited).unwrap();
        assert!(engine.action_diagnostics().is_empty());
        assert_eq!(engine.get_output("t"), Some("7\n"));
    }

    #[test]
    fn expression_query_cannot_resolve_a_template_declared_later() {
        let mut engine = new_utf8_engine();
        load_ok(&mut engine, "(defrule keep => (assert (kept)))");
        let errors = engine
            .load_str(
                r"
            (defrule keep => (printout t (any-factp ((?f later)) TRUE) crlf))
            (deftemplate later (slot value))
        ",
            )
            .unwrap_err();
        assert!(errors
            .iter()
            .any(|error| error.to_string().contains("unknown template `later`")));
        engine.reset().unwrap();
        engine.run(crate::RunLimit::Unlimited).unwrap();
        assert_eq!(engine.find_facts("kept").unwrap().len(), 1);
        assert_eq!(engine.get_output("t").unwrap_or(""), "");
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
            (deffunction foo (42))
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
            joined.contains("deffunction parameter must be a variable"),
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
        engine.reset().unwrap();

        assert!(result.asserted_facts.is_empty());
        assert!(result.rules.is_empty());
    }

    #[test]
    fn load_deffacts_ambiguous_empty_slot_form_evaluates_ordered_expression() {
        let mut engine = new_utf8_engine();
        let result = load_ok(
            &mut engine,
            "(deffunction field () clear) (deffacts startup (foo bar) (foo (field)))",
        );
        assert!(result.asserted_facts.is_empty());
        engine.reset().unwrap();
        let facts = engine.find_facts("foo").unwrap();
        let Fact::Ordered(fact) = facts[1].1 else {
            panic!("expected ordered fact")
        };
        let [Value::Symbol(value)] = fact.fields.as_slice() else {
            panic!("expected one symbol")
        };
        assert_eq!(engine.resolve_core_symbol(*value), Some("clear"));
    }

    #[test]
    fn load_deffacts_unknown_ordered_field_function_errors() {
        let mut engine = new_utf8_engine();
        let errors = load_err(&mut engine, "(deffacts startup (ghost (slot1 value)))");

        assert!(
            errors
                .iter()
                .any(|e| matches!(e, LoadError::Compile(msg) if msg.contains("Missing function declaration for slot1"))),
            "expected unknown-function error, got: {errors:?}"
        );
    }

    #[test]
    fn load_nested_unknown_function_rejects_the_whole_fact() {
        let mut engine = new_utf8_engine();
        let source = r#"(assert (person (name "John") (age 30)))"#;
        let errors = load_err(&mut engine, source);
        assert!(errors[0]
            .to_string()
            .contains("Missing function declaration for name"));
        assert!(engine.find_facts("person").unwrap().is_empty());
    }

    #[test]
    fn load_encoding_error_rejects_the_whole_fact() {
        let mut engine = Engine::new(EngineConfig::ascii());
        let source = "(assert (person \"héllo\"))";
        let errors = load_err(&mut engine, source);
        assert!(errors[0].to_string().contains("encoding error"));
        assert!(engine.find_facts("person").unwrap().is_empty());
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
    // defmodule / defgeneric / defmethod loader tests
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
            .load_str("(defmethod display 1 (?x) ?x)")
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
    // deffunction/defgeneric conflict diagnostics
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
mod pattern_restriction_tests {
    use super::{is_pure_test_condition, validate_rule_patterns};
    use ferric_rules_parser::{
        interpret_constructs, parse_sexprs, Construct, FileId, InterpreterConfig, RuleConstruct,
    };

    fn rule(lhs: &str) -> RuleConstruct {
        let source = format!("(defrule example\n  {lhs}\n  =>)");
        let parsed = parse_sexprs(&source, FileId(0));
        assert!(parsed.errors.is_empty(), "{:?}", parsed.errors);
        let mut interpreted = interpret_constructs(&parsed.exprs, &InterpreterConfig::default());
        assert!(interpreted.errors.is_empty(), "{:?}", interpreted.errors);
        let Construct::Rule(rule) = interpreted.constructs.remove(0) else {
            panic!("expected rule");
        };
        rule
    }

    #[test]
    fn retained_conditional_element_limits_are_located_through_grouping_wrappers() {
        for (lhs, code, detail) in [
            ("(forall (a) (forall (b) (c)))", "E0003", "cannot be nested"),
            (
                "(forall (a) (and (forall (b) (c))))",
                "E0003",
                "cannot be nested",
            ),
            (
                "(not (and (a) (forall (b) (c))))",
                "E0005",
                "inside not or exists",
            ),
            (
                "(exists (a) (and (forall (b) (c))))",
                "E0005",
                "inside not or exists",
            ),
            (
                "(forall (a) (b) (c))",
                "E0005",
                "exactly one fact condition",
            ),
            ("(forall (exists (a)) (b))", "E0002", "single fact pattern"),
            ("(forall (a) (not (b)))", "E0005", "then-clause must be"),
            ("(forall (and (a)) (b))", "E0002", "single fact pattern"),
        ] {
            let parsed = rule(lhs);
            let errors = validate_rule_patterns(&parsed.patterns, 4);
            let error = errors
                .iter()
                .find(|error| error.code == code && error.to_string().contains(detail))
                .unwrap_or_else(|| panic!("{lhs}: {errors:?}"));
            let location = error
                .location
                .expect("source validation retains a location");
            assert_eq!(location.line, 2, "{lhs}");
            assert!(location.column >= 3, "{lhs}");
            assert!(error
                .suggestion
                .as_ref()
                .is_some_and(|suggestion| !suggestion.is_empty()));
        }
    }

    #[test]
    fn source_quantifier_depth_counts_wrappers_but_not_grouping() {
        for depth in [4, 5] {
            let mut lhs = "(a)".to_owned();
            for _ in 0..depth {
                lhs = format!("(not (and {lhs}))");
            }
            let parsed = rule(&lhs);
            let errors = validate_rule_patterns(&parsed.patterns, 4);
            if depth == 4 {
                assert!(errors.is_empty(), "{errors:?}");
            } else {
                assert_eq!(errors.len(), 1);
                assert_eq!(errors[0].code, "E0001");
                assert!(errors[0]
                    .to_string()
                    .contains("nesting depth 5 exceeds maximum of 4"));
                assert_eq!(errors[0].location.unwrap().line, 2);
            }
        }
        let parsed = rule("(exists (a) (exists (b) (exists (c) (exists (d) (forall (e) (f))))))");
        let errors = validate_rule_patterns(&parsed.patterns, 4);
        assert!(
            errors.iter().any(|error| error.code == "E0001"),
            "forall counts in source depth"
        );
    }

    #[test]
    fn supported_test_wrappers_and_negated_exists_operands_pass_validation() {
        for lhs in [
            "(forall (a ?x) (test (> ?x 0)))",
            "(forall (a ?x) (not (test (< ?x 0))))",
            "(forall (a ?x) (exists (and (test (> ?x 0)) (test (< ?x 5)))))",
            "(exists (not (test (< 1 0))))",
            "(exists (not (a)))",
            "(exists (and (not (a))))",
            "(exists (or (not (a)) (b)))",
            "(exists (a) (not (b)))",
            "(exists (and (a) (not (b))))",
            "(forall (a) (b)) (forall (c) (d))",
            "(and (forall (a) (b)) (or (forall (c) (d)) (e)))",
        ] {
            let parsed = rule(lhs);
            let errors = validate_rule_patterns(&parsed.patterns, 4);
            assert!(errors.is_empty(), "{lhs}: {errors:?}");
        }
    }

    #[test]
    fn logical_wrappers_are_never_classified_as_pure_tests() {
        let parsed = rule("(exists (logical (test (eq 1 1))))");
        assert!(!is_pure_test_condition(&parsed.patterns[0]));
    }
}
