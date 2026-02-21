//! # Ferric Runtime
//!
//! Engine, execution environment, value types, and symbol interning.
//!
//! This crate is not intended for direct use by end-users; prefer the
//! `ferric` facade crate instead.
//!
//! ## Phase 2 complete
//!
//! - Stage 2 AST and semantic interpretation for defrule, deftemplate, deffacts.
//! - Rule compilation from Stage 2 AST into shared rete network.
//! - Execution loop: `run`, `step`, `halt`, `reset`.
//! - Basic RHS action execution: `assert`, `retract`, `modify`, `duplicate`, `halt`.
//! - NCC (not/and), exists, and negative node types in rete.
//! - Pattern validation (nesting depth, unsupported combinations).
//! - Agenda conflict strategies: Depth, Breadth, LEX, MEA.
//!
//! ## Phase 3 complete
//!
//! - Shared expression evaluator for RHS actions and `test` CEs (Pass 002).
//! - `test` CEs compile and evaluate at firing time (Pass 002).
//! - Nested function calls in RHS (arithmetic, comparison, boolean, type predicates).
//! - Template-aware `modify`/`duplicate` (Pass 003).
//! - `printout` with per-channel output capture via `OutputRouter` (Pass 004).
//! - `deffunction` runtime: user-defined functions callable from rules and
//!   other functions, with parameter binding and recursion limits (Pass 006).
//! - `defglobal` runtime: global variable read/write via `bind`, with
//!   reset re-initialization (Pass 006).
//! - `defmodule` runtime: module registry, focus stack, focus-aware `run()`,
//!   `focus` RHS action, and cross-module template visibility (Pass 008).
//! - `defgeneric`/`defmethod` runtime: type-based method dispatch with
//!   index ordering and auto-index assignment (Pass 009).
//! - `forall` CE: limited subset (single condition + single then-clause),
//!   desugared to NCC, vacuous truth, initial-fact support (Pass 010).
//!
//! ## Phase 3 known limitations
//!
//! - Module-qualified names (e.g., `MAIN::template`) not yet supported.
//! - `forall` limited to single condition + single then-clause.
//! - No truth maintenance / logical support.
//! - `defclass`/`definstances`/`defmessage-handler` not implemented.

pub mod actions;
pub mod config;
pub mod engine;
pub mod evaluator;
pub mod execution;
pub mod functions;
pub mod loader;
pub mod modules;
pub mod router;
pub(crate) mod templates;

#[cfg(test)]
mod integration_tests;
#[cfg(test)]
mod phase2_integration_tests;
#[cfg(test)]
mod phase3_integration_tests;
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
pub use functions::{FunctionEnv, GenericRegistry, GlobalStore};
pub use loader::{LoadError, LoadResult, RuleDef};
pub use modules::{ModuleId, ModuleRegistry};

// Re-export Stage 2 AST types for working with loaded constructs
pub use ferric_parser::{
    FunctionConstruct, GenericConstruct, GlobalConstruct, GlobalDefinition, MethodConstruct,
    ModuleConstruct, RuleConstruct, TemplateConstruct,
};
