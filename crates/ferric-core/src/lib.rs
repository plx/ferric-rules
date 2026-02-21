//! # Ferric Core
//!
//! Rete network, pattern matching, agenda, and related core engine internals.
//!
//! This crate is not intended for direct use by end-users; prefer the
//! `ferric` facade crate instead.
//!
//! ## Architecture
//!
//! - Value/symbol/string/encoding primitives live here in `ferric-core`, not in
//!   `ferric-runtime`. This was an intentional cycle-breaking decision.
//! - Rete code is organized as flat modules (`alpha.rs`, `beta.rs`, `rete.rs`,
//!   `token.rs`) rather than a nested `src/rete/*` tree.
//! - Beta-memory cleanup during retraction is owner-node-directed (no all-memory
//!   scan on hot paths).
//! - Consistency checks (`debug_assert_consistency`) are available on
//!   `TokenStore`, `AlphaNetwork`, `BetaNetwork`, `Agenda`, and `ReteNetwork`.
//!
//! ## Phase 2 complete
//!
//! - Rule compilation from interpreted constructs into shared rete network.
//! - Negative, NCC, and exists node types.
//! - Full agenda conflict strategies (Depth, Breadth, LEX, MEA).
//! - Pattern validation (nesting depth, unsupported nesting combinations).
//!
//! ## Phase 3 scope
//!
//! - Template pattern compilation support.
//! - `forall` CE node type.
//! - Expression evaluation infrastructure for `test` CE and RHS functions.

pub mod agenda;
pub mod alpha;
pub mod beta;
pub mod binding;
pub mod compiler;
pub mod encoding;
pub mod exists;
pub mod fact;
pub mod ncc;
pub mod negative;
pub mod rete;
pub mod strategy;
pub mod string;
pub mod symbol;
pub mod token;
pub mod validation;
pub mod value;

// Re-export primary types at crate root for convenience.
pub use agenda::{Activation, ActivationId, Agenda, AgendaKey, StrategyOrd};
pub use alpha::{
    AlphaEntryType, AlphaMemory, AlphaMemoryId, AlphaNetwork, AlphaNode, ConstantTest,
    ConstantTestType, SlotIndex,
};
pub use beta::{BetaMemory, BetaMemoryId, BetaNetwork, BetaNode, JoinTest, JoinTestType, RuleId};
pub use binding::{BindingSet, VarId, VarMap};
pub use compiler::{
    CompilableCondition, CompilablePattern, CompilableRule, CompileError, CompileResult,
    ReteCompiler,
};
pub use encoding::{EncodingError, StringEncoding};
pub use exists::{ExistsMemory, ExistsMemoryId};
pub use fact::{Fact, FactBase, FactEntry, FactId, OrderedFact, TemplateFact, TemplateId};
pub use ncc::{NccMemory, NccMemoryId};
pub use negative::{NegativeMemory, NegativeMemoryId};
pub use rete::ReteNetwork;
pub use strategy::ConflictResolutionStrategy;
pub use string::FerricString;
pub use symbol::{Symbol, SymbolTable};
pub use token::{NodeId, Token, TokenId, TokenStore};
pub use validation::{PatternValidationError, PatternViolation, SourceLocation, ValidationStage};
pub use value::{AtomKey, ExternalAddress, ExternalTypeId, Multifield, Value};
