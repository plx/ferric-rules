//! Data-driven, reference-verified CLIPS programs. Known gaps are active
//! characterization assertions: neither a new failure nor an unexpected fix is
//! silently accepted. See `tests/clips_compat/corpus/README.md`.

mod host;

use ferric_rules::core::ConflictResolutionStrategy;
use ferric_rules::runtime::{
    Engine, EngineConfig, HaltReason, RunLimit, SerializationError, SerializationFormat,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const RUN_LIMIT: usize = 1_000;
/// Restore replays snapshot the engine before its first firing. With CBOR,
/// the persistence format, and rules loaded up front, they also snapshot
/// before each later firing up to this bound, which keeps the few large
/// programs fast in debug builds.
const RESTORED_FIRINGS: usize = 12;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    reference: serde_json::Value,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(clippy::struct_excessive_bools)] // Independent manifest opt-ins.
struct Case {
    path: String,
    level: String,
    covers: Vec<String>,
    #[serde(default = "one")]
    resets: usize,
    #[serde(default)]
    strategy: Option<Strategy>,
    #[serde(default)]
    error: Option<ErrorPhase>,
    #[serde(default)]
    recoverable_fact_notices: bool,
    #[serde(default)]
    recoverable_control_notices: bool,
    #[serde(default)]
    recoverable_random_notices: bool,
    #[serde(default)]
    recoverable_build_notices: bool,
    #[serde(default)]
    recoverable_introspection_notices: bool,
    #[serde(default)]
    gap: Option<Gap>,
}

/// A conflict resolution strategy other than CLIPS's default depth.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Strategy {
    Breadth,
    Lex,
    Mea,
}

const fn one() -> usize {
    1
}

/// Where CLIPS reports an error for this program. Diagnostic text is
/// CLIPS-specific, so Ferric's own messages are never compared with it.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum ErrorPhase {
    /// CLIPS rejects the program; the golden is its load diagnostic. Ferric
    /// must reject the program at load too.
    Load,
    /// CLIPS halts on a run-time error. Ferric must report an error and
    /// print the golden without CLIPS's diagnostic lines.
    Run,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Gap {
    issues: Vec<String>,
    summary: String,
    observed: Observation,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Observation {
    phase: String,
    output: String,
    /// `[SCANNER1]` notices, as written to `wwarning` then `werror`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    notices: String,
    diagnostics: Vec<String>,
}

impl Observation {
    fn failed(mut self, phase: &str, diagnostic: String) -> Self {
        self.phase = phase.into();
        self.diagnostics.push(diagnostic);
        self
    }
}

/// CLIPS scanner notices. CLIPS writes them to its warning and error routers,
/// which share stdout with `t` in the reference run; Ferric keeps routers apart.
const NOTICES: [(&str, &str); 2] = [
    (
        "wwarning",
        "[SCANNER1] WARNING: Over or underflow of long long integer.\n",
    ),
    (
        "werror",
        "\n[SCANNER1] Encountered End-Of-File while scanning a string\n",
    ),
];

/// A CLIPS golden split into `t` output and its notices (warnings, then
/// errors). Bytes stay exact: a golden need not be valid UTF-8.
struct Golden {
    output: Vec<u8>,
    notices: Vec<u8>,
}

fn golden(
    bytes: &[u8],
    error: Option<ErrorPhase>,
    recoverable_fact_notices: bool,
    recoverable_control_notices: bool,
    recoverable_random_notices: bool,
) -> Golden {
    let mut output = Vec::new();
    let mut found: [Vec<u8>; 2] = Default::default();
    let mut rest = bytes;
    'scan: while let Some((&first, tail)) = rest.split_first() {
        if recoverable_random_notices {
            if let Some(length) = random_notice_length(rest) {
                rest = &rest[length..];
                continue 'scan;
            }
        }
        if recoverable_fact_notices {
            if let Some(length) = fact_notice_length(rest) {
                rest = &rest[length..];
                continue 'scan;
            }
        }
        if recoverable_control_notices {
            if let Some(length) = control_notice_length(rest) {
                rest = &rest[length..];
                continue 'scan;
            }
        }
        for (index, (_, notice)) in NOTICES.iter().enumerate() {
            if let Some(after) = rest.strip_prefix(notice.as_bytes()) {
                found[index].extend_from_slice(notice.as_bytes());
                rest = after;
                continue 'scan;
            }
        }
        output.push(first);
        rest = tail;
    }
    if error == Some(ErrorPhase::Run) {
        // These parser diagnostics start with their own newline even when eval
        // or assert-string is called midway through a printout. Remove exactly
        // that newline, preserving any preceding newline printed by the program.
        const PARSER_PREFIXES: &[&[u8]] = &[b"[EXPRNPSR3] ", b"[PRCDRPSR2] ", b"[PRNTUTIL2] "];
        output = output
            .iter()
            .enumerate()
            .filter_map(|(index, &byte)| {
                (!(byte == b'\n'
                    && PARSER_PREFIXES
                        .iter()
                        .any(|prefix| output[index + 1..].starts_with(prefix))))
                .then_some(byte)
            })
            .collect();
        output = output
            .split_inclusive(|&byte| byte == b'\n')
            .flat_map(|line| &line[..diagnostic_offset(line).unwrap_or(line.len())])
            .copied()
            .collect();
    }
    let [warnings, errors] = found;
    Golden {
        output,
        notices: [warnings, errors].concat(),
    }
}

/// Only these exact recoverable fact-designator notices may be omitted. In
/// particular, fatal slot/operand errors and other bracketed output stay intact.
fn fact_notice_length(bytes: &[u8]) -> Option<usize> {
    fn digits_then(bytes: &[u8], suffix: &[u8], nonzero: bool) -> bool {
        let count = bytes
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .count();
        count > 0 && (!nonzero || bytes[0] != b'0') && &bytes[count..] == suffix
    }
    let length = bytes.iter().position(|&byte| byte == b'\n')? + 1;
    let line = &bytes[..length];
    if let Some(index) = line.strip_prefix(b"[PRNTUTIL1] Unable to find fact f-") {
        return digits_then(index, b".\n", false).then_some(length);
    }
    let function = line.strip_prefix(b"[ARGACCES5] Function ")?;
    if function == b"fact-index expected argument #1 to be of type fact-address\n" {
        return Some(length);
    }
    if let Some(argument) = function.strip_prefix(b"retract expected argument #") {
        return digits_then(
            argument,
            b" to be of type fact-address, fact-index, or the symbol *\n",
            true,
        )
        .then_some(length);
    }
    [
        "fact-existp",
        "fact-relation",
        "fact-slot-names",
        "fact-slot-value",
    ]
    .iter()
    .any(|name| {
        function.strip_prefix(name.as_bytes())
            == Some(b" expected argument #1 to be of type fact-address or fact-index\n".as_slice())
    })
    .then_some(length)
}

const RANDOM_NOTICE: &[u8] =
    b"[MISCFUN3] Function random expected argument #1 to be less than argument #2\n";

const RANDOM_ARITY_NOTICE: &[u8] = b"[MISCFUN2] Function random expected either 0 or 2 arguments\n";

fn random_notice_length(bytes: &[u8]) -> Option<usize> {
    [RANDOM_NOTICE, RANDOM_ARITY_NOTICE]
        .into_iter()
        .find(|notice| bytes.starts_with(notice))
        .map(<[u8]>::len)
}

fn control_notice_length(bytes: &[u8]) -> Option<usize> {
    const MODULE: &[u8] = b"[PRNTUTIL1] Unable to find defmodule ";
    const CLEAR: &[u8] = b"[CONSTRCT1] Some constructs are still in use. Clear cannot continue.\n";
    if bytes.starts_with(CLEAR) {
        return Some(CLEAR.len());
    }
    let name = bytes.strip_prefix(MODULE)?;
    let end = name.iter().position(|&byte| byte == b'\n')?;
    let name = name[..end].strip_suffix(b".")?;
    (!name.is_empty()
        && name
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_:-".contains(byte)))
    .then_some(MODULE.len() + end + 1)
}

/// CLIPS rejects a `build` that would redefine a deftemplate in use with
/// CSTRCPSR4 and echoes the construct up to its module-qualified name.
/// Return the block's length and the deftemplate name.
fn build_notice(bytes: &[u8]) -> Option<(usize, &[u8])> {
    const MESSAGE: &[u8] = b"\n[CSTRCPSR4] Cannot redefine deftemplate ";
    const ECHO: &[u8] = b" while it is in use.\n\nERROR:\n(deftemplate ";
    let is_name = |name: &[u8]| {
        !name.is_empty()
            && name
                .iter()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(byte))
    };
    let rest = bytes.strip_prefix(MESSAGE)?;
    let name = &rest[..rest.iter().position(|&byte| byte == b' ')?];
    let echo = rest[name.len()..].strip_prefix(ECHO)?;
    let line = &echo[..echo.iter().position(|&byte| byte == b'\n')?];
    let module = line.strip_suffix(name)?.strip_suffix(b"::")?;
    (is_name(name) && is_name(module)).then_some((
        MESSAGE.len() + name.len() + ECHO.len() + line.len() + 1,
        name,
    ))
}

/// The notice a recoverable build rejection is compared as.
fn build_notice_line(name: &[u8]) -> Vec<u8> {
    [
        b"[CSTRCPSR4] Cannot redefine deftemplate ".as_slice(),
        name,
        b" while it is in use.\n",
    ]
    .concat()
}

/// Move CLIPS's recoverable build rejections out of a golden: return the
/// golden without them and their notices, in order.
fn split_build_notices(bytes: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let (mut output, mut notices) = (Vec::new(), Vec::new());
    let mut rest = bytes;
    while let Some((&first, tail)) = rest.split_first() {
        if let Some((length, name)) = build_notice(rest) {
            notices.extend(build_notice_line(name));
            rest = &rest[length..];
        } else {
            output.push(first);
            rest = tail;
        }
    }
    (output, notices)
}

/// Ferric reports a rejected `build` with its own load error text, one line
/// per rejection. Compare each such line as CLIPS's notice for the same
/// deftemplate, so the count and names must still agree.
fn normalize_ferric_build_notices(notices: &str) -> String {
    fn rejected_name(line: &str) -> Option<&str> {
        let rest = line.strip_prefix("compile error: ")?;
        let (rest, suffix) = match rest.strip_prefix("[CSTRCPSR4] cannot redefine template `") {
            Some(rest) => (rest, "` while it is in use by facts or constructs at line "),
            None => (
                rest.strip_prefix("cannot define template `")?,
                "` while its ordered relation is in use by facts or constructs at line ",
            ),
        };
        let (name, location) = rest.split_once(suffix)?;
        let (line, column) = location.strip_suffix('\n')?.split_once(", column ")?;
        let number = |text: &str| !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
        (!name.is_empty() && !name.contains(char::is_whitespace) && number(line) && number(column))
            .then_some(name)
    }
    notices
        .split_inclusive('\n')
        .map(|line| {
            rejected_name(line).map_or_else(
                || line.to_owned(),
                |name| String::from_utf8(build_notice_line(name.as_bytes())).unwrap(),
            )
        })
        .collect()
}

/// CLIPS's recoverable template and construct introspection notices: a
/// missing deftemplate, a first argument that does not name a deftemplate or
/// defmodule, or a `funcall` name that reaches no visible function. Return the
/// notice's length.
fn introspection_notice_length(bytes: &[u8]) -> Option<usize> {
    const TEMPLATE: &[u8] = b"[PRNTUTIL1] Unable to find deftemplate ";
    const ARGUMENT: &[u8] = b"[ARGACCES5] Function ";
    const TEMPLATE_QUERIES: [&str; 10] = [
        "deftemplate-slot-names",
        "deftemplate-slot-allowed-values",
        "deftemplate-slot-types",
        "deftemplate-slot-default-value",
        "deftemplate-slot-defaultp",
        "deftemplate-slot-existp",
        "deftemplate-slot-multip",
        "deftemplate-slot-singlep",
        "deftemplate-slot-range",
        "deftemplate-slot-cardinality",
    ];
    const CONSTRUCT_LISTS: [&str; 3] = [
        "get-defrule-list",
        "get-deftemplate-list",
        "get-defglobal-list",
    ];
    let length = bytes.iter().position(|&byte| byte == b'\n')? + 1;
    let line = &bytes[..length];
    if let Some(name) = line.strip_prefix(TEMPLATE) {
        let name = name.strip_suffix(b".\n")?;
        return (!name.is_empty()
            && name
                .iter()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_:-".contains(byte)))
        .then_some(length);
    }
    let function = line.strip_prefix(ARGUMENT)?;
    let expected = |functions: &[&str], kind: &[u8]| {
        functions.iter().any(|name| {
            function
                .strip_prefix(name.as_bytes())
                .and_then(|rest| rest.strip_prefix(b" expected argument #1 to be of type "))
                .and_then(|rest| rest.strip_suffix(b" name\n"))
                == Some(kind)
        })
    };
    (expected(&TEMPLATE_QUERIES, b"deftemplate")
        || expected(&CONSTRUCT_LISTS, b"defmodule")
        || expected(&["funcall"], b"function, deffunction, or generic function"))
    .then_some(length)
}

/// Move CLIPS's recoverable introspection notices out of a golden: return the
/// golden without them and the notices, in order. Ferric prints them on
/// `werror`, so they are compared exactly.
fn split_introspection_notices(bytes: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let (mut output, mut notices) = (Vec::new(), Vec::new());
    let mut rest = bytes;
    while let Some((&first, tail)) = rest.split_first() {
        if let Some(length) = introspection_notice_length(rest) {
            notices.extend_from_slice(&rest[..length]);
            rest = &rest[length..];
        } else {
            output.push(first);
            rest = tail;
        }
    }
    (output, notices)
}

/// The golden a corpus case's program is compared with.
fn case_golden(case: &Case, bytes: &[u8], error: Option<ErrorPhase>) -> Golden {
    let (bytes, build_notices) = if case.recoverable_build_notices {
        split_build_notices(bytes)
    } else {
        (bytes.to_vec(), Vec::new())
    };
    let (bytes, introspection_notices) = if case.recoverable_introspection_notices {
        split_introspection_notices(&bytes)
    } else {
        (bytes, Vec::new())
    };
    let mut expected = golden(
        &bytes,
        error,
        case.recoverable_fact_notices,
        case.recoverable_control_notices,
        case.recoverable_random_notices,
    );
    expected.notices.extend(build_notices);
    expected.notices.extend(introspection_notices);
    expected
}

fn strip_control_notices(bytes: &[u8]) -> Vec<u8> {
    let mut output = Vec::new();
    let mut rest = bytes;
    while let Some((&first, tail)) = rest.split_first() {
        if let Some(length) = control_notice_length(rest) {
            rest = &rest[length..];
        } else {
            output.push(first);
            rest = tail;
        }
    }
    output
}

/// CLIPS may append an error to a partially printed line. Keep the program's
/// prefix, but discard the diagnostic and its newline when comparing output.
fn diagnostic_offset(line: &[u8]) -> Option<usize> {
    // These are the runtime diagnostics in the pinned corpus. Extend this list
    // only with a newly verified CLIPS error; arbitrary bracketed text is output.
    const PREFIXES: &[&[u8]] = &[
        b"[ARGACCES4] ",
        b"[ARGACCES5] ",
        b"[EMATHFUN1] ",
        b"[EMATHFUN2] ",
        b"[EMATHFUN3] ",
        b"[MULTIFUN1] ",
        b"[STRNGFUN2] ",
        b"[EXPRNPSR3] ",
        b"[PRCDRPSR2] ",
        b"[PRNTUTIL1] Unable to find deftemplate ",
        b"[PRNTUTIL2] ",
        b"[PRCCODE4] ",
        b"[PRCCODE5] ",
        b"[PRNTUTIL7] ",
        b"[TMPLTDEF1] ",
        b"[GENRCEXE1] ",
        b"[GENRCEXE4] ",
        b"[INSFUN3] ",
    ];
    line.iter().enumerate().find_map(|(offset, &byte)| {
        (byte == b'['
            && PREFIXES
                .iter()
                .any(|prefix| line[offset..].starts_with(prefix)))
        .then_some(offset)
    })
}

/// A CLIPS diagnostic line: `[CODE123] message`.
fn is_diagnostic(line: &[u8]) -> bool {
    let Some(rest) = line.strip_prefix(b"[") else {
        return false;
    };
    let letters = rest.iter().take_while(|b| b.is_ascii_uppercase()).count();
    let digits = rest[letters..]
        .iter()
        .take_while(|b| b.is_ascii_digit())
        .count();
    letters > 0 && digits > 0 && rest[letters + digits..].starts_with(b"] ")
}

/// Frame input as CLIPS reads `stdin` lines: a CR or an LF ends each line.
fn input_lines(input: &str) -> impl Iterator<Item = &str> {
    let body = input.strip_suffix(['\r', '\n']).unwrap_or(input);
    body.split(['\r', '\n']).filter(move |_| !input.is_empty())
}

fn corpus_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/clips_compat/corpus")
}

fn manifest() -> Manifest {
    serde_json::from_str(&std::fs::read_to_string(corpus_root().join("manifest.json")).unwrap())
        .expect("valid corpus manifest")
}

/// How the engine is driven. Every mode must reproduce the CLIPS golden: an
/// engine restored before each firing must continue exactly where it left
/// off, and rules installed after reset (and after a restore) must see the
/// existing facts. CLIPS itself orders the activations of rules loaded after
/// reset differently, so a late-rule replay must print the same lines, in
/// any order.
#[derive(Clone, Copy, Debug)]
struct Mode {
    late_rules: bool,
    restore: Option<SerializationFormat>,
}

const NORMAL: Mode = Mode {
    late_rules: false,
    restore: None,
};

/// Split before the first line that starts a `defrule`. The prefix is loaded
/// and reset first; a program with deffacts after its first rule has no
/// late-rule replay.
fn split_rules(source: &str) -> Option<(&str, &str)> {
    let start = source
        .match_indices("(defrule")
        .map(|(index, _)| index)
        .find(|&index| index == 0 || source.as_bytes()[index - 1] == b'\n')?;
    let rules = &source[start..];
    (!rules.contains("(deffacts")).then(|| source.split_at(start))
}

/// Serialize and restore, or `None` when JSON cannot hold a non-finite float.
fn restored(engine: &Engine, format: SerializationFormat) -> Option<Result<Engine, String>> {
    match engine.serialize(format) {
        Ok(bytes) => Some(Engine::deserialize(&bytes, format).map_err(|error| error.to_string())),
        Err(SerializationError::Encode(_)) if format == SerializationFormat::Json => None,
        Err(error) => Some(Err(error.to_string())),
    }
}

/// Move printed output and diagnostics from the engine into `observation`.
fn drain(engine: &mut Engine, observation: &mut Observation) {
    observation
        .output
        .push_str(engine.get_output("t").unwrap_or(""));
    engine.clear_output_channel("t");
    for (router, _) in NOTICES {
        observation
            .notices
            .push_str(engine.get_output(router).unwrap_or(""));
        engine.clear_output_channel(router);
    }
    observation
        .diagnostics
        .extend(engine.action_diagnostics().iter().map(ToString::to_string));
    engine.clear_action_diagnostics();
}

/// Run a program, or return `None` when `mode` does not apply to it.
fn observe(case: &Case, source: &str, input: Option<&str>, mode: Mode) -> Option<Observation> {
    let (early, late) = if mode.late_rules {
        split_rules(source)?
    } else {
        (source, "")
    };
    let strategy = match case.strategy {
        None => ConflictResolutionStrategy::Depth,
        Some(Strategy::Breadth) => ConflictResolutionStrategy::Breadth,
        Some(Strategy::Lex) => ConflictResolutionStrategy::Lex,
        Some(Strategy::Mea) => ConflictResolutionStrategy::Mea,
    };
    let mut engine = Engine::new(EngineConfig::utf8().with_strategy(strategy));
    let mut observation = Observation {
        phase: "complete".into(),
        output: String::new(),
        notices: String::new(),
        diagnostics: Vec::new(),
    };
    if let Err(errors) = engine.load_str(early) {
        observation.phase = "load".into();
        observation.diagnostics = errors.iter().map(ToString::to_string).collect();
        return Some(observation);
    }
    for index in 0..case.resets {
        if let Err(error) = engine.reset() {
            return Some(observation.failed("reset", error.to_string()));
        }
        if index == 0 && mode.late_rules {
            // Rules also install into a restored network.
            if let Some(format) = mode.restore {
                engine = match restored(&engine, format)? {
                    Ok(engine) => engine,
                    Err(error) => return Some(observation.failed("restore", error)),
                };
            }
            if let Err(errors) = engine.load_str(late) {
                observation.phase = "load".into();
                observation.diagnostics = errors.iter().map(ToString::to_string).collect();
                return Some(observation);
            }
        }
        engine.clear_output_channel("t");
        if let Some(input) = input {
            for line in input_lines(input) {
                engine.push_input(line);
            }
        }
        let mut fired = 0;
        let outcome = loop {
            let format = match mode.restore {
                Some(format)
                    if fired == 0
                        || (format == SerializationFormat::RECOMMENDED
                            && !mode.late_rules
                            && fired < RESTORED_FIRINGS) =>
                {
                    format
                }
                _ => break engine.run(RunLimit::Count(RUN_LIMIT - fired)),
            };
            drain(&mut engine, &mut observation);
            engine = match restored(&engine, format)? {
                Ok(engine) => engine,
                Err(error) => return Some(observation.failed("restore", error)),
            };
            match engine.run(RunLimit::Count(1)) {
                Ok(step) if step.halt_reason == HaltReason::LimitReached && !engine.is_halted() => {
                    fired += step.rules_fired;
                }
                outcome => break outcome,
            }
        };
        match outcome {
            Ok(result) => {
                assert!(
                    result.halt_reason != HaltReason::LimitReached || engine.is_halted(),
                    "corpus exceeded {RUN_LIMIT} firings; refusing to bless non-quiescence"
                );
            }
            Err(error) => {
                observation.phase = "run".into();
                observation.diagnostics.push(error.to_string());
            }
        }
        drain(&mut engine, &mut observation);
        if observation.phase != "complete" {
            break;
        }
    }
    Some(observation)
}

/// Whether Ferric's observation agrees with the CLIPS golden. `ordered`
/// false compares the printed lines as a multiset.
fn conforms(
    error: Option<ErrorPhase>,
    expected: &Golden,
    actual: &Observation,
    ordered: bool,
) -> bool {
    fn lines(bytes: &[u8]) -> Vec<&[u8]> {
        let mut lines: Vec<_> = bytes.split_inclusive(|&byte| byte == b'\n').collect();
        lines.sort_unstable();
        lines
    }
    // Compare bytes, so a golden that is not valid UTF-8 never matches.
    let output = if ordered {
        actual.output.as_bytes() == expected.output
    } else {
        lines(actual.output.as_bytes()) == lines(&expected.output)
    };
    let printed = output && actual.notices.as_bytes() == expected.notices;
    match error {
        None => actual.phase == "complete" && actual.diagnostics.is_empty() && printed,
        Some(ErrorPhase::Load) => actual.phase == "load",
        Some(ErrorPhase::Run) => {
            matches!(actual.phase.as_str(), "complete" | "run")
                && !actual.diagnostics.is_empty()
                && printed
        }
    }
}

fn fixture_files(directory: &Path, root: &Path, paths: &mut BTreeSet<String>) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            fixture_files(&path, root, paths);
        } else if path.extension().is_some_and(|ext| ext == "clp") {
            let relative = path.strip_prefix(root).unwrap();
            let manifest_path = relative
                .components()
                .map(|component| component.as_os_str().to_str().unwrap())
                .collect::<Vec<_>>()
                .join("/");
            paths.insert(manifest_path);
        }
    }
}

/// The runner strips only listed run-time diagnostics; an unlisted code would
/// otherwise surface as an unexplained output mismatch.
fn assert_run_diagnostics_are_listed(case: &Case, expected: &[u8]) {
    for line in golden(
        expected,
        None,
        case.recoverable_fact_notices,
        case.recoverable_control_notices,
        case.recoverable_random_notices,
    )
    .output
    .split(|&byte| byte == b'\n')
    {
        if is_diagnostic(line) {
            let code = line.split(|&byte| byte == b' ').next().unwrap_or(line);
            assert_eq!(
                diagnostic_offset(line),
                Some(0),
                "{}: run-time diagnostic {} is not listed in diagnostic_offset",
                case.path,
                String::from_utf8_lossy(code)
            );
        }
    }
}

/// A case that opts in to recoverable notices must succeed and its golden
/// must hold at least one notice of each kind it allows.
fn assert_recoverable_notices_are_present(case: &Case, expected: &[u8]) {
    if case.recoverable_fact_notices {
        assert!(case.error.is_none(), "recoverable notices require success");
        assert!(
            (0..expected.len()).any(|index| fact_notice_length(&expected[index..]).is_some()),
            "missing recoverable fact notice: {}",
            case.path
        );
    }
    if case.recoverable_control_notices {
        assert!(case.error.is_none(), "recoverable notices require success");
        assert!(
            (0..expected.len()).any(|index| control_notice_length(&expected[index..]).is_some()),
            "missing recoverable control notice: {}",
            case.path
        );
    }
    if case.recoverable_random_notices {
        assert!(case.error.is_none(), "recoverable notices require success");
        assert!(
            (0..expected.len()).any(|index| random_notice_length(&expected[index..]).is_some()),
            "missing recoverable random notice: {}",
            case.path
        );
    }
    if case.recoverable_build_notices {
        assert!(case.error.is_none(), "recoverable notices require success");
        assert!(
            !split_build_notices(expected).1.is_empty(),
            "missing recoverable build notice: {}",
            case.path
        );
    }
    if case.recoverable_introspection_notices {
        assert!(case.error.is_none(), "recoverable notices require success");
        assert!(
            !split_introspection_notices(expected).1.is_empty(),
            "missing recoverable introspection notice: {}",
            case.path
        );
    }
}

#[test]
fn manifest_covers_every_program() {
    let manifest = manifest();
    assert_eq!(manifest.schema_version, 1);
    assert!(manifest.reference["version"]
        .as_str()
        .unwrap()
        .starts_with("6.30"));
    let root = corpus_root();
    let mut actual = BTreeSet::new();
    fixture_files(&root, &root, &mut actual);
    let mut registered = BTreeSet::new();
    for case in &manifest.cases {
        assert!(
            registered.insert(case.path.clone()),
            "duplicate {}",
            case.path
        );
        assert!(Path::new(&case.path)
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_))));
        assert!(matches!(
            case.level.as_str(),
            "basic" | "boundary" | "interaction"
        ));
        assert!(
            !case.covers.is_empty(),
            "missing coverage tags: {}",
            case.path
        );
        assert!((1..=3).contains(&case.resets));
        let oracle = root.join(&case.path).with_extension("out");
        assert!(oracle.is_file(), "missing oracle for {}", case.path);
        let expected = std::fs::read(&oracle).unwrap();
        assert!(
            !expected.is_empty() && expected.ends_with(b"\n"),
            "empty/vacuous oracle: {}",
            case.path
        );
        assert_eq!(
            case.error.is_some(),
            case_golden(case, &expected, None)
                .output
                .split(|&byte| byte == b'\n')
                .any(|line| is_diagnostic(line) || diagnostic_offset(line).is_some()),
            "only a golden with a CLIPS diagnostic has an error phase: {}",
            case.path
        );
        assert_recoverable_notices_are_present(case, &expected);
        if case.error == Some(ErrorPhase::Run) {
            assert_run_diagnostics_are_listed(case, &expected);
        }
        if root.join(&case.path).with_extension("in").is_file() {
            assert_eq!(case.resets, 1, "input replay across resets is not defined");
        }
        if let Some(gap) = &case.gap {
            assert!(!gap.summary.is_empty());
            assert!(!gap.issues.is_empty(), "untracked gap: {}", case.path);
            for issue in &gap.issues {
                assert!(issue.starts_with("https://github.com/plx/ferric-rules/issues/"));
                assert!(issue.rsplit('/').next().unwrap().parse::<u64>().is_ok());
            }
        }
    }
    assert!(!registered.is_empty());
    assert_eq!(
        actual, registered,
        "every .clp must be registered exactly once"
    );
}

/// A selected corpus program with its source, input and CLIPS golden.
struct Program {
    case: Case,
    source: String,
    input: Option<String>,
    expected: Golden,
}

/// The programs `FERRIC_CORPUS_FILTER` and `FERRIC_CORPUS_LEVEL` select.
fn selected_programs() -> Vec<Program> {
    let root = corpus_root();
    let filter = std::env::var("FERRIC_CORPUS_FILTER").unwrap_or_default();
    let level = std::env::var("FERRIC_CORPUS_LEVEL").unwrap_or_default();
    assert!(
        matches!(level.as_str(), "" | "basic" | "boundary" | "interaction"),
        "unknown FERRIC_CORPUS_LEVEL: {level:?}"
    );
    let programs: Vec<_> = manifest()
        .cases
        .into_iter()
        .filter(|case| case.path.contains(&filter) && (level.is_empty() || case.level == level))
        .map(|case| {
            let input_path = root.join(&case.path).with_extension("in");
            Program {
                source: std::fs::read_to_string(root.join(&case.path)).unwrap(),
                input: input_path
                    .is_file()
                    .then(|| std::fs::read_to_string(input_path).unwrap()),
                expected: case_golden(
                    &case,
                    &std::fs::read(root.join(&case.path).with_extension("out")).unwrap(),
                    case.error,
                ),
                case,
            }
        })
        .collect();
    assert!(
        !programs.is_empty(),
        "filter {filter:?}, level {level:?} matched no cases"
    );
    programs
}

impl Program {
    fn observe(&self, mode: Mode) -> Option<Observation> {
        let mut observation = observe(&self.case, &self.source, self.input.as_deref(), mode)?;
        if self.case.recoverable_control_notices {
            observation.notices =
                String::from_utf8(strip_control_notices(observation.notices.as_bytes())).unwrap();
        }
        if self.case.recoverable_build_notices {
            observation.notices = normalize_ferric_build_notices(&observation.notices);
        }
        if self.case.recoverable_random_notices {
            for notice in [RANDOM_NOTICE, RANDOM_ARITY_NOTICE] {
                observation.notices = observation
                    .notices
                    .replace(std::str::from_utf8(notice).unwrap(), "");
            }
        }
        Some(observation)
    }

    /// Whether the program runs, and conforms, without replay.
    fn conforming(&self) -> bool {
        self.case.gap.is_none()
            && self.case.error != Some(ErrorPhase::Load)
            && conforms(
                self.case.error,
                &self.expected,
                &self.observe(NORMAL).unwrap(),
                true,
            )
    }
}

#[test]
fn characterize_corpus() {
    let mut status = CorpusStatus::start();
    let mut failures = Vec::new();
    let mut report = serde_json::Map::new();
    let (mut passing, mut gaps) = (0, 0);
    let programs = selected_programs();
    status.select(&programs);
    for program in programs {
        let case = &program.case;
        let actual = program.observe(NORMAL).unwrap();
        report.insert(case.path.clone(), serde_json::to_value(&actual).unwrap());
        let matches = conforms(case.error, &program.expected, &actual, true);
        status.record(
            &case.path,
            verdict(matches, case.gap.as_ref().map(|gap| actual == gap.observed)),
        );
        if let Some(gap) = &case.gap {
            gaps += 1;
            if matches {
                failures.push(format!(
                    "{}: unexpected CLIPS match; remove/update gap {}",
                    case.path,
                    gap.issues.join(", ")
                ));
            } else if actual != gap.observed {
                failures.push(format!(
                    "{}: characterization changed ({})\nexpected {:#?}\nactual {actual:#?}",
                    case.path,
                    gap.issues.join(", "),
                    gap.observed
                ));
            }
            continue;
        }
        passing += 1;
        if !matches {
            failures.push(format!(
                "{}: CLIPS mismatch\nexpected output {:?}\nexpected notices {:?}\nactual {actual:#?}",
                case.path,
                String::from_utf8_lossy(&program.expected.output),
                String::from_utf8_lossy(&program.expected.notices),
            ));
        }
    }
    let serialized = serde_json::to_vec_pretty(&report).unwrap();
    if let Ok(path) = std::env::var("FERRIC_CORPUS_REPORT") {
        std::fs::write(path, &serialized).unwrap();
    }
    status.value["observations_sha256"] = format!("{:x}", Sha256::digest(serialized)).into();
    status.finish();
    eprintln!("corpus: {passing} conformance cases, {gaps} characterized gaps");
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

/// Keep the historical raw observation report separate from verdict metadata.
struct CorpusStatus {
    path: Option<PathBuf>,
    value: serde_json::Value,
}

impl CorpusStatus {
    fn start() -> Self {
        let mut status = Self {
            path: std::env::var_os("FERRIC_CORPUS_STATUS").map(PathBuf::from),
            value: serde_json::json!({
                "schema": "ferric.compat-corpus-status", "version": 1,
                "run_id": std::env::var("FERRIC_CORPUS_RUN_ID").ok(),
                "revision": null, "manifest_sha256": null, "scope": "characterization",
                "selection": {
                    "filter": std::env::var("FERRIC_CORPUS_FILTER").unwrap_or_default(),
                    "level": std::env::var("FERRIC_CORPUS_LEVEL").ok().filter(|s| !s.is_empty()),
                    "paths": [],
                },
                "complete": false, "status": "incomplete", "results": {}, "failures": [],
            }),
        };
        // An early manifest/read/selection panic leaves an explicitly incomplete file.
        status.write();
        if status.path.is_some() {
            let output = std::process::Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(env!("CARGO_MANIFEST_DIR"))
                .output()
                .expect("read corpus candidate revision");
            assert!(output.status.success(), "read corpus candidate revision");
            let revision = String::from_utf8(output.stdout).unwrap().trim().to_owned();
            status.value["revision"] = revision.clone().into();
            let bytes = std::fs::read(corpus_root().join("manifest.json")).unwrap();
            status.value["manifest_sha256"] = format!("{:x}", Sha256::digest(bytes)).into();
            let mut files = serde_json::Map::new();
            for case in manifest().cases {
                for extension in ["clp", "out", "in"] {
                    let relative = Path::new(&case.path).with_extension(extension);
                    let file = corpus_root().join(&relative);
                    let hash = if extension != "in" || file.exists() {
                        let bytes = std::fs::read(file).unwrap();
                        serde_json::Value::String(format!("{:x}", Sha256::digest(bytes)))
                    } else {
                        serde_json::Value::Null
                    };
                    files.insert(relative.to_str().unwrap().replace('\\', "/"), hash);
                }
            }
            status.value["files_sha256"] =
                format!("{:x}", Sha256::digest(serde_json::to_vec(&files).unwrap())).into();
            status.write();
            if let Ok(expected) = std::env::var("FERRIC_CORPUS_REVISION") {
                assert_eq!(
                    revision, expected,
                    "corpus checkout differs from requested revision"
                );
            }
        }
        status
    }

    fn select(&mut self, programs: &[Program]) {
        self.value["selection"]["paths"] = programs
            .iter()
            .map(|program| serde_json::Value::String(program.case.path.clone()))
            .collect();
        self.write();
    }

    fn record(&mut self, path: &str, result: serde_json::Value) {
        if result["accepted"] == false {
            self.value["failures"]
                .as_array_mut()
                .unwrap()
                .push(path.into());
        }
        self.value["results"][path] = result;
    }

    fn finish(&mut self) {
        self.value["complete"] = true.into();
        self.value["status"] = if self.value["failures"].as_array().unwrap().is_empty() {
            "passed"
        } else {
            "failed"
        }
        .into();
        self.write();
    }

    fn write(&self) {
        if let Some(path) = &self.path {
            let temporary = path.with_extension("status.tmp");
            std::fs::write(&temporary, serde_json::to_vec_pretty(&self.value).unwrap()).unwrap();
            std::fs::rename(temporary, path).unwrap();
        }
    }
}

fn verdict(conforms: bool, unchanged_gap: Option<bool>) -> serde_json::Value {
    let verdict = match (conforms, unchanged_gap) {
        (true, None) => "conformance",
        (false, None) => "mismatch",
        (true, Some(_)) => "unexpected_fix",
        (false, Some(true)) => "known_gap",
        (false, Some(false)) => "gap_changed",
    };
    serde_json::json!({
        "verdict": verdict, "conforms": conforms,
        "accepted": matches!(verdict, "conformance" | "known_gap"),
    })
}

#[test]
fn corpus_status_distinguishes_conformance_from_accepted_gaps() {
    for (conforms, gap, expected, accepted) in [
        (true, None, "conformance", true),
        (false, None, "mismatch", false),
        (true, Some(false), "unexpected_fix", false),
        (false, Some(true), "known_gap", true),
        (false, Some(false), "gap_changed", false),
    ] {
        let result = verdict(conforms, gap);
        assert_eq!(result["verdict"], expected);
        assert_eq!(result["conforms"], conforms);
        assert_eq!(result["accepted"], accepted);
    }
}

/// Replay every conforming program in the given modes. A program that does
/// not conform without replay is reported by `characterize_corpus` instead.
fn replay(modes: &[Mode]) {
    let mut failures = Vec::new();
    let mut replayed = 0;
    for program in selected_programs() {
        if !program.conforming() {
            continue;
        }
        for &mode in modes {
            let Some(replay) = program.observe(mode) else {
                continue;
            };
            replayed += 1;
            if !conforms(
                program.case.error,
                &program.expected,
                &replay,
                !mode.late_rules,
            ) {
                failures.push(format!(
                    "{}: {mode:?} replay mismatch\n{replay:#?}",
                    program.case.path
                ));
            }
        }
    }
    eprintln!("corpus: {replayed} replays");
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn replay_after_snapshot_restores() {
    let modes: Vec<_> = SerializationFormat::ALL
        .iter()
        .map(|&format| Mode {
            late_rules: false,
            restore: Some(format),
        })
        .collect();
    replay(&modes);
}

#[test]
fn replay_with_rules_loaded_after_reset() {
    let modes: Vec<_> = std::iter::once(None)
        .chain(SerializationFormat::ALL.iter().copied().map(Some))
        .map(|restore| Mode {
            late_rules: true,
            restore,
        })
        .collect();
    replay(&modes);
}

#[test]
fn golden_run_error_preserves_exact_partial_output() {
    let expected = golden(
        b"ready\nprefix \xff [ARGACCES5] invalid operand\n\
          [PRCCODE4] Execution halted.\n",
        Some(ErrorPhase::Run),
        false,
        false,
        false,
    );
    assert_eq!(expected.output, b"ready\nprefix \xff ");
    assert!(expected.notices.is_empty());
}

#[test]
fn golden_run_error_strips_verified_math_and_multifield_diagnostics() {
    for code in ["EMATHFUN1", "EMATHFUN2", "EMATHFUN3", "MULTIFUN1"] {
        let output = format!("prefix [{code}] runtime failure\n[PRCCODE4] Execution halted.\n");
        let expected = golden(
            output.as_bytes(),
            Some(ErrorPhase::Run),
            false,
            false,
            false,
        );
        assert_eq!(expected.output, b"prefix ", "{code}");
        assert!(expected.notices.is_empty());
    }
}

#[test]
fn golden_run_error_preserves_non_diagnostic_bracket_text() {
    let output = b"[USER123] literal\nprefix [USER123] literal\n[USER123]\n\
        [lower1] literal\n[CODE] literal\n[CODE1]\tliteral\n";
    let expected = golden(output, Some(ErrorPhase::Run), false, false, false);
    assert_eq!(expected.output, output);
    assert!(expected.notices.is_empty());
}

#[test]
fn golden_query_target_errors_preserve_prefix_and_other_prntutil1_messages() {
    let output = b"before[PRNTUTIL1] Unable to find deftemplate missing in function any-factp.\n\n\
        [PRNTUTIL2] Syntax Error:  Check appropriate syntax for fact-set query class restrictions.\n\
        [PRCCODE4] Execution halted during the actions of defrule query.\n";
    assert_eq!(
        golden(output, Some(ErrorPhase::Run), false, false, false).output,
        b"before"
    );
    let literal = b"[PRNTUTIL1] literal\nprefix [PRNTUTIL1] Unable to find fact f-9.\n\
        [PRNTUTIL1] Unable to find deftemplate\n";
    assert_eq!(
        golden(literal, Some(ErrorPhase::Run), false, false, false).output,
        literal
    );
    for phase in [None, Some(ErrorPhase::Load)] {
        assert_eq!(golden(output, phase, false, false, false).output, output);
    }
}

#[test]
fn golden_dynamic_parser_errors_remove_only_the_diagnostic_leading_newline() {
    for code in ["EXPRNPSR3", "PRCDRPSR2", "PRNTUTIL2"] {
        for prefix in ["prefix:", "prefix:\n"] {
            let output = format!("{prefix}\n[{code}] parser failure\n[PRCCODE4] halted\n");
            let expected = golden(
                output.as_bytes(),
                Some(ErrorPhase::Run),
                false,
                false,
                false,
            );
            assert_eq!(expected.output, prefix.as_bytes(), "{code}");
            assert!(expected.notices.is_empty());
        }
    }
    let literal = b"prefix:\n[USER123] literal\n[EXPRNPSR3]\tliteral\n\
        [PRCDRPSR2]\ninline [PRNTUTIL2]literal\n";
    assert_eq!(
        golden(literal, Some(ErrorPhase::Run), false, false, false).output,
        literal
    );
}

#[test]
fn golden_preserves_diagnostics_outside_run_error_cases() {
    let output = b"prefix [ARGACCES5] invalid operand\n[PRCCODE4] Execution halted.\n";
    for phase in [None, Some(ErrorPhase::Load)] {
        let expected = golden(output, phase, false, false, false);
        assert_eq!(expected.output, output);
        assert!(expected.notices.is_empty());
    }
}

#[test]
fn golden_run_error_retains_scanner_notices_separately() {
    let notice = NOTICES[0].1;
    let output = format!("prefix {notice}tail [ARGACCES5] invalid operand\n");
    let expected = golden(
        output.as_bytes(),
        Some(ErrorPhase::Run),
        false,
        false,
        false,
    );
    assert_eq!(expected.output, b"prefix tail ");
    assert_eq!(expected.notices, notice.as_bytes());
}

#[test]
fn golden_fact_notices_preserve_exact_partial_output() {
    let source = b"before:[PRNTUTIL1] Unable to find fact f-9.\nFALSE\n\
        [ARGACCES5] Function fact-slot-value expected argument #1 to be of type fact-address or fact-index\n\
        [ARGACCES5] Function retract expected argument #2 to be of type fact-address, fact-index, or the symbol *\n\
        [ARGACCES5] Function fact-index expected argument #1 to be of type fact-address\n\
        -1\n\
        continued\n";
    let expected = golden(source, None, true, false, false);
    assert_eq!(expected.output, b"before:FALSE\n-1\ncontinued\n");
    assert!(expected.notices.is_empty());
    assert_eq!(golden(source, None, false, false, false).output, source);
}

#[test]
fn golden_fact_notices_retain_fatal_errors_and_literal_near_matches() {
    let source = b"[PRNTUTIL1] Unable to find fact f-nine.\n\
        prefix [PRNTUTIL1] Unable to find fact f-9. extra\n\
        [ARGACCES5] Function + expected argument #1 to be of type integer or float\n\
        [ARGACCES5] Function fact-slot-value expected argument #2 to be of type symbol\n\
        [ARGACCES5] Function fact-index expected argument #1 to be of type fact-address or fact-index\n\
        [PRCCODE4] Execution halted.\n[USER123] literal\n";
    assert_eq!(golden(source, None, true, false, false).output, source);
}

#[test]
fn golden_control_notices_preserve_prefix_and_require_opt_in() {
    let source =
        b"clear:[[CONSTRCT1] Some constructs are still in use. Clear cannot continue.\n]\n\
        focus:[[PRNTUTIL1] Unable to find defmodule MISSING.\nFALSE]\ncontinued\n";
    assert_eq!(
        golden(source, None, false, true, false).output,
        b"clear:[]\nfocus:[FALSE]\ncontinued\n"
    );
    assert_eq!(golden(source, None, false, false, false).output, source);
    assert_eq!(
        strip_control_notices(source),
        b"clear:[]\nfocus:[FALSE]\ncontinued\n"
    );
}

#[test]
fn control_notice_filter_retains_literal_near_matches_and_fatal_errors() {
    let source = b"[CONSTRCT1] Some constructs are still in use. Clear cannot continue. extra\n\
        [PRNTUTIL1] Unable to find defmodule MISSING. extra\n\
        [PRNTUTIL1] Unable to find deftemplate MISSING.\n\
        [ARGACCES5] Function focus expected argument #1 to be of type symbol\n\
        [PRCCODE4] Execution halted.\n[USER123] literal\n";
    assert_eq!(golden(source, None, false, true, false).output, source);
    assert_eq!(strip_control_notices(source), source);
}

#[test]
fn golden_random_notice_is_exact_and_requires_opt_in() {
    let source = [b"before".as_slice(), RANDOM_NOTICE, b"after\n"].concat();
    assert_eq!(
        golden(&source, None, false, false, true).output,
        b"beforeafter\n"
    );
    assert_eq!(golden(&source, None, false, false, false).output, source);
    let near_match =
        b"[MISCFUN3] Function random expected argument #1 to be less than argument #3\n";
    assert_eq!(
        golden(near_match, None, false, false, true).output,
        near_match
    );
}

#[test]
fn golden_build_notices_become_exact_notices() {
    let block = b"\n[CSTRCPSR4] Cannot redefine deftemplate p while it is in use.\n\nERROR:\n(deftemplate MAIN::p\n";
    let source = [
        b"before".as_slice(),
        block,
        b"<Fact-1>\n",
        block,
        b"after\n",
    ]
    .concat();
    let (output, notices) = split_build_notices(&source);
    assert_eq!(output, b"before<Fact-1>\nafter\n");
    let line = b"[CSTRCPSR4] Cannot redefine deftemplate p while it is in use.\n";
    assert_eq!(notices, [line.as_slice(), line].concat());
    for near_match in [
        b"\n[CSTRCPSR4] Cannot redefine deftemplate p while it is in use.\n\nERROR:\n(deftemplate MAIN::q\n".as_slice(),
        b"\n[CSTRCPSR4] Cannot redefine deftemplate p while it is in use.\n\nERROR:\n(deftemplate p\n",
        b"\n[CSTRCPSR4] Cannot redefine defrule p while it is in use.\n\nERROR:\n(defrule MAIN::p\n",
        b"\n[CSTRCPSR4] Cannot redefine deftemplate p while it is in use.\n",
    ] {
        assert_eq!(split_build_notices(near_match), (near_match.to_vec(), Vec::new()));
    }
}

#[test]
fn golden_introspection_notices_become_exact_notices() {
    let missing = b"[PRNTUTIL1] Unable to find deftemplate missing.\n".as_slice();
    let template =
        b"[ARGACCES5] Function deftemplate-slot-types expected argument #1 to be of type deftemplate name\n"
            .as_slice();
    let module =
        b"[ARGACCES5] Function get-defrule-list expected argument #1 to be of type defmodule name\n"
            .as_slice();
    let funcall = b"[ARGACCES5] Function funcall expected argument #1 to be of type function, deffunction, or generic function name\n"
        .as_slice();
    let source = [
        missing,
        b"()\n",
        template,
        b"()\n",
        module,
        b"()\n",
        funcall,
        b"FALSE\nafter\n",
    ]
    .concat();
    let (output, notices) = split_introspection_notices(&source);
    assert_eq!(output, b"()\n()\n()\nFALSE\nafter\n");
    assert_eq!(notices, [missing, template, module, funcall].concat());
    for near_match in [
        b"[PRNTUTIL1] Unable to find deftemplate missing. extra\n".as_slice(),
        b"[PRNTUTIL1] Unable to find fact f-9.\n",
        b"[ARGACCES5] Function deftemplate-slot-types expected argument #2 to be of type symbol\n",
        b"[ARGACCES5] Function deftemplate-slot-types expected argument #1 to be of type defmodule name\n",
        b"[ARGACCES5] Function get-defrule-list expected argument #1 to be of type deftemplate name\n",
        b"[ARGACCES5] Function focus expected argument #1 to be of type defmodule name\n",
        b"[ARGACCES5] Function funcall expected argument #1 to be of type symbol or string\n",
        b"[ARGACCES5] Function sort expected argument #1 to be of type function, deffunction, or generic function name\n",
    ] {
        assert_eq!(
            split_introspection_notices(near_match),
            (near_match.to_vec(), Vec::new())
        );
    }
}

#[test]
fn ferric_build_rejections_normalize_to_their_clips_notice() {
    let notices = "compile error: [CSTRCPSR4] cannot redefine template `p` while it is in use by facts or constructs at line 1, column 1\n\
        compile error: cannot define template `q` while its ordered relation is in use by facts or constructs at line 1, column 1\n\
        compile error: unknown template `q` at line 1, column 1\n\
        compile error: cannot define template `q` while its ordered relation is in use by facts or constructs\n";
    assert_eq!(
        normalize_ferric_build_notices(notices),
        "[CSTRCPSR4] Cannot redefine deftemplate p while it is in use.\n\
        [CSTRCPSR4] Cannot redefine deftemplate q while it is in use.\n\
        compile error: unknown template `q` at line 1, column 1\n\
        compile error: cannot define template `q` while its ordered relation is in use by facts or constructs\n"
    );
}
