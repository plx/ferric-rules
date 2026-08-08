"""Project engine-specific observation envelopes into oracle v1.

Raw adapters intentionally expose their actual capabilities.  This module is
the single place that translates those envelopes into the strict,
engine-neutral schema consumed by :mod:`ferric_tools.compat.oracle`.
"""

from __future__ import annotations

import re

from ferric_tools.compat.diagnostics import (
    DIAGNOSTIC_TAXONOMY_VERSION,
    ENGINE_DIAGNOSTIC_CATEGORIES,
    ENGINE_DIAGNOSTIC_PHASES,
    UNKNOWN_DIAGNOSTIC,
)

_OBSERVATION_SCHEMA = "ferric.compat-observation"
_OBSERVATION_VERSION = 1
_HARNESS_LINE_RE = re.compile(r"^FERRIC-HARNESS\|\d+\|.*(?:\r?\n)?$")
_FACT_ID_RE = re.compile(r"(?:0|[1-9][0-9]*)")
_INTEGER_RE = re.compile(r"-?(?:0|[1-9][0-9]*)")
_HALT_REASONS = frozenset(
    {
        "agenda-empty",
        "limit-reached",
        "halt-requested",
        "action-error",
        "not-run",
    }
)
_DIAGNOSTIC_CATEGORY_BY_PHASE = {
    "parse": "syntax-error",
    "load": "construct-error",
    "reset": "evaluation-error",
    "run": "evaluation-error",
}
_INTERRUPTED_TAIL_PROTOCOL_ISSUES = frozenset(
    {
        "lifecycle-cardinality-or-order",
        "native-phase-records-missing",
        "native-run-metadata-missing",
        "phase-cardinality-or-order",
        "module-cardinality",
    }
)


class ObservationProjectionError(ValueError):
    """Raised when a raw adapter cannot support a trustworthy projection."""


def _canonical_value(raw: object, *, engine: str) -> dict:
    if type(raw) is not dict:
        raise ObservationProjectionError(f"{engine} value is not an object")
    assert isinstance(raw, dict)
    value_type = raw.get("type")
    if type(value_type) is not str:
        raise ObservationProjectionError(f"{engine} value has no string type")
    normalized_type = value_type.lower().replace("-", "_")

    if normalized_type in {"symbol", "string"}:
        value = raw.get("value")
        if type(value) is not str:
            raise ObservationProjectionError(f"{engine} {normalized_type} value is not a string")
        return {"type": normalized_type, "value": value}
    if normalized_type == "integer":
        value = raw.get("value")
        if type(value) is not str or _INTEGER_RE.fullmatch(value) is None:
            raise ObservationProjectionError(
                f"{engine} integer value is not canonical decimal text"
            )
        return {"type": "integer", "value": int(value)}
    if normalized_type == "float":
        value = raw.get("value")
        if type(value) is not str:
            raise ObservationProjectionError(f"{engine} float value is not text")
        return {"type": "float", "value": value}
    if normalized_type == "multifield":
        values = raw.get("values") if engine == "ferric" else raw.get("items")
        if type(values) is not list:
            raise ObservationProjectionError(f"{engine} multifield is not an array")
        return {
            "type": "multifield",
            "value": [_canonical_value(value, engine=engine) for value in values],
        }

    raise ObservationProjectionError(
        f"{engine} value type {value_type!r} is not supported by oracle v1"
    )


def _fact_id(raw: object, *, engine: str) -> int:
    if type(raw) is not str or _FACT_ID_RE.fullmatch(raw) is None:
        raise ObservationProjectionError(
            f"{engine} fact id is not canonical non-negative decimal text"
        )
    return int(raw)


def _canonical_fact(raw: object, *, engine: str, harnessed: bool) -> dict:
    if type(raw) is not dict:
        raise ObservationProjectionError(f"{engine} fact is not an object")
    assert isinstance(raw, dict)
    try:
        fact_id = _fact_id(raw["fact_id"], engine=engine)
    except KeyError as error:
        raise ObservationProjectionError(f"{engine} fact id is missing") from error
    module = raw.get("module")
    if type(module) is not str or not module:
        raise ObservationProjectionError(
            f"{engine} fact module is unavailable; the observation cannot prove final state"
        )

    kind = raw.get("kind")
    fact_name = raw.get("relation", raw.get("name"))
    origin = (
        "instrumentation"
        if harnessed and type(fact_name) is str and fact_name.startswith("ferric-harness-")
        else "fixture"
    )
    if kind == "ordered":
        fields = raw.get("fields")
        relation = raw.get("relation")
        if type(fields) is not list or type(relation) is not str or not relation:
            raise ObservationProjectionError(f"{engine} ordered fact is malformed")
        return {
            "kind": "ordered",
            "id": fact_id,
            "origin": origin,
            "module": module,
            "relation": relation,
            "fields": [_canonical_value(value, engine=engine) for value in fields],
        }
    if kind == "template":
        slots = raw.get("slots")
        template = raw.get("name") if engine == "ferric" else raw.get("relation")
        if type(slots) is not list or type(template) is not str or not template:
            raise ObservationProjectionError(f"{engine} template fact is malformed")
        canonical_slots: list[dict] = []
        for slot in slots:
            if type(slot) is not dict or type(slot.get("name")) is not str or not slot["name"]:
                raise ObservationProjectionError(f"{engine} template slot is malformed")
            canonical_slots.append(
                {
                    "name": slot["name"],
                    "value": _canonical_value(slot.get("value"), engine=engine),
                }
            )
        slot_names = [slot["name"] for slot in canonical_slots]
        if len(slot_names) != len(set(slot_names)):
            raise ObservationProjectionError(f"{engine} template has duplicate slot names")
        return {
            "kind": "template",
            "id": fact_id,
            "origin": origin,
            "module": module,
            "template": template,
            "slots": canonical_slots,
        }
    raise ObservationProjectionError(f"{engine} fact kind is unsupported")


def _fact_effect(fact: dict) -> dict:
    if fact["kind"] == "ordered":
        name = f"fact:{fact['module']}::{fact['relation']}"
        value = {"type": "multifield", "value": fact["fields"]}
    else:
        name = f"fact:{fact['module']}::{fact['template']}"
        slot_values = [
            {
                "type": "multifield",
                "value": [
                    {"type": "symbol", "value": slot["name"]},
                    slot["value"],
                ],
            }
            for slot in fact["slots"]
        ]
        value = {"type": "multifield", "value": slot_values}
    return {"name": name, "value": value, "origin": fact["origin"]}


def _strip_harness_output(text: str) -> tuple[str, list[str]]:
    semantic_lines: list[str] = []
    instrumentation: list[str] = []
    for line in text.splitlines(keepends=True):
        if _HARNESS_LINE_RE.fullmatch(line):
            instrumentation.append(line.rstrip("\r\n"))
        else:
            semantic_lines.append(line)
    return "".join(semantic_lines), instrumentation


def _canonical_diagnostic(
    raw: dict,
    *,
    engine: str,
    interrupted: bool = False,
    preserve_unknown: bool = False,
    diagnostic_subset: bool = False,
) -> dict:
    diagnostics = raw.get("diagnostics")
    if type(diagnostics) is not list or not all(
        type(diagnostic) is dict for diagnostic in diagnostics
    ):
        raise ObservationProjectionError(f"{engine} diagnostics are malformed")
    if engine == "ferric":
        protocol_issues = raw.get("protocol_issues", [])
    elif diagnostic_subset:
        protocol_issues = raw.get(
            "diagnostic_protocol_issues",
            raw.get("protocol_issues"),
        )
    else:
        protocol_issues = raw.get("protocol_issues")
    if type(protocol_issues) is not list or not all(
        type(issue) is str for issue in protocol_issues
    ):
        raise ObservationProjectionError(f"{engine} protocol issue evidence is malformed")
    allowed_protocol_issues = set(_INTERRUPTED_TAIL_PROTOCOL_ISSUES)
    if interrupted and "native-run-metadata-missing" in protocol_issues:
        # The parser also reports this state check when interruption cut off
        # the RUN record. It is contradictory, rather than incomplete, when a
        # RUN record was present and explicitly denied the diagnostic state.
        allowed_protocol_issues.add("native-run-diagnostic-state")
    unexpected_protocol_issues = (
        [issue for issue in protocol_issues if issue not in allowed_protocol_issues]
        if interrupted
        else protocol_issues
    )
    if unexpected_protocol_issues:
        raise ObservationProjectionError(
            f"{engine} protocol violations: {', '.join(map(str, unexpected_protocol_issues))}"
        )
    if not diagnostics:
        return {"phase": "none", "category": "none", "continued": True}

    summaries: list[tuple[str, str, bool]] = []
    for index, raw_diagnostic in enumerate(diagnostics):
        assert isinstance(raw_diagnostic, dict)
        taxonomy_version = raw_diagnostic.get("taxonomy_version")
        phase = raw_diagnostic.get("phase")
        category = raw_diagnostic.get("category")
        continued = raw_diagnostic.get("continued")
        if type(taxonomy_version) is not int or taxonomy_version != DIAGNOSTIC_TAXONOMY_VERSION:
            raise ObservationProjectionError(
                f"{engine} diagnostic {index} taxonomy version is unsupported"
            )
        unknown_pair = (phase, category) == (UNKNOWN_DIAGNOSTIC, UNKNOWN_DIAGNOSTIC)
        if type(phase) is not str or (
            phase not in ENGINE_DIAGNOSTIC_PHASES and not (preserve_unknown and unknown_pair)
        ):
            raise ObservationProjectionError(
                f"{engine} diagnostic {index} phase is unsupported: {phase!r}"
            )
        if type(category) is not str or (
            category not in ENGINE_DIAGNOSTIC_CATEGORIES and not (preserve_unknown and unknown_pair)
        ):
            raise ObservationProjectionError(
                f"{engine} diagnostic {index} category is unsupported: {category!r}"
            )
        expected_category = _DIAGNOSTIC_CATEGORY_BY_PHASE.get(phase)
        if not unknown_pair and category != expected_category:
            raise ObservationProjectionError(
                f"{engine} diagnostic {index} phase/category pair is unsupported: "
                f"{phase!r}/{category!r}"
            )
        if type(continued) is not bool:
            raise ObservationProjectionError(
                f"{engine} diagnostic {index} continuation is malformed"
            )
        if type(raw_diagnostic.get("message")) is not str:
            raise ObservationProjectionError(f"{engine} diagnostic {index} message is malformed")
        if engine == "ferric":
            if raw_diagnostic.get("severity") not in {"error", "warning"}:
                raise ObservationProjectionError(
                    f"{engine} diagnostic {index} severity is unsupported"
                )
        elif raw_diagnostic.get("channel") != "stderr":
            raise ObservationProjectionError(f"{engine} diagnostic {index} channel is unsupported")
        summaries.append((phase, category, continued))

    first = summaries[0]
    if any(summary != first for summary in summaries[1:]):
        raise ObservationProjectionError(
            f"{engine} diagnostics are heterogeneous and cannot be collapsed"
        )
    phase, category, continued = first
    return {"phase": phase, "category": category, "continued": continued}


def _canonical_markers(
    raw: dict,
    *,
    engine: str,
    interrupted: bool = False,
) -> list[dict]:
    fixture = raw.get("fixture")
    lifecycle = raw.get("lifecycle")
    if type(fixture) is not dict or type(lifecycle) is not list:
        raise ObservationProjectionError("observation identity or lifecycle is malformed")
    complete_sequences = (0, 1) if engine == "ferric" else (0, 3)
    sequences = [record.get("sequence") if type(record) is dict else None for record in lifecycle]
    expected_sequences = (0,) if interrupted and tuple(sequences) == (0,) else complete_sequences
    if (
        not all(type(sequence) is int for sequence in sequences)
        or tuple(sequences) != expected_sequences
    ):
        raise ObservationProjectionError(f"{engine} lifecycle sequence is malformed")
    markers: list[dict] = []
    for record in lifecycle:
        if type(record) is not dict:
            raise ObservationProjectionError("lifecycle record is malformed")
        event = record.get("event")
        if type(event) is not str:
            raise ObservationProjectionError("lifecycle event is malformed")
        kind = event.upper()
        if kind not in {"START", "COMPLETE"}:
            raise ObservationProjectionError("lifecycle event is malformed")
        markers.append(
            {
                "kind": kind,
                "id": record.get("fixture_id"),
                "source_sha256": record.get("source_sha256"),
                "composed_sha256": record.get("composed_sha256"),
                "nonce": record.get("nonce"),
            }
        )
    expected_kinds = ["START"] if expected_sequences == (0,) else ["START", "COMPLETE"]
    if [marker["kind"] for marker in markers] != expected_kinds:
        raise ObservationProjectionError(f"{engine} lifecycle event order is malformed")
    return markers


def _phase(raw: object) -> str:
    if type(raw) is not str:
        raise ObservationProjectionError(f"unsupported observed phase: {raw!r}")
    normalized = raw.replace("_", "-")
    if normalized == "post-run":
        return "run-complete"
    if normalized in ENGINE_DIAGNOSTIC_PHASES:
        return normalized
    raise ObservationProjectionError(f"unsupported observed phase: {raw!r}")


def _halt_reason(raw: object) -> str:
    if type(raw) is not str:
        raise ObservationProjectionError("run halt reason is missing")
    normalized = raw.replace("_", "-")
    if normalized == "error":
        # The pinned CLIPS adapter reports its native evaluation-error stop as
        # `error`; Ferric exposes the same semantic boundary as `action-error`.
        normalized = "action-error"
    if normalized not in _HALT_REASONS:
        raise ObservationProjectionError(f"run halt reason is unsupported: {raw!r}")
    return normalized


def _validate_raw_envelope(raw: dict, *, engine: str) -> None:
    if raw.get("schema") != _OBSERVATION_SCHEMA:
        raise ObservationProjectionError(f"{engine} observation schema is unsupported")
    if type(raw.get("version")) is not int or raw["version"] != _OBSERVATION_VERSION:
        raise ObservationProjectionError(f"{engine} observation version is unsupported")
    engine_identity = raw.get("engine")
    if type(engine_identity) is not dict or engine_identity.get("name") != engine:
        raise ObservationProjectionError(f"{engine} observation engine identity is malformed")


def _validate_diagnostic_context(
    raw: dict,
    *,
    engine: str,
    canonical: dict,
    interrupted: bool,
) -> None:
    """Validate phase/run fields that directly attest a diagnostic summary."""
    diagnostic_phase = canonical["phase"]
    if diagnostic_phase in {"none", UNKNOWN_DIAGNOSTIC}:
        return

    observed_phase = _phase(raw.get("phase_reached"))
    run = raw.get("run")
    if canonical["continued"]:
        if interrupted:
            phase_order = {"parse": 0, "load": 1, "reset": 2, "run": 3, "run-complete": 4}
            if phase_order[observed_phase] < phase_order[diagnostic_phase]:
                raise ObservationProjectionError(
                    f"{engine} continued diagnostic precedes its observed phase"
                )
            return
        if observed_phase != "run-complete" or type(run) is not dict:
            raise ObservationProjectionError(f"{engine} continued diagnostic lacks a completed run")
        return

    if observed_phase != diagnostic_phase:
        raise ObservationProjectionError(
            f"{engine} terminal phase and diagnostic phase are inconsistent"
        )
    if diagnostic_phase == "run":
        if run is None and interrupted:
            return
        if type(run) is not dict or _halt_reason(run.get("halt_reason")) != "action-error":
            raise ObservationProjectionError(
                f"{engine} terminal run diagnostic lacks an action-error halt"
            )
    elif run is not None:
        raise ObservationProjectionError(
            f"{engine} terminal pre-run diagnostic contains run evidence"
        )


def project_observation_diagnostic(
    raw: object,
    *,
    engine: str,
    expected_fixture: dict[str, str],
    interrupted: bool = False,
) -> dict:
    """Validate the trusted diagnostic subset before full semantic projection."""
    if type(raw) is not dict:
        raise ObservationProjectionError(f"{engine} observation is not an object")
    _validate_raw_envelope(raw, engine=engine)
    fixture = raw.get("fixture")
    if type(fixture) is not dict:
        raise ObservationProjectionError(f"{engine} fixture identity is malformed")
    for field in ("id", "nonce", "source_sha256", "composed_sha256"):
        if fixture.get(field) != expected_fixture.get(field):
            raise ObservationProjectionError(
                f"{engine} fixture identity field {field!r} does not match the invocation"
            )
    for marker_index, marker in enumerate(
        _canonical_markers(raw, engine=engine, interrupted=interrupted)
    ):
        for field in ("id", "nonce", "source_sha256", "composed_sha256"):
            if marker.get(field) != expected_fixture.get(field):
                raise ObservationProjectionError(
                    f"{engine} lifecycle marker {marker_index} field {field!r} "
                    "does not match the invocation"
                )
    canonical = _canonical_diagnostic(
        raw,
        engine=engine,
        interrupted=interrupted,
        preserve_unknown=True,
        diagnostic_subset=True,
    )
    _validate_diagnostic_context(
        raw,
        engine=engine,
        canonical=canonical,
        interrupted=interrupted,
    )
    return canonical


def _validate_capabilities(
    raw: dict,
    *,
    engine: str,
    require_firing_names: bool = False,
    require_globals: bool = False,
) -> None:
    capabilities = raw.get("capabilities")
    if type(capabilities) is not dict:
        raise ObservationProjectionError(f"{engine} capabilities are malformed")
    completed_run = type(raw.get("run")) is dict
    required = {"fact_modules"} if completed_run else set()
    if engine == "ferric":
        required.add("composed_digest_verification")
        if require_firing_names and completed_run:
            required.add("fired_rule_names")
        if require_globals and completed_run:
            required.add("global_values")
    else:
        if completed_run:
            required.add("rules_fired")
        if require_firing_names and completed_run:
            required.add("fired_rule_names")
    unavailable = sorted(name for name in required if capabilities.get(name) is not True)
    if unavailable:
        raise ObservationProjectionError(
            f"{engine} required capabilities are unavailable: {', '.join(unavailable)}"
        )


def _channel_map(raw: dict, *, engine: str) -> dict[str, str]:
    channels = raw.get("channels")
    if type(channels) is not list:
        raise ObservationProjectionError(f"{engine} channels are malformed")

    observed: dict[str, str] = {}
    seen: set[str] = set()
    for channel in channels:
        if type(channel) is not dict:
            raise ObservationProjectionError(f"{engine} channel record is malformed")
        name = channel.get("name")
        text = channel.get("text")
        if type(name) is not str or not name or type(text) is not str:
            raise ObservationProjectionError(f"{engine} channel record is malformed")
        if name in seen:
            raise ObservationProjectionError(f"{engine} has duplicate channel {name!r}")
        seen.add(name)
        if engine == "ferric":
            present = channel.get("present")
            if type(present) is not bool:
                raise ObservationProjectionError(f"{engine} channel presence is malformed")
            if not present and text:
                raise ObservationProjectionError(
                    f"{engine} absent channel {name!r} contains output"
                )
        observed[name] = text

    missing = {"t", "stderr"} - observed.keys()
    if missing:
        raise ObservationProjectionError(
            f"{engine} required channels are unavailable: {', '.join(sorted(missing))}"
        )
    return observed


def _focus_stack(raw: dict, *, engine: str) -> list[str]:
    modules = raw.get("modules")
    if type(modules) is not dict:
        raise ObservationProjectionError(f"{engine} module observation is malformed")
    focus_stack = modules.get("focus_stack")
    if type(focus_stack) is not list or not all(
        type(module) is str and module for module in focus_stack
    ):
        raise ObservationProjectionError(f"{engine} focus stack is malformed")
    return focus_stack


def _base_projection(
    raw: dict,
    *,
    engine: str,
    harnessed: bool,
) -> tuple[dict, list[dict]]:
    _validate_raw_envelope(raw, engine=engine)
    fixture = raw.get("fixture")
    run = raw.get("run")
    if type(fixture) is not dict:
        raise ObservationProjectionError(f"{engine} fixture identity is malformed")
    if run is not None and type(run) is not dict:
        raise ObservationProjectionError(f"{engine} run observation is malformed")
    diagnostic = _canonical_diagnostic(raw, engine=engine)
    observed_phase = _phase(raw.get("phase_reached"))
    raw_facts = raw.get("facts")
    if type(raw_facts) is not list:
        raise ObservationProjectionError(f"{engine} facts are malformed")
    if run is None:
        if diagnostic["phase"] == "none":
            raise ObservationProjectionError(
                f"{engine} did not complete a run and has no semantic diagnostic"
            )
        if diagnostic["continued"]:
            raise ObservationProjectionError(
                f"{engine} diagnostic claims continuation without a completed run"
            )
        if observed_phase != diagnostic["phase"]:
            raise ObservationProjectionError(
                f"{engine} terminal phase and diagnostic phase are inconsistent"
            )
        # A stopped load/reset/run does not provide a comparable final-state
        # snapshot. Preserve the envelope and channels, but do not project any
        # partial engine state as semantic evidence.
        facts: list[dict] = []
        run_projection = {"limit": None, "halt_reason": "not-run"}
    else:
        assert isinstance(run, dict)
        facts = [_canonical_fact(fact, engine=engine, harnessed=harnessed) for fact in raw_facts]
        fact_ids = [fact["id"] for fact in facts]
        if len(fact_ids) != len(set(fact_ids)):
            raise ObservationProjectionError(f"{engine} observation contains a duplicate fact id")
        facts = [fact for fact in facts if fact["origin"] != "instrumentation"]
        halt_reason = _halt_reason(run.get("halt_reason"))
        run_projection = {"limit": None, "halt_reason": halt_reason}
        if diagnostic["phase"] == "none":
            if observed_phase != "run-complete":
                raise ObservationProjectionError(
                    f"{engine} stopped before run completion without a diagnostic"
                )
        elif diagnostic["continued"]:
            if observed_phase != "run-complete":
                raise ObservationProjectionError(
                    f"{engine} continued diagnostic lacks a completed run"
                )
        elif diagnostic["phase"] != "run" or observed_phase != "run":
            raise ObservationProjectionError(
                f"{engine} non-continuing diagnostic is inconsistent with run evidence"
            )
        terminal_run_diagnostic = {
            "phase": "run",
            "category": "evaluation-error",
            "continued": False,
        }
        if halt_reason == "action-error" and diagnostic != terminal_run_diagnostic:
            raise ObservationProjectionError(
                f"{engine} action-error halt lacks a matching evaluation diagnostic"
            )
        if diagnostic == terminal_run_diagnostic and halt_reason != "action-error":
            raise ObservationProjectionError(
                f"{engine} terminal run diagnostic lacks an action-error halt"
            )
    return (
        {
            "version": 1,
            "id": fixture.get("id"),
            "source_sha256": fixture.get("source_sha256"),
            "composed_sha256": fixture.get("composed_sha256"),
            "nonce": fixture.get("nonce"),
            "markers": _canonical_markers(raw, engine=engine),
            "phase": observed_phase,
            "effects": [_fact_effect(fact) for fact in facts],
            "facts": facts,
            "diagnostic": diagnostic,
            "run": run_projection,
            "globals": None,
        },
        facts,
    )


def project_ferric_observation(
    raw: object,
    *,
    harnessed: bool,
    require_firing_names: bool = False,
    require_globals: bool = False,
) -> dict:
    """Project one raw ``ferric compat-observe`` envelope."""
    if type(raw) is not dict:
        raise ObservationProjectionError("Ferric observation is not an object")
    assert isinstance(raw, dict)
    _validate_capabilities(
        raw,
        engine="ferric",
        require_firing_names=require_firing_names,
        require_globals=require_globals,
    )
    projected, _facts = _base_projection(raw, engine="ferric", harnessed=harnessed)
    run = raw.get("run")
    if run is None:
        rules_fired = 0
    else:
        assert isinstance(run, dict)
        rules_fired = run.get("rules_fired")
        if type(rules_fired) is not int or rules_fired < 0:
            raise ObservationProjectionError("Ferric firing count is unavailable")
    if harnessed and run is not None:
        raise ObservationProjectionError(
            "Ferric cannot separate harness firings from fixture firings"
        )
    projected["firings"] = [
        {"rule": f"counted-firing-{index + 1}", "origin": "fixture"} for index in range(rules_fired)
    ]

    channel_map = _channel_map(raw, engine="ferric")
    stdout = channel_map["t"]
    if harnessed:
        stdout, _instrumentation = _strip_harness_output(stdout)
    projected["channels"] = {
        "stdout": stdout,
        "stderr": channel_map["stderr"],
    }
    projected["focus_stack"] = _focus_stack(raw, engine="ferric")
    return projected


def project_clips_observation(
    raw: object,
    *,
    harnessed: bool,
    require_firing_names: bool = False,
) -> dict:
    """Project one parsed reference-CLIPS observation envelope."""
    if type(raw) is not dict:
        raise ObservationProjectionError("CLIPS observation is not an object")
    assert isinstance(raw, dict)
    _validate_capabilities(
        raw,
        engine="clips",
        require_firing_names=require_firing_names,
    )
    projected, _facts = _base_projection(raw, engine="clips", harnessed=harnessed)
    run = raw.get("run")
    if run is None:
        rules_fired = 0
    else:
        assert isinstance(run, dict)
        rules_fired = run.get("rules_fired")
        if type(rules_fired) is not int or rules_fired < 0:
            raise ObservationProjectionError("CLIPS firing count is unavailable")
    if harnessed and run is not None:
        raise ObservationProjectionError(
            "CLIPS cannot separate harness firings from fixture firings"
        )
    fired_rules = raw.get("fired_rules")
    if require_firing_names and run is not None:
        if type(fired_rules) is not list or not all(
            type(rule) is str and rule for rule in fired_rules
        ):
            raise ObservationProjectionError("CLIPS fired-rule trace is unavailable")
        if rules_fired != len(fired_rules):
            raise ObservationProjectionError("CLIPS firing count and trace are inconsistent")
        projected["firings"] = [{"rule": rule, "origin": "fixture"} for rule in fired_rules]
    else:
        projected["firings"] = [
            {"rule": f"counted-firing-{index + 1}", "origin": "fixture"}
            for index in range(rules_fired)
        ]

    channel_map = _channel_map(raw, engine="clips")
    stdout = channel_map["t"]
    if harnessed:
        stdout, _instrumentation = _strip_harness_output(stdout)
    projected["channels"] = {
        "stdout": stdout,
        "stderr": channel_map["stderr"],
    }
    projected["focus_stack"] = _focus_stack(raw, engine="clips")
    globals_raw = raw.get("globals")
    if type(globals_raw) is not dict:
        raise ObservationProjectionError("CLIPS globals are malformed")
    if globals_raw:
        if not all(type(name) is str and name for name in globals_raw):
            raise ObservationProjectionError("CLIPS global name is malformed")
        projected["globals"] = [
            {"name": name, "value": _canonical_value(value, engine="clips")}
            for name, value in sorted(globals_raw.items())
        ]
    return projected
