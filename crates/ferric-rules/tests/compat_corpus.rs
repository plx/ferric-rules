//! Data-driven, reference-verified CLIPS programs. Known gaps are active
//! characterization assertions: neither a new failure nor an unexpected fix is
//! silently accepted. See `tests/clips_compat/corpus/README.md`.

use ferric_rules::runtime::{Engine, EngineConfig, HaltReason, RunLimit};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const RUN_LIMIT: usize = 1_000;

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
    gap: Option<Gap>,
}

const fn one() -> usize {
    1
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
    diagnostics: Vec<String>,
}

fn corpus_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/clips_compat/corpus")
}

fn manifest() -> Manifest {
    serde_json::from_str(&std::fs::read_to_string(corpus_root().join("manifest.json")).unwrap())
        .expect("valid corpus manifest")
}

fn observe(source: &str, resets: usize, input: Option<&str>) -> Observation {
    let mut engine = Engine::new(EngineConfig::utf8());
    let mut observation = Observation {
        phase: "complete".into(),
        output: String::new(),
        diagnostics: Vec::new(),
    };
    if let Err(errors) = engine.load_str(source) {
        observation.phase = "load".into();
        observation.diagnostics = errors.iter().map(ToString::to_string).collect();
        return observation;
    }
    for _ in 0..resets {
        if let Err(error) = engine.reset() {
            observation.phase = "reset".into();
            observation.diagnostics.push(error.to_string());
            return observation;
        }
        engine.clear_output_channel("t");
        if let Some(input) = input {
            for line in input.lines() {
                engine.push_input(line);
            }
        }
        match engine.run(RunLimit::Count(RUN_LIMIT)) {
            Ok(result) => {
                assert_ne!(
                    result.halt_reason,
                    HaltReason::LimitReached,
                    "corpus exceeded {RUN_LIMIT} firings; refusing to bless non-quiescence"
                );
            }
            Err(error) => {
                observation.phase = "run".into();
                observation.diagnostics.push(error.to_string());
            }
        }
        observation
            .output
            .push_str(engine.get_output("t").unwrap_or(""));
        observation
            .diagnostics
            .extend(engine.action_diagnostics().iter().map(ToString::to_string));
        if observation.phase != "complete" {
            break;
        }
    }
    observation
}

fn fixture_files(directory: &Path, root: &Path, paths: &mut BTreeSet<String>) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            fixture_files(&path, root, paths);
        } else if path.extension().is_some_and(|ext| ext == "clp") {
            paths.insert(
                path.strip_prefix(root)
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_owned(),
            );
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
        let golden = root.join(&case.path).with_extension("out");
        assert!(golden.is_file(), "missing oracle for {}", case.path);
        let expected = std::fs::read_to_string(&golden).unwrap();
        assert!(
            !expected.is_empty() && expected.ends_with('\n'),
            "empty/vacuous oracle: {}",
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

#[test]
fn characterize_corpus() {
    let manifest = manifest();
    let root = corpus_root();
    let filter = std::env::var("FERRIC_CORPUS_FILTER").unwrap_or_default();
    let level = std::env::var("FERRIC_CORPUS_LEVEL").unwrap_or_default();
    assert!(
        matches!(level.as_str(), "" | "basic" | "boundary" | "interaction"),
        "unknown FERRIC_CORPUS_LEVEL: {level:?}"
    );
    let mut failures = Vec::new();
    let mut report = serde_json::Map::new();
    let mut passing = 0;
    let mut gaps = 0;
    for case in manifest
        .cases
        .iter()
        .filter(|case| case.path.contains(&filter) && (level.is_empty() || case.level == level))
    {
        let source = std::fs::read_to_string(root.join(&case.path)).unwrap();
        let output = std::fs::read_to_string(root.join(&case.path).with_extension("out")).unwrap();
        let expected = Observation {
            phase: "complete".into(),
            output,
            diagnostics: Vec::new(),
        };
        let input_path = root.join(&case.path).with_extension("in");
        let input = input_path
            .is_file()
            .then(|| std::fs::read_to_string(input_path).unwrap());
        let actual = observe(&source, case.resets, input.as_deref());
        report.insert(case.path.clone(), serde_json::to_value(&actual).unwrap());
        if let Some(gap) = &case.gap {
            gaps += 1;
            if actual == expected {
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
        } else {
            passing += 1;
            if actual != expected {
                failures.push(format!(
                    "{}: CLIPS mismatch\nexpected {expected:#?}\nactual {actual:#?}",
                    case.path
                ));
            }
        }
    }
    assert!(
        passing + gaps > 0,
        "filter {filter:?}, level {level:?} matched no cases"
    );
    if let Ok(path) = std::env::var("FERRIC_CORPUS_REPORT") {
        std::fs::write(path, serde_json::to_string_pretty(&report).unwrap()).unwrap();
    }
    eprintln!("corpus: {passing} conformance cases, {gaps} characterized gaps");
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
