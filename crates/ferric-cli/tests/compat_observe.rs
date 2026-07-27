//! Black-box tests for the dedicated structured compatibility observation.

use std::io::Write as _;
use std::process::{Command, Output};

use serde_json::Value;
use sha2::{Digest, Sha256};

const FIXTURE_ID: &str = "tests/fixtures/compat-observation.clp";
const NONCE: &str = "0123456789abcdef0123456789abcdef";
const SOURCE: &str = r#"
(deftemplate person
  (slot name)
  (multislot tags))

(deffacts seed
  (person (name "Ada") (tags))
  (signal "ready" 7)
  (signal "ready" 7)
  (go))

(defrule report-person
  ?g <- (go)
  ?p <- (person (name "Ada"))
  =>
  (retract ?g)
  (modify ?p (tags (create$ alpha 2 3.5)))
  (printout t "hello" crlf)
  (printout stderr "problem" crlf))
"#;

fn run_ferric(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ferric"))
        .args(args)
        .output()
        .expect("execute ferric binary")
}

fn assert_exit_code(output: &Output, expected: i32) {
    assert_eq!(
        output.status.code(),
        Some(expected),
        "stdout: {}\nstderr: {}",
        stdout_str(output),
        stderr_str(output)
    );
}

fn stdout_str(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr_str(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn sha256(source: &[u8]) -> String {
    format!("{:x}", Sha256::digest(source))
}

fn source_file(source: &str) -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new().expect("create source file");
    file.write_all(source.as_bytes())
        .expect("write source file");
    file.flush().expect("flush source file");
    file
}

fn observe_with_digests(
    source: &str,
    source_sha256: &str,
    composed_sha256: &str,
) -> std::process::Output {
    let file = source_file(source);
    run_ferric(&[
        "compat-observe",
        "--fixture-id",
        FIXTURE_ID,
        "--nonce",
        NONCE,
        "--source-sha256",
        source_sha256,
        "--composed-sha256",
        composed_sha256,
        file.path().to_str().expect("UTF-8 temporary path"),
    ])
}

fn observe(source: &str, composed_sha256: &str) -> std::process::Output {
    observe_with_digests(source, &sha256(source.as_bytes()), composed_sha256)
}

fn parse_single_observation(output: &std::process::Output) -> Value {
    let stdout = stdout_str(output);
    assert_eq!(
        stdout.lines().count(),
        1,
        "expected exactly one JSON line on stdout: {stdout:?}"
    );
    serde_json::from_str(&stdout).expect("stdout must be a JSON observation")
}

fn assert_identity_and_lifecycle(observation: &Value, digest: &str) {
    assert_eq!(observation["schema"], "ferric.compat-observation");
    assert_eq!(observation["version"], 1);
    assert_eq!(observation["engine"]["name"], "ferric");
    assert_eq!(observation["fixture"]["id"], FIXTURE_ID);
    assert_eq!(observation["fixture"]["nonce"], NONCE);
    assert_eq!(observation["fixture"]["source_sha256"], digest);
    assert_eq!(observation["fixture"]["composed_sha256"], digest);
    assert_eq!(observation["phase_reached"], "post-run");

    let lifecycle = observation["lifecycle"]
        .as_array()
        .expect("lifecycle array");
    assert_eq!(lifecycle.len(), 2);
    assert_eq!(lifecycle[0]["sequence"], 0);
    assert_eq!(lifecycle[0]["event"], "START");
    assert_eq!(lifecycle[1]["sequence"], 1);
    assert_eq!(lifecycle[1]["event"], "COMPLETE");
    for record in lifecycle {
        assert_eq!(record["fixture_id"], FIXTURE_ID);
        assert_eq!(record["nonce"], NONCE);
        assert_eq!(record["source_sha256"], digest);
        assert_eq!(record["composed_sha256"], digest);
    }
}

#[test]
fn successful_observation_captures_typed_state_and_lifecycle() {
    let digest = sha256(SOURCE.as_bytes());
    let output = observe(SOURCE, &digest);
    assert_exit_code(&output, 0);
    assert!(
        stderr_str(&output).is_empty(),
        "successful structured observation must not leak diagnostics to stderr"
    );

    let observation = parse_single_observation(&output);
    assert_identity_and_lifecycle(&observation, &digest);

    assert_eq!(observation["run"]["rules_fired"], 1);
    assert!(observation["run"]["fired_rule_names"].is_null());
    assert_eq!(observation["run"]["halt_reason"], "agenda-empty");
    assert_eq!(observation["run"]["agenda_size"], 0);
    assert_eq!(observation["run"]["halted"], false);
    assert_eq!(observation["diagnostics"], Value::Array(Vec::new()));

    let facts = observation["facts"].as_array().expect("facts array");
    assert_eq!(facts.len(), 3);
    for (ordinal, fact) in facts.iter().enumerate() {
        assert_eq!(fact["ordinal"], ordinal);
        assert!(fact["fact_id"]
            .as_str()
            .expect("fact id string")
            .bytes()
            .all(|byte| byte.is_ascii_digit()));
        // With only MAIN registered, ownership is unambiguous despite the
        // absence of a per-fact module accessor.
        assert_eq!(fact["module"], "MAIN");
    }

    let template = facts
        .iter()
        .find(|fact| fact["kind"] == "template")
        .expect("template fact");
    assert_eq!(template["name"], "person");
    let slots = template["slots"].as_array().expect("template slots");
    assert_eq!(slots[0]["name"], "name");
    assert_eq!(slots[0]["value"]["type"], "string");
    assert_eq!(slots[0]["value"]["value"], "Ada");
    assert_eq!(slots[1]["name"], "tags");
    assert_eq!(slots[1]["value"]["type"], "multifield");
    assert_eq!(slots[1]["value"]["values"][0]["type"], "symbol");
    assert_eq!(slots[1]["value"]["values"][0]["value"], "alpha");
    assert_eq!(slots[1]["value"]["values"][1]["type"], "integer");
    assert_eq!(slots[1]["value"]["values"][1]["value"], "2");
    assert_eq!(slots[1]["value"]["values"][2]["type"], "float");
    assert_eq!(slots[1]["value"]["values"][2]["value"], "3.5");
    assert_eq!(slots[1]["value"]["values"][2]["bits"], "0x400c000000000000");

    let ordered = facts
        .iter()
        .filter(|fact| fact["kind"] == "ordered")
        .collect::<Vec<_>>();
    assert_eq!(ordered.len(), 2, "duplicate facts must remain distinct");
    assert_ne!(ordered[0]["fact_id"], ordered[1]["fact_id"]);
    assert_eq!(ordered[0]["fields"], ordered[1]["fields"]);
    assert_eq!(ordered[0]["relation"], "signal");
    assert_eq!(ordered[0]["fields"][0]["type"], "string");
    assert_eq!(ordered[0]["fields"][0]["value"], "ready");
    assert_eq!(ordered[0]["fields"][1]["type"], "integer");
    assert_eq!(ordered[0]["fields"][1]["value"], "7");

    let channels = observation["channels"].as_array().expect("channel array");
    let channel = |name: &str| {
        channels
            .iter()
            .find(|channel| channel["name"] == name)
            .unwrap_or_else(|| panic!("missing channel {name}"))
    };
    assert_eq!(channel("t")["present"], true);
    assert_eq!(channel("t")["text"], "hello\n");
    assert_eq!(channel("stderr")["present"], true);
    assert_eq!(channel("stderr")["text"], "problem\n");
    assert_eq!(channel("stdout")["present"], false);
    assert_eq!(channel("stdout")["text"], "");

    assert_eq!(observation["modules"]["current"], "MAIN");
    assert_eq!(observation["modules"]["focus"], "MAIN");
    assert_eq!(
        observation["modules"]["focus_stack"],
        serde_json::json!(["MAIN"])
    );
    assert_eq!(observation["capabilities"]["fact_modules"], true);
    assert_eq!(observation["capabilities"]["fired_rule_names"], false);
    assert_eq!(
        observation["capabilities"]["router_channel_enumeration"],
        false
    );
    assert_eq!(
        observation["capabilities"]["composed_digest_verification"],
        true
    );
}

#[test]
fn digest_mismatch_emits_parseable_start_only_partial_observation() {
    let output = observe(SOURCE, &"0".repeat(64));
    assert_exit_code(&output, 1);
    assert!(stderr_str(&output).is_empty());

    let observation = parse_single_observation(&output);
    assert_eq!(observation["phase_reached"], "load");
    assert!(observation["run"].is_null());
    assert_eq!(observation["lifecycle"].as_array().unwrap().len(), 1);
    assert_eq!(observation["lifecycle"][0]["event"], "START");
    assert_eq!(observation["diagnostics"][0]["phase"], "load");
    assert_eq!(observation["diagnostics"][0]["category"], "digest-mismatch");
    assert_eq!(observation["modules"]["current"], "MAIN");
}

#[test]
fn distinct_source_and_composed_digests_remain_bound_without_false_verification() {
    let source_sha256 = "1".repeat(64);
    let composed_sha256 = sha256(SOURCE.as_bytes());
    let output = observe_with_digests(SOURCE, &source_sha256, &composed_sha256);
    assert_exit_code(&output, 0);

    let observation = parse_single_observation(&output);
    assert_eq!(
        observation["fixture"]["source_sha256"],
        source_sha256.as_str()
    );
    assert_eq!(
        observation["fixture"]["composed_sha256"],
        composed_sha256.as_str()
    );
    for record in observation["lifecycle"].as_array().unwrap() {
        assert_eq!(record["source_sha256"], source_sha256.as_str());
        assert_eq!(record["composed_sha256"], composed_sha256.as_str());
    }
    assert_eq!(
        observation["capabilities"]["source_digest_verification"],
        false
    );
    assert_eq!(
        observation["capabilities"]["composed_digest_verification"],
        true
    );
}

#[test]
fn load_failure_emits_parseable_start_only_partial_observation() {
    let invalid_source = "(defrule incomplete";
    let digest = sha256(invalid_source.as_bytes());
    let output = observe(invalid_source, &digest);
    assert_exit_code(&output, 1);
    assert!(stderr_str(&output).is_empty());

    let observation = parse_single_observation(&output);
    assert_eq!(observation["phase_reached"], "load");
    assert!(observation["run"].is_null());
    assert_eq!(observation["lifecycle"].as_array().unwrap().len(), 1);
    assert_eq!(observation["lifecycle"][0]["event"], "START");
    assert_eq!(observation["diagnostics"][0]["severity"], "error");
    assert_eq!(observation["diagnostics"][0]["category"], "parse-error");
}

#[test]
fn multiple_modules_leave_fact_ownership_explicitly_unavailable() {
    let source = r"
(defmodule MAIN (export ?ALL))
(deffacts main-seed (main-fact))
(defmodule AUX)
(deffacts aux-seed (aux-fact))
";
    let digest = sha256(source.as_bytes());
    let output = observe(source, &digest);
    assert_exit_code(&output, 0);

    let observation = parse_single_observation(&output);
    assert_eq!(observation["capabilities"]["fact_modules"], false);
    let facts = observation["facts"].as_array().expect("facts array");
    assert_eq!(facts.len(), 2);
    assert!(facts.iter().all(|fact| fact["module"].is_null()));
}

#[test]
fn action_error_stops_before_halt_and_is_captured_without_output_leakage() {
    let source = r#"
(deffacts seed (channel t))
(defrule stop-first
  (declare (salience 10))
  (channel ?channel)
  =>
  (printout ?channel "must-not-leak")
  (halt))
(defrule left-on-agenda
  (channel ?channel)
  =>
  (printout t "must-not-run"))
"#;
    let digest = sha256(source.as_bytes());
    let output = observe(source, &digest);
    assert_exit_code(&output, 0);
    assert!(stderr_str(&output).is_empty());

    let observation = parse_single_observation(&output);
    assert_eq!(observation["run"]["rules_fired"], 1);
    assert_eq!(observation["run"]["halt_reason"], "action-error");
    assert_eq!(observation["run"]["agenda_size"], 1);
    assert_eq!(observation["run"]["halted"], false);
    assert_eq!(observation["diagnostics"][0]["phase"], "run");
    assert_eq!(observation["diagnostics"][0]["severity"], "warning");
    assert_eq!(
        observation["diagnostics"][0]["category"],
        "evaluation-error"
    );
    assert!(observation["channels"]
        .as_array()
        .unwrap()
        .iter()
        .all(|channel| channel["text"] == ""));
}

#[test]
fn cli_rejects_noncanonical_digest_before_execution() {
    let file = source_file(SOURCE);
    let digest = sha256(SOURCE.as_bytes());
    let uppercase_digest = digest.to_uppercase();
    let output = run_ferric(&[
        "compat-observe",
        "--fixture-id",
        FIXTURE_ID,
        "--nonce",
        NONCE,
        "--source-sha256",
        &uppercase_digest,
        "--composed-sha256",
        &digest,
        file.path().to_str().expect("UTF-8 temporary path"),
    ]);

    assert_exit_code(&output, 2);
    assert!(stdout_str(&output).is_empty());
    assert!(stderr_str(&output).contains("64 lowercase hexadecimal"));
}
