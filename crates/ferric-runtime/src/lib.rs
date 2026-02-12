//! # Ferric Runtime
//!
//! Engine, execution environment, value types, and symbol interning.
//!
//! This crate is not intended for direct use by end-users; prefer the
//! `ferric` facade crate instead.
//!
//! ## Phase 1 baseline (loader and engine contracts)
//!
//! - `Engine::load_str` / `Engine::load_file` return `Result<LoadResult, Vec<LoadError>>`.
//! - `LoadResult` includes asserted fact IDs, collected `RuleDef`s, and warnings.
//! - `deffacts` is accepted as batch-assert behavior in Phase 1.
//! - Rule ingestion remains S-expression-level (`RuleDef`), with no automatic
//!   rule-to-rete compilation yet. Phase 2 adds the `RuleDef` → compiled network
//!   bridge.
//! - Engine API Phase 1 subset: `assert_ordered`, `assert(Fact)`, `retract`,
//!   `get_fact`, `facts()`, `intern_symbol`, `create_string`,
//!   `unsafe move_to_current_thread`. Full API (run/reset/call/modules) is
//!   later-phase scope.

pub mod actions;
pub mod config;
pub mod engine;
pub mod execution;
pub mod loader;

#[cfg(test)]
mod integration_tests;
#[cfg(test)]
mod phase2_integration_tests;
#[cfg(test)]
pub(crate) mod test_helpers;

// Re-export types from ferric-core for convenience.
pub use ferric_core::{
    AtomKey, EncodingError, ExternalAddress, ExternalTypeId, FerricString, Multifield,
    StringEncoding, Symbol, Value,
};

// Re-export primary types at crate root for convenience.
pub use actions::ActionError;
pub use config::EngineConfig;
pub use engine::{Engine, EngineError};
pub use execution::{FiredRule, HaltReason, RunLimit, RunResult};
pub use loader::{LoadError, LoadResult, RuleDef};

// Re-export Stage 2 AST types for working with loaded constructs
pub use ferric_parser::{RuleConstruct, TemplateConstruct};
