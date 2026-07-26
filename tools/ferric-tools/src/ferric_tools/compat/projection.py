"""Project engine-specific observation envelopes into oracle v1.

Raw adapters intentionally expose their actual capabilities.  This module is
the single place that translates those envelopes into the strict,
engine-neutral schema consumed by :mod:`ferric_tools.compat.oracle`.
"""

from __future__ import annotations

import re

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
        "error",
        "not-run",
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


def _canonical_diagnostic(raw: dict, *, engine: str) -> dict:
    diagnostics = raw.get("diagnostics")
    if type(diagnostics) is not list or not all(
        type(diagnostic) is dict for diagnostic in diagnostics
    ):
        raise ObservationProjectionError(f"{engine} diagnostics are malformed")
    protocol_issues = (
        raw.get("protocol_issues", []) if engine == "ferric" else raw.get("protocol_issues")
    )
    if type(protocol_issues) is not list:
        raise ObservationProjectionError(f"{engine} protocol issue evidence is malformed")
    if protocol_issues:
        raise ObservationProjectionError(
            f"{engine} protocol violations: {', '.join(map(str, protocol_issues))}"
        )
    if diagnostics:
        # Phase-aware diagnostic equivalence is deliberately unavailable until
        # FR-COMPAT-005 (#119). Unknown evidence fails strict oracle validation.
        return {"phase": "unknown", "category": "unknown", "continued": False}
    return {"phase": "none", "category": "none", "continued": True}


def _canonical_markers(raw: dict, *, engine: str) -> list[dict]:
    fixture = raw.get("fixture")
    lifecycle = raw.get("lifecycle")
    if type(fixture) is not dict or type(lifecycle) is not list:
        raise ObservationProjectionError("observation identity or lifecycle is malformed")
    expected_sequences = (0, 1) if engine == "ferric" else (0, 3)
    sequences = [record.get("sequence") if type(record) is dict else None for record in lifecycle]
    if (
        len(sequences) != 2
        or not all(type(sequence) is int for sequence in sequences)
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
    if [marker["kind"] for marker in markers] != ["START", "COMPLETE"]:
        raise ObservationProjectionError(f"{engine} lifecycle event order is malformed")
    return markers


def _phase(raw: object) -> str:
    if raw in {"post-run", "post_run"}:
        return "run-complete"
    raise ObservationProjectionError(f"unsupported observed phase: {raw!r}")


def _halt_reason(raw: object) -> str:
    if type(raw) is not str:
        raise ObservationProjectionError("run halt reason is missing")
    normalized = raw.replace("_", "-")
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
    required = {"fact_modules"}
    if engine == "ferric":
        required.add("composed_digest_verification")
        if require_firing_names:
            required.add("fired_rule_names")
        if require_globals:
            required.add("global_values")
    else:
        required.add("rules_fired")
        if require_firing_names:
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
    if type(run) is not dict:
        raise ObservationProjectionError(f"{engine} did not complete a run")
    raw_facts = raw.get("facts")
    if type(raw_facts) is not list:
        raise ObservationProjectionError(f"{engine} facts are malformed")
    facts = [_canonical_fact(fact, engine=engine, harnessed=harnessed) for fact in raw_facts]
    fact_ids = [fact["id"] for fact in facts]
    if len(fact_ids) != len(set(fact_ids)):
        raise ObservationProjectionError(f"{engine} observation contains a duplicate fact id")
    facts = [fact for fact in facts if fact["origin"] != "instrumentation"]
    return (
        {
            "version": 1,
            "id": fixture.get("id"),
            "source_sha256": fixture.get("source_sha256"),
            "composed_sha256": fixture.get("composed_sha256"),
            "nonce": fixture.get("nonce"),
            "markers": _canonical_markers(raw, engine=engine),
            "phase": _phase(raw.get("phase_reached")),
            "effects": [_fact_effect(fact) for fact in facts],
            "facts": facts,
            "diagnostic": _canonical_diagnostic(raw, engine=engine),
            "run": {
                "limit": None,
                "halt_reason": _halt_reason(run.get("halt_reason")),
            },
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
    run = raw["run"]
    assert isinstance(run, dict)
    rules_fired = run.get("rules_fired")
    if type(rules_fired) is not int or rules_fired < 0:
        raise ObservationProjectionError("Ferric firing count is unavailable")
    if harnessed:
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
    run = raw["run"]
    assert isinstance(run, dict)
    rules_fired = run.get("rules_fired")
    if type(rules_fired) is not int or rules_fired < 0:
        raise ObservationProjectionError("CLIPS firing count is unavailable")
    if harnessed:
        raise ObservationProjectionError(
            "CLIPS cannot separate harness firings from fixture firings"
        )
    fired_rules = raw.get("fired_rules")
    if require_firing_names:
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
