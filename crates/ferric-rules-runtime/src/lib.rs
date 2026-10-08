//! # Ferric Runtime
//!
//! Engine, execution environment, value types, and symbol interning.
//!
//! This crate is not intended for direct use by end-users; prefer the
//! `ferric-rules` facade crate instead.
//!
//! ## Capabilities
//!
//! - Loading: Stage 2 constructs (`deftemplate`, `defrule`, `deffacts`,
//!   `deffunction`, `defglobal`, `defmodule`, `defgeneric`, `defmethod`) are
//!   translated and compiled into a shared Rete network, with pattern
//!   validation and source-located diagnostics.
//! - Matching: ordered and template patterns, `test` CEs (predicate nodes),
//!   `not`, `exists`, NCC, and `forall` (desugared to NCC, including vacuous
//!   truth).
//! - Execution: `run`, `step`, `halt`, `reset`, and `clear`; Depth, Breadth,
//!   LEX, and MEA conflict strategies; a module focus stack.
//! - Actions and expressions: a shared evaluator for RHS actions, `test` CEs,
//!   and callable bodies; `assert`/`retract`/`modify`/`duplicate`, `bind`,
//!   `if`/`while`/`loop-for-count`, and `printout` with per-channel output
//!   capture via `OutputRouter`.
//! - Callables: `deffunction`s, generic dispatch with CLIPS-style specificity
//!   ranking and `call-next-method`, module-qualified `MODULE::name`
//!   resolution, and cross-module visibility enforcement.
//! - The builtin function library (math, string/symbol, multifield, I/O,
//!   environment, and agenda/focus queries).
//!
//! ## Known limitations
//!
//! - No truth maintenance: `logical` CEs are rejected at load time.
//! - `defclass`/`definstances`/`defmessage-handler` (COOL) are not implemented.
//! - `forall` supports a single condition and a single then-clause, and
//!   cannot be nested.
//!
//! See `docs/compatibility.md` in the repository for the full supported
//! subset and known differences from CLIPS.

mod tracing_support;

pub mod actions;
mod builtin_validation;
mod callable_validation;
pub mod config;
mod effects;
pub mod engine;
mod environment;
mod evaluation;
pub mod evaluator;
pub mod execution;
mod fact_address;
mod fact_initializer;
mod fact_io;
mod field_scanner;
mod formatting;
pub mod functions;
pub mod host;
mod inspection;
mod introspection;
pub mod loader;
pub mod modules;
pub mod qualified_name;
mod query_cursor;
mod query_source_order;
mod query_targets;
mod query_validation;
mod random;
pub mod router;
mod rule_complexity;
#[cfg(feature = "serde")]
pub mod serialization;
pub(crate) mod slot_constraints;
mod source_limits;
pub use source_limits::MAX_SOURCE_BYTES;
mod template_defaults;
mod template_identity;
mod template_reload;
pub(crate) mod templates;
mod value_print;

#[cfg(test)]
mod integration_tests;
#[cfg(test)]
mod phase2_integration_tests;
#[cfg(test)]
mod phase3_integration_tests;
#[cfg(test)]
mod phase4_integration_tests;
#[cfg(test)]
pub(crate) mod test_helpers;

// Re-export types from ferric-rules-core for convenience.
pub use ferric_rules_core::{
    AtomKey, EncodingError, ExternalAddress, ExternalTypeId, FactAddress, FerricString,
    InstanceName, IntoFieldValues, Multifield, StringEncoding, Symbol, Value,
};

// Re-export primary types at crate root for convenience.
pub use actions::ActionError;
pub use config::EngineConfig;
pub use engine::{Engine, EngineError, FactAssertionResult, InitError, InputSource};
pub use evaluation::EvalStrError;
pub use execution::{FiredRule, HaltReason, RunLimit, RunResult};
pub use functions::{FunctionEnv, GenericRegistry, GlobalStore};
pub use host::{
    FactHandle, HostFact, HostValue, IntoHostFields, SymbolHandle, HOST_VALUE_MAX_DEPTH,
    HOST_VALUE_MAX_ITEMS,
};
pub use inspection::AgendaEntry;
pub use loader::{LoadError, LoadResult, RuleDef};
pub use modules::{ModuleId, ModuleRegistry};
pub use qualified_name::{parse_qualified_name, QualifiedName};
pub use router::STANDARD_CHANNELS;
#[cfg(feature = "serde")]
pub use serialization::{
    SerializationError, SerializationFormat, SnapshotFileError, MAX_SNAPSHOT_BYTES,
};

// Re-export Stage 2 AST types for working with loaded constructs
pub use ferric_rules_parser::{
    FunctionConstruct, GenericConstruct, GlobalConstruct, GlobalDefinition, MethodConstruct,
    ModuleConstruct, RuleConstruct, TemplateConstruct,
};
