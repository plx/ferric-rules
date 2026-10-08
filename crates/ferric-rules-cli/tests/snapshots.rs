//! Black-box persistence defaults and diagnostics from an external directory.
#![cfg(feature = "serde")]

use std::io::Write;
use std::process::{Command, Stdio};

use ferric_rules_runtime::{Engine, RunLimit, SerializationFormat};

const SOURCE: &str = r#"
(deffacts candidates (candidate 7) (candidate 8))
(defrule choose (candidate ?n) => (assert (seen ?n)) (printout t "selected " ?n crlf))
"#;

#[test]
fn default_snapshot_and_repl_resume_use_cbor_and_preserve_pending_work() {
    let consumer = tempfile::tempdir().unwrap();
    std::fs::write(consumer.path().join("rules.clp"), SOURCE).unwrap();
    let saved = Command::new(env!("CARGO_BIN_EXE_ferric"))
        .current_dir(consumer.path())
        .args(["snapshot", "rules.clp", "-o", "state.ferric"])
        .output()
        .unwrap();
    assert!(saved.status.success(), "{saved:?}");
    let bytes = std::fs::read(consumer.path().join("state.ferric")).unwrap();
    let mut engine = Engine::deserialize(&bytes, SerializationFormat::Cbor).unwrap();
    assert_eq!(engine.facts().unwrap().count(), 2);
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 2);
    assert_eq!(engine.get_output("t"), Some("selected 8\nselected 7\n"));
    assert_eq!(engine.find_facts("seen").unwrap().len(), 2);

    let mut repl = Command::new(env!("CARGO_BIN_EXE_ferric"))
        .current_dir(consumer.path())
        .args(["repl", "--snapshot", "state.ferric"])
        .env_remove("HOME") // Keep REPL history out of the real user's home.
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    repl.stdin
        .take()
        .unwrap()
        .write_all(b"(run)\n(facts)\n(exit)\n")
        .unwrap();
    let output = repl.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("selected 8\nselected 7\n"), "{stdout}");
    assert!(stdout.contains("(seen 7)"), "{stdout}");
    assert!(stdout.contains("(seen 8)"), "{stdout}");
}

#[test]
fn explicit_json_codec_works_and_legacy_errors_are_useful() {
    let consumer = tempfile::tempdir().unwrap();
    std::fs::write(consumer.path().join("rules.clp"), SOURCE).unwrap();
    let saved = Command::new(env!("CARGO_BIN_EXE_ferric"))
        .current_dir(consumer.path())
        .args([
            "snapshot",
            "rules.clp",
            "-o",
            "state.ferric",
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert!(saved.status.success(), "{saved:?}");
    let bytes = std::fs::read(consumer.path().join("state.ferric")).unwrap();
    let mut engine = Engine::deserialize(&bytes, SerializationFormat::Json).unwrap();
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 2);
    assert_eq!(engine.find_facts("seen").unwrap().len(), 2);

    std::fs::write(
        consumer.path().join("legacy.ferric"),
        b"unversioned raw data",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ferric"))
        .current_dir(consumer.path())
        .args(["repl", "--snapshot", "legacy.ferric"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("legacy raw snapshots are unsupported"),
        "{stderr}"
    );
    assert!(stderr.contains("export application data"), "{stderr}");
}

#[test]
fn snapshot_reports_match_time_errors_raised_while_loading() {
    let consumer = tempfile::tempdir().unwrap();
    std::fs::write(
        consumer.path().join("match-error.clp"),
        "(defrule bad (item ?x) (test (> (/ 1 ?x) 0)) => (printout t bad crlf))
         (assert (item 0))",
    )
    .unwrap();
    for json in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ferric"));
        command.current_dir(consumer.path()).arg("snapshot");
        if json {
            command.arg("--json");
        }
        let output = command
            .args(["match-error.clp", "-o", "state.ferric"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(consumer.path().join("state.ferric").exists());
        let stderr = String::from_utf8(output.stderr).unwrap();
        // The confirmation line is not a diagnostic.
        let lines: Vec<_> = stderr
            .lines()
            .filter(|line| !line.starts_with("Wrote "))
            .collect();
        assert_eq!(lines.len(), 1, "{stderr}");
        let message = if json {
            let diagnostic: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
            assert_eq!(diagnostic["command"], "snapshot");
            assert_eq!(diagnostic["level"], "warning");
            assert_eq!(diagnostic["kind"], "action_warning");
            diagnostic["message"].as_str().unwrap().to_owned()
        } else {
            assert!(
                lines[0].starts_with("ferric snapshot: warning:"),
                "{stderr}"
            );
            stderr
        };
        assert_eq!(message.matches("zero").count(), 1, "{message}");
    }
}
