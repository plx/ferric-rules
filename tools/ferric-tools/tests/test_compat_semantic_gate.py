"""Contract tests for the pinned-CLIPS semantic differential gate."""

from __future__ import annotations

import base64
import hashlib
import hmac
import json
from pathlib import Path

import pytest

from ferric_tools._harness import sha256_bytes
from ferric_tools.compat.clips_oracle import NATIVE_RECORD_PREFIX, parse_probe_output
from ferric_tools.compat.diagnostics import diagnostic, termination
from ferric_tools.compat.oracle import (
    evaluate_oracle,
    observation_semantic_fingerprint,
    scenario_plan_sha256,
    validate_declaration,
)
from ferric_tools.compat.projection import (
    project_clips_observation,
    project_ferric_observation,
)
from ferric_tools.compat.run import classify_results, oracle_outcome
from ferric_tools.compat.semantic_gate import (
    POLICY_SOURCE,
    REFERENCE_SCHEMA,
    REQUIRED_CASE_ISSUES,
    ExpectedResult,
    MismatchPin,
    ReferencePlatform,
    ReferencePolicy,
    SemanticCase,
    SemanticGateError,
    SemanticPolicy,
    evaluate_manifest,
    load_policy,
)

NONCE = "1" * 32
BINARY_DIGEST = "b" * 64
LIBRARY_DIGEST = "c" * 64
BASE_IMAGE = f"debian:bookworm-slim@sha256:{'d' * 64}"
IMAGE_ID = f"sha256:{'e' * 64}"
AUTH_KEY = "f" * 64


def _reference_policy() -> ReferencePolicy:
    return ReferencePolicy(
        schema=REFERENCE_SCHEMA,
        version=1,
        engine="clips",
        engine_version="6.30",
        package="clips",
        package_version="6.30-4.1",
        base_image=BASE_IMAGE,
        platforms={
            "linux/amd64": ReferencePlatform(
                binary_sha256=BINARY_DIGEST,
                library_sha256=LIBRARY_DIGEST,
            )
        },
    )


def _reference_evidence() -> dict[str, object]:
    return {
        "schema": REFERENCE_SCHEMA,
        "version": 1,
        "engine": "clips",
        "engine_version": "6.30",
        "package": "clips",
        "package_version": "6.30-4.1",
        "platform": "linux/amd64",
        "binary_sha256": BINARY_DIGEST,
        "library_sha256": LIBRARY_DIGEST,
        "base_image": BASE_IMAGE,
        "image_id": IMAGE_ID,
    }


def _declaration(source_digest: str) -> dict[str, object]:
    declaration: dict[str, object] = {
        "version": 2,
        "id": "FR-RETE-001",
        "feature": "semantic gate probe",
        "source_sha256": source_digest,
        "composed_sha256": "0" * 64,
        "nonce": NONCE,
        "sources": [
            {
                "name": "primary",
                "path": "ferric-semantic/fr-rete-001.clp",
                "sha256": source_digest,
            }
        ],
        "setup": {
            "steps": [
                {"operation": "load", "source": "primary", "on_error": "stop"},
                {"operation": "reset", "on_error": "stop"},
                {"operation": "run", "limit": None, "on_error": "stop"},
            ]
        },
        "expectations": {
            "phase": "run-complete",
            "firings": {"count": 1, "names": None},
            "effects": [
                {
                    "name": "fact:MAIN::result",
                    "value": {
                        "type": "multifield",
                        "value": [{"type": "integer", "value": 42}],
                    },
                }
            ],
            "facts": [
                {
                    "kind": "ordered",
                    "id": 0,
                    "origin": "fixture",
                    "module": "MAIN",
                    "relation": "result",
                    "fields": [{"type": "integer", "value": 42}],
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
    declaration["composed_sha256"] = scenario_plan_sha256(declaration)
    return declaration


def _raw_observation(
    declaration: dict[str, object],
    *,
    engine: str,
    value: int = 42,
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
        "channels": (
            [
                {"name": "t", "present": False, "text": ""},
                {"name": "stderr", "present": False, "text": ""},
                {"name": "stdout", "present": False, "text": ""},
            ]
            if engine == "ferric"
            else [{"name": "t", "text": ""}, {"name": "stderr", "text": ""}]
        ),
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
    else:
        observation.update(
            {
                "fired_rules": None,
                "globals": {},
                "instrumentation": {
                    "harness_records": [],
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
    value: int,
) -> tuple[bytes, bytes, dict[str, object]]:
    setup = declaration["setup"]
    assert isinstance(setup, dict)
    steps = setup["steps"]
    assert isinstance(steps, list)
    expected_phases = tuple(
        str(step["operation"])
        for step in steps
        if isinstance(step, dict) and step["operation"] != "set-strategy"
    )
    records = [
        _native_record(
            declaration,
            "LIFECYCLE",
            0,
            "START",
            declaration["id"],
            declaration["source_sha256"],
            declaration["composed_sha256"],
        )
    ]
    sequence = 1
    for phase in expected_phases:
        records.append(_native_record(declaration, "PHASE", sequence, phase, "BEGIN"))
        sequence += 1
        records.append(_native_record(declaration, "PHASE", sequence, phase, "END", "OK"))
        sequence += 1
    records.extend(
        [
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
    )
    raw_stdout = b""
    raw_stderr = b"".join(records)
    observation = parse_probe_output(
        raw_stdout,
        raw_stderr=raw_stderr,
        fixture_id=str(declaration["id"]),
        nonce=str(declaration["nonce"]),
        source_sha256=str(declaration["source_sha256"]),
        composed_sha256=str(declaration["composed_sha256"]),
        auth_key=AUTH_KEY,
        expected_phases=expected_phases,
    )
    return raw_stdout, raw_stderr, observation


def _result(
    observation: dict[str, object],
    *,
    raw_observation: dict[str, object],
    engine: str,
    raw_stdout: bytes | None = None,
    raw_stderr: bytes | None = None,
) -> dict[str, object]:
    if engine == "ferric":
        raw_stdout = (json.dumps(raw_observation, separators=(",", ":")) + "\n").encode()
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
        "observation": raw_observation,
        "diagnostic": diagnostic("none", "none", continued=True),
        "termination": termination(exit_code=0, timed_out=False),
        "canonical_observation": observation,
    }
    if engine == "clips":
        result["observer_auth_key"] = AUTH_KEY
    return result


def _fixture_evidence(tmp_path: Path, *, ferric_value: int = 42, clips_value: int = 42):
    examples_dir = tmp_path / "examples"
    source_path = examples_dir / "ferric-semantic" / "fr-rete-001.clp"
    source_path.parent.mkdir(parents=True)
    source_path.write_text("(deffacts startup (seed))\n", encoding="utf-8")
    source_digest = sha256_bytes(source_path.read_bytes())
    declaration = _declaration(source_digest)
    (examples_dir / "compat-oracles.json").write_text(
        json.dumps(
            {
                "version": 1,
                "fixtures": {"ferric-semantic/fr-rete-001.clp": declaration},
            }
        ),
        encoding="utf-8",
    )
    ferric_raw = _raw_observation(declaration, engine="ferric", value=ferric_value)
    clips_stdout, clips_stderr, clips_raw = _clips_raw_evidence(
        declaration,
        value=clips_value,
    )
    ferric_observation = project_ferric_observation(ferric_raw, harness_identity=None)
    clips_observation = project_clips_observation(clips_raw, harness_identity=None)
    ferric = _result(ferric_observation, raw_observation=ferric_raw, engine="ferric")
    clips = _result(
        clips_observation,
        raw_observation=clips_raw,
        engine="clips",
        raw_stdout=clips_stdout,
        raw_stderr=clips_stderr,
    )
    evaluation = evaluate_oracle(
        declaration,
        ferric_observation,
        clips_observation,
        expected_source_sha256=source_digest,
        expected_composed_sha256=declaration["composed_sha256"],
    )
    _oracle_classification, _oracle_reason, oracle_evidence = oracle_outcome(evaluation)
    ferric["oracle_evidence"] = oracle_evidence
    clips["oracle_evidence"] = oracle_evidence
    classification, reason = classify_results(ferric, clips, evaluation=evaluation)
    entry = {
        "source": POLICY_SOURCE,
        "source_sha256": source_digest,
        "classification": classification,
        "reason": reason,
        "oracle": declaration,
        "oracle_evidence": oracle_evidence,
        "ferric": ferric,
        "clips": clips,
    }
    manifest = {
        "version": 3,
        "oracle_protocol_version": 1,
        "reference": _reference_evidence(),
        "files": {"ferric-semantic/fr-rete-001.clp": entry},
    }
    evidence = validate_declaration(
        declaration,
        expected_source_sha256=source_digest,
        expected_composed_sha256=declaration["composed_sha256"],
    )
    assert evidence.value is not None
    return examples_dir, declaration, evidence.value, evaluation, manifest


def _policy(expected: ExpectedResult) -> SemanticPolicy:
    case = SemanticCase(
        id="FR-RETE-001",
        issue="https://github.com/plx/ferric-rules/issues/103",
        fixture="ferric-semantic/fr-rete-001.clp",
        family="fact duplication",
        expected=expected,
    )
    return SemanticPolicy(
        schema_version=1,
        suite_version="1.0.0",
        source=POLICY_SOURCE,
        reference=_reference_policy(),
        cases=(case,),
    )


def _divergence_policy(tmp_path: Path):
    examples_dir, _raw, declaration, evaluation, manifest = _fixture_evidence(
        tmp_path,
        ferric_value=43,
    )
    entry = manifest["files"]["ferric-semantic/fr-rete-001.clp"]
    mismatches = tuple(
        sorted(
            (MismatchPin(item.scope, item.field) for item in evaluation.mismatches),
            key=lambda item: (item.scope, item.field),
        )
    )
    expected = ExpectedResult(
        classification="divergent",
        reason=entry["reason"],
        mismatches=mismatches,
        ferric_fingerprint=observation_semantic_fingerprint(
            entry["ferric"]["canonical_observation"],
            declaration=declaration,
        ),
        rationale="Tracked fact-state mismatch.",
        since="0.1.0",
        tracking_issue="https://github.com/plx/ferric-rules/issues/103",
    )
    return examples_dir, _policy(expected), manifest


def _full_policy_json() -> dict[str, object]:
    cases = []
    for case_id, issue_number in sorted(REQUIRED_CASE_ISSUES.items()):
        cases.append(
            {
                "id": case_id,
                "issue": f"https://github.com/plx/ferric-rules/issues/{issue_number}",
                "fixture": f"ferric-semantic/{case_id.lower()}.clp",
                "family": f"family for {case_id}",
                "expected": {"classification": "equivalent"},
            }
        )
    return {
        "schema_version": 1,
        "suite_version": "1.0.0",
        "source": POLICY_SOURCE,
        "reference": {
            "schema": REFERENCE_SCHEMA,
            "version": 1,
            "engine": "clips",
            "engine_version": "6.30",
            "package": "clips",
            "package_version": "6.30-4.1",
            "base_image": BASE_IMAGE,
            "platforms": {
                "linux/amd64": {
                    "binary_sha256": BINARY_DIGEST,
                    "library_sha256": LIBRARY_DIGEST,
                }
            },
        },
        "cases": cases,
    }


def test_equivalent_matrix_entry_passes(tmp_path: Path) -> None:
    examples_dir, _raw, _declaration, _evaluation, manifest = _fixture_evidence(tmp_path)

    report = evaluate_manifest(
        _policy(ExpectedResult(classification="equivalent")),
        manifest,
        examples_dir=examples_dir,
    )

    assert report.failures == ()
    assert report.accepted_deviations == ()


def test_swapping_one_expected_result_fails(tmp_path: Path) -> None:
    examples_dir, _raw, _declaration, _evaluation, manifest = _fixture_evidence(tmp_path)
    manifest["files"]["ferric-semantic/fr-rete-001.clp"]["classification"] = "divergent"

    report = evaluate_manifest(
        _policy(ExpectedResult(classification="equivalent")),
        manifest,
        examples_dir=examples_dir,
    )

    assert any("persisted result disagrees" in failure for failure in report.failures)


def test_exact_known_divergence_passes_but_stale_equivalence_fails(tmp_path: Path) -> None:
    examples_dir, policy, manifest = _divergence_policy(tmp_path)

    report = evaluate_manifest(policy, manifest, examples_dir=examples_dir)
    assert report.failures == ()
    assert len(report.accepted_deviations) == 1

    equivalent_examples, _raw, _declaration, _evaluation, equivalent = _fixture_evidence(
        tmp_path / "stale"
    )
    stale = evaluate_manifest(policy, equivalent, examples_dir=equivalent_examples)
    assert any("remove the stale known divergence" in failure for failure in stale.failures)


def test_known_divergence_rejects_fingerprint_drift(tmp_path: Path) -> None:
    examples_dir, policy, manifest = _divergence_policy(tmp_path)
    entry = manifest["files"]["ferric-semantic/fr-rete-001.clp"]
    changed_raw = _raw_observation(entry["oracle"], engine="ferric", value=44)
    changed = project_ferric_observation(changed_raw, harness_identity=None)
    entry["ferric"] = _result(
        changed,
        raw_observation=changed_raw,
        engine="ferric",
    )
    evaluation = evaluate_oracle(
        entry["oracle"],
        changed,
        entry["clips"]["canonical_observation"],
        expected_source_sha256=entry["source_sha256"],
        expected_composed_sha256=entry["oracle"]["composed_sha256"],
    )
    _oracle_classification, _oracle_reason, oracle_evidence = oracle_outcome(evaluation)
    entry["oracle_evidence"] = oracle_evidence
    entry["ferric"]["oracle_evidence"] = oracle_evidence
    entry["clips"]["oracle_evidence"] = oracle_evidence
    entry["classification"], entry["reason"] = classify_results(
        entry["ferric"],
        entry["clips"],
        evaluation=evaluation,
    )

    report = evaluate_manifest(policy, manifest, examples_dir=examples_dir)

    assert any("semantic fingerprint changed" in failure for failure in report.failures)


def test_clips_expectation_mismatch_is_never_an_accepted_deviation(tmp_path: Path) -> None:
    examples_dir, _raw, _declaration, _evaluation, manifest = _fixture_evidence(
        tmp_path,
        clips_value=43,
    )
    entry = manifest["files"]["ferric-semantic/fr-rete-001.clp"]
    expected = ExpectedResult(
        classification="divergent",
        reason=entry["reason"],
        mismatches=(
            MismatchPin("clips", "effects"),
            MismatchPin("clips", "facts"),
            MismatchPin("engines", "effects"),
            MismatchPin("engines", "facts"),
        ),
        ferric_fingerprint="f" * 64,
        rationale="Must not be accepted.",
        since="0.1.0",
        tracking_issue="https://github.com/plx/ferric-rules/issues/103",
    )
    policy = _policy(expected)

    report = evaluate_manifest(policy, manifest, examples_dir=examples_dir)

    assert any("pinned CLIPS does not satisfy" in failure for failure in report.failures)


def test_reference_artifact_drift_fails(tmp_path: Path) -> None:
    examples_dir, _raw, _declaration, _evaluation, manifest = _fixture_evidence(tmp_path)
    manifest["reference"]["binary_sha256"] = "f" * 64

    report = evaluate_manifest(
        _policy(ExpectedResult(classification="equivalent")),
        manifest,
        examples_dir=examples_dir,
    )

    assert any("binary_sha256 mismatch" in failure for failure in report.failures)


def test_policy_requires_every_scenario_id_exactly_once(tmp_path: Path) -> None:
    raw = _full_policy_json()
    policy_path = tmp_path / "policy.json"
    policy_path.write_text(json.dumps(raw), encoding="utf-8")
    policy = load_policy(policy_path)
    assert len(policy.cases) == 22
    assert len({case.issue for case in policy.cases}) == 20

    raw["cases"].pop()
    policy_path.write_text(json.dumps(raw), encoding="utf-8")
    with pytest.raises(SemanticGateError, match="cover exactly the required scenario IDs"):
        load_policy(policy_path)


def test_policy_rejects_stale_or_untracked_deviation(tmp_path: Path) -> None:
    raw = _full_policy_json()
    raw["cases"][0]["expected"] = {
        "classification": "divergent",
        "reason": "oracle-ferric-facts-mismatch",
        "mismatches": [{"scope": "ferric", "field": "facts"}],
        "ferric_fingerprint": "f" * 64,
        "rationale": "Known result.",
        "since": "0.1.0",
        "tracking_issue": "https://github.com/plx/ferric-rules/issues/999",
    }
    policy_path = tmp_path / "policy.json"
    policy_path.write_text(json.dumps(raw), encoding="utf-8")

    with pytest.raises(SemanticGateError, match="must match the case issue"):
        load_policy(policy_path)


@pytest.mark.parametrize("protocol_version", [None, True, 2])
def test_manifest_requires_exact_oracle_protocol_version(
    tmp_path: Path,
    protocol_version: object,
) -> None:
    examples_dir, _raw, _declaration, _evaluation, manifest = _fixture_evidence(tmp_path)
    if protocol_version is None:
        manifest.pop("oracle_protocol_version")
    else:
        manifest["oracle_protocol_version"] = protocol_version

    report = evaluate_manifest(
        _policy(ExpectedResult(classification="equivalent")),
        manifest,
        examples_dir=examples_dir,
    )

    assert "manifest must use oracle protocol version 1" in report.failures


@pytest.mark.parametrize("engine", ["ferric", "clips"])
@pytest.mark.parametrize(
    "field",
    ["diagnostic", "termination", "raw_output", "observation", "canonical_observation"],
)
def test_engine_result_cannot_drop_required_adapter_evidence(
    tmp_path: Path,
    engine: str,
    field: str,
) -> None:
    examples_dir, _raw, _declaration, _evaluation, manifest = _fixture_evidence(tmp_path)
    entry = manifest["files"]["ferric-semantic/fr-rete-001.clp"]
    entry[engine].pop(field)

    report = evaluate_manifest(
        _policy(ExpectedResult(classification="equivalent")),
        manifest,
        examples_dir=examples_dir,
    )

    assert report.failures


@pytest.mark.parametrize("location", ["entry", "ferric", "clips"])
def test_persisted_oracle_evidence_is_required_at_every_manifest_level(
    tmp_path: Path,
    location: str,
) -> None:
    examples_dir, _raw, _declaration, _evaluation, manifest = _fixture_evidence(tmp_path)
    entry = manifest["files"]["ferric-semantic/fr-rete-001.clp"]
    target = entry if location == "entry" else entry[location]
    target.pop("oracle_evidence")

    report = evaluate_manifest(
        _policy(ExpectedResult(classification="equivalent")),
        manifest,
        examples_dir=examples_dir,
    )

    assert any("persisted oracle evidence" in failure for failure in report.failures)


def test_ferric_out_of_band_stderr_and_noncanonical_json_framing_fail(tmp_path: Path) -> None:
    examples_dir, _raw, _declaration, _evaluation, manifest = _fixture_evidence(tmp_path)
    ferric = manifest["files"]["ferric-semantic/fr-rete-001.clp"]["ferric"]
    ferric["stderr"] = "unexpected\n"
    ferric["raw_output"]["stderr"] = base64.b64encode(b"unexpected\n").decode()

    stderr_report = evaluate_manifest(
        _policy(ExpectedResult(classification="equivalent")),
        manifest,
        examples_dir=examples_dir,
    )
    assert any("out-of-band stderr" in failure for failure in stderr_report.failures)

    examples_dir, _raw, _declaration, _evaluation, manifest = _fixture_evidence(
        tmp_path / "framing"
    )
    ferric = manifest["files"]["ferric-semantic/fr-rete-001.clp"]["ferric"]
    raw_stdout = b" " + base64.b64decode(ferric["raw_output"]["stdout"])
    ferric["stdout"] = raw_stdout.decode()
    ferric["raw_output"]["stdout"] = base64.b64encode(raw_stdout).decode()

    framing_report = evaluate_manifest(
        _policy(ExpectedResult(classification="equivalent")),
        manifest,
        examples_dir=examples_dir,
    )
    assert any("newline-terminated JSON" in failure for failure in framing_report.failures)


def test_clips_semantic_stdout_is_bound_to_exact_process_bytes(tmp_path: Path) -> None:
    examples_dir, _raw, _declaration, _evaluation, manifest = _fixture_evidence(tmp_path)
    clips = manifest["files"]["ferric-semantic/fr-rete-001.clp"]["clips"]
    clips["stdout"] = "substituted\n"
    clips["raw_output"]["stdout"] = base64.b64encode(b"substituted\n").decode()

    report = evaluate_manifest(
        _policy(ExpectedResult(classification="equivalent")),
        manifest,
        examples_dir=examples_dir,
    )

    assert any("raw observer transcript disagrees" in failure for failure in report.failures)


def test_clips_semantic_stderr_is_bound_to_authenticated_process_bytes(tmp_path: Path) -> None:
    examples_dir, _raw, _declaration, _evaluation, manifest = _fixture_evidence(tmp_path)
    clips = manifest["files"]["ferric-semantic/fr-rete-001.clp"]["clips"]
    original_stderr = base64.b64decode(clips["raw_output"]["stderr"])
    substituted = b"VISIBLE-FIXTURE-ERROR\n" + original_stderr
    clips["stderr"] = substituted.decode()
    clips["raw_output"]["stderr"] = base64.b64encode(substituted).decode()

    report = evaluate_manifest(
        _policy(ExpectedResult(classification="equivalent")),
        manifest,
        examples_dir=examples_dir,
    )

    assert any("raw observer transcript disagrees" in failure for failure in report.failures)


def test_policy_and_reference_versions_reject_boolean_aliases(tmp_path: Path) -> None:
    raw_policy = _full_policy_json()
    raw_policy["schema_version"] = True
    policy_path = tmp_path / "policy.json"
    policy_path.write_text(json.dumps(raw_policy), encoding="utf-8")
    with pytest.raises(SemanticGateError, match="schema_version"):
        load_policy(policy_path)

    examples_dir, _raw, _declaration, _evaluation, manifest = _fixture_evidence(
        tmp_path / "reference"
    )
    manifest["reference"]["version"] = True
    report = evaluate_manifest(
        _policy(ExpectedResult(classification="equivalent")),
        manifest,
        examples_dir=examples_dir,
    )
    assert any("reference version mismatch" in failure for failure in report.failures)


@pytest.mark.parametrize("mutation", ["normalizer", "expectation"])
def test_manifest_oracle_must_equal_the_committed_registry_declaration(
    tmp_path: Path,
    mutation: str,
) -> None:
    examples_dir, _raw, _declaration, _evaluation, manifest = _fixture_evidence(tmp_path)
    oracle = manifest["files"]["ferric-semantic/fr-rete-001.clp"]["oracle"]
    if mutation == "normalizer":
        oracle["normalizers"].append("fact-order")
    else:
        oracle["expectations"]["channels"]["stdout"] = "substituted\n"

    report = evaluate_manifest(
        _policy(ExpectedResult(classification="equivalent")),
        manifest,
        examples_dir=examples_dir,
    )

    assert any("committed registry declaration" in failure for failure in report.failures)
