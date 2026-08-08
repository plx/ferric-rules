//! Dedicated structured observation surface for compatibility tooling.
//!
//! This command deliberately does not share the human-oriented output contract
//! of `ferric run`. It captures the engine's known router channels and emits one
//! versioned JSON document on process stdout.
//!
//! The public runtime APIs do not currently expose the owning module for a fact,
//! an enumeration of router channels or globals, or the names of rules fired by
//! `Engine::run`. Version 1 reports the sole registered module when ownership is
//! unambiguous and otherwise reports fact modules as `null`; the other limits
//! are advertised in `capabilities`.

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::Path;

use ferric_rules_core::{Fact, Value};
use ferric_rules_runtime::{
    ActionError, Engine, EngineConfig, HaltReason, LoadError, RunLimit, RunResult,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use slotmap::Key as _;

const SCHEMA_NAME: &str = "ferric.compat-observation";
const SCHEMA_VERSION: u8 = 1;
const DIAGNOSTIC_TAXONOMY_VERSION: u8 = 1;
const MAX_FIXTURE_ID_BYTES: usize = 128;
const MIN_NONCE_HEX_BYTES: usize = 32;
const MAX_NONCE_HEX_BYTES: usize = 128;
const KNOWN_CHANNELS: &[&str] = &[
    "t", "stdin", "stdout", "stderr", "wclips", "wdialog", "wdisplay", "werror", "wtrace",
    "wwarning",
];

/// Validate a fixture identifier at the CLI boundary.
pub(crate) fn parse_fixture_id(value: &str) -> Result<String, String> {
    if value.is_empty() || value.len() > MAX_FIXTURE_ID_BYTES {
        return Err(format!(
            "fixture id must be a protocol-safe token of 1 to {MAX_FIXTURE_ID_BYTES} ASCII bytes"
        ));
    }
    let mut bytes = value.bytes();
    let valid = bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && bytes.all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':' | b'/')
        });
    if !valid {
        return Err(
            "fixture id must start with an ASCII letter or digit and contain only ASCII letters, \
             digits, hyphen, underscore, dot, colon, and slash"
                .to_string(),
        );
    }
    Ok(value.to_string())
}

/// Validate a nonce at the CLI boundary.
pub(crate) fn parse_nonce(value: &str) -> Result<String, String> {
    if !(MIN_NONCE_HEX_BYTES..=MAX_NONCE_HEX_BYTES).contains(&value.len())
        || value.len() % 2 != 0
        || !value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err("nonce must encode 16 to 64 bytes as lowercase hexadecimal".to_string());
    }
    Ok(value.to_string())
}

/// Validate canonical lowercase SHA-256 text at the CLI boundary.
pub(crate) fn parse_sha256(value: &str) -> Result<String, String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err("digest must be exactly 64 lowercase hexadecimal characters".to_string());
    }
    Ok(value.to_string())
}

/// Execute the hidden `compat-observe` subcommand.
pub fn execute(
    file_path: &Path,
    fixture_id: String,
    nonce: String,
    source_sha256: String,
    composed_sha256: String,
) -> i32 {
    let fixture = FixtureIdentity {
        id: fixture_id,
        nonce,
        source_sha256,
        composed_sha256,
    };
    let mut observation = Observation::started(fixture);
    let mut engine = Engine::new(EngineConfig::default());

    observation.phase_reached = Phase::Load;
    let source_bytes = match std::fs::read(file_path) {
        Ok(source) => source,
        Err(error) => {
            observation.diagnostics.push(Diagnostic::error(
                Phase::Harness,
                "harness-error",
                format!("failed to read {}: {error}", file_path.display()),
            ));
            return emit_failed_observation(observation, &engine);
        }
    };

    let actual_composed_sha256 = lowercase_sha256(&source_bytes);
    if actual_composed_sha256 != observation.fixture.composed_sha256 {
        observation.diagnostics.push(Diagnostic::error(
            Phase::Harness,
            "harness-error",
            format!(
                "composed input digest mismatch: expected {}, observed {actual_composed_sha256}",
                observation.fixture.composed_sha256
            ),
        ));
        return emit_failed_observation(observation, &engine);
    }

    let source = match std::str::from_utf8(&source_bytes) {
        Ok(source) => source,
        Err(error) => {
            observation.diagnostics.push(Diagnostic::error(
                Phase::Harness,
                "harness-error",
                format!("composed input is not valid UTF-8: {error}"),
            ));
            return emit_failed_observation(observation, &engine);
        }
    };

    let load_result = match engine.load_str(source) {
        Ok(result) => result,
        Err(errors) => {
            observation.phase_reached = load_failure_phase(&errors);
            observation
                .diagnostics
                .extend(errors.iter().map(load_error_diagnostic));
            return emit_failed_observation(observation, &engine);
        }
    };
    observation.diagnostics.extend(
        load_result
            .warnings
            .into_iter()
            .map(|message| Diagnostic::warning(Phase::Load, "construct-error", message, true)),
    );

    observation.phase_reached = Phase::Reset;
    if let Err(error) = engine.reset() {
        observation.diagnostics.push(Diagnostic::error(
            Phase::Reset,
            "evaluation-error",
            format!("reset failed: {error}"),
        ));
        return emit_failed_observation(observation, &engine);
    }

    observation.phase_reached = Phase::Run;
    let run_result = match engine.run(RunLimit::Unlimited) {
        Ok(result) => result,
        Err(error) => {
            observation.diagnostics.push(Diagnostic::error(
                Phase::Run,
                "evaluation-error",
                format!("execution failed: {error}"),
            ));
            return emit_failed_observation(observation, &engine);
        }
    };

    observation.phase_reached = if matches!(run_result.halt_reason, HaltReason::ActionError) {
        Phase::Run
    } else {
        Phase::PostRun
    };
    match capture_state(&engine, Some(run_result)) {
        Ok(state) => {
            observation.apply_capture(state);
            // COMPLETE is deliberately created only after all post-run state has
            // been converted into owned observation data.
            observation
                .lifecycle
                .push(LifecycleRecord::complete(&observation.fixture));
            emit_observation(&observation, 0)
        }
        Err(error) => {
            observation
                .diagnostics
                .push(Diagnostic::error(Phase::Harness, "harness-error", error));
            emit_observation(&observation, 1)
        }
    }
}

fn emit_failed_observation(mut observation: Observation, engine: &Engine) -> i32 {
    match capture_state(engine, None) {
        Ok(state) => observation.apply_capture(state),
        Err(error) => {
            observation
                .diagnostics
                .push(Diagnostic::error(Phase::Harness, "harness-error", error));
        }
    }
    // COMPLETE attests that the adapter finished capturing the terminal
    // envelope; it does not imply that the fixture reached or completed run.
    observation
        .lifecycle
        .push(LifecycleRecord::complete(&observation.fixture));
    emit_observation(&observation, 1)
}

fn emit_observation(observation: &Observation, intended_exit_code: i32) -> i32 {
    let json = match serde_json::to_string(observation) {
        Ok(json) => json,
        Err(error) => {
            eprintln!("ferric compat-observe: failed to serialize observation: {error}");
            return 1;
        }
    };
    let mut encoded = json.into_bytes();
    encoded.push(b'\n');
    if let Err(error) = std::io::stdout().lock().write_all(&encoded) {
        eprintln!("ferric compat-observe: failed to write observation: {error}");
        return 1;
    }
    intended_exit_code
}

fn lowercase_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        write!(encoded, "{byte:02x}").expect("writing to a String cannot fail");
    }
    encoded
}

fn capture_state(engine: &Engine, run_result: Option<RunResult>) -> Result<CapturedState, String> {
    let module_names = engine.modules();
    let fact_module = (module_names.len() == 1).then(|| module_names[0].to_string());
    let facts = engine
        .facts()
        .map_err(|error| format!("failed to enumerate facts: {error}"))?
        .enumerate()
        .map(|(ordinal, (fact_id, fact))| {
            observe_fact(
                engine,
                ordinal,
                fact_id.data().as_ffi().to_string(),
                fact_module.as_deref(),
                fact,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;

    let channels = KNOWN_CHANNELS
        .iter()
        .map(|name| {
            let output = engine.get_output(name);
            ChannelObservation {
                name: (*name).to_string(),
                present: output.is_some(),
                text: output.unwrap_or_default().to_string(),
            }
        })
        .collect();

    let action_diagnostic_continued =
        !run_result.is_some_and(|result| matches!(result.halt_reason, HaltReason::ActionError));
    let action_diagnostics = engine
        .action_diagnostics()
        .iter()
        .map(|error| action_error_diagnostic(error, action_diagnostic_continued))
        .collect();

    let run = run_result.map(|result| RunObservation {
        rules_fired: result.rules_fired,
        fired_rule_names: None,
        halt_reason: match result.halt_reason {
            HaltReason::AgendaEmpty => HaltReasonObservation::AgendaEmpty,
            HaltReason::LimitReached => HaltReasonObservation::LimitReached,
            HaltReason::HaltRequested => HaltReasonObservation::HaltRequested,
            HaltReason::ActionError => HaltReasonObservation::ActionError,
        },
        agenda_size: engine.agenda_len(),
        halted: engine.is_halted(),
    });

    Ok(CapturedState {
        run,
        facts,
        fact_modules: fact_module.is_some(),
        channels,
        action_diagnostics,
        modules: ModuleObservation {
            current: engine.current_module().to_string(),
            focus: engine.get_focus().map(str::to_string),
            focus_stack: engine
                .get_focus_stack()
                .into_iter()
                .map(str::to_string)
                .collect(),
        },
    })
}

fn observe_fact(
    engine: &Engine,
    ordinal: usize,
    fact_id: String,
    module: Option<&str>,
    fact: &Fact,
) -> Result<FactObservation, String> {
    match fact {
        Fact::Ordered(ordered) => {
            let relation = engine.resolve_symbol(ordered.relation).ok_or_else(|| {
                format!("ordered fact {fact_id} has an unresolved relation symbol")
            })?;
            let fields = ordered
                .fields
                .iter()
                .map(|value| observe_value(engine, value))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(FactObservation::Ordered {
                ordinal,
                fact_id,
                module: module.map(str::to_string),
                relation: relation.to_string(),
                fields,
            })
        }
        Fact::Template(template) => {
            let name = engine
                .template_name_by_id(template.template_id)
                .ok_or_else(|| format!("template fact {fact_id} has an unknown template id"))?;
            let slot_names = engine
                .template_slot_names_by_id(template.template_id)
                .ok_or_else(|| format!("template fact {fact_id} has no public slot metadata"))?;
            if slot_names.len() != template.slots.len() {
                return Err(format!(
                    "template fact {fact_id} has {} values but {} public slot names",
                    template.slots.len(),
                    slot_names.len()
                ));
            }
            let slots = slot_names
                .into_iter()
                .zip(template.slots.iter())
                .map(|(slot_name, value)| {
                    Ok(SlotObservation {
                        name: slot_name.to_string(),
                        value: observe_value(engine, value)?,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok(FactObservation::Template {
                ordinal,
                fact_id,
                module: module.map(str::to_string),
                name: name.to_string(),
                slots,
            })
        }
    }
}

fn observe_value(engine: &Engine, value: &Value) -> Result<ValueObservation, String> {
    match value {
        Value::Symbol(symbol) => {
            let value = engine
                .resolve_symbol(*symbol)
                .ok_or_else(|| "fact value has an unresolved symbol".to_string())?;
            Ok(ValueObservation::Symbol {
                value: value.to_string(),
            })
        }
        Value::String(value) => Ok(ValueObservation::String {
            value: value.as_str().to_string(),
        }),
        Value::Integer(value) => Ok(ValueObservation::Integer {
            value: value.to_string(),
        }),
        Value::Float(value) => Ok(ValueObservation::Float {
            value: canonical_float(*value),
            bits: format!("0x{:016x}", value.to_bits()),
        }),
        Value::Multifield(values) => Ok(ValueObservation::Multifield {
            values: values
                .iter()
                .map(|value| observe_value(engine, value))
                .collect::<Result<Vec<_>, _>>()?,
        }),
        Value::ExternalAddress(address) => Ok(ValueObservation::ExternalAddress {
            external_type_id: address.type_id.0,
            opaque: true,
        }),
        Value::Void => Ok(ValueObservation::Void),
    }
}

fn canonical_float(value: f64) -> String {
    if value.is_nan() {
        "NaN".to_string()
    } else if value == f64::INFINITY {
        "Infinity".to_string()
    } else if value == f64::NEG_INFINITY {
        "-Infinity".to_string()
    } else {
        value.to_string()
    }
}

fn load_error_diagnostic(error: &LoadError) -> Diagnostic {
    let (phase, category) = match error {
        LoadError::Parse(_) => (Phase::Parse, "syntax-error"),
        LoadError::Interpret(_)
        | LoadError::UnsupportedForm { .. }
        | LoadError::InvalidAssert(_)
        | LoadError::InvalidDefrule(_)
        | LoadError::Compile(_)
        | LoadError::Validation(_)
        | LoadError::Engine(_)
        | LoadError::Io(_) => (Phase::Load, "construct-error"),
    };
    Diagnostic::error(phase, category, error.to_string())
}

fn load_failure_phase(errors: &[LoadError]) -> Phase {
    if errors
        .iter()
        .all(|error| matches!(error, LoadError::Parse(_)))
    {
        Phase::Parse
    } else {
        Phase::Load
    }
}

fn action_error_diagnostic(error: &ActionError, continued: bool) -> Diagnostic {
    Diagnostic::warning(Phase::Run, "evaluation-error", error.to_string(), continued)
}

#[derive(Serialize)]
struct Observation {
    schema: &'static str,
    version: u8,
    engine: EngineIdentity,
    fixture: FixtureIdentity,
    phase_reached: Phase,
    lifecycle: Vec<LifecycleRecord>,
    run: Option<RunObservation>,
    facts: Vec<FactObservation>,
    channels: Vec<ChannelObservation>,
    diagnostics: Vec<Diagnostic>,
    modules: ModuleObservation,
    capabilities: Capabilities,
}

impl Observation {
    fn started(fixture: FixtureIdentity) -> Self {
        let start = LifecycleRecord::start(&fixture);
        Self {
            schema: SCHEMA_NAME,
            version: SCHEMA_VERSION,
            engine: EngineIdentity {
                name: "ferric",
                version: env!("CARGO_PKG_VERSION"),
            },
            fixture,
            phase_reached: Phase::Start,
            lifecycle: vec![start],
            run: None,
            facts: Vec::new(),
            channels: Vec::new(),
            diagnostics: Vec::new(),
            modules: ModuleObservation::default(),
            capabilities: Capabilities {
                fact_modules: false,
                fired_rule_names: false,
                global_values: false,
                router_channel_enumeration: false,
                external_address_identity: false,
                source_digest_verification: false,
                composed_digest_verification: true,
            },
        }
    }

    fn apply_capture(&mut self, capture: CapturedState) {
        self.run = capture.run;
        self.facts = capture.facts;
        self.capabilities.fact_modules = capture.fact_modules;
        self.channels = capture.channels;
        self.diagnostics.extend(capture.action_diagnostics);
        self.modules = capture.modules;
    }
}

struct CapturedState {
    run: Option<RunObservation>,
    facts: Vec<FactObservation>,
    fact_modules: bool,
    channels: Vec<ChannelObservation>,
    action_diagnostics: Vec<Diagnostic>,
    modules: ModuleObservation,
}

#[derive(Serialize)]
struct EngineIdentity {
    name: &'static str,
    version: &'static str,
}

#[derive(Serialize)]
struct FixtureIdentity {
    id: String,
    nonce: String,
    source_sha256: String,
    composed_sha256: String,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Phase {
    Start,
    Parse,
    Load,
    Reset,
    Run,
    PostRun,
    Harness,
}

#[derive(Serialize)]
struct LifecycleRecord {
    sequence: u8,
    event: LifecycleEvent,
    fixture_id: String,
    nonce: String,
    source_sha256: String,
    composed_sha256: String,
}

impl LifecycleRecord {
    fn start(fixture: &FixtureIdentity) -> Self {
        Self::new(0, LifecycleEvent::Start, fixture)
    }

    fn complete(fixture: &FixtureIdentity) -> Self {
        Self::new(1, LifecycleEvent::Complete, fixture)
    }

    fn new(sequence: u8, event: LifecycleEvent, fixture: &FixtureIdentity) -> Self {
        Self {
            sequence,
            event,
            fixture_id: fixture.id.clone(),
            nonce: fixture.nonce.clone(),
            source_sha256: fixture.source_sha256.clone(),
            composed_sha256: fixture.composed_sha256.clone(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "UPPERCASE")]
enum LifecycleEvent {
    Start,
    Complete,
}

#[derive(Serialize)]
struct RunObservation {
    rules_fired: usize,
    fired_rule_names: Option<Vec<String>>,
    halt_reason: HaltReasonObservation,
    agenda_size: usize,
    halted: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
enum HaltReasonObservation {
    AgendaEmpty,
    LimitReached,
    HaltRequested,
    ActionError,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum FactObservation {
    Ordered {
        ordinal: usize,
        fact_id: String,
        module: Option<String>,
        relation: String,
        fields: Vec<ValueObservation>,
    },
    Template {
        ordinal: usize,
        fact_id: String,
        module: Option<String>,
        name: String,
        slots: Vec<SlotObservation>,
    },
}

#[derive(Serialize)]
struct SlotObservation {
    name: String,
    value: ValueObservation,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
enum ValueObservation {
    Symbol { value: String },
    String { value: String },
    Integer { value: String },
    Float { value: String, bits: String },
    Multifield { values: Vec<ValueObservation> },
    ExternalAddress { external_type_id: u32, opaque: bool },
    Void,
}

#[derive(Serialize)]
struct ChannelObservation {
    name: String,
    present: bool,
    text: String,
}

#[derive(Serialize)]
struct Diagnostic {
    taxonomy_version: u8,
    phase: Phase,
    severity: Severity,
    category: &'static str,
    continued: bool,
    message: String,
}

impl Diagnostic {
    fn error(phase: Phase, category: &'static str, message: String) -> Self {
        Self {
            taxonomy_version: DIAGNOSTIC_TAXONOMY_VERSION,
            phase,
            severity: Severity::Error,
            category,
            continued: false,
            message,
        }
    }

    fn warning(phase: Phase, category: &'static str, message: String, continued: bool) -> Self {
        Self {
            taxonomy_version: DIAGNOSTIC_TAXONOMY_VERSION,
            phase,
            severity: Severity::Warning,
            category,
            continued,
            message,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum Severity {
    Error,
    Warning,
}

#[derive(Default, Serialize)]
struct ModuleObservation {
    current: String,
    focus: Option<String>,
    focus_stack: Vec<String>,
}

#[derive(Serialize)]
#[allow(clippy::struct_excessive_bools)] // Independent wire-format capability flags are intentional.
struct Capabilities {
    fact_modules: bool,
    fired_rule_names: bool,
    global_values: bool,
    router_channel_enumeration: bool,
    external_address_identity: bool,
    source_digest_verification: bool,
    composed_digest_verification: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_parser_requires_lowercase_canonical_text() {
        let valid = "0123456789abcdef".repeat(4);
        assert_eq!(parse_sha256(&valid), Ok(valid));
        assert!(parse_sha256(&"A".repeat(64)).is_err());
        assert!(parse_sha256(&"a".repeat(63)).is_err());
        assert!(parse_sha256(&format!("{}g", "a".repeat(63))).is_err());
    }

    #[test]
    fn nonce_parser_applies_character_and_length_bounds() {
        let valid = "0123456789abcdef".repeat(2);
        assert_eq!(parse_nonce(&valid), Ok(valid));
        assert!(parse_nonce("").is_err());
        assert!(parse_nonce(&"A".repeat(MIN_NONCE_HEX_BYTES)).is_err());
        assert!(parse_nonce(&"a".repeat(MIN_NONCE_HEX_BYTES - 1)).is_err());
        assert!(parse_nonce(&"a".repeat(MAX_NONCE_HEX_BYTES + 2)).is_err());
    }

    #[test]
    fn fixture_id_parser_rejects_ambiguous_or_unbounded_values() {
        assert_eq!(
            parse_fixture_id("examples/hello-world.clp"),
            Ok("examples/hello-world.clp".to_string())
        );
        assert!(parse_fixture_id("").is_err());
        assert!(parse_fixture_id("contains space").is_err());
        assert!(parse_fixture_id("line\nbreak").is_err());
        assert!(parse_fixture_id(&"a".repeat(MAX_FIXTURE_ID_BYTES + 1)).is_err());
    }

    #[test]
    fn float_text_and_bits_preserve_edge_case_identity() {
        assert_eq!(canonical_float(-0.0), "-0");
        assert_eq!(canonical_float(f64::INFINITY), "Infinity");
        assert_eq!(canonical_float(f64::NEG_INFINITY), "-Infinity");
        assert_eq!(canonical_float(f64::NAN), "NaN");
    }
}
