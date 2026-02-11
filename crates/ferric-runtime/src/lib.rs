//! # Ferric Runtime
//!
//! Engine, execution environment, value types, and symbol interning.
//!
//! This crate is not intended for direct use by end-users; prefer the
//! `ferric` facade crate instead.

pub mod config;
pub mod engine;
pub mod loader;

#[cfg(test)]
mod integration_tests;

// Re-export types from ferric-core for convenience.
pub use ferric_core::{
    AtomKey, EncodingError, ExternalAddress, ExternalTypeId, FerricString, Multifield,
    StringEncoding, Symbol, Value,
};

// Re-export primary types at crate root for convenience.
pub use config::EngineConfig;
pub use engine::{Engine, EngineError};
pub use loader::{LoadError, LoadResult, RuleDef};
