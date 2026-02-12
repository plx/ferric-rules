//! The Ferric rules engine.
//!
//! This module provides the main `Engine` type, which is the primary interface
//! for embedding applications. Phase 1 includes basic fact assertion/retraction
//! and thread affinity checking.

use std::marker::PhantomData;
use std::thread::ThreadId;
use thiserror::Error;

use ferric_core::{
    EncodingError, Fact, FactBase, FactId, FerricString, ReteCompiler, ReteNetwork, Symbol,
    SymbolTable, Value,
};

use crate::config::EngineConfig;
use crate::execution::{FiredRule, HaltReason, RunLimit, RunResult};

/// The Ferric rules engine.
///
/// This is the main entry point for embedding applications. The engine is
/// not `Send` or `Sync` — it must remain on the thread that created it.
///
/// ## Phase 1 API surface
///
/// - Fact assertion/retraction (`assert_ordered`, `assert`, `retract`)
/// - Fact query (`get_fact`, `facts`)
/// - Symbol interning and string creation
/// - Source loading (`load_str`, `load_file`) — returns `RuleDef` placeholders
/// - Thread affinity enforcement with `unsafe move_to_current_thread`
///
/// ## Phase 2 additions (planned)
///
/// - Rule compilation from Stage 2 AST into shared rete network
/// - Execution loop (`run`, `step`, `halt`, `reset`)
/// - RHS action execution (`assert`, `retract`, `modify`, `duplicate`)
/// - Agenda conflict strategy selection
pub struct Engine {
    pub(crate) fact_base: FactBase,
    pub(crate) symbol_table: SymbolTable,
    pub(crate) config: EngineConfig,
    pub(crate) rete: ReteNetwork,
    pub(crate) compiler: ReteCompiler,
    /// Registered deffacts for re-assertion on reset.
    pub(crate) registered_deffacts: Vec<Vec<Fact>>,
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
        let fact = self.fact_base.get(id).unwrap().fact.clone();
        self.rete.assert_fact(id, &fact, &self.fact_base);

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
        let fact = self.fact_base.get(id).unwrap().fact.clone();
        self.rete.assert_fact(id, &fact, &self.fact_base);

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

    /// Iterate over all facts in working memory.
    ///
    /// Returns an iterator of `(FactId, &Fact)` pairs.
    ///
    /// # Errors
    ///
    /// Returns an error if the engine is called from the wrong thread.
    pub fn facts(&self) -> Result<impl Iterator<Item = (FactId, &Fact)>, EngineError> {
        self.check_thread_affinity()?;

        Ok(self.fact_base.iter().map(|(id, entry)| (id, &entry.fact)))
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
    /// activation and fires it.
    ///
    /// Note: In Phase 2 Pass 008, "firing" pops the activation but does not
    /// execute RHS actions (action execution is added in Pass 009).
    ///
    /// # Errors
    ///
    /// Returns an error if the engine is called from the wrong thread.
    pub fn step(&mut self) -> Result<Option<FiredRule>, EngineError> {
        self.check_thread_affinity()?;

        let Some(activation) = self.rete.agenda.pop() else {
            return Ok(None);
        };

        // Pass 009 will add RHS action execution here.

        Ok(Some(FiredRule {
            rule_id: activation.rule,
            token_id: activation.token,
        }))
    }

    /// Run the engine, firing rules until the agenda is empty, the limit is
    /// reached, or halt is requested.
    ///
    /// Clears any previous halt request before starting.
    ///
    /// # Errors
    ///
    /// Returns an error if the engine is called from the wrong thread.
    pub fn run(&mut self, limit: RunLimit) -> Result<RunResult, EngineError> {
        self.check_thread_affinity()?;
        self.halted = false;

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

            let Some(activation) = self.rete.agenda.pop() else {
                return Ok(RunResult {
                    rules_fired,
                    halt_reason: HaltReason::AgendaEmpty,
                });
            };

            // Pass 009 will add RHS action execution here.
            let _ = activation; // Suppress unused warning

            rules_fired += 1;
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
        self.halted = false;

        // Re-assert registered deffacts
        for deffacts in &self.registered_deffacts.clone() {
            for fact in deffacts {
                let fact_id = match fact {
                    Fact::Ordered(ordered) => {
                        self.fact_base
                            .assert_ordered(ordered.relation, ordered.fields.clone())
                    }
                    Fact::Template(template) => {
                        self.fact_base
                            .assert_template(template.template_id, template.slots.clone())
                    }
                };
                // Propagate through rete
                let stored_fact = self.fact_base.get(fact_id).unwrap().fact.clone();
                self.rete.assert_fact(fact_id, &stored_fact, &self.fact_base);
            }
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
        assert_eq!(result.halt_reason, crate::execution::HaltReason::AgendaEmpty);
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
        assert_eq!(result.halt_reason, crate::execution::HaltReason::LimitReached);
        assert_eq!(engine.rete.agenda.len(), 1);
    }

    #[test]
    fn run_on_empty_agenda() {
        use crate::execution::RunLimit;
        let mut engine = Engine::new(EngineConfig::utf8());
        let result = engine.run(RunLimit::Unlimited).unwrap();
        assert_eq!(result.rules_fired, 0);
        assert_eq!(result.halt_reason, crate::execution::HaltReason::AgendaEmpty);
    }

    #[test]
    fn run_with_zero_limit() {
        use crate::execution::RunLimit;
        let mut engine = Engine::new(EngineConfig::utf8());
        engine.load_str("(defrule test (person ?x) =>)").unwrap();
        engine.load_str("(assert (person Alice))").unwrap();

        let result = engine.run(RunLimit::Count(0)).unwrap();
        assert_eq!(result.rules_fired, 0);
        assert_eq!(result.halt_reason, crate::execution::HaltReason::LimitReached);
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
        engine.assert_ordered("person", vec![Value::Symbol(alice_sym)]).unwrap();
        assert_eq!(engine.rete.agenda.len(), 1);
    }

    #[test]
    fn retract_removes_from_rete() {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine.load_str("(defrule test (person ?x) =>)").unwrap();

        let alice_sym = engine.intern_symbol("Alice").unwrap();
        let fid = engine.assert_ordered("person", vec![Value::Symbol(alice_sym)]).unwrap();
        assert_eq!(engine.rete.agenda.len(), 1);

        engine.retract(fid).unwrap();
        assert!(engine.rete.agenda.is_empty());
    }
}
