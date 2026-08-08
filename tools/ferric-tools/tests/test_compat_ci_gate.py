"""Focused contract tests for the outer compatibility CI gate."""

from __future__ import annotations

import base64
import copy
import hashlib
import hmac
import json
from pathlib import Path

import pytest
from typer.testing import CliRunner

from ferric_tools._harness import build_harness_plan, sha256_bytes
from ferric_tools.compat import ci_gate as ci_gate_module
from ferric_tools.compat.ci_gate import (
    CIGateError,
    CIGateReport,
    evaluate_manifest,
    load_policy,
    write_gate_reports,
)
from ferric_tools.compat.clips_oracle import NATIVE_RECORD_PREFIX, parse_probe_output
from ferric_tools.compat.diagnostics import diagnostic, termination
from ferric_tools.compat.oracle import evaluate_oracle
from ferric_tools.compat.projection import (
    project_clips_observation,
    project_ferric_observation,
)
from ferric_tools.compat.run import candidate_provenance, classify_results, oracle_outcome
from ferric_tools.compat.semantic_gate import (
    ExpectedResult,
    GateReport,
    ReferencePlatform,
    ReferencePolicy,
    SemanticCase,
    SemanticPolicy,
)

CONTROL_FIXTURE = "ferric-oracle/empty-output-state.clp"
CONTROL_ID = "ferric-oracle.empty-output-state"
SEMANTIC_FIXTURE = "ferric-semantic/probe.clp"
NONCE = "1" * 32
COMMIT_SHA = "a" * 40
AUTH_KEY = "b" * 64
REPO_ROOT = Path(__file__).resolve().parents[3]
COMMITTED_POLICY = REPO_ROOT / "tests" / "examples" / "compat-ci-policy.json"


def _semantic_policy() -> SemanticPolicy:
    return SemanticPolicy(
        schema_version=1,
        suite_version="test",
        source="ferric-semantic",
        reference=ReferencePolicy(
            schema="ferric.clips-reference-provenance",
            version=1,
            engine="clips",
            engine_version="6.30",
            package="clips",
            package_version="6.30-4.1",
            base_image=f"debian:bookworm-slim@sha256:{'b' * 64}",
            platforms={
                "linux/amd64": ReferencePlatform(
                    binary_sha256="c" * 64,
                    library_sha256="d" * 64,
                )
            },
        ),
        cases=(
            SemanticCase(
                id="TEST-SEMANTIC",
                issue="https://github.com/plx/ferric-rules/issues/92",
                fixture=SEMANTIC_FIXTURE,
                family="outer gate test",
                expected=ExpectedResult(classification="equivalent"),
            ),
        ),
    )


def _semantic_pass(*_args, **_kwargs) -> GateReport:
    return GateReport((), ())


def _declaration(source_digest: str, composed_digest: str) -> dict[str, object]:
    integer = {"type": "integer", "value": 42}
    return {
        "version": 1,
        "id": CONTROL_ID,
        "feature": "library-only ordered fact state with intentionally empty output",
        "source_sha256": source_digest,
        "composed_sha256": composed_digest,
        "nonce": "0" * 32,
        "setup": ["load", "reset", "run"],
        "expectations": {
            "phase": "run-complete",
            "firings": {"count": 0, "names": None},
            "effects": [
                {
                    "name": "fact:MAIN::result",
                    "value": {"type": "multifield", "value": [integer]},
                }
            ],
            "facts": [
                {
                    "kind": "ordered",
                    "id": 0,
                    "origin": "fixture",
                    "module": "MAIN",
                    "relation": "result",
                    "fields": [integer],
                }
            ],
            "channels": {"stdout": "", "stderr": ""},
            "diagnostic": {"phase": "none", "category": "none", "continued": True},
            "run": {"limit": None, "halt_reason": "agenda-empty"},
            "focus_stack": None,
            "globals": None,
        },
        "normalizers": ["fact-ids"],
    }


def _harness_records(harness_identity: str) -> list[dict[str, object]]:
    return [
        {"version": 2, "record": f"{harness_identity}|START"},
        {"version": 2, "record": f"{harness_identity}|STATE|focus=MAIN"},
        {"version": 2, "record": f"{harness_identity}|COMPLETE"},
    ]


def _raw_observation(
    declaration: dict[str, object],
    *,
    engine: str,
    value: int,
    harness_identity: str,
) -> dict[str, object]:
    identity = {
        "id": declaration["id"],
        "source_sha256": declaration["source_sha256"],
        "composed_sha256": declaration["composed_sha256"],
        "nonce": declaration["nonce"],
    }
    lifecycle = [
        {
            "sequence": sequence,
            "event": event,
            "fixture_id": identity["id"],
            "source_sha256": identity["source_sha256"],
            "composed_sha256": identity["composed_sha256"],
            "nonce": identity["nonce"],
        }
        for sequence, event in (
            ((0, "START"), (1, "COMPLETE"))
            if engine == "ferric"
            else ((0, "start"), (3, "complete"))
        )
    ]
    channels: list[dict[str, object]]
    if engine == "ferric":
        harness_output = "".join(
            f"FERRIC-HARNESS|{record['version']}|{record['record']}\n"
            for record in _harness_records(harness_identity)
        )
        channels = [
            {"name": "t", "present": True, "text": harness_output},
            {"name": "stderr", "present": False, "text": ""},
            {"name": "stdout", "present": False, "text": ""},
        ]
    else:
        channels = [{"name": "t", "text": ""}, {"name": "stderr", "text": ""}]
    observation: dict[str, object] = {
        "schema": "ferric.compat-observation",
        "version": 1,
        "engine": {"name": engine, "version": "test"},
        "fixture": identity,
        "phase_reached": "post-run" if engine == "ferric" else "post_run",
        "lifecycle": lifecycle,
        "run": {
            "rules_fired": 1,
            "halt_reason": "agenda-empty" if engine == "ferric" else "agenda_empty",
            "agenda_size": 0,
            "halted": False,
        },
        "facts": [
            {
                "ordinal": 0,
                "fact_id": "9",
                "module": "MAIN",
                "kind": "ordered",
                "relation": "result",
                "fields": [{"type": "integer", "value": str(value)}],
            }
        ],
        "channels": channels,
        "diagnostics": [],
        "modules": {"current": "MAIN", "focus": "MAIN", "focus_stack": ["MAIN"]},
        "capabilities": {
            "fact_modules": True,
            "composed_digest_verification": engine == "ferric",
            "fired_rule_names": False,
            "rules_fired": engine == "clips",
            "global_values": False,
        },
    }
    if engine == "ferric":
        observation["run"]["fired_rule_names"] = None
        observation["protocol_issues"] = []
    else:
        observation.update(
            {
                "fired_rules": None,
                "globals": {},
                "instrumentation": {
                    "harness_records": _harness_records(harness_identity),
                    "completion_sentinel": True,
                    "rules_fired": 1,
                },
                "protocol_issues": [],
            }
        )
    return observation


def _authenticated_record(logical: bytes, *, nonce: str) -> bytes:
    digest = hmac.new(bytes.fromhex(AUTH_KEY), logical, hashlib.sha256).hexdigest()
    return f"\n{NATIVE_RECORD_PREFIX}{nonce}|".encode() + logical + f"|{digest}\n".encode()


def _native_record(declaration: dict[str, object], kind: str, *fields: object) -> bytes:
    payload = "|".join(str(field) for field in fields)
    return _authenticated_record(
        f"{kind}|{payload}".encode(),
        nonce=str(declaration["nonce"]),
    )


def _probe(declaration: dict[str, object], payload: str) -> bytes:
    encoded = payload.encode()
    return _authenticated_record(
        f"PROBE|{len(encoded)}|".encode() + encoded,
        nonce=str(declaration["nonce"]),
    )


def _clips_raw_evidence(
    declaration: dict[str, object],
    *,
    harness_identity: str,
    value: int,
) -> tuple[bytes, bytes, dict[str, object]]:
    harness_output = "".join(
        f"FERRIC-HARNESS|{record['version']}|{record['record']}\n"
        for record in _harness_records(harness_identity)
    ).encode()
    records = [
        _native_record(
            declaration,
            "LIFECYCLE",
            0,
            "START",
            declaration["id"],
            declaration["source_sha256"],
            declaration["composed_sha256"],
        ),
        _native_record(declaration, "PHASE", 1, "load", "BEGIN"),
        _native_record(declaration, "PHASE", 2, "load", "END", "OK"),
        _native_record(declaration, "PHASE", 3, "reset", "BEGIN"),
        _native_record(declaration, "PHASE", 4, "reset", "END", "OK"),
        _native_record(declaration, "PHASE", 5, "run", "BEGIN"),
        _native_record(declaration, "PHASE", 6, "run", "END", "OK"),
        _native_record(declaration, "RUN", -1, 1, 0, 0, 0, 0, 0),
        _probe(declaration, "PHASE|1|RESET_COMPLETE"),
        _probe(declaration, "PHASE|2|RUN_COMPLETE"),
        _probe(declaration, "MODULE|MAIN"),
        _probe(declaration, "FOCUS|MAIN"),
        _probe(declaration, "FACT|1|9|MAIN|result|ordered|1"),
        _probe(declaration, "SLOT|1|9|1|implied|MULTIFIELD|1"),
        _probe(declaration, f"VALUE|1|9|MAIN|result|1|implied|1|INTEGER|{value}"),
        _native_record(
            declaration,
            "LIFECYCLE",
            3,
            "COMPLETE",
            declaration["id"],
            declaration["source_sha256"],
            declaration["composed_sha256"],
        ),
    ]
    raw_stderr = b"".join(records)
    observation = parse_probe_output(
        harness_output,
        raw_stderr=raw_stderr,
        fixture_id=str(declaration["id"]),
        nonce=str(declaration["nonce"]),
        source_sha256=str(declaration["source_sha256"]),
        composed_sha256=str(declaration["composed_sha256"]),
        auth_key=AUTH_KEY,
        expected_harness_identity=harness_identity,
    )
    return harness_output, raw_stderr, observation


def _result(
    canonical: dict[str, object],
    *,
    raw: dict[str, object],
    engine: str,
    raw_stdout: bytes | None = None,
    raw_stderr: bytes | None = None,
) -> dict[str, object]:
    if engine == "ferric":
        raw_stdout = (json.dumps(raw, separators=(",", ":")) + "\n").encode()
        raw_stderr = b""
    else:
        assert raw_stdout is not None
        assert raw_stderr is not None
    result = {
        "exit_code": 0,
        "stdout": raw_stdout.decode(),
        "stderr": raw_stderr.decode(),
        "duration_ms": 1,
        "timed_out": False,
        "raw_output": {
            "encoding": "base64",
            "stdout": base64.b64encode(raw_stdout).decode(),
            "stderr": base64.b64encode(raw_stderr).decode(),
        },
        "observation": raw,
        "diagnostic": diagnostic("none", "none", continued=True),
        "termination": termination(exit_code=0, timed_out=False),
        "canonical_observation": canonical,
    }
    if engine == "clips":
        result["observer_auth_key"] = AUTH_KEY
    return result


def _gate_evidence(tmp_path: Path, *, ferric_value: int = 42):
    root = tmp_path / "repo"
    examples = root / "tests" / "examples"
    source = examples / CONTROL_FIXTURE
    source.parent.mkdir(parents=True)
    source.write_text("(deffacts startup (result 42))\n", encoding="utf-8")
    plan = build_harness_plan(
        CONTROL_FIXTURE,
        examples_dir=examples,
        output_dir=root / "tests" / "harnesses",
        root=root,
    )
    assert plan.harness_path is not None
    assert plan.harness_bytes is not None
    assert plan.verifier_identity is not None
    plan.harness_path.parent.mkdir(parents=True)
    plan.harness_path.write_bytes(plan.harness_bytes)
    composed = plan.source_bytes + b"\n" + plan.harness_bytes
    composed_digest = sha256_bytes(composed)
    registry_declaration = _declaration(sha256_bytes(plan.source_bytes), composed_digest)
    runtime_declaration = copy.deepcopy(registry_declaration)
    runtime_declaration["nonce"] = NONCE

    ferric_raw = _raw_observation(
        runtime_declaration,
        engine="ferric",
        value=ferric_value,
        harness_identity=plan.verifier_identity,
    )
    clips_stdout, clips_stderr, clips_raw = _clips_raw_evidence(
        runtime_declaration,
        harness_identity=plan.verifier_identity,
        value=42,
    )
    ferric_canonical = project_ferric_observation(
        ferric_raw,
        harness_identity=plan.verifier_identity,
    )
    clips_canonical = project_clips_observation(
        clips_raw,
        harness_identity=plan.verifier_identity,
    )
    ferric = _result(
        ferric_canonical,
        raw=ferric_raw,
        engine="ferric",
    )
    clips = _result(
        clips_canonical,
        raw=clips_raw,
        engine="clips",
        raw_stdout=clips_stdout,
        raw_stderr=clips_stderr,
    )
    evaluation = evaluate_oracle(
        runtime_declaration,
        ferric_canonical,
        clips_canonical,
        expected_source_sha256=registry_declaration["source_sha256"],
        expected_composed_sha256=composed_digest,
    )
    _oracle_classification, _oracle_reason, oracle_evidence = oracle_outcome(evaluation)
    classification, reason = classify_results(ferric, clips, evaluation=evaluation)
    expected_composed = {"sha256": composed_digest, "size_bytes": len(composed)}
    for result in (ferric, clips):
        result["oracle_evidence"] = oracle_evidence
        result["harness"] = dict(plan.metadata)
        result["composed_source"] = dict(expected_composed)

    entry = {
        "source": "ferric-oracle",
        "source_sha256": registry_declaration["source_sha256"],
        "classification": classification,
        "reason": reason,
        "runability": "library",
        "harness": dict(plan.metadata),
        "oracle": registry_declaration,
        "oracle_evidence": oracle_evidence,
        "ferric": ferric,
        "clips": clips,
    }
    registry = {
        "version": 1,
        "fixtures": {
            CONTROL_FIXTURE: registry_declaration,
            SEMANTIC_FIXTURE: {},
        },
    }
    (examples / "compat-oracles.json").write_text(json.dumps(registry), encoding="utf-8")

    candidate_binary = root / "target" / "release" / "ferric"
    candidate_binary.parent.mkdir(parents=True)
    candidate_binary.write_bytes(b"candidate-binary")
    files = {
        CONTROL_FIXTURE: entry,
        SEMANTIC_FIXTURE: {
            "source": "ferric-semantic",
            "classification": "equivalent",
            "reason": "oracle-v2-match",
            "ferric": {},
            "clips": {},
        },
    }
    manifest = {
        "version": 3,
        "oracle_protocol_version": 1,
        "candidate": candidate_provenance(candidate_binary, commit_sha=COMMIT_SHA),
        "summary": {
            "total": 2,
            "equivalent": 2 if classification == "equivalent" else 1,
            "divergent": 1 if classification == "divergent" else 0,
            "incompatible": 0,
            "pending": 0,
        },
        "files": files,
    }
    return root, examples, candidate_binary, manifest


def _evaluate(tmp_path: Path, *, ferric_value: int = 42):
    root, examples, candidate, manifest = _gate_evidence(
        tmp_path,
        ferric_value=ferric_value,
    )
    report = evaluate_manifest(
        load_policy(COMMITTED_POLICY),
        _semantic_policy(),
        manifest,
        examples_dir=examples,
        root=root,
        ferric_bin=candidate,
        expected_commit_sha=COMMIT_SHA,
        semantic_evaluator=_semantic_pass,
    )
    return report, root, examples, candidate, manifest


def test_committed_policy_is_exact_and_rejects_a_weakened_control(tmp_path: Path) -> None:
    policy = load_policy(COMMITTED_POLICY)
    assert [control.fixture for control in policy.required_equivalent_controls] == [CONTROL_FIXTURE]

    raw = json.loads(COMMITTED_POLICY.read_text(encoding="utf-8"))
    raw["required_equivalent_controls"][0]["expected_effects"] = []
    weakened = tmp_path / "policy.json"
    weakened.write_text(json.dumps(raw), encoding="utf-8")

    with pytest.raises(CIGateError, match="locked empty-output-state control contract"):
        load_policy(weakened)


def test_gate_reports_are_deterministic_and_capture_exact_failures(tmp_path: Path) -> None:
    json_path = tmp_path / "artifacts" / "gate.json"
    markdown_path = tmp_path / "artifacts" / "gate.md"
    report = CIGateReport(
        failures=("zeta failure", "alpha\ncontext"),
        accepted_deviations=("known beta",),
    )

    write_gate_reports(
        report,
        claimed_outcomes=23,
        json_path=json_path,
        markdown_path=markdown_path,
    )
    first_json = json_path.read_bytes()
    first_markdown = markdown_path.read_bytes()
    write_gate_reports(
        report,
        claimed_outcomes=23,
        json_path=json_path,
        markdown_path=markdown_path,
    )

    assert json_path.read_bytes() == first_json
    assert markdown_path.read_bytes() == first_markdown
    assert json.loads(first_json) == {
        "schema": "ferric.compat-ci-gate-report",
        "version": 1,
        "status": "failed",
        "claimed_outcomes": 23,
        "failure_count": 2,
        "accepted_deviation_count": 1,
        "failures": ["alpha\ncontext", "zeta failure"],
        "accepted_deviations": ["known beta"],
    }
    markdown = first_markdown.decode("utf-8")
    assert "Status: **FAIL**" in markdown
    assert "- alpha context" in markdown
    assert "- zeta failure" in markdown


def test_cli_writes_failure_artifacts_before_exiting_one(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    root = tmp_path / "repo"
    examples = root / "tests" / "examples"
    examples.mkdir(parents=True)
    json_path = tmp_path / "gate.json"
    markdown_path = tmp_path / "gate.md"
    policy = load_policy(COMMITTED_POLICY)
    semantic_policy = _semantic_policy()
    monkeypatch.setattr(ci_gate_module, "repo_root", lambda: root)
    monkeypatch.setattr(ci_gate_module, "default_examples_dir", lambda: examples)
    monkeypatch.setattr(ci_gate_module, "load_policy", lambda _path: policy)
    monkeypatch.setattr(
        ci_gate_module,
        "load_semantic_policy",
        lambda _path: semantic_policy,
    )
    monkeypatch.setattr(ci_gate_module, "load_json", lambda *_args, **_kwargs: {})
    monkeypatch.setattr(
        ci_gate_module,
        "evaluate_manifest",
        lambda *_args, **_kwargs: CIGateReport(("captured gate failure",), ()),
    )

    result = CliRunner().invoke(
        ci_gate_module.app,
        [
            "--expected-commit-sha",
            COMMIT_SHA,
            "--report-json",
            str(json_path),
            "--report-markdown",
            str(markdown_path),
        ],
    )

    assert result.exit_code == 1
    assert json.loads(json_path.read_text(encoding="utf-8"))["failures"] == [
        "captured gate failure"
    ]
    assert "captured gate failure" in markdown_path.read_text(encoding="utf-8")


def test_cli_rejects_substituting_an_alternate_semantic_policy(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    root = tmp_path / "repo"
    examples = root / "tests" / "examples"
    examples.mkdir(parents=True)
    committed_semantic = examples / "compat-semantic-policy.json"
    committed_semantic.write_text("{}\n", encoding="utf-8")
    alternate_semantic = tmp_path / "alternate-semantic-policy.json"
    alternate_semantic.write_text("{}\n", encoding="utf-8")
    json_path = tmp_path / "gate.json"
    markdown_path = tmp_path / "gate.md"
    policy = load_policy(COMMITTED_POLICY)
    monkeypatch.setattr(ci_gate_module, "repo_root", lambda: root)
    monkeypatch.setattr(ci_gate_module, "default_examples_dir", lambda: examples)
    monkeypatch.setattr(ci_gate_module, "load_policy", lambda _path: policy)

    result = CliRunner().invoke(
        ci_gate_module.app,
        [
            "--semantic-policy",
            str(alternate_semantic),
            "--expected-commit-sha",
            COMMIT_SHA,
            "--report-json",
            str(json_path),
            "--report-markdown",
            str(markdown_path),
        ],
    )

    assert result.exit_code == 1
    failures = json.loads(json_path.read_text(encoding="utf-8"))["failures"]
    assert any("must resolve to the committed policy" in failure for failure in failures)


def test_complete_equivalent_control_passes_with_deep_raw_reprojection(tmp_path: Path) -> None:
    report, *_rest = _evaluate(tmp_path)

    assert report.failures == ()


def test_gate_rejects_complete_records_from_a_foreign_generated_harness(
    tmp_path: Path,
) -> None:
    _report, root, examples, candidate, manifest = _evaluate(tmp_path)
    ferric = manifest["files"][CONTROL_FIXTURE]["ferric"]
    observation = ferric["observation"]
    channel = next(item for item in observation["channels"] if item["name"] == "t")
    original_identity = channel["text"].split("|", 3)[2]
    foreign_identity = "ferric-harness-" + ("f" * 64)
    channel["text"] = channel["text"].replace(original_identity, foreign_identity)
    raw_stdout = (json.dumps(observation, separators=(",", ":")) + "\n").encode()
    ferric["stdout"] = raw_stdout.decode()
    ferric["raw_output"]["stdout"] = base64.b64encode(raw_stdout).decode()

    report = evaluate_manifest(
        load_policy(COMMITTED_POLICY),
        _semantic_policy(),
        manifest,
        examples_dir=examples,
        root=root,
        ferric_bin=candidate,
        expected_commit_sha=COMMIT_SHA,
        semantic_evaluator=_semantic_pass,
    )

    assert any("identity does not match" in failure for failure in report.failures)
    assert report.accepted_deviations == ()


def test_deliberate_control_divergence_is_rejected_even_when_consistently_persisted(
    tmp_path: Path,
) -> None:
    report, *_rest = _evaluate(tmp_path, ferric_value=43)

    assert any("required equivalent/oracle-v1-match" in failure for failure in report.failures)


@pytest.mark.parametrize(
    ("mutation", "message"),
    [
        (
            lambda manifest: manifest["summary"].__setitem__("equivalent", 0),
            "manifest summary is stale",
        ),
        (
            lambda manifest: manifest["candidate"].__setitem__("binary_sha256", "0" * 64),
            "candidate provenance does not match",
        ),
        (
            lambda manifest: manifest["files"][CONTROL_FIXTURE]["clips"].pop("harness"),
            "clips harness metadata is missing or stale",
        ),
        (
            lambda manifest: manifest["files"][CONTROL_FIXTURE]["ferric"].pop("composed_source"),
            "ferric composed-source evidence is missing or stale",
        ),
    ],
)
def test_gate_rejects_stale_summary_candidate_and_physical_result_metadata(
    tmp_path: Path,
    mutation,
    message: str,
) -> None:
    root, examples, candidate, manifest = _gate_evidence(tmp_path)
    mutation(manifest)

    report = evaluate_manifest(
        load_policy(COMMITTED_POLICY),
        _semantic_policy(),
        manifest,
        examples_dir=examples,
        root=root,
        ferric_bin=candidate,
        expected_commit_sha=COMMIT_SHA,
        semantic_evaluator=_semantic_pass,
    )

    assert any(message in failure for failure in report.failures)


def test_gate_rejects_missing_and_unexplained_completed_outcomes(tmp_path: Path) -> None:
    root, examples, candidate, manifest = _gate_evidence(tmp_path)
    manifest["files"]["unclaimed.clp"] = {
        "classification": "equivalent",
        "ferric": {},
        "clips": {},
    }
    manifest["summary"] = {
        "total": 3,
        "equivalent": 3,
        "divergent": 0,
        "incompatible": 0,
        "pending": 0,
    }
    manifest["files"][CONTROL_FIXTURE]["clips"] = None

    report = evaluate_manifest(
        load_policy(COMMITTED_POLICY),
        _semantic_policy(),
        manifest,
        examples_dir=examples,
        root=root,
        ferric_bin=candidate,
        expected_commit_sha=COMMIT_SHA,
        semantic_evaluator=_semantic_pass,
    )

    assert any("completed outcomes are not claimed" in failure for failure in report.failures)
    assert any("claimed outcomes are missing or partial" in failure for failure in report.failures)


def test_gate_rejects_unclaimed_engine_evidence_hidden_behind_pending_label(
    tmp_path: Path,
) -> None:
    root, examples, candidate, manifest = _gate_evidence(tmp_path)
    manifest["files"]["hidden-result.clp"] = {
        "classification": "pending",
        "ferric": {"exit_code": 0},
        "clips": None,
        "oracle": None,
        "oracle_evidence": None,
    }
    manifest["summary"] = {
        "total": 3,
        "equivalent": 2,
        "divergent": 0,
        "incompatible": 0,
        "pending": 1,
    }

    report = evaluate_manifest(
        load_policy(COMMITTED_POLICY),
        _semantic_policy(),
        manifest,
        examples_dir=examples,
        root=root,
        ferric_bin=candidate,
        expected_commit_sha=COMMIT_SHA,
        semantic_evaluator=_semantic_pass,
    )

    assert any(
        "unclaimed entries contain engine or oracle evidence" in failure
        for failure in report.failures
    )


def test_malformed_unhashable_classification_fails_without_raising(tmp_path: Path) -> None:
    root, examples, candidate, manifest = _gate_evidence(tmp_path)
    manifest["files"][CONTROL_FIXTURE]["classification"] = []

    report = evaluate_manifest(
        load_policy(COMMITTED_POLICY),
        _semantic_policy(),
        manifest,
        examples_dir=examples,
        root=root,
        ferric_bin=candidate,
        expected_commit_sha=COMMIT_SHA,
        semantic_evaluator=_semantic_pass,
    )

    assert any("classification is missing or unsupported" in failure for failure in report.failures)
    assert any("claimed outcomes are missing or partial" in failure for failure in report.failures)


def test_gate_requires_registry_membership_exactly_equal_to_claimed_set(tmp_path: Path) -> None:
    root, examples, candidate, manifest = _gate_evidence(tmp_path)
    registry_path = examples / "compat-oracles.json"
    registry = json.loads(registry_path.read_text(encoding="utf-8"))
    registry["fixtures"]["unclaimed.clp"] = {}
    registry_path.write_text(json.dumps(registry), encoding="utf-8")

    report = evaluate_manifest(
        load_policy(COMMITTED_POLICY),
        _semantic_policy(),
        manifest,
        examples_dir=examples,
        root=root,
        ferric_bin=candidate,
        expected_commit_sha=COMMIT_SHA,
        semantic_evaluator=_semantic_pass,
    )

    assert any("oracle registry membership mismatch" in failure for failure in report.failures)
