//! # Ferric Core
//!
//! Rete network, pattern matching, agenda, and related core engine internals.
//!
//! This crate is not intended for direct use by end-users; prefer the
//! `ferric` facade crate instead.

pub mod agenda;
pub mod alpha;
pub mod beta;
pub mod binding;
pub mod encoding;
pub mod fact;
pub mod rete;
pub mod string;
pub mod symbol;
pub mod token;
pub mod value;

// Re-export primary types at crate root for convenience.
pub use agenda::{Activation, ActivationId, Agenda, AgendaKey};
pub use alpha::{
    AlphaEntryType, AlphaMemory, AlphaMemoryId, AlphaNetwork, AlphaNode, ConstantTest,
    ConstantTestType, SlotIndex,
};
pub use beta::{BetaMemory, BetaMemoryId, BetaNetwork, BetaNode, JoinTest, JoinTestType, RuleId};
pub use binding::{BindingSet, VarId, VarMap};
pub use encoding::{EncodingError, StringEncoding};
pub use fact::{Fact, FactBase, FactEntry, FactId, OrderedFact, TemplateFact, TemplateId};
pub use rete::ReteNetwork;
pub use string::FerricString;
pub use symbol::{Symbol, SymbolTable};
pub use token::{NodeId, Token, TokenId, TokenStore};
pub use value::{AtomKey, ExternalAddress, ExternalTypeId, Multifield, Value};
