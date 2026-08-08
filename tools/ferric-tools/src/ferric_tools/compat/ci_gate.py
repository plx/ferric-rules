"""Fail-closed outer gate for the blocking compatibility CI lane."""

from __future__ import annotations

import copy
import json
import re
import subprocess
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path, PurePosixPath, PureWindowsPath
from typing import Annotated

import typer
from rich.console import Console

from ferric_tools._harness import (
    HARNESS_GENERATION_VERSION,
    HarnessContractError,
    atomic_write_bytes,
    resolve_harness_contract,
    sha256_bytes,
)
from ferric_tools._paths import examples_dir as default_examples_dir
from ferric_tools._paths import repo_root
from ferric_tools.compat.oracle import (
    EvidenceStatus,
    OracleDeclaration,
    evaluate_oracle,
    validate_declaration,
)
from ferric_tools.compat.run import (
    CandidateProvenanceError,
    candidate_provenance,
    classify_results,
    oracle_outcome,
)
from ferric_tools.compat.semantic_gate import (
    GateReport,
    SemanticGateError,
    SemanticPolicy,
    validated_engine_observation,
)
from ferric_tools.compat.semantic_gate import (
    evaluate_manifest as evaluate_semantic_manifest,
)
from ferric_tools.compat.semantic_gate import (
    load_policy as load_semantic_policy,
)

POLICY_SCHEMA_VERSION = 1
POLICY_SCHEMA = "ferric.compat-ci-policy"
CANDIDATE_SCHEMA = "ferric.compat-candidate-provenance"
CANDIDATE_VERSION = 1
MANIFEST_VERSION = 3
ORACLE_PROTOCOL_VERSION = 1
REPORT_SCHEMA = "ferric.compat-ci-gate-report"
REPORT_VERSION = 1
SUMMARY_FIELD_ORDER = ("total", "equivalent", "divergent", "incompatible", "pending")
SUMMARY_FIELDS = frozenset(SUMMARY_FIELD_ORDER)
COMPLETED_CLASSIFICATIONS = frozenset({"equivalent", "divergent"})
ALL_CLASSIFICATIONS = frozenset({"equivalent", "divergent", "incompatible", "pending"})
_COMMIT_SHA_RE = re.compile(r"[0-9a-f]{40}")
_SHA256_RE = re.compile(r"[0-9a-f]{64}")

_CONTROL_EFFECTS = (
    {
        "name": "fact:MAIN::result",
        "value": {
            "type": "multifield",
            "value": [{"type": "integer", "value": 42}],
        },
    },
)
_CONTROL_CHANNELS = {"stdout": "", "stderr": ""}
_LOCKED_CONTROL = {
    "fixture": "ferric-oracle/empty-output-state.clp",
    "id": "ferric-oracle.empty-output-state",
    "source": "ferric-oracle",
    "declaration_version": 1,
    "runability": "library",
    "harness_generation_version": HARNESS_GENERATION_VERSION,
    "expected_firings": 0,
    "expected_effects": list(_CONTROL_EFFECTS),
    "expected_channels": _CONTROL_CHANNELS,
}
_UNCLAIMED_MISSING_EVIDENCE = {
    "status": "missing",
    "version": 1,
    "declaration": False,
    "reached": False,
    "completed": False,
    "effect": False,
    "normalizations": [],
    "violations": [],
}

app = typer.Typer(help="Enforce the complete, non-vacuous compatibility CI policy.")
console = Console(stderr=True)


class CIGateError(ValueError):
    """The outer policy or its physical evidence violates the CI contract."""


@dataclass(frozen=True)
class EquivalentControl:
    """One non-semantic registry fixture that must prove exact equivalence."""

    fixture: str
    id: str
    source: str
    declaration_version: int
    runability: str
    harness_generation_version: int
    expected_firings: int
    expected_effects: tuple[dict[str, object], ...]
    expected_channels: dict[str, str]


@dataclass(frozen=True)
class CIGatePolicy:
    """Validated outer-gate selection and non-vacuity policy."""

    schema: str
    schema_version: int
    semantic_policy: str
    required_equivalent_controls: tuple[EquivalentControl, ...]


@dataclass(frozen=True)
class CIGateReport:
    """Failures and accepted semantic deviations from one outer-gate evaluation."""

    failures: tuple[str, ...]
    accepted_deviations: tuple[str, ...]


def gate_report_payload(
    report: CIGateReport,
    *,
    claimed_outcomes: int | None,
) -> dict[str, object]:
    """Build the stable machine-readable result emitted for every gate invocation."""
    failures = sorted(report.failures)
    deviations = sorted(report.accepted_deviations)
    return {
        "schema": REPORT_SCHEMA,
        "version": REPORT_VERSION,
        "status": "failed" if failures else "passed",
        "claimed_outcomes": claimed_outcomes,
        "failure_count": len(failures),
        "accepted_deviation_count": len(deviations),
        "failures": failures,
        "accepted_deviations": deviations,
    }


def _markdown_line(value: str) -> str:
    return " ".join(value.splitlines())


def gate_report_markdown(payload: dict[str, object]) -> str:
    """Render the deterministic gate result as a concise human-readable report."""
    status = "PASS" if payload["status"] == "passed" else "FAIL"
    claimed = payload["claimed_outcomes"]
    claimed_text = "unavailable" if claimed is None else str(claimed)
    failures = payload["failures"]
    deviations = payload["accepted_deviations"]
    assert isinstance(failures, list)
    assert isinstance(deviations, list)
    lines = [
        "# Compatibility CI gate report",
        "",
        f"- Status: **{status}**",
        f"- Claimed outcomes: {claimed_text}",
        f"- Failures: {payload['failure_count']}",
        f"- Accepted known divergences: {payload['accepted_deviation_count']}",
        "",
        "## Failures",
        "",
    ]
    lines.extend([f"- {_markdown_line(str(failure))}" for failure in failures] or ["- None"])
    lines.extend(["", "## Accepted known divergences", ""])
    lines.extend([f"- {_markdown_line(str(deviation))}" for deviation in deviations] or ["- None"])
    return "\n".join(lines) + "\n"


def write_gate_reports(
    report: CIGateReport,
    *,
    claimed_outcomes: int | None,
    json_path: Path,
    markdown_path: Path,
) -> None:
    """Atomically persist deterministic JSON and Markdown gate artifacts."""
    try:
        if json_path.resolve(strict=False) == markdown_path.resolve(strict=False):
            raise CIGateError("JSON and Markdown compatibility CI gate reports must be distinct")
    except (OSError, RuntimeError) as error:
        raise CIGateError(f"cannot resolve compatibility CI gate report paths: {error}") from error
    payload = gate_report_payload(report, claimed_outcomes=claimed_outcomes)
    json_bytes = (json.dumps(payload, indent=2, sort_keys=True) + "\n").encode("utf-8")
    markdown_bytes = gate_report_markdown(payload).encode("utf-8")
    try:
        atomic_write_bytes(json_path, json_bytes)
        atomic_write_bytes(markdown_path, markdown_bytes)
    except OSError as error:
        raise CIGateError(f"cannot write compatibility CI gate reports: {error}") from error


def _reject_duplicate_fields(pairs: list[tuple[str, object]]) -> dict[str, object]:
    result: dict[str, object] = {}
    for key, value in pairs:
        if key in result:
            raise CIGateError(f"duplicate JSON field: {key!r}")
        result[key] = value
    return result


def load_json(path: Path, *, label: str) -> object:
    """Load JSON while rejecting duplicate object fields."""
    try:
        return json.loads(
            path.read_text(encoding="utf-8"),
            object_pairs_hook=_reject_duplicate_fields,
        )
    except (OSError, UnicodeError, json.JSONDecodeError, CIGateError) as error:
        raise CIGateError(f"cannot load {label} {path}: {error}") from error


def _exact_object(
    raw: object,
    *,
    label: str,
    fields: frozenset[str],
) -> dict[str, object]:
    if type(raw) is not dict:
        raise CIGateError(f"{label} must be an object")
    assert isinstance(raw, dict)
    if set(raw) != fields:
        missing = sorted(fields - set(raw))
        extra = sorted(set(raw) - fields)
        raise CIGateError(f"{label} has unexpected fields (missing={missing}, extra={extra})")
    return raw


def _string(raw: object, *, label: str) -> str:
    if type(raw) is not str or not raw:
        raise CIGateError(f"{label} must be a non-empty string")
    return raw


def _integer(raw: object, *, label: str, minimum: int = 0) -> int:
    if type(raw) is not int or raw < minimum:
        raise CIGateError(f"{label} must be an integer greater than or equal to {minimum}")
    return raw


def _normalized_path(raw: object, *, label: str) -> str:
    value = _string(raw, label=label)
    if "\\" in value or any(ord(character) < 32 or ord(character) == 127 for character in value):
        raise CIGateError(f"{label} must be a normalized POSIX relative path")
    path = PurePosixPath(value)
    windows_path = PureWindowsPath(value)
    if (
        path.is_absolute()
        or windows_path.is_absolute()
        or windows_path.drive
        or ".." in path.parts
        or path.as_posix() != value
    ):
        raise CIGateError(f"{label} must be a normalized POSIX relative path")
    return value


def _typed_equal(left: object, right: object) -> bool:
    if type(left) is not type(right):
        return False
    if isinstance(left, dict) and isinstance(right, dict):
        return set(left) == set(right) and all(_typed_equal(left[key], right[key]) for key in left)
    if isinstance(left, list) and isinstance(right, list):
        return len(left) == len(right) and all(
            _typed_equal(left_item, right_item)
            for left_item, right_item in zip(left, right, strict=True)
        )
    return left == right


def _control_policy(raw: object, *, index: int) -> EquivalentControl:
    label = f"required_equivalent_controls[{index}]"
    control = _exact_object(
        raw,
        label=label,
        fields=frozenset(_LOCKED_CONTROL),
    )
    if not _typed_equal(control, _LOCKED_CONTROL):
        raise CIGateError(f"{label} must equal the locked empty-output-state control contract")
    return EquivalentControl(
        fixture=_normalized_path(control["fixture"], label=f"{label}.fixture"),
        id=_string(control["id"], label=f"{label}.id"),
        source=_string(control["source"], label=f"{label}.source"),
        declaration_version=_integer(
            control["declaration_version"], label=f"{label}.declaration_version", minimum=1
        ),
        runability=_string(control["runability"], label=f"{label}.runability"),
        harness_generation_version=_integer(
            control["harness_generation_version"],
            label=f"{label}.harness_generation_version",
            minimum=1,
        ),
        expected_firings=_integer(control["expected_firings"], label=f"{label}.expected_firings"),
        expected_effects=tuple(copy.deepcopy(control["expected_effects"])),
        expected_channels=dict(control["expected_channels"]),
    )


def load_policy(path: Path) -> CIGatePolicy:
    """Load the exact committed outer compatibility policy."""
    raw = _exact_object(
        load_json(path, label="compatibility CI policy"),
        label="compatibility CI policy",
        fields=frozenset(
            {"schema", "schema_version", "semantic_policy", "required_equivalent_controls"}
        ),
    )
    if raw["schema"] != POLICY_SCHEMA or type(raw["schema"]) is not str:
        raise CIGateError(f"policy schema must equal {POLICY_SCHEMA!r}")
    if raw["schema_version"] != POLICY_SCHEMA_VERSION or type(raw["schema_version"]) is not int:
        raise CIGateError(f"policy schema_version must equal {POLICY_SCHEMA_VERSION}")
    semantic_policy = _normalized_path(raw["semantic_policy"], label="semantic_policy")
    if semantic_policy != "compat-semantic-policy.json":
        raise CIGateError("semantic_policy must equal 'compat-semantic-policy.json'")
    raw_controls = raw["required_equivalent_controls"]
    if type(raw_controls) is not list:
        raise CIGateError("required_equivalent_controls must be an array")
    controls = tuple(_control_policy(item, index=index) for index, item in enumerate(raw_controls))
    if [control.fixture for control in controls] != [_LOCKED_CONTROL["fixture"]]:
        raise CIGateError(
            "required_equivalent_controls must contain exactly the locked control fixture"
        )
    return CIGatePolicy(
        schema=POLICY_SCHEMA,
        schema_version=POLICY_SCHEMA_VERSION,
        semantic_policy=semantic_policy,
        required_equivalent_controls=controls,
    )


def _load_registry(examples_dir: Path) -> dict[str, dict[str, object]]:
    root = _exact_object(
        load_json(examples_dir / "compat-oracles.json", label="oracle registry"),
        label="oracle registry",
        fields=frozenset({"version", "fixtures"}),
    )
    if type(root["version"]) is not int or root["version"] != 1:
        raise CIGateError("oracle registry version must equal 1")
    raw_fixtures = root["fixtures"]
    if type(raw_fixtures) is not dict:
        raise CIGateError("oracle registry fixtures must be an object")
    assert isinstance(raw_fixtures, dict)
    fixtures: dict[str, dict[str, object]] = {}
    for raw_path, raw_declaration in raw_fixtures.items():
        path = _normalized_path(raw_path, label="oracle registry fixture path")
        if type(raw_declaration) is not dict:
            raise CIGateError(f"oracle registry declaration for {path!r} must be an object")
        assert isinstance(raw_declaration, dict)
        fixtures[path] = raw_declaration
    return fixtures


def _summary_failures(manifest: dict[str, object]) -> tuple[list[str], dict[str, dict] | None]:
    failures: list[str] = []
    raw_files = manifest.get("files")
    if type(raw_files) is not dict:
        return ["manifest files must be an object"], None
    assert isinstance(raw_files, dict)
    counts = {field: 0 for field in SUMMARY_FIELD_ORDER}
    files: dict[str, dict] = {}
    for raw_path, raw_entry in raw_files.items():
        try:
            path = _normalized_path(raw_path, label="manifest file path")
        except CIGateError as error:
            failures.append(str(error))
            continue
        counts["total"] += 1
        if type(raw_entry) is not dict:
            failures.append(f"{path}: manifest entry must be an object")
            continue
        assert isinstance(raw_entry, dict)
        files[path] = raw_entry
        classification = raw_entry.get("classification")
        if type(classification) is not str or classification not in ALL_CLASSIFICATIONS:
            failures.append(f"{path}: classification is missing or unsupported")
            continue
        counts[classification] += 1

    raw_summary = manifest.get("summary")
    if type(raw_summary) is not dict or set(raw_summary) != SUMMARY_FIELDS:
        failures.append("manifest summary must contain exactly the five classification counts")
        return failures, files
    assert isinstance(raw_summary, dict)
    malformed = [
        field
        for field in SUMMARY_FIELD_ORDER
        if type(raw_summary[field]) is not int or raw_summary[field] < 0
    ]
    if malformed:
        failures.append(f"manifest summary has malformed counts: {sorted(malformed)}")
    elif raw_summary != counts:
        failures.append(f"manifest summary is stale (expected {counts}, got {raw_summary})")
    if counts["total"] == 0:
        failures.append("compatibility manifest contains zero files")
    return failures, files


def _candidate_failure(
    manifest: dict[str, object],
    *,
    ferric_bin: Path,
    expected_commit_sha: str,
) -> str | None:
    if (
        type(expected_commit_sha) is not str
        or _COMMIT_SHA_RE.fullmatch(expected_commit_sha) is None
    ):
        return "expected commit SHA must be exactly 40 lowercase hexadecimal characters"
    try:
        measured = candidate_provenance(ferric_bin, commit_sha=expected_commit_sha)
    except CandidateProvenanceError as error:
        return str(error)
    candidate = manifest.get("candidate")
    if type(candidate) is not dict:
        return "manifest candidate provenance is missing or malformed"
    assert isinstance(candidate, dict)
    expected_fields = {"schema", "version", "commit_sha", "binary_sha256"}
    if set(candidate) != expected_fields:
        return "manifest candidate provenance must contain exactly the four required fields"
    if (
        candidate.get("schema") != CANDIDATE_SCHEMA
        or type(candidate.get("schema")) is not str
        or candidate.get("version") != CANDIDATE_VERSION
        or type(candidate.get("version")) is not int
        or type(candidate.get("commit_sha")) is not str
        or _COMMIT_SHA_RE.fullmatch(candidate["commit_sha"]) is None
        or type(candidate.get("binary_sha256")) is not str
        or _SHA256_RE.fullmatch(candidate["binary_sha256"]) is None
    ):
        return "manifest candidate provenance fields are malformed"
    if candidate != measured:
        return "manifest candidate provenance does not match the expected commit and binary bytes"
    return None


def _runtime_control_declaration(
    control: EquivalentControl,
    entry: dict,
    registry_declaration: dict[str, object],
    *,
    source_digest: str,
    composed_digest: str,
) -> tuple[dict[str, object], OracleDeclaration]:
    raw_ferric = entry.get("ferric")
    if type(raw_ferric) is not dict:
        raise CIGateError(f"{control.id}: Ferric result is missing")
    assert isinstance(raw_ferric, dict)
    raw_observation = raw_ferric.get("observation")
    if type(raw_observation) is not dict:
        raise CIGateError(f"{control.id}: Ferric raw observation is missing")
    assert isinstance(raw_observation, dict)
    fixture = raw_observation.get("fixture")
    if type(fixture) is not dict or type(fixture.get("nonce")) is not str:
        raise CIGateError(f"{control.id}: Ferric runtime nonce is missing")
    assert isinstance(fixture, dict)
    runtime = copy.deepcopy(registry_declaration)
    runtime["nonce"] = fixture["nonce"]

    evidence = validate_declaration(
        runtime,
        expected_source_sha256=source_digest,
        expected_composed_sha256=composed_digest,
    )
    if evidence.status is not EvidenceStatus.VALID or evidence.value is None:
        details = "; ".join(f"{issue.field}: {issue.message}" for issue in evidence.issues)
        raise CIGateError(f"{control.id}: invalid runtime oracle declaration: {details}")
    return runtime, evidence.value


def _control_declaration_contract(
    control: EquivalentControl,
    declaration: dict[str, object],
) -> None:
    if (
        type(declaration.get("version")) is not int
        or declaration["version"] != control.declaration_version
    ):
        raise CIGateError(
            f"{control.id}: oracle declaration version must equal {control.declaration_version}"
        )
    if declaration.get("id") != control.id or type(declaration.get("id")) is not str:
        raise CIGateError(f"{control.id}: oracle declaration ID is stale")
    expectations = declaration.get("expectations")
    if type(expectations) is not dict:
        raise CIGateError(f"{control.id}: oracle expectations are missing")
    assert isinstance(expectations, dict)
    firings = expectations.get("firings")
    if type(firings) is not dict or set(firings) != {"count", "names"}:
        raise CIGateError(f"{control.id}: oracle firing expectation is malformed")
    assert isinstance(firings, dict)
    if (
        type(firings.get("count")) is not int
        or firings.get("count") != control.expected_firings
        or firings.get("names") is not None
    ):
        raise CIGateError(f"{control.id}: oracle firing expectation is stale")
    if not _typed_equal(expectations.get("effects"), list(control.expected_effects)):
        raise CIGateError(f"{control.id}: oracle effects do not match the non-vacuity policy")
    if not _typed_equal(expectations.get("channels"), control.expected_channels):
        raise CIGateError(f"{control.id}: oracle channel expectations are stale")


def _exact_harness_contract(control: EquivalentControl, entry: dict) -> dict[str, object]:
    contract = entry.get("harness")
    fields = {
        "path",
        "source_sha256",
        "harness_sha256",
        "generation_version",
        "executable",
    }
    if type(contract) is not dict or set(contract) != fields:
        raise CIGateError(
            f"{control.id}: entry harness metadata must contain exactly the executable shape"
        )
    assert isinstance(contract, dict)
    if (
        type(contract.get("generation_version")) is not int
        or contract["generation_version"] != control.harness_generation_version
        or contract.get("executable") is not True
    ):
        raise CIGateError(f"{control.id}: entry harness is not the required executable version")
    return contract


def _validate_control(
    control: EquivalentControl,
    entry: dict,
    registry_declaration: dict[str, object],
    *,
    examples_dir: Path,
    root: Path,
) -> None:
    if entry.get("source") != control.source or type(entry.get("source")) is not str:
        raise CIGateError(f"{control.id}: manifest source must equal {control.source!r}")
    if entry.get("runability") != control.runability or type(entry.get("runability")) is not str:
        raise CIGateError(f"{control.id}: manifest runability must equal {control.runability!r}")
    if not _typed_equal(entry.get("oracle"), registry_declaration):
        raise CIGateError(f"{control.id}: manifest oracle does not match the committed registry")
    _control_declaration_contract(control, registry_declaration)

    source_path = examples_dir.joinpath(*PurePosixPath(control.fixture).parts)
    if source_path.is_symlink():
        raise CIGateError(f"{control.id}: control source must not be a symlink")
    contract = _exact_harness_contract(control, entry)
    harness_relpath = contract.get("path")
    if type(harness_relpath) is not str:
        raise CIGateError(f"{control.id}: executable harness path must be a string")
    normalized_harness = _normalized_path(harness_relpath, label=f"{control.id}: harness.path")
    if root.joinpath(*PurePosixPath(normalized_harness).parts).is_symlink():
        raise CIGateError(f"{control.id}: generated harness must not be a symlink")
    try:
        resolved = resolve_harness_contract(
            entry,
            source_path=source_path,
            root=root,
            manifest_key=control.fixture,
        )
    except HarnessContractError as error:
        raise CIGateError(f"{control.id}: {error}") from error
    if resolved is None or not _typed_equal(resolved.metadata, contract):
        raise CIGateError(f"{control.id}: executable harness did not physically resolve")

    source_digest = sha256_bytes(resolved.source_bytes)
    composed_bytes = resolved.source_bytes + b"\n" + resolved.harness_bytes
    composed_digest = sha256_bytes(composed_bytes)
    if entry.get("source_sha256") != source_digest:
        raise CIGateError(f"{control.id}: manifest source digest is stale")
    if registry_declaration.get("source_sha256") != source_digest:
        raise CIGateError(f"{control.id}: registry source digest is stale")
    if registry_declaration.get("composed_sha256") != composed_digest:
        raise CIGateError(f"{control.id}: registry composed digest is stale")

    ferric = entry.get("ferric")
    clips = entry.get("clips")
    if type(ferric) is not dict or type(clips) is not dict:
        raise CIGateError(f"{control.id}: both engine results are required")
    assert isinstance(ferric, dict)
    assert isinstance(clips, dict)
    expected_composed = {"sha256": composed_digest, "size_bytes": len(composed_bytes)}
    for engine, result in (("ferric", ferric), ("clips", clips)):
        if not _typed_equal(result.get("harness"), contract):
            raise CIGateError(f"{control.id}: {engine} harness metadata is missing or stale")
        if not _typed_equal(result.get("composed_source"), expected_composed):
            raise CIGateError(
                f"{control.id}: {engine} composed-source evidence is missing or stale"
            )

    runtime_raw, declaration = _runtime_control_declaration(
        control,
        entry,
        registry_declaration,
        source_digest=source_digest,
        composed_digest=composed_digest,
    )
    try:
        ferric_observation = validated_engine_observation(
            control.id,
            engine="ferric",
            raw_result=ferric,
            declaration=declaration,
            harness_identity=resolved.verifier_identity,
        )
        clips_observation = validated_engine_observation(
            control.id,
            engine="clips",
            raw_result=clips,
            declaration=declaration,
            harness_identity=resolved.verifier_identity,
        )
    except SemanticGateError as error:
        raise CIGateError(str(error)) from error

    evaluation = evaluate_oracle(
        runtime_raw,
        ferric_observation,
        clips_observation,
        expected_source_sha256=source_digest,
        expected_composed_sha256=composed_digest,
    )
    oracle_classification, oracle_reason, evidence = oracle_outcome(evaluation)
    persisted_evidence = {
        "entry": entry.get("oracle_evidence"),
        "ferric": ferric.get("oracle_evidence"),
        "clips": clips.get("oracle_evidence"),
    }
    stale = [
        label for label, value in persisted_evidence.items() if not _typed_equal(value, evidence)
    ]
    if stale:
        raise CIGateError(
            f"{control.id}: persisted oracle evidence is missing or stale in {', '.join(stale)}"
        )
    classification, reason = classify_results(ferric, clips, evaluation=evaluation)
    if entry.get("classification") != classification or entry.get("reason") != reason:
        raise CIGateError(
            f"{control.id}: persisted result disagrees with recomputation "
            f"({entry.get('classification')!r}/{entry.get('reason')!r} != "
            f"{classification!r}/{reason!r})"
        )
    if (
        evaluation.status is not EvidenceStatus.VALID
        or not evaluation.equivalent
        or oracle_classification != "equivalent"
        or oracle_reason != "oracle-v1-match"
        or classification != "equivalent"
        or reason != "oracle-v1-match"
    ):
        raise CIGateError(
            f"{control.id}: required equivalent/oracle-v1-match, got {classification}/{reason}"
        )
    if (
        evidence.get("status") != "valid"
        or evidence.get("declaration") is not True
        or evidence.get("reached") is not True
        or evidence.get("completed") is not True
        or evidence.get("effect") is not True
        or evidence.get("violations") != []
    ):
        raise CIGateError(f"{control.id}: oracle evidence is incomplete or vacuous")
    for engine, observation in (
        ("ferric", ferric_observation),
        ("clips", clips_observation),
    ):
        if not observation.get("effects"):
            raise CIGateError(f"{control.id}: {engine} observation has no semantic effect")
        if observation.get("firings") != []:
            raise CIGateError(f"{control.id}: {engine} fixture firing evidence is not zero")


def evaluate_manifest(
    policy: CIGatePolicy,
    semantic_policy: SemanticPolicy,
    manifest: object,
    *,
    examples_dir: Path,
    root: Path,
    ferric_bin: Path,
    expected_commit_sha: str,
    semantic_evaluator: Callable[..., GateReport] = evaluate_semantic_manifest,
) -> CIGateReport:
    """Evaluate the full claimed compatibility result set and its physical inputs."""
    failures: list[str] = []
    accepted: list[str] = []
    try:
        semantic_report = semantic_evaluator(
            semantic_policy,
            manifest,
            examples_dir=examples_dir,
        )
    except Exception as error:
        failures.append(f"semantic gate could not evaluate: {error}")
    else:
        failures.extend(f"semantic: {failure}" for failure in semantic_report.failures)
        accepted.extend(semantic_report.accepted_deviations)

    if type(manifest) is not dict:
        failures.append("manifest root must be an object")
        return CIGateReport(tuple(failures), tuple(accepted))
    assert isinstance(manifest, dict)
    if type(manifest.get("version")) is not int or manifest.get("version") != MANIFEST_VERSION:
        failures.append(f"manifest version must equal {MANIFEST_VERSION}")
    if (
        type(manifest.get("oracle_protocol_version")) is not int
        or manifest.get("oracle_protocol_version") != ORACLE_PROTOCOL_VERSION
    ):
        failures.append(f"manifest oracle_protocol_version must equal {ORACLE_PROTOCOL_VERSION}")

    candidate_failure = _candidate_failure(
        manifest,
        ferric_bin=ferric_bin,
        expected_commit_sha=expected_commit_sha,
    )
    if candidate_failure is not None:
        failures.append(candidate_failure)
    summary_failures, files = _summary_failures(manifest)
    failures.extend(summary_failures)
    if files is None:
        return CIGateReport(tuple(failures), tuple(accepted))

    semantic_fixtures = {case.fixture for case in semantic_policy.cases}
    control_fixtures = {control.fixture for control in policy.required_equivalent_controls}
    if semantic_fixtures & control_fixtures:
        failures.append("semantic and equivalent-control fixture sets overlap")
    claimed_fixtures = semantic_fixtures | control_fixtures
    if not claimed_fixtures:
        failures.append("compatibility CI policy claims zero fixtures")
    missing_manifest = claimed_fixtures - files.keys()
    if missing_manifest:
        failures.append(f"claimed manifest entries are missing: {sorted(missing_manifest)}")

    unexplained = sorted(
        path
        for path, entry in files.items()
        if type(entry.get("classification")) is str
        and entry.get("classification") in COMPLETED_CLASSIFICATIONS
        and path not in claimed_fixtures
    )
    if unexplained:
        failures.append(f"completed outcomes are not claimed by policy: {unexplained}")
    evidenced_unclaimed = sorted(
        path
        for path, entry in files.items()
        if path not in claimed_fixtures
        and (
            entry.get("ferric") is not None
            or entry.get("clips") is not None
            or entry.get("oracle") is not None
            or (
                entry.get("oracle_evidence") is not None
                and not _typed_equal(
                    entry.get("oracle_evidence"),
                    _UNCLAIMED_MISSING_EVIDENCE,
                )
            )
        )
    )
    if evidenced_unclaimed:
        failures.append(
            f"unclaimed entries contain engine or oracle evidence: {evidenced_unclaimed}"
        )
    incomplete = sorted(
        path
        for path in claimed_fixtures & files.keys()
        if type(files[path].get("classification")) is not str
        or files[path].get("classification") not in COMPLETED_CLASSIFICATIONS
        or type(files[path].get("ferric")) is not dict
        or type(files[path].get("clips")) is not dict
    )
    if incomplete:
        failures.append(f"claimed outcomes are missing or partial: {incomplete}")

    try:
        registry = _load_registry(examples_dir)
    except CIGateError as error:
        failures.append(str(error))
        registry = None
    if registry is not None and set(registry) != claimed_fixtures:
        failures.append(
            "oracle registry membership mismatch "
            f"(missing={sorted(claimed_fixtures - registry.keys())}, "
            f"extra={sorted(registry.keys() - claimed_fixtures)})"
        )

    if registry is not None:
        for control in policy.required_equivalent_controls:
            entry = files.get(control.fixture)
            declaration = registry.get(control.fixture)
            if type(entry) is not dict or type(declaration) is not dict:
                continue
            assert isinstance(entry, dict)
            assert isinstance(declaration, dict)
            try:
                _validate_control(
                    control,
                    entry,
                    declaration,
                    examples_dir=examples_dir,
                    root=root,
                )
            except (CIGateError, OSError) as error:
                failures.append(str(error))
            except Exception as error:
                failures.append(
                    f"{control.id}: control validation failed closed: "
                    f"{type(error).__name__}: {error}"
                )

    return CIGateReport(tuple(failures), tuple(accepted))


def _git_head(root: Path) -> str:
    try:
        result = subprocess.run(
            ["git", "rev-parse", "HEAD"],
            cwd=root,
            check=False,
            capture_output=True,
            text=True,
            timeout=10,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise CIGateError(f"cannot resolve expected Git commit SHA: {error}") from error
    commit_sha = result.stdout.strip()
    if result.returncode != 0 or _COMMIT_SHA_RE.fullmatch(commit_sha) is None:
        detail = result.stderr.strip() or f"exit code {result.returncode}"
        raise CIGateError(f"cannot resolve expected Git commit SHA: {detail}")
    return commit_sha


@app.command()
def main(
    policy_path: Annotated[
        Path | None,
        typer.Option("--policy", help="Path to the committed compatibility CI policy."),
    ] = None,
    semantic_policy_path: Annotated[
        Path | None,
        typer.Option("--semantic-policy", help="Path to the semantic differential policy."),
    ] = None,
    manifest_path: Annotated[
        Path | None,
        typer.Option("--manifest", help="Path to the compatibility manifest."),
    ] = None,
    ferric_bin: Annotated[
        Path | None,
        typer.Option("--ferric-bin", help="Exact Ferric candidate binary to rehash."),
    ] = None,
    expected_commit_sha: Annotated[
        str | None,
        typer.Option(
            "--expected-commit-sha",
            help="Expected 40-character candidate commit (default: git rev-parse HEAD).",
        ),
    ] = None,
    report_json: Annotated[
        Path | None,
        typer.Option(
            "--report-json",
            help="JSON gate artifact (default: .ferric-compat/compat-ci-gate.json).",
        ),
    ] = None,
    report_markdown: Annotated[
        Path | None,
        typer.Option(
            "--report-markdown",
            help="Markdown gate artifact (default: .ferric-compat/compat-ci-gate.md).",
        ),
    ] = None,
) -> None:
    """Reject incomplete, stale, vacuous, or unexplained compatibility outcomes."""
    root = repo_root().resolve()
    examples_dir = default_examples_dir().resolve()
    policy_file = policy_path or examples_dir / "compat-ci-policy.json"
    manifest_file = manifest_path or examples_dir / "compat-manifest.json"
    candidate_binary = ferric_bin or root / "target" / "release" / "ferric"
    report_dir = root / ".ferric-compat"
    json_report_file = report_json or report_dir / "compat-ci-gate.json"
    markdown_report_file = report_markdown or report_dir / "compat-ci-gate.md"
    claimed_outcomes: int | None = None
    try:
        policy = load_policy(policy_file)
        semantic_file = examples_dir / policy.semantic_policy
        if semantic_policy_path is not None:
            try:
                requested_semantic_file = semantic_policy_path.resolve(strict=True)
                committed_semantic_file = semantic_file.resolve(strict=True)
            except (OSError, RuntimeError) as error:
                raise CIGateError(f"cannot resolve semantic policy path: {error}") from error
            if requested_semantic_file != committed_semantic_file:
                raise CIGateError(
                    "--semantic-policy must resolve to the committed policy selected by "
                    "compat-ci-policy.json"
                )
        semantic_policy = load_semantic_policy(semantic_file)
        claimed_outcomes = len(semantic_policy.cases) + len(policy.required_equivalent_controls)
        manifest = load_json(manifest_file, label="compatibility manifest")
        commit_sha = expected_commit_sha or _git_head(root)
        report = evaluate_manifest(
            policy,
            semantic_policy,
            manifest,
            examples_dir=examples_dir,
            root=root,
            ferric_bin=candidate_binary,
            expected_commit_sha=commit_sha,
        )
    except (CIGateError, SemanticGateError) as error:
        report = CIGateReport((str(error),), ())
        try:
            write_gate_reports(
                report,
                claimed_outcomes=claimed_outcomes,
                json_path=json_report_file,
                markdown_path=markdown_report_file,
            )
        except CIGateError as report_error:
            console.print(f"[red]error:[/] {report_error}")
        console.print(f"[red]error:[/] {error}")
        raise typer.Exit(1) from error

    try:
        write_gate_reports(
            report,
            claimed_outcomes=claimed_outcomes,
            json_path=json_report_file,
            markdown_path=markdown_report_file,
        )
    except CIGateError as error:
        console.print(f"[red]error:[/] {error}")
        raise typer.Exit(1) from error

    if report.failures:
        for failure in report.failures:
            console.print(f"[red]FAIL[/] {failure}")
        raise typer.Exit(1)

    assert claimed_outcomes is not None
    console.print(
        f"[green]compatibility CI gate passed[/]: {claimed_outcomes} claimed outcomes, "
        f"{len(report.accepted_deviations)} exact known divergences"
    )
    for deviation in report.accepted_deviations:
        console.print(f"[yellow]KNOWN DIVERGENCE[/] {deviation}")


if __name__ == "__main__":
    app()
