//! Data-driven, reference-verified CLIPS programs. Known gaps are active
//! characterization assertions: neither a new failure nor an unexpected fix is
//! silently accepted. See `tests/clips_compat/corpus/README.md`.

mod host;

use ferric_rules::core::ConflictResolutionStrategy;
use ferric_rules::runtime::{
    Engine, EngineConfig, HaltReason, RunLimit, SerializationError, SerializationFormat,
};
use serde::{Deserialize, Serialize};
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
    gap: Option<Gap>,
}

/// A conflict resolution strategy other than CLIPS's default depth.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Strategy {
    Breadth,
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

fn golden(bytes: &[u8], error: Option<ErrorPhase>) -> Golden {
    let mut output = Vec::new();
    let mut found: [Vec<u8>; 2] = Default::default();
    let mut rest = bytes;
    'scan: while let Some((&first, tail)) = rest.split_first() {
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
        output = output
            .split_inclusive(|&byte| byte == b'\n')
            .filter(|line| !is_diagnostic(line))
            .flatten()
            .copied()
            .collect();
    }
    let [warnings, errors] = found;
    Golden {
        output,
        notices: [warnings, errors].concat(),
    }
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
            golden(&expected, None)
                .output
                .split(|&byte| byte == b'\n')
                .any(is_diagnostic),
            "only a golden with a CLIPS diagnostic has an error phase: {}",
            case.path
        );
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
                expected: golden(
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
        observe(&self.case, &self.source, self.input.as_deref(), mode)
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
    let mut failures = Vec::new();
    let mut report = serde_json::Map::new();
    let (mut passing, mut gaps) = (0, 0);
    for program in selected_programs() {
        let case = &program.case;
        let actual = program.observe(NORMAL).unwrap();
        report.insert(case.path.clone(), serde_json::to_value(&actual).unwrap());
        let matches = conforms(case.error, &program.expected, &actual, true);
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
    if let Ok(path) = std::env::var("FERRIC_CORPUS_REPORT") {
        std::fs::write(path, serde_json::to_string_pretty(&report).unwrap()).unwrap();
    }
    eprintln!("corpus: {passing} conformance cases, {gaps} characterized gaps");
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
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
