//! The Ferric rules engine.
//!
//! This module provides the main `Engine` type, which is the primary interface
//! for embedding applications, including fact assertion/retraction and
//! transferable ownership for serialized host work.

use rustc_hash::FxHashMap as HashMap;
use std::collections::VecDeque;
use std::sync::Arc;
use std::thread::ThreadId;
use thiserror::Error;

use ferric_rules_core::beta::RuleId;
use ferric_rules_core::{
    EncodingError, Fact, FactBase, FactId, FactInsertionResult, FerricString, ReteCompiler,
    ReteNetwork, Symbol, SymbolTable, TemplateFact, TemplateId, Value,
};

use crate::actions::{self, ActionError, CompiledRuleInfo};
use crate::config::EngineConfig;
use crate::execution::{FiredRule, HaltReason, RunLimit, RunResult};
use crate::functions::{FunctionEnv, GenericRegistry, GlobalStore, ModuleNameMap};
use crate::host::{
    FactHandle, HostFact, HostState, HostValue, InstanceNameHandle, IntoHostFields, SymbolHandle,
    HOST_VALUE_MAX_ITEMS,
};
use crate::modules::{ModuleId, ModuleRegistry};
use crate::router::OutputRouter;
use crate::templates::RegisteredTemplate;
use crate::tracing_support::{ferric_event, ferric_span};

fn run_result(rules_fired: usize, halt_reason: HaltReason) -> RunResult {
    RunResult {
        rules_fired,
        halt_reason,
    }
}

/// Dense indexed storage for per-rule data, keyed by `RuleId`.
///
/// This uses a `Vec<Option<T>>` instead of a `HashMap` because `RuleId`s are
/// initially allocated by `ReteCompiler` starting at 1. Retired slots are reused
/// after successful planning, so reload/undefine cycles do not accumulate holes
/// or consume new metadata slots. Rule IDs are internal executable identities,
/// not durable identities for a source rule across replacement.
///
/// Direct indexed access provides faster O(1) lookups on the execution hot
/// path (activation selection, rule firing, agenda display) compared to
/// hash-based lookup.
pub(crate) type RuleIndex<T> = Vec<Option<T>>;

/// A named seed definition. Vector order is definition order; replacement
/// moves the definition to the end of its module's reset sequence.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub(crate) struct RegisteredDeffacts {
    pub module: ModuleId,
    pub name: String,
    pub facts: Vec<Fact>,
}

pub(crate) fn rule_index_get<T>(entries: &[Option<T>], rule_id: RuleId) -> Option<&T> {
    entries.get(rule_id.0 as usize)?.as_ref()
}

pub(crate) fn rule_index_insert<T>(
    entries: &mut RuleIndex<T>,
    rule_id: RuleId,
    value: T,
) -> Option<T> {
    let index = rule_id.0 as usize;
    if entries.len() <= index {
        entries.resize_with(index + 1, || None);
    }
    entries[index].replace(value)
}

fn propagate_fact_assertion(rete: &mut ReteNetwork, fact_base: &FactBase, fact_id: FactId) {
    let fact = &fact_base
        .get(fact_id)
        .expect("asserted fact should exist in fact base")
        .fact;
    rete.assert_fact(fact_id, fact, fact_base);
}

/// Result of attempting to assert a fact.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FactAssertionResult<Id = FactHandle> {
    /// A new fact was inserted and propagated through the RETE network.
    Asserted(Id),
    /// Duplication was disabled and an equivalent active fact already existed.
    Duplicate(Id),
}

impl<Id: Copy> FactAssertionResult<Id> {
    /// Return the newly asserted or existing equivalent fact ID.
    #[must_use]
    pub fn fact_id(self) -> Id {
        match self {
            Self::Asserted(fact_id) | Self::Duplicate(fact_id) => fact_id,
        }
    }

    /// Return whether this assertion created a new fact.
    #[must_use]
    pub fn was_asserted(self) -> bool {
        matches!(self, Self::Asserted(_))
    }
}

/// The Ferric rules engine.
///
/// This is the main entry point for embedding applications. The engine is
/// `Send`: ownership can move to another thread for use and destruction.
/// Mutation requires exclusive access. Use a host mutex when multiple threads
/// need to operate on one engine.
///
/// The engine is also `Sync`: shared references may be read concurrently.
/// Mutating operations still require exclusive access. Owned values and shared
/// match metadata can be cloned and used independently of an engine on other
/// threads. The C handle has its own serialized-call contract.
///
/// ```
/// use ferric_rules_runtime::Engine;
/// fn assert_send<T: Send>() {}
/// assert_send::<Engine>();
/// ```
///
/// ```
/// use ferric_rules_runtime::Engine;
/// fn assert_sync<T: Sync>() {}
/// assert_sync::<Engine>();
/// ```
///
/// ## Phase 2 complete
///
/// - Fact assertion/retraction (`assert_ordered`, `assert`, `retract`)
/// - Fact query (`get_fact`, `facts`)
/// - Symbol interning and string creation
/// - Source loading (`load_str`, `load_file`) with Stage 2 interpretation
/// - Rule compilation from Stage 2 AST into shared rete network
/// - Execution loop (`run`, `step`, `halt`, `reset`)
/// - RHS action execution (`assert`, `retract`, `modify`, `duplicate`, `halt`)
/// - Agenda conflict strategy selection (Depth, Breadth, LEX, MEA)
/// - Ownership transfer between threads with exclusive mutation
///
/// ## Phase 3 complete
///
/// - Expression evaluator for nested function calls in RHS and test CEs.
/// - Template-aware `modify`/`duplicate` with slot validation.
/// - `printout` with per-channel output capture via `OutputRouter`.
/// - `deffunction` runtime: user-defined functions with parameter binding and
///   recursion limits.
/// - `defglobal` runtime: global variable read/write via `bind`, with reset
///   re-initialization.
/// - `defmodule` runtime: module registry, focus stack, focus-aware `run()`,
///   `focus` RHS action, and cross-module template visibility.
/// - `defgeneric`/`defmethod` runtime: type-based method dispatch with
///   index ordering and auto-index assignment.
/// - `forall` CE (limited): single condition + single then-clause,
///   desugared to NCC, with vacuous truth and empty-prefix support.
///
/// ## Phase 4 complete
///
/// - Module-qualified `MODULE::name` resolution for callables and globals.
/// - Cross-module `deffunction`/`defglobal` visibility enforcement.
/// - `deffunction`/`defgeneric` same-name conflict diagnostics.
/// - CLIPS-style generic specificity ranking and `call-next-method`.
/// - Full Section 10.2 builtin surface: predicate/math/type, string/symbol,
///   multifield, I/O (`format`, `read`, `readline`), environment (`reset`,
///   `clear`), agenda/focus query functions (`get-focus`, `get-focus-stack`,
///   `list-focus-stack`, `agenda`).
pub struct Engine {
    pub(crate) fact_base: FactBase,
    pub(crate) host: HostState,
    pub(crate) symbol_table: SymbolTable,
    pub(crate) config: EngineConfig,
    pub(crate) rete: ReteNetwork,
    pub(crate) compiler: ReteCompiler,
    /// Registered deffacts for re-assertion on reset.
    pub(crate) registered_deffacts: Vec<RegisteredDeffacts>,
    /// Compiled rule info for action execution.
    pub(crate) rule_info: RuleIndex<Arc<CompiledRuleInfo>>,
    /// Registered template definitions: name → `TemplateId`.
    pub(crate) template_ids: HashMap<Box<str>, TemplateId>,
    /// Template slot metadata indexed by `TemplateId`.
    // Definitions are immutable after installation. Actions and owned host facts
    // retain a cheap handle; redefinition installs a fresh allocation so captured
    // facts keep the exact shape against which they were validated.
    pub(crate) template_defs: slotmap::SlotMap<TemplateId, Arc<RegisteredTemplate>>,
    pub(crate) template_local_ids: crate::loader::TemplateLocalIndex,
    /// Output router for capturing `printout` and related I/O.
    pub(crate) router: OutputRouter,
    /// Registry of user-defined functions loaded via `deffunction`.
    pub(crate) functions: FunctionEnv,
    /// Runtime storage for `defglobal` variables.
    pub(crate) globals: GlobalStore,
    /// Snapshot of global initial values for re-initialization on reset.
    pub(crate) registered_globals: Vec<(ModuleId, String, Value)>,
    /// Registry of generic functions and methods loaded via `defgeneric`/`defmethod`.
    pub(crate) generics: GenericRegistry,
    /// Module registry: module definitions, focus stack, visibility.
    pub(crate) module_registry: ModuleRegistry,
    /// Rule-to-module association for focus-aware execution.
    pub(crate) rule_modules: RuleIndex<ModuleId>,
    /// Template-to-module association for visibility checking.
    pub(crate) template_modules: slotmap::SecondaryMap<ferric_rules_core::TemplateId, ModuleId>,
    /// Function-to-module association for consistency-check bookkeeping.
    pub(crate) function_modules: ModuleNameMap<ModuleId>,
    /// Global-to-module association for consistency-check bookkeeping.
    pub(crate) global_modules: ModuleNameMap<ModuleId>,
    /// Generic-to-module association for consistency-check bookkeeping.
    pub(crate) generic_modules: ModuleNameMap<ModuleId>,
    /// The `FactId` of the synthetic `(initial-fact)` in working memory, if present.
    ///
    /// It supports explicit `(initial-fact)` patterns. Empty/negative prefixes
    /// use the independent non-fact root. Public host queries hide this protected
    /// implementation fact, and retraction rejects its ID.
    pub(crate) initial_fact_id: Option<FactId>,
    /// Evaluation diagnostics; whether execution stopped is reported separately.
    pub(crate) action_diagnostics: Vec<ActionError>,
    /// Guards match-time predicate draining against evaluator-triggered assertions.
    pub(crate) processing_predicates: bool,
    /// Whether a halt has been requested.
    pub(crate) halted: bool,
    /// Input buffer for `read`/`readline` calls from rules.
    pub(crate) input_buffer: VecDeque<String>,
}

impl Engine {
    /// Remove executable metadata and reclaim only rule-exclusive graph state.
    pub(crate) fn remove_compiled_rules(&mut self, rules: &[RuleId]) {
        if rules.is_empty() {
            return;
        }
        for rule in rules {
            if let Some(slot) = self.rule_info.get_mut(rule.0 as usize) {
                *slot = None;
            }
            if let Some(slot) = self.rule_modules.get_mut(rule.0 as usize) {
                *slot = None;
            }
        }
        self.compiler.remove_rules(&mut self.rete, rules);
    }

    /// Requested callable-depth limit, retained for configuration and snapshots.
    #[must_use]
    pub fn max_call_depth(&self) -> usize {
        self.config.max_call_depth
    }

    /// Callable-depth ceiling applied by evaluation (at most 32).
    #[must_use]
    pub fn effective_max_call_depth(&self) -> usize {
        self.config.effective_max_call_depth()
    }

    /// Create a new engine with the given configuration.
    #[must_use]
    pub fn new(config: EngineConfig) -> Self {
        let strategy = config.strategy;
        Self {
            fact_base: FactBase::new(),
            host: HostState::new(),
            symbol_table: SymbolTable::new(),
            config,
            rete: ReteNetwork::with_strategy(strategy),
            compiler: ReteCompiler::new(),
            registered_deffacts: Vec::new(),
            rule_info: Vec::new(),
            template_ids: HashMap::default(),
            template_defs: slotmap::SlotMap::with_key(),
            template_local_ids: crate::loader::TemplateLocalIndex::default(),
            router: OutputRouter::new(),
            functions: FunctionEnv::new(),
            globals: GlobalStore::new(),
            registered_globals: Vec::new(),
            generics: GenericRegistry::new(),
            module_registry: ModuleRegistry::new(),
            rule_modules: Vec::new(),
            template_modules: slotmap::SecondaryMap::new(),
            function_modules: HashMap::default(),
            global_modules: HashMap::default(),
            generic_modules: HashMap::default(),
            initial_fact_id: None,
            action_diagnostics: Vec::new(),
            processing_predicates: false,
            halted: false,
            input_buffer: VecDeque::new(),
        }
    }

    /// Create an engine, load CLIPS source, and reset — all in one call.
    ///
    /// Uses the default configuration ([`EngineConfig::default()`], which is UTF-8).
    /// Equivalent to:
    /// ```ignore
    /// let mut engine = Engine::new(EngineConfig::default());
    /// engine.load_str(source)?;
    /// engine.reset()?;
    /// ```
    ///
    /// For access to the [`LoadResult`](crate::loader::LoadResult) (warnings,
    /// parsed constructs), use the three-step manual flow instead.
    ///
    /// # Errors
    ///
    /// Returns [`InitError::Load`] if parsing/loading fails, or
    /// [`InitError::Reset`] if the post-load reset fails.
    pub fn with_rules(source: &str) -> Result<Self, InitError> {
        Self::with_rules_config(source, EngineConfig::default())
    }

    /// Create an engine with explicit configuration, load CLIPS source, and reset.
    ///
    /// # Errors
    ///
    /// Returns [`InitError::Load`] if parsing/loading fails, or
    /// [`InitError::Reset`] if the post-load reset fails.
    pub fn with_rules_config(source: &str, config: EngineConfig) -> Result<Self, InitError> {
        let mut engine = Self::new(config);
        engine.load_str(source).map_err(InitError::Load)?;
        engine.reset().map_err(InitError::Reset)?;
        Ok(engine)
    }

    /// Return whether structurally equivalent facts may coexist.
    ///
    /// The CLIPS-compatible default is `false`.
    #[must_use]
    pub fn fact_duplication(&self) -> bool {
        self.config.fact_duplication()
    }

    /// Change whether structurally equivalent facts may coexist.
    ///
    /// Returns the previous setting, matching CLIPS
    /// `set-fact-duplication` semantics. Existing duplicates are not removed
    /// when duplication is disabled.
    pub fn set_fact_duplication(&mut self, enabled: bool) -> bool {
        self.config.set_fact_duplication(enabled)
    }

    pub(crate) fn assert_fact_internal(
        &mut self,
        fact: Fact,
    ) -> Result<FactAssertionResult<FactId>, EngineError> {
        Ok(
            match self
                .fact_base
                .try_assert_fact(fact, self.fact_duplication())?
            {
                FactInsertionResult::Inserted(fact_id) => {
                    // CLIPS starts pattern evaluation with a clear Error bit,
                    // while retaining a failure's sticky HaltExecution state.
                    self.globals.clear_evaluation_error();
                    propagate_fact_assertion(&mut self.rete, &self.fact_base, fact_id);
                    self.drain_pending_predicate_matches();
                    FactAssertionResult::Asserted(fact_id)
                }
                FactInsertionResult::Duplicate(fact_id) => FactAssertionResult::Duplicate(fact_id),
            },
        )
    }

    pub(crate) fn drain_pending_predicate_matches(&mut self) {
        if self.processing_predicates {
            return;
        }
        self.processing_predicates = true;

        while let Some(pending) = self.rete.pop_pending_runtime_match() {
            let Some(bindings) = self.rete.runtime_match_bindings(&pending, &self.fact_base) else {
                // A candidate can disappear before its callback is reached.
                // Let the core validate/advance any still-live lazy search.
                self.rete
                    .resolve_runtime_match(pending, false, &self.fact_base);
                continue;
            };
            let Some(info) = rule_index_get(&self.rule_info, pending.rule).cloned() else {
                self.action_diagnostics.push(ActionError::EvalError(format!(
                    "internal invariant violation: predicate for rule {:?} has no executable metadata",
                    pending.rule
                )));
                continue;
            };
            let Some(condition) = info.test_conditions.get(pending.condition_index as usize) else {
                self.action_diagnostics.push(ActionError::EvalError(format!(
                    "rule `{}` references missing match condition {}",
                    info.name, pending.condition_index
                )));
                continue;
            };
            if condition.role != pending.role {
                self.action_diagnostics.push(ActionError::EvalError(format!(
                    "rule `{}` has a mismatched role for match condition {}",
                    info.name, pending.condition_index
                )));
                continue;
            }
            let Some(current_module) = rule_index_get(&self.rule_modules, pending.rule).copied()
            else {
                self.action_diagnostics.push(ActionError::EvalError(format!(
                    "internal invariant violation: predicate for rule {:?} has no module metadata",
                    pending.rule
                )));
                continue;
            };
            let evaluation = if pending.role
                == ferric_rules_core::RuntimeConditionRole::PositiveJoin
                && !info.multifield_tail_bindings.is_empty()
            {
                let Some(parent) = pending.parent_token else {
                    continue;
                };
                let Some(token) = self.rete.token_store.get(parent).cloned() else {
                    self.rete
                        .resolve_runtime_match(pending, false, &self.fact_base);
                    continue;
                };
                let collected_facts = self.rete.token_store.collect_all_facts(parent);
                let mut context = actions::ActionExecutionContext {
                    engine: self,
                    current_module,
                };
                actions::evaluate_test_condition(
                    &token,
                    info.as_ref(),
                    condition,
                    &collected_facts,
                    &mut context,
                )
            } else {
                let mut context = actions::ActionExecutionContext {
                    engine: self,
                    current_module,
                };
                Ok(actions::evaluate_runtime_condition(
                    &bindings,
                    &info.var_map,
                    condition,
                    pending.replacing_conflict,
                    &mut context,
                ))
            };
            let passed = match evaluation {
                Ok(passed) => passed,
                Err(error) => {
                    ferric_event!(
                        warn,
                        rule = %info.name,
                        error = %error,
                        "match_condition_eval_error"
                    );
                    self.action_diagnostics.push(error);
                    false
                }
            };

            self.rete
                .resolve_runtime_match(pending, passed, &self.fact_base);
        }

        self.processing_predicates = false;
        self.drain_evaluator_diagnostics();
    }

    fn host_fields(
        &self,
        fields: impl IntoHostFields,
    ) -> Result<smallvec::SmallVec<[Value; 8]>, EngineError> {
        let fields = fields.into_host_fields().0;
        let mut remaining = HOST_VALUE_MAX_ITEMS;
        for value in &fields {
            value.validate(
                Some(self.host.owner),
                self.config.string_encoding,
                &mut remaining,
            )?;
        }
        Ok(fields.into_iter().map(|value| value.value).collect())
    }

    fn validate_ordered_host_relation(&self, relation: &str) -> Result<(), EngineError> {
        // Ordered identities are global in the current RETE representation.
        // Match the loader's inverse guard by local name, including private
        // and module-qualified template definitions.
        let local = relation.rsplit("::").next().unwrap_or(relation);
        if self
            .template_defs
            .values()
            .any(|definition| definition.name.rsplit("::").next() == Some(local))
        {
            return Err(EngineError::InvalidHostValue(format!(
                "ordered relation `{relation}` conflicts with an explicit template; use assert_template_slots"
            )));
        }
        Ok(())
    }

    fn host_assertion_result(
        &mut self,
        result: FactAssertionResult<FactId>,
    ) -> FactAssertionResult {
        self.globals.take_evaluation_halt();
        self.globals.take_sort_return();
        self.host.prune(&self.fact_base);
        match result {
            FactAssertionResult::Asserted(id) => {
                FactAssertionResult::Asserted(self.host.export(id))
            }
            FactAssertionResult::Duplicate(id) => {
                FactAssertionResult::Duplicate(self.host.export(id))
            }
        }
    }

    /// Assert an ordered fact into working memory.
    ///
    /// The relation name is interned as a symbol. Fields can be passed as a
    /// `Vec<Value>`, a single `Value`, a primitive (`i64`, `i32`, `f64`),
    /// a `SymbolHandle`, a `FerricString`, or a fixed-size array of host values.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// engine.assert_ordered("count", 42_i64)?;           // single integer
    /// engine.assert_ordered("tier", free_symbol)?;        // single SymbolHandle
    /// engine.assert_ordered("pair", vec![v1, v2])?;       // multiple values
    /// engine.assert_ordered("empty", ())?;            // no fields
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The relation name violates encoding constraints (e.g., non-ASCII in ASCII mode)
    pub fn assert_ordered<F: IntoHostFields>(
        &mut self,
        relation: &str,
        fields: F,
    ) -> Result<FactHandle, EngineError> {
        Ok(self.assert_ordered_with_result(relation, fields)?.fact_id())
    }

    /// Assert an ordered fact and report whether it was newly inserted.
    ///
    /// When fact duplication is disabled, structural equality compares the
    /// relation and every ordered field. A rejected duplicate returns
    /// [`FactAssertionResult::Duplicate`] with the oldest equivalent active
    /// fact ID and creates no new working-memory or RETE state.
    ///
    /// # Errors
    ///
    /// Returns an error if the relation violates encoding constraints.
    pub fn assert_ordered_with_result<F: IntoHostFields>(
        &mut self,
        relation: &str,
        fields: F,
    ) -> Result<FactAssertionResult, EngineError> {
        ferric_span!(info_span, "engine_assert_ordered", relation);

        self.validate_ordered_host_relation(relation)?;
        let fields_small = self.host_fields(fields)?;
        let relation_sym = self
            .symbol_table
            .intern_symbol(relation, self.config.string_encoding)?;

        let result = self.assert_fact_internal(Fact::Ordered(ferric_rules_core::OrderedFact {
            relation: relation_sym,
            fields: fields_small,
        }))?;
        Ok(self.host_assertion_result(result))
    }

    /// Reassert a captured owned fact after checking engine ownership and template shape.
    pub fn assert(&mut self, fact: HostFact) -> Result<FactHandle, EngineError> {
        Ok(self.assert_with_result(fact)?.fact_id())
    }

    /// Reassert a captured owned fact and report whether it was newly inserted.
    pub fn assert_with_result(
        &mut self,
        fact: HostFact,
    ) -> Result<FactAssertionResult, EngineError> {
        ferric_span!(info_span, "engine_assert");

        if fact.owner != self.host.owner {
            return Err(EngineError::ForeignHandle);
        }
        if let Fact::Ordered(ordered) = &fact.fact {
            let relation = self
                .resolve_core_symbol(ordered.relation)
                .ok_or(EngineError::ForeignHandle)?;
            self.validate_ordered_host_relation(relation)?;
        }
        if let Fact::Template(template) = &fact.fact {
            let definition = self
                .template_defs
                .get(template.template_id)
                .ok_or(EngineError::ForeignHandle)?;
            if fact
                .template
                .as_ref()
                .map_or(true, |captured| !captured.same_shape(definition))
            {
                return Err(EngineError::ForeignHandle);
            }
            definition
                .validate_slots(&template.slots)
                .map_err(EngineError::InvalidHostValue)?;
        }
        let result = self.assert_fact_internal(fact.fact)?;
        Ok(self.host_assertion_result(result))
    }

    /// Assert a template fact by template name and named slot values.
    ///
    /// Looks up the template definition, resolves each slot name to its
    /// positional index, fills in defaults for any unspecified slots, and
    /// asserts the resulting fact into working memory.
    ///
    /// `slot_names` and `slot_values` must have the same length. Each
    /// `slot_names[i]` is matched to the corresponding `slot_values[i]`.
    /// Prefer [`Self::assert_template_slots`] when constructing new callers.
    /// Single-field slots require scalar values. A scalar supplied for a
    /// multislot becomes a one-element multifield; multifields retain all items.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The template name is not registered
    /// - A slot name does not exist in the template
    pub fn assert_template(
        &mut self,
        template_name: &str,
        slot_names: &[&str],
        slot_values: impl IntoHostFields,
    ) -> Result<FactHandle, EngineError> {
        Ok(self
            .assert_template_with_result(template_name, slot_names, slot_values)?
            .fact_id())
    }

    /// Assert a template fact and report whether it was newly inserted.
    ///
    /// When fact duplication is disabled, structural equality compares the
    /// template identity and every positional slot value after defaults and
    /// named overrides have been applied.
    ///
    /// # Errors
    ///
    /// Returns an error for an unknown template or slot.
    pub fn assert_template_with_result(
        &mut self,
        template_name: &str,
        slot_names: &[&str],
        slot_values: impl IntoHostFields,
    ) -> Result<FactAssertionResult, EngineError> {
        let slot_values = self.host_fields(slot_values)?;
        if slot_names.len() != slot_values.len() {
            return Err(EngineError::SlotCountMismatch {
                names: slot_names.len(),
                values: slot_values.len(),
            });
        }

        let tid = *self
            .template_ids
            .get(template_name)
            .ok_or_else(|| EngineError::TemplateNotFound(template_name.to_string()))?;

        let def = self
            .template_defs
            .get(tid)
            .ok_or_else(|| EngineError::TemplateNotFound(template_name.to_string()))?;

        // Start with default values for all slots.
        let mut slots = def.defaults.clone().into_boxed_slice();

        // Validate every override before mutating working memory.
        let mut seen = vec![false; slots.len()];
        for (name, value) in slot_names.iter().zip(slot_values) {
            let idx = def
                .slot_index(name)
                .ok_or_else(|| EngineError::SlotNotFound {
                    template: template_name.to_string(),
                    slot: (*name).to_string(),
                })?;
            if std::mem::replace(&mut seen[idx], true) {
                return Err(EngineError::DuplicateSlot {
                    template: template_name.to_owned(),
                    slot: (*name).to_owned(),
                });
            }
            let invalid = |reason: &str| EngineError::InvalidSlotValue {
                template: template_name.to_owned(),
                slot: (*name).to_owned(),
                reason: reason.to_owned(),
            };
            if matches!(value, Value::Void) {
                return Err(invalid("a void value cannot be stored in a fact"));
            }
            slots[idx] = match (def.slot_types[idx], value) {
                (ferric_rules_parser::SlotType::Single, Value::Multifield(_)) => {
                    return Err(invalid("a single-field slot requires one scalar value"));
                }
                (ferric_rules_parser::SlotType::Multi, value @ Value::Multifield(_)) => value,
                (ferric_rules_parser::SlotType::Multi, scalar) => {
                    Value::Multifield(Box::new(std::iter::once(scalar).collect()))
                }
                (_, scalar) => scalar,
            };
        }
        for (index, value) in slots.iter().enumerate() {
            def.validate_slot(index, value)
                .map_err(|reason| EngineError::InvalidSlotValue {
                    template: template_name.to_owned(),
                    slot: def.slot_names[index].clone(),
                    reason,
                })?;
        }

        let fact = Fact::Template(TemplateFact {
            template_id: tid,
            slots,
        });

        let result = self.assert_fact_internal(fact)?;
        Ok(self.host_assertion_result(result))
    }

    /// Assert named template slot/value pairs, applying defaults to omitted slots.
    ///
    /// This form keeps each name with its value; unlike parallel collections it
    /// cannot contain mismatched counts. Duplicate/unknown slots and invalid
    /// single-field values are rejected before working memory changes.
    ///
    /// # Errors
    /// Returns the same slot, template and encoding errors as [`Self::assert_template`].
    pub fn assert_template_slots<'a, V: Into<HostValue>>(
        &mut self,
        template_name: &str,
        slots: impl IntoIterator<Item = (&'a str, V)>,
    ) -> Result<FactHandle, EngineError> {
        let (names, values): (Vec<_>, Vec<_>) = slots.into_iter().unzip();
        self.assert_template(template_name, &names, values)
    }

    /// Get the value of a template fact's slot by name.
    ///
    /// Returns the slot value for the given fact and slot name. The fact must
    /// be a template fact; ordered facts produce an error.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The fact ID does not exist
    /// - The ID identifies the protected internal `(initial-fact)`
    /// - The fact is not a template fact
    /// - The slot name does not exist in the template
    pub fn get_fact_slot_by_name(
        &self,
        fact_id: FactHandle,
        slot_name: &str,
    ) -> Result<&Value, EngineError> {
        let handle = fact_id;
        let fact_id = self
            .host
            .resolve(handle)
            .ok_or(EngineError::FactNotFound(handle))?;
        if Some(fact_id) == self.initial_fact_id {
            return Err(EngineError::FactNotFound(handle));
        }

        let fact = self
            .fact_base
            .get(fact_id)
            .ok_or(EngineError::FactNotFound(handle))?;

        match &fact.fact {
            Fact::Template(t) => {
                let def = self
                    .template_defs
                    .get(t.template_id)
                    .ok_or(EngineError::FactNotFound(handle))?;

                let idx = def
                    .slot_index(slot_name)
                    .ok_or_else(|| EngineError::SlotNotFound {
                        template: def.name.clone(),
                        slot: slot_name.to_string(),
                    })?;

                t.slots.get(idx).ok_or(EngineError::FactNotFound(handle))
            }
            Fact::Ordered(_) => Err(EngineError::NotATemplateFact(handle)),
        }
    }

    /// Retract a fact from working memory.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The fact ID does not exist
    pub fn retract(&mut self, fact_id: FactHandle) -> Result<(), EngineError> {
        let handle = fact_id;
        let fact_id = self
            .host
            .resolve(handle)
            .ok_or(EngineError::FactNotFound(handle))?;
        ferric_span!(info_span, "engine_retract", fact_id = ?fact_id);

        if Some(fact_id) == self.initial_fact_id {
            return Err(EngineError::ProtectedInitialFact);
        }
        let entry = self
            .fact_base
            .get(fact_id)
            .ok_or(EngineError::FactNotFound(handle))?;
        // Retract from rete first (needs fact_base for negative node handling).
        // Rete borrows the fact base throughout cleanup, so no owned copy is needed.
        self.rete
            .retract_fact(fact_id, &entry.fact, &self.fact_base);

        // Then retract from fact base
        self.fact_base
            .retract(fact_id)
            .ok_or(EngineError::FactNotFound(handle))?;
        self.drain_pending_predicate_matches();
        self.host.remove(fact_id);
        self.globals.take_evaluation_halt();
        self.globals.take_sort_return();

        Ok(())
    }

    /// Get a user-visible fact by ID.
    ///
    /// Returns `None` for the protected internal `(initial-fact)`, consistently
    /// with `facts()` and `find_facts()`.
    ///
    /// The `Result` return type is retained for API compatibility.
    pub fn get_fact(&self, fact_id: FactHandle) -> Result<Option<&Fact>, EngineError> {
        let handle = fact_id;
        let Some(fact_id) = self.host.resolve(handle) else {
            return Ok(None);
        };
        if Some(fact_id) == self.initial_fact_id {
            return Ok(None);
        }
        Ok(self.fact_base.get(fact_id).map(|entry| &entry.fact))
    }

    /// Count user-visible facts without allocating or exporting host handles.
    #[must_use]
    pub fn fact_count(&self) -> usize {
        self.fact_base.len() - usize::from(self.initial_fact_id.is_some())
    }

    /// Iterate over all user-visible facts in working memory.
    ///
    /// Returns an iterator of `(FactHandle, &Fact)` pairs. The synthetic
    /// protected `(initial-fact)` used by explicit initial-fact patterns is excluded
    /// from the results.
    ///
    /// The `Result` return type is retained for API compatibility.
    pub fn facts(&self) -> Result<impl Iterator<Item = (FactHandle, &Fact)>, EngineError> {
        let exclude_id = self.initial_fact_id;
        Ok(self
            .fact_base
            .iter()
            .filter(move |(id, _)| Some(*id) != exclude_id)
            .map(|(id, entry)| (self.host.export(id), &entry.fact)))
    }

    /// Find ordered facts by relation name.
    ///
    /// Returns a vector of `(FactHandle, &Fact)` pairs for all ordered facts
    /// whose relation matches the given name. Returns an empty vector if the
    /// relation name has not been interned or no matching facts exist.
    ///
    /// The `Result` return type is retained for API compatibility.
    pub fn find_facts(&self, relation: &str) -> Result<Vec<(FactHandle, &Fact)>, EngineError> {
        // Look up the relation symbol without interning it (read-only query).
        let Some(relation_sym) = self
            .symbol_table
            .find_symbol(relation, self.config.string_encoding)
        else {
            return Ok(Vec::new());
        };

        Ok(self
            .fact_base
            .facts_by_relation(relation_sym)
            .filter_map(|fid| {
                if Some(fid) == self.initial_fact_id {
                    return None;
                }
                let entry = self.fact_base.get(fid)?;
                Some((self.host.export(fid), &entry.fact))
            })
            .collect())
    }

    /// Intern a symbol.
    ///
    /// Symbols are interned strings that are cheap to copy and compare.
    /// The same symbol name always returns the same `Symbol` value within
    /// this engine.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The string violates encoding constraints
    pub fn intern_symbol(&mut self, s: &str) -> Result<SymbolHandle, EngineError> {
        self.intern_symbol_bytes(s.as_bytes())
    }

    /// Intern a byte-preserving symbol with this engine's ownership.
    ///
    /// # Errors
    /// Returns an error if bytes violate the configured encoding constraints.
    pub fn intern_symbol_bytes(&mut self, bytes: &[u8]) -> Result<SymbolHandle, EngineError> {
        Ok(SymbolHandle {
            owner: self.host.owner,
            symbol: self
                .symbol_table
                .intern_symbol_bytes(bytes, self.config.string_encoding)?,
        })
    }

    /// Create a `FerricString` from a string slice.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The string violates encoding constraints
    pub fn create_string(&self, s: &str) -> Result<FerricString, EngineError> {
        Ok(FerricString::new(s, self.config.string_encoding)?)
    }

    /// Create a string preserving its exact bytes.
    ///
    /// # Errors
    /// Returns an error if bytes violate the configured encoding constraints.
    pub fn create_string_bytes(&self, bytes: &[u8]) -> Result<FerricString, EngineError> {
        Ok(FerricString::from_bytes(
            bytes,
            self.config.string_encoding,
        )?)
    }

    /// Intern a symbol as a host value retaining this engine's ownership.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The string violates encoding constraints
    pub fn symbol_value(&mut self, s: &str) -> Result<HostValue, EngineError> {
        Ok(self.intern_symbol(s)?.into())
    }

    /// Intern exact symbol bytes as an owned host value.
    ///
    /// # Errors
    /// Returns an error if bytes violate the configured encoding constraints.
    pub fn symbol_value_bytes(&mut self, bytes: &[u8]) -> Result<HostValue, EngineError> {
        Ok(self.intern_symbol_bytes(bytes)?.into())
    }

    /// Intern a typed instance name, without creating a COOL instance.
    ///
    /// # Errors
    /// Returns an error if the name violates encoding constraints.
    pub fn intern_instance_name(&mut self, name: &str) -> Result<InstanceNameHandle, EngineError> {
        self.intern_instance_name_bytes(name.as_bytes())
    }

    /// Intern an instance name from its unbracketed byte spelling.
    ///
    /// # Errors
    /// Returns an error if bytes violate the configured encoding constraints.
    pub fn intern_instance_name_bytes(
        &mut self,
        bytes: &[u8],
    ) -> Result<InstanceNameHandle, EngineError> {
        let symbol = self.intern_symbol_bytes(bytes)?;
        Ok(InstanceNameHandle {
            owner: symbol.owner,
            name: ferric_rules_core::InstanceName::from_symbol(symbol.symbol),
        })
    }

    /// Create a typed instance-name host value with this engine's ownership.
    ///
    /// # Errors
    /// Returns an error if the name violates encoding constraints.
    pub fn instance_name_value(&mut self, name: &str) -> Result<HostValue, EngineError> {
        Ok(self.intern_instance_name(name)?.into())
    }

    /// Create a typed instance-name value from its exact unbracketed bytes.
    ///
    /// # Errors
    /// Returns an error if bytes violate the configured encoding constraints.
    pub fn instance_name_value_bytes(&mut self, bytes: &[u8]) -> Result<HostValue, EngineError> {
        Ok(self.intern_instance_name_bytes(bytes)?.into())
    }

    /// Assert a single-field ordered fact whose value is a symbol.
    ///
    /// Combines symbol interning and fact assertion into one call. Equivalent to:
    /// ```ignore
    /// let sym = engine.intern_symbol(symbol_name)?;
    /// engine.assert_ordered(relation, sym)?;
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Either name violates encoding constraints
    pub fn assert_ordered_symbol(
        &mut self,
        relation: &str,
        symbol_name: &str,
    ) -> Result<FactHandle, EngineError> {
        let value = self.symbol_value(symbol_name)?;
        self.assert_ordered(relation, value)
    }

    /// Return the CLIPS `TRUE` symbol as an owned host value.
    ///
    /// The symbol is interned on first use and cached thereafter.
    pub fn clips_true(&mut self) -> Result<HostValue, EngineError> {
        self.symbol_value("TRUE")
    }

    /// Return the CLIPS `FALSE` symbol as an owned host value.
    ///
    /// The symbol is interned on first use and cached thereafter.
    pub fn clips_false(&mut self) -> Result<HostValue, EngineError> {
        self.symbol_value("FALSE")
    }

    /// Resolve a [`Symbol`] to its string representation.
    ///
    /// Returns `None` for unknown/foreign symbols or non-UTF-8 payloads.
    /// Use [`Self::resolve_symbol_bytes`] to inspect arbitrary bytes.
    /// Symbol table contents are immutable once interned.
    #[must_use]
    pub fn resolve_symbol(&self, sym: SymbolHandle) -> Option<&str> {
        (sym.owner == self.host.owner)
            .then(|| self.symbol_table.resolve_symbol_str(sym.symbol))
            .flatten()
    }

    /// Inspect a core symbol taken from this engine's borrowed RETE/fact state.
    /// Core keys have no provenance; use `resolve_symbol` with a `SymbolHandle`
    /// in ordinary host code. Do not pass a key extracted from another engine.
    /// Returns `None` for unknown symbols or non-UTF-8 payloads.
    #[must_use]
    pub fn resolve_core_symbol(&self, symbol: Symbol) -> Option<&str> {
        self.symbol_table.resolve_symbol_str(symbol)
    }

    /// Resolve owned symbol bytes; foreign handles return `None`.
    #[must_use]
    pub fn resolve_symbol_bytes(&self, symbol: SymbolHandle) -> Option<&[u8]> {
        (symbol.owner == self.host.owner)
            .then(|| self.symbol_table.resolve_symbol_bytes(symbol.symbol))
            .flatten()
    }

    /// Inspect exact bytes of a core symbol borrowed from this engine's state.
    #[must_use]
    pub fn resolve_core_symbol_bytes(&self, symbol: Symbol) -> Option<&[u8]> {
        self.symbol_table.resolve_symbol_bytes(symbol)
    }

    /// Resolve an owned instance name's unbracketed bytes.
    #[must_use]
    pub fn resolve_instance_name_bytes(&self, name: InstanceNameHandle) -> Option<&[u8]> {
        (name.owner == self.host.owner)
            .then(|| {
                self.symbol_table
                    .resolve_symbol_bytes(name.name.as_symbol())
            })
            .flatten()
    }

    /// Resolve an owned instance name as text, checking its byte encoding.
    ///
    /// # Errors
    /// Returns the UTF-8 error when the name contains invalid text bytes.
    pub fn resolve_instance_name(
        &self,
        name: InstanceNameHandle,
    ) -> Result<Option<&str>, std::str::Utf8Error> {
        self.resolve_instance_name_bytes(name)
            .map(std::str::from_utf8)
            .transpose()
    }

    /// Clone an engine-owned fact for checked reassertion or owned value access.
    ///
    /// # Errors
    /// Returns an error for an unknown, stale or foreign fact handle.
    pub fn get_fact_owned(&self, handle: FactHandle) -> Result<Option<HostFact>, EngineError> {
        let Some(fact) = self.get_fact(handle)? else {
            return Ok(None);
        };
        let template = if let Fact::Template(fact) = fact {
            let definition = self
                .template_defs
                .get(fact.template_id)
                .ok_or(EngineError::FactNotFound(handle))?;
            Some(definition.clone())
        } else {
            None
        };
        Ok(Some(HostFact {
            owner: self.host.owner,
            fact: fact.clone(),
            template,
        }))
    }

    /// Access the engine's Rete network for inspection.
    #[must_use]
    pub fn rete(&self) -> &ReteNetwork {
        &self.rete
    }

    /// List the names and salience values of all registered rules.
    ///
    /// Returns a vector of `(name, salience)` pairs for every rule
    /// that has been compiled into the Rete network.
    pub fn rules(&self) -> Vec<(&str, i32)> {
        let mut rules = Vec::with_capacity(self.rule_info.len().saturating_sub(1));
        for info in self.rule_info.iter().skip(1).flatten() {
            rules.push((info.name.as_str(), info.salience.get()));
        }
        rules
    }

    /// List the names of all registered templates.
    pub fn templates(&self) -> Vec<&str> {
        self.template_ids.keys().map(Box::as_ref).collect()
    }

    /// Return the slot names for a named template, or `None` if the template
    /// does not exist.
    pub fn template_slot_names(&self, name: &str) -> Option<Vec<&str>> {
        let tid = self.template_ids.get(name)?;
        let def = self.template_defs.get(*tid)?;
        Some(def.slot_names.iter().map(String::as_str).collect())
    }

    /// Return the slot names for a template by its `TemplateId`, or `None`
    /// if the ID is not registered.
    pub fn template_slot_names_by_id(&self, tid: TemplateId) -> Option<Vec<&str>> {
        let def = self.template_defs.get(tid)?;
        Some(def.slot_names.iter().map(String::as_str).collect())
    }

    /// Return the template name for a `TemplateId`, or `None` if the ID
    /// is not registered.
    pub fn template_name_by_id(&self, tid: TemplateId) -> Option<&str> {
        self.template_defs.get(tid).map(|def| def.name.as_str())
    }

    /// Return the owning module name for a `TemplateId`, or `None` if the ID
    /// or its recorded module is not registered.
    pub fn template_module_name_by_id(&self, tid: TemplateId) -> Option<&str> {
        let module_id = self.template_modules.get(tid)?;
        self.module_registry.module_name(*module_id)
    }

    /// Return the names of all registered modules.
    pub fn modules(&self) -> Vec<&str> {
        self.module_registry.module_names()
    }

    /// Look up a rule name by its internal ID.
    ///
    /// Returns `None` if the ID does not correspond to a known rule.
    pub fn rule_name(&self, rule_id: RuleId) -> Option<&str> {
        rule_index_get(&self.rule_info, rule_id).map(|info| info.name.as_str())
    }

    /// Compatibility shim for the former thread-affinity contract.
    ///
    /// Always succeeds: an exclusively owned engine may be used on any thread.
    #[deprecated(note = "Engine is Send; thread-affinity checks are no longer necessary")]
    pub fn check_thread_affinity(&self) -> Result<(), EngineError> {
        Ok(())
    }

    /// Execute RHS actions for a rule activation.
    ///
    /// Returns `(logically_fired, reset_requested, clear_requested, action_error)`.
    /// - `logically_fired` is `true` if the already-matched activation executed.
    /// - `reset_requested` is `true` if a `(reset)` action was executed in the RHS.
    /// - `clear_requested` is `true` if a `(clear)` action was executed in the RHS.
    /// - `action_error` is `true` if evaluation failed and stopped the RHS.
    fn execute_activation_actions(
        &mut self,
        rule_id: RuleId,
        token_id: ferric_rules_core::token::TokenId,
    ) -> (bool, bool, bool, bool) {
        ferric_span!(debug_span, "fire_rule", rule = rule_id.0);
        let Some(token) = self.rete.token_store.get(token_id).cloned() else {
            ferric_event!(debug, rule = rule_id.0, token = ?token_id, "activation_missing_token");
            self.action_diagnostics.push(ActionError::EvalError(format!(
                "internal invariant violation: activation for rule {rule_id:?} references missing token {token_id:?}"
            )));
            return (false, false, false, true);
        };

        // Clone the handle so we can pass both this rule and the full map to
        // action helpers without deep-cloning `CompiledRuleInfo`.
        let Some(info) = rule_index_get(&self.rule_info, rule_id).cloned() else {
            ferric_event!(debug, rule = rule_id.0, "activation_missing_rule_info");
            self.action_diagnostics.push(ActionError::EvalError(format!(
                "internal invariant violation: activation for rule {rule_id:?} has no executable metadata"
            )));
            return (false, false, false, true);
        };

        let Some(current_module) = rule_index_get(&self.rule_modules, rule_id).copied() else {
            self.action_diagnostics.push(ActionError::EvalError(format!(
                "internal invariant violation: activation for rule {rule_id:?} has no module metadata"
            )));
            return (false, false, false, true);
        };

        let collected_facts = self.rete.token_store.collect_all_facts(token_id);

        let (fired, reset_requested, clear_requested, evaluation_halted, errors) = {
            let mut action_context = actions::ActionExecutionContext {
                engine: self,
                current_module,
            };
            actions::execute_actions(&token, info.as_ref(), &mut action_context, &collected_facts)
        };
        // Retractions and modifications performed by RHS actions can unblock
        // negative nodes and create new predicate candidates.
        self.drain_pending_predicate_matches();

        ferric_event!(
            debug,
            rule = rule_id.0,
            fired,
            reset_requested,
            clear_requested,
            diagnostics = errors.len(),
            "activation_actions_complete"
        );
        let predicate_halted = self.globals.take_evaluation_halt();
        self.globals.take_sort_return();
        let action_error = evaluation_halted || predicate_halted || !errors.is_empty();
        self.action_diagnostics.extend(errors);
        (fired, reset_requested, clear_requested, action_error)
    }

    /// Compatibility shim for the former explicit ownership-transfer operation.
    ///
    /// This is a no-op; ordinary Rust ownership transfer is sufficient.
    #[deprecated(note = "Engine is Send; move it directly to the receiving thread")]
    pub fn move_to_current_thread(&mut self) {}

    /// Pop the next activation eligible under current focus semantics.
    ///
    /// Selection is module-aware: only activations whose rule belongs to the
    /// current focus module are eligible. If the current focus module has no
    /// eligible activations, the top focus is popped and selection continues,
    /// including the last focus. A new run defaults to MAIN when the stack is empty.
    fn pop_next_focus_activation(&mut self) -> Option<ferric_rules_core::Activation> {
        loop {
            let focus_module = self.module_registry.current_focus()?;
            let rule_info = &self.rule_info;
            let rule_modules = &self.rule_modules;

            if let Some(activation) = self.rete.agenda.pop_matching_rule(|rule| {
                if rule_index_get(rule_info, rule).is_none() {
                    return true;
                }
                match rule_index_get(rule_modules, rule) {
                    Some(module) => *module == focus_module,
                    None => true,
                }
            }) {
                return Some(activation);
            }

            self.module_registry.pop_focus();
        }
    }

    /// Fire a single rule activation from the agenda.
    ///
    /// Returns `None` when no activation is eligible under current focus
    /// semantics. Otherwise pops the highest-priority eligible activation and
    /// fires its already-matched RHS actions.
    ///
    /// The `Result` return type is retained for API compatibility.
    pub fn step(&mut self) -> Result<Option<FiredRule>, EngineError> {
        ferric_span!(info_span, "engine_step");
        self.action_diagnostics.clear();
        self.globals.take_evaluation_halt();
        self.globals.take_sort_return();
        if self.module_registry.current_focus().is_none() {
            self.module_registry
                .push_focus(self.module_registry.main_module_id());
        }

        let Some(activation) = self.pop_next_focus_activation() else {
            ferric_event!(debug, "engine_step_no_activation");
            self.drain_evaluator_diagnostics();
            return Ok(None);
        };

        let fired = FiredRule {
            rule_id: activation.rule,
            token_id: activation.token,
        };

        // Execute actions. Diagnostics remain available through
        // action_diagnostics(), while step() returns Some(fired) to indicate
        // that the activation was processed even when action evaluation fails.
        let (logically_fired, reset_requested, clear_requested, action_error) =
            self.execute_activation_actions(activation.rule, activation.token);
        #[cfg(not(feature = "tracing"))]
        let _ = (logically_fired, action_error);
        ferric_event!(
            debug,
            rule = activation.rule.0,
            token = ?activation.token,
            logically_fired,
            reset_requested,
            clear_requested,
            action_error,
            "engine_step_activation_processed"
        );

        if clear_requested {
            self.clear();
        } else if reset_requested {
            let _ = self.reset();
        }
        // After reset or clear, the engine is in a new state.
        // step() still returns the FiredRule indicating what fired.

        self.drain_evaluator_diagnostics();
        self.host.prune(&self.fact_base);
        Ok(Some(fired))
    }

    /// Run the engine, firing rules until the agenda is empty, the limit is
    /// reached, halt is requested, or an activation reports an action error.
    ///
    /// Clears any previous halt request before starting.
    ///
    /// Rule selection is focus-aware: only activations belonging to the module
    /// at the top of the focus stack are eligible to fire. When no eligible
    /// activations remain for the current focus module, the focus stack is
    /// popped and the next module is tried, leaving an empty focus stack on
    /// quiescence. A new run defaults to MAIN if the stack starts empty.
    ///
    /// The `Result` return type is retained for API compatibility.
    pub fn run(&mut self, limit: RunLimit) -> Result<RunResult, EngineError> {
        let result = self.run_inner(limit, true);
        self.drain_evaluator_diagnostics();
        self.host.prune(&self.fact_base);
        Ok(result)
    }

    /// Continue a count-limited run without clearing its halt flag or action
    /// diagnostics.
    ///
    /// This is an internal integration hook for hosts that split one logical
    /// run into bounded chunks. Call it only after [`Self::run`] or another
    /// continuation returned [`HaltReason::LimitReached`].
    ///
    /// The `Result` return type is retained for API compatibility.
    #[doc(hidden)]
    pub fn continue_run(&mut self, limit: RunLimit) -> Result<RunResult, EngineError> {
        let result = self.run_inner(limit, false);
        self.drain_evaluator_diagnostics();
        self.host.prune(&self.fact_base);
        Ok(result)
    }

    fn run_inner(&mut self, limit: RunLimit, clear_execution_state: bool) -> RunResult {
        ferric_span!(info_span, "engine_run", limit = ?limit);
        if clear_execution_state {
            self.halted = false;
            self.action_diagnostics.clear();
            self.globals.take_evaluation_halt();
            self.globals.take_sort_return();
            if self.module_registry.current_focus().is_none() {
                self.module_registry
                    .push_focus(self.module_registry.main_module_id());
            }
        }

        let max_fires = match limit {
            RunLimit::Unlimited => usize::MAX,
            RunLimit::Count(n) => n,
        };

        let mut rules_fired = 0;

        while rules_fired < max_fires {
            if self.halted {
                ferric_event!(
                    info,
                    rules_fired,
                    halt_reason = "halt_requested",
                    "engine_run_complete"
                );
                return run_result(rules_fired, HaltReason::HaltRequested);
            }

            // Exhausted focuses are popped until a matching activation is
            // found or the complete focus stack has drained.
            let Some(activation) = self.pop_next_focus_activation() else {
                ferric_event!(
                    info,
                    rules_fired,
                    halt_reason = "agenda_empty",
                    "engine_run_complete"
                );
                return run_result(rules_fired, HaltReason::AgendaEmpty);
            };

            let (logically_fired, reset_requested, clear_requested, action_error) =
                self.execute_activation_actions(activation.rule, activation.token);

            if logically_fired {
                rules_fired += 1;
            }
            ferric_event!(
                debug,
                rule = activation.rule.0,
                token = ?activation.token,
                logically_fired,
                reset_requested,
                clear_requested,
                action_error,
                rules_fired,
                "engine_run_activation_processed"
            );

            if action_error {
                ferric_event!(
                    info,
                    rules_fired,
                    halt_reason = "action_error",
                    "engine_run_complete"
                );
                return run_result(rules_fired, HaltReason::ActionError);
            }

            if clear_requested {
                self.clear();
                ferric_event!(
                    info,
                    rules_fired,
                    halt_reason = "clear_requested",
                    "engine_run_complete"
                );
                return run_result(rules_fired, HaltReason::HaltRequested);
            }

            if reset_requested {
                let _ = self.reset();
                // Stop execution after reset — the caller can invoke run() again
                // with the freshly-reset working memory.
                ferric_event!(
                    info,
                    rules_fired,
                    halt_reason = "reset_requested",
                    "engine_run_complete"
                );
                return run_result(rules_fired, HaltReason::HaltRequested);
            }
        }

        ferric_event!(
            info,
            rules_fired,
            halt_reason = "limit_reached",
            "engine_run_complete"
        );
        run_result(rules_fired, HaltReason::LimitReached)
    }

    /// Request that the engine stop execution after the current rule completes.
    ///
    /// This sets a flag that is checked between rule firings during `run`.
    /// Has no effect if the engine is not currently running.
    pub fn halt(&mut self) {
        ferric_event!(info, "engine_halt");
        self.halted = true;
    }

    /// Reset the engine: clear all facts, tokens, and activations, then
    /// assert the protected initial fact and all registered deffacts.
    ///
    /// Loading deffacts only registers seeds. Reset asserts them in module
    /// creation order, then definition order within each module. Replacing a
    /// named definition moves it to the end of that module's definition order.
    ///
    /// The compiled rule network is preserved — only runtime state is cleared.
    ///
    /// The `Result` return type is retained for API compatibility.
    pub fn reset(&mut self) -> Result<(), EngineError> {
        self.host.clear_facts();
        ferric_span!(info_span, "engine_reset");

        // Clear all runtime state
        self.fact_base = FactBase::new();
        self.initial_fact_id = None;
        self.rete.clear_working_memory();
        self.router.clear();
        self.action_diagnostics.clear();
        self.halted = false;
        // Note: input_buffer is intentionally NOT cleared on reset.
        // Input is live I/O state that should persist across resets.

        // Reset focus stack to [MAIN] and current module to MAIN
        self.module_registry.reset_focus();

        // Re-initialize globals from registered initial values
        self.globals.clear();
        for (module_id, name, value) in &self.registered_globals {
            self.globals.set(*module_id, name, value.clone());
        }
        self.drain_pending_predicate_matches();

        // Establish the built-in initial fact before any application seed.
        // Unconditional/leading-negative matches already use the non-fact root.
        let initial_sym = self
            .symbol_table
            .intern_symbol("initial-fact", self.config.string_encoding)?;
        let result = self.assert_fact_internal(Fact::Ordered(ferric_rules_core::OrderedFact {
            relation: initial_sym,
            fields: smallvec::SmallVec::new(),
        }))?;
        self.initial_fact_id = Some(result.fact_id());

        // CLIPS traverses modules in creation order, then each module's current
        // deffacts definitions in definition order. Stable sort preserves both.
        let mut definitions = self.registered_deffacts.clone();
        definitions.sort_by_key(|definition| definition.module.0);
        for definition in definitions {
            for fact in definition.facts {
                self.assert_fact_internal(fact)?;
            }
        }
        self.globals.take_evaluation_halt();
        self.globals.take_sort_return();

        Ok(())
    }

    /// Push a line of input for `read`/`readline` to consume.
    ///
    /// Entries are already framed by the caller and consumed in FIFO order.
    /// `read` skips empty or comment-only entries, returns the first scanned
    /// field, and discards that entry's remaining text. `readline` returns one
    /// complete entry, including an empty entry. Invalid input names and an
    /// inherited evaluation halt leave queued entries untouched.
    ///
    /// Embedded CR/LF characters are not split here. Reset preserves queued
    /// input; clear removes it.
    pub fn push_input(&mut self, line: &str) {
        self.input_buffer.push_back(line.to_string());
    }

    /// Clear the engine: remove all rules, facts, templates, functions, globals,
    /// and module definitions. Returns the engine to its initial empty state.
    ///
    /// Unlike `reset()`, which preserves compiled rules and templates,
    /// `clear()` removes everything.
    pub fn clear(&mut self) {
        self.host = HostState::new();
        ferric_span!(info_span, "engine_clear");
        self.fact_base = FactBase::new();
        self.rete = ReteNetwork::with_strategy(self.config.strategy);
        self.compiler = ReteCompiler::new();
        self.registered_deffacts.clear();
        self.rule_info.clear();
        self.template_ids.clear();
        self.template_defs = slotmap::SlotMap::with_key();
        self.template_local_ids.clear();
        self.router.clear();
        self.functions = FunctionEnv::new();
        self.globals = GlobalStore::new();
        self.registered_globals.clear();
        self.generics = GenericRegistry::new();
        self.module_registry = ModuleRegistry::new();
        self.rule_modules.clear();
        self.template_modules = slotmap::SecondaryMap::new();
        self.function_modules.clear();
        self.global_modules.clear();
        self.generic_modules.clear();
        self.initial_fact_id = None;
        self.action_diagnostics.clear();
        self.processing_predicates = false;
        self.halted = false;
        self.input_buffer.clear();
    }

    /// Check whether the engine is currently halted.
    #[must_use]
    pub fn is_halted(&self) -> bool {
        self.halted
    }

    /// Get the number of activations currently on the agenda.
    #[must_use]
    pub fn agenda_len(&self) -> usize {
        self.rete.agenda.len()
    }

    /// Get captured output for a named `printout` channel.
    ///
    /// Returns `None` if nothing has been written to that channel.
    ///
    /// # Errors
    /// Returns the UTF-8 error if captured bytes are not valid text.
    pub fn get_output(&self, channel: &str) -> Result<Option<&str>, std::str::Utf8Error> {
        self.router.get_output(channel)
    }

    /// Get captured output without interpreting its bytes as text.
    #[must_use]
    pub fn get_output_bytes(&self, channel: &str) -> Option<&[u8]> {
        self.router.get_output_bytes(channel)
    }

    /// Clear captured output for a named `printout` channel.
    pub fn clear_output_channel(&mut self, channel: &str) {
        self.router.clear_channel(channel);
    }

    /// Get evaluation diagnostics, including warnings that did not stop execution.
    ///
    /// A fresh run/step clears earlier diagnostics. Its outcome reports whether
    /// an action failed; a nonempty diagnostic list alone does not imply failure.
    #[must_use]
    pub fn action_diagnostics(&self) -> &[ActionError] {
        &self.action_diagnostics
    }

    pub(crate) fn drain_evaluator_diagnostics(&mut self) {
        self.action_diagnostics.extend(
            self.globals
                .take_diagnostics()
                .into_iter()
                .map(ActionError::from),
        );
    }

    /// Clear accumulated action diagnostics.
    pub fn clear_action_diagnostics(&mut self) {
        self.action_diagnostics.clear();
    }

    /// Get the current value of a global variable by name.
    ///
    /// Returns `None` if the variable has not been set.
    #[must_use]
    pub fn get_global(&self, name: &str) -> Option<&Value> {
        let current_module = self.module_registry.current_module();
        if let Some(value) = self.globals.get(current_module, name) {
            return Some(value);
        }

        let mut visible_module = None;
        for module_id in self.globals.modules_for_name(name) {
            if !self.module_registry.is_construct_visible(
                current_module,
                module_id,
                "defglobal",
                name,
            ) {
                continue;
            }

            match visible_module {
                None => visible_module = Some(module_id),
                Some(existing) if existing == module_id => {}
                Some(_) => return None,
            }
        }

        visible_module.and_then(|module_id| self.globals.get(module_id, name))
    }

    /// Get the name of the current module.
    #[must_use]
    pub fn current_module(&self) -> &str {
        self.module_registry
            .module_name(self.module_registry.current_module())
            .unwrap_or("MAIN")
    }

    /// Get the current focus module name (top of focus stack).
    #[must_use]
    pub fn get_focus(&self) -> Option<&str> {
        self.module_registry
            .current_focus()
            .and_then(|id| self.module_registry.module_name(id))
    }

    /// Get the full focus stack as module names (bottom to top).
    #[must_use]
    pub fn get_focus_stack(&self) -> Vec<&str> {
        self.module_registry
            .focus_stack()
            .iter()
            .filter_map(|id| self.module_registry.module_name(*id))
            .collect()
    }

    /// Set focus to exactly one module, replacing the previous focus stack.
    ///
    /// # Errors
    ///
    /// Returns `ModuleNotFound` if the module has not been registered.
    pub fn set_focus(&mut self, module_name: &str) -> Result<(), EngineError> {
        let module_id = self.resolve_focus_module(module_name)?;
        self.module_registry.set_focus(module_id);
        Ok(())
    }

    /// Push a module onto the focus stack by name.
    ///
    /// # Errors
    ///
    /// Returns `ModuleNotFound` if the module has not been registered.
    pub fn push_focus(&mut self, module_name: &str) -> Result<(), EngineError> {
        let module_id = self.resolve_focus_module(module_name)?;
        self.module_registry.push_focus(module_id);
        Ok(())
    }

    fn resolve_focus_module(&self, module_name: &str) -> Result<ModuleId, EngineError> {
        self.module_registry
            .get_by_name(module_name)
            .ok_or_else(|| EngineError::ModuleNotFound(module_name.to_string()))
    }

    /// Verify engine-level structural consistency.
    ///
    /// This extends rete consistency checks with Phase 3 registries
    /// (modules/focus, functions, globals, generics).
    #[cfg(any(test, debug_assertions))]
    #[allow(clippy::too_many_lines)]
    pub fn debug_assert_consistency(&self) {
        use std::collections::HashSet;
        self.rete
            .validate_consistency()
            .expect("RETE consistency violation");
        self.module_registry.debug_assert_consistency();
        self.functions.debug_assert_consistency();
        self.globals.debug_assert_consistency();
        self.generics.debug_assert_consistency();

        if let Some(id) = self.initial_fact_id {
            let fact = &self
                .fact_base
                .get(id)
                .expect("initial-fact ID must be live")
                .fact;
            assert!(
                matches!(fact, Fact::Ordered(fact) if fact.fields.is_empty()
                && self.resolve_core_symbol(fact.relation) == Some("initial-fact")),
                "initial-fact must be its reserved zero-field fact"
            );
        }
        let mut seed_names = HashSet::new();
        for definition in &self.registered_deffacts {
            assert!(self.module_registry.get(definition.module).is_some());
            assert!(
                seed_names.insert((definition.module, definition.name.as_str())),
                "duplicate deffacts identity"
            );
        }

        let rule_slot_count = self.rule_info.len().max(self.rule_modules.len());
        for index in 0..rule_slot_count {
            let info = self.rule_info.get(index).and_then(Option::as_ref);
            let module = self.rule_modules.get(index).and_then(Option::as_ref);
            assert_eq!(
                info.is_some(),
                module.is_some(),
                "rule metadata/module presence differs at rule slot {index}"
            );
        }

        for (index, maybe_module_id) in self.rule_modules.iter().enumerate() {
            let Some(module_id) = maybe_module_id else {
                continue;
            };
            #[allow(clippy::cast_possible_truncation)]
            let rule_id = RuleId(index as u32);
            assert!(
                self.module_registry.get(*module_id).is_some(),
                "rule {rule_id:?} points to unknown module {module_id:?}"
            );
        }

        let mut rules_with_terminals = HashSet::new();
        for (node_id, node) in self.rete.beta.iter_nodes() {
            let (rule_id, condition_index) = match node {
                ferric_rules_core::BetaNode::Predicate {
                    rule,
                    condition_index,
                    ..
                } => (*rule, Some(*condition_index)),
                ferric_rules_core::BetaNode::Terminal { rule, .. } => {
                    rules_with_terminals.insert(*rule);
                    (*rule, None)
                }
                _ => continue,
            };
            if self.rete.is_rule_disabled(rule_id) {
                continue;
            }
            let info = rule_index_get(&self.rule_info, rule_id).unwrap_or_else(|| {
                panic!("active beta node {node_id:?} references rule {rule_id:?} without metadata")
            });
            assert!(
                rule_index_get(&self.rule_modules, rule_id).is_some(),
                "active beta node {node_id:?} references rule {rule_id:?} without module metadata"
            );
            if let Some(condition_index) = condition_index {
                assert!(
                    info.test_conditions
                        .get(condition_index as usize)
                        .is_some(),
                    "predicate node {node_id:?} references missing condition {condition_index} for rule {rule_id:?}"
                );
            }
        }

        for (index, maybe_info) in self.rule_info.iter().enumerate() {
            if maybe_info.is_none() {
                continue;
            }
            #[allow(clippy::cast_possible_truncation)]
            let rule_id = RuleId(index as u32);
            assert!(
                rules_with_terminals.contains(&rule_id),
                "live rule {rule_id:?} has no terminal node"
            );
        }

        for activation in self.rete.agenda.iter_activations() {
            assert!(
                self.rete.token_store.get(activation.token).is_some(),
                "activation {:?} references missing token {:?}",
                activation.id,
                activation.token
            );
            assert!(
                rule_index_get(&self.rule_info, activation.rule).is_some(),
                "activation {:?} references rule {:?} without executable metadata",
                activation.id,
                activation.rule
            );
            assert!(
                rule_index_get(&self.rule_modules, activation.rule).is_some(),
                "activation {:?} references rule {:?} without module metadata",
                activation.id,
                activation.rule
            );
        }

        let expected = Self::build_template_local_index(&self.template_defs);
        assert_eq!(self.template_local_ids.len(), expected.len());
        for (name, ids) in &self.template_local_ids {
            let expected_ids = &expected[name];
            assert_eq!(ids.len(), expected_ids.len());
            assert!(expected_ids.iter().all(|id| ids.contains(id)));
        }

        for (template_id, module_id) in &self.template_modules {
            assert!(
                self.template_defs.contains_key(template_id),
                "template_modules contains unknown template id {template_id:?}"
            );
            assert!(
                self.module_registry.get(*module_id).is_some(),
                "template {template_id:?} points to unknown module {module_id:?}"
            );
        }

        let mut seen_globals = HashSet::new();
        for (module_id, name, _) in &self.registered_globals {
            assert!(
                seen_globals.insert((*module_id, name.as_str())),
                "duplicate registered global definition `{name}` in module {module_id:?}"
            );
            assert!(
                self.globals.contains(*module_id, name),
                "registered global `{name}` in module {module_id:?} missing from runtime global store"
            );
        }

        // Verify function module associations
        for (&module_id, local_names) in &self.functions.functions {
            for name in local_names.keys() {
                assert!(
                    crate::functions::contains_module_entry(
                        &self.function_modules,
                        module_id,
                        name
                    ),
                    "function `{name}` in module {module_id:?} missing from function_modules"
                );
            }
        }
        for (&module_id, local_names) in &self.function_modules {
            for (name, &owner_module) in local_names {
                assert!(
                    self.functions.contains(module_id, name),
                    "function_modules contains `{name}` in module {module_id:?} but function not registered"
                );
                assert!(
                    self.module_registry.get(owner_module).is_some(),
                    "function `{name}` points to unknown module {owner_module:?}"
                );
            }
        }

        // Verify global module associations
        for (&module_id, local_names) in &self.global_modules {
            for (name, &owner_module) in local_names {
                assert!(
                    self.globals.contains(module_id, name),
                    "global_modules contains `{name}` in module {module_id:?} but global not registered"
                );
                assert!(
                    self.module_registry.get(owner_module).is_some(),
                    "global `{name}` points to unknown module {owner_module:?}"
                );
            }
        }

        // Verify generic module associations
        for (&module_id, local_names) in &self.generic_modules {
            for (name, &owner_module) in local_names {
                assert!(
                    self.generics.contains(module_id, name),
                    "generic_modules contains `{name}` in module {module_id:?} but generic not registered"
                );
                assert!(
                    self.module_registry.get(owner_module).is_some(),
                    "generic `{name}` points to unknown module {owner_module:?}"
                );
            }
        }
    }
}

/// Errors that can occur during engine operations.
#[derive(Debug, Error)]
pub enum EngineError {
    #[error("handle belongs to a different or cleared engine, or its template has changed")]
    ForeignHandle,

    #[error("invalid host value: {0}")]
    InvalidHostValue(String),

    #[error("encoding error: {0}")]
    Encoding(#[from] EncodingError),

    #[error(transparent)]
    FactTimestampExhausted(#[from] ferric_rules_core::FactTimestampExhausted),

    #[error("fact not found: {0:?}")]
    FactNotFound(FactHandle),

    #[error("the internal initial-fact is protected and cannot be retracted")]
    ProtectedInitialFact,

    /// Retained for wrappers that impose their own thread-affinity contract.
    /// The Rust engine does not produce this error.
    #[error("engine called from wrong thread (created on {creator:?}, called from {current:?})")]
    WrongThread {
        creator: ThreadId,
        current: ThreadId,
    },

    #[error("module not found: {0}")]
    ModuleNotFound(String),

    #[error("template not found: {0}")]
    TemplateNotFound(String),

    #[error("fact {0:?} is not a template fact")]
    NotATemplateFact(FactHandle),

    #[error("slot not found: template \"{template}\" has no slot \"{slot}\"")]
    SlotNotFound { template: String, slot: String },

    #[error("template slot count mismatch: {names} names but {values} values")]
    SlotCountMismatch { names: usize, values: usize },

    #[error("duplicate slot \"{slot}\" in template \"{template}\"")]
    DuplicateSlot { template: String, slot: String },

    #[error("invalid value for slot \"{slot}\" in template \"{template}\": {reason}")]
    InvalidSlotValue {
        template: String,
        slot: String,
        reason: String,
    },
}

/// Errors that can occur when initializing an engine via
/// [`Engine::with_rules`] or [`Engine::with_rules_config`].
///
/// This preserves full error granularity from both the loading phase
/// (parsing, compilation) and the reset phase.
#[derive(Debug, Error)]
pub enum InitError {
    /// One or more errors occurred while parsing or loading source code.
    ///
    /// The vector may contain multiple errors (e.g., several parse errors
    /// collected from the same source).
    #[error("load errors: {}", format_load_errors(.0))]
    Load(Vec<crate::loader::LoadError>),

    /// An error occurred during the post-load `reset()` call.
    #[error("reset error: {0}")]
    Reset(EngineError),
}

impl From<Vec<crate::loader::LoadError>> for InitError {
    fn from(errors: Vec<crate::loader::LoadError>) -> Self {
        InitError::Load(errors)
    }
}

impl From<EngineError> for InitError {
    fn from(error: EngineError) -> Self {
        InitError::Reset(error)
    }
}

fn format_load_errors(errors: &[crate::loader::LoadError]) -> String {
    errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferric_rules_core::StringEncoding;

    #[test]
    fn new_engine_has_utf8_encoding_by_default() {
        let engine = Engine::new(EngineConfig::default());
        assert_eq!(engine.config.string_encoding, StringEncoding::Utf8);
    }

    #[test]
    fn assert_ordered_fact() {
        let mut engine = Engine::new(EngineConfig::utf8());
        let fields = vec![Value::Integer(42)];

        let id = engine.assert_ordered("person", fields).unwrap();

        let fact = engine.get_fact(id).unwrap().unwrap();
        if let Fact::Ordered(ordered) = fact {
            let relation_str = engine
                .symbol_table
                .resolve_symbol_str(ordered.relation)
                .unwrap();
            assert_eq!(relation_str, "person");
            assert_eq!(ordered.fields.len(), 1);
        } else {
            panic!("Expected ordered fact");
        }
    }

    #[test]
    fn fr_rete_001_default_rejects_duplicate_ordered_fact() {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine
            .load_str("(defrule r (x) => (printout t fired crlf))")
            .unwrap();
        engine.reset().unwrap();

        let first = engine.assert_ordered_with_result("x", ()).unwrap();
        let duplicate = engine.assert_ordered_with_result("x", ()).unwrap();

        assert!(matches!(first, FactAssertionResult::Asserted(_)));
        assert_eq!(duplicate, FactAssertionResult::Duplicate(first.fact_id()));
        assert_eq!(engine.facts().unwrap().count(), 1);
        assert_eq!(engine.agenda_len(), 1);

        let result = engine.run(RunLimit::Unlimited).unwrap();
        assert_eq!(result.rules_fired, 1);
        assert_eq!(engine.get_output("t").unwrap(), Some("fired\n"));
    }

    #[test]
    fn fr_rete_001_default_rejects_duplicate_template_fact() {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine
            .load_str(
                r"
                (deftemplate person (slot name) (slot age (default 0)))
                (defrule r (person (name Alice)) => (printout t fired crlf))
                ",
            )
            .unwrap();
        engine.reset().unwrap();
        let alice = engine.intern_symbol("Alice").unwrap();

        let first = engine
            .assert_template_with_result("person", &["name"], vec![HostValue::from(alice)])
            .unwrap();
        let duplicate = engine
            .assert_template_with_result(
                "person",
                &["age", "name"],
                vec![HostValue::from(0_i64), HostValue::from(alice)],
            )
            .unwrap();

        assert!(matches!(first, FactAssertionResult::Asserted(_)));
        assert_eq!(duplicate, FactAssertionResult::Duplicate(first.fact_id()));
        assert_eq!(engine.facts().unwrap().count(), 1);
        assert_eq!(engine.agenda_len(), 1);

        let result = engine.run(RunLimit::Unlimited).unwrap();
        assert_eq!(result.rules_fired, 1);
    }

    #[test]
    fn fr_rete_001_toggle_allows_duplicates() {
        let mut engine = Engine::new(EngineConfig::utf8());

        let first = engine.assert_ordered_with_result("x", 1_i64).unwrap();
        let rejected = engine.assert_ordered_with_result("x", 1_i64).unwrap();
        assert_eq!(rejected, FactAssertionResult::Duplicate(first.fact_id()));

        assert!(!engine.set_fact_duplication(true));
        let second = engine.assert_ordered_with_result("x", 1_i64).unwrap();
        assert!(matches!(second, FactAssertionResult::Asserted(_)));
        assert_ne!(first.fact_id(), second.fact_id());
        assert_eq!(
            engine
                .fact_base
                .get(engine.host.resolve(second.fact_id()).unwrap())
                .expect("second fact exists")
                .timestamp,
            ferric_rules_core::Timestamp::new(1),
            "rejected duplicates must not consume assertion timestamps"
        );

        assert!(engine.set_fact_duplication(false));
        let rejected_again = engine.assert_ordered_with_result("x", 1_i64).unwrap();
        assert_eq!(
            rejected_again,
            FactAssertionResult::Duplicate(first.fact_id())
        );

        engine.retract(first.fact_id()).unwrap();
        let rejected_after_retract = engine.assert_ordered_with_result("x", 1_i64).unwrap();
        assert_eq!(
            rejected_after_retract,
            FactAssertionResult::Duplicate(second.fact_id())
        );
        assert_eq!(engine.facts().unwrap().count(), 1);
    }

    #[test]
    fn fr_rete_001_getter_setter_return_semantics() {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine
            .load_str(
                r#"
                (defrule observe
                    (go)
                    =>
                    (printout t
                        (get-fact-duplication) " "
                        (set-fact-duplication TRUE) " "
                        (get-fact-duplication) " "
                        (set-fact-duplication FALSE) " "
                        (get-fact-duplication)
                        crlf))
                "#,
            )
            .unwrap();
        engine.reset().unwrap();
        engine.assert_ordered("go", ()).unwrap();

        let result = engine.run(RunLimit::Unlimited).unwrap();
        assert_eq!(result.rules_fired, 1);
        assert_eq!(
            engine.get_output("t").unwrap(),
            Some("FALSE FALSE TRUE TRUE FALSE\n")
        );
        assert!(!engine.fact_duplication());
    }

    #[test]
    fn fr_rete_001_reset_preserves_policy_and_duplicate_index() {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine.set_fact_duplication(true);
        engine
            .load_str(
                r"
                (deffacts duplicates
                    (item 1)
                    (item 1))
                ",
            )
            .unwrap();

        engine.reset().unwrap();
        assert!(engine.fact_duplication());
        assert_eq!(engine.find_facts("item").unwrap().len(), 2);

        engine.set_fact_duplication(false);
        let result = engine.assert_ordered_with_result("item", 1_i64).unwrap();
        assert!(matches!(result, FactAssertionResult::Duplicate(_)));
        assert_eq!(engine.find_facts("item").unwrap().len(), 2);
    }

    #[test]
    fn fr_rete_001_clear_preserves_policy() {
        let mut engine = Engine::new(EngineConfig::utf8());
        assert!(!engine.fact_duplication());

        engine.set_fact_duplication(true);
        engine.clear();
        assert!(engine.fact_duplication());

        engine.set_fact_duplication(false);
        engine.clear();
        assert!(!engine.fact_duplication());
    }

    fn assert_single_root_prefix_token(engine: &Engine) {
        let root = engine.rete.beta.root_id();
        let root_memory = engine
            .rete
            .beta
            .memory_id_for_node(root)
            .and_then(|memory_id| engine.rete.beta.get_memory(memory_id))
            .expect("root beta memory");
        assert_eq!(
            root_memory.len(),
            1,
            "root memory must contain exactly one empty-prefix token"
        );
        let root_token = engine
            .rete
            .token_store
            .get(root_memory.iter().next().expect("root token"))
            .expect("stored root token");
        assert_eq!(root_token.owner_node, root);
        assert!(root_token.fact.is_none());
        assert!(root_token.parent.is_none());
    }

    #[test]
    fn fr_rete_002_leading_not_activates_when_empty() {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine
            .load_str(
                r#"
                (defrule absent
                    (not (blocked))
                    =>
                    (printout t "unblocked" crlf))
                "#,
            )
            .unwrap();
        engine.reset().unwrap();

        assert_eq!(engine.agenda_len(), 1);
        assert_single_root_prefix_token(&engine);
        let result = engine.run(RunLimit::Unlimited).unwrap();
        assert_eq!(result.rules_fired, 1);
        assert_eq!(engine.get_output("t").unwrap(), Some("unblocked\n"));
    }

    #[test]
    fn fr_rete_002_leading_not_blocks_on_assert() {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine
            .load_str("(defrule absent (not (blocked)) => (assert (unexpected)))")
            .unwrap();
        engine.reset().unwrap();
        assert_eq!(engine.agenda_len(), 1);

        engine.assert_ordered("blocked", ()).unwrap();

        assert_eq!(engine.agenda_len(), 0);
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 0);
        assert!(engine.find_facts("unexpected").unwrap().is_empty());
        assert_single_root_prefix_token(&engine);
    }

    #[test]
    fn fr_rete_002_leading_not_reactivates_on_retract() {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine
            .load_str(
                r#"
                (defrule absent
                    (not (blocked))
                    =>
                    (printout t "unblocked" crlf))
                "#,
            )
            .unwrap();
        engine.reset().unwrap();

        let blocker = engine.assert_ordered("blocked", ()).unwrap();
        assert_eq!(engine.agenda_len(), 0);
        engine.retract(blocker).unwrap();

        assert_eq!(engine.agenda_len(), 1);
        let result = engine.run(RunLimit::Unlimited).unwrap();
        assert_eq!(result.rules_fired, 1);
        assert_eq!(engine.get_output("t").unwrap(), Some("unblocked\n"));
        assert_single_root_prefix_token(&engine);
    }

    #[test]
    fn fr_rete_002_leading_not_two_rule_isolation_and_reset() {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine
            .load_str(
                r#"
                (defrule absent-a
                    (not (blocked-a))
                    =>
                    (printout t "A" crlf))
                (defrule absent-b
                    (not (blocked-b))
                    =>
                    (printout t "B" crlf))
                "#,
            )
            .unwrap();

        for _ in 0..3 {
            engine.reset().unwrap();
            assert_eq!(engine.agenda_len(), 2);
            assert_single_root_prefix_token(&engine);
        }

        let blocker_a = engine.assert_ordered("blocked-a", ()).unwrap();
        assert_eq!(engine.agenda_len(), 1, "rule B remains independent");
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
        assert_eq!(engine.get_output("t").unwrap(), Some("B\n"));

        engine.retract(blocker_a).unwrap();
        assert_eq!(engine.agenda_len(), 1, "rule A reactivates alone");
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
        assert_eq!(engine.get_output("t").unwrap(), Some("B\nA\n"));
        assert_single_root_prefix_token(&engine);
    }

    fn activation_count_for_rule(engine: &Engine, rule_name: &str) -> usize {
        engine
            .rete
            .agenda
            .iter_activations()
            .filter(|activation| engine.rule_name(activation.rule) == Some(rule_name))
            .count()
    }

    #[test]
    fn fr_rete_003_late_single_pattern_rule_backfills() {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine.assert_ordered("foo", ()).unwrap();

        engine
            .load_str(
                r#"
                (defrule late
                    (foo)
                    =>
                    (printout t "late" crlf))
                "#,
            )
            .unwrap();

        assert_eq!(engine.agenda_len(), 1);
        assert_eq!(activation_count_for_rule(&engine, "late"), 1);
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
        assert_eq!(engine.get_output("t").unwrap(), Some("late\n"));
    }

    #[test]
    fn fr_rete_003_late_join_backfills() {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine.load_str("(defrule prefix (left ?x) =>)").unwrap();
        engine.load_str("(assert (left 1) (right 1 old))").unwrap();
        assert_eq!(activation_count_for_rule(&engine, "prefix"), 1);

        engine
            .load_str(
                r#"
                (defrule late-join
                    (left ?x)
                    (right ?x ?tag)
                    =>
                    (printout t "late " ?tag crlf))
                "#,
            )
            .unwrap();

        assert_eq!(
            activation_count_for_rule(&engine, "prefix"),
            1,
            "initialization must not replay into the old terminal"
        );
        assert_eq!(activation_count_for_rule(&engine, "late-join"), 1);
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 2);
        assert_eq!(engine.get_output("t").unwrap(), Some("late old\n"));

        engine.load_str("(assert (right 1 new))").unwrap();
        assert_eq!(
            activation_count_for_rule(&engine, "late-join"),
            1,
            "the populated parent memory's equality index must serve future facts"
        );
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
        assert_eq!(
            engine.get_output("t").unwrap(),
            Some("late old\nlate new\n")
        );
    }

    #[test]
    fn fr_rete_003_late_negative_and_exists_backfill() {
        let mut engine = Engine::new(EngineConfig::utf8());
        let blocker = engine.assert_ordered("blocked", ()).unwrap();
        engine.assert_ordered("seed", ()).unwrap();
        engine.assert_ordered("ready", ()).unwrap();

        engine
            .load_str(
                r#"
                (defrule late-negative
                    (seed)
                    (not (blocked))
                    =>
                    (printout t "negative" crlf))
                (defrule late-exists
                    (seed)
                    (exists (ready))
                    =>
                    (printout t "exists" crlf))
                "#,
            )
            .unwrap();

        assert_eq!(activation_count_for_rule(&engine, "late-negative"), 0);
        assert_eq!(activation_count_for_rule(&engine, "late-exists"), 1);
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
        assert_eq!(engine.get_output("t").unwrap(), Some("exists\n"));

        engine.retract(blocker).unwrap();
        assert_eq!(activation_count_for_rule(&engine, "late-negative"), 1);
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
        assert_eq!(engine.get_output("t").unwrap(), Some("exists\nnegative\n"));
    }

    #[test]
    fn fr_rete_003_backfill_does_not_duplicate_old_activations() {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine.load_str("(defrule old-rule (item ?x) =>)").unwrap();
        engine.load_str("(assert (item 1))").unwrap();
        assert_eq!(activation_count_for_rule(&engine, "old-rule"), 1);

        engine.load_str("(defrule new-rule (item ?x) =>)").unwrap();

        assert_eq!(activation_count_for_rule(&engine, "old-rule"), 1);
        assert_eq!(activation_count_for_rule(&engine, "new-rule"), 1);
        assert_eq!(engine.agenda_len(), 2);
    }

    #[test]
    fn fr_rete_003_rule_loaded_after_retractions_sees_current_state() {
        let mut engine = Engine::new(EngineConfig::utf8());
        let stale = engine.assert_ordered_symbol("item", "stale").unwrap();
        engine.assert_ordered_symbol("item", "current").unwrap();
        engine.retract(stale).unwrap();

        engine
            .load_str(
                r"
                (defrule late-current
                    (item ?value)
                    =>
                    (printout t ?value crlf))
                ",
            )
            .unwrap();

        assert_eq!(activation_count_for_rule(&engine, "late-current"), 1);
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
        assert_eq!(engine.get_output("t").unwrap(), Some("current\n"));
    }

    #[test]
    fn fr_rete_003_late_ncc_and_empty_prefix_rules_backfill() {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine
            .load_str("(assert (item 1) (block 1) (reason 1) (item 2))")
            .unwrap();

        engine
            .load_str(
                r#"
                (defrule late-ncc
                    (item ?x)
                    (not (and (block ?x) (reason ?x)))
                    =>
                    (printout t "item " ?x crlf))
                (defrule late-empty
                    =>
                    (printout t "empty" crlf))
                (defrule late-test-only
                    (test (> 2 1))
                    =>
                    (printout t "test" crlf))
                "#,
            )
            .unwrap();

        assert_eq!(activation_count_for_rule(&engine, "late-ncc"), 1);
        assert_eq!(activation_count_for_rule(&engine, "late-empty"), 1);
        assert_eq!(activation_count_for_rule(&engine, "late-test-only"), 1);
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 3);
        let output = engine.get_output("t").unwrap().unwrap_or_default();
        assert!(output.contains("item 2\n"));
        assert!(output.contains("empty\n"));
        assert!(output.contains("test\n"));
    }

    #[test]
    fn fr_rete_003_failed_installation_leaves_no_terminal_or_activation() {
        let mut engine = Engine::new(EngineConfig::ascii());
        engine.load_str("(assert (foo 1))").unwrap();
        let baseline_tokens = engine.rete.token_store.len();

        let result = engine.load_str(
            r#"
            (defrule broken
                (foo ?x)
                =>
                (printout t "é" crlf))
            "#,
        );
        assert!(result.is_err(), "non-ASCII RHS must fail in ASCII mode");
        assert!(engine.rules().is_empty());
        assert_eq!(engine.agenda_len(), 0);
        assert_eq!(engine.rete.token_store.len(), baseline_tokens);

        engine.load_str("(assert (foo 2))").unwrap();
        assert_eq!(
            engine.agenda_len(),
            0,
            "future facts must not reach a failed rule installation"
        );
        assert_eq!(engine.rete.token_store.len(), baseline_tokens);
    }

    fn rule_install_state(
        engine: &Engine,
    ) -> (ferric_rules_core::ReteCardinality, usize, usize, usize) {
        (
            engine.rete.cardinality(),
            engine.rule_info.len(),
            engine.rule_modules.len(),
            engine.symbol_table.len(),
        )
    }

    const LATE_OR_FAILURE: &str = r"
        (defrule broken-or
            (or
                (candidate ?x)
                (missing-template (value ?x)))
            =>
            (assert (accepted ?x)))
    ";

    #[test]
    fn fr_rete_007_bad_rhs_leaves_no_terminal() {
        let mut engine = Engine::new(EngineConfig::ascii());
        engine
            .assert_ordered("initial-fact", Vec::<Value>::new())
            .unwrap();
        let baseline = rule_install_state(&engine);

        let result = engine.load_str(
            r#"
            (defrule broken-rhs
                (trigger ?x)
                =>
                (printout t "é" ?x crlf))
            "#,
        );

        assert!(result.is_err(), "non-ASCII RHS must fail in ASCII mode");
        assert_eq!(engine.rules(), Vec::<(&str, i32)>::new());
        assert_eq!(
            rule_install_state(&engine),
            baseline,
            "a rejected RHS must not expose any rule installation state"
        );
        engine.debug_assert_consistency();
    }

    #[test]
    fn fr_rete_007_bad_rhs_cannot_create_activation_later() {
        let mut engine = Engine::new(EngineConfig::ascii());
        engine
            .assert_ordered("initial-fact", Vec::<Value>::new())
            .unwrap();
        let result = engine.load_str(
            r#"
            (defrule broken-rhs
                (trigger ?x)
                =>
                (printout t "é" ?x crlf))
            "#,
        );
        assert!(result.is_err());
        let baseline = engine.rete.cardinality();

        engine.assert_ordered("trigger", 1_i64).unwrap();

        let after_assert = engine.rete.cardinality();
        assert_eq!(after_assert.beta_nodes, baseline.beta_nodes);
        assert_eq!(after_assert.beta_memories, baseline.beta_memories);
        assert_eq!(after_assert.tokens, baseline.tokens);
        assert_eq!(after_assert.activations, 0);
        let run = engine.run(RunLimit::Unlimited).unwrap();
        assert_eq!(run.rules_fired, 0);
        assert_eq!(run.halt_reason, HaltReason::AgendaEmpty);
        engine.debug_assert_consistency();
    }

    #[test]
    fn fr_rete_007_failed_compile_preserves_existing_rules() {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine
            .load_str(
                r"
                (defrule existing
                    (good ?x)
                    =>
                    (assert (preserved ?x)))
                ",
            )
            .unwrap();
        let baseline = rule_install_state(&engine);
        let existing_rules: Vec<(String, i32)> = engine
            .rules()
            .into_iter()
            .map(|(name, salience)| (name.to_string(), salience))
            .collect();

        let result = engine.load_str(LATE_OR_FAILURE);

        assert!(
            result.is_err(),
            "the second expanded branch must reject its unknown template"
        );
        assert_eq!(
            engine.rules(),
            existing_rules
                .iter()
                .map(|(name, salience)| (name.as_str(), *salience))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            rule_install_state(&engine),
            baseline,
            "failure in a later expansion must roll back the whole source rule"
        );

        engine.assert_ordered("good", 7_i64).unwrap();
        let run = engine.run(RunLimit::Unlimited).unwrap();
        assert_eq!(run.rules_fired, 1);
        assert!(engine
            .facts()
            .unwrap()
            .any(|(_, fact)| matches!(fact, Fact::Ordered(ordered) if engine
                .resolve_core_symbol(ordered.relation) == Some("preserved"))));
        engine.debug_assert_consistency();
    }

    #[test]
    fn fr_rete_007_repeated_failure_has_constant_network_size() {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine
            .assert_ordered("initial-fact", Vec::<Value>::new())
            .unwrap();
        let baseline = rule_install_state(&engine);

        for attempt in 0..32 {
            assert!(
                engine.load_str(LATE_OR_FAILURE).is_err(),
                "attempt {attempt} must fail"
            );
            assert_eq!(
                rule_install_state(&engine),
                baseline,
                "attempt {attempt} leaked rule installation state"
            );
        }

        engine.assert_ordered("candidate", 42_i64).unwrap();
        assert_eq!(engine.agenda_len(), 0);
        engine.debug_assert_consistency();
    }

    #[test]
    fn fr_rete_007_failures_before_each_commit_phase_are_non_mutating() {
        let mut engine = Engine::new(EngineConfig::ascii());
        engine
            .assert_ordered("initial-fact", Vec::<Value>::new())
            .unwrap();
        let baseline = rule_install_state(&engine);
        let failures = [
            // Action callable validation.
            "(defrule bad-callable (x) => (missing-function))",
            // LHS translation/template resolution.
            "(defrule bad-lhs (missing-template (value ?x)) => (assert (x ?x)))",
            // RHS translation/encoding.
            "(defrule bad-rhs (x) => (printout t \"é\" crlf))",
            // A late expanded variant after an earlier variant plans cleanly.
            LATE_OR_FAILURE,
        ];

        for source in failures {
            assert!(engine.load_str(source).is_err(), "{source} must fail");
            assert_eq!(
                rule_install_state(&engine),
                baseline,
                "failed phase leaked installation state for {source}"
            );
        }
        engine.debug_assert_consistency();
    }

    #[test]
    fn fr_rete_007_missing_activation_metadata_is_action_error() {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine
            .load_str(
                r"
                (defmodule OTHER)
                (defrule doomed (trigger) => (assert (should-not-run)))
                ",
            )
            .unwrap();
        engine
            .assert_ordered("trigger", Vec::<Value>::new())
            .unwrap();
        let rule_id = engine
            .rete
            .agenda
            .iter_activations()
            .next()
            .expect("activation")
            .rule;
        engine.rule_info[rule_id.0 as usize] = None;
        assert_ne!(
            engine.rule_modules[rule_id.0 as usize],
            Some(engine.module_registry.current_focus().unwrap()),
            "test requires an invalid activation outside the active focus"
        );

        let run = engine.run(RunLimit::Unlimited).unwrap();

        assert_eq!(run.rules_fired, 0);
        assert_eq!(run.halt_reason, HaltReason::ActionError);
        assert_eq!(
            engine.agenda_len(),
            0,
            "invalid activation must be consumed"
        );
        assert!(engine.action_diagnostics().iter().any(
            |error| matches!(error, ActionError::EvalError(message)
                if message.contains("internal invariant violation")
                    && message.contains("no executable metadata"))
        ));
    }

    #[test]
    fn retract_fact() {
        let mut engine = Engine::new(EngineConfig::utf8());
        let id = engine.assert_ordered("test", ()).unwrap();

        let result = engine.retract(id);
        assert!(result.is_ok());

        assert!(engine.get_fact(id).unwrap().is_none());
    }

    #[test]
    fn retract_nonexistent_fact_returns_error() {
        let mut engine = Engine::new(EngineConfig::utf8());
        let id = engine.assert_ordered("test", ()).unwrap();

        engine.retract(id).unwrap();
        let result = engine.retract(id);

        assert!(matches!(result, Err(EngineError::FactNotFound(_))));
    }

    #[test]
    fn assert_structured_ordered_fact() {
        let mut engine = Engine::new(EngineConfig::utf8());
        let original = engine.assert_ordered("person", 42_i64).unwrap();
        let fact = engine.get_fact_owned(original).unwrap().unwrap();
        engine.retract(original).unwrap();
        let id = engine.assert(fact).unwrap();
        let stored = engine.get_fact(id).unwrap().unwrap();

        match stored {
            Fact::Ordered(ordered) => assert_eq!(ordered.fields.len(), 1),
            Fact::Template(_) => panic!("expected ordered fact"),
        }
    }

    #[test]
    fn intern_symbol_is_idempotent() {
        let mut engine = Engine::new(EngineConfig::utf8());

        let sym1 = engine.intern_symbol("test").unwrap();
        let sym2 = engine.intern_symbol("test").unwrap();

        assert_eq!(sym1, sym2);
    }

    #[test]
    fn intern_symbol_respects_encoding() {
        let mut engine = Engine::new(EngineConfig::ascii());

        let result = engine.intern_symbol("héllo");
        assert!(matches!(result, Err(EngineError::Encoding(_))));
    }

    #[test]
    fn create_string() {
        let engine = Engine::new(EngineConfig::utf8());
        let s = engine.create_string("hello world").unwrap();
        assert_eq!(s.as_str().unwrap(), "hello world");
    }

    #[test]
    fn create_string_respects_encoding() {
        let engine = Engine::new(EngineConfig::ascii());
        let result = engine.create_string("héllo");
        assert!(matches!(result, Err(EngineError::Encoding(_))));
    }

    #[test]
    fn iterate_facts() {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine.set_fact_duplication(true);

        let id1 = engine.assert_ordered("test", ()).unwrap();
        let id2 = engine.assert_ordered("test", ()).unwrap();

        let all: Vec<_> = engine.facts().unwrap().map(|(id, _)| id).collect();
        assert_eq!(all.len(), 2);
        assert!(all.contains(&id1));
        assert!(all.contains(&id2));
    }

    // --- Execution loop tests ---

    #[test]
    fn step_on_empty_agenda_returns_none() {
        let mut engine = Engine::new(EngineConfig::utf8());
        let result = engine.step().unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn step_fires_one_activation() {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine.load_str("(defrule test (person ?x) =>)").unwrap();
        engine.load_str("(assert (person Alice))").unwrap();
        assert_eq!(engine.rete.agenda.len(), 1);

        let result = engine.step().unwrap();
        assert!(result.is_some());
        assert_eq!(engine.rete.agenda.len(), 0);
    }

    #[test]
    fn step_returns_fired_rule_info() {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine.load_str("(defrule test (person ?x) =>)").unwrap();
        engine.load_str("(assert (person Alice))").unwrap();

        let fired = engine.step().unwrap().unwrap();
        assert_eq!(fired.rule_id, ferric_rules_core::beta::RuleId(1));
    }

    #[test]
    fn run_fires_all_activations() {
        use crate::execution::RunLimit;
        let mut engine = Engine::new(EngineConfig::utf8());
        engine.load_str("(defrule test (person ?x) =>)").unwrap();
        engine.load_str("(assert (person Alice))").unwrap();
        engine.load_str("(assert (person Bob))").unwrap();
        assert_eq!(engine.rete.agenda.len(), 2);

        let result = engine.run(RunLimit::Unlimited).unwrap();
        assert_eq!(result.rules_fired, 2);
        assert_eq!(
            result.halt_reason,
            crate::execution::HaltReason::AgendaEmpty
        );
        assert!(engine.rete.agenda.is_empty());
    }

    #[test]
    fn run_with_limit() {
        use crate::execution::RunLimit;
        let mut engine = Engine::new(EngineConfig::utf8());
        engine.load_str("(defrule test (person ?x) =>)").unwrap();
        engine.load_str("(assert (person Alice))").unwrap();
        engine.load_str("(assert (person Bob))").unwrap();
        engine.load_str("(assert (person Charlie))").unwrap();
        assert_eq!(engine.rete.agenda.len(), 3);

        let result = engine.run(RunLimit::Count(2)).unwrap();
        assert_eq!(result.rules_fired, 2);
        assert_eq!(
            result.halt_reason,
            crate::execution::HaltReason::LimitReached
        );
        assert_eq!(engine.rete.agenda.len(), 1);
    }

    #[test]
    fn run_on_empty_agenda() {
        use crate::execution::RunLimit;
        let mut engine = Engine::new(EngineConfig::utf8());
        let result = engine.run(RunLimit::Unlimited).unwrap();
        assert_eq!(result.rules_fired, 0);
        assert_eq!(
            result.halt_reason,
            crate::execution::HaltReason::AgendaEmpty
        );
    }

    #[test]
    fn run_with_zero_limit() {
        use crate::execution::RunLimit;
        let mut engine = Engine::new(EngineConfig::utf8());
        engine.load_str("(defrule test (person ?x) =>)").unwrap();
        engine.load_str("(assert (person Alice))").unwrap();

        let result = engine.run(RunLimit::Count(0)).unwrap();
        assert_eq!(result.rules_fired, 0);
        assert_eq!(
            result.halt_reason,
            crate::execution::HaltReason::LimitReached
        );
        assert_eq!(engine.rete.agenda.len(), 1);
    }

    #[test]
    fn halt_stops_execution() {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine.halt();
        assert!(engine.is_halted());

        engine.load_str("(defrule test (person ?x) =>)").unwrap();
        engine.load_str("(assert (person Alice))").unwrap();

        // run() clears halt before starting
        let result = engine.run(crate::execution::RunLimit::Unlimited).unwrap();
        assert_eq!(result.rules_fired, 1);
    }

    #[test]
    fn reset_clears_facts_and_agenda() {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine.load_str("(defrule test (person ?x) =>)").unwrap();
        engine.load_str("(assert (person Alice))").unwrap();

        assert_eq!(engine.rete.agenda.len(), 1);
        assert!(engine.facts().unwrap().count() > 0);

        engine.reset().unwrap();

        assert!(engine.rete.agenda.is_empty());
        assert_eq!(engine.facts().unwrap().count(), 0);
    }

    #[test]
    fn reset_preserves_compiled_rules() {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine.load_str("(defrule test (person ?x) =>)").unwrap();
        engine.reset().unwrap();

        // Rules still compiled — asserting a matching fact produces activation
        engine.load_str("(assert (person Alice))").unwrap();
        assert_eq!(engine.rete.agenda.len(), 1);
    }

    #[test]
    fn reset_reasserts_deffacts() {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine.load_str("(defrule test (person ?x) =>)").unwrap();
        engine
            .load_str("(deffacts startup (person Alice) (person Bob))")
            .unwrap();

        engine.reset().unwrap();
        // Reset creates 2 activations from deffacts.
        assert_eq!(engine.rete.agenda.len(), 2);

        // Run to clear agenda
        engine.run(crate::execution::RunLimit::Unlimited).unwrap();
        assert!(engine.rete.agenda.is_empty());

        // Reset should re-assert deffacts
        engine.reset().unwrap();
        assert_eq!(engine.facts().unwrap().count(), 2);
        assert_eq!(engine.rete.agenda.len(), 2);
    }

    #[test]
    fn reset_clears_halt_flag() {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine.halt();
        assert!(engine.is_halted());

        engine.reset().unwrap();
        assert!(!engine.is_halted());
    }

    #[test]
    fn step_is_equivalent_to_run_count_1() {
        use crate::execution::RunLimit;
        let mut engine1 = Engine::new(EngineConfig::utf8());
        engine1.load_str("(defrule test (person ?x) =>)").unwrap();
        engine1.load_str("(assert (person Alice))").unwrap();
        engine1.load_str("(assert (person Bob))").unwrap();

        let mut engine2 = Engine::new(EngineConfig::utf8());
        engine2.load_str("(defrule test (person ?x) =>)").unwrap();
        engine2.load_str("(assert (person Alice))").unwrap();
        engine2.load_str("(assert (person Bob))").unwrap();

        let step_result = engine1.step().unwrap();
        let run_result = engine2.run(RunLimit::Count(1)).unwrap();

        assert!(step_result.is_some());
        assert_eq!(run_result.rules_fired, 1);
        assert_eq!(engine1.rete.agenda.len(), engine2.rete.agenda.len());
    }

    #[test]
    fn step_respects_focus_filter_like_run_count_1() {
        use crate::execution::RunLimit;

        let program = r"
            (defrule main-high
                (declare (salience 10))
                (go)
                =>
                (assert (main-fired)))

            (defmodule SENSOR (export ?ALL))
            (defrule sensor-low
                (go)
                =>
                (assert (sensor-fired)))
        ";

        let mut step_engine = Engine::new(EngineConfig::utf8());
        step_engine.load_str(program).unwrap();
        step_engine.load_str("(assert (go))").unwrap();
        step_engine.push_focus("SENSOR").unwrap();

        let mut run_engine = Engine::new(EngineConfig::utf8());
        run_engine.load_str(program).unwrap();
        run_engine.load_str("(assert (go))").unwrap();
        run_engine.push_focus("SENSOR").unwrap();

        let step_result = step_engine.step().unwrap();
        let run_result = run_engine.run(RunLimit::Count(1)).unwrap();

        let has_relation = |engine: &Engine, relation: &str| {
            engine.facts().unwrap().any(|(_, fact)| match fact {
                Fact::Ordered(ordered) => engine
                    .symbol_table
                    .resolve_symbol_str(ordered.relation)
                    .is_some_and(|name| name == relation),
                Fact::Template(_) => false,
            })
        };

        assert!(step_result.is_some());
        assert_eq!(run_result.rules_fired, 1);
        assert!(has_relation(&step_engine, "sensor-fired"));
        assert!(!has_relation(&step_engine, "main-fired"));
        assert!(has_relation(&run_engine, "sensor-fired"));
        assert!(!has_relation(&run_engine, "main-fired"));
    }

    #[test]
    fn multiple_resets_reassert_deffacts_each_time() {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine.load_str("(defrule test (person ?x) =>)").unwrap();
        engine
            .load_str("(deffacts startup (person Alice))")
            .unwrap();

        for _ in 0..3 {
            engine.reset().unwrap();
            assert_eq!(engine.facts().unwrap().count(), 1);
            assert_eq!(engine.rete.agenda.len(), 1);

            let result = engine.run(crate::execution::RunLimit::Unlimited).unwrap();
            assert_eq!(result.rules_fired, 1);
        }
    }

    #[test]
    fn assert_propagates_through_rete() {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine.load_str("(defrule test (person ?x) =>)").unwrap();

        // assert_ordered should automatically propagate through rete
        let alice_sym = engine.intern_symbol("Alice").unwrap();
        engine
            .assert_ordered("person", vec![HostValue::from(alice_sym)])
            .unwrap();
        assert_eq!(engine.rete.agenda.len(), 1);
    }

    #[test]
    fn retract_removes_from_rete() {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine.load_str("(defrule test (person ?x) =>)").unwrap();

        let alice_sym = engine.intern_symbol("Alice").unwrap();
        let fid = engine
            .assert_ordered("person", vec![HostValue::from(alice_sym)])
            .unwrap();
        assert_eq!(engine.rete.agenda.len(), 1);

        engine.retract(fid).unwrap();
        assert!(engine.rete.agenda.is_empty());
    }

    // -----------------------------------------------------------------------
    // Property-based tests
    // -----------------------------------------------------------------------

    use proptest::prelude::*;

    /// Operations exercised in the fact-lifecycle shadow-model test.
    #[derive(Debug, Clone)]
    enum FactOp {
        /// Assert an ordered fact using a relation from the pre-interned pool.
        AssertOrdered(usize),
        /// Retract a fact selected from the live set by index.
        Retract(usize),
        /// Query a fact selected from the ever-asserted set by index.
        GetFact(usize),
    }

    fn arb_fact_op() -> impl Strategy<Value = FactOp> {
        prop_oneof![
            (0usize..3).prop_map(FactOp::AssertOrdered),
            // Indices into live/all-asserted lists — clamped at use time.
            any::<usize>().prop_map(FactOp::Retract),
            any::<usize>().prop_map(FactOp::GetFact),
        ]
    }

    proptest! {
        /// Shadow-model verification for ordered fact assertion and retraction.
        ///
        /// Invariants verified after each operation:
        /// - `get_fact(id)` returns `Some` iff the fact is in the live set.
        /// - `get_fact(id)` returns `None` for retracted (but previously asserted) facts.
        /// - Engine structural consistency holds throughout the sequence.
        #[test]
        fn fact_lifecycle_shadow_model(ops in proptest::collection::vec(arb_fact_op(), 0..40)) {
            let mut engine = Engine::new(EngineConfig::default());
            engine.set_fact_duplication(true);

            // Pre-intern a small pool of relation symbols so we can refer to
            // them by index in the operation stream.
            let relation_pool: Vec<&str> = vec!["rel0", "rel1", "rel2"];

            // Shadow model: track which FactIds are currently live.
            let mut live: Vec<FactHandle> = Vec::new();
            // All FactIds ever successfully asserted (live or retracted).
            let mut all_asserted: Vec<FactHandle> = Vec::new();

            for op in &ops {
                match op {
                    FactOp::AssertOrdered(name_idx) => {
                        let relation = relation_pool[name_idx % relation_pool.len()];
                        let fid = engine.assert_ordered(relation, ()).unwrap();
                        // Postcondition: newly asserted fact must be immediately retrievable.
                        let retrieved = engine.get_fact(fid).unwrap();
                        prop_assert!(
                            retrieved.is_some(),
                            "newly asserted fact must be retrievable via get_fact"
                        );
                        live.push(fid);
                        all_asserted.push(fid);
                    }
                    FactOp::Retract(idx) => {
                        if live.is_empty() {
                            // No live facts — skip retract.
                            continue;
                        }
                        let pick = idx % live.len();
                        let fid = live.remove(pick);
                        engine.retract(fid).unwrap();
                        // Postcondition: retracted fact must no longer be retrievable.
                        let after = engine.get_fact(fid).unwrap();
                        prop_assert!(
                            after.is_none(),
                            "retracted fact must not be retrievable via get_fact"
                        );
                    }
                    FactOp::GetFact(idx) => {
                        if all_asserted.is_empty() {
                            continue;
                        }
                        let pick = idx % all_asserted.len();
                        let fid = all_asserted[pick];
                        let result = engine.get_fact(fid).unwrap();
                        let is_live = live.contains(&fid);
                        // Invariant: presence in get_fact matches live-set membership.
                        prop_assert_eq!(
                            result.is_some(),
                            is_live,
                            "get_fact liveness mismatch for fact {:?}: shadow says live={}, engine returned {}",
                            fid,
                            is_live,
                            if result.is_some() { "Some" } else { "None" }
                        );
                    }
                }

                // Structural consistency must hold after every operation.
                engine.debug_assert_consistency();
            }

            // Final cross-check: every ID in `live` must be retrievable.
            for &fid in &live {
                prop_assert!(
                    engine.get_fact(fid).unwrap().is_some(),
                    "live fact {:?} must still be retrievable at end of sequence",
                    fid
                );
            }
            // Every retracted ID (in all_asserted but not live) must be absent.
            for &fid in &all_asserted {
                if !live.contains(&fid) {
                    prop_assert!(
                        engine.get_fact(fid).unwrap().is_none(),
                        "retracted fact {:?} must not be retrievable at end of sequence",
                        fid
                    );
                }
            }
        }

        /// Property: assert N facts then retract them all in arbitrary order;
        /// the user-visible fact count must return to zero.
        ///
        /// Invariants:
        /// - After retracting every asserted fact, `facts().count() == 0`.
        /// - `agenda_len() == 0` (no pending activations).
        /// - Structural consistency holds after the final retraction.
        #[test]
        fn assert_retract_idempotent_cleanup(
            n in 0usize..20,
            shuffle in proptest::collection::vec(any::<usize>(), 0..20),
        ) {
            let mut engine = Engine::new(EngineConfig::default());
            engine.set_fact_duplication(true);

            // Assert N ordered facts and collect their IDs.
            let mut live: Vec<FactHandle> = (0..n)
                .map(|i| {
                    let relation = if i % 2 == 0 { "even" } else { "odd" };
                    engine.assert_ordered(relation, ()).unwrap()
                })
                .collect();

            // Retract them in the order prescribed by the `shuffle` indices
            // (clamped to the shrinking live list).
            let mut shuffle_iter = shuffle.into_iter();
            while !live.is_empty() {
                let pick = shuffle_iter.next().unwrap_or(0) % live.len();
                let fid = live.remove(pick);
                engine.retract(fid).unwrap();
            }

            // Invariant: no user-visible facts remain.
            let remaining = engine.facts().unwrap().count();
            prop_assert_eq!(
                remaining,
                0,
                "all facts retracted but {} user-visible facts remain",
                remaining
            );
            // Invariant: agenda is empty (no rules loaded, so trivially satisfied,
            // but the check still exercises agenda_len consistency).
            prop_assert_eq!(
                engine.agenda_len(),
                0,
                "agenda must be empty after retracting all facts with no rules loaded"
            );

            engine.debug_assert_consistency();
        }

        /// Property: sequences of push_focus / set_focus keep get_focus consistent.
        ///
        /// Invariants:
        /// - After set_focus(name), get_focus() == Some(name).
        /// - After push_focus(name), get_focus() == Some(name) (it's on top).
        /// - Structural consistency holds after every focus operation.
        #[test]
        fn focus_stack_operations(
            ops in proptest::collection::vec(
                prop_oneof![
                    // 0 = push MAIN, 1 = push SENSOR, 2 = push DATA
                    (0usize..3usize).prop_map(|i| (false, i)),
                    // set_focus
                    (0usize..3usize).prop_map(|i| (true, i)),
                ],
                1..30,
            )
        ) {
            let mut engine = Engine::new(EngineConfig::default());

            // Register two extra modules so we have a pool of three
            // (MAIN is always present).
            engine.load_str("(defmodule SENSOR)").unwrap();
            engine.load_str("(defmodule DATA)").unwrap();

            let module_names = ["MAIN", "SENSOR", "DATA"];

            for (is_set, idx) in &ops {
                let name = module_names[idx % module_names.len()];
                if *is_set {
                    engine.set_focus(name).unwrap();
                    // Postcondition: set_focus makes `name` the unique focus.
                    prop_assert_eq!(
                        engine.get_focus(),
                        Some(name),
                        "after set_focus({}) get_focus must return Some({})",
                        name, name
                    );
                    // After set_focus, the stack has exactly one element.
                    prop_assert_eq!(
                        engine.get_focus_stack().len(),
                        1,
                        "set_focus must leave stack with exactly 1 element"
                    );
                } else {
                    engine.push_focus(name).unwrap();
                    // Postcondition: push_focus makes `name` the new top.
                    prop_assert_eq!(
                        engine.get_focus(),
                        Some(name),
                        "after push_focus({}) get_focus must return Some({})",
                        name, name
                    );
                    // Stack must be non-empty.
                    prop_assert!(
                        !engine.get_focus_stack().is_empty(),
                        "focus stack must be non-empty after push_focus"
                    );
                }

                engine.debug_assert_consistency();
            }
        }

        /// Property: halt/reset sequences keep the halted flag and consistency invariant.
        ///
        /// Invariants:
        /// - `halt()` always sets `is_halted()` to true.
        /// - `reset()` always clears `is_halted()` to false.
        /// - After reset, agenda is empty and fact count is 0 (no deffacts loaded).
        /// - Structural consistency holds after every state transition.
        #[test]
        fn halt_reset_state_machine(
            ops in proptest::collection::vec(
                prop_oneof![
                    Just(0u8), // halt
                    Just(1u8), // reset
                    Just(2u8), // step (no-op on empty agenda)
                ],
                1..40,
            )
        ) {
            let mut engine = Engine::new(EngineConfig::default());
            let mut expected_halted = false;

            for op in &ops {
                match op {
                    0 => {
                        // halt: sets the flag unconditionally.
                        engine.halt();
                        expected_halted = true;
                        prop_assert!(
                            engine.is_halted(),
                            "after halt(), is_halted() must be true"
                        );
                    }
                    1 => {
                        // reset: clears the flag and runtime state.
                        engine.reset().unwrap();
                        expected_halted = false;
                        prop_assert!(
                            !engine.is_halted(),
                            "after reset(), is_halted() must be false"
                        );
                        // Invariant: reset leaves an empty agenda and no user facts.
                        prop_assert_eq!(
                            engine.agenda_len(),
                            0,
                            "agenda must be empty after reset with no rules/deffacts"
                        );
                        prop_assert_eq!(
                            engine.facts().unwrap().count(),
                            0,
                            "fact count must be 0 after reset with no deffacts"
                        );
                    }
                    _ => {
                        // step on empty agenda returns None.
                        let result = engine.step().unwrap();
                        prop_assert!(
                            result.is_none(),
                            "step on empty agenda must return None"
                        );
                        // step() does not affect the halted flag.
                        prop_assert_eq!(
                            engine.is_halted(),
                            expected_halted,
                            "step must not alter the halted flag"
                        );
                    }
                }

                engine.debug_assert_consistency();
            }
        }

        /// Property: clear() removes all facts and empties the input buffer.
        ///
        /// Invariants:
        /// - After clear(), `facts().count() == 0`.
        /// - After clear(), `agenda_len() == 0`.
        /// - After clear(), `is_halted() == false`.
        /// - After clear(), input_buffer is empty (clear resets I/O state).
        /// - Structural consistency holds after clear.
        #[test]
        fn clear_resets_all_state(
            n_facts in 0usize..15,
            n_inputs in 0usize..10,
        ) {
            let mut engine = Engine::new(EngineConfig::default());

            // Assert some facts.
            for i in 0..n_facts {
                let relation = if i % 2 == 0 { "alpha" } else { "beta" };
                engine.assert_ordered(relation, ()).unwrap();
            }
            // Push some input lines.
            for i in 0..n_inputs {
                engine.push_input(&format!("line{i}"));
            }
            // Set halt flag.
            engine.halt();

            engine.clear();

            // Invariants after clear:
            prop_assert_eq!(
                engine.facts().unwrap().count(),
                0,
                "clear must remove all facts"
            );
            prop_assert_eq!(
                engine.agenda_len(),
                0,
                "clear must empty the agenda"
            );
            prop_assert!(
                !engine.is_halted(),
                "clear must reset the halted flag"
            );
            // Input buffer must be empty — push a sentinel then verify buffer
            // holds exactly one item (the sentinel), not any stale lines.
            engine.push_input("sentinel");
            prop_assert_eq!(
                engine.input_buffer.len(),
                1,
                "after clear, input buffer must contain only the sentinel (1 item)"
            );

            engine.debug_assert_consistency();
        }
    }
}

#[cfg(test)]
mod initial_fact_contract_tests {
    use super::*;

    #[test]
    fn initial_fact_host_visibility_and_retraction_are_consistent() {
        let mut engine =
            Engine::with_rules("(defrule explicit (initial-fact) =>) (defrule empty =>)").unwrap();
        let id = engine.host.export(engine.initial_fact_id.unwrap());
        assert!(engine.get_fact(id).unwrap().is_none());
        assert!(engine.find_facts("initial-fact").unwrap().is_empty());
        assert_eq!(engine.facts().unwrap().count(), 0);
        assert!(matches!(
            engine.get_fact_slot_by_name(id, "x"),
            Err(EngineError::FactNotFound(_))
        ));
        assert!(matches!(
            engine.retract(id),
            Err(EngineError::ProtectedInitialFact)
        ));
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 2);
        engine.reset().unwrap();
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 2);
        engine.debug_assert_consistency();
        engine.clear();
        engine.debug_assert_consistency();
        engine.load_str("(defrule empty =>)").unwrap();
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
    }

    #[test]
    fn initial_fact_rhs_mutation_is_rejected_without_corrupting_root_state() {
        for action in [
            "(retract ?f)",
            "(modify ?f (slot broken))",
            "(duplicate ?f)",
        ] {
            let mut engine = Engine::with_rules(&format!(
                "(defrule mutate ?f <- (initial-fact) => {action})"
            ))
            .unwrap();
            assert_eq!(
                engine.run(RunLimit::Unlimited).unwrap().halt_reason,
                HaltReason::ActionError
            );
            assert!(engine
                .action_diagnostics()
                .iter()
                .any(|error| error.to_string().contains("protected")));
            engine.debug_assert_consistency();
            engine.load_str("(defrule empty =>)").unwrap();
            assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn restored_initial_fact_retains_protection_and_root_order() {
        let engine =
            Engine::with_rules("(defrule explicit (initial-fact) =>) (deffacts seed (item 1))")
                .unwrap();
        for &format in crate::serialization::SerializationFormat::ALL {
            let mut restored =
                Engine::deserialize(&engine.serialize(format).unwrap(), format).unwrap();
            let id = restored.host.export(restored.initial_fact_id.unwrap());
            assert!(restored.get_fact(id).unwrap().is_none());
            assert!(matches!(
                restored.retract(id),
                Err(EngineError::ProtectedInitialFact)
            ));
            restored.reset().unwrap();
            assert_eq!(restored.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
            restored.debug_assert_consistency();
        }
    }
}
