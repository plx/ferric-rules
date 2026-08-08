"""Fail-closed policy gate for the pinned-CLIPS semantic regression matrix."""

from __future__ import annotations

import base64
import binascii
import copy
import json
import re
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
from typing import Annotated

import typer
from rich.console import Console

from ferric_tools._harness import sha256_bytes
from ferric_tools._paths import examples_dir as default_examples_dir
from ferric_tools.compat.diagnostics import diagnostic, validated_result_diagnostic
from ferric_tools.compat.oracle import (
    EvidenceStatus,
    OracleDeclaration,
    evaluate_oracle,
    observation_semantic_fingerprint,
    scenario_plan_sha256,
    validate_declaration,
    validate_scenario_source_sizes,
)
from ferric_tools.compat.projection import (
    ObservationProjectionError,
    project_clips_observation,
    project_ferric_observation,
    project_observation_diagnostic,
)
from ferric_tools.compat.run import classify_results, oracle_outcome

POLICY_SCHEMA_VERSION = 1
POLICY_SOURCE = "ferric-semantic"
REFERENCE_SCHEMA = "ferric.clips-reference-provenance"
REFERENCE_VERSION = 1
_SHA256_RE = re.compile(r"[0-9a-f]{64}")
_ISSUE_RE = re.compile(r"https://github\.com/plx/ferric-rules/issues/[1-9][0-9]*")
_PLATFORM_RE = re.compile(r"linux/(?:amd64|arm64)")
_BASE_IMAGE_RE = re.compile(r"debian:bookworm-slim@sha256:[0-9a-f]{64}")
_IMAGE_ID_RE = re.compile(r"sha256:[0-9a-f]{64}")

REQUIRED_CASE_ISSUES = {
    "FR-LANG-001": 98,
    "FR-LANG-002": 99,
    "FR-RETE-001": 103,
    "FR-RETE-002": 104,
    "FR-RETE-003": 105,
    "FR-RETE-004": 106,
    "FR-RETE-005": 107,
    "FR-RETE-006": 108,
    "FR-RETE-007": 109,
    "FR-RETE-008": 154,
    "FR-RETE-008-BREADTH": 154,
    "FR-RETE-009": 155,
    "FR-RETE-009-MEA": 155,
    "FR-RETE-010": 156,
    "FR-RETE-011": 157,
    "FR-RETE-012": 158,
    "FR-RETE-013": 159,
    "FR-RETE-014": 191,
    "FR-RETE-015": 160,
    "FR-RETE-016": 192,
    "FR-RETE-017": 193,
    "FR-RETE-018": 161,
}

app = typer.Typer(help="Enforce the pinned-CLIPS semantic differential policy.")
console = Console(stderr=True)


class SemanticGateError(ValueError):
    """The policy or compatibility evidence violates the semantic-lane contract."""


@dataclass(frozen=True)
class MismatchPin:
    """One exact mismatch scope and field accepted by a known deviation."""

    scope: str
    field: str


@dataclass(frozen=True)
class ExpectedResult:
    """The only acceptable result for one semantic case."""

    classification: str
    reason: str | None = None
    mismatches: tuple[MismatchPin, ...] = ()
    ferric_fingerprint: str | None = None
    rationale: str | None = None
    since: str | None = None
    tracking_issue: str | None = None


@dataclass(frozen=True)
class SemanticCase:
    """One required semantic scenario and its committed policy."""

    id: str
    issue: str
    fixture: str
    family: str
    expected: ExpectedResult


@dataclass(frozen=True)
class ReferencePlatform:
    """Immutable reference artifacts for one container platform."""

    binary_sha256: str
    library_sha256: str


@dataclass(frozen=True)
class ReferencePolicy:
    """Pinned common identity and per-platform artifact digests."""

    schema: str
    version: int
    engine: str
    engine_version: str
    package: str
    package_version: str
    base_image: str
    platforms: dict[str, ReferencePlatform]


@dataclass(frozen=True)
class SemanticPolicy:
    """Validated policy for the complete differential matrix."""

    schema_version: int
    suite_version: str
    source: str
    reference: ReferencePolicy
    cases: tuple[SemanticCase, ...]


@dataclass(frozen=True)
class GateReport:
    """All failures and accepted known deviations from one gate evaluation."""

    failures: tuple[str, ...]
    accepted_deviations: tuple[str, ...]


def _reject_duplicate_fields(pairs: list[tuple[str, object]]) -> dict[str, object]:
    result: dict[str, object] = {}
    for key, value in pairs:
        if key in result:
            raise SemanticGateError(f"duplicate JSON field: {key!r}")
        result[key] = value
    return result


def _load_json(path: Path, *, label: str) -> object:
    try:
        raw = path.read_text(encoding="utf-8")
        return json.loads(raw, object_pairs_hook=_reject_duplicate_fields)
    except (OSError, UnicodeError, json.JSONDecodeError, SemanticGateError) as error:
        raise SemanticGateError(f"cannot load {label} {path}: {error}") from error


def _exact_object(
    raw: object,
    *,
    label: str,
    fields: frozenset[str],
) -> dict[str, object]:
    if type(raw) is not dict:
        raise SemanticGateError(f"{label} must be an object")
    assert isinstance(raw, dict)
    if set(raw) != fields:
        missing = sorted(fields - set(raw))
        extra = sorted(set(raw) - fields)
        raise SemanticGateError(f"{label} has unexpected fields (missing={missing}, extra={extra})")
    return raw


def _string(raw: object, *, label: str) -> str:
    if type(raw) is not str or not raw:
        raise SemanticGateError(f"{label} must be a non-empty string")
    return raw


def _digest(raw: object, *, label: str) -> str:
    value = _string(raw, label=label)
    if _SHA256_RE.fullmatch(value) is None:
        raise SemanticGateError(f"{label} must be a lowercase SHA-256 digest")
    return value


def _issue(raw: object, *, label: str) -> str:
    value = _string(raw, label=label)
    if _ISSUE_RE.fullmatch(value) is None:
        raise SemanticGateError(f"{label} must be a ferric-rules GitHub issue URL")
    return value


def _reference_policy(raw: object) -> ReferencePolicy:
    reference = _exact_object(
        raw,
        label="reference",
        fields=frozenset(
            {
                "schema",
                "version",
                "engine",
                "engine_version",
                "package",
                "package_version",
                "base_image",
                "platforms",
            }
        ),
    )
    constants = {
        "schema": REFERENCE_SCHEMA,
        "version": REFERENCE_VERSION,
        "engine": "clips",
        "engine_version": "6.30",
        "package": "clips",
        "package_version": "6.30-4.1",
    }
    for field, expected in constants.items():
        if type(reference[field]) is not type(expected) or reference[field] != expected:
            raise SemanticGateError(f"reference.{field} must equal {expected!r}")
    base_image = _string(reference["base_image"], label="reference.base_image")
    if _BASE_IMAGE_RE.fullmatch(base_image) is None:
        raise SemanticGateError("reference.base_image must pin the Debian image by digest")

    raw_platforms = reference["platforms"]
    if type(raw_platforms) is not dict or not raw_platforms:
        raise SemanticGateError("reference.platforms must be a non-empty object")
    assert isinstance(raw_platforms, dict)
    if "linux/amd64" not in raw_platforms:
        raise SemanticGateError("reference.platforms must pin the CI platform linux/amd64")
    platforms: dict[str, ReferencePlatform] = {}
    for platform, raw_artifacts in raw_platforms.items():
        if type(platform) is not str or _PLATFORM_RE.fullmatch(platform) is None:
            raise SemanticGateError(f"reference.platforms has unsupported platform {platform!r}")
        artifacts = _exact_object(
            raw_artifacts,
            label=f"reference.platforms.{platform}",
            fields=frozenset({"binary_sha256", "library_sha256"}),
        )
        platforms[platform] = ReferencePlatform(
            binary_sha256=_digest(
                artifacts["binary_sha256"],
                label=f"reference.platforms.{platform}.binary_sha256",
            ),
            library_sha256=_digest(
                artifacts["library_sha256"],
                label=f"reference.platforms.{platform}.library_sha256",
            ),
        )
    return ReferencePolicy(
        schema=REFERENCE_SCHEMA,
        version=REFERENCE_VERSION,
        engine="clips",
        engine_version="6.30",
        package="clips",
        package_version="6.30-4.1",
        base_image=base_image,
        platforms=platforms,
    )


def _mismatch_pins(raw: object, *, label: str) -> tuple[MismatchPin, ...]:
    if type(raw) is not list or not raw:
        raise SemanticGateError(f"{label} must be a non-empty array")
    assert isinstance(raw, list)
    pins: list[MismatchPin] = []
    for index, item in enumerate(raw):
        mismatch = _exact_object(
            item,
            label=f"{label}[{index}]",
            fields=frozenset({"scope", "field"}),
        )
        scope = _string(mismatch["scope"], label=f"{label}[{index}].scope")
        if scope not in {"ferric", "engines"}:
            raise SemanticGateError(
                f"{label}[{index}].scope must identify Ferric or an engine comparison"
            )
        pins.append(
            MismatchPin(
                scope=scope,
                field=_string(mismatch["field"], label=f"{label}[{index}].field"),
            )
        )
    if len(pins) != len(set(pins)):
        raise SemanticGateError(f"{label} must not contain duplicate scope/field pairs")
    if pins != sorted(pins, key=lambda pin: (pin.scope, pin.field)):
        raise SemanticGateError(f"{label} must be sorted by scope then field")
    return tuple(pins)


def _expected_result(raw: object, *, label: str, case_issue: str) -> ExpectedResult:
    if type(raw) is not dict:
        raise SemanticGateError(f"{label} must be an object")
    assert isinstance(raw, dict)
    classification = raw.get("classification")
    if classification == "equivalent":
        _exact_object(raw, label=label, fields=frozenset({"classification"}))
        return ExpectedResult(classification="equivalent")
    if classification != "divergent":
        raise SemanticGateError(f"{label}.classification must be equivalent or divergent")
    expected = _exact_object(
        raw,
        label=label,
        fields=frozenset(
            {
                "classification",
                "reason",
                "mismatches",
                "ferric_fingerprint",
                "rationale",
                "since",
                "tracking_issue",
            }
        ),
    )
    tracking_issue = _issue(expected["tracking_issue"], label=f"{label}.tracking_issue")
    if tracking_issue != case_issue:
        raise SemanticGateError(f"{label}.tracking_issue must match the case issue")
    return ExpectedResult(
        classification="divergent",
        reason=_string(expected["reason"], label=f"{label}.reason"),
        mismatches=_mismatch_pins(expected["mismatches"], label=f"{label}.mismatches"),
        ferric_fingerprint=_digest(
            expected["ferric_fingerprint"],
            label=f"{label}.ferric_fingerprint",
        ),
        rationale=_string(expected["rationale"], label=f"{label}.rationale"),
        since=_string(expected["since"], label=f"{label}.since"),
        tracking_issue=tracking_issue,
    )


def load_policy(path: Path) -> SemanticPolicy:
    """Load and strictly validate the complete semantic-lane policy."""
    root = _exact_object(
        _load_json(path, label="semantic policy"),
        label="$",
        fields=frozenset({"schema_version", "suite_version", "source", "reference", "cases"}),
    )
    if type(root["schema_version"]) is not int or root["schema_version"] != POLICY_SCHEMA_VERSION:
        raise SemanticGateError(f"schema_version must equal {POLICY_SCHEMA_VERSION}")
    suite_version = _string(root["suite_version"], label="suite_version")
    if root["source"] != POLICY_SOURCE:
        raise SemanticGateError(f"source must equal {POLICY_SOURCE!r}")
    raw_cases = root["cases"]
    if type(raw_cases) is not list:
        raise SemanticGateError("cases must be an array")
    assert isinstance(raw_cases, list)
    cases: list[SemanticCase] = []
    for index, raw_case in enumerate(raw_cases):
        label = f"cases[{index}]"
        case = _exact_object(
            raw_case,
            label=label,
            fields=frozenset({"id", "issue", "fixture", "family", "expected"}),
        )
        case_id = _string(case["id"], label=f"{label}.id")
        issue = _issue(case["issue"], label=f"{label}.issue")
        fixture = _string(case["fixture"], label=f"{label}.fixture")
        expected_fixture = f"{POLICY_SOURCE}/{case_id.lower()}.clp"
        if fixture != expected_fixture:
            raise SemanticGateError(f"{label}.fixture must equal {expected_fixture!r}")
        cases.append(
            SemanticCase(
                id=case_id,
                issue=issue,
                fixture=fixture,
                family=_string(case["family"], label=f"{label}.family"),
                expected=_expected_result(
                    case["expected"],
                    label=f"{label}.expected",
                    case_issue=issue,
                ),
            )
        )

    case_ids = [case.id for case in cases]
    if len(case_ids) != len(set(case_ids)):
        raise SemanticGateError("case IDs must be unique")
    required_ids = set(REQUIRED_CASE_ISSUES)
    if set(case_ids) != required_ids:
        raise SemanticGateError(
            "cases must cover exactly the required scenario IDs "
            f"(missing={sorted(required_ids - set(case_ids))}, "
            f"extra={sorted(set(case_ids) - required_ids)})"
        )
    if case_ids != sorted(case_ids):
        raise SemanticGateError("cases must be sorted by ID")
    fixtures = [case.fixture for case in cases]
    if len(fixtures) != len(set(fixtures)):
        raise SemanticGateError("case fixtures must be unique")
    for case in cases:
        expected_issue = (
            f"https://github.com/plx/ferric-rules/issues/{REQUIRED_CASE_ISSUES[case.id]}"
        )
        if case.issue != expected_issue:
            raise SemanticGateError(f"{case.id} must link to {expected_issue}")

    return SemanticPolicy(
        schema_version=POLICY_SCHEMA_VERSION,
        suite_version=suite_version,
        source=POLICY_SOURCE,
        reference=_reference_policy(root["reference"]),
        cases=tuple(cases),
    )


def _validate_reference(policy: ReferencePolicy, raw: object) -> list[str]:
    failures: list[str] = []
    expected_fields = {
        "schema",
        "version",
        "engine",
        "engine_version",
        "package",
        "package_version",
        "platform",
        "binary_sha256",
        "library_sha256",
        "base_image",
        "image_id",
    }
    if type(raw) is not dict or set(raw) != expected_fields:
        return ["manifest reference provenance is missing or malformed"]
    assert isinstance(raw, dict)
    common = {
        "schema": policy.schema,
        "version": policy.version,
        "engine": policy.engine,
        "engine_version": policy.engine_version,
        "package": policy.package,
        "package_version": policy.package_version,
        "base_image": policy.base_image,
    }
    for field, expected in common.items():
        if type(raw.get(field)) is not type(expected) or raw.get(field) != expected:
            failures.append(
                f"reference {field} mismatch: expected {expected!r}, got {raw.get(field)!r}"
            )
    platform = raw.get("platform")
    if type(platform) is not str or _PLATFORM_RE.fullmatch(platform) is None:
        failures.append(f"reference platform is invalid: {platform!r}")
        return failures
    artifacts = policy.platforms.get(platform)
    if artifacts is None:
        failures.append(f"reference platform {platform!r} has no committed artifact pin")
        return failures
    for field, expected in (
        ("binary_sha256", artifacts.binary_sha256),
        ("library_sha256", artifacts.library_sha256),
    ):
        if raw.get(field) != expected:
            failures.append(
                f"reference {platform} {field} mismatch: "
                f"expected {expected}, got {raw.get(field)!r}"
            )
    image_id = raw.get("image_id")
    if type(image_id) is not str or _IMAGE_ID_RE.fullmatch(image_id) is None:
        failures.append(f"reference image_id is invalid: {image_id!r}")
    return failures


def _source_bytes(examples_dir: Path, path: str) -> bytes:
    pure = PurePosixPath(path)
    if (
        pure.is_absolute()
        or pure.as_posix() != path
        or any(part in {"", ".", ".."} for part in pure.parts)
        or not pure.parts
        or pure.parts[0] != POLICY_SOURCE
    ):
        raise SemanticGateError(f"scenario source path is not normalized: {path!r}")
    try:
        root = examples_dir.resolve(strict=True)
        candidate = examples_dir.joinpath(*pure.parts)
        if candidate.is_symlink():
            raise SemanticGateError(f"scenario source must not be a symlink: {path}")
        resolved = candidate.resolve(strict=True)
        resolved.relative_to(root)
        if not resolved.is_file():
            raise SemanticGateError(f"scenario source is not a regular file: {path}")
        return resolved.read_bytes()
    except SemanticGateError:
        raise
    except (OSError, ValueError) as error:
        raise SemanticGateError(f"cannot resolve scenario source {path!r}: {error}") from error


def _runtime_declaration(
    case: SemanticCase,
    entry: dict[str, object],
    *,
    examples_dir: Path,
) -> tuple[dict[str, object], OracleDeclaration]:
    raw_oracle = entry.get("oracle")
    if type(raw_oracle) is not dict:
        raise SemanticGateError(f"{case.id}: oracle declaration is missing")
    assert isinstance(raw_oracle, dict)
    if raw_oracle.get("version") != 2:
        raise SemanticGateError(f"{case.id}: semantic-lane oracle must use scenario version 2")
    if raw_oracle.get("id") != case.id:
        raise SemanticGateError(f"{case.id}: oracle ID does not match the policy case")
    raw_sources = raw_oracle.get("sources")
    if type(raw_sources) is not list or not raw_sources:
        raise SemanticGateError(f"{case.id}: scenario sources are missing")
    assert isinstance(raw_sources, list)
    seen_names: set[str] = set()
    source_sizes: list[int] = []
    for index, raw_source in enumerate(raw_sources):
        if type(raw_source) is not dict:
            raise SemanticGateError(f"{case.id}: sources[{index}] is malformed")
        assert isinstance(raw_source, dict)
        name = raw_source.get("name")
        path = raw_source.get("path")
        digest = raw_source.get("sha256")
        if type(name) is not str or not name or name in seen_names:
            raise SemanticGateError(f"{case.id}: sources[{index}].name is invalid")
        seen_names.add(name)
        if type(path) is not str or type(digest) is not str:
            raise SemanticGateError(f"{case.id}: sources[{index}] is malformed")
        content = _source_bytes(examples_dir, path)
        source_sizes.append(len(content))
        validate_scenario_source_sizes((len(content),))
        actual_digest = sha256_bytes(content)
        if digest != actual_digest:
            raise SemanticGateError(f"{case.id}: sources[{index}] digest is stale for {path!r}")
    validate_scenario_source_sizes(tuple(source_sizes))
    primary = raw_sources[0]
    assert isinstance(primary, dict)
    if primary.get("name") != "primary" or primary.get("path") != case.fixture:
        raise SemanticGateError(f"{case.id}: first source must be primary at {case.fixture!r}")
    source_digest = primary["sha256"]
    if entry.get("source_sha256") != source_digest:
        raise SemanticGateError(f"{case.id}: manifest source digest is stale")
    plan_digest = scenario_plan_sha256(raw_oracle)
    if raw_oracle.get("composed_sha256") != plan_digest:
        raise SemanticGateError(f"{case.id}: canonical scenario plan digest is stale")

    ferric = entry.get("ferric")
    if type(ferric) is not dict or type(ferric.get("canonical_observation")) is not dict:
        raise SemanticGateError(f"{case.id}: Ferric canonical observation is missing")
    observation = ferric["canonical_observation"]
    assert isinstance(observation, dict)
    nonce = observation.get("nonce")
    runtime_raw = copy.deepcopy(raw_oracle)
    runtime_raw["nonce"] = nonce
    evidence = validate_declaration(
        runtime_raw,
        expected_source_sha256=source_digest,
        expected_composed_sha256=plan_digest,
    )
    if evidence.status is not EvidenceStatus.VALID or evidence.value is None:
        detail = "; ".join(f"{issue.field}: {issue.message}" for issue in evidence.issues)
        raise SemanticGateError(f"{case.id}: invalid runtime oracle declaration: {detail}")
    return runtime_raw, evidence.value


def _validated_engine_observation(
    case: SemanticCase,
    *,
    engine: str,
    raw_result: object,
    declaration: OracleDeclaration,
) -> dict[str, object]:
    """Require complete successful adapter evidence for one semantic-lane result."""
    if type(raw_result) is not dict:
        raise SemanticGateError(f"{case.id}: {engine} result is missing")
    result = raw_result
    assert isinstance(result, dict)
    for field in (
        "harness_error",
        "observation_error",
        "projection_error",
        "spawn_error",
        "not_run",
    ):
        if field in result:
            raise SemanticGateError(f"{case.id}: {engine} result contains {field!r}")
    if type(result.get("exit_code")) is not int or result["exit_code"] != 0:
        raise SemanticGateError(f"{case.id}: {engine} result did not exit successfully")
    if result.get("timed_out") is not False:
        raise SemanticGateError(f"{case.id}: {engine} result has invalid timeout evidence")
    duration_ms = result.get("duration_ms")
    if type(duration_ms) is not int or duration_ms < 0:
        raise SemanticGateError(f"{case.id}: {engine} result duration is malformed")
    for field in ("stdout", "stderr"):
        if type(result.get(field)) is not str:
            raise SemanticGateError(f"{case.id}: {engine} readable {field} is malformed")

    if result.get("termination") != {"kind": "exit", "exit_code": 0, "signal": None}:
        raise SemanticGateError(f"{case.id}: {engine} termination evidence is not a clean exit")

    raw_output = result.get("raw_output")
    if type(raw_output) is not dict or set(raw_output) != {"encoding", "stdout", "stderr"}:
        raise SemanticGateError(f"{case.id}: {engine} raw output evidence is malformed")
    assert isinstance(raw_output, dict)
    if raw_output.get("encoding") != "base64":
        raise SemanticGateError(f"{case.id}: {engine} raw output encoding is unsupported")
    decoded: dict[str, bytes] = {}
    for field in ("stdout", "stderr"):
        encoded = raw_output.get(field)
        if type(encoded) is not str:
            raise SemanticGateError(f"{case.id}: {engine} raw {field} is malformed")
        try:
            decoded[field] = base64.b64decode(encoded, validate=True)
        except (binascii.Error, ValueError) as error:
            raise SemanticGateError(
                f"{case.id}: {engine} raw {field} is not canonical base64"
            ) from error
        if base64.b64encode(decoded[field]).decode("ascii") != encoded:
            raise SemanticGateError(f"{case.id}: {engine} raw {field} is not canonical base64")
        if decoded[field].decode("utf-8", errors="replace") != result[field]:
            raise SemanticGateError(
                f"{case.id}: {engine} readable {field} disagrees with raw bytes"
            )

    observation = result.get("observation")
    canonical = result.get("canonical_observation")
    if type(observation) is not dict or type(canonical) is not dict:
        raise SemanticGateError(f"{case.id}: {engine} observation evidence is incomplete")
    assert isinstance(observation, dict)
    assert isinstance(canonical, dict)
    if engine == "ferric":
        if decoded["stderr"]:
            raise SemanticGateError(f"{case.id}: Ferric emitted out-of-band stderr")
        if (
            decoded["stdout"].count(b"\n") != 1
            or not decoded["stdout"].startswith(b"{")
            or not decoded["stdout"].endswith(b"}\n")
        ):
            raise SemanticGateError(
                f"{case.id}: Ferric raw stdout is not one newline-terminated JSON object"
            )
        try:
            decoded_observation = json.loads(decoded["stdout"])
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise SemanticGateError(
                f"{case.id}: Ferric raw stdout is not its JSON observation"
            ) from error
        if decoded_observation != observation:
            raise SemanticGateError(
                f"{case.id}: Ferric raw stdout disagrees with its observation envelope"
            )
    elif not decoded["stderr"]:
        raise SemanticGateError(f"{case.id}: CLIPS authenticated raw evidence is empty")

    expected_fixture = {
        "id": declaration.id,
        "nonce": declaration.nonce,
        "source_sha256": declaration.source_sha256,
        "composed_sha256": declaration.composed_sha256,
    }
    try:
        canonical_diagnostic = project_observation_diagnostic(
            observation,
            engine=engine,
            expected_fixture=expected_fixture,
        )
        if engine == "ferric":
            reprojected = project_ferric_observation(
                observation,
                harnessed=False,
                require_firing_names=declaration.expectations.firings.names is not None,
                require_globals=declaration.expectations.globals is not None,
            )
        else:
            reprojected = project_clips_observation(
                observation,
                harnessed=False,
                require_firing_names=declaration.expectations.firings.names is not None,
            )
    except ObservationProjectionError as error:
        raise SemanticGateError(
            f"{case.id}: {engine} raw observation does not reproject: {error}"
        ) from error
    expected_diagnostic = diagnostic(
        canonical_diagnostic["phase"],
        canonical_diagnostic["category"],
        continued=canonical_diagnostic["continued"],
    )
    persisted_diagnostic = validated_result_diagnostic(result.get("diagnostic"))
    if (
        persisted_diagnostic != result.get("diagnostic")
        or persisted_diagnostic != expected_diagnostic
    ):
        raise SemanticGateError(
            f"{case.id}: {engine} result diagnostic disagrees with raw evidence"
        )
    if reprojected != canonical:
        raise SemanticGateError(
            f"{case.id}: {engine} canonical observation disagrees with raw evidence"
        )
    if engine == "clips" and result["stdout"] != canonical["channels"]["stdout"]:
        raise SemanticGateError(
            f"{case.id}: CLIPS semantic stdout disagrees with exact process bytes"
        )
    return canonical


def evaluate_manifest(
    policy: SemanticPolicy,
    manifest: object,
    *,
    examples_dir: Path,
    fingerprint: Callable[..., str] = observation_semantic_fingerprint,
) -> GateReport:
    """Evaluate one compatibility manifest against the exact committed policy."""
    failures: list[str] = []
    accepted: list[str] = []
    if type(manifest) is not dict:
        return GateReport(("manifest root must be an object",), ())
    assert isinstance(manifest, dict)
    if type(manifest.get("version")) is not int or manifest.get("version") != 3:
        failures.append("manifest must use compatibility schema version 3")
    if (
        type(manifest.get("oracle_protocol_version")) is not int
        or manifest.get("oracle_protocol_version") != 1
    ):
        failures.append("manifest must use oracle protocol version 1")
    failures.extend(_validate_reference(policy.reference, manifest.get("reference")))
    files = manifest.get("files")
    if type(files) is not dict:
        failures.append("manifest files must be an object")
        return GateReport(tuple(failures), ())
    assert isinstance(files, dict)
    policy_fixtures = {case.fixture for case in policy.cases}
    try:
        registry_root = _exact_object(
            _load_json(examples_dir / "compat-oracles.json", label="oracle registry"),
            label="oracle registry",
            fields=frozenset({"version", "fixtures"}),
        )
        if type(registry_root["version"]) is not int or registry_root["version"] != 1:
            raise SemanticGateError("oracle registry version must equal 1")
        registry = registry_root["fixtures"]
        if type(registry) is not dict:
            raise SemanticGateError("oracle registry fixtures must be an object")
        assert isinstance(registry, dict)
        registry_fixtures = {
            path for path in registry if type(path) is str and path.startswith(f"{policy.source}/")
        }
        if registry_fixtures != policy_fixtures:
            raise SemanticGateError(
                "semantic oracle registry membership mismatch "
                f"(missing={sorted(policy_fixtures - registry_fixtures)}, "
                f"extra={sorted(registry_fixtures - policy_fixtures)})"
            )
    except SemanticGateError as error:
        failures.append(str(error))
        return GateReport(tuple(failures), ())
    source_fixtures = {
        path
        for path, entry in files.items()
        if type(entry) is dict and entry.get("source") == policy.source
    }
    if source_fixtures != policy_fixtures:
        failures.append(
            "semantic manifest membership mismatch "
            f"(missing={sorted(policy_fixtures - source_fixtures)}, "
            f"extra={sorted(source_fixtures - policy_fixtures)})"
        )

    for case in policy.cases:
        raw_entry = files.get(case.fixture)
        if type(raw_entry) is not dict:
            failures.append(f"{case.id}: manifest entry is missing")
            continue
        assert isinstance(raw_entry, dict)
        if raw_entry.get("source") != policy.source:
            failures.append(f"{case.id}: manifest source is not {policy.source!r}")
            continue
        if raw_entry.get("oracle") != registry.get(case.fixture):
            failures.append(
                f"{case.id}: manifest oracle does not match the committed registry declaration"
            )
            continue
        try:
            runtime_raw, declaration = _runtime_declaration(
                case,
                raw_entry,
                examples_dir=examples_dir,
            )
        except (SemanticGateError, ValueError) as error:
            failures.append(str(error))
            continue

        ferric = raw_entry.get("ferric")
        clips = raw_entry.get("clips")
        try:
            ferric_observation = _validated_engine_observation(
                case,
                engine="ferric",
                raw_result=ferric,
                declaration=declaration,
            )
            clips_observation = _validated_engine_observation(
                case,
                engine="clips",
                raw_result=clips,
                declaration=declaration,
            )
        except SemanticGateError as error:
            failures.append(str(error))
            continue
        assert isinstance(ferric, dict)
        assert isinstance(clips, dict)
        evaluation = evaluate_oracle(
            runtime_raw,
            ferric_observation,
            clips_observation,
            expected_source_sha256=declaration.source_sha256,
            expected_composed_sha256=declaration.composed_sha256,
        )
        _oracle_classification, _oracle_reason, expected_evidence = oracle_outcome(evaluation)
        evidence_views = {
            "entry": raw_entry.get("oracle_evidence"),
            "ferric": ferric.get("oracle_evidence"),
            "clips": clips.get("oracle_evidence"),
        }
        stale_evidence = [
            label for label, persisted in evidence_views.items() if persisted != expected_evidence
        ]
        if stale_evidence:
            failures.append(
                f"{case.id}: persisted oracle evidence is missing or stale in "
                f"{', '.join(stale_evidence)}"
            )
            continue
        classification, reason = classify_results(ferric, clips, evaluation=evaluation)
        if raw_entry.get("classification") != classification or raw_entry.get("reason") != reason:
            failures.append(
                f"{case.id}: persisted result disagrees with recomputation "
                f"({raw_entry.get('classification')!r}/{raw_entry.get('reason')!r} != "
                f"{classification!r}/{reason!r})"
            )
            continue
        if evaluation.status is not EvidenceStatus.VALID:
            failures.append(f"{case.id}: oracle evidence is not valid")
            continue
        mismatch_pins = tuple(
            sorted(
                (MismatchPin(mismatch.scope, mismatch.field) for mismatch in evaluation.mismatches),
                key=lambda pin: (pin.scope, pin.field),
            )
        )
        if any(pin.scope == "clips" for pin in mismatch_pins):
            failures.append(f"{case.id}: pinned CLIPS does not satisfy its declaration")
            continue

        expected = case.expected
        if expected.classification == "equivalent":
            if classification != "equivalent" or not evaluation.equivalent:
                failures.append(f"{case.id}: expected equivalence, got {classification}/{reason}")
            elif mismatch_pins:
                failures.append(f"{case.id}: equivalent result contains mismatches")
            continue

        if classification == "equivalent" and evaluation.equivalent:
            failures.append(
                f"{case.id}: now matches pinned CLIPS; remove the stale known divergence"
            )
            continue
        if classification != "divergent" or reason != expected.reason:
            failures.append(
                f"{case.id}: expected divergent/{expected.reason}, got {classification}/{reason}"
            )
            continue
        if mismatch_pins != expected.mismatches:
            failures.append(
                f"{case.id}: mismatch scope changed: "
                f"expected {expected.mismatches!r}, got {mismatch_pins!r}"
            )
            continue
        try:
            actual_fingerprint = fingerprint(
                ferric_observation,
                declaration=declaration,
            )
        except ValueError as error:
            failures.append(f"{case.id}: cannot fingerprint Ferric observation: {error}")
            continue
        if actual_fingerprint != expected.ferric_fingerprint:
            failures.append(
                f"{case.id}: Ferric semantic fingerprint changed: "
                f"expected {expected.ferric_fingerprint}, got {actual_fingerprint}"
            )
            continue
        accepted.append(
            f"{case.id} ({expected.since}, {expected.tracking_issue}): {expected.rationale}"
        )

    return GateReport(tuple(failures), tuple(accepted))


@app.command()
def main(
    policy_path: Annotated[
        Path | None,
        typer.Option("--policy", help="Path to the committed semantic policy."),
    ] = None,
    manifest_path: Annotated[
        Path | None,
        typer.Option("--manifest", help="Path to the compatibility manifest."),
    ] = None,
) -> None:
    """Reject missing, unexplained, changed, or stale semantic results."""
    examples_dir = default_examples_dir()
    policy_file = policy_path or examples_dir / "compat-semantic-policy.json"
    manifest_file = manifest_path or examples_dir / "compat-manifest.json"
    try:
        policy = load_policy(policy_file)
        manifest = _load_json(manifest_file, label="compatibility manifest")
        report = evaluate_manifest(policy, manifest, examples_dir=examples_dir)
    except SemanticGateError as error:
        console.print(f"[red]error:[/] {error}")
        raise typer.Exit(1) from error

    if report.failures:
        for failure in report.failures:
            console.print(f"[red]FAIL[/] {failure}")
        raise typer.Exit(1)

    console.print(
        f"[green]semantic differential gate passed[/]: {len(policy.cases)} cases, "
        f"{len(report.accepted_deviations)} exact known divergences"
    )
    for deviation in report.accepted_deviations:
        console.print(f"[yellow]KNOWN DIVERGENCE[/] {deviation}")


if __name__ == "__main__":
    app()
