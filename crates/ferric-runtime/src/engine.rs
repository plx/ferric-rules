//! The Ferric rules engine.
//!
//! This module provides the main `Engine` type, which is the primary interface
//! for embedding applications. Phase 1 includes basic fact assertion/retraction
//! and thread affinity checking.

use std::collections::{HashMap, HashSet};
use std::marker::PhantomData;
use std::thread::ThreadId;
use thiserror::Error;

use ferric_core::beta::RuleId;
use ferric_core::{
    EncodingError, Fact, FactBase, FactId, FerricString, ReteCompiler, ReteNetwork, Symbol,
    SymbolTable, TemplateId, Value,
};

use crate::actions::{self, ActionError, CompiledRuleInfo};
use crate::config::EngineConfig;
use crate::execution::{FiredRule, HaltReason, RunLimit, RunResult};
use crate::functions::{FunctionEnv, GenericRegistry, GlobalStore};
use crate::modules::{ModuleId, ModuleRegistry};
use crate::router::OutputRouter;
use crate::templates::RegisteredTemplate;

fn propagate_fact_assertion(rete: &mut ReteNetwork, fact_base: &FactBase, fact_id: FactId) {
    let fact = fact_base
        .get(fact_id)
        .expect("asserted fact should exist in fact base")
        .fact
        .clone();
    rete.assert_fact(fact_id, &fact, fact_base);
}

/// The Ferric rules engine.
///
/// This is the main entry point for embedding applications. The engine is
/// not `Send` or `Sync` — it must remain on the thread that created it.
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
/// - Thread affinity enforcement with `unsafe move_to_current_thread`
///
/// ## Phase 3 planned
///
/// - Template-aware `modify`/`duplicate`
/// - Real `printout` with output routing
/// - Expression evaluator for nested function calls in RHS
/// - `deffunction`/`defglobal` support
/// - `defmodule` import/export
/// - `defgeneric`/`defmethod` dispatch
/// - `forall` CE
pub struct Engine {
    pub(crate) fact_base: FactBase,
    pub(crate) symbol_table: SymbolTable,
    pub(crate) config: EngineConfig,
    pub(crate) rete: ReteNetwork,
    pub(crate) compiler: ReteCompiler,
    /// Registered deffacts for re-assertion on reset.
    pub(crate) registered_deffacts: Vec<Vec<Fact>>,
    /// Compiled rule info for action execution.
    pub(crate) rule_info: HashMap<RuleId, CompiledRuleInfo>,
    /// Registered template definitions: name → `TemplateId`.
    pub(crate) template_ids: HashMap<String, TemplateId>,
    /// Template slot metadata indexed by `TemplateId`.
    pub(crate) template_defs: HashMap<TemplateId, RegisteredTemplate>,
    /// Allocator for `TemplateId` keys.
    pub(crate) template_id_alloc: slotmap::SlotMap<TemplateId, ()>,
    /// Output router for capturing `printout` and related I/O.
    pub(crate) router: OutputRouter,
    /// Registry of user-defined functions loaded via `deffunction`.
    pub(crate) functions: FunctionEnv,
    /// Runtime storage for `defglobal` variables.
    pub(crate) globals: GlobalStore,
    /// Snapshot of global initial values for re-initialization on reset.
    pub(crate) registered_globals: Vec<(String, Value)>,
    /// Registry of generic functions and methods loaded via `defgeneric`/`defmethod`.
    pub(crate) generics: GenericRegistry,
    /// Module registry: module definitions, focus stack, visibility.
    pub(crate) module_registry: ModuleRegistry,
    /// Rule-to-module association for focus-aware execution.
    pub(crate) rule_modules: HashMap<RuleId, ModuleId>,
    /// Template-to-module association for visibility checking.
    pub(crate) template_modules: HashMap<ferric_core::TemplateId, ModuleId>,
    /// The `FactId` of the synthetic `(initial-fact)` in working memory, if present.
    ///
    /// `(initial-fact)` is asserted by the engine to provide a root token for
    /// top-level NCC/negation/forall CEs (mirroring CLIPS' built-in mechanism).
    /// It is tracked here so that `facts()` can exclude it from user-visible results.
    pub(crate) initial_fact_id: Option<FactId>,
    /// Non-fatal action diagnostics captured during execution.
    action_diagnostics: Vec<ActionError>,
    /// Whether a halt has been requested.
    halted: bool,
    creator_thread: ThreadId,
    // Marker to ensure Engine is !Send + !Sync
    _not_send_sync: PhantomData<*mut ()>,
}

impl Engine {
    /// Create a new engine with the given configuration.
    #[must_use]
    pub fn new(config: EngineConfig) -> Self {
        let strategy = config.strategy;
        Self {
            fact_base: FactBase::new(),
            symbol_table: SymbolTable::new(),
            config,
            rete: ReteNetwork::with_strategy(strategy),
            compiler: ReteCompiler::new(),
            registered_deffacts: Vec::new(),
            rule_info: HashMap::new(),
            template_ids: HashMap::new(),
            template_defs: HashMap::new(),
            template_id_alloc: slotmap::SlotMap::with_key(),
            router: OutputRouter::new(),
            functions: FunctionEnv::new(),
            globals: GlobalStore::new(),
            registered_globals: Vec::new(),
            generics: GenericRegistry::new(),
            module_registry: ModuleRegistry::new(),
            rule_modules: HashMap::new(),
            template_modules: HashMap::new(),
            initial_fact_id: None,
            action_diagnostics: Vec::new(),
            halted: false,
            creator_thread: std::thread::current().id(),
            _not_send_sync: PhantomData,
        }
    }

    /// Assert an ordered fact into working memory.
    ///
    /// The relation name is interned as a symbol. Field values are used as-is.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The relation name violates encoding constraints (e.g., non-ASCII in ASCII mode)
    /// - The engine is called from the wrong thread
    pub fn assert_ordered(
        &mut self,
        relation: &str,
        fields: Vec<Value>,
    ) -> Result<FactId, EngineError> {
        self.check_thread_affinity()?;

        let relation_sym = self
            .symbol_table
            .intern_symbol(relation, self.config.string_encoding)?;

        let fields_small = fields.into_iter().collect();
        let id = self.fact_base.assert_ordered(relation_sym, fields_small);

        // Propagate through rete network
        propagate_fact_assertion(&mut self.rete, &self.fact_base, id);

        Ok(id)
    }

    /// Assert a fully constructed fact into working memory.
    ///
    /// # Errors
    ///
    /// Returns an error if the engine is called from the wrong thread.
    pub fn assert(&mut self, fact: Fact) -> Result<FactId, EngineError> {
        self.check_thread_affinity()?;

        let id = match fact {
            Fact::Ordered(ordered) => self
                .fact_base
                .assert_ordered(ordered.relation, ordered.fields),
            Fact::Template(template) => self
                .fact_base
                .assert_template(template.template_id, template.slots),
        };

        // Propagate through rete network
        propagate_fact_assertion(&mut self.rete, &self.fact_base, id);

        Ok(id)
    }

    /// Retract a fact from working memory.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The fact ID does not exist
    /// - The engine is called from the wrong thread
    pub fn retract(&mut self, fact_id: FactId) -> Result<(), EngineError> {
        self.check_thread_affinity()?;

        let entry = self
            .fact_base
            .get(fact_id)
            .ok_or(EngineError::FactNotFound(fact_id))?;
        let fact = entry.fact.clone();

        // Retract from rete first (needs fact_base for negative node handling)
        self.rete.retract_fact(fact_id, &fact, &self.fact_base);

        // Then retract from fact base
        self.fact_base
            .retract(fact_id)
            .ok_or(EngineError::FactNotFound(fact_id))?;

        Ok(())
    }

    /// Get a fact by ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the engine is called from the wrong thread.
    pub fn get_fact(&self, fact_id: FactId) -> Result<Option<&Fact>, EngineError> {
        self.check_thread_affinity()?;

        Ok(self.fact_base.get(fact_id).map(|entry| &entry.fact))
    }

    /// Iterate over all user-visible facts in working memory.
    ///
    /// Returns an iterator of `(FactId, &Fact)` pairs. The synthetic
    /// `(initial-fact)` inserted by the engine for internal NCC/forall support
    /// is excluded from the results.
    ///
    /// # Errors
    ///
    /// Returns an error if the engine is called from the wrong thread.
    pub fn facts(&self) -> Result<impl Iterator<Item = (FactId, &Fact)>, EngineError> {
        self.check_thread_affinity()?;

        let exclude_id = self.initial_fact_id;
        Ok(self
            .fact_base
            .iter()
            .filter(move |(id, _)| Some(*id) != exclude_id)
            .map(|(id, entry)| (id, &entry.fact)))
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
    /// - The engine is called from the wrong thread
    pub fn intern_symbol(&mut self, s: &str) -> Result<Symbol, EngineError> {
        self.check_thread_affinity()?;

        Ok(self
            .symbol_table
            .intern_symbol(s, self.config.string_encoding)?)
    }

    /// Create a `FerricString` from a string slice.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The string violates encoding constraints
    /// - The engine is called from the wrong thread
    pub fn create_string(&self, s: &str) -> Result<FerricString, EngineError> {
        self.check_thread_affinity()?;

        Ok(FerricString::new(s, self.config.string_encoding)?)
    }

    /// Access the engine's Rete network for inspection.
    #[must_use]
    pub fn rete(&self) -> &ReteNetwork {
        &self.rete
    }

    /// Check that the current thread is the same as the creator thread.
    pub(crate) fn check_thread_affinity(&self) -> Result<(), EngineError> {
        let current = std::thread::current().id();
        if current != self.creator_thread {
            return Err(EngineError::WrongThread {
                creator: self.creator_thread,
                current,
            });
        }
        Ok(())
    }

    /// Execute RHS actions for a rule activation.
    ///
    /// Returns `true` if the rule logically fired (all test CEs passed and
    /// actions were executed), `false` if a test CE caused the rule to be
    /// suppressed.
    fn execute_activation_actions(
        &mut self,
        rule_id: RuleId,
        token_id: ferric_core::token::TokenId,
    ) -> bool {
        let Some(token) = self.rete.token_store.get(token_id).cloned() else {
            // No token — treat as not fired.
            return false;
        };

        let (fired, mut errors, focus_requests) = {
            let Some(info) = self.rule_info.get(&rule_id) else {
                // No compiled rule info — treat as not fired.
                return false;
            };

            let mut focus_requests = Vec::new();
            let (fired, errors) = actions::execute_actions(
                &mut self.fact_base,
                &mut self.rete,
                &mut self.symbol_table,
                &mut self.halted,
                &self.config,
                &token,
                info,
                &self.template_defs,
                &mut self.router,
                &self.functions,
                &mut self.globals,
                &mut focus_requests,
                &self.generics,
                &self.module_registry,
            );
            (fired, errors, focus_requests)
        };

        // Apply focus requests (push in reverse order so first arg is on top)
        for module_name in focus_requests.iter().rev() {
            match self.module_registry.get_by_name(module_name) {
                Some(id) => self.module_registry.push_focus(id),
                None => errors.push(ActionError::EvalError(format!(
                    "focus: unknown module `{module_name}`"
                ))),
            }
        }

        self.action_diagnostics.extend(errors);
        fired
    }

    /// Transfer ownership of this engine to the current thread.
    ///
    /// # Safety
    ///
    /// The caller must guarantee there are no outstanding references into engine
    /// internals that continue to be used from the previous owning thread.
    #[allow(unsafe_code)]
    pub unsafe fn move_to_current_thread(&mut self) {
        self.creator_thread = std::thread::current().id();
    }

    /// Fire a single rule activation from the agenda.
    ///
    /// Returns `None` if the agenda is empty. Otherwise pops the highest-priority
    /// activation and fires it (executing RHS actions if all test CEs pass).
    ///
    /// # Errors
    ///
    /// Returns an error if the engine is called from the wrong thread.
    pub fn step(&mut self) -> Result<Option<FiredRule>, EngineError> {
        self.check_thread_affinity()?;
        self.action_diagnostics.clear();

        let Some(activation) = self.rete.agenda.pop() else {
            return Ok(None);
        };

        let fired = FiredRule {
            rule_id: activation.rule,
            token_id: activation.token,
        };

        // Execute actions (errors are currently silently ignored).
        // The boolean return indicates whether test CEs passed, but step()
        // always returns Some(fired) to indicate an activation was processed.
        let _ = self.execute_activation_actions(activation.rule, activation.token);

        Ok(Some(fired))
    }

    /// Run the engine, firing rules until the agenda is empty, the limit is
    /// reached, or halt is requested.
    ///
    /// Clears any previous halt request before starting.
    ///
    /// Rule selection is focus-aware: only activations belonging to the module
    /// at the top of the focus stack are eligible to fire. When no eligible
    /// activations remain for the current focus module, the focus stack is
    /// popped and the next module is tried. The final baseline focus is
    /// preserved across runs; if it has no matching activations, execution
    /// halts with `AgendaEmpty`.
    ///
    /// # Errors
    ///
    /// Returns an error if the engine is called from the wrong thread.
    pub fn run(&mut self, limit: RunLimit) -> Result<RunResult, EngineError> {
        self.check_thread_affinity()?;
        self.halted = false;
        self.action_diagnostics.clear();

        let max_fires = match limit {
            RunLimit::Unlimited => usize::MAX,
            RunLimit::Count(n) => n,
        };

        let mut rules_fired = 0;

        while rules_fired < max_fires {
            if self.halted {
                return Ok(RunResult {
                    rules_fired,
                    halt_reason: HaltReason::HaltRequested,
                });
            }

            // Focus-aware activation selection: find the highest-priority
            // activation whose rule belongs to the current focus module.
            // When no activations exist for the focus module, pop the focus
            // stack and try the next module.
            let activation = loop {
                let Some(focus_module) = self.module_registry.current_focus() else {
                    return Ok(RunResult {
                        rules_fired,
                        halt_reason: HaltReason::AgendaEmpty,
                    });
                };

                let rule_modules = &self.rule_modules;
                if let Some(act) = self
                    .rete
                    .agenda
                    .pop_matching(|a| rule_modules.get(&a.rule).copied() == Some(focus_module))
                {
                    break act;
                }

                // No activations for this module. Pop to the next focus module,
                // but preserve the final baseline focus so subsequent run() calls
                // still have a stable module context.
                if self.module_registry.focus_stack().len() > 1 {
                    self.module_registry.pop_focus();
                    continue;
                }

                return Ok(RunResult {
                    rules_fired,
                    halt_reason: HaltReason::AgendaEmpty,
                });
            };

            let logically_fired =
                self.execute_activation_actions(activation.rule, activation.token);

            if logically_fired {
                rules_fired += 1;
            }
        }

        Ok(RunResult {
            rules_fired,
            halt_reason: HaltReason::LimitReached,
        })
    }

    /// Request that the engine stop execution after the current rule completes.
    ///
    /// This sets a flag that is checked between rule firings during `run`.
    /// Has no effect if the engine is not currently running.
    pub fn halt(&mut self) {
        self.halted = true;
    }

    /// Reset the engine: clear all facts, tokens, and activations, then
    /// re-assert all registered deffacts.
    ///
    /// The compiled rule network is preserved — only runtime state is cleared.
    ///
    /// # Errors
    ///
    /// Returns an error if the engine is called from the wrong thread.
    pub fn reset(&mut self) -> Result<(), EngineError> {
        self.check_thread_affinity()?;

        // Clear all runtime state
        self.fact_base = FactBase::new();
        self.rete.clear_working_memory();
        self.router.clear();
        self.action_diagnostics.clear();
        self.halted = false;

        // Reset focus stack to [MAIN] and current module to MAIN
        self.module_registry.reset_focus();

        // Re-initialize globals from registered initial values
        self.globals.clear();
        for (name, value) in &self.registered_globals {
            self.globals.set(name, value.clone());
        }

        // Re-assert registered deffacts
        for deffacts in &self.registered_deffacts {
            for fact in deffacts {
                let fact_id = match fact {
                    Fact::Ordered(ordered) => self
                        .fact_base
                        .assert_ordered(ordered.relation, ordered.fields.clone()),
                    Fact::Template(template) => self
                        .fact_base
                        .assert_template(template.template_id, template.slots.clone()),
                };
                // Propagate through rete
                propagate_fact_assertion(&mut self.rete, &self.fact_base, fact_id);
            }
        }

        // Re-assert (initial-fact) to restore the root token needed by NCC/forall CEs.
        // Update initial_fact_id so that facts() continues to exclude it.
        if self.initial_fact_id.is_some() {
            let initial_sym = self
                .symbol_table
                .intern_symbol("initial-fact", self.config.string_encoding)
                .expect("initial-fact symbol interning must succeed");
            let initial_fid = self
                .fact_base
                .assert_ordered(initial_sym, smallvec::SmallVec::new());
            propagate_fact_assertion(&mut self.rete, &self.fact_base, initial_fid);
            self.initial_fact_id = Some(initial_fid);
        }

        Ok(())
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
    #[must_use]
    pub fn get_output(&self, channel: &str) -> Option<&str> {
        self.router.get_output(channel)
    }

    /// Get non-fatal action diagnostics captured during the most recent run/step call.
    #[must_use]
    pub fn action_diagnostics(&self) -> &[ActionError] {
        &self.action_diagnostics
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
        self.globals.get(name)
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
        let module_id = self
            .module_registry
            .get_by_name(module_name)
            .ok_or_else(|| EngineError::ModuleNotFound(module_name.to_string()))?;
        self.module_registry.set_focus(module_id);
        Ok(())
    }

    /// Push a module onto the focus stack by name.
    ///
    /// # Errors
    ///
    /// Returns `ModuleNotFound` if the module has not been registered.
    pub fn push_focus(&mut self, module_name: &str) -> Result<(), EngineError> {
        let module_id = self
            .module_registry
            .get_by_name(module_name)
            .ok_or_else(|| EngineError::ModuleNotFound(module_name.to_string()))?;
        self.module_registry.push_focus(module_id);
        Ok(())
    }

    /// Verify engine-level structural consistency.
    ///
    /// This extends rete consistency checks with Phase 3 registries
    /// (modules/focus, functions, globals, generics).
    pub fn debug_assert_consistency(&self) {
        self.rete.debug_assert_consistency();
        self.module_registry.debug_assert_consistency();
        self.functions.debug_assert_consistency();
        self.globals.debug_assert_consistency();
        self.generics.debug_assert_consistency();

        for (rule_id, module_id) in &self.rule_modules {
            assert!(
                self.module_registry.get(*module_id).is_some(),
                "rule {rule_id:?} points to unknown module {module_id:?}"
            );
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
        for (name, _value) in &self.registered_globals {
            assert!(
                seen_globals.insert(name.as_str()),
                "duplicate registered global definition `{name}`"
            );
            assert!(
                self.globals.contains(name),
                "registered global `{name}` missing from runtime global store"
            );
        }
    }
}

/// Errors that can occur during engine operations.
#[derive(Debug, Error)]
pub enum EngineError {
    #[error("encoding error: {0}")]
    Encoding(#[from] EncodingError),

    #[error("fact not found: {0:?}")]
    FactNotFound(FactId),

    #[error("engine called from wrong thread (created on {creator:?}, called from {current:?})")]
    WrongThread {
        creator: ThreadId,
        current: ThreadId,
    },

    #[error("module not found: {0}")]
    ModuleNotFound(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferric_core::StringEncoding;

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
    fn retract_fact() {
        let mut engine = Engine::new(EngineConfig::utf8());
        let id = engine.assert_ordered("test", vec![]).unwrap();

        let result = engine.retract(id);
        assert!(result.is_ok());

        assert!(engine.get_fact(id).unwrap().is_none());
    }

    #[test]
    fn retract_nonexistent_fact_returns_error() {
        let mut engine = Engine::new(EngineConfig::utf8());
        let id = engine.assert_ordered("test", vec![]).unwrap();

        engine.retract(id).unwrap();
        let result = engine.retract(id);

        assert!(matches!(result, Err(EngineError::FactNotFound(_))));
    }

    #[test]
    fn assert_structured_ordered_fact() {
        let mut engine = Engine::new(EngineConfig::utf8());
        let relation = engine.intern_symbol("person").unwrap();
        let fact = Fact::Ordered(ferric_core::OrderedFact {
            relation,
            fields: smallvec::smallvec![Value::Integer(42)],
        });

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
        assert_eq!(s.as_str(), "hello world");
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

        let id1 = engine.assert_ordered("test", vec![]).unwrap();
        let id2 = engine.assert_ordered("test", vec![]).unwrap();

        let all: Vec<_> = engine.facts().unwrap().map(|(id, _)| id).collect();
        assert_eq!(all.len(), 2);
        assert!(all.contains(&id1));
        assert!(all.contains(&id2));
    }

    #[test]
    fn thread_affinity_marker_exists() {
        // Verify that Engine has the !Send + !Sync marker by checking its size.
        // The PhantomData<*mut ()> field ensures Engine is !Send + !Sync.
        let engine = Engine::new(EngineConfig::utf8());
        // The key point is that Engine contains PhantomData<*mut ()>,
        // which makes it !Send and !Sync. This test just verifies the marker exists.
        assert!(std::mem::size_of_val(&engine._not_send_sync) == 0);
    }

    #[test]
    fn move_to_current_thread_enables_safe_handoff() {
        #[allow(unsafe_code)]
        struct SendEngine(Engine);

        #[allow(unsafe_code)]
        unsafe impl Send for SendEngine {}

        let send_engine = SendEngine(Engine::new(EngineConfig::utf8()));
        let handle = std::thread::spawn(move || {
            let mut send_engine = send_engine;

            // Before transfer, calls from this thread should fail.
            assert!(matches!(
                send_engine.0.intern_symbol("before-transfer"),
                Err(EngineError::WrongThread { .. })
            ));

            #[allow(unsafe_code)]
            unsafe {
                send_engine.0.move_to_current_thread();
            }

            // After transfer, calls should succeed on this thread.
            let sym = send_engine.0.intern_symbol("after-transfer");
            assert!(sym.is_ok());
        });

        handle.join().expect("thread should complete");
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
        assert_eq!(fired.rule_id, ferric_core::beta::RuleId(1));
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

        // Should have 2 activations from deffacts
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
            .assert_ordered("person", vec![Value::Symbol(alice_sym)])
            .unwrap();
        assert_eq!(engine.rete.agenda.len(), 1);
    }

    #[test]
    fn retract_removes_from_rete() {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine.load_str("(defrule test (person ?x) =>)").unwrap();

        let alice_sym = engine.intern_symbol("Alice").unwrap();
        let fid = engine
            .assert_ordered("person", vec![Value::Symbol(alice_sym)])
            .unwrap();
        assert_eq!(engine.rete.agenda.len(), 1);

        engine.retract(fid).unwrap();
        assert!(engine.rete.agenda.is_empty());
    }
}
