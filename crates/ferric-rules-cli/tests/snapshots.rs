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
fn explicit_experimental_codec_still_works_and_legacy_errors_are_useful() {
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
            "bincode",
        ])
        .output()
        .unwrap();
    assert!(saved.status.success(), "{saved:?}");
    let bytes = std::fs::read(consumer.path().join("state.ferric")).unwrap();
    let mut engine = Engine::deserialize(&bytes, SerializationFormat::Bincode).unwrap();
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
