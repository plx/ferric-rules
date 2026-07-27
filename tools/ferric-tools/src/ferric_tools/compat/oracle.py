"""Strict, engine-neutral compatibility oracle contracts.

This module deliberately does not know how either engine is invoked.  It
validates versioned declaration and observation dictionaries, then compares
the independently captured observations against the declaration and each
other.  Callers retain responsibility for producing trustworthy observations.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from enum import StrEnum

DECLARATION_VERSION = 1
OBSERVATION_VERSION = 1

NORMALIZE_FACT_IDS = "fact-ids"
NORMALIZE_FACT_ORDER = "fact-order"
NORMALIZE_FLOAT_FORMAT = "float-format"
SUPPORTED_NORMALIZERS = frozenset(
    {
        NORMALIZE_FACT_IDS,
        NORMALIZE_FACT_ORDER,
        NORMALIZE_FLOAT_FORMAT,
    }
)

_SHA256_RE = re.compile(r"[0-9a-f]{64}")
_NONCE_RE = re.compile(r"[0-9a-f]{32,128}")
_PROTOCOL_TOKEN_RE = re.compile(r"[A-Za-z0-9][A-Za-z0-9._:/-]{0,127}")
_FLOAT_RE = re.compile(r"-?(?:(?:\d+(?:\.\d*)?)|(?:\.\d+))(?:[eE][+-]?\d+)?")
_VALUE_TYPES = frozenset({"symbol", "string", "integer", "float", "multifield"})
_FACT_ORIGINS = frozenset({"fixture", "instrumentation", "engine"})
_EVIDENCE_ORIGINS = frozenset({"fixture", "instrumentation"})
_HALT_REASONS = frozenset(
    {
        "agenda-empty",
        "limit-reached",
        "halt-requested",
        "action-error",
        "not-run",
    }
)
_V1_SETUP = ("load", "reset", "run")


class EvidenceStatus(StrEnum):
    """Whether an oracle contract was present and structurally trustworthy."""

    VALID = "valid"
    MISSING = "missing"
    INVALID = "invalid"


@dataclass(frozen=True)
class OracleIssue:
    """One field-addressed contract validation failure."""

    field: str
    message: str


@dataclass(frozen=True)
class Evidence[T]:
    """A validated declaration or observation."""

    status: EvidenceStatus
    value: T | None = None
    issues: tuple[OracleIssue, ...] = ()


@dataclass(frozen=True)
class TypedValue:
    """A canonical, recursively typed CLIPS value."""

    type: str
    value: str | int | tuple[TypedValue, ...]


@dataclass(frozen=True)
class Slot:
    """One named template slot."""

    name: str
    value: TypedValue


@dataclass(frozen=True)
class OrderedFact:
    """An ordered fact, including identity and provenance."""

    kind: str
    id: int
    origin: str
    module: str
    relation: str
    fields: tuple[TypedValue, ...]


@dataclass(frozen=True)
class TemplateFact:
    """A template fact, including slot names, identity, and provenance."""

    kind: str
    id: int
    origin: str
    module: str
    template: str
    slots: tuple[Slot, ...]


type Fact = OrderedFact | TemplateFact


@dataclass(frozen=True)
class ExpectedEffect:
    """A feature-specific effect declared by the fixture."""

    name: str
    value: TypedValue


@dataclass(frozen=True)
class ObservedEffect:
    """A captured effect with explicit fixture/instrumentation provenance."""

    name: str
    value: TypedValue
    origin: str


@dataclass(frozen=True)
class ObservedFiring:
    """A captured firing with explicit fixture/instrumentation provenance."""

    rule: str
    origin: str


@dataclass(frozen=True)
class FiringExpectation:
    """Expected fixture firings; either the count or ordered names may be omitted."""

    count: int | None
    names: tuple[str, ...] | None


@dataclass(frozen=True)
class DiagnosticState:
    """Structured diagnostic phase, category, and continuation behavior."""

    phase: str
    category: str
    continued: bool


@dataclass(frozen=True)
class RunState:
    """Run limit and terminal state."""

    limit: int | None
    halt_reason: str


@dataclass(frozen=True)
class GlobalValue:
    """One named global value."""

    name: str
    value: TypedValue


@dataclass(frozen=True)
class Expectations:
    """All independently reviewable semantic expectations for one fixture."""

    phase: str
    firings: FiringExpectation
    effects: tuple[ExpectedEffect, ...]
    facts: tuple[Fact, ...]
    channels: tuple[tuple[str, str], ...]
    diagnostic: DiagnosticState
    run: RunState
    focus_stack: tuple[str, ...] | None
    globals: tuple[GlobalValue, ...] | None


@dataclass(frozen=True)
class OracleDeclaration:
    """A validated version-1 fixture declaration."""

    version: int
    id: str
    feature: str
    source_sha256: str
    composed_sha256: str
    nonce: str
    setup: tuple[str, ...]
    expectations: Expectations
    normalizers: tuple[str, ...]


@dataclass(frozen=True)
class Marker:
    """One identity-bound protocol marker."""

    kind: str
    id: str
    source_sha256: str
    composed_sha256: str
    nonce: str


@dataclass(frozen=True)
class OracleObservation:
    """A validated version-1 engine observation."""

    version: int
    id: str
    source_sha256: str
    composed_sha256: str
    nonce: str
    markers: tuple[Marker, ...]
    phase: str
    firings: tuple[ObservedFiring, ...]
    effects: tuple[ObservedEffect, ...]
    facts: tuple[Fact, ...]
    channels: tuple[tuple[str, str], ...]
    diagnostic: DiagnosticState
    run: RunState
    focus_stack: tuple[str, ...] | None
    globals: tuple[GlobalValue, ...] | None


@dataclass(frozen=True)
class OracleMismatch:
    """One field-specific semantic or evidence mismatch."""

    scope: str
    field: str
    message: str


@dataclass(frozen=True)
class OracleEvaluation:
    """The complete validation and comparison result for two engines."""

    status: EvidenceStatus
    equivalent: bool
    declaration: Evidence[OracleDeclaration]
    ferric: Evidence[OracleObservation]
    clips: Evidence[OracleObservation]
    mismatches: tuple[OracleMismatch, ...]


class _ContractViolation(ValueError):
    def __init__(self, field: str, message: str) -> None:
        super().__init__(f"{field}: {message}")
        self.field = field
        self.message = message


def _fail(field: str, message: str) -> None:
    raise _ContractViolation(field, message)


def _strict_dict(
    raw: object,
    *,
    field: str,
    required: frozenset[str],
    optional: frozenset[str] = frozenset(),
) -> dict[str, object]:
    if type(raw) is not dict:
        _fail(field, "must be an object")
    assert isinstance(raw, dict)
    if any(type(key) is not str for key in raw):
        _fail(field, "must contain only string field names")
    actual = set(raw)
    missing = sorted(required - actual)
    unknown = sorted(actual - required - optional)
    if missing:
        _fail(field, f"missing fields: {', '.join(missing)}")
    if unknown:
        _fail(field, f"unknown fields: {', '.join(unknown)}")
    return raw


def _strict_list(raw: object, *, field: str) -> list[object]:
    if type(raw) is not list:
        _fail(field, "must be an array")
    assert isinstance(raw, list)
    return raw


def _string(raw: object, *, field: str) -> str:
    if type(raw) is not str:
        _fail(field, "must be a string")
    assert isinstance(raw, str)
    if not raw:
        _fail(field, "must not be empty")
    if any(ord(character) < 32 or ord(character) == 127 for character in raw):
        _fail(field, "must not contain control characters")
    return raw


def _digest(raw: object, *, field: str) -> str:
    value = _string(raw, field=field)
    if _SHA256_RE.fullmatch(value) is None:
        _fail(field, "must be a lowercase SHA-256 digest")
    return value


def _nonce(raw: object, *, field: str) -> str:
    value = _string(raw, field=field)
    if len(value) % 2 != 0 or _NONCE_RE.fullmatch(value) is None:
        _fail(
            field,
            "must encode 16 to 64 bytes as even-length lowercase hexadecimal",
        )
    return value


def _protocol_token(raw: object, *, field: str) -> str:
    value = _string(raw, field=field)
    if _PROTOCOL_TOKEN_RE.fullmatch(value) is None:
        _fail(field, "must be a protocol-safe token of at most 128 ASCII bytes")
    return value


def _version(raw: object, *, field: str, expected: int) -> int:
    if type(raw) is not int or raw != expected:
        _fail(field, f"unsupported version: {raw!r}")
    assert isinstance(raw, int)
    return raw


def _optional_nonnegative_int(raw: object, *, field: str) -> int | None:
    if raw is None:
        return None
    if type(raw) is not int or raw < 0:
        _fail(field, "must be a non-negative integer or null")
    assert isinstance(raw, int)
    return raw


def _typed_value(raw: object, *, field: str) -> TypedValue:
    value_object = _strict_dict(
        raw,
        field=field,
        required=frozenset({"type", "value"}),
    )
    value_type = _string(value_object["type"], field=f"{field}.type")
    if value_type not in _VALUE_TYPES:
        _fail(f"{field}.type", f"unsupported typed value: {value_type!r}")
    value = value_object["value"]

    if value_type in {"symbol", "string"}:
        if type(value) is not str:
            _fail(f"{field}.value", f"must be a string for {value_type}")
        assert isinstance(value, str)
        return TypedValue(value_type, value)
    if value_type == "integer":
        if type(value) is not int:
            _fail(f"{field}.value", "must be an integer for integer")
        assert isinstance(value, int)
        return TypedValue(value_type, value)
    if value_type == "float":
        if type(value) is not str or _FLOAT_RE.fullmatch(value) is None:
            _fail(
                f"{field}.value",
                "must be a finite decimal string for float",
            )
        assert isinstance(value, str)
        return TypedValue(value_type, value)

    items = _strict_list(value, field=f"{field}.value")
    return TypedValue(
        value_type,
        tuple(
            _typed_value(item, field=f"{field}.value[{index}]") for index, item in enumerate(items)
        ),
    )


def _slot(raw: object, *, field: str) -> Slot:
    slot_object = _strict_dict(
        raw,
        field=field,
        required=frozenset({"name", "value"}),
    )
    return Slot(
        name=_string(slot_object["name"], field=f"{field}.name"),
        value=_typed_value(slot_object["value"], field=f"{field}.value"),
    )


def _fact(raw: object, *, field: str) -> Fact:
    if type(raw) is not dict:
        _fail(field, "must be an object")
    assert isinstance(raw, dict)
    kind = raw.get("kind")
    if kind == "ordered":
        fact_object = _strict_dict(
            raw,
            field=field,
            required=frozenset(
                {
                    "kind",
                    "id",
                    "origin",
                    "module",
                    "relation",
                    "fields",
                }
            ),
        )
        fact_id = _optional_nonnegative_int(fact_object["id"], field=f"{field}.id")
        if fact_id is None:
            _fail(f"{field}.id", "must not be null")
        origin = _string(fact_object["origin"], field=f"{field}.origin")
        if origin not in _FACT_ORIGINS:
            _fail(f"{field}.origin", f"unsupported fact origin: {origin!r}")
        fields = _strict_list(fact_object["fields"], field=f"{field}.fields")
        return OrderedFact(
            kind="ordered",
            id=fact_id,
            origin=origin,
            module=_string(fact_object["module"], field=f"{field}.module"),
            relation=_string(fact_object["relation"], field=f"{field}.relation"),
            fields=tuple(
                _typed_value(value, field=f"{field}.fields[{index}]")
                for index, value in enumerate(fields)
            ),
        )
    if kind == "template":
        fact_object = _strict_dict(
            raw,
            field=field,
            required=frozenset(
                {
                    "kind",
                    "id",
                    "origin",
                    "module",
                    "template",
                    "slots",
                }
            ),
        )
        fact_id = _optional_nonnegative_int(fact_object["id"], field=f"{field}.id")
        if fact_id is None:
            _fail(f"{field}.id", "must not be null")
        origin = _string(fact_object["origin"], field=f"{field}.origin")
        if origin not in _FACT_ORIGINS:
            _fail(f"{field}.origin", f"unsupported fact origin: {origin!r}")
        raw_slots = _strict_list(fact_object["slots"], field=f"{field}.slots")
        slots = tuple(
            _slot(slot, field=f"{field}.slots[{index}]") for index, slot in enumerate(raw_slots)
        )
        slot_names = [slot.name for slot in slots]
        if len(slot_names) != len(set(slot_names)):
            _fail(f"{field}.slots", "must not contain duplicate slot names")
        return TemplateFact(
            kind="template",
            id=fact_id,
            origin=origin,
            module=_string(fact_object["module"], field=f"{field}.module"),
            template=_string(fact_object["template"], field=f"{field}.template"),
            slots=slots,
        )
    _fail(f"{field}.kind", "must be 'ordered' or 'template'")


def _facts(raw: object, *, field: str) -> tuple[Fact, ...]:
    items = _strict_list(raw, field=field)
    return tuple(_fact(item, field=f"{field}[{index}]") for index, item in enumerate(items))


def _channels(raw: object, *, field: str) -> tuple[tuple[str, str], ...]:
    if type(raw) is not dict:
        _fail(field, "must be an object")
    assert isinstance(raw, dict)
    if any(type(key) is not str for key in raw):
        _fail(field, "must contain only string channel names")
    channels_object = raw
    if "stdout" not in channels_object or "stderr" not in channels_object:
        _fail(field, "must define stdout and stderr channels")
    channels: list[tuple[str, str]] = []
    for name in sorted(channels_object):
        channel_name = _string(name, field=f"{field} field")
        output = channels_object[name]
        if type(output) is not str:
            _fail(f"{field}.{channel_name}", "must be a string")
        assert isinstance(output, str)
        channels.append((channel_name, output))
    return tuple(channels)


def _diagnostic(raw: object, *, field: str) -> DiagnosticState:
    diagnostic_object = _strict_dict(
        raw,
        field=field,
        required=frozenset({"phase", "category", "continued"}),
    )
    phase = _string(diagnostic_object["phase"], field=f"{field}.phase")
    category = _string(diagnostic_object["category"], field=f"{field}.category")
    if phase == "unknown" or category == "unknown":
        _fail(field, "unknown diagnostic evidence is not valid")
    continued = diagnostic_object["continued"]
    if type(continued) is not bool:
        _fail(f"{field}.continued", "must be a boolean")
    assert isinstance(continued, bool)
    return DiagnosticState(phase=phase, category=category, continued=continued)


def _run_state(raw: object, *, field: str) -> RunState:
    run_object = _strict_dict(
        raw,
        field=field,
        required=frozenset({"limit", "halt_reason"}),
    )
    halt_reason = _string(run_object["halt_reason"], field=f"{field}.halt_reason")
    if halt_reason not in _HALT_REASONS:
        _fail(f"{field}.halt_reason", f"unsupported halt reason: {halt_reason!r}")
    return RunState(
        limit=_optional_nonnegative_int(run_object["limit"], field=f"{field}.limit"),
        halt_reason=halt_reason,
    )


def _focus_stack(raw: object, *, field: str) -> tuple[str, ...] | None:
    if raw is None:
        return None
    items = _strict_list(raw, field=field)
    return tuple(_string(item, field=f"{field}[{index}]") for index, item in enumerate(items))


def _globals(raw: object, *, field: str) -> tuple[GlobalValue, ...] | None:
    if raw is None:
        return None
    items = _strict_list(raw, field=field)
    globals_values: list[GlobalValue] = []
    for index, item in enumerate(items):
        item_field = f"{field}[{index}]"
        global_object = _strict_dict(
            item,
            field=item_field,
            required=frozenset({"name", "value"}),
        )
        globals_values.append(
            GlobalValue(
                name=_protocol_token(global_object["name"], field=f"{item_field}.name"),
                value=_typed_value(global_object["value"], field=f"{item_field}.value"),
            )
        )
    names = [global_value.name for global_value in globals_values]
    if len(names) != len(set(names)):
        _fail(field, "must not contain duplicate global names")
    return tuple(globals_values)


def _firing_expectation(raw: object, *, field: str) -> FiringExpectation:
    firing_object = _strict_dict(
        raw,
        field=field,
        required=frozenset({"count", "names"}),
    )
    count = _optional_nonnegative_int(firing_object["count"], field=f"{field}.count")
    raw_names = firing_object["names"]
    names: tuple[str, ...] | None
    if raw_names is None:
        names = None
    else:
        name_items = _strict_list(raw_names, field=f"{field}.names")
        names = tuple(
            _string(name, field=f"{field}.names[{index}]") for index, name in enumerate(name_items)
        )
    if count is None and names is None:
        _fail(field, "must declare a firing count, ordered names, or both")
    if count is not None and names is not None and count != len(names):
        _fail(
            field,
            "firing count must equal the number of ordered firing names",
        )
    return FiringExpectation(count=count, names=names)


def _expected_effect(raw: object, *, field: str) -> ExpectedEffect:
    effect_object = _strict_dict(
        raw,
        field=field,
        required=frozenset({"name", "value"}),
    )
    return ExpectedEffect(
        name=_string(effect_object["name"], field=f"{field}.name"),
        value=_typed_value(effect_object["value"], field=f"{field}.value"),
    )


def _expectations(raw: object, *, field: str) -> Expectations:
    expectations_object = _strict_dict(
        raw,
        field=field,
        required=frozenset(
            {
                "phase",
                "firings",
                "effects",
                "facts",
                "channels",
                "diagnostic",
                "run",
                "focus_stack",
                "globals",
            }
        ),
    )
    raw_effects = _strict_list(expectations_object["effects"], field=f"{field}.effects")
    effects = tuple(
        _expected_effect(effect, field=f"{field}.effects[{index}]")
        for index, effect in enumerate(raw_effects)
    )
    if not effects:
        _fail(
            f"{field}.effects",
            "must declare at least one fixture-specific semantic effect",
        )
    channels = _channels(expectations_object["channels"], field=f"{field}.channels")
    if {name for name, _output in channels} != {"stdout", "stderr"}:
        _fail(
            f"{field}.channels",
            "oracle v1 supports exactly stdout and stderr",
        )
    run = _run_state(expectations_object["run"], field=f"{field}.run")
    if run.limit is not None:
        _fail(
            f"{field}.run.limit",
            "oracle v1 supports only an unlimited run",
        )
    if run.halt_reason not in {"agenda-empty", "halt-requested", "action-error"}:
        _fail(
            f"{field}.run.halt_reason",
            "oracle v1 supports only agenda-empty, halt-requested, or action-error completion",
        )
    return Expectations(
        phase=_string(expectations_object["phase"], field=f"{field}.phase"),
        firings=_firing_expectation(
            expectations_object["firings"],
            field=f"{field}.firings",
        ),
        effects=effects,
        facts=_facts(expectations_object["facts"], field=f"{field}.facts"),
        channels=channels,
        diagnostic=_diagnostic(
            expectations_object["diagnostic"],
            field=f"{field}.diagnostic",
        ),
        run=run,
        focus_stack=_focus_stack(
            expectations_object["focus_stack"],
            field=f"{field}.focus_stack",
        ),
        globals=_globals(expectations_object["globals"], field=f"{field}.globals"),
    )


def _normalizers(raw: object, *, field: str) -> tuple[str, ...]:
    items = _strict_list(raw, field=field)
    normalizers = tuple(
        _string(item, field=f"{field}[{index}]") for index, item in enumerate(items)
    )
    if len(normalizers) != len(set(normalizers)):
        _fail(field, "must not contain duplicate normalizers")
    unsupported = sorted(set(normalizers) - SUPPORTED_NORMALIZERS)
    if unsupported:
        _fail(field, f"unsupported normalizers: {', '.join(unsupported)}")
    return normalizers


def _parse_declaration(
    raw: object,
    *,
    expected_source_sha256: str,
    expected_composed_sha256: str,
) -> OracleDeclaration:
    expected_digest = _digest(
        expected_source_sha256,
        field="expected_source_sha256",
    )
    expected_composed_digest = _digest(
        expected_composed_sha256,
        field="expected_composed_sha256",
    )
    declaration_object = _strict_dict(
        raw,
        field="$",
        required=frozenset(
            {
                "version",
                "id",
                "feature",
                "source_sha256",
                "composed_sha256",
                "nonce",
                "setup",
                "expectations",
                "normalizers",
            }
        ),
    )
    source_digest = _digest(
        declaration_object["source_sha256"],
        field="source_sha256",
    )
    if source_digest != expected_digest:
        _fail("source_sha256", "source digest is stale")
    composed_digest = _digest(
        declaration_object["composed_sha256"],
        field="composed_sha256",
    )
    if composed_digest != expected_composed_digest:
        _fail("composed_sha256", "composed input digest is stale")
    raw_setup = _strict_list(declaration_object["setup"], field="setup")
    setup = tuple(
        _string(operation, field=f"setup[{index}]") for index, operation in enumerate(raw_setup)
    )
    if setup != _V1_SETUP:
        _fail("setup", "version 1 requires exactly: load, reset, run")
    return OracleDeclaration(
        version=_version(
            declaration_object["version"],
            field="version",
            expected=DECLARATION_VERSION,
        ),
        id=_protocol_token(declaration_object["id"], field="id"),
        feature=_string(declaration_object["feature"], field="feature"),
        source_sha256=source_digest,
        composed_sha256=composed_digest,
        nonce=_nonce(declaration_object["nonce"], field="nonce"),
        setup=setup,
        expectations=_expectations(
            declaration_object["expectations"],
            field="expectations",
        ),
        normalizers=_normalizers(
            declaration_object["normalizers"],
            field="normalizers",
        ),
    )


def validate_declaration(
    raw: object | None,
    *,
    expected_source_sha256: str,
    expected_composed_sha256: str,
) -> Evidence[OracleDeclaration]:
    """Validate a v1 declaration without raising for missing or invalid evidence."""
    if raw is None:
        return Evidence(status=EvidenceStatus.MISSING)
    try:
        declaration = _parse_declaration(
            raw,
            expected_source_sha256=expected_source_sha256,
            expected_composed_sha256=expected_composed_sha256,
        )
    except _ContractViolation as error:
        return Evidence(
            status=EvidenceStatus.INVALID,
            issues=(OracleIssue(error.field, error.message),),
        )
    return Evidence(status=EvidenceStatus.VALID, value=declaration)


def _marker(raw: object, *, field: str) -> Marker:
    marker_object = _strict_dict(
        raw,
        field=field,
        required=frozenset(
            {
                "kind",
                "id",
                "source_sha256",
                "composed_sha256",
                "nonce",
            }
        ),
    )
    kind = _string(marker_object["kind"], field=f"{field}.kind")
    if kind not in {"START", "COMPLETE"}:
        _fail(f"{field}.kind", "must be START or COMPLETE")
    return Marker(
        kind=kind,
        id=_string(marker_object["id"], field=f"{field}.id"),
        source_sha256=_digest(
            marker_object["source_sha256"],
            field=f"{field}.source_sha256",
        ),
        composed_sha256=_digest(
            marker_object["composed_sha256"],
            field=f"{field}.composed_sha256",
        ),
        nonce=_nonce(marker_object["nonce"], field=f"{field}.nonce"),
    )


def _markers(
    raw: object,
    *,
    field: str,
    declaration: OracleDeclaration,
) -> tuple[Marker, ...]:
    items = _strict_list(raw, field=field)
    markers = tuple(_marker(item, field=f"{field}[{index}]") for index, item in enumerate(items))
    starts = [marker for marker in markers if marker.kind == "START"]
    completions = [marker for marker in markers if marker.kind == "COMPLETE"]
    if len(starts) != 1 or len(completions) != 1 or len(markers) != 2:
        _fail(field, "must contain exactly one START and exactly one COMPLETE")
    if tuple(marker.kind for marker in markers) != ("START", "COMPLETE"):
        _fail(field, "START must precede COMPLETE")
    for index, marker in enumerate(markers):
        marker_field = f"{field}[{index}]"
        if marker.id != declaration.id:
            _fail(f"{marker_field}.id", "does not match declaration id")
        if marker.source_sha256 != declaration.source_sha256:
            _fail(
                f"{marker_field}.source_sha256",
                "does not match declaration source digest",
            )
        if marker.composed_sha256 != declaration.composed_sha256:
            _fail(
                f"{marker_field}.composed_sha256",
                "does not match declaration composed input digest",
            )
        if marker.nonce != declaration.nonce:
            _fail(f"{marker_field}.nonce", "does not match declaration nonce")
    return markers


def _observed_firing(raw: object, *, field: str) -> ObservedFiring:
    firing_object = _strict_dict(
        raw,
        field=field,
        required=frozenset({"rule", "origin"}),
    )
    origin = _string(firing_object["origin"], field=f"{field}.origin")
    if origin not in _EVIDENCE_ORIGINS:
        _fail(f"{field}.origin", f"unsupported firing origin: {origin!r}")
    return ObservedFiring(
        rule=_string(firing_object["rule"], field=f"{field}.rule"),
        origin=origin,
    )


def _observed_effect(raw: object, *, field: str) -> ObservedEffect:
    effect_object = _strict_dict(
        raw,
        field=field,
        required=frozenset({"name", "value", "origin"}),
    )
    origin = _string(effect_object["origin"], field=f"{field}.origin")
    if origin not in _EVIDENCE_ORIGINS:
        _fail(f"{field}.origin", f"unsupported effect origin: {origin!r}")
    return ObservedEffect(
        name=_string(effect_object["name"], field=f"{field}.name"),
        value=_typed_value(effect_object["value"], field=f"{field}.value"),
        origin=origin,
    )


def _parse_observation(
    raw: object,
    *,
    declaration: OracleDeclaration,
) -> OracleObservation:
    observation_object = _strict_dict(
        raw,
        field="$",
        required=frozenset(
            {
                "version",
                "id",
                "source_sha256",
                "composed_sha256",
                "nonce",
                "markers",
                "phase",
                "firings",
                "effects",
                "facts",
                "channels",
                "diagnostic",
                "run",
                "focus_stack",
                "globals",
            }
        ),
    )
    fixture_id = _string(observation_object["id"], field="id")
    if fixture_id != declaration.id:
        _fail("id", "does not match declaration id")
    source_digest = _digest(
        observation_object["source_sha256"],
        field="source_sha256",
    )
    if source_digest != declaration.source_sha256:
        _fail("source_sha256", "does not match declaration source digest")
    composed_digest = _digest(
        observation_object["composed_sha256"],
        field="composed_sha256",
    )
    if composed_digest != declaration.composed_sha256:
        _fail("composed_sha256", "does not match declaration composed input digest")
    nonce = _nonce(observation_object["nonce"], field="nonce")
    if nonce != declaration.nonce:
        _fail("nonce", "does not match declaration nonce")

    raw_firings = _strict_list(observation_object["firings"], field="firings")
    raw_effects = _strict_list(observation_object["effects"], field="effects")
    return OracleObservation(
        version=_version(
            observation_object["version"],
            field="version",
            expected=OBSERVATION_VERSION,
        ),
        id=fixture_id,
        source_sha256=source_digest,
        composed_sha256=composed_digest,
        nonce=nonce,
        markers=_markers(
            observation_object["markers"],
            field="markers",
            declaration=declaration,
        ),
        phase=_string(observation_object["phase"], field="phase"),
        firings=tuple(
            _observed_firing(firing, field=f"firings[{index}]")
            for index, firing in enumerate(raw_firings)
        ),
        effects=tuple(
            _observed_effect(effect, field=f"effects[{index}]")
            for index, effect in enumerate(raw_effects)
        ),
        facts=_facts(observation_object["facts"], field="facts"),
        channels=_channels(observation_object["channels"], field="channels"),
        diagnostic=_diagnostic(observation_object["diagnostic"], field="diagnostic"),
        run=_run_state(observation_object["run"], field="run"),
        focus_stack=_focus_stack(
            observation_object["focus_stack"],
            field="focus_stack",
        ),
        globals=_globals(observation_object["globals"], field="globals"),
    )


def validate_observation(
    raw: object | None,
    *,
    declaration: OracleDeclaration,
) -> Evidence[OracleObservation]:
    """Validate a v1 observation against one already validated declaration."""
    if raw is None:
        return Evidence(
            status=EvidenceStatus.INVALID,
            issues=(OracleIssue("$", "engine observation is missing"),),
        )
    try:
        observation = _parse_observation(raw, declaration=declaration)
    except _ContractViolation as error:
        return Evidence(
            status=EvidenceStatus.INVALID,
            issues=(OracleIssue(error.field, error.message),),
        )
    return Evidence(status=EvidenceStatus.VALID, value=observation)


def _compare_unsigned_decimal(left: str, right: str) -> int:
    """Compare canonical unsigned decimal integers without converting to int."""
    if len(left) != len(right):
        return -1 if len(left) < len(right) else 1
    return (left > right) - (left < right)


def _add_unsigned_decimal(left: str, right: str) -> str:
    """Add canonical unsigned decimal integers without interpreter digit limits."""
    result: list[str] = []
    carry = 0
    width = max(len(left), len(right))
    for offset in range(1, width + 1):
        left_digit = ord(left[-offset]) - ord("0") if offset <= len(left) else 0
        right_digit = ord(right[-offset]) - ord("0") if offset <= len(right) else 0
        total = left_digit + right_digit + carry
        result.append(chr(ord("0") + (total % 10)))
        carry = total // 10
    if carry:
        result.append(chr(ord("0") + carry))
    return "".join(reversed(result))


def _subtract_unsigned_decimal(left: str, right: str) -> str:
    """Subtract canonical unsigned integers where ``left`` is at least ``right``."""
    result: list[str] = []
    borrow = 0
    for offset in range(1, len(left) + 1):
        left_digit = ord(left[-offset]) - ord("0") - borrow
        right_digit = ord(right[-offset]) - ord("0") if offset <= len(right) else 0
        if left_digit < right_digit:
            left_digit += 10
            borrow = 1
        else:
            borrow = 0
        result.append(chr(ord("0") + left_digit - right_digit))
    assert borrow == 0
    return "".join(reversed(result)).lstrip("0") or "0"


def _add_decimal_offset(value: str, offset: int) -> str:
    """Add a bounded integer to an arbitrary-length signed decimal integer."""
    value_sign = -1 if value.startswith("-") else 1
    value_magnitude = value.lstrip("+-").lstrip("0") or "0"
    if value_magnitude == "0":
        value_sign = 1

    offset_sign = -1 if offset < 0 else 1
    offset_magnitude = str(abs(offset))
    if offset_magnitude == "0":
        offset_sign = 1

    if value_sign == offset_sign:
        result_sign = value_sign
        result_magnitude = _add_unsigned_decimal(value_magnitude, offset_magnitude)
    else:
        comparison = _compare_unsigned_decimal(value_magnitude, offset_magnitude)
        if comparison == 0:
            return "0"
        if comparison > 0:
            result_sign = value_sign
            result_magnitude = _subtract_unsigned_decimal(value_magnitude, offset_magnitude)
        else:
            result_sign = offset_sign
            result_magnitude = _subtract_unsigned_decimal(offset_magnitude, value_magnitude)

    prefix = "-" if result_sign < 0 else ""
    return f"{prefix}{result_magnitude}"


def _canonical_float(value: str) -> tuple[str, str, str] | tuple[str]:
    """Return an exact canonical decimal representation independent of context.

    The coefficient and exponent remain strings. This avoids both Decimal's
    context-sensitive 28-digit normalization and its implementation exponent
    limits while retaining exact equality for every accepted finite literal.
    """
    negative = value.startswith("-")
    unsigned = value[1:] if negative else value
    if "e" in unsigned:
        mantissa, exponent = unsigned.split("e", 1)
    elif "E" in unsigned:
        mantissa, exponent = unsigned.split("E", 1)
    else:
        mantissa, exponent = unsigned, "0"

    if "." in mantissa:
        integer, fraction = mantissa.split(".", 1)
    else:
        integer, fraction = mantissa, ""

    coefficient_with_zeros = f"{integer}{fraction}".lstrip("0")
    if not coefficient_with_zeros:
        return ("0",)

    coefficient = coefficient_with_zeros.rstrip("0")
    trailing_zeros = len(coefficient_with_zeros) - len(coefficient)
    canonical_exponent = _add_decimal_offset(
        exponent,
        trailing_zeros - len(fraction),
    )
    return ("-" if negative else "+", coefficient, canonical_exponent)


def _normalized_value(value: TypedValue, normalizers: frozenset[str]) -> object:
    if value.type == "multifield":
        assert isinstance(value.value, tuple)
        return (
            value.type,
            tuple(_normalized_value(item, normalizers) for item in value.value),
        )
    if value.type == "float" and NORMALIZE_FLOAT_FORMAT in normalizers:
        assert isinstance(value.value, str)
        return (value.type, _canonical_float(value.value))
    return (value.type, value.value)


def _normalized_fact(fact: Fact, normalizers: frozenset[str]) -> object:
    fact_id: int | None = fact.id
    if NORMALIZE_FACT_IDS in normalizers:
        fact_id = None
    if isinstance(fact, OrderedFact):
        return (
            fact.kind,
            fact_id,
            fact.origin,
            fact.module,
            fact.relation,
            tuple(_normalized_value(value, normalizers) for value in fact.fields),
        )
    return (
        fact.kind,
        fact_id,
        fact.origin,
        fact.module,
        fact.template,
        tuple((slot.name, _normalized_value(slot.value, normalizers)) for slot in fact.slots),
    )


def _normalized_facts(
    facts: tuple[Fact, ...],
    normalizers: frozenset[str],
) -> tuple[object, ...]:
    normalized = tuple(_normalized_fact(fact, normalizers) for fact in facts)
    if NORMALIZE_FACT_ORDER in normalizers:
        return tuple(sorted(normalized, key=repr))
    return normalized


def _normalized_expected_effects(
    effects: tuple[ExpectedEffect, ...],
    normalizers: frozenset[str],
) -> tuple[object, ...]:
    normalized = tuple(
        (effect.name, _normalized_value(effect.value, normalizers)) for effect in effects
    )
    if NORMALIZE_FACT_ORDER in normalizers:
        return tuple(sorted(normalized, key=repr))
    return normalized


def _normalized_observed_effects(
    effects: tuple[ObservedEffect, ...],
    normalizers: frozenset[str],
) -> tuple[object, ...]:
    normalized = tuple(
        (effect.name, _normalized_value(effect.value, normalizers))
        for effect in effects
        if effect.origin == "fixture"
    )
    if NORMALIZE_FACT_ORDER in normalizers:
        return tuple(sorted(normalized, key=repr))
    return normalized


def _normalized_globals(
    globals_values: tuple[GlobalValue, ...] | None,
    normalizers: frozenset[str],
) -> tuple[object, ...] | None:
    if globals_values is None:
        return None
    return tuple(
        (global_value.name, _normalized_value(global_value.value, normalizers))
        for global_value in globals_values
    )


def _append_mismatch(
    mismatches: list[OracleMismatch],
    *,
    scope: str,
    field: str,
    message: str = "does not match the declared expectation",
) -> None:
    mismatches.append(OracleMismatch(scope=scope, field=field, message=message))


def _compare_engine(
    scope: str,
    declaration: OracleDeclaration,
    observation: OracleObservation,
) -> list[OracleMismatch]:
    expected = declaration.expectations
    normalizers = frozenset(declaration.normalizers)
    mismatches: list[OracleMismatch] = []

    if observation.phase != expected.phase:
        _append_mismatch(mismatches, scope=scope, field="phase")

    fixture_firings = tuple(
        firing.rule for firing in observation.firings if firing.origin == "fixture"
    )
    if expected.firings.count is not None and len(fixture_firings) != expected.firings.count:
        _append_mismatch(mismatches, scope=scope, field="firings.count")
    if expected.firings.names is not None and fixture_firings != expected.firings.names:
        _append_mismatch(mismatches, scope=scope, field="firings.names")

    if _normalized_observed_effects(
        observation.effects,
        normalizers,
    ) != _normalized_expected_effects(expected.effects, normalizers):
        _append_mismatch(mismatches, scope=scope, field="effects")

    if _normalized_facts(observation.facts, normalizers) != _normalized_facts(
        expected.facts,
        normalizers,
    ):
        _append_mismatch(mismatches, scope=scope, field="facts")

    expected_channels = dict(expected.channels)
    observed_channels = dict(observation.channels)
    if expected_channels.keys() != observed_channels.keys():
        _append_mismatch(mismatches, scope=scope, field="channels")
    for channel in sorted(expected_channels.keys() & observed_channels.keys()):
        if expected_channels[channel] != observed_channels[channel]:
            _append_mismatch(
                mismatches,
                scope=scope,
                field=f"channels.{channel}",
            )

    if observation.diagnostic.phase != expected.diagnostic.phase:
        _append_mismatch(mismatches, scope=scope, field="diagnostic.phase")
    if observation.diagnostic.category != expected.diagnostic.category:
        _append_mismatch(mismatches, scope=scope, field="diagnostic.category")
    if observation.diagnostic.continued != expected.diagnostic.continued:
        _append_mismatch(mismatches, scope=scope, field="diagnostic.continued")
    if observation.run.limit != expected.run.limit:
        _append_mismatch(mismatches, scope=scope, field="run.limit")
    if observation.run.halt_reason != expected.run.halt_reason:
        _append_mismatch(mismatches, scope=scope, field="run.halt_reason")
    if expected.focus_stack is not None and observation.focus_stack != expected.focus_stack:
        _append_mismatch(mismatches, scope=scope, field="focus_stack")
    if expected.globals is not None and _normalized_globals(
        observation.globals,
        normalizers,
    ) != _normalized_globals(expected.globals, normalizers):
        _append_mismatch(mismatches, scope=scope, field="globals")

    return mismatches


def _compare_engines(
    declaration: OracleDeclaration,
    ferric: OracleObservation,
    clips: OracleObservation,
) -> list[OracleMismatch]:
    expected = declaration.expectations
    normalizers = frozenset(declaration.normalizers)
    mismatches: list[OracleMismatch] = []
    scope = "engines"

    if ferric.phase != clips.phase:
        _append_mismatch(mismatches, scope=scope, field="phase", message="differs by engine")

    ferric_firings = tuple(firing.rule for firing in ferric.firings if firing.origin == "fixture")
    clips_firings = tuple(firing.rule for firing in clips.firings if firing.origin == "fixture")
    if expected.firings.count is not None and len(ferric_firings) != len(clips_firings):
        _append_mismatch(
            mismatches,
            scope=scope,
            field="firings.count",
            message="differs by engine",
        )
    if expected.firings.names is not None and ferric_firings != clips_firings:
        _append_mismatch(
            mismatches,
            scope=scope,
            field="firings.names",
            message="differs by engine",
        )
    if _normalized_observed_effects(
        ferric.effects,
        normalizers,
    ) != _normalized_observed_effects(clips.effects, normalizers):
        _append_mismatch(
            mismatches,
            scope=scope,
            field="effects",
            message="differs by engine",
        )
    if _normalized_facts(ferric.facts, normalizers) != _normalized_facts(
        clips.facts,
        normalizers,
    ):
        _append_mismatch(
            mismatches,
            scope=scope,
            field="facts",
            message="differs by engine",
        )

    ferric_channels = dict(ferric.channels)
    clips_channels = dict(clips.channels)
    if ferric_channels.keys() != clips_channels.keys():
        _append_mismatch(
            mismatches,
            scope=scope,
            field="channels",
            message="differs by engine",
        )
    for channel in sorted(ferric_channels.keys() & clips_channels.keys()):
        if ferric_channels[channel] != clips_channels[channel]:
            _append_mismatch(
                mismatches,
                scope=scope,
                field=f"channels.{channel}",
                message="differs by engine",
            )

    for field in ("phase", "category", "continued"):
        if getattr(ferric.diagnostic, field) != getattr(clips.diagnostic, field):
            _append_mismatch(
                mismatches,
                scope=scope,
                field=f"diagnostic.{field}",
                message="differs by engine",
            )
    for field in ("limit", "halt_reason"):
        if getattr(ferric.run, field) != getattr(clips.run, field):
            _append_mismatch(
                mismatches,
                scope=scope,
                field=f"run.{field}",
                message="differs by engine",
            )
    if expected.focus_stack is not None and ferric.focus_stack != clips.focus_stack:
        _append_mismatch(
            mismatches,
            scope=scope,
            field="focus_stack",
            message="differs by engine",
        )
    if expected.globals is not None and _normalized_globals(
        ferric.globals,
        normalizers,
    ) != _normalized_globals(clips.globals, normalizers):
        _append_mismatch(
            mismatches,
            scope=scope,
            field="globals",
            message="differs by engine",
        )
    return mismatches


def _unchecked_observation(raw: object | None) -> Evidence[OracleObservation]:
    if raw is None:
        return Evidence(status=EvidenceStatus.MISSING)
    return Evidence(
        status=EvidenceStatus.INVALID,
        issues=(
            OracleIssue(
                "$",
                "cannot validate an observation without a valid declaration",
            ),
        ),
    )


def _validation_mismatches(
    scope: str,
    evidence: Evidence[object],
) -> list[OracleMismatch]:
    return [
        OracleMismatch(scope=scope, field=issue.field, message=issue.message)
        for issue in evidence.issues
    ]


def evaluate_oracle(
    raw_declaration: object | None,
    ferric_raw: object | None,
    clips_raw: object | None,
    *,
    expected_source_sha256: str,
    expected_composed_sha256: str,
) -> OracleEvaluation:
    """Validate and compare two independent engine observations.

    ``equivalent`` can only be true when all three evidence objects are valid,
    each engine satisfies the declaration, and the normalized observations
    agree with each other.  Missing or malformed evidence never raises and
    never produces equivalence.
    """
    declaration_evidence = validate_declaration(
        raw_declaration,
        expected_source_sha256=expected_source_sha256,
        expected_composed_sha256=expected_composed_sha256,
    )
    if declaration_evidence.status is not EvidenceStatus.VALID:
        ferric_evidence = _unchecked_observation(ferric_raw)
        clips_evidence = _unchecked_observation(clips_raw)
        mismatches = _validation_mismatches(
            "declaration",
            declaration_evidence,
        )
        mismatches.extend(_validation_mismatches("ferric", ferric_evidence))
        mismatches.extend(_validation_mismatches("clips", clips_evidence))
        return OracleEvaluation(
            status=declaration_evidence.status,
            equivalent=False,
            declaration=declaration_evidence,
            ferric=ferric_evidence,
            clips=clips_evidence,
            mismatches=tuple(mismatches),
        )

    declaration = declaration_evidence.value
    assert declaration is not None
    ferric_evidence = validate_observation(ferric_raw, declaration=declaration)
    clips_evidence = validate_observation(clips_raw, declaration=declaration)
    statuses = {ferric_evidence.status, clips_evidence.status}
    if EvidenceStatus.INVALID in statuses:
        status = EvidenceStatus.INVALID
    elif EvidenceStatus.MISSING in statuses:
        status = EvidenceStatus.MISSING
    else:
        status = EvidenceStatus.VALID

    mismatches = _validation_mismatches("ferric", ferric_evidence)
    mismatches.extend(_validation_mismatches("clips", clips_evidence))
    if status is EvidenceStatus.VALID:
        ferric_observation = ferric_evidence.value
        clips_observation = clips_evidence.value
        assert ferric_observation is not None
        assert clips_observation is not None
        mismatches.extend(_compare_engine("ferric", declaration, ferric_observation))
        mismatches.extend(_compare_engine("clips", declaration, clips_observation))
        mismatches.extend(
            _compare_engines(
                declaration,
                ferric_observation,
                clips_observation,
            )
        )

    return OracleEvaluation(
        status=status,
        equivalent=status is EvidenceStatus.VALID and not mismatches,
        declaration=declaration_evidence,
        ferric=ferric_evidence,
        clips=clips_evidence,
        mismatches=tuple(mismatches),
    )


def evidence_to_dict(evidence: Evidence[object]) -> dict[str, object]:
    """Return deterministic JSON-ready evidence status and validation issues."""
    return {
        "status": evidence.status.value,
        "issues": [{"field": issue.field, "message": issue.message} for issue in evidence.issues],
    }


def evaluation_to_dict(evaluation: OracleEvaluation) -> dict[str, object]:
    """Return a compact, deterministic, JSON-ready evaluation summary."""
    return {
        "status": evaluation.status.value,
        "equivalent": evaluation.equivalent,
        "declaration": evidence_to_dict(evaluation.declaration),
        "ferric": evidence_to_dict(evaluation.ferric),
        "clips": evidence_to_dict(evaluation.clips),
        "mismatches": [
            {
                "scope": mismatch.scope,
                "field": mismatch.field,
                "message": mismatch.message,
            }
            for mismatch in evaluation.mismatches
        ],
    }
