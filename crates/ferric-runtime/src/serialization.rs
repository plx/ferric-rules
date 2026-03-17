//! Engine serialization and deserialization.
//!
//! Provides [`Engine::serialize_to_bytes`] and [`Engine::deserialize_from_bytes`]
//! for converting a fully-loaded engine to/from a compact binary representation.
//! This enables workflows where a canonical rule set is loaded and compiled once,
//! serialized, and then deserialized many times to create fresh ready-to-run
//! engines — skipping the parse/compile pipeline entirely.
//!
//! ## Wire format
//!
//! ```text
//! [4 bytes] Magic number: b"FRSE"
//! [4 bytes] Format version: u32 = 2  (little-endian)
//! [rest]    bincode-encoded EngineSnapshot
//! ```
//!
//! ## Limitations
//!
//! - `ExternalAddress` values cannot be serialized. If any are present in the
//!   fact base, [`Engine::serialize_to_bytes`] returns
//!   [`SerializationError::ExternalAddressPresent`].

use std::collections::VecDeque;
use std::marker::PhantomData;
use std::rc::Rc;

use ferric_core::{Fact, FactBase, ReteCompiler, ReteNetwork, SymbolTable, TemplateId, Value};

use crate::actions::{ActionError, CompiledRuleInfo};
use crate::config::EngineConfig;
use crate::engine::{Engine, RuleIndex};
use crate::functions::{FunctionEnv, GenericRegistry, GlobalStore, ModuleNameMap};
use crate::modules::{ModuleId, ModuleRegistry};
use crate::router::OutputRouter;
use crate::templates::RegisteredTemplate;

/// Magic bytes at the start of the serialized format.
const MAGIC: [u8; 4] = *b"FRSE";

/// Current format version.
const FORMAT_VERSION: u32 = 2;

/// Header size in bytes.
const HEADER_SIZE: usize = 8;

/// Errors from serialization and deserialization.
#[derive(Debug, thiserror::Error)]
pub enum SerializationError {
    #[error("engine contains ExternalAddress values which cannot be serialized")]
    ExternalAddressPresent,

    #[error("data too short to contain a valid header")]
    InvalidHeader,

    #[error("invalid magic number (expected FRSE)")]
    InvalidMagic,

    #[error("unsupported format version {0} (expected {FORMAT_VERSION})")]
    UnsupportedVersion(u32),

    #[error("serialization failed: {0}")]
    Encode(#[source] bincode::Error),

    #[error("deserialization failed: {0}")]
    Decode(#[source] bincode::Error),
}

/// Borrowed snapshot of engine state — used for serialization (avoids cloning).
#[derive(serde::Serialize)]
struct EngineSnapshotRef<'a> {
    fact_base: &'a FactBase,
    symbol_table: &'a SymbolTable,
    config: &'a EngineConfig,
    rete: &'a ReteNetwork,
    compiler: &'a ReteCompiler,
    registered_deffacts: &'a Vec<Vec<Fact>>,
    rule_info: &'a RuleIndex<Rc<CompiledRuleInfo>>,
    #[serde(with = "ferric_core::serde_helpers::fx_hash_map")]
    template_ids: &'a rustc_hash::FxHashMap<Box<str>, TemplateId>,
    template_defs: &'a slotmap::SlotMap<TemplateId, RegisteredTemplate>,
    router: &'a OutputRouter,
    functions: &'a FunctionEnv,
    globals: &'a GlobalStore,
    registered_globals: &'a Vec<(ModuleId, String, Value)>,
    generics: &'a GenericRegistry,
    module_registry: &'a ModuleRegistry,
    rule_modules: &'a RuleIndex<ModuleId>,
    template_modules: &'a slotmap::SecondaryMap<TemplateId, ModuleId>,
    #[serde(with = "ferric_core::serde_helpers::fx_hash_map_of_fx_hash_map")]
    function_modules: &'a ModuleNameMap<ModuleId>,
    #[serde(with = "ferric_core::serde_helpers::fx_hash_map_of_fx_hash_map")]
    global_modules: &'a ModuleNameMap<ModuleId>,
    #[serde(with = "ferric_core::serde_helpers::fx_hash_map_of_fx_hash_map")]
    generic_modules: &'a ModuleNameMap<ModuleId>,
    initial_fact_id: &'a Option<ferric_core::FactId>,
    action_diagnostics: &'a Vec<ActionError>,
    halted: bool,
    input_buffer: &'a VecDeque<String>,
}

/// Owned snapshot of engine state — used for deserialization.
#[derive(serde::Deserialize)]
struct EngineSnapshotOwned {
    fact_base: FactBase,
    symbol_table: SymbolTable,
    config: EngineConfig,
    rete: ReteNetwork,
    compiler: ReteCompiler,
    registered_deffacts: Vec<Vec<Fact>>,
    rule_info: RuleIndex<Rc<CompiledRuleInfo>>,
    #[serde(with = "ferric_core::serde_helpers::fx_hash_map")]
    template_ids: rustc_hash::FxHashMap<Box<str>, TemplateId>,
    template_defs: slotmap::SlotMap<TemplateId, RegisteredTemplate>,
    router: OutputRouter,
    functions: FunctionEnv,
    globals: GlobalStore,
    registered_globals: Vec<(ModuleId, String, Value)>,
    generics: GenericRegistry,
    module_registry: ModuleRegistry,
    rule_modules: RuleIndex<ModuleId>,
    template_modules: slotmap::SecondaryMap<TemplateId, ModuleId>,
    #[serde(with = "ferric_core::serde_helpers::fx_hash_map_of_fx_hash_map")]
    function_modules: ModuleNameMap<ModuleId>,
    #[serde(with = "ferric_core::serde_helpers::fx_hash_map_of_fx_hash_map")]
    global_modules: ModuleNameMap<ModuleId>,
    #[serde(with = "ferric_core::serde_helpers::fx_hash_map_of_fx_hash_map")]
    generic_modules: ModuleNameMap<ModuleId>,
    initial_fact_id: Option<ferric_core::FactId>,
    action_diagnostics: Vec<ActionError>,
    halted: bool,
    input_buffer: VecDeque<String>,
}

impl EngineSnapshotOwned {
    fn into_engine(self) -> Engine {
        Engine {
            fact_base: self.fact_base,
            symbol_table: self.symbol_table,
            config: self.config,
            rete: self.rete,
            compiler: self.compiler,
            registered_deffacts: self.registered_deffacts,
            rule_info: self.rule_info,
            template_ids: self.template_ids,
            template_defs: self.template_defs,
            router: self.router,
            functions: self.functions,
            globals: self.globals,
            registered_globals: self.registered_globals,
            generics: self.generics,
            module_registry: self.module_registry,
            rule_modules: self.rule_modules,
            template_modules: self.template_modules,
            function_modules: self.function_modules,
            global_modules: self.global_modules,
            generic_modules: self.generic_modules,
            initial_fact_id: self.initial_fact_id,
            action_diagnostics: self.action_diagnostics,
            halted: self.halted,
            input_buffer: self.input_buffer,
            creator_thread: std::thread::current().id(),
            _not_send_sync: PhantomData,
        }
    }
}

/// Check whether any `Value` in a slice is an `ExternalAddress`.
fn values_contain_external_address(values: &[Value]) -> bool {
    values
        .iter()
        .any(|v| matches!(v, Value::ExternalAddress(_)))
}

impl Engine {
    /// Serialize this engine to bytes.
    ///
    /// The returned bytes include a format header and can be passed to
    /// [`Engine::deserialize_from_bytes`] to reconstruct an equivalent engine.
    ///
    /// # Errors
    ///
    /// Returns [`SerializationError::ExternalAddressPresent`] if the fact base
    /// contains any `ExternalAddress` values (which cannot be serialized).
    pub fn serialize_to_bytes(&self) -> Result<Vec<u8>, SerializationError> {
        self.validate_serializable()?;

        let snapshot = EngineSnapshotRef {
            fact_base: &self.fact_base,
            symbol_table: &self.symbol_table,
            config: &self.config,
            rete: &self.rete,
            compiler: &self.compiler,
            registered_deffacts: &self.registered_deffacts,
            rule_info: &self.rule_info,
            template_ids: &self.template_ids,
            template_defs: &self.template_defs,
            router: &self.router,
            functions: &self.functions,
            globals: &self.globals,
            registered_globals: &self.registered_globals,
            generics: &self.generics,
            module_registry: &self.module_registry,
            rule_modules: &self.rule_modules,
            template_modules: &self.template_modules,
            function_modules: &self.function_modules,
            global_modules: &self.global_modules,
            generic_modules: &self.generic_modules,
            initial_fact_id: &self.initial_fact_id,
            action_diagnostics: &self.action_diagnostics,
            halted: self.halted,
            input_buffer: &self.input_buffer,
        };

        let mut buf = Vec::with_capacity(4096);
        buf.extend_from_slice(&MAGIC);
        buf.extend_from_slice(&FORMAT_VERSION.to_le_bytes());

        bincode::serialize_into(&mut buf, &snapshot).map_err(SerializationError::Encode)?;

        Ok(buf)
    }

    /// Deserialize an engine from bytes previously produced by
    /// [`Engine::serialize_to_bytes`].
    ///
    /// The returned engine is ready for [`Engine::run`]. Its thread affinity
    /// is set to the calling thread.
    ///
    /// # Errors
    ///
    /// Returns an error if the data is too short, has an invalid magic number,
    /// uses an unsupported format version, or is otherwise corrupt.
    pub fn deserialize_from_bytes(data: &[u8]) -> Result<Self, SerializationError> {
        if data.len() < HEADER_SIZE {
            return Err(SerializationError::InvalidHeader);
        }

        if data[..4] != MAGIC {
            return Err(SerializationError::InvalidMagic);
        }

        let version = u32::from_le_bytes(data[4..8].try_into().expect("slice is exactly 4 bytes"));
        if version != FORMAT_VERSION {
            return Err(SerializationError::UnsupportedVersion(version));
        }

        let snapshot: EngineSnapshotOwned =
            bincode::deserialize(&data[HEADER_SIZE..]).map_err(SerializationError::Decode)?;

        Ok(snapshot.into_engine())
    }

    /// Pre-flight check: ensure no `ExternalAddress` values exist in the
    /// fact base, registered globals, or registered deffacts.
    fn validate_serializable(&self) -> Result<(), SerializationError> {
        // Check fact base
        for (_id, entry) in self.fact_base.iter() {
            let has_external = match &entry.fact {
                Fact::Ordered(of) => values_contain_external_address(&of.fields),
                Fact::Template(tf) => values_contain_external_address(&tf.slots),
            };
            if has_external {
                return Err(SerializationError::ExternalAddressPresent);
            }
        }

        // Check registered globals
        for (_module, _name, value) in &self.registered_globals {
            if matches!(value, Value::ExternalAddress(_)) {
                return Err(SerializationError::ExternalAddressPresent);
            }
        }

        // Check registered deffacts
        for deffacts in &self.registered_deffacts {
            for fact in deffacts {
                let has_external = match fact {
                    Fact::Ordered(of) => values_contain_external_address(&of.fields),
                    Fact::Template(tf) => values_contain_external_address(&tf.slots),
                };
                if has_external {
                    return Err(SerializationError::ExternalAddressPresent);
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EngineConfig;
    use crate::execution::RunLimit;

    #[test]
    fn roundtrip_empty_engine() {
        let engine = Engine::new(EngineConfig::default());
        let bytes = engine.serialize_to_bytes().unwrap();

        assert_eq!(&bytes[..4], b"FRSE");
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 1);

        let engine2 = Engine::deserialize_from_bytes(&bytes).unwrap();
        // Both empty engines should have no user-visible facts
        assert_eq!(
            engine.facts().unwrap().count(),
            engine2.facts().unwrap().count()
        );
    }

    #[test]
    fn roundtrip_with_rules_and_facts() {
        let mut engine = Engine::new(EngineConfig::default());
        engine
            .load_str(
                r#"
                (deftemplate person (slot name) (slot age))
                (defrule greet
                    (person (name ?n))
                    =>
                    (printout t "Hello " ?n crlf))
                (deffacts people
                    (person (name "Alice") (age 30))
                    (person (name "Bob") (age 25)))
            "#,
            )
            .unwrap();
        engine.reset().unwrap();

        let bytes = engine.serialize_to_bytes().unwrap();
        let mut engine2 = Engine::deserialize_from_bytes(&bytes).unwrap();

        // Run both engines and compare output
        let result1 = engine.run(RunLimit::Unlimited).unwrap();
        let result2 = engine2.run(RunLimit::Unlimited).unwrap();

        assert_eq!(result1.rules_fired, result2.rules_fired);
        assert_eq!(result1.rules_fired, 2);
    }

    #[test]
    fn roundtrip_with_globals_and_functions() {
        let mut engine = Engine::new(EngineConfig::default());
        engine
            .load_str(
                r#"
                (defglobal ?*counter* = 0)
                (deffunction increment (?x) (+ ?x 1))
                (defrule count-up
                    (trigger)
                    =>
                    (bind ?*counter* (increment ?*counter*)))
            "#,
            )
            .unwrap();
        engine.reset().unwrap();

        let bytes = engine.serialize_to_bytes().unwrap();
        let mut engine2 = Engine::deserialize_from_bytes(&bytes).unwrap();

        // Load a trigger fact and run
        engine2.load_str("(assert (trigger))").unwrap();
        let result = engine2.run(RunLimit::Unlimited).unwrap();
        assert_eq!(result.rules_fired, 1);
    }

    #[test]
    fn reject_invalid_magic() {
        let result = Engine::deserialize_from_bytes(b"BADDxxxxxxxx");
        assert!(matches!(result, Err(SerializationError::InvalidMagic)));
    }

    #[test]
    fn reject_unsupported_version() {
        let mut data = Vec::new();
        data.extend_from_slice(b"FRSE");
        data.extend_from_slice(&99u32.to_le_bytes());
        data.extend_from_slice(b"some data");

        let result = Engine::deserialize_from_bytes(&data);
        assert!(matches!(
            result,
            Err(SerializationError::UnsupportedVersion(99))
        ));
    }

    #[test]
    fn reject_truncated_data() {
        let result = Engine::deserialize_from_bytes(b"FRS");
        assert!(matches!(result, Err(SerializationError::InvalidHeader)));
    }

    #[test]
    fn reject_corrupt_payload() {
        let mut data = Vec::new();
        data.extend_from_slice(b"FRSE");
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(b"not valid bincode data at all");

        let result = Engine::deserialize_from_bytes(&data);
        assert!(matches!(result, Err(SerializationError::Decode(_))));
    }

    #[test]
    fn deserialized_engine_has_current_thread_affinity() {
        let engine = Engine::new(EngineConfig::default());
        let bytes = engine.serialize_to_bytes().unwrap();
        let engine2 = Engine::deserialize_from_bytes(&bytes).unwrap();

        // Should not return WrongThread error
        assert!(engine2.check_thread_affinity().is_ok());
    }

    #[test]
    fn roundtrip_preserves_multiple_modules() {
        let mut engine = Engine::new(EngineConfig::default());
        engine
            .load_str(
                r#"
                (defmodule A (export ?ALL))
                (defrule A::rule-a (fact-a) => (printout t "A fired" crlf))
                (defmodule B (import A ?ALL))
                (defrule B::rule-b (fact-b) => (printout t "B fired" crlf))
            "#,
            )
            .unwrap();
        engine.reset().unwrap();

        let bytes = engine.serialize_to_bytes().unwrap();
        let engine2 = Engine::deserialize_from_bytes(&bytes).unwrap();

        // Verify the deserialized engine has the modules
        assert!(engine2.check_thread_affinity().is_ok());
    }
}
