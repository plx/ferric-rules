//! Dedicated structured observation surface for compatibility tooling.
//!
//! This command deliberately does not share the human-oriented output contract
//! of `ferric run`. It captures the engine's known router channels and emits one
//! versioned JSON document on process stdout.
//!
//! The public runtime APIs do not currently expose the owning module for an
//! ordered fact, an enumeration of router channels or globals, or the names of
//! rules fired by `Engine::run`. Version 1 reports template ownership from the
//! registered template and uses the sole registered module for ordered facts;
//! unresolved ownership remains `null` and is advertised in `capabilities`.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::io::{Read as _, Write as _};
use std::path::{Component, Path, PathBuf};

use ferric_rules_core::{ConflictResolutionStrategy, Fact, Value};
use ferric_rules_runtime::{
    parse_qualified_name, ActionError, Engine, EngineConfig, HaltReason, LoadError, QualifiedName,
    RunLimit, RunResult,
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
const SCENARIO_HEADER: &str = "FERRIC-COMPAT-SCENARIO|1";
const SCENARIO_SOURCE_PREFIX: &str = "tests/examples/";
const MAX_SCENARIO_BYTES: usize = 1 << 20;
const MAX_SCENARIO_LINE_BYTES: usize = 4095;
const MAX_SCENARIO_SOURCES: usize = 64;
const MAX_SCENARIO_STEPS: usize = 256;
const MAX_SCENARIO_TOKEN_BYTES: usize = 128;
const MAX_SCENARIO_PATH_BYTES: usize = MAX_SCENARIO_LINE_BYTES;
const MAX_SOURCE_BYTES: usize = 16 << 20;
const MAX_TOTAL_SOURCE_BYTES: usize = 64 << 20;
const KNOWN_CHANNELS: &[&str] = &[
    "t", "stdin", "stdout", "stderr", "wclips", "wdialog", "wdisplay", "werror", "wtrace",
    "wwarning",
];

#[derive(Clone, Copy)]
enum FailurePolicy {
    Stop,
    Continue,
}

struct ScenarioSourceDeclaration {
    name: String,
    sha256: String,
    path: PathBuf,
}

struct ScenarioSource {
    text: String,
}

enum ScenarioStep {
    Load {
        source_index: usize,
        policy: FailurePolicy,
    },
    Reset {
        policy: FailurePolicy,
    },
    SetStrategy,
    Run,
}

struct ScenarioPlan {
    sources: Vec<ScenarioSourceDeclaration>,
    steps: Vec<ScenarioStep>,
    strategy: ConflictResolutionStrategy,
}

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

fn read_bounded(path: &Path, maximum_bytes: usize, label: &str) -> Result<Vec<u8>, String> {
    let maximum_bytes_u64 = u64::try_from(maximum_bytes).expect("scenario byte limits fit in u64");
    let path_metadata = path
        .symlink_metadata()
        .map_err(|error| format!("failed to inspect {label} {}: {error}", path.display()))?;
    if path_metadata.file_type().is_symlink() || !path_metadata.is_file() {
        return Err(format!("{label} {} must be a regular file", path.display()));
    }
    if path_metadata.len() > maximum_bytes_u64 {
        return Err(format!(
            "{label} {} exceeds the {maximum_bytes}-byte limit",
            path.display()
        ));
    }
    let file = std::fs::File::open(path)
        .map_err(|error| format!("failed to open {label} {}: {error}", path.display()))?;
    let opened_metadata = file
        .metadata()
        .map_err(|error| format!("failed to inspect open {label} {}: {error}", path.display()))?;
    if !opened_metadata.is_file() || opened_metadata.len() > maximum_bytes_u64 {
        return Err(format!(
            "{label} {} changed or exceeds the {maximum_bytes}-byte limit",
            path.display()
        ));
    }
    let limit = maximum_bytes_u64.saturating_add(1);
    let mut bytes = Vec::new();
    file.take(limit)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("failed to read {label} {}: {error}", path.display()))?;
    if bytes.len() > maximum_bytes {
        return Err(format!(
            "{label} {} exceeds the {maximum_bytes}-byte limit",
            path.display()
        ));
    }
    Ok(bytes)
}

fn validate_scenario_token(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > MAX_SCENARIO_TOKEN_BYTES {
        return Err(format!(
            "{label} must contain 1 to {MAX_SCENARIO_TOKEN_BYTES} ASCII bytes"
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
        return Err(format!(
            "{label} must start with an ASCII letter or digit and contain only ASCII letters, digits, hyphen, underscore, dot, colon, and slash"
        ));
    }
    Ok(())
}

fn validate_repo_relative_path(value: &str) -> Result<PathBuf, String> {
    if value.is_empty() || value.len() > MAX_SCENARIO_PATH_BYTES {
        return Err(format!(
            "source path must contain 1 to {MAX_SCENARIO_PATH_BYTES} UTF-8 bytes"
        ));
    }
    if value.contains(['|', '\\']) || value.bytes().any(|byte| byte < b' ' || byte == b'\x7f') {
        return Err(
            "source path must not contain `|`, backslash, or control characters".to_string(),
        );
    }

    let path = PathBuf::from(value);
    let has_noncanonical_segment = value
        .split('/')
        .any(|segment| segment.is_empty() || matches!(segment, "." | ".."));
    if path.is_absolute()
        || !value.starts_with(SCENARIO_SOURCE_PREFIX)
        || has_noncanonical_segment
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(
            format!(
                "source path must be a normalized repo-relative POSIX path below `{SCENARIO_SOURCE_PREFIX}` without `.` or `..`"
            ),
        );
    }
    Ok(path)
}

fn parse_failure_policy(value: &str) -> Result<FailurePolicy, String> {
    match value {
        "stop" => Ok(FailurePolicy::Stop),
        "continue" => Ok(FailurePolicy::Continue),
        _ => Err("failure policy must be exactly `stop` or `continue`".to_string()),
    }
}

fn parse_strategy(value: &str) -> Result<ConflictResolutionStrategy, String> {
    match value {
        "depth" => Ok(ConflictResolutionStrategy::Depth),
        "breadth" => Ok(ConflictResolutionStrategy::Breadth),
        "lex" => Ok(ConflictResolutionStrategy::Lex),
        "mea" => Ok(ConflictResolutionStrategy::Mea),
        _ => Err("strategy must be exactly `depth`, `breadth`, `lex`, or `mea`".to_string()),
    }
}

fn scenario_line_error(line: usize, message: impl AsRef<str>) -> String {
    format!("invalid scenario line {line}: {}", message.as_ref())
}

#[allow(clippy::too_many_lines)] // The strict record grammar is kept linear and auditable.
fn parse_scenario_plan(bytes: &[u8], expected_source_sha256: &str) -> Result<ScenarioPlan, String> {
    if bytes.is_empty() || !bytes.ends_with(b"\n") {
        return Err("scenario must end with a single LF-delimited record".to_string());
    }
    if bytes.contains(&b'\r') {
        return Err("scenario must use LF line endings; CR bytes are forbidden".to_string());
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|error| format!("scenario is not valid UTF-8: {error}"))?;
    let body = text
        .strip_suffix('\n')
        .expect("the final LF was checked above");
    let lines = body.split('\n').collect::<Vec<_>>();
    if lines.first().copied() != Some(SCENARIO_HEADER) {
        return Err(format!("scenario must begin with `{SCENARIO_HEADER}`"));
    }
    if lines.last().copied() != Some("END") {
        return Err("scenario must end with an exact `END` record".to_string());
    }

    let mut sources = Vec::new();
    let mut source_indexes = HashMap::new();
    let mut source_paths = HashSet::new();
    let mut steps = Vec::new();
    let mut saw_step = false;
    let mut saw_strategy = false;
    let mut saw_load = false;
    let mut saw_reset = false;
    let mut loaded_primary = false;
    let mut strategy = ConflictResolutionStrategy::Depth;

    for (zero_based_index, line) in lines
        .iter()
        .enumerate()
        .skip(1)
        .take(lines.len().saturating_sub(2))
    {
        let line_number = zero_based_index + 1;
        if line.is_empty() {
            return Err(scenario_line_error(
                line_number,
                "empty records are forbidden",
            ));
        }
        if line.len() > MAX_SCENARIO_LINE_BYTES {
            return Err(scenario_line_error(
                line_number,
                format!("record exceeds the {MAX_SCENARIO_LINE_BYTES}-byte limit"),
            ));
        }
        let fields = line.split('|').collect::<Vec<_>>();
        match fields.first().copied() {
            Some("SOURCE") => {
                if saw_step {
                    return Err(scenario_line_error(
                        line_number,
                        "SOURCE records must precede every STEP record",
                    ));
                }
                if fields.len() != 4 {
                    return Err(scenario_line_error(
                        line_number,
                        "SOURCE must have name, sha256, and repo-relative path fields",
                    ));
                }
                if sources.len() >= MAX_SCENARIO_SOURCES {
                    return Err(scenario_line_error(
                        line_number,
                        format!("more than {MAX_SCENARIO_SOURCES} SOURCE records"),
                    ));
                }
                validate_scenario_token(fields[1], "source name")
                    .map_err(|message| scenario_line_error(line_number, message))?;
                let sha256 = parse_sha256(fields[2])
                    .map_err(|message| scenario_line_error(line_number, message))?;
                let path = validate_repo_relative_path(fields[3])
                    .map_err(|message| scenario_line_error(line_number, message))?;
                if source_indexes.contains_key(fields[1]) {
                    return Err(scenario_line_error(
                        line_number,
                        format!("duplicate source name `{}`", fields[1]),
                    ));
                }
                if !source_paths.insert(path.clone()) {
                    return Err(scenario_line_error(
                        line_number,
                        format!("duplicate source path `{}`", fields[3]),
                    ));
                }
                let source_index = sources.len();
                source_indexes.insert(fields[1].to_string(), source_index);
                sources.push(ScenarioSourceDeclaration {
                    name: fields[1].to_string(),
                    sha256,
                    path,
                });
            }
            Some("STEP") => {
                saw_step = true;
                if fields.len() != 5 {
                    return Err(scenario_line_error(
                        line_number,
                        "STEP must have sequence, operation, argument, and policy fields",
                    ));
                }
                if steps.len() >= MAX_SCENARIO_STEPS {
                    return Err(scenario_line_error(
                        line_number,
                        format!("more than {MAX_SCENARIO_STEPS} STEP records"),
                    ));
                }
                let expected_sequence = steps.len() + 1;
                if fields[1] != expected_sequence.to_string() {
                    return Err(scenario_line_error(
                        line_number,
                        format!("STEP sequence must be canonical contiguous decimal `{expected_sequence}`"),
                    ));
                }
                let step = match fields[2] {
                    "LOAD" => {
                        validate_scenario_token(fields[3], "LOAD source name")
                            .map_err(|message| scenario_line_error(line_number, message))?;
                        let source_index =
                            source_indexes.get(fields[3]).copied().ok_or_else(|| {
                                scenario_line_error(
                                    line_number,
                                    format!("LOAD references undeclared source `{}`", fields[3]),
                                )
                            })?;
                        saw_load = true;
                        loaded_primary |= source_index == 0;
                        ScenarioStep::Load {
                            source_index,
                            policy: parse_failure_policy(fields[4])
                                .map_err(|message| scenario_line_error(line_number, message))?,
                        }
                    }
                    "RESET" => {
                        if fields[3] != "-" {
                            return Err(scenario_line_error(
                                line_number,
                                "RESET argument must be exactly `-`",
                            ));
                        }
                        if !saw_load {
                            return Err(scenario_line_error(
                                line_number,
                                "RESET must follow a LOAD step",
                            ));
                        }
                        saw_reset = true;
                        ScenarioStep::Reset {
                            policy: parse_failure_policy(fields[4])
                                .map_err(|message| scenario_line_error(line_number, message))?,
                        }
                    }
                    "SET-STRATEGY" => {
                        if saw_strategy {
                            return Err(scenario_line_error(
                                line_number,
                                "at most one SET-STRATEGY step is permitted",
                            ));
                        }
                        if fields[4] != "stop" {
                            return Err(scenario_line_error(
                                line_number,
                                "SET-STRATEGY policy must be exactly `stop`",
                            ));
                        }
                        strategy = parse_strategy(fields[3])
                            .map_err(|message| scenario_line_error(line_number, message))?;
                        saw_strategy = true;
                        ScenarioStep::SetStrategy
                    }
                    "RUN" => {
                        if fields[3] != "-1" || fields[4] != "stop" {
                            return Err(scenario_line_error(
                                line_number,
                                "RUN must be exactly `RUN|-1|stop`",
                            ));
                        }
                        ScenarioStep::Run
                    }
                    operation => {
                        return Err(scenario_line_error(
                            line_number,
                            format!("unsupported STEP operation `{operation}`"),
                        ));
                    }
                };
                steps.push(step);
            }
            Some(record) => {
                return Err(scenario_line_error(
                    line_number,
                    format!("unsupported record type `{record}`"),
                ));
            }
            None => unreachable!("split always returns at least one field"),
        }
    }

    let primary = sources
        .first()
        .ok_or_else(|| "scenario must declare at least one SOURCE".to_string())?;
    if primary.name != "primary" {
        return Err("the first SOURCE is primary and must be named exactly `primary`".to_string());
    }
    if primary.sha256 != expected_source_sha256 {
        return Err(format!(
            "primary source digest mismatch: invocation binds {expected_source_sha256}, scenario declares {}",
            primary.sha256
        ));
    }
    if !matches!(steps.last(), Some(ScenarioStep::Run))
        || steps
            .iter()
            .filter(|step| matches!(step, ScenarioStep::Run))
            .count()
            != 1
    {
        return Err("scenario must contain exactly one final RUN step".to_string());
    }
    if !saw_load {
        return Err("scenario must contain at least one LOAD step".to_string());
    }
    if !saw_reset {
        return Err("scenario must contain a RESET step after a LOAD step".to_string());
    }
    if !loaded_primary {
        return Err("scenario must load the primary source".to_string());
    }

    Ok(ScenarioPlan {
        sources,
        steps,
        strategy,
    })
}

fn read_scenario_sources(plan: &ScenarioPlan) -> Result<Vec<ScenarioSource>, String> {
    let repo_root = std::env::current_dir()
        .map_err(|error| format!("failed to determine repository root: {error}"))?
        .canonicalize()
        .map_err(|error| format!("failed to resolve repository root: {error}"))?;
    let examples_root = repo_root
        .join(SCENARIO_SOURCE_PREFIX.trim_end_matches('/'))
        .canonicalize()
        .map_err(|error| format!("failed to resolve canonical scenario source root: {error}"))?;
    if !examples_root.starts_with(&repo_root) {
        return Err("canonical scenario source root escapes the repository root".to_string());
    }
    let mut total_bytes = 0usize;
    let mut sources = Vec::with_capacity(plan.sources.len());

    for declaration in &plan.sources {
        let requested_path = repo_root.join(&declaration.path);
        let symlink_metadata = requested_path.symlink_metadata().map_err(|error| {
            format!(
                "failed to inspect source `{}` at {}: {error}",
                declaration.name,
                declaration.path.display()
            )
        })?;
        if symlink_metadata.file_type().is_symlink() {
            return Err(format!(
                "source `{}` must not be a symbolic link",
                declaration.name
            ));
        }
        let resolved_path = requested_path.canonicalize().map_err(|error| {
            format!(
                "failed to resolve source `{}` at {}: {error}",
                declaration.name,
                declaration.path.display()
            )
        })?;
        if !resolved_path.starts_with(&repo_root) || !resolved_path.starts_with(&examples_root) {
            return Err(format!(
                "source `{}` resolves outside the canonical scenario source root",
                declaration.name
            ));
        }
        if !symlink_metadata.is_file() {
            return Err(format!(
                "source `{}` is not a regular file",
                declaration.name
            ));
        }
        let remaining_bytes = MAX_TOTAL_SOURCE_BYTES.saturating_sub(total_bytes);
        let source_limit = MAX_SOURCE_BYTES.min(remaining_bytes);
        let bytes = read_bounded(&resolved_path, source_limit, "scenario source").map_err(
            |error| {
                if source_limit < MAX_SOURCE_BYTES {
                    format!(
                        "scenario sources exceed the {MAX_TOTAL_SOURCE_BYTES}-byte aggregate limit: {error}"
                    )
                } else {
                    error
                }
            },
        )?;
        total_bytes += bytes.len();
        let observed_sha256 = lowercase_sha256(&bytes);
        if observed_sha256 != declaration.sha256 {
            return Err(format!(
                "source `{}` digest mismatch: expected {}, observed {observed_sha256}",
                declaration.name, declaration.sha256
            ));
        }
        let text = String::from_utf8(bytes).map_err(|error| {
            format!("source `{}` is not valid UTF-8: {error}", declaration.name)
        })?;
        sources.push(ScenarioSource { text });
    }
    Ok(sources)
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

/// Execute a strict multi-step compatibility scenario in one engine.
#[allow(clippy::too_many_lines)] // Each protocol step is intentionally handled at the boundary.
pub fn execute_scenario(
    scenario_path: &Path,
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
    observation.phase_reached = Phase::Load;
    observation.capabilities.source_digest_verification = true;

    let plan_bytes = match read_bounded(scenario_path, MAX_SCENARIO_BYTES, "scenario plan") {
        Ok(bytes) => bytes,
        Err(error) => return emit_scenario_harness_failure(observation, error),
    };
    let actual_composed_sha256 = lowercase_sha256(&plan_bytes);
    if actual_composed_sha256 != observation.fixture.composed_sha256 {
        let message = format!(
            "scenario plan digest mismatch: expected {}, observed {actual_composed_sha256}",
            observation.fixture.composed_sha256
        );
        return emit_scenario_harness_failure(observation, message);
    }

    let plan = match parse_scenario_plan(&plan_bytes, &observation.fixture.source_sha256) {
        Ok(plan) => plan,
        Err(error) => return emit_scenario_harness_failure(observation, error),
    };
    let sources = match read_scenario_sources(&plan) {
        Ok(sources) => sources,
        Err(error) => return emit_scenario_harness_failure(observation, error),
    };

    // The runtime does not expose an agenda-strategy setter. Because the plan
    // permits at most one strategy selection and RUN is uniquely final,
    // configuring the sole Engine before replay is semantically equivalent.
    let mut engine = Engine::new(EngineConfig::default().with_strategy(plan.strategy));
    for step in plan.steps {
        match step {
            ScenarioStep::Load {
                source_index,
                policy,
            } => {
                observation.phase_reached = Phase::Load;
                match engine.load_str(&sources[source_index].text) {
                    Ok(result) => observation
                        .diagnostics
                        .extend(result.warnings.into_iter().map(|message| {
                            Diagnostic::warning(Phase::Load, "construct-error", message, true)
                        })),
                    Err(errors) => {
                        let continued = matches!(policy, FailurePolicy::Continue);
                        observation.phase_reached = load_failure_phase(&errors);
                        observation.diagnostics.extend(errors.iter().map(|error| {
                            load_error_diagnostic_with_continuation(error, continued)
                        }));
                        if !continued {
                            return emit_failed_observation(observation, &engine);
                        }
                    }
                }
            }
            ScenarioStep::Reset { policy } => {
                observation.phase_reached = Phase::Reset;
                if let Err(error) = engine.reset() {
                    let continued = matches!(policy, FailurePolicy::Continue);
                    observation
                        .diagnostics
                        .push(Diagnostic::error_with_continuation(
                            Phase::Reset,
                            "evaluation-error",
                            format!("reset failed: {error}"),
                            continued,
                        ));
                    if !continued {
                        return emit_failed_observation(observation, &engine);
                    }
                }
            }
            ScenarioStep::SetStrategy => {}
            ScenarioStep::Run => {
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

                observation.phase_reached =
                    if matches!(run_result.halt_reason, HaltReason::ActionError) {
                        Phase::Run
                    } else {
                        Phase::PostRun
                    };
                return match capture_state(&engine, Some(run_result)) {
                    Ok(state) => {
                        observation.apply_capture(state);
                        observation
                            .lifecycle
                            .push(LifecycleRecord::complete(&observation.fixture));
                        emit_observation(&observation, 0)
                    }
                    Err(error) => {
                        observation.diagnostics.push(Diagnostic::error(
                            Phase::Harness,
                            "harness-error",
                            error,
                        ));
                        emit_observation(&observation, 1)
                    }
                };
            }
        }
    }

    unreachable!("validated scenarios contain one final RUN step")
}

fn emit_scenario_harness_failure(mut observation: Observation, message: String) -> i32 {
    observation
        .diagnostics
        .push(Diagnostic::error(Phase::Harness, "harness-error", message));
    let engine = Engine::new(EngineConfig::default());
    emit_failed_observation(observation, &engine)
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
    let ordered_fact_module = (module_names.len() == 1).then_some(module_names[0]);
    let facts = engine
        .facts()
        .map_err(|error| format!("failed to enumerate facts: {error}"))?
        .enumerate()
        .map(|(ordinal, (fact_id, fact))| {
            observe_fact(
                engine,
                ordinal,
                fact_id.data().as_ffi().to_string(),
                ordered_fact_module,
                fact,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let fact_modules = !facts.is_empty() && facts.iter().all(FactObservation::has_module);

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
        fact_modules,
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
            let registered_name = engine
                .template_name_by_id(template.template_id)
                .ok_or_else(|| format!("template fact {fact_id} has an unknown template id"))?;
            let owning_module = engine
                .template_module_name_by_id(template.template_id)
                .or(module)
                .ok_or_else(|| format!("template fact {fact_id} has no owning module"))?;
            let parsed_name = parse_qualified_name(registered_name).map_err(|error| {
                format!("template fact {fact_id} has an invalid registered name: {error}")
            })?;
            if let Some(qualified_module) = parsed_name.module_name() {
                if qualified_module != owning_module {
                    return Err(format!(
                        "template fact {fact_id} name names module {qualified_module} but metadata names {owning_module}"
                    ));
                }
            }
            let local_name = match &parsed_name {
                QualifiedName::Unqualified(name) | QualifiedName::Qualified { name, .. } => name,
            };
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
                module: Some(owning_module.to_string()),
                name: local_name.clone(),
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
    load_error_diagnostic_with_continuation(error, false)
}

fn load_error_diagnostic_with_continuation(error: &LoadError, continued: bool) -> Diagnostic {
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
    Diagnostic::error_with_continuation(phase, category, error.to_string(), continued)
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

impl FactObservation {
    fn has_module(&self) -> bool {
        match self {
            Self::Ordered { module, .. } | Self::Template { module, .. } => module.is_some(),
        }
    }
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
        Self::error_with_continuation(phase, category, message, false)
    }

    fn error_with_continuation(
        phase: Phase,
        category: &'static str,
        message: String,
        continued: bool,
    ) -> Self {
        Self {
            taxonomy_version: DIAGNOSTIC_TAXONOMY_VERSION,
            phase,
            severity: Severity::Error,
            category,
            continued,
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

    #[test]
    fn scenario_parser_enforces_canonical_lifecycle_and_bounds() {
        let digest = "0".repeat(64);
        let canonical = format!(
            "{SCENARIO_HEADER}\n\
             SOURCE|primary|{digest}|tests/examples/primary.clp\n\
             STEP|1|LOAD|primary|stop\n\
             STEP|2|RESET|-|continue\n\
             STEP|3|SET-STRATEGY|mea|stop\n\
             STEP|4|RUN|-1|stop\n\
             END\n"
        );
        let parsed = parse_scenario_plan(canonical.as_bytes(), &digest).expect("canonical plan");
        assert_eq!(parsed.sources.len(), 1);
        assert_eq!(parsed.steps.len(), 4);
        assert_eq!(parsed.strategy, ConflictResolutionStrategy::Mea);

        assert!(parse_scenario_plan(canonical.trim_end().as_bytes(), &digest).is_err());
        assert!(parse_scenario_plan(canonical.replace('\n', "\r\n").as_bytes(), &digest).is_err());
        assert!(parse_scenario_plan(
            canonical
                .replace("SOURCE|primary|", "SOURCE|not-primary|")
                .as_bytes(),
            &digest,
        )
        .is_err());
        assert!(parse_scenario_plan(
            canonical
                .replace("STEP|2|RESET|-|continue\n", "")
                .replace("STEP|3|", "STEP|2|")
                .replace("STEP|4|", "STEP|3|")
                .as_bytes(),
            &digest,
        )
        .is_err());

        let oversized_token = "a".repeat(MAX_SCENARIO_TOKEN_BYTES + 1);
        assert!(parse_scenario_plan(
            canonical
                .replace("SOURCE|primary|", &format!("SOURCE|{oversized_token}|"))
                .as_bytes(),
            &digest,
        )
        .is_err());

        let mut too_many_steps =
            format!("{SCENARIO_HEADER}\nSOURCE|primary|{digest}|tests/examples/primary.clp\n");
        for sequence in 1..MAX_SCENARIO_STEPS {
            writeln!(too_many_steps, "STEP|{sequence}|LOAD|primary|stop").expect("write test plan");
        }
        writeln!(too_many_steps, "STEP|{MAX_SCENARIO_STEPS}|RESET|-|stop")
            .expect("write test plan");
        writeln!(
            too_many_steps,
            "STEP|{}|RUN|-1|stop",
            MAX_SCENARIO_STEPS + 1
        )
        .expect("write test plan");
        too_many_steps.push_str("END\n");
        assert!(parse_scenario_plan(too_many_steps.as_bytes(), &digest).is_err());
    }

    #[test]
    fn bounded_reader_rejects_an_oversized_regular_file_before_allocation() {
        let file = tempfile::NamedTempFile::new().expect("create sparse test file");
        file.as_file()
            .set_len(u64::try_from(MAX_SOURCE_BYTES + 1).expect("source limit fits in u64"))
            .expect("extend sparse test file");

        let error = read_bounded(file.path(), MAX_SOURCE_BYTES, "scenario source")
            .expect_err("oversized source must fail closed");
        assert!(error.contains("exceeds"));
    }
}
