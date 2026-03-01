//! The Ferric rules engine.
//!
//! This module provides the main `Engine` type, which is the primary interface
//! for embedding applications. Phase 1 includes basic fact assertion/retraction
//! and thread affinity checking.

use rustc_hash::FxHashMap as HashMap;
use std::collections::VecDeque;
use std::marker::PhantomData;
use std::rc::Rc;
use std::thread::ThreadId;
use thiserror::Error;

use ferric_core::beta::RuleId;
use ferric_core::{
    EncodingError, Fact, FactBase, FactId, FerricString, IntoFieldValues, ReteCompiler,
    ReteNetwork, Symbol, SymbolTable, TemplateId, Value,
};

use crate::actions::{self, ActionError, CompiledRuleInfo};
use crate::config::EngineConfig;
use crate::execution::{FiredRule, HaltReason, RunLimit, RunResult};
use crate::functions::{FunctionEnv, GenericRegistry, GlobalStore, ModuleNameMap};
use crate::modules::{ModuleId, ModuleRegistry};
use crate::router::OutputRouter;
use crate::templates::RegisteredTemplate;

pub(crate) type RuleIndex<T> = Vec<Option<T>>;

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
///   desugared to NCC, with vacuous truth and initial-fact support.
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
    pub(crate) symbol_table: SymbolTable,
    pub(crate) config: EngineConfig,
    pub(crate) rete: ReteNetwork,
    pub(crate) compiler: ReteCompiler,
    /// Registered deffacts for re-assertion on reset.
    pub(crate) registered_deffacts: Vec<Vec<Fact>>,
    /// Compiled rule info for action execution.
    pub(crate) rule_info: RuleIndex<Rc<CompiledRuleInfo>>,
    /// Registered template definitions: name → `TemplateId`.
    pub(crate) template_ids: HashMap<Box<str>, TemplateId>,
    /// Template slot metadata indexed by `TemplateId`.
    pub(crate) template_defs: slotmap::SlotMap<TemplateId, RegisteredTemplate>,
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
    pub(crate) template_modules: slotmap::SecondaryMap<ferric_core::TemplateId, ModuleId>,
    /// Function-to-module association for consistency-check bookkeeping.
    pub(crate) function_modules: ModuleNameMap<ModuleId>,
    /// Global-to-module association for consistency-check bookkeeping.
    pub(crate) global_modules: ModuleNameMap<ModuleId>,
    /// Generic-to-module association for consistency-check bookkeeping.
    pub(crate) generic_modules: ModuleNameMap<ModuleId>,
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
    /// Input buffer for `read`/`readline` calls from rules.
    pub(crate) input_buffer: VecDeque<String>,
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
            rule_info: Vec::new(),
            template_ids: HashMap::default(),
            template_defs: slotmap::SlotMap::with_key(),
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
            halted: false,
            input_buffer: VecDeque::new(),
            creator_thread: std::thread::current().id(),
            _not_send_sync: PhantomData,
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

    /// Assert an ordered fact into working memory.
    ///
    /// The relation name is interned as a symbol. Fields can be passed as a
    /// `Vec<Value>`, a single `Value`, a primitive (`i64`, `i32`, `f64`),
    /// a `Symbol`, a `FerricString`, or a fixed-size array of `Value`s.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// engine.assert_ordered("count", 42_i64)?;           // single integer
    /// engine.assert_ordered("tier", free_symbol)?;        // single Symbol
    /// engine.assert_ordered("pair", vec![v1, v2])?;       // multiple values
    /// engine.assert_ordered("empty", vec![])?;            // no fields
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The relation name violates encoding constraints (e.g., non-ASCII in ASCII mode)
    /// - The engine is called from the wrong thread
    pub fn assert_ordered<F: IntoFieldValues>(
        &mut self,
        relation: &str,
        fields: F,
    ) -> Result<FactId, EngineError> {
        self.check_thread_affinity()?;

        let relation_sym = self
            .symbol_table
            .intern_symbol(relation, self.config.string_encoding)?;

        let fields_small = fields.into_field_values();
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

    /// Find ordered facts by relation name.
    ///
    /// Returns a vector of `(FactId, &Fact)` pairs for all ordered facts
    /// whose relation matches the given name. Returns an empty vector if the
    /// relation name has not been interned or no matching facts exist.
    ///
    /// # Errors
    ///
    /// Returns an error if the engine is called from the wrong thread.
    pub fn find_facts(&self, relation: &str) -> Result<Vec<(FactId, &Fact)>, EngineError> {
        self.check_thread_affinity()?;

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
                let entry = self.fact_base.get(fid)?;
                Some((fid, &entry.fact))
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

    /// Intern a symbol and wrap it as a [`Value::Symbol`].
    ///
    /// This is a convenience for the common pattern of
    /// `Value::Symbol(engine.intern_symbol(s)?)`.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The string violates encoding constraints
    /// - The engine is called from the wrong thread
    pub fn symbol_value(&mut self, s: &str) -> Result<Value, EngineError> {
        Ok(Value::Symbol(self.intern_symbol(s)?))
    }

    /// Assert a single-field ordered fact whose value is a symbol.
    ///
    /// Combines symbol interning and fact assertion into one call. Equivalent to:
    /// ```ignore
    /// let sym = engine.intern_symbol(symbol_name)?;
    /// engine.assert_ordered(relation, Value::Symbol(sym))?;
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Either name violates encoding constraints
    /// - The engine is called from the wrong thread
    pub fn assert_ordered_symbol(
        &mut self,
        relation: &str,
        symbol_name: &str,
    ) -> Result<FactId, EngineError> {
        self.check_thread_affinity()?;

        let relation_sym = self
            .symbol_table
            .intern_symbol(relation, self.config.string_encoding)?;
        let value_sym = self
            .symbol_table
            .intern_symbol(symbol_name, self.config.string_encoding)?;

        let fields = smallvec::smallvec![Value::Symbol(value_sym)];
        let id = self.fact_base.assert_ordered(relation_sym, fields);

        propagate_fact_assertion(&mut self.rete, &self.fact_base, id);

        Ok(id)
    }

    /// Return the CLIPS `TRUE` symbol as a [`Value`].
    ///
    /// The symbol is interned on first use and cached thereafter.
    pub fn clips_true(&mut self) -> Result<Value, EngineError> {
        self.check_thread_affinity()?;
        let sym = self
            .symbol_table
            .intern_symbol("TRUE", self.config.string_encoding)
            .expect("TRUE is valid in all encodings");
        Ok(Value::Symbol(sym))
    }

    /// Return the CLIPS `FALSE` symbol as a [`Value`].
    ///
    /// The symbol is interned on first use and cached thereafter.
    pub fn clips_false(&mut self) -> Result<Value, EngineError> {
        self.check_thread_affinity()?;
        let sym = self
            .symbol_table
            .intern_symbol("FALSE", self.config.string_encoding)
            .expect("FALSE is valid in all encodings");
        Ok(Value::Symbol(sym))
    }

    /// Resolve a [`Symbol`] to its string representation.
    ///
    /// Returns `None` if the symbol is not in this engine's symbol table.
    /// No thread-affinity check — symbol table contents are immutable once interned.
    #[must_use]
    pub fn resolve_symbol(&self, sym: Symbol) -> Option<&str> {
        self.symbol_table.resolve_symbol_str(sym)
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

    /// Look up a rule name by its internal ID.
    ///
    /// Returns `None` if the ID does not correspond to a known rule.
    pub fn rule_name(&self, rule_id: RuleId) -> Option<&str> {
        rule_index_get(&self.rule_info, rule_id).map(|info| info.name.as_str())
    }

    /// Check that the current thread is the same as the creator thread.
    pub fn check_thread_affinity(&self) -> Result<(), EngineError> {
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
    /// Returns `(logically_fired, reset_requested, clear_requested)`.
    /// - `logically_fired` is `true` if all test CEs passed and actions were executed.
    /// - `reset_requested` is `true` if a `(reset)` action was executed in the RHS.
    /// - `clear_requested` is `true` if a `(clear)` action was executed in the RHS.
    fn execute_activation_actions(
        &mut self,
        rule_id: RuleId,
        token_id: ferric_core::token::TokenId,
    ) -> (bool, bool, bool) {
        let Some(token) = self.rete.token_store.get(token_id).cloned() else {
            // No token — treat as not fired.
            return (false, false, false);
        };

        // Clone the handle so we can pass both this rule and the full map to
        // action helpers without deep-cloning `CompiledRuleInfo`.
        let Some(info) = rule_index_get(&self.rule_info, rule_id).cloned() else {
            // No compiled rule info — treat as not fired.
            return (false, false, false);
        };

        let current_module = rule_index_get(&self.rule_modules, rule_id)
            .copied()
            .unwrap_or_else(|| self.module_registry.main_module_id());

        let mut focus_requests = Vec::new();
        let (fired, reset_requested, clear_requested, mut errors) = {
            let mut action_context = actions::ActionExecutionContext {
                engine: self,
                focus_requests: &mut focus_requests,
                current_module,
            };
            actions::execute_actions(&token, info.as_ref(), &mut action_context)
        };

        // Apply focus requests (push in reverse order so first arg is on top)
        for module_name in focus_requests.iter().rev() {
            match self.resolve_focus_module(module_name) {
                Ok(id) => self.module_registry.push_focus(id),
                Err(_) => errors.push(ActionError::EvalError(format!(
                    "focus: unknown module `{module_name}`"
                ))),
            }
        }

        self.action_diagnostics.extend(errors);
        (fired, reset_requested, clear_requested)
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

    /// Pop the next activation eligible under current focus semantics.
    ///
    /// Selection is module-aware: only activations whose rule belongs to the
    /// current focus module are eligible. If the current focus module has no
    /// eligible activations and there are stacked focuses, the top focus is
    /// popped and selection continues. The final baseline focus is preserved.
    fn pop_next_focus_activation(&mut self) -> Option<ferric_core::Activation> {
        loop {
            let focus_module = self.module_registry.current_focus()?;
            let rule_modules = &self.rule_modules;

            if let Some(activation) = self.rete.agenda.pop_matching(|a| {
                rule_index_get(rule_modules, a.rule).copied() == Some(focus_module)
            }) {
                return Some(activation);
            }

            if self.module_registry.focus_stack().len() > 1 {
                self.module_registry.pop_focus();
                continue;
            }

            return None;
        }
    }

    /// Fire a single rule activation from the agenda.
    ///
    /// Returns `None` when no activation is eligible under current focus
    /// semantics. Otherwise pops the highest-priority eligible activation and
    /// fires it (executing RHS actions if all test CEs pass).
    ///
    /// # Errors
    ///
    /// Returns an error if the engine is called from the wrong thread.
    pub fn step(&mut self) -> Result<Option<FiredRule>, EngineError> {
        self.check_thread_affinity()?;
        self.action_diagnostics.clear();

        let Some(activation) = self.pop_next_focus_activation() else {
            return Ok(None);
        };

        let fired = FiredRule {
            rule_id: activation.rule,
            token_id: activation.token,
        };

        // Execute actions (errors are currently silently ignored).
        // The boolean return indicates whether test CEs passed, but step()
        // always returns Some(fired) to indicate an activation was processed.
        let (_, reset_requested, clear_requested) =
            self.execute_activation_actions(activation.rule, activation.token);

        if clear_requested {
            self.clear();
        } else if reset_requested {
            let _ = self.reset();
        }
        // After reset or clear, the engine is in a new state.
        // step() still returns the FiredRule indicating what fired.

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

            // Focus-aware activation selection preserves the final baseline
            // focus when no activations are eligible.
            let Some(activation) = self.pop_next_focus_activation() else {
                return Ok(RunResult {
                    rules_fired,
                    halt_reason: HaltReason::AgendaEmpty,
                });
            };

            let (logically_fired, reset_requested, clear_requested) =
                self.execute_activation_actions(activation.rule, activation.token);

            if logically_fired {
                rules_fired += 1;
            }

            if clear_requested {
                self.clear();
                return Ok(RunResult {
                    rules_fired,
                    halt_reason: HaltReason::HaltRequested,
                });
            }

            if reset_requested {
                let _ = self.reset();
                // Stop execution after reset — the caller can invoke run() again
                // with the freshly-reset working memory.
                return Ok(RunResult {
                    rules_fired,
                    halt_reason: HaltReason::HaltRequested,
                });
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
        // Note: input_buffer is intentionally NOT cleared on reset.
        // Input is live I/O state that should persist across resets.

        // Reset focus stack to [MAIN] and current module to MAIN
        self.module_registry.reset_focus();

        // Re-initialize globals from registered initial values
        self.globals.clear();
        for (module_id, name, value) in &self.registered_globals {
            self.globals.set(*module_id, name, value.clone());
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

    /// Push a line of input for `read`/`readline` to consume.
    ///
    /// Lines are consumed in FIFO order. Each call to `(read)` or `(readline)`
    /// in a rule RHS pops one entry from this buffer.
    pub fn push_input(&mut self, line: &str) {
        self.input_buffer.push_back(line.to_string());
    }

    /// Clear the engine: remove all rules, facts, templates, functions, globals,
    /// and module definitions. Returns the engine to its initial empty state.
    ///
    /// Unlike `reset()`, which preserves compiled rules and templates,
    /// `clear()` removes everything.
    pub fn clear(&mut self) {
        self.fact_base = FactBase::new();
        self.rete = ReteNetwork::with_strategy(self.config.strategy);
        self.compiler = ReteCompiler::new();
        self.registered_deffacts.clear();
        self.rule_info.clear();
        self.template_ids.clear();
        self.template_defs = slotmap::SlotMap::with_key();
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
    #[must_use]
    pub fn get_output(&self, channel: &str) -> Option<&str> {
        self.router.get_output(channel)
    }

    /// Clear captured output for a named `printout` channel.
    pub fn clear_output_channel(&mut self, channel: &str) {
        self.router.clear_channel(channel);
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
    pub fn debug_assert_consistency(&self) {
        use std::collections::HashSet;
        self.rete.debug_assert_consistency();
        self.module_registry.debug_assert_consistency();
        self.functions.debug_assert_consistency();
        self.globals.debug_assert_consistency();
        self.generics.debug_assert_consistency();

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

            // Pre-intern a small pool of relation symbols so we can refer to
            // them by index in the operation stream.
            let relation_pool: Vec<&str> = vec!["rel0", "rel1", "rel2"];

            // Shadow model: track which FactIds are currently live.
            let mut live: Vec<FactId> = Vec::new();
            // All FactIds ever successfully asserted (live or retracted).
            let mut all_asserted: Vec<FactId> = Vec::new();

            for op in &ops {
                match op {
                    FactOp::AssertOrdered(name_idx) => {
                        let relation = relation_pool[name_idx % relation_pool.len()];
                        let fid = engine.assert_ordered(relation, vec![]).unwrap();
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

            // Assert N ordered facts and collect their IDs.
            let mut live: Vec<FactId> = (0..n)
                .map(|i| {
                    let relation = if i % 2 == 0 { "even" } else { "odd" };
                    engine.assert_ordered(relation, vec![]).unwrap()
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
                engine.assert_ordered(relation, vec![]).unwrap();
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
