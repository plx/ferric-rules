//! The Ferric rules engine.
//!
//! This module provides the main `Engine` type, which is the primary interface
//! for embedding applications. Phase 1 includes basic fact assertion/retraction
//! and thread affinity checking.

use std::marker::PhantomData;
use std::thread::ThreadId;
use thiserror::Error;

use ferric_core::{
    EncodingError, Fact, FactBase, FactId, FerricString, Symbol, SymbolTable, Value,
};

use crate::config::EngineConfig;

/// The Ferric rules engine.
///
/// This is the main entry point for embedding applications. The engine is
/// not `Send` or `Sync` — it must remain on the thread that created it.
///
/// Phase 1 provides fact assertion and retraction, symbol interning, and
/// string creation. Pattern matching, rules, and agenda will be added in
/// later phases.
pub struct Engine {
    pub(crate) fact_base: FactBase,
    pub(crate) symbol_table: SymbolTable,
    pub(crate) config: EngineConfig,
    creator_thread: ThreadId,
    // Marker to ensure Engine is !Send + !Sync
    _not_send_sync: PhantomData<*mut ()>,
}

impl Engine {
    /// Create a new engine with the given configuration.
    #[must_use]
    pub fn new(config: EngineConfig) -> Self {
        Self {
            fact_base: FactBase::new(),
            symbol_table: SymbolTable::new(),
            config,
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
}
