"""Structured post-run observations for the reference CLIPS process.

The reference image exposes a small native observer alongside the CLIPS
interpreter.  The native boundary brackets the exact load/reset/run sequence,
counts the remaining agenda across every module, and gates post-run probe
records so fixture code cannot mint accepted evidence.  Its per-invocation
binding is consumed from the input stream before the CLIPS environment is
created; the nonce is never placed in the fixture-visible environment or
command stream.
"""

from __future__ import annotations

import hashlib
import hmac
import re
from collections.abc import Iterable
from dataclasses import dataclass

ORACLE_SCHEMA = "ferric.compat-observation"
ORACLE_VERSION = 1
RECORD_PREFIX = "__FERRIC_COMPAT_ORACLE_V1__|"
NATIVE_RECORD_PREFIX = "__FERRIC_COMPAT_NATIVE_V1__|"
NATIVE_EMIT_FUNCTION = "ferric-compat-native-emit"
NATIVE_COMPLETE_FUNCTION = "ferric-compat-native-complete"
_TOKEN_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._:/-]{0,127}$")
_DIGEST_RE = re.compile(r"^[0-9a-f]{64}$")
_NONCE_RE = re.compile(r"^[0-9a-f]{32,128}$")
_AUTH_KEY_RE = re.compile(r"^[0-9a-f]{64}$")
_DIAGNOSTIC_RE = re.compile(r"\[[A-Z][A-Z0-9]*\d+\]")
_HARNESS_LINE_RE = re.compile(r"^FERRIC-HARNESS\|(?P<version>\d+)\|(?P<body>.*)$")
_MAX_PROBE_PAYLOAD = 16 * 1024 * 1024
_PROBE_KINDS = frozenset({"PHASE", "MODULE", "FOCUS", "FACT", "SLOT", "VALUE", "GLOBAL"})


class ClipsOracleProtocolError(ValueError):
    """Raised when nonce-bound native CLIPS output is malformed."""


@dataclass(frozen=True)
class _Record:
    kind: str
    fields: tuple[str, ...]
    value: str | None = None


@dataclass(frozen=True)
class _NativeRecord:
    kind: str
    fields: tuple[str, ...]
    payload: bytes | None
    start: int
    end: int


def _require_token(value: str, *, label: str) -> str:
    if not _TOKEN_RE.fullmatch(value):
        raise ValueError(f"{label} must be a non-empty protocol-safe token")
    return value


def _require_digest(value: str, *, label: str) -> str:
    if not _DIGEST_RE.fullmatch(value):
        raise ValueError(f"{label} must be a lowercase SHA-256 digest")
    return value


def _require_nonce(value: str) -> str:
    if not _NONCE_RE.fullmatch(value) or len(value) % 2 != 0:
        raise ValueError("nonce must be 16-64 bytes encoded as lowercase hexadecimal")
    return value


def _require_auth_key(value: str) -> str:
    if not _AUTH_KEY_RE.fullmatch(value):
        raise ValueError("observer authentication key must be 32 bytes of lowercase hexadecimal")
    return value


def _clips_string(value: str) -> str:
    """Quote a protocol-controlled value as a CLIPS string."""
    if any(ord(character) < 0x20 or ord(character) == 0x7F for character in value):
        raise ValueError("CLIPS protocol string contains a control character")
    return '"' + value.replace("\\", "\\\\").replace('"', '\\"') + '"'


def build_probe_operations(
    *,
    fixture_id: str,
    nonce: str,
    source_sha256: str,
    composed_sha256: str,
    globals_to_capture: Iterable[str] = (),
) -> list[str]:
    """Build post-load operations for one native-gated observation.

    The nonce and identity are validated here but deliberately do not appear in
    these CLIPS expressions.  They reach the native observer through a
    consumed-before-exec descriptor established by ``clips-reference.sh``.
    """
    _require_token(fixture_id, label="fixture id")
    _require_nonce(nonce)
    _require_digest(source_sha256, label="source digest")
    _require_digest(composed_sha256, label="composed digest")
    global_names = tuple(
        _require_token(name, label="global name").removeprefix("?*").removesuffix("*")
        for name in globals_to_capture
    )

    emit_name = "__ferric_compat_emit_value"
    dump_name = "__ferric_compat_dump"

    emit_definition = (
        f"(deffunction MAIN::{emit_name} "
        "(?fact-seq ?fact-id ?module ?relation ?slot-index ?slot ?position ?value) "
        "(bind ?text (str-cat ?value)) "
        f"({NATIVE_EMIT_FUNCTION} "
        '(str-cat "VALUE|" ?fact-seq "|" ?fact-id "|" ?module "|" '
        '?relation "|" ?slot-index "|" ?slot "|" ?position "|" (type ?value) "|" '
        "?text)))"
    )

    dump_definition = (
        f"(deffunction MAIN::{dump_name} () "
        "(bind ?fact-seq 0) "
        "(progn$ (?fact (get-fact-list *)) "
        "(if (neq (fact-relation ?fact) initial-fact) then "
        "(bind ?fact-seq (+ ?fact-seq 1)) "
        "(bind ?relation (fact-relation ?fact)) "
        "(bind ?module (deftemplate-module ?relation)) "
        "(bind ?slots (fact-slot-names ?fact)) "
        "(bind ?kind (if (and (= (length$ ?slots) 1) "
        "(eq (nth$ 1 ?slots) implied)) then ordered else template)) "
        f"({NATIVE_EMIT_FUNCTION} "
        '(str-cat "FACT|" ?fact-seq "|" (fact-index ?fact) "|" ?module "|" '
        '?relation "|" ?kind "|" (length$ ?slots))) '
        "(bind ?slot-index 0) "
        "(progn$ (?slot ?slots) "
        "(bind ?slot-index (+ ?slot-index 1)) "
        "(bind ?value (fact-slot-value ?fact ?slot)) "
        "(if (multifieldp ?value) then "
        f"({NATIVE_EMIT_FUNCTION} "
        '(str-cat "SLOT|" ?fact-seq "|" (fact-index ?fact) "|" '
        '?slot-index "|" ?slot "|MULTIFIELD|" (length$ ?value))) '
        "(loop-for-count (?position 1 (length$ ?value)) "
        f"({emit_name} ?fact-seq (fact-index ?fact) ?module ?relation "
        "?slot-index ?slot ?position (nth$ ?position ?value))) "
        "else "
        f"({NATIVE_EMIT_FUNCTION} "
        '(str-cat "SLOT|" ?fact-seq "|" (fact-index ?fact) "|" '
        '?slot-index "|" ?slot "|ATOMIC|1")) '
        f"({emit_name} ?fact-seq (fact-index ?fact) ?module ?relation "
        "?slot-index ?slot 0 ?value))))) "
        "(return TRUE))"
    )

    operations = [
        f'({NATIVE_EMIT_FUNCTION} "PHASE|1|RESET_COMPLETE")',
        f'({NATIVE_EMIT_FUNCTION} "PHASE|2|RUN_COMPLETE")',
        f'({NATIVE_EMIT_FUNCTION} (str-cat "MODULE|" (get-current-module)))',
        (f'(progn$ (?focus (get-focus-stack)) ({NATIVE_EMIT_FUNCTION} (str-cat "FOCUS|" ?focus)))'),
        "(set-current-module MAIN)",
        emit_definition,
        dump_definition,
        f"({dump_name})",
    ]
    for name in global_names:
        clips_reference = f"?*{name}*"
        operations.append(f"(bind ?__ferric_global_text (str-cat {clips_reference}))")
        operations.append(
            f"({NATIVE_EMIT_FUNCTION} "
            f'(str-cat "GLOBAL|{name}|" (type {clips_reference}) "|" '
            "?__ferric_global_text))"
        )
    operations.append(f"({NATIVE_COMPLETE_FUNCTION})")
    return operations


def _decode_utf8(value: bytes, *, label: str) -> str:
    try:
        return value.decode("utf-8")
    except UnicodeDecodeError as error:
        raise ClipsOracleProtocolError(f"{label} is not valid UTF-8") from error


def _merge_intervals(intervals: list[tuple[int, int]]) -> list[tuple[int, int]]:
    if not intervals:
        return []
    merged: list[tuple[int, int]] = []
    for start, end in sorted(intervals):
        if merged and start <= merged[-1][1]:
            merged[-1] = (merged[-1][0], max(end, merged[-1][1]))
        else:
            merged.append((start, end))
    return merged


def _remove_intervals(data: bytes, intervals: list[tuple[int, int]]) -> bytes:
    pieces: list[bytes] = []
    cursor = 0
    for start, end in _merge_intervals(intervals):
        pieces.append(data[cursor:start])
        cursor = end
    pieces.append(data[cursor:])
    return b"".join(pieces)


def _parse_native_records(
    raw: bytes,
    *,
    nonce: str,
    auth_key: str,
) -> tuple[list[_NativeRecord], str, list[str]]:
    exact_prefix = f"\n{NATIVE_RECORD_PREFIX}{nonce}|".encode()
    reserved_prefix = NATIVE_RECORD_PREFIX.encode()
    auth_key_bytes = bytes.fromhex(auth_key)
    records: list[_NativeRecord] = []
    intervals: list[tuple[int, int]] = []
    issues: list[str] = []
    cursor = 0

    while True:
        start = raw.find(exact_prefix, cursor)
        if start < 0:
            break
        kind_start = start + len(exact_prefix)
        kind_end = raw.find(b"|", kind_start)
        newline = raw.find(b"\n", kind_start)
        if newline >= 0 and (kind_end < 0 or newline < kind_end):
            kind = _decode_utf8(raw[kind_start:newline], label="native record kind")
            end = newline + 1
            records.append(_NativeRecord(kind, (), None, start, end))
            intervals.append((start, end))
            cursor = end
            continue
        if kind_end < 0:
            raise ClipsOracleProtocolError("native record has no kind delimiter")
        kind = _decode_utf8(raw[kind_start:kind_end], label="native record kind")
        payload_start = kind_end + 1

        if kind == "PROBE":
            length_end = raw.find(b"|", payload_start)
            if length_end < 0:
                raise ClipsOracleProtocolError("native PROBE length is truncated")
            try:
                payload_length = int(raw[payload_start:length_end])
            except ValueError as error:
                raise ClipsOracleProtocolError("native PROBE length is not an integer") from error
            if payload_length < 0 or payload_length > _MAX_PROBE_PAYLOAD:
                raise ClipsOracleProtocolError("native PROBE length is out of range")
            value_start = length_end + 1
            value_end = value_start + payload_length
            mac_start = value_end + 1
            mac_end = mac_start + 64
            if (
                value_end >= len(raw)
                or raw[value_end] != ord("|")
                or mac_end >= len(raw)
                or raw[mac_end] != ord("\n")
            ):
                raise ClipsOracleProtocolError("native PROBE payload length does not match record")
            end = mac_end + 1
            logical_record = raw[kind_start:value_end]
            supplied_mac = raw[mac_start:mac_end]
            expected_mac = (
                hmac.new(
                    auth_key_bytes,
                    logical_record,
                    hashlib.sha256,
                )
                .hexdigest()
                .encode()
            )
            if not hmac.compare_digest(supplied_mac, expected_mac):
                issues.append("native-authentication-failed")
            else:
                records.append(_NativeRecord(kind, (), raw[value_start:value_end], start, end))
        else:
            end_of_line = raw.find(b"\n", payload_start)
            if end_of_line < 0:
                raise ClipsOracleProtocolError("native record is not newline terminated")
            end = end_of_line + 1
            authenticated = raw[kind_start:end_of_line]
            logical_record, separator, supplied_mac = authenticated.rpartition(b"|")
            if not separator or len(supplied_mac) != 64:
                raise ClipsOracleProtocolError("native record authentication is malformed")
            expected_mac = (
                hmac.new(
                    auth_key_bytes,
                    logical_record,
                    hashlib.sha256,
                )
                .hexdigest()
                .encode()
            )
            if not hmac.compare_digest(supplied_mac, expected_mac):
                issues.append("native-authentication-failed")
            else:
                logical_kind, kind_separator, logical_payload = logical_record.partition(b"|")
                if not kind_separator:
                    raise ClipsOracleProtocolError("native record has no logical payload")
                if _decode_utf8(logical_kind, label="native logical record kind") != kind:
                    raise ClipsOracleProtocolError("native record kind is inconsistent")
                fields = tuple(
                    _decode_utf8(field, label=f"native {kind} field")
                    for field in logical_payload.split(b"|")
                )
                records.append(_NativeRecord(kind, fields, None, start, end))

        intervals.append((start, end))
        cursor = end

    semantic_bytes = _remove_intervals(raw, intervals)
    if reserved_prefix in semantic_bytes:
        issues.append("unexpected-native-reserved-prefix")
    semantic_stderr = _decode_utf8(semantic_bytes, label="semantic stderr")
    return records, semantic_stderr, issues


def _parse_probe_payload(payload: bytes) -> _Record:
    kind_end = payload.find(b"|")
    if kind_end < 0:
        return _Record(_decode_utf8(payload, label="probe kind"), ())
    kind = _decode_utf8(payload[:kind_end], label="probe kind")
    remainder = payload[kind_end + 1 :]

    if kind == "VALUE":
        fields = remainder.split(b"|", 8)
        if len(fields) != 9:
            return _Record(
                kind,
                tuple(_decode_utf8(field, label="VALUE field") for field in fields),
            )
        return _Record(
            kind,
            tuple(_decode_utf8(field, label="VALUE field") for field in fields[:-1]),
            _decode_utf8(fields[-1], label="VALUE payload"),
        )
    if kind == "GLOBAL":
        fields = remainder.split(b"|", 2)
        if len(fields) != 3:
            return _Record(
                kind,
                tuple(_decode_utf8(field, label="GLOBAL field") for field in fields),
            )
        return _Record(
            kind,
            tuple(_decode_utf8(field, label="GLOBAL field") for field in fields[:-1]),
            _decode_utf8(fields[-1], label="GLOBAL payload"),
        )
    return _Record(
        kind,
        tuple(_decode_utf8(field, label=f"{kind} field") for field in remainder.split(b"|")),
    )


def _typed_value(type_name: str, value: str) -> dict:
    normalized_type = type_name.lower().replace("-", "_")
    return {"type": normalized_type, "value": value}


def _parse_int(text: str, *, issue: str, issues: list[str]) -> int | None:
    try:
        return int(text)
    except ValueError:
        issues.append(issue)
        return None


def parse_probe_output(
    raw_stdout: str | bytes,
    *,
    raw_stderr: str | bytes,
    fixture_id: str,
    nonce: str,
    source_sha256: str,
    composed_sha256: str,
    auth_key: str,
    harnessed: bool = False,
) -> dict:
    """Parse one quiet CLIPS invocation into a structured observation."""
    fixture_id = _require_token(fixture_id, label="fixture id")
    nonce = _require_nonce(nonce)
    source_sha256 = _require_digest(source_sha256, label="source digest")
    composed_sha256 = _require_digest(composed_sha256, label="composed digest")
    auth_key = _require_auth_key(auth_key)

    stdout_bytes = raw_stdout.encode("utf-8") if isinstance(raw_stdout, str) else raw_stdout
    stderr_bytes = raw_stderr.encode("utf-8") if isinstance(raw_stderr, str) else raw_stderr
    native_records, semantic_stderr, protocol_issues = _parse_native_records(
        stderr_bytes,
        nonce=nonce,
        auth_key=auth_key,
    )

    allowed_native_kinds = {"LIFECYCLE", "RUN", "PROBE", "ISSUE"}
    unknown_native_kinds = [
        record.kind for record in native_records if record.kind not in allowed_native_kinds
    ]
    if unknown_native_kinds:
        protocol_issues.append("unknown-native-record-kind")

    for record in native_records:
        if record.kind == "ISSUE":
            if len(record.fields) != 1 or not record.fields[0]:
                protocol_issues.append("native-issue-field-count")
            else:
                protocol_issues.append(f"native-{record.fields[0]}")

    lifecycle_records = [record for record in native_records if record.kind == "LIFECYCLE"]
    lifecycle: list[dict] = []
    expected_binding = (fixture_id, source_sha256, composed_sha256)
    for record in lifecycle_records:
        if len(record.fields) != 5:
            protocol_issues.append("lifecycle-field-count")
            continue
        sequence_text, event, bound_fixture, bound_source, bound_composed = record.fields
        sequence = _parse_int(
            sequence_text,
            issue="lifecycle-sequence",
            issues=protocol_issues,
        )
        if sequence is None:
            continue
        if (bound_fixture, bound_source, bound_composed) != expected_binding:
            protocol_issues.append("lifecycle-binding")
        lifecycle.append(
            {
                "sequence": sequence,
                "event": event.lower(),
                "fixture_id": bound_fixture,
                "nonce": nonce,
                "source_sha256": bound_source,
                "composed_sha256": bound_composed,
            }
        )

    if [entry["event"] for entry in lifecycle] != ["start", "complete"]:
        protocol_issues.append("lifecycle-cardinality-or-order")
    if lifecycle and [entry["sequence"] for entry in lifecycle] != [0, 3]:
        protocol_issues.append("lifecycle-sequence-order")

    native_run_records = [record for record in native_records if record.kind == "RUN"]
    native_run: dict[str, int] | None = None
    if len(native_run_records) != 1:
        protocol_issues.append(
            "native-run-metadata-missing" if not native_run_records else "native-run-cardinality"
        )
    else:
        record = native_run_records[0]
        if len(record.fields) != 7:
            protocol_issues.append("native-run-field-count")
        else:
            numeric_fields = [
                _parse_int(field, issue="native-run-numeric-field", issues=protocol_issues)
                for field in record.fields
            ]
            if all(value is not None for value in numeric_fields):
                (
                    run_limit,
                    rules_fired,
                    halt_rules,
                    halt_execution,
                    evaluation_error,
                    agenda_size,
                    observer_violation,
                ) = numeric_fields
                assert run_limit is not None
                assert rules_fired is not None
                assert halt_rules is not None
                assert halt_execution is not None
                assert evaluation_error is not None
                assert agenda_size is not None
                assert observer_violation is not None
                if (
                    run_limit != -1
                    or rules_fired < 0
                    or halt_rules not in {0, 1}
                    or halt_execution not in {0, 1}
                    or evaluation_error not in {0, 1}
                    or agenda_size < 0
                    or observer_violation not in {0, 1}
                ):
                    protocol_issues.append("native-run-value")
                else:
                    native_run = {
                        "run_limit": run_limit,
                        "rules_fired": rules_fired,
                        "halt_rules": halt_rules,
                        "halt_execution": halt_execution,
                        "evaluation_error": evaluation_error,
                        "agenda_size": agenda_size,
                        "observer_violation": observer_violation,
                    }
                    if observer_violation:
                        protocol_issues.append("native-observer-violation")

    record_positions = {id(record): index for index, record in enumerate(native_records)}
    if len(lifecycle_records) == 2 and len(native_run_records) == 1:
        start_position = record_positions[id(lifecycle_records[0])]
        run_position = record_positions[id(native_run_records[0])]
        complete_position = record_positions[id(lifecycle_records[1])]
        probe_positions = [
            record_positions[id(record)] for record in native_records if record.kind == "PROBE"
        ]
        if not (
            start_position < run_position
            and all(run_position < position < complete_position for position in probe_positions)
            and complete_position == len(native_records) - 1
        ):
            protocol_issues.append("native-record-order")

    records = [
        _parse_probe_payload(record.payload)
        for record in native_records
        if record.kind == "PROBE" and record.payload is not None
    ]
    unknown_probe_kinds = [record.kind for record in records if record.kind not in _PROBE_KINDS]
    if unknown_probe_kinds:
        protocol_issues.append("unknown-probe-record-kind")

    phase_records = [record for record in records if record.kind == "PHASE"]
    phase_values = [record.fields for record in phase_records]
    if phase_values != [("1", "RESET_COMPLETE"), ("2", "RUN_COMPLETE")]:
        protocol_issues.append("phase-cardinality-or-order")

    facts_by_sequence: dict[int, dict] = {}
    slots_by_key: dict[tuple[int, int], dict] = {}
    for record in records:
        if record.kind == "FACT":
            if len(record.fields) != 6:
                protocol_issues.append("fact-field-count")
                continue
            sequence_text, fact_id, module, relation, kind, slot_count_text = record.fields
            sequence = _parse_int(
                sequence_text,
                issue="fact-numeric-field",
                issues=protocol_issues,
            )
            slot_count = _parse_int(
                slot_count_text,
                issue="fact-numeric-field",
                issues=protocol_issues,
            )
            if sequence is None or slot_count is None:
                continue
            if sequence <= 0 or slot_count < 0 or kind not in {"ordered", "template"}:
                protocol_issues.append("fact-value")
                continue
            if sequence in facts_by_sequence:
                protocol_issues.append("duplicate-fact-sequence")
                continue
            facts_by_sequence[sequence] = {
                "ordinal": sequence,
                "fact_id": fact_id,
                "module": module,
                "kind": kind,
                "relation": relation,
                "_slot_count": slot_count,
                "_slots": {},
            }
        elif record.kind == "SLOT":
            if len(record.fields) != 6:
                protocol_issues.append("slot-field-count")
                continue
            sequence_text, fact_id, slot_index_text, name, slot_kind, count_text = record.fields
            sequence = _parse_int(
                sequence_text,
                issue="slot-numeric-field",
                issues=protocol_issues,
            )
            slot_index = _parse_int(
                slot_index_text,
                issue="slot-numeric-field",
                issues=protocol_issues,
            )
            item_count = _parse_int(
                count_text,
                issue="slot-numeric-field",
                issues=protocol_issues,
            )
            if sequence is None or slot_index is None or item_count is None:
                continue
            if (
                sequence <= 0
                or slot_index <= 0
                or item_count < 0
                or slot_kind not in {"MULTIFIELD", "ATOMIC"}
            ):
                protocol_issues.append("slot-value")
                continue
            slot = {
                "fact_id": fact_id,
                "index": slot_index,
                "name": name,
                "kind": slot_kind,
                "item_count": item_count,
                "items": {},
            }
            key = (sequence, slot_index)
            if key in slots_by_key:
                protocol_issues.append("duplicate-slot")
            slots_by_key[key] = slot
        elif record.kind == "VALUE":
            if len(record.fields) != 8 or record.value is None:
                protocol_issues.append("value-field-count")
                continue
            (
                sequence_text,
                fact_id,
                module,
                relation,
                slot_index_text,
                slot_name,
                position_text,
                type_name,
            ) = record.fields
            sequence = _parse_int(
                sequence_text,
                issue="value-numeric-field",
                issues=protocol_issues,
            )
            slot_index = _parse_int(
                slot_index_text,
                issue="value-numeric-field",
                issues=protocol_issues,
            )
            position = _parse_int(
                position_text,
                issue="value-numeric-field",
                issues=protocol_issues,
            )
            if sequence is None or slot_index is None or position is None:
                continue
            slot = slots_by_key.get((sequence, slot_index))
            if slot is None:
                protocol_issues.append("value-before-slot")
                continue
            if (
                slot["fact_id"] != fact_id
                or slot["name"] != slot_name
                or facts_by_sequence.get(sequence, {}).get("module") != module
                or facts_by_sequence.get(sequence, {}).get("relation") != relation
            ):
                protocol_issues.append("value-binding")
            if position in slot["items"]:
                protocol_issues.append("duplicate-value-position")
            slot["items"][position] = _typed_value(type_name, record.value)

    fact_sequences = sorted(facts_by_sequence)
    if fact_sequences != list(range(1, len(fact_sequences) + 1)):
        protocol_issues.append("fact-sequence-order")

    facts: list[dict] = []
    for sequence, fact in sorted(facts_by_sequence.items()):
        fact_slots = [
            slot
            for (fact_sequence, _), slot in sorted(slots_by_key.items())
            if fact_sequence == sequence
        ]
        if len(fact_slots) != fact["_slot_count"]:
            protocol_issues.append("fact-slot-count")
        if [slot["index"] for slot in fact_slots] != list(range(1, len(fact_slots) + 1)):
            protocol_issues.append("slot-index-order")

        canonical_slots: list[dict] = []
        for slot in fact_slots:
            positions = sorted(slot["items"])
            expected_positions = (
                list(range(1, slot["item_count"] + 1)) if slot["kind"] == "MULTIFIELD" else [0]
            )
            if positions != expected_positions:
                protocol_issues.append("slot-item-positions")
            items = [slot["items"][position] for position in positions]
            if slot["kind"] == "MULTIFIELD":
                value = {"type": "multifield", "items": items}
            elif slot["item_count"] == 1 and len(items) == 1:
                value = items[0]
            else:
                protocol_issues.append("atomic-slot-cardinality")
                value = {"type": "invalid", "items": items}
            canonical_slots.append({"name": slot["name"], "value": value})

        canonical_fact = {key: value for key, value in fact.items() if not key.startswith("_")}
        if fact["kind"] == "ordered":
            if len(canonical_slots) != 1 or canonical_slots[0]["name"] != "implied":
                protocol_issues.append("ordered-implied-slot")
                canonical_fact["fields"] = []
            else:
                implied = canonical_slots[0]["value"]
                canonical_fact["fields"] = implied.get("items", [])
        else:
            canonical_fact["slots"] = canonical_slots
        facts.append(canonical_fact)

    if any(sequence not in facts_by_sequence for sequence, _slot_index in slots_by_key):
        protocol_issues.append("orphan-slot")

    modules = {"current": None, "focus": None, "focus_stack": []}
    module_records = [record for record in records if record.kind == "MODULE"]
    if len(module_records) == 1 and len(module_records[0].fields) == 1:
        modules["current"] = module_records[0].fields[0]
    else:
        protocol_issues.append("module-cardinality")
    focus_records = [record for record in records if record.kind == "FOCUS"]
    if any(len(record.fields) != 1 or not record.fields[0] for record in focus_records):
        protocol_issues.append("focus-record")
    modules["focus_stack"] = [
        record.fields[0] for record in focus_records if len(record.fields) == 1
    ]
    modules["focus"] = modules["focus_stack"][-1] if modules["focus_stack"] else None

    globals_observed: dict[str, dict] = {}
    for record in records:
        if record.kind != "GLOBAL":
            continue
        if len(record.fields) != 2 or record.value is None:
            protocol_issues.append("global-field-count")
            continue
        name, type_name = record.fields
        if name in globals_observed:
            protocol_issues.append("duplicate-global")
        globals_observed[name] = _typed_value(type_name, record.value)

    semantic_stdout = _decode_utf8(stdout_bytes, label="semantic stdout")
    harness_records: list[dict] = []
    feature_lines: list[str] = []
    for line in semantic_stdout.splitlines(keepends=True):
        match = _HARNESS_LINE_RE.match(line.rstrip("\r\n")) if harnessed else None
        if match:
            harness_records.append(
                {"version": int(match.group("version")), "record": match.group("body")}
            )
        else:
            feature_lines.append(line)
    semantic_stdout = "".join(feature_lines)

    if RECORD_PREFIX in semantic_stdout or NATIVE_RECORD_PREFIX in semantic_stdout:
        protocol_issues.append("unexpected-reserved-prefix")

    diagnostics: list[dict] = []
    for channel, text in (("stdout", semantic_stdout), ("stderr", semantic_stderr)):
        for match in _DIAGNOSTIC_RE.finditer(text):
            diagnostics.append(
                {
                    "phase": "unknown",
                    "category": match.group(0),
                    "continuation": "unknown",
                    "channel": channel,
                }
            )

    if native_run is None:
        run_state = None
    else:
        if native_run["evaluation_error"] or (
            native_run["halt_execution"] and not native_run["halt_rules"]
        ):
            halt_reason = "error"
        elif native_run["halt_rules"]:
            halt_reason = "halt_requested"
        elif native_run["agenda_size"] != 0:
            protocol_issues.append("run-returned-with-remaining-activations")
            halt_reason = "error"
        else:
            halt_reason = "agenda_empty"
        run_state = {
            "rules_fired": native_run["rules_fired"],
            "halt_reason": halt_reason,
            "agenda_size": native_run["agenda_size"],
            "halted": halt_reason != "agenda_empty",
        }

    complete = not protocol_issues and len(lifecycle) == 2 and lifecycle[-1]["event"] == "complete"
    reached_run = native_run is not None
    phase_reached = "post_run" if complete else ("run" if reached_run else "load")

    return {
        "schema": ORACLE_SCHEMA,
        "version": ORACLE_VERSION,
        "engine": {"name": "clips", "version": "6.30-debian"},
        "fixture": {
            "id": fixture_id,
            "nonce": nonce,
            "source_sha256": source_sha256,
            "composed_sha256": composed_sha256,
        },
        "phase_reached": phase_reached,
        "lifecycle": lifecycle,
        "run": run_state,
        "fired_rules": None,
        "facts": facts,
        "channels": [
            {"name": "t", "text": semantic_stdout},
            {"name": "stderr", "text": semantic_stderr},
        ],
        "diagnostics": diagnostics,
        "modules": modules,
        "globals": globals_observed,
        "instrumentation": {
            "harness_records": harness_records,
            "native_run_records": len(native_run_records),
        },
        "capabilities": {
            "fact_modules": True,
            "fired_rule_names": False,
            "rules_fired": True,
            "native_run_metadata": True,
        },
        "protocol_issues": protocol_issues,
    }
