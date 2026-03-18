//! Engine serialization and deserialization.
//!
//! Provides [`Engine::serialize`] and [`Engine::deserialize`] for converting a
//! fully-loaded engine to/from bytes in one of several formats. This enables
//! workflows where a canonical rule set is loaded and compiled once, serialized,
//! and then deserialized many times to create fresh ready-to-run engines —
//! skipping the parse/compile pipeline entirely.
//!
//! ## Supported formats
//!
//! | Format      | Crate          | Notes                                 |
//! |-------------|----------------|---------------------------------------|
//! | Bincode     | `bincode`      | Compact binary, fast (default)        |
//! | JSON        | `serde_json`   | Human-readable, larger output         |
//! | CBOR        | `ciborium`     | Concise Binary Object Representation  |
//! | `MessagePack` | `rmp-serde`    | Compact binary, JSON-like schema      |
//! | Postcard    | `postcard`     | Compact, `no_std`-friendly binary     |
//!
//! ## Limitations
//!
//! - `ExternalAddress` values cannot be serialized. If any are present in the
//!   fact base, [`Engine::serialize`] returns
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

/// Supported serialization formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SerializationFormat {
    /// Compact binary format via `bincode`. Fast and small.
    Bincode,
    /// JSON via `serde_json`. Human-readable, larger output.
    /// Note: JSON does not support `NaN` or `Infinity` float values.
    Json,
    /// CBOR (Concise Binary Object Representation) via `ciborium`.
    Cbor,
    /// `MessagePack` via `rmp-serde`. Compact binary with JSON-like schema.
    MessagePack,
    /// Postcard — compact, `no_std`-friendly binary format.
    Postcard,
}

impl SerializationFormat {
    /// Returns a human-readable name for this format.
    pub fn name(self) -> &'static str {
        match self {
            Self::Bincode => "bincode",
            Self::Json => "json",
            Self::Cbor => "cbor",
            Self::MessagePack => "msgpack",
            Self::Postcard => "postcard",
        }
    }

    /// All supported formats, in declaration order.
    pub const ALL: &'static [SerializationFormat] = &[
        Self::Bincode,
        Self::Json,
        Self::Cbor,
        Self::MessagePack,
        Self::Postcard,
    ];
}

/// Errors from serialization and deserialization.
#[derive(Debug, thiserror::Error)]
pub enum SerializationError {
    #[error("engine contains ExternalAddress values which cannot be serialized")]
    ExternalAddressPresent,

    #[error("serialization failed: {0}")]
    Encode(String),

    #[error("deserialization failed: {0}")]
    Decode(String),
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
    /// Serialize this engine to bytes in the given format.
    ///
    /// The returned bytes can be passed to [`Engine::deserialize`] (with the
    /// same format) to reconstruct an equivalent engine.
    ///
    /// # Errors
    ///
    /// Returns [`SerializationError::ExternalAddressPresent`] if the engine
    /// contains any `ExternalAddress` values (which cannot be serialized).
    pub fn serialize(&self, format: SerializationFormat) -> Result<Vec<u8>, SerializationError> {
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

        encode(&snapshot, format)
    }

    /// Deserialize an engine from bytes previously produced by
    /// [`Engine::serialize`] with the same format.
    ///
    /// The returned engine is ready for [`Engine::run`]. Its thread affinity
    /// is set to the calling thread.
    ///
    /// # Errors
    ///
    /// Returns an error if the data is malformed or does not match the
    /// expected format.
    pub fn deserialize(
        data: &[u8],
        format: SerializationFormat,
    ) -> Result<Self, SerializationError> {
        let snapshot: EngineSnapshotOwned = decode(data, format)?;
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

/// Encode a snapshot to bytes in the given format.
fn encode<T: serde::Serialize>(
    value: &T,
    format: SerializationFormat,
) -> Result<Vec<u8>, SerializationError> {
    match format {
        SerializationFormat::Bincode => {
            bincode::serialize(value).map_err(|e| SerializationError::Encode(e.to_string()))
        }
        SerializationFormat::Json => {
            serde_json::to_vec(value).map_err(|e| SerializationError::Encode(e.to_string()))
        }
        SerializationFormat::Cbor => {
            let mut buf = Vec::new();
            ciborium::ser::into_writer(value, &mut buf)
                .map_err(|e| SerializationError::Encode(e.to_string()))?;
            Ok(buf)
        }
        SerializationFormat::MessagePack => {
            rmp_serde::to_vec(value).map_err(|e| SerializationError::Encode(e.to_string()))
        }
        SerializationFormat::Postcard => {
            postcard::to_allocvec(value).map_err(|e| SerializationError::Encode(e.to_string()))
        }
    }
}

/// Decode a snapshot from bytes in the given format.
fn decode<T: serde::de::DeserializeOwned>(
    data: &[u8],
    format: SerializationFormat,
) -> Result<T, SerializationError> {
    match format {
        SerializationFormat::Bincode => {
            bincode::deserialize(data).map_err(|e| SerializationError::Decode(e.to_string()))
        }
        SerializationFormat::Json => {
            serde_json::from_slice(data).map_err(|e| SerializationError::Decode(e.to_string()))
        }
        SerializationFormat::Cbor => {
            ciborium::de::from_reader(data).map_err(|e| SerializationError::Decode(e.to_string()))
        }
        SerializationFormat::MessagePack => {
            rmp_serde::from_slice(data).map_err(|e| SerializationError::Decode(e.to_string()))
        }
        SerializationFormat::Postcard => {
            postcard::from_bytes(data).map_err(|e| SerializationError::Decode(e.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EngineConfig;
    use crate::execution::RunLimit;

    /// Test roundtrip for a given format with an empty engine.
    fn roundtrip_empty(format: SerializationFormat) {
        let engine = Engine::new(EngineConfig::default());
        let bytes = engine.serialize(format).unwrap();
        assert!(
            !bytes.is_empty(),
            "serialized {format:?} should be non-empty"
        );

        let engine2 = Engine::deserialize(&bytes, format).unwrap();
        assert_eq!(
            engine.facts().unwrap().count(),
            engine2.facts().unwrap().count()
        );
    }

    /// Test roundtrip for a given format with rules and facts.
    fn roundtrip_with_rules(format: SerializationFormat) {
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

        let bytes = engine.serialize(format).unwrap();
        let mut engine2 = Engine::deserialize(&bytes, format).unwrap();

        let result1 = engine.run(RunLimit::Unlimited).unwrap();
        let result2 = engine2.run(RunLimit::Unlimited).unwrap();

        assert_eq!(result1.rules_fired, result2.rules_fired);
        assert_eq!(result1.rules_fired, 2);
    }

    /// Test roundtrip for a given format with globals and functions.
    fn roundtrip_with_globals(format: SerializationFormat) {
        let mut engine = Engine::new(EngineConfig::default());
        engine
            .load_str(
                r"
                (defglobal ?*counter* = 0)
                (deffunction increment (?x) (+ ?x 1))
                (defrule count-up
                    (trigger)
                    =>
                    (bind ?*counter* (increment ?*counter*)))
            ",
            )
            .unwrap();
        engine.reset().unwrap();

        let bytes = engine.serialize(format).unwrap();
        let mut engine2 = Engine::deserialize(&bytes, format).unwrap();

        engine2.load_str("(assert (trigger))").unwrap();
        let result = engine2.run(RunLimit::Unlimited).unwrap();
        assert_eq!(result.rules_fired, 1);
    }

    /// Test that an engine with multiple modules roundtrips correctly.
    fn roundtrip_modules(format: SerializationFormat) {
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

        let bytes = engine.serialize(format).unwrap();
        let engine2 = Engine::deserialize(&bytes, format).unwrap();
        assert!(engine2.check_thread_affinity().is_ok());
    }

    // ── Per-format tests ─────────────────────────────────────────────────

    macro_rules! format_tests {
        ($format:ident, $mod_name:ident) => {
            mod $mod_name {
                use super::*;

                #[test]
                fn roundtrip_empty_engine() {
                    roundtrip_empty(SerializationFormat::$format);
                }

                #[test]
                fn roundtrip_with_rules_and_facts() {
                    roundtrip_with_rules(SerializationFormat::$format);
                }

                #[test]
                fn roundtrip_with_globals_and_functions() {
                    roundtrip_with_globals(SerializationFormat::$format);
                }

                #[test]
                fn roundtrip_preserves_multiple_modules() {
                    roundtrip_modules(SerializationFormat::$format);
                }
            }
        };
    }

    format_tests!(Bincode, bincode_tests);
    format_tests!(Json, json_tests);
    format_tests!(Cbor, cbor_tests);
    format_tests!(MessagePack, msgpack_tests);
    format_tests!(Postcard, postcard_tests);

    // ── Cross-format and error tests ─────────────────────────────────────

    #[test]
    fn reject_wrong_format() {
        let engine = Engine::new(EngineConfig::default());
        let bincode_bytes = engine.serialize(SerializationFormat::Bincode).unwrap();

        // Trying to decode bincode data as JSON should fail.
        let result = Engine::deserialize(&bincode_bytes, SerializationFormat::Json);
        assert!(result.is_err());
    }

    #[test]
    fn reject_corrupt_data() {
        for &format in SerializationFormat::ALL {
            let result = Engine::deserialize(b"not valid data at all", format);
            assert!(
                result.is_err(),
                "format {format:?} should reject corrupt data"
            );
        }
    }

    #[test]
    fn reject_empty_data() {
        for &format in SerializationFormat::ALL {
            let result = Engine::deserialize(b"", format);
            assert!(
                result.is_err(),
                "format {format:?} should reject empty data"
            );
        }
    }

    #[test]
    fn deserialized_engine_has_current_thread_affinity() {
        let engine = Engine::new(EngineConfig::default());
        let bytes = engine.serialize(SerializationFormat::Bincode).unwrap();
        let engine2 = Engine::deserialize(&bytes, SerializationFormat::Bincode).unwrap();
        assert!(engine2.check_thread_affinity().is_ok());
    }

    #[test]
    fn format_name() {
        assert_eq!(SerializationFormat::Bincode.name(), "bincode");
        assert_eq!(SerializationFormat::Json.name(), "json");
        assert_eq!(SerializationFormat::Cbor.name(), "cbor");
        assert_eq!(SerializationFormat::MessagePack.name(), "msgpack");
        assert_eq!(SerializationFormat::Postcard.name(), "postcard");
    }

    #[test]
    fn all_formats_list() {
        assert_eq!(SerializationFormat::ALL.len(), 5);
    }

    /// Regression: asserting a template fact (via `load_str`) into a
    /// deserialized engine must propagate through the Rete network.
    #[test]
    fn assert_template_after_deserialize_fires_rule() {
        let source = r#"
(deftemplate sensor (slot id (type INTEGER)) (slot value (type FLOAT)))
(defrule alert
    (sensor (id ?id) (value ?v&:(> ?v 100.0)))
    =>
    (printout t "ALERT " ?id crlf))
"#;
        for &format in SerializationFormat::ALL {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(source).unwrap();
            engine.reset().unwrap();

            let bytes = engine.serialize(format).unwrap();
            let mut restored = Engine::deserialize(&bytes, format).unwrap();

            restored
                .load_str("(assert (sensor (id 7) (value 200.0)))")
                .unwrap();
            let result = restored.run(RunLimit::Unlimited).unwrap();
            assert_eq!(
                result.rules_fired, 1,
                "format {format:?}: expected 1 rule to fire"
            );
        }
    }
}
