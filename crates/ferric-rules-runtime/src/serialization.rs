//! Engine serialization and deserialization.
//!
//! Provides [`Engine::serialize`] and [`Engine::deserialize`] for converting a
//! fully loaded engine to/from bytes in one of several formats. This enables
//! workflows where a canonical rule set is loaded and compiled once, serialized,
//! and then deserialized many times to create fresh ready-to-run engines —
//! skipping the parse/compile pipeline entirely.
//!
//! ## Supported formats
//!
//! | Format | Crate | Status |
//! | --- | --- | --- |
//! | CBOR | `ciborium` | Recommended persistence format |
//! | Bincode | `bincode` | Experimental compact binary |
//! | JSON | `serde_json` | Experimental human-readable payload |
//! | `MessagePack` | `rmp-serde` | Experimental compact binary |
//! | Postcard | `postcard` | Experimental compact binary |
//!
//! Every codec payload is wrapped in the same versioned binary envelope.
//!
//! ## Limitations
//!
//! - `ExternalAddress` values cannot be serialized. If any are present in the
//!   engine state, [`Engine::serialize`] rejects the snapshot, including
//!   nested external identities in facts and globals, with
//!   [`SerializationError::ExternalAddressPresent`].

mod limited;
mod validation;

use bincode::Options;
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::sync::Arc;

use ferric_rules_core::{
    Fact, FactBase, ReteCompiler, ReteNetwork, SymbolTable, TemplateId, Value,
};

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
    /// Experimental compact binary format via `bincode`.
    Bincode,
    /// JSON via `serde_json`. Human-readable, larger output.
    /// Note: JSON does not support `NaN` or `Infinity` float values.
    Json,
    /// Recommended CBOR persistence format via `ciborium`.
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

    /// Recommended persistence format for new applications.
    pub const RECOMMENDED: Self = Self::Cbor;

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

    #[error("legacy raw snapshots are unsupported; use the producing Ferric version to export application data")]
    LegacySnapshot,

    #[error("unsupported snapshot schema version {0}; this build supports version 3")]
    UnsupportedVersion(u16),

    #[error("snapshot format does not match requested {0}")]
    WrongFormat(&'static str),

    #[error("snapshot has unsupported capability flags {0:#x}")]
    UnsupportedCapabilities(u8),

    #[error("snapshot exceeds the {0} limit")]
    LimitExceeded(&'static str),

    #[error("snapshot integrity checksum does not match")]
    ChecksumMismatch,

    #[error("invalid restored engine state: {0}")]
    InvalidState(String),

    #[error("serialization failed: {0}")]
    Encode(String),

    #[error("deserialization failed: {0}")]
    Decode(String),
}

/// A snapshot file could not be read or did not contain a valid snapshot.
#[derive(Debug, thiserror::Error)]
pub enum SnapshotFileError {
    #[error("snapshot file I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Serialization(#[from] SerializationError),
}

/// Maximum complete snapshot size (16 MiB), checked before codec work.
pub const MAX_SNAPSHOT_BYTES: usize = 16 * 1024 * 1024;
const MAGIC: &[u8; 8] = b"FERRIC\0S";
const HEADER_LEN: usize = 52;
const SCHEMA_VERSION: u16 = 3;

fn format_id(format: SerializationFormat) -> u8 {
    match format {
        SerializationFormat::Bincode => 0,
        SerializationFormat::Json => 1,
        SerializationFormat::Cbor => 2,
        SerializationFormat::MessagePack => 3,
        SerializationFormat::Postcard => 4,
    }
}

fn envelope(payload: Vec<u8>, format: SerializationFormat) -> Result<Vec<u8>, SerializationError> {
    if payload.len() > MAX_SNAPSHOT_BYTES - HEADER_LEN {
        return Err(SerializationError::LimitExceeded("16 MiB byte"));
    }
    let mut bytes = Vec::with_capacity(HEADER_LEN + payload.len());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&SCHEMA_VERSION.to_le_bytes());
    bytes.push(format_id(format));
    bytes.push(0); // No optional capabilities in schema 3.
    bytes.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    let mut checksum = Sha256::new();
    checksum.update(&bytes);
    checksum.update(&payload);
    bytes.extend_from_slice(&checksum.finalize());
    bytes.extend(payload);
    Ok(bytes)
}

fn open_envelope(data: &[u8], format: SerializationFormat) -> Result<&[u8], SerializationError> {
    if data.len() > MAX_SNAPSHOT_BYTES {
        return Err(SerializationError::LimitExceeded("16 MiB byte"));
    }
    if !data.starts_with(MAGIC) {
        return Err(SerializationError::LegacySnapshot);
    }
    if data.len() < HEADER_LEN {
        return Err(SerializationError::Decode(
            "truncated snapshot header".to_owned(),
        ));
    }
    let version = u16::from_le_bytes([data[8], data[9]]);
    if version != SCHEMA_VERSION {
        return Err(SerializationError::UnsupportedVersion(version));
    }
    if data[10] != format_id(format) {
        return Err(SerializationError::WrongFormat(format.name()));
    }
    if data[11] != 0 {
        return Err(SerializationError::UnsupportedCapabilities(data[11]));
    }
    let length = u64::from_le_bytes(data[12..20].try_into().expect("fixed header slice"));
    if length != (data.len() - HEADER_LEN) as u64 {
        return Err(SerializationError::Decode(
            "snapshot payload length mismatch".to_owned(),
        ));
    }
    let mut checksum = Sha256::new();
    checksum.update(&data[..20]);
    checksum.update(&data[HEADER_LEN..]);
    if checksum.finalize().as_slice() != &data[20..HEADER_LEN] {
        return Err(SerializationError::ChecksumMismatch);
    }
    Ok(&data[HEADER_LEN..])
}

/// Borrowed snapshot of engine state — used for serialization (avoids cloning).
#[derive(serde::Serialize)]
struct EngineSnapshotRef<'a> {
    fact_base: &'a FactBase,
    symbol_table: &'a SymbolTable,
    config: &'a EngineConfig,
    rete: &'a ReteNetwork,
    compiler: &'a ReteCompiler,
    registered_deffacts: &'a Vec<crate::engine::RegisteredDeffacts>,
    rule_info: &'a RuleIndex<Arc<CompiledRuleInfo>>,
    #[serde(with = "ferric_rules_core::serde_helpers::fx_hash_map")]
    template_ids: &'a rustc_hash::FxHashMap<Box<str>, TemplateId>,
    template_defs: &'a slotmap::SlotMap<TemplateId, Arc<RegisteredTemplate>>,
    router: &'a OutputRouter,
    functions: &'a FunctionEnv,
    globals: &'a GlobalStore,
    registered_globals: &'a Vec<(ModuleId, String, Value)>,
    generics: &'a GenericRegistry,
    module_registry: &'a ModuleRegistry,
    rule_modules: &'a RuleIndex<ModuleId>,
    template_modules: &'a slotmap::SecondaryMap<TemplateId, ModuleId>,
    #[serde(with = "ferric_rules_core::serde_helpers::fx_hash_map_of_fx_hash_map")]
    function_modules: &'a ModuleNameMap<ModuleId>,
    #[serde(with = "ferric_rules_core::serde_helpers::fx_hash_map_of_fx_hash_map")]
    global_modules: &'a ModuleNameMap<ModuleId>,
    #[serde(with = "ferric_rules_core::serde_helpers::fx_hash_map_of_fx_hash_map")]
    generic_modules: &'a ModuleNameMap<ModuleId>,
    initial_fact_id: &'a Option<ferric_rules_core::FactId>,
    action_diagnostics: &'a Vec<ActionError>,
    halted: bool,
    input_buffer: &'a VecDeque<String>,
}

/// Owned snapshot of engine state — used for deserialization.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct EngineSnapshotOwned {
    fact_base: FactBase,
    symbol_table: SymbolTable,
    config: EngineConfig,
    rete: ReteNetwork,
    compiler: ReteCompiler,
    registered_deffacts: Vec<crate::engine::RegisteredDeffacts>,
    rule_info: RuleIndex<Arc<CompiledRuleInfo>>,
    #[serde(with = "ferric_rules_core::serde_helpers::fx_hash_map")]
    template_ids: rustc_hash::FxHashMap<Box<str>, TemplateId>,
    template_defs: slotmap::SlotMap<TemplateId, Arc<RegisteredTemplate>>,
    router: OutputRouter,
    functions: FunctionEnv,
    globals: GlobalStore,
    registered_globals: Vec<(ModuleId, String, Value)>,
    generics: GenericRegistry,
    module_registry: ModuleRegistry,
    rule_modules: RuleIndex<ModuleId>,
    template_modules: slotmap::SecondaryMap<TemplateId, ModuleId>,
    #[serde(with = "ferric_rules_core::serde_helpers::fx_hash_map_of_fx_hash_map")]
    function_modules: ModuleNameMap<ModuleId>,
    #[serde(with = "ferric_rules_core::serde_helpers::fx_hash_map_of_fx_hash_map")]
    global_modules: ModuleNameMap<ModuleId>,
    #[serde(with = "ferric_rules_core::serde_helpers::fx_hash_map_of_fx_hash_map")]
    generic_modules: ModuleNameMap<ModuleId>,
    initial_fact_id: Option<ferric_rules_core::FactId>,
    action_diagnostics: Vec<ActionError>,
    halted: bool,
    input_buffer: VecDeque<String>,
}

impl EngineSnapshotOwned {
    fn into_engine(self) -> Engine {
        Engine {
            fact_base: self.fact_base,
            host: crate::host::HostState::new(),
            symbol_table: self.symbol_table,
            config: self.config,
            rete: self.rete,
            compiler: self.compiler,
            registered_deffacts: self.registered_deffacts,
            rule_info: self.rule_info,
            template_ids: self.template_ids,
            template_local_ids: Engine::build_template_local_index(&self.template_defs),
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
            processing_predicates: false,
            halted: self.halted,
            input_buffer: self.input_buffer,
        }
    }
}

/// Check values including nested multifields without recursing on the stack.
fn values_contain_external_address(mut values: &[Value]) -> bool {
    let mut pending = Vec::new();
    loop {
        for value in values {
            match value {
                Value::ExternalAddress(_) => return true,
                Value::Multifield(fields) => pending.push(fields.as_slice()),
                _ => {}
            }
        }
        let Some(nested) = pending.pop() else {
            return false;
        };
        values = nested;
    }
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
    /// Invalid engine invariants, persistence limits, and unsupported codec
    /// values also produce errors. Successful writes satisfy the read limits.
    pub fn serialize(&self, format: SerializationFormat) -> Result<Vec<u8>, SerializationError> {
        self.validate_serializable()?;
        self.validate_restored_state()?;

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

        let payload = encode(&snapshot, format)?;
        // Apply the same wire limits to writes: never return bytes that this
        // build cannot read, including JSON's inability to represent NaN.
        let _: EngineSnapshotOwned = decode(&payload, format)
            .map_err(|error| SerializationError::Encode(error.to_string()))?;
        envelope(payload, format)
    }

    /// Deserialize an engine from bytes previously produced by
    /// [`Engine::serialize`] with the same format.
    ///
    /// The returned engine is ready for [`Engine::run`]. Ownership may be moved
    /// to another thread before further use or destruction.
    ///
    /// # Errors
    ///
    /// Returns an error for legacy raw data, unknown versions or capabilities,
    /// a mismatched codec, corruption, exceeded persistence limits, or invalid
    /// restored state. A failed decode never installs a partial engine.
    pub fn deserialize(
        data: &[u8],
        format: SerializationFormat,
    ) -> Result<Self, SerializationError> {
        let payload = open_envelope(data, format)?;
        let snapshot: EngineSnapshotOwned = decode(payload, format)?;
        let engine = snapshot.into_engine();
        engine.validate_restored_state()?;
        Ok(engine)
    }

    /// Restore a snapshot without reading more than the supported byte limit.
    ///
    /// File reads stop after [`MAX_SNAPSHOT_BYTES`] plus one byte, including for
    /// files whose size is unavailable or changes while being read. The same
    /// envelope, codec, and restored-state checks as [`Self::deserialize`] apply.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotFileError::Io`] for file read failures, or
    /// [`SnapshotFileError::Serialization`] for oversized or invalid snapshots.
    pub fn deserialize_from_file(
        path: &std::path::Path,
        format: SerializationFormat,
    ) -> Result<Self, SnapshotFileError> {
        use std::io::Read;

        let file = std::fs::File::open(path)?;
        let mut bytes = Vec::new();
        file.take((MAX_SNAPSHOT_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        Ok(Self::deserialize(&bytes, format)?)
    }

    /// Pre-flight check: ensure no `ExternalAddress` values exist in the
    /// fact base, registered globals, or registered deffacts.
    fn validate_serializable(&self) -> Result<(), SerializationError> {
        for values in self.globals.values.values() {
            for value in values.values() {
                if values_contain_external_address(std::slice::from_ref(value)) {
                    return Err(SerializationError::ExternalAddressPresent);
                }
            }
        }
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
            if values_contain_external_address(std::slice::from_ref(value)) {
                return Err(SerializationError::ExternalAddressPresent);
            }
        }

        // Check registered deffacts
        for deffacts in &self.registered_deffacts {
            for fact in &deffacts.facts {
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

#[derive(Default)]
struct BoundedWriter(Vec<u8>);

impl std::io::Write for BoundedWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > (MAX_SNAPSHOT_BYTES - HEADER_LEN).saturating_sub(self.0.len()) {
            return Err(std::io::Error::other(
                "snapshot exceeds the 16 MiB byte limit",
            ));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl postcard::ser_flavors::Flavor for BoundedWriter {
    type Output = Vec<u8>;
    fn try_push(&mut self, byte: u8) -> postcard::Result<()> {
        self.try_extend(&[byte])
    }
    fn try_extend(&mut self, bytes: &[u8]) -> postcard::Result<()> {
        std::io::Write::write_all(self, bytes).map_err(|_| postcard::Error::SerializeBufferFull)
    }
    fn finalize(self) -> postcard::Result<Self::Output> {
        Ok(self.0)
    }
}

/// Encode a snapshot to bytes in the given format.
fn encode<T: serde::Serialize>(
    value: &T,
    format: SerializationFormat,
) -> Result<Vec<u8>, SerializationError> {
    let mut writer = BoundedWriter::default();
    match format {
        SerializationFormat::Bincode => {
            bincode::serialize_into(&mut writer, value)
                .map_err(|e| SerializationError::Encode(e.to_string()))?;
        }
        SerializationFormat::Json => {
            serde_json::to_writer(&mut writer, value)
                .map_err(|e| SerializationError::Encode(e.to_string()))?;
        }
        SerializationFormat::Cbor => {
            ciborium::ser::into_writer(value, &mut writer)
                .map_err(|e| SerializationError::Encode(e.to_string()))?;
        }
        SerializationFormat::MessagePack => {
            value
                .serialize(&mut rmp_serde::Serializer::new(&mut writer))
                .map_err(|e| SerializationError::Encode(e.to_string()))?;
        }
        SerializationFormat::Postcard => {
            return postcard::serialize_with_flavor::<T, BoundedWriter, Vec<u8>>(value, writer)
                .map_err(|e| SerializationError::Encode(e.to_string()));
        }
    }
    Ok(writer.0)
}

/// Decode a snapshot from bytes in the given format.
fn decode<T: serde::de::DeserializeOwned>(
    data: &[u8],
    format: SerializationFormat,
) -> Result<T, SerializationError> {
    match format {
        SerializationFormat::Bincode => bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_limit(MAX_SNAPSHOT_BYTES as u64)
            .reject_trailing_bytes()
            .deserialize::<limited::Limited<T>>(data)
            .map(|value| value.0)
            .map_err(|e| SerializationError::Decode(e.to_string())),
        SerializationFormat::Json => serde_json::from_slice::<limited::Limited<T>>(data)
            .map(|value| value.0)
            .map_err(|e| SerializationError::Decode(e.to_string())),
        SerializationFormat::Cbor => {
            let mut reader = data;
            let value = ciborium::de::from_reader_with_recursion_limit::<limited::Limited<T>, _>(
                &mut reader,
                limited::MAX_DEPTH,
            )
            .map_err(|e| SerializationError::Decode(e.to_string()))?;
            if !reader.is_empty() {
                return Err(SerializationError::Decode("trailing CBOR data".to_owned()));
            }
            Ok(value.0)
        }
        SerializationFormat::MessagePack => {
            let mut decoder = rmp_serde::Deserializer::new(std::io::Cursor::new(data));
            decoder.set_max_depth(limited::MAX_DEPTH);
            let value: limited::Limited<T> = serde::Deserialize::deserialize(&mut decoder)
                .map_err(|e| SerializationError::Decode(e.to_string()))?;
            if decoder.position() != data.len() as u64 {
                return Err(SerializationError::Decode(
                    "trailing MessagePack data".to_owned(),
                ));
            }
            Ok(value.0)
        }
        SerializationFormat::Postcard => {
            let (value, remaining) = postcard::take_from_bytes::<limited::Limited<T>>(data)
                .map_err(|e| SerializationError::Decode(e.to_string()))?;
            if !remaining.is_empty() {
                return Err(SerializationError::Decode(
                    "trailing Postcard data".to_owned(),
                ));
            }
            Ok(value.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EngineConfig;
    use crate::execution::RunLimit;

    fn alter_state(
        engine: &Engine,
        change: impl FnOnce(&mut serde_json::Value),
    ) -> Result<Engine, SerializationError> {
        let bytes = engine.serialize(SerializationFormat::Json).unwrap();
        let mut state: serde_json::Value = serde_json::from_slice(&bytes[HEADER_LEN..]).unwrap();
        change(&mut state);
        let bytes = envelope(
            serde_json::to_vec(&state).unwrap(),
            SerializationFormat::Json,
        )
        .unwrap();
        Engine::deserialize(&bytes, SerializationFormat::Json)
    }

    #[test]
    fn snapshot_rejects_nonroot_root_without_panicking() {
        let engine = Engine::with_rules("(defrule ready (ready) =>)").unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            alter_state(&engine, |state| {
                let nonroot = state["rete"]["beta"]["nodes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|entry| entry[1].get("Root").is_none())
                    .unwrap()[0]
                    .clone();
                state["rete"]["beta"]["root_id"] = nonroot;
            })
        }));
        assert!(
            result.is_ok(),
            "invalid root must return an error, not unwind"
        );
        assert!(matches!(
            result.unwrap(),
            Err(SerializationError::InvalidState(_))
        ));
    }

    #[test]
    fn blocked_negative_cannot_retain_output_token() {
        let mut engine =
            Engine::with_rules("(defrule choose (not (blocker)) => (assert (selected)))").unwrap();
        let bytes = engine.serialize(SerializationFormat::Json).unwrap();
        let mut unblocked: serde_json::Value =
            serde_json::from_slice(&bytes[HEADER_LEN..]).unwrap();
        engine
            .assert_ordered("blocker", Vec::<Value>::new())
            .unwrap();
        let bytes = engine.serialize(SerializationFormat::Json).unwrap();
        let blocked: serde_json::Value = serde_json::from_slice(&bytes[HEADER_LEN..]).unwrap();
        // Retain the earlier pass-through token/activation, but supply the real
        // blocker and its complete alpha/negative reverse memberships.
        unblocked["fact_base"] = blocked["fact_base"].clone();
        unblocked["symbol_table"] = blocked["symbol_table"].clone();
        unblocked["rete"]["alpha"] = blocked["rete"]["alpha"].clone();
        unblocked["rete"]["beta"]["neg_memories"] = blocked["rete"]["beta"]["neg_memories"].clone();
        let bytes = envelope(
            serde_json::to_vec(&unblocked).unwrap(),
            SerializationFormat::Json,
        )
        .unwrap();
        match Engine::deserialize(&bytes, SerializationFormat::Json) {
            Err(SerializationError::InvalidState(_)) => {}
            Ok(mut restored) => panic!(
                "blocked negative snapshot accepted; incorrect firings: {}",
                restored.run(RunLimit::Unlimited).unwrap().rules_fired
            ),
            Err(error) => panic!("expected invalid state error, got {error}"),
        }
    }

    #[test]
    fn compiler_allocator_must_match_runtime_rule_capacity() {
        for source in ["", "(defrule ready (ready) =>)"] {
            let engine = Engine::with_rules(source).unwrap();
            for next in [1000, u32::MAX - 1] {
                let result = alter_state(&engine, |state| {
                    state["compiler"]["next_rule_id"] = serde_json::json!(next);
                });
                assert!(matches!(result, Err(SerializationError::InvalidState(_))), "forged allocator {next} must not make the next load allocate a huge sparse index");
            }
        }
    }

    #[test]
    fn timestamp_exhaustion_is_fallible_and_remains_persistable() {
        let engine = Engine::with_rules("").unwrap();
        let mut restored = alter_state(&engine, |state| {
            state["fact_base"]["next_timestamp"] = serde_json::json!(u64::MAX - 1);
        })
        .unwrap();
        let last = restored
            .assert_ordered("last", Vec::<Value>::new())
            .unwrap();
        let failure = restored
            .assert_ordered("overflow", Vec::<Value>::new())
            .unwrap_err();
        assert!(matches!(
            failure,
            crate::EngineError::FactTimestampExhausted(_)
        ));
        assert!(failure.to_string().contains("reset the engine"));
        assert!(restored.find_facts("overflow").unwrap().is_empty());
        assert!(restored.get_fact(last).unwrap().is_some());
        // Existing duplicates require no new timestamp.
        restored.set_fact_duplication(false);
        assert_eq!(
            restored
                .assert_ordered("last", Vec::<Value>::new())
                .unwrap(),
            last
        );
        for &format in SerializationFormat::ALL {
            let bytes = restored.serialize(format).unwrap();
            let mut again = Engine::deserialize(&bytes, format).unwrap();
            assert!(again
                .assert_ordered("overflow", Vec::<Value>::new())
                .is_err());
            // Host handles belong to their restored engine; resolve afresh.
            let restored_last = again.find_facts("last").unwrap()[0].0;
            again.retract(restored_last).unwrap();
            again.reset().unwrap();
            again
                .assert_ordered("after-reset", Vec::<Value>::new())
                .unwrap();
        }
    }

    #[test]
    fn rhs_assertion_stops_at_timestamp_exhaustion_without_wrapping() {
        for (definition, first, second) in [
            ("", "(first 1)", "(second 2)"),
            (
                "(deftemplate item (slot value))",
                "(item (value 1))",
                "(item (value 2))",
            ),
        ] {
            let source = format!("{definition} (defrule create => (assert {first}) (assert {second}) (printout t after crlf))");
            let engine = Engine::with_rules(&source).unwrap();
            let mut restored = alter_state(&engine, |state| {
                state["fact_base"]["next_timestamp"] = serde_json::json!(u64::MAX - 1);
            })
            .unwrap();
            let before = restored.facts().unwrap().count();
            let run = restored.run(RunLimit::Unlimited).unwrap();
            assert_eq!(run.halt_reason, crate::HaltReason::ActionError);
            assert_eq!(restored.facts().unwrap().count(), before + 1);
            assert!(restored.action_diagnostics()[0]
                .to_string()
                .contains("timestamp capacity exhausted"));
            assert!(restored.get_output("t").map_or(true, str::is_empty));
            let bytes = restored.serialize(SerializationFormat::Cbor).unwrap();
            Engine::deserialize(&bytes, SerializationFormat::Cbor).unwrap();
        }
    }

    #[test]
    fn exhausted_modify_keeps_original_fact_and_stops_rhs() {
        for (definition, seed, pattern, replacement) in [
            ("", "(item 1)", "(item ?value)", ""),
            (
                "(deftemplate item (slot value))",
                "(item (value 1))",
                "(item (value ?value))",
                "(value 2)",
            ),
        ] {
            let source = format!("{definition} (deffacts seed {seed}) (defrule change ?fact <- {pattern} => (modify ?fact {replacement}) (assert (after)))");
            let engine = Engine::with_rules(&source).unwrap();
            let mut restored = alter_state(&engine, |state| {
                state["fact_base"]["next_timestamp"] = serde_json::json!(u64::MAX);
            })
            .unwrap();
            let before: Vec<_> = restored
                .facts()
                .unwrap()
                .map(|(id, fact)| (id, fact.clone()))
                .collect();
            let run = restored.run(RunLimit::Unlimited).unwrap();
            assert_eq!(run.halt_reason, crate::HaltReason::ActionError);
            assert_eq!(run.rules_fired, 1);
            assert!(restored.action_diagnostics()[0]
                .to_string()
                .contains("timestamp capacity exhausted"));
            let after: Vec<_> = restored.facts().unwrap().collect();
            assert_eq!(after.len(), before.len());
            for (id, fact) in before {
                assert!(restored.get_fact(id).unwrap().unwrap().structural_eq(&fact));
            }
            assert!(restored.find_facts("after").unwrap().is_empty());
            let bytes = restored.serialize(SerializationFormat::Cbor).unwrap();
            Engine::deserialize(&bytes, SerializationFormat::Cbor).unwrap();
        }
    }

    #[test]
    fn beta_node_exhaustion_preserves_old_rule_and_all_or_variants() {
        let engine = Engine::with_rules(
            "(deffacts seed (ready)) (defrule choose (ready) => (printout t old crlf))",
        )
        .unwrap();
        for (next, replacement) in [
            (
                u32::MAX - 1,
                "(defrule choose (different) => (printout t new crlf))",
            ),
            // Each arm fits separately; the complete replacement does not.
            (
                u32::MAX - 3,
                "(defrule choose (or (different) (another)) => (printout t new crlf))",
            ),
            (u32::MAX, "(defrule choose => (printout t new crlf))"),
            (
                u32::MAX - 4,
                "(defrule choose (not (and (different) (another))) => (printout t new crlf))",
            ),
        ] {
            let mut restored = alter_state(&engine, |state| {
                state["rete"]["beta"]["next_node_id"] = serde_json::json!(next);
            })
            .unwrap();
            let errors = restored.load_str(replacement).unwrap_err();
            assert!(
                errors
                    .iter()
                    .any(|error| error.to_string().contains("remaining beta node IDs")),
                "{errors:?}"
            );
            assert_eq!(restored.rules().len(), 1);
            assert_eq!(restored.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
            assert_eq!(restored.get_output("t"), Some("old\n"));
            let bytes = restored.serialize(SerializationFormat::Cbor).unwrap();
            Engine::deserialize(&bytes, SerializationFormat::Cbor).unwrap();
        }
        // The last ID can be allocated once; a subsequent load fails without
        // wrapping onto an existing node. Unconditional rules need one terminal.
        let mut restored = alter_state(&engine, |state| {
            state["rete"]["beta"]["next_node_id"] = serde_json::json!(u32::MAX - 1);
        })
        .unwrap();
        restored
            .load_str("(defrule final => (printout t final crlf))")
            .unwrap();
        assert!(restored.load_str("(defrule overflow =>)").is_err());
        assert_eq!(restored.run(RunLimit::Unlimited).unwrap().rules_fired, 2);
        let bytes = restored.serialize(SerializationFormat::Cbor).unwrap();
        Engine::deserialize(&bytes, SerializationFormat::Cbor).unwrap();
    }

    #[test]
    fn module_allocator_must_match_dense_registry() {
        let engine = Engine::with_rules("(defmodule A) (defmodule B) (defmodule A)").unwrap();
        for next in [1, 1000, u32::MAX - 1, u32::MAX] {
            let result = alter_state(&engine, |state| {
                state["module_registry"]["next_id"] = serde_json::json!(next);
            });
            assert!(matches!(result, Err(SerializationError::InvalidState(_))));
        }
        let bytes = engine.serialize(SerializationFormat::Cbor).unwrap();
        let mut restored = Engine::deserialize(&bytes, SerializationFormat::Cbor).unwrap();
        restored.load_str("(defmodule C)").unwrap();
    }

    #[test]
    fn versioned_envelope_rejects_unknown_corrupt_and_mismatched_inputs() {
        let engine = Engine::new(EngineConfig::default());
        for &format in SerializationFormat::ALL {
            let bytes = engine.serialize(format).unwrap();
            let mut changed = bytes.clone();
            changed[8..10].copy_from_slice(&(SCHEMA_VERSION + 1).to_le_bytes());
            assert!(matches!(
                Engine::deserialize(&changed, format),
                Err(SerializationError::UnsupportedVersion(version)) if version == SCHEMA_VERSION + 1
            ));
            changed = bytes.clone();
            changed[11] = 1;
            assert!(matches!(
                Engine::deserialize(&changed, format),
                Err(SerializationError::UnsupportedCapabilities(1))
            ));
            changed = bytes.clone();
            changed[10] = (changed[10] + 1) % 5;
            assert!(matches!(
                Engine::deserialize(&changed, format),
                Err(SerializationError::WrongFormat(_))
            ));
            changed = bytes.clone();
            *changed.last_mut().unwrap() ^= 1;
            assert!(matches!(
                Engine::deserialize(&changed, format),
                Err(SerializationError::ChecksumMismatch)
            ));
            changed = bytes.clone();
            changed.push(0);
            assert!(matches!(
                Engine::deserialize(&changed, format),
                Err(SerializationError::Decode(_))
            ));
            assert!(matches!(
                Engine::deserialize(&bytes[..HEADER_LEN - 1], format),
                Err(SerializationError::Decode(_))
            ));
            let mut payload = bytes[HEADER_LEN..].to_vec();
            payload.push(0);
            let changed = envelope(payload, format).unwrap();
            assert!(
                matches!(
                    Engine::deserialize(&changed, format),
                    Err(SerializationError::Decode(_))
                ),
                "codec must reject trailing data: {format:?}"
            );
        }
        assert!(matches!(
            Engine::deserialize(&vec![0; MAX_SNAPSHOT_BYTES + 1], SerializationFormat::Cbor),
            Err(SerializationError::LimitExceeded(_))
        ));
    }

    #[test]
    fn legacy_raw_fixture_has_an_explicit_rejection_path() {
        #[derive(serde::Deserialize)]
        struct LegacyData {
            fact_base: FactBase,
            symbol_table: SymbolTable,
        }
        let raw = include_bytes!("../tests/fixtures/snapshots/legacy-raw.cbor");
        // Verify this is a meaningful old payload, not arbitrary garbage.
        let legacy: LegacyData = decode(raw, SerializationFormat::Cbor).unwrap();
        assert_eq!(
            legacy
                .fact_base
                .iter()
                .filter(|(_, entry)| matches!(&entry.fact, Fact::Ordered(fact)
                if legacy.symbol_table.resolve_symbol_str(fact.relation) == Some("durable")))
                .count(),
            1
        );
        assert!(matches!(
            Engine::deserialize(raw, SerializationFormat::Cbor),
            Err(SerializationError::LegacySnapshot)
        ));
    }

    #[test]
    fn limits_apply_before_collection_allocation_and_recursive_decode() {
        // Hostile lengths with almost no payload must fail without reserving them.
        let mut cbor = vec![0x9b];
        cbor.extend(u64::MAX.to_be_bytes());
        assert!(decode::<Vec<Value>>(&cbor, SerializationFormat::Cbor)
            .unwrap_err()
            .to_string()
            .contains("limit"));
        assert!(
            decode::<Vec<Value>>(&u64::MAX.to_le_bytes(), SerializationFormat::Bincode)
                .unwrap_err()
                .to_string()
                .contains("limit")
        );
        let mut msgpack = vec![0xdd];
        msgpack.extend(u32::MAX.to_be_bytes());
        assert!(
            decode::<Vec<Value>>(&msgpack, SerializationFormat::MessagePack)
                .unwrap_err()
                .to_string()
                .contains("limit")
        );
        let postcard = postcard::to_allocvec(&(limited::MAX_ITEMS + 1)).unwrap();
        assert!(decode::<Vec<Value>>(&postcard, SerializationFormat::Postcard).is_err());
        for &format in SerializationFormat::ALL {
            let mut nested = Value::Integer(7);
            for _ in 0..8 {
                nested = Value::Multifield(Box::new(vec![nested].into_iter().collect()));
            }
            let valid = encode(&nested, format).unwrap();
            assert!(decode::<Value>(&valid, format)
                .unwrap()
                .structural_eq(&nested));
            for _ in 8..60 {
                nested = Value::Multifield(Box::new(vec![nested].into_iter().collect()));
            }
            let deep = encode(&nested, format).unwrap();
            let error = decode::<Value>(&deep, format)
                .unwrap_err()
                .to_string()
                .to_lowercase();
            assert!(
                format == SerializationFormat::Postcard || error.contains("limit"),
                "{format:?}: {error}"
            );
        }
        // The aggregate budget also applies when no collection advertises a size.
        let mut cbor = vec![0x9f];
        cbor.extend(std::iter::repeat_n(0xf6, limited::MAX_ITEMS + 1));
        cbor.push(0xff);
        assert!(decode::<Vec<()>>(&cbor, SerializationFormat::Cbor)
            .unwrap_err()
            .to_string()
            .contains("item limit"));
    }

    #[test]
    fn checksum_valid_but_inconsistent_graphs_are_rejected() {
        let mut engine = Engine::with_rules(
            "(defrule pick (item ?n) (not (block ?n ?reason)) => (assert (picked ?n)))",
        )
        .unwrap();
        engine
            .load_str("(assert (item 7) (block 7 a) (block 7 b))")
            .unwrap();
        for pointer in [
            "/fact_base/by_relation/utf8",
            "/rete/alpha/fact_to_memories",
            "/rete/token_store/fact_to_tokens",
        ] {
            let result = alter_state(&engine, |state| {
                *state.pointer_mut(pointer).unwrap() = if pointer.ends_with("fact_to_memories") {
                    serde_json::json!([{ "version": 0, "value": null }])
                } else {
                    serde_json::json!([])
                }
            });
            assert!(
                matches!(result, Err(SerializationError::InvalidState(_))),
                "{pointer}: {:?}",
                result.err()
            );
        }
        let result = alter_state(&engine, |state| {
            state["rete"]["beta"]["next_node_id"] = serde_json::json!(0);
        });
        assert!(matches!(result, Err(SerializationError::InvalidState(_))));
        let result = alter_state(&engine, |state| {
            state["compiler"]["join_node_cache"][0][1] = serde_json::json!(42);
        });
        assert!(matches!(result, Err(SerializationError::InvalidState(_))));
        // Remove one blocker and its reciprocal link. Local inverse checks still
        // pass, but semantic validation must reject the incomplete support set.
        let result = alter_state(&engine, |state| {
            let memory = &mut state["rete"]["beta"]["neg_memories"][0];
            let removed = memory["blocked"][0][1]
                .as_array_mut()
                .unwrap()
                .pop()
                .unwrap();
            memory["fact_to_blocked"]
                .as_array_mut()
                .unwrap()
                .retain(|entry| entry[0] != removed);
        });
        assert!(
            matches!(result, Err(SerializationError::InvalidState(message)) if message.contains("blocker"))
        );
    }

    #[test]
    fn resume_preserves_refraction_globals_output_and_negative_transitions() {
        for &format in SerializationFormat::ALL {
            let mut original = Engine::with_rules("(defglobal ?*count* = 0) (defrule pick (item ?n) (not (block ?n ?reason)) => (bind ?*count* (+ ?*count* 1)) (assert (picked ?n)) (printout t ?n crlf))").unwrap();
            original
                .load_str("(assert (item 7) (item 8) (block 7 a) (block 7 b))")
                .unwrap();
            assert_eq!(original.run(RunLimit::Count(1)).unwrap().rules_fired, 1);
            let bytes = original.serialize(format).unwrap();
            let mut resumed = Engine::deserialize(&bytes, format).unwrap();
            assert_eq!(resumed.get_output("t"), Some("8\n"));
            assert_eq!(
                resumed.run(RunLimit::Unlimited).unwrap().rules_fired,
                0,
                "a fired activation must not reappear after restore"
            );
            let blockers: Vec<_> = resumed
                .find_facts("block")
                .unwrap()
                .into_iter()
                .map(|(id, _)| id)
                .collect();
            resumed.retract(blockers[0]).unwrap();
            assert_eq!(resumed.run(RunLimit::Unlimited).unwrap().rules_fired, 0);
            resumed.retract(blockers[1]).unwrap();
            assert_eq!(resumed.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
            assert_eq!(resumed.get_output("t"), Some("8\n7\n"));
            assert_eq!(resumed.find_facts("picked").unwrap().len(), 2);
            assert!(matches!(
                resumed
                    .globals
                    .get(resumed.module_registry.main_module_id(), "count"),
                Some(Value::Integer(2))
            ));
            // Future compilation must use the retained, validated sharing cache.
            resumed
                .load_str("(defrule later (picked ?n) => (assert (observed ?n)))")
                .unwrap();
            assert_eq!(resumed.run(RunLimit::Unlimited).unwrap().rules_fired, 2);
            assert_eq!(resumed.find_facts("observed").unwrap().len(), 2);
        }
    }

    #[test]
    fn resume_preserves_exists_and_ncc_support_transitions() {
        for condition in [
            "(exists (support ?n ?reason))",
            "(not (and (support ?n ?reason) (confirmed ?reason)))",
        ] {
            let mut engine = Engine::with_rules(&format!(
                "(defrule choose (item ?n) {condition} => (assert (picked ?n)))"
            ))
            .unwrap();
            engine
                .load_str(
                    "(assert (item 7) (support 7 a) (support 7 b) (confirmed a) (confirmed b))",
                )
                .unwrap();
            let snapshot = engine.serialize(SerializationFormat::Cbor).unwrap();
            let mut restored = Engine::deserialize(&snapshot, SerializationFormat::Cbor).unwrap();
            let expected = usize::from(condition.starts_with("(exists"));
            assert_eq!(
                restored.run(RunLimit::Unlimited).unwrap().rules_fired,
                expected
            );
            let support: Vec<_> = restored
                .find_facts("support")
                .unwrap()
                .into_iter()
                .map(|(id, _)| id)
                .collect();
            restored.retract(support[0]).unwrap();
            assert_eq!(restored.run(RunLimit::Unlimited).unwrap().rules_fired, 0);
            restored.retract(support[1]).unwrap();
            assert_eq!(
                restored.run(RunLimit::Unlimited).unwrap().rules_fired,
                1 - expected
            );
            assert_eq!(restored.find_facts("picked").unwrap().len(), 1);
        }
    }

    #[test]
    fn snapshots_preserve_float_identity_including_nan_payloads() {
        let fields = vec![
            Value::Float(f64::from_bits(0x7ff8_0000_0000_1234)),
            Value::Float(-0.0),
            Value::Float(f64::INFINITY),
        ];
        let mut engine = Engine::new(EngineConfig::default());
        let id = engine
            .assert_ordered("measurement", fields.clone())
            .unwrap();
        assert!(
            matches!(
                engine.serialize(SerializationFormat::Json),
                Err(SerializationError::Encode(_))
            ),
            "experimental JSON must reject non-finite values explicitly"
        );
        for format in [
            SerializationFormat::Cbor,
            SerializationFormat::Bincode,
            SerializationFormat::MessagePack,
            SerializationFormat::Postcard,
        ] {
            let bytes = engine.serialize(format).unwrap();
            let restored = Engine::deserialize(&bytes, format).unwrap();
            assert!(
                restored.get_fact(id).unwrap().is_none(),
                "host handles do not survive restore"
            );
            let facts = restored.find_facts("measurement").unwrap();
            let Fact::Ordered(fact) = facts[0].1 else {
                panic!("ordered measurement");
            };
            assert!(
                fields
                    .iter()
                    .zip(&fact.fields)
                    .all(|(expected, actual)| expected.structural_eq(actual)),
                "float bits changed in {format:?}"
            );
        }
    }

    #[test]
    fn duplicate_persisted_entries_are_rejected_instead_of_overwritten() {
        let mut engine = Engine::with_rules(
            "(defrule pick (item ?n) (not (block ?n ?reason)) => (assert (picked ?n)))",
        )
        .unwrap();
        engine
            .load_str("(assert (item 7) (item 8) (block 7 a) (block 7 b))")
            .unwrap();
        for pointer in [
            "/compiler/join_node_cache",
            "/rete/beta/memories/0/tokens",
            "/rete/beta/neg_memories/0/blocked",
            "/rete/alpha/memories/1/slot_indices",
            "/rete/beta/memories/1/var_indices",
            "/rete/agenda/ordering",
        ] {
            let result = alter_state(&engine, |state| {
                let entries = state.pointer_mut(pointer).unwrap().as_array_mut().unwrap();
                let duplicate = entries[0].clone();
                entries.push(duplicate);
            });
            assert!(
                matches!(result, Err(SerializationError::Decode(message)) if message.contains("duplicate")),
                "{pointer}"
            );
        }
    }

    #[test]
    fn corrupt_runtime_metadata_and_ordering_are_rejected() {
        let mut engine = Engine::with_rules("(deftemplate item (slot value)) (defglobal ?*count* = 0) (defrule pick (item (value ?n)) => (bind ?*count* (+ ?*count* 1)))").unwrap();
        engine
            .load_str("(assert (item (value 1)) (item (value 2)))")
            .unwrap();
        for (pointer, value) in [
            ("/module_registry/next_id", serde_json::json!(0)),
            ("/module_registry/current_module", serde_json::json!(99)),
            ("/global_modules", serde_json::json!([])),
            ("/template_defs/1/value/defaults", serde_json::json!([])),
            ("/rete/agenda/strategy", serde_json::json!("Breadth")),
            ("/config/strategy", serde_json::json!("Breadth")),
        ] {
            let result = alter_state(&engine, |state| {
                *state.pointer_mut(pointer).expect(pointer) = value;
            });
            assert!(
                matches!(result, Err(SerializationError::InvalidState(_))),
                "{pointer}: {:?}",
                result.err()
            );
        }
        let result = alter_state(&engine, |state| {
            state["unknown_application_state"] = serde_json::json!("must not disappear");
        });
        assert!(
            matches!(result, Err(SerializationError::Decode(message)) if message.contains("unknown field"))
        );
    }

    #[test]
    fn malformed_template_type_metadata_and_defaults_are_rejected() {
        let engine = Engine::with_rules("(deftemplate item (slot n (type NUMBER)))").unwrap();
        for allowed in [
            serde_json::json!([]),
            serde_json::json!([[]]),
            serde_json::json!([["Integer", "Integer"]]),
            serde_json::json!([["Float", "Integer"]]),
        ] {
            let result = alter_state(&engine, |state| {
                state["template_defs"][1]["value"]["allowed_types"] = allowed.clone();
            });
            assert!(
                matches!(result, Err(SerializationError::InvalidState(_))),
                "accepted {allowed}: {:?}",
                result.err()
            );
        }
        let result = alter_state(&engine, |state| {
            state["template_defs"][1]["value"]["defaults"][0] =
                serde_json::to_value(Value::String(
                    ferric_rules_core::FerricString::new(
                        "wrong",
                        ferric_rules_core::StringEncoding::Utf8,
                    )
                    .unwrap(),
                ))
                .unwrap();
        });
        assert!(
            matches!(result, Err(SerializationError::InvalidState(message)) if message.contains("allowed types"))
        );
    }

    #[test]
    fn restored_live_and_dormant_template_facts_obey_declared_types() {
        for reset in [false, true] {
            for (slots, values) in [
                ("(slot n (type NUMBER) (default ?NONE))", "(n 2.5)"),
                ("(multislot n (type NUMBER) (default ?NONE))", "(n 1 2.5)"),
            ] {
                let mut engine = Engine::new(EngineConfig::default());
                engine
                    .load_str(&format!(
                        "(deftemplate item {slots}) (deffacts seed (item {values}))"
                    ))
                    .unwrap();
                if reset {
                    engine.reset().unwrap();
                }
                let result = alter_state(&engine, |state| {
                    state["template_defs"][1]["value"]["allowed_types"] =
                        serde_json::json!([["Integer"]]);
                });
                assert!(
                    matches!(result, Err(SerializationError::InvalidState(message)) if message.contains("allowed types")),
                    "{slots}, reset={reset}"
                );
            }
        }
    }

    #[test]
    fn writes_reject_unsupported_values_and_limits_before_returning_bytes() {
        let mut engine = Engine::new(EngineConfig::default());
        // Deliberately corrupt internal state: the public host boundary now
        // rejects this value before assertion, independently of persistence.
        let relation = engine
            .symbol_table
            .intern_symbol("broken", engine.config.string_encoding)
            .unwrap();
        engine
            .assert_fact_internal(Fact::Ordered(ferric_rules_core::OrderedFact {
                relation,
                fields: smallvec::smallvec![Value::String(ferric_rules_core::FerricString::Ascii(
                    vec![0xff].into_boxed_slice()
                ))],
            }))
            .unwrap();
        assert!(
            matches!(engine.serialize(SerializationFormat::Cbor), Err(SerializationError::InvalidState(message)) if message.contains("ASCII"))
        );
        let mut engine = Engine::new(EngineConfig::default());
        let mut value = Value::Integer(7);
        for _ in 0..33 {
            value = Value::Multifield(Box::new(vec![value].into_iter().collect()));
        }
        let relation = engine
            .symbol_table
            .intern_symbol("deep", engine.config.string_encoding)
            .unwrap();
        engine
            .assert_fact_internal(Fact::Ordered(ferric_rules_core::OrderedFact {
                relation,
                fields: smallvec::smallvec![value],
            }))
            .unwrap();
        assert!(
            matches!(engine.serialize(SerializationFormat::Cbor), Err(SerializationError::InvalidState(message)) if message.contains("value limit"))
        );
        let mut engine = Engine::with_rules("(defglobal ?*value* = 0)").unwrap();
        engine.globals.set(
            engine.module_registry.main_module_id(),
            "value",
            Value::Multifield(Box::new(
                vec![Value::ExternalAddress(ferric_rules_core::ExternalAddress {
                    type_id: ferric_rules_core::ExternalTypeId(1),
                    token: 42,
                })]
                .into_iter()
                .collect(),
            )),
        );
        assert!(matches!(
            engine.serialize(SerializationFormat::Cbor),
            Err(SerializationError::ExternalAddressPresent)
        ));
        let mut engine = Engine::new(EngineConfig::default());
        engine
            .assert_ordered(
                "large",
                vec![Value::String(ferric_rules_core::FerricString::Utf8(
                    "x".repeat(MAX_SNAPSHOT_BYTES).into_boxed_str(),
                ))],
            )
            .unwrap();
        assert!(
            matches!(engine.serialize(SerializationFormat::Cbor), Err(SerializationError::Encode(message)) if message.contains("byte limit"))
        );
    }

    #[test]
    fn complete_compiler_caches_preserve_distinct_paths_with_shared_prefixes() {
        let mut engine = Engine::with_rules("(defrule first (edge 1 ?x) => (assert (first ?x))) (defrule second (edge 1 2) => (assert (second)))").unwrap();
        engine.load_str("(assert (edge 1 2))").unwrap();
        for &format in SerializationFormat::ALL {
            let bytes = engine.serialize(format).unwrap();
            let mut restored = Engine::deserialize(&bytes, format).unwrap();
            assert_eq!(restored.run(RunLimit::Unlimited).unwrap().rules_fired, 2);
            restored
                .load_str("(defrule third (edge 1 ?x) => (assert (third ?x))) (assert (edge 1 3))")
                .unwrap();
            assert_eq!(restored.run(RunLimit::Unlimited).unwrap().rules_fired, 3);
            assert_eq!(restored.find_facts("first").unwrap().len(), 2);
            assert_eq!(restored.find_facts("second").unwrap().len(), 1);
            assert_eq!(restored.find_facts("third").unwrap().len(), 2);
        }
        for pointer in ["/compiler/alpha_path_cache", "/compiler/join_node_cache"] {
            let result = alter_state(&engine, |state| {
                state
                    .pointer_mut(pointer)
                    .unwrap()
                    .as_array_mut()
                    .unwrap()
                    .pop()
                    .unwrap();
            });
            assert!(
                matches!(result, Err(SerializationError::InvalidState(_))),
                "{pointer}"
            );
        }
    }

    #[test]
    fn source_rule_or_variants_resume_and_are_replaced_together() {
        let mut engine = Engine::with_rules(
            "(defrule select (or (candidate a ?n) (candidate b ?n)) => (assert (selected old ?n)))",
        )
        .unwrap();
        engine
            .load_str("(assert (candidate a 1) (candidate b 2))")
            .unwrap();
        for &format in SerializationFormat::ALL {
            let bytes = engine.serialize(format).unwrap();
            let mut restored = Engine::deserialize(&bytes, format).unwrap();
            assert_eq!(restored.run(RunLimit::Unlimited).unwrap().rules_fired, 2);
            assert_eq!(restored.find_facts("selected").unwrap().len(), 2);
            restored
                .load_str("(defrule select (candidate c ?n) => (assert (selected new ?n)))")
                .unwrap();
            restored
                .load_str("(assert (candidate a 3) (candidate b 4) (candidate c 5))")
                .unwrap();
            assert_eq!(restored.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
            assert_eq!(restored.find_facts("selected").unwrap().len(), 3);
            let bytes = restored.serialize(format).unwrap();
            let mut resumed = Engine::deserialize(&bytes, format).unwrap();
            assert_eq!(resumed.run(RunLimit::Unlimited).unwrap().rules_fired, 0);
        }
    }

    #[test]
    fn distinct_terminals_cannot_share_an_executable_rule_id() {
        let engine = Engine::with_rules("(defrule select (or (a) (b)) =>)").unwrap();
        let result = alter_state(&engine, |state| {
            let nodes = state
                .pointer_mut("/rete/beta/nodes")
                .unwrap()
                .as_array_mut()
                .unwrap();
            let mut terminals = nodes
                .iter_mut()
                .filter_map(|entry| entry[1].get_mut("Terminal"));
            let first_rule = terminals.next().unwrap()["rule"].clone();
            terminals.next().unwrap()["rule"] = first_rule;
        });
        assert!(
            matches!(result, Err(SerializationError::InvalidState(message)) if message.contains("executable rule ID"))
        );
    }

    fn schema_one_fixture_engine() -> Engine {
        let mut engine =
            Engine::with_rules(include_str!("../tests/fixtures/snapshots/schema-1.clp")).unwrap();
        engine.set_focus("WORK").unwrap();
        assert_eq!(engine.run(RunLimit::Count(1)).unwrap().rules_fired, 1);
        assert_eq!(engine.get_output("t"), Some("done 3\n"));
        assert!(matches!(engine.get_global("seen"), Some(Value::Integer(1))));
        engine
    }

    fn verify_schema_one_resume(mut engine: Engine) {
        assert_eq!(engine.facts().unwrap().count(), 4);
        assert_eq!(engine.get_output("t"), Some("done 3\n"));
        assert!(matches!(engine.get_global("seen"), Some(Value::Integer(1))));
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
        assert_eq!(engine.get_output("t"), Some("done 3\ndone 1\n"));
        assert!(matches!(engine.get_global("seen"), Some(Value::Integer(2))));
        let blocker = engine.find_facts("blocked").unwrap()[0].0;
        engine.retract(blocker).unwrap();
        engine.set_focus("WORK").unwrap();
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
        assert_eq!(engine.get_output("t"), Some("done 3\ndone 1\ndone 2\n"));
        assert!(matches!(engine.get_global("seen"), Some(Value::Integer(3))));
        assert_eq!(engine.facts().unwrap().count(), 3);
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 0);
        for (id, _) in engine.facts().unwrap() {
            let state = engine.get_fact_slot_by_name(id, "state").unwrap();
            let Value::Symbol(state) = state else {
                panic!("item state must be a symbol")
            };
            assert_eq!(engine.symbol_table.resolve_symbol_str(*state), Some("done"));
        }
        engine.reset().unwrap();
        engine.set_focus("WORK").unwrap();
        assert!(matches!(engine.get_global("seen"), Some(Value::Integer(0))));
        assert_eq!(engine.facts().unwrap().count(), 4);
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 2);
        assert!(matches!(engine.get_global("seen"), Some(Value::Integer(2))));
    }

    #[test]
    fn committed_schema_one_snapshot_is_explicitly_rejected() {
        let bytes = include_bytes!("../tests/fixtures/snapshots/schema-1.cbor");
        assert!(matches!(
            Engine::deserialize(bytes, SerializationFormat::Cbor),
            Err(SerializationError::UnsupportedVersion(1))
        ));
    }

    #[test]
    fn schema_one_fixture_source_has_expected_resume_behavior() {
        let engine = schema_one_fixture_engine();
        let bytes = engine.serialize(SerializationFormat::Cbor).unwrap();
        let restored = Engine::deserialize(&bytes, SerializationFormat::Cbor).unwrap();
        verify_schema_one_resume(restored);
    }

    fn cardinality_fixture_engine() -> Engine {
        let mut engine =
            Engine::with_rules(include_str!("../tests/fixtures/snapshots/schema-2.clp")).unwrap();
        assert_eq!(engine.run(RunLimit::Count(1)).unwrap().rules_fired, 1);
        assert!(matches!(engine.get_global("seen"), Some(Value::Integer(1))));
        assert_eq!(engine.get_output("t"), Some("one field\n"));
        engine
    }

    fn verify_cardinality_resume(mut engine: Engine) {
        assert_eq!(engine.get_output("t"), Some("one field\n"));
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
        assert!(matches!(engine.get_global("seen"), Some(Value::Integer(2))));
        // Restored compiled paths must reject new facts of the wrong width.
        engine.load_str("(assert (row) (row c d))").unwrap();
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 0);
        engine.load_str("(assert (row c))").unwrap();
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
        assert!(matches!(engine.get_global("seen"), Some(Value::Integer(3))));
        // Later compilation also preserves the guard while backfilling facts.
        engine
            .load_str("(defrule later (row ?x&~z) => (printout t later crlf))")
            .unwrap();
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 3);
        engine.load_str("(assert (row d e f))").unwrap();
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 0);
        engine.reset().unwrap();
        assert!(matches!(engine.get_global("seen"), Some(Value::Integer(0))));
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 4);
        assert!(matches!(engine.get_global("seen"), Some(Value::Integer(2))));
        assert!(engine.action_diagnostics().is_empty());
    }

    #[test]
    fn committed_schema_two_snapshot_is_explicitly_rejected() {
        let bytes = include_bytes!("../tests/fixtures/snapshots/schema-2.cbor");
        assert!(matches!(
            Engine::deserialize(bytes, SerializationFormat::Cbor),
            Err(SerializationError::UnsupportedVersion(2))
        ));
    }

    #[test]
    fn ordered_cardinality_roundtrips_in_all_snapshot_formats() {
        let engine = cardinality_fixture_engine();
        for &format in SerializationFormat::ALL {
            let bytes = engine.serialize(format).unwrap();
            verify_cardinality_resume(Engine::deserialize(&bytes, format).unwrap());
        }
    }

    fn schema_three_fixture_engine() -> Engine {
        let mut engine =
            Engine::with_rules(include_str!("../tests/fixtures/snapshots/schema-3.clp")).unwrap();
        assert_eq!(engine.run(RunLimit::Count(1)).unwrap().rules_fired, 1);
        assert_eq!(engine.get_output("t"), Some("split\n"));
        assert!(matches!(engine.get_global("seen"), Some(Value::Integer(1))));
        engine
    }

    fn verify_schema_three_resume(mut engine: Engine) {
        assert_eq!(engine.get_output("t"), Some("split\n"));
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 2);
        assert!(matches!(engine.get_global("seen"), Some(Value::Integer(3))));
        assert_eq!(engine.get_output("t"), Some("split\nsplit\nsplit\n"));
        let mut widths: Vec<_> = engine
            .find_facts("widths")
            .unwrap()
            .into_iter()
            .map(|(_, fact)| match fact {
                Fact::Ordered(fact) => match fact.fields.as_slice() {
                    [Value::Integer(left), Value::Integer(right)] => (*left, *right),
                    _ => panic!("widths must contain two scalar integers"),
                },
                Fact::Template(_) => panic!("widths must be ordered"),
            })
            .collect();
        widths.sort_unstable();
        assert_eq!(widths, [(0, 2), (1, 1), (2, 0)]);
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 0);
        let row = engine.find_facts("row").unwrap()[0].0;
        engine.retract(row).unwrap();
        engine.load_str("(assert (row c))").unwrap();
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 2);
        assert!(matches!(engine.get_global("seen"), Some(Value::Integer(5))));
        // Reuse the restored sequence join, including its capture metadata.
        engine
            .load_str("(defrule observe (row $?before $?after) => (printout t shared crlf))")
            .unwrap();
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 2);
        engine.reset().unwrap();
        assert!(matches!(engine.get_global("seen"), Some(Value::Integer(0))));
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 6);
        assert!(matches!(engine.get_global("seen"), Some(Value::Integer(3))));
        assert!(engine.action_diagnostics().is_empty());
    }

    #[test]
    fn committed_schema_three_snapshot_preserves_split_resume_and_reset_behavior() {
        let bytes = include_bytes!("../tests/fixtures/snapshots/schema-3.cbor");
        let restored = Engine::deserialize(bytes, SerializationFormat::Cbor).unwrap();
        verify_schema_three_resume(restored);
    }

    #[test]
    fn sequence_matches_roundtrip_in_all_snapshot_formats() {
        let engine = schema_three_fixture_engine();
        for &format in SerializationFormat::ALL {
            let bytes = engine.serialize(format).unwrap();
            verify_schema_three_resume(Engine::deserialize(&bytes, format).unwrap());
        }
    }

    #[test]
    #[ignore = "regenerates the committed schema-3 fixture; run explicitly after a schema change"]
    fn regenerate_schema_three_fixture() {
        let engine = schema_three_fixture_engine();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/snapshots/schema-3.cbor");
        std::fs::write(path, engine.serialize(SerializationFormat::Cbor).unwrap()).unwrap();
    }

    #[test]
    fn malformed_sequence_match_metadata_is_rejected() {
        let engine = schema_three_fixture_engine();
        for corruption in 0..3 {
            let result = alter_state(&engine, |state| {
                let slots = state["rete"]["token_store"]["sequence_matches"]
                    .as_array_mut()
                    .unwrap();
                let occupied: Vec<_> = slots
                    .iter()
                    .enumerate()
                    .filter(|(_, slot)| slot["value"].is_array())
                    .map(|(index, _)| index)
                    .collect();
                assert_eq!(occupied.len(), 3);
                match corruption {
                    0 => slots[occupied[0]]["value"] = serde_json::json!([usize::MAX, 0]),
                    1 => slots[occupied[1]]["value"] = slots[occupied[0]]["value"].clone(),
                    _ => {
                        slots[occupied[0]]["value"] = serde_json::Value::Null;
                        slots[occupied[0]]["version"] = serde_json::json!(0);
                    }
                }
            });
            assert!(
                matches!(result, Err(SerializationError::InvalidState(_))),
                "capture metadata corruption {corruption} was accepted"
            );
        }
    }

    #[test]
    fn sequence_projection_bindings_are_validated_after_restore() {
        let engine = schema_three_fixture_engine();
        let result = alter_state(&engine, |state| {
            let slots = state["rete"]["token_store"]["tokens"]
                .as_array_mut()
                .unwrap();
            let token = slots
                .iter_mut()
                .find(|slot| slot["value"]["fact"].is_object())
                .unwrap();
            token["value"]["bindings"]["bindings"][0] =
                serde_json::json!({"Inline": {"Integer": 42}});
        });
        assert!(matches!(result, Err(SerializationError::InvalidState(_))));
    }

    #[test]
    fn sequence_metadata_cannot_be_attached_to_a_root_token() {
        let engine = schema_three_fixture_engine();
        let result = alter_state(&engine, |state| {
            let root_slot = state["rete"]["token_store"]["tokens"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
                .find(|(_, slot)| slot["value"].is_object() && slot["value"]["parent"].is_null())
                .map(|(index, slot)| (index, slot["version"].clone()))
                .unwrap();
            state["rete"]["token_store"]["sequence_matches"][root_slot.0] =
                serde_json::json!({"value": [], "version": root_slot.1});
        });
        assert!(
            matches!(result, Err(SerializationError::InvalidState(message))
            if message.contains("sequence match metadata") || message.contains("nonsequence token"))
        );
    }

    #[test]
    fn sequence_conditionals_preserve_fact_support_through_restore() {
        let mut engine = Engine::with_rules(r"
            (deffacts rows (row a b) (enabled))
            (defrule absent (not (row $?left mark $?right)) => (printout t absent crlf))
            (defrule present (exists (row $?left mark $?right)) => (printout t present crlf))
            (defrule clear (not (and (row $?left mark $?right) (enabled))) => (printout t clear crlf))
        ").unwrap();
        for &format in SerializationFormat::ALL {
            let bytes = engine.serialize(format).unwrap();
            let mut restored = Engine::deserialize(&bytes, format).unwrap();
            assert_eq!(restored.run(RunLimit::Unlimited).unwrap().rules_fired, 2);
            restored
                .load_str("(assert (row mark mark) (row mark x mark))")
                .unwrap();
            assert_eq!(restored.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
            let bytes = restored.serialize(format).unwrap();
            let mut restored = Engine::deserialize(&bytes, format).unwrap();
            let rows: Vec<_> = restored
                .find_facts("row")
                .unwrap()
                .into_iter()
                .filter_map(|(id, fact)| match fact {
                    Fact::Ordered(fact)
                        if matches!(fact.fields.first(), Some(Value::Symbol(symbol))
                        if restored.symbol_table.resolve_symbol_str(*symbol) == Some("mark")) =>
                    {
                        Some(id)
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(rows.len(), 2);
            // The original row never supports the condition. Each marked row
            // has two cuts, but is still one independently retractable support.
            restored.retract(rows[0]).unwrap();
            assert_eq!(restored.run(RunLimit::Unlimited).unwrap().rules_fired, 0);
            restored.retract(rows[1]).unwrap();
            assert_eq!(restored.run(RunLimit::Unlimited).unwrap().rules_fired, 2);
            assert!(restored.action_diagnostics().is_empty());
        }
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 2);
    }

    #[test]
    fn sequence_plan_and_compilation_cache_are_validated_after_restore() {
        let engine = Engine::with_rules(
            "(defrule capture (row $?left pivot $?right) =>) (assert (row pivot))",
        )
        .unwrap();
        let invalid_selector = alter_state(&engine, |state| {
            let nodes = state["rete"]["beta"]["nodes"].as_array_mut().unwrap();
            let join = nodes
                .iter_mut()
                .find_map(|entry| entry[1].get_mut("Join"))
                .unwrap();
            join["sequence"]["tests"][0]["slot"]["Ordered"] = serde_json::json!(999);
        });
        assert!(
            matches!(invalid_selector, Err(SerializationError::InvalidState(message))
            if message.contains("invalid logical field"))
        );
        let invalid_cache = alter_state(&engine, |state| {
            state["compiler"]["join_node_cache"][0][0]["sequence"]["fields"][0] =
                serde_json::json!("Single");
        });
        assert!(
            matches!(invalid_cache, Err(SerializationError::InvalidState(message))
            if message.contains("cached join node mismatch"))
        );
    }

    #[test]
    fn restored_named_seeds_require_valid_unique_module_identities() {
        let engine = schema_one_fixture_engine();
        for (pointer, value) in [
            ("/registered_deffacts/0/module", serde_json::json!(99)),
            ("/registered_deffacts/0/name", serde_json::json!("")),
        ] {
            let result = alter_state(&engine, |state| {
                *state.pointer_mut(pointer).unwrap() = value;
            });
            assert!(
                matches!(result, Err(SerializationError::InvalidState(_))),
                "{pointer}"
            );
        }
        let result = alter_state(&engine, |state| {
            let seeds = state["registered_deffacts"].as_array_mut().unwrap();
            seeds.push(seeds[0].clone());
        });
        assert!(
            matches!(result, Err(SerializationError::InvalidState(message)) if message.contains("duplicate named deffacts"))
        );
    }

    #[test]
    fn restored_global_initializers_keep_their_registered_identities() {
        let mut engine =
            Engine::with_rules("(defglobal ?*count* = 7) (defrule update => (bind ?*count* 9))")
                .unwrap();
        engine.run(RunLimit::Unlimited).unwrap();
        assert!(matches!(
            engine.get_global("count"),
            Some(Value::Integer(9))
        ));
        assert!(engine.load_str("(defglobal ?*count* = 100)").is_err());
        let snapshot = engine.serialize(SerializationFormat::Cbor).unwrap();
        let mut restored = Engine::deserialize(&snapshot, SerializationFormat::Cbor).unwrap();
        restored.reset().unwrap();
        assert!(matches!(
            restored.get_global("count"),
            Some(Value::Integer(7))
        ));

        let duplicate = alter_state(&engine, |state| {
            let globals = state["registered_globals"].as_array_mut().unwrap();
            globals.push(globals[0].clone());
        });
        assert!(
            matches!(duplicate, Err(SerializationError::InvalidState(message)) if message.contains("duplicate registered global"))
        );
        let missing = alter_state(&engine, |state| {
            state["registered_globals"][0][1] = serde_json::json!("missing");
        });
        assert!(
            matches!(missing, Err(SerializationError::InvalidState(message)) if message.contains("registered global missing"))
        );
    }

    #[test]
    fn restored_graph_paths_have_the_source_depth_limits() {
        use std::fmt::Write;
        let fields = (1..=64)
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        let mut alpha = Engine::with_rules(&format!(
            "(defrule match (wide {fields}) => (assert (matched)))"
        ))
        .unwrap();
        let bytes = alpha.serialize(SerializationFormat::Cbor).unwrap();
        let mut restored = Engine::deserialize(&bytes, SerializationFormat::Cbor).unwrap();
        restored
            .load_str(&format!("(assert (wide {fields}))"))
            .unwrap();
        assert_eq!(restored.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
        assert_eq!(restored.find_facts("matched").unwrap().len(), 1);
        let result = alter_state(&alpha, |state| {
            let nodes = state["rete"]["alpha"]["nodes"].as_array_mut().unwrap();
            let id = nodes.len();
            let appended = nodes.last().unwrap().clone();
            let previous = nodes.last_mut().unwrap().get_mut("ConstantTest").unwrap();
            previous["memory"] = serde_json::Value::Null;
            previous["children"] = serde_json::json!([id]);
            nodes.push(appended);
            state["rete"]["alpha"]["next_node_id"] = serde_json::json!(id + 1);
        });
        assert!(
            matches!(result, Err(SerializationError::InvalidState(message)) if message.contains("64 tests"))
        );
        // Removing an accepted fact must also stay within the bounded path.
        alpha
            .load_str(&format!("(assert (wide {fields}))"))
            .unwrap();
        let fact = alpha.find_facts("wide").unwrap()[0].0;
        alpha.retract(fact).unwrap();
        assert_eq!(alpha.run(RunLimit::Unlimited).unwrap().rules_fired, 0);

        let mut source = String::from("(defrule deep");
        for index in 0..64 {
            write!(source, " (p{index} ?x)").unwrap();
        }
        source.push_str(" => (assert (matched)))");
        let engine = Engine::with_rules(&source).unwrap();
        let bytes = engine.serialize(SerializationFormat::Cbor).unwrap();
        let mut restored = Engine::deserialize(&bytes, SerializationFormat::Cbor).unwrap();
        for index in 0..64 {
            restored
                .load_str(&format!("(assert (p{index} 7))"))
                .unwrap();
        }
        assert_eq!(restored.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
        assert_eq!(restored.find_facts("matched").unwrap().len(), 1);
        let result = alter_state(&engine, |state| {
            let beta = &mut state["rete"]["beta"];
            let id = beta["next_node_id"].as_u64().unwrap();
            let memory = beta["next_memory_id"].as_u64().unwrap();
            let nodes = beta["nodes"].as_array_mut().unwrap();
            let terminal = nodes
                .iter()
                .find(|node| node[1].get("Terminal").is_some())
                .unwrap();
            let terminal_id = terminal[0].as_u64().unwrap();
            let parent = terminal[1]["Terminal"]["parent"].as_u64().unwrap();
            let rule = terminal[1]["Terminal"]["rule"].clone();
            beta_body_mut(nodes, terminal_id)["parent"] = serde_json::json!(id);
            let children = beta_body_mut(nodes, parent)["children"]
                .as_array_mut()
                .unwrap();
            *children
                .iter_mut()
                .find(|child| child.as_u64() == Some(terminal_id))
                .unwrap() = serde_json::json!(id);
            nodes.push(serde_json::json!([id, {"Predicate": {"parent": parent, "rule": rule, "condition_index": 0, "memory": memory, "children": [terminal_id]}}]));
            let memories = beta["memories"].as_array_mut().unwrap();
            let mut empty = memories.last().unwrap().clone();
            empty["id"] = serde_json::json!(memory);
            memories.push(empty);
            beta["next_node_id"] = serde_json::json!(id + 1);
            beta["next_memory_id"] = serde_json::json!(memory + 1);
        });
        assert!(
            matches!(result, Err(SerializationError::InvalidState(message)) if message.contains("66 nodes"))
        );
    }

    fn beta_body_mut(nodes: &mut [serde_json::Value], id: u64) -> &mut serde_json::Value {
        nodes
            .iter_mut()
            .find(|node| node[0].as_u64() == Some(id))
            .unwrap()[1]
            .as_object_mut()
            .unwrap()
            .values_mut()
            .next()
            .unwrap()
    }

    #[test]
    fn short_beta_paths_cannot_hide_deep_or_cyclic_ncc_callbacks() {
        use std::fmt::Write;
        for (count, cycle, expected) in
            [(2, true, "cyclic NCC"), (5, false, "NCC nesting exceeds 4")]
        {
            let mut source = String::new();
            for index in 0..count {
                write!(
                    source,
                    "(defrule r{index} (not (and (a{index}) (b{index}))) =>)"
                )
                .unwrap();
            }
            let engine = Engine::with_rules(&source).unwrap();
            let result = alter_state(&engine, |state| {
                let nodes = state["rete"]["beta"]["nodes"].as_array_mut().unwrap();
                let nccs = nodes
                    .iter()
                    .filter_map(|node| {
                        node[1].get("Ncc").map(|body| {
                            (node[0].as_u64().unwrap(), body["partner"].as_u64().unwrap())
                        })
                    })
                    .collect::<Vec<_>>();
                for index in 0..(if cycle { count } else { count - 1 }) {
                    let (_, partner) = nccs[index];
                    let next = nccs[(index + 1) % count].0;
                    let old_parent = beta_body_mut(nodes, partner)["parent"].as_u64().unwrap();
                    beta_body_mut(nodes, old_parent)["children"]
                        .as_array_mut()
                        .unwrap()
                        .retain(|child| child.as_u64() != Some(partner));
                    beta_body_mut(nodes, next)["children"]
                        .as_array_mut()
                        .unwrap()
                        .push(serde_json::json!(partner));
                    beta_body_mut(nodes, partner)["parent"] = serde_json::json!(next);
                }
            });
            assert!(
                matches!(result, Err(SerializationError::InvalidState(message)) if message.contains(expected)),
                "{expected}"
            );
        }
    }

    #[test]
    fn corrupted_join_memory_and_index_order_is_rejected() {
        let mut engine =
            Engine::with_rules("(defrule joined (item ?key ?id) (gate ?key) =>) (defrule reversed (gate ?key) (item ?key ?id) =>)").unwrap();
        for id in 0..20 {
            engine
                .assert_ordered(
                    "item",
                    vec![
                        ferric_rules_core::Value::Integer(1),
                        ferric_rules_core::Value::Integer(id),
                    ],
                )
                .unwrap();
        }
        for (network, field) in [
            ("alpha", "facts"),
            ("alpha", "slot_indices"),
            ("beta", "var_indices"),
        ] {
            let result = alter_state(&engine, |state| {
                let mut changed = false;
                for memory in state["rete"][network]["memories"].as_array_mut().unwrap() {
                    if field == "facts" {
                        let ids = memory[field].as_array_mut().unwrap();
                        if ids.len() > 1 {
                            ids.reverse();
                            changed = true;
                            break;
                        }
                    } else {
                        for entry in memory[field].as_array_mut().unwrap() {
                            for bucket in entry[1].as_array_mut().unwrap() {
                                let ids = bucket[1].as_array_mut().unwrap();
                                if ids.len() > 1 {
                                    ids.reverse();
                                    changed = true;
                                    break;
                                }
                            }
                            if changed {
                                break;
                            }
                        }
                        if changed {
                            break;
                        }
                    }
                }
                assert!(changed, "missing multi-entry {network}/{field}");
            });
            assert!(
                matches!(result, Err(SerializationError::InvalidState(_))),
                "{network}/{field}: {:?}",
                result.err()
            );
        }
    }

    #[test]
    fn otherwise_consistent_beta_relabeling_cannot_change_dispatch_order() {
        let engine = Engine::with_rules("(defrule joined (a ?x) (b ?y) =>)").unwrap();
        let result = alter_state(&engine, |state| {
            let mut joins: Vec<_> = state["rete"]["beta"]["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|entry| entry[1].get("Join").is_some())
                .map(|entry| entry[0].as_u64().unwrap())
                .collect();
            joins.sort_unstable();
            assert_eq!(joins.len(), 2);
            let swap = |value: &mut serde_json::Value| {
                if let Some(id) = value.as_u64() {
                    if id == joins[0] {
                        *value = serde_json::json!(joins[1]);
                    } else if id == joins[1] {
                        *value = serde_json::json!(joins[0]);
                    }
                }
            };
            for entry in state["rete"]["beta"]["nodes"].as_array_mut().unwrap() {
                swap(&mut entry[0]);
                let body = entry[1]
                    .as_object_mut()
                    .unwrap()
                    .values_mut()
                    .next()
                    .unwrap();
                if let Some(parent) = body.get_mut("parent") {
                    swap(parent);
                }
                if let Some(children) = body.get_mut("children") {
                    for child in children.as_array_mut().unwrap() {
                        swap(child);
                    }
                }
            }
            for entry in state["rete"]["beta"]["alpha_to_joins"]
                .as_array_mut()
                .unwrap()
            {
                for node in entry[1].as_array_mut().unwrap() {
                    swap(node);
                }
            }
            for entry in state["compiler"]["join_node_cache"].as_array_mut().unwrap() {
                swap(&mut entry[0]["parent"]);
                swap(&mut entry[1]);
            }
        });
        assert!(
            matches!(result, Err(SerializationError::InvalidState(ref message)) if message.contains("beta allocation order")),
            "{:?}",
            result.err()
        );
    }

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
        let mut modules = engine2.modules();
        modules.sort_unstable();
        assert_eq!(modules, ["A", "B", "MAIN"]);
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
    fn fr_rete_001_snapshot_preserves_policy_and_duplicate_index() {
        for &format in SerializationFormat::ALL {
            let mut engine = Engine::new(EngineConfig::default());
            engine.set_fact_duplication(true);
            let first = engine.assert_ordered("item", 1_i64).unwrap();
            let second = engine.assert_ordered("item", 1_i64).unwrap();
            assert_ne!(first, second);

            let bytes = engine.serialize(format).unwrap();
            let mut restored = Engine::deserialize(&bytes, format).unwrap();
            assert!(
                restored.fact_duplication(),
                "format {format:?} lost duplication policy"
            );

            assert!(restored.set_fact_duplication(false));
            let rejected = restored.assert_ordered_with_result("item", 1_i64).unwrap();
            assert!(
                matches!(rejected, crate::FactAssertionResult::Duplicate(_)),
                "format {format:?} lost the structural duplicate index"
            );
            assert_eq!(restored.facts().unwrap().count(), 2);
        }
    }

    #[test]
    fn fr_rete_004_snapshot_preserves_predicate_nodes() {
        let source = r"
            (defrule positive
                (value ?x)
                (test (> ?x 0))
                =>
                (printout t ?x crlf))
        ";

        for &format in SerializationFormat::ALL {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(source).unwrap();
            engine.reset().unwrap();
            engine.assert_ordered("value", -1_i64).unwrap();
            engine.assert_ordered("value", 1_i64).unwrap();
            assert_eq!(engine.agenda_len(), 1);

            let bytes = engine.serialize(format).unwrap();
            let mut restored = Engine::deserialize(&bytes, format).unwrap();
            let result = restored.run(RunLimit::Unlimited).unwrap();

            assert_eq!(
                result.rules_fired, 1,
                "format {format:?} lost the passing predicate token"
            );
            assert_eq!(restored.get_output("t"), Some("1\n"));
        }
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
