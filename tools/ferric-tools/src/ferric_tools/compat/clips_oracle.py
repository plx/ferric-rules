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

from ferric_tools.compat.diagnostics import (
    DIAGNOSTIC_TAXONOMY_VERSION,
    ENGINE_DIAGNOSTIC_CATEGORIES,
    ENGINE_DIAGNOSTIC_PHASES,
    UNKNOWN_DIAGNOSTIC,
)

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
_CLIPS_DIAGNOSTIC_CODE_RE = re.compile(r"(?m)^[ \t]*\[([A-Z][A-Z0-9]*\d+)\]")
_HARNESS_LINE_RE = re.compile(r"^FERRIC-HARNESS\|(?P<version>\d+)\|(?P<body>.*)$")
_MAX_NATIVE_PAYLOAD = 16 * 1024 * 1024
_PROBE_KINDS = frozenset({"PHASE", "MODULE", "FOCUS", "FACT", "SLOT", "VALUE", "GLOBAL"})
_NATIVE_PHASES = ("load", "reset", "run")
_NATIVE_PHASE_STATUSES = frozenset({"OK", "CONTINUED", "ERROR"})
_LOAD_SYNTAX_DIAGNOSTIC_FAMILIES = frozenset({"CSTRCPSR", "SCANNER"})
_LOAD_SYNTAX_DIAGNOSTIC_CODES = frozenset({"PRNTUTIL2"})
_LOAD_CONSTRUCT_DIAGNOSTIC_FAMILIES = frozenset(
    {
        "ARGACCES",
        "CLASSPSR",
        "DFFCTPSR",
        "DFFNXPSR",
        "EXPRNPSR",
        "GENRCPSR",
        "GLOBLPSR",
        "INHERPSR",
        "MODULPSR",
        "RULEPSR",
        "TMPLTPSR",
    }
)


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
    interrupted: bool = False,
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
            end = newline + 1
            issues.append("native-authentication-malformed")
            intervals.append((start, end))
            cursor = end
            continue
        if kind_end < 0:
            issues.append("truncated-native-record")
            intervals.append((start, len(raw)))
            break
        kind = _decode_utf8(raw[kind_start:kind_end], label="native record kind")
        payload_start = kind_end + 1

        if kind in {"PROBE", "DIAGNOSTIC"}:
            framed_fields: list[bytes] = []
            length_start = payload_start
            if kind == "DIAGNOSTIC":
                metadata_truncated = False
                for _ in range(3):
                    field_end = raw.find(b"|", length_start)
                    if field_end < 0:
                        metadata_truncated = True
                        break
                    framed_fields.append(raw[length_start:field_end])
                    length_start = field_end + 1
                if metadata_truncated:
                    issues.append("truncated-native-record")
                    intervals.append((start, len(raw)))
                    break
            length_end = raw.find(b"|", length_start)
            if length_end < 0:
                issues.append("truncated-native-record")
                intervals.append((start, len(raw)))
                break
            try:
                payload_length = int(raw[length_start:length_end])
            except ValueError:
                end_of_line = raw.find(b"\n", length_end + 1)
                if end_of_line < 0:
                    issues.append("truncated-native-record")
                    intervals.append((start, len(raw)))
                    break
                end = end_of_line + 1
                issues.append("native-framing-malformed")
                intervals.append((start, end))
                cursor = end
                continue
            if payload_length < 0 or payload_length > _MAX_NATIVE_PAYLOAD:
                end_of_line = raw.find(b"\n", length_end + 1)
                if end_of_line < 0:
                    issues.append("truncated-native-record")
                    intervals.append((start, len(raw)))
                    break
                end = end_of_line + 1
                issues.append("native-framing-malformed")
                intervals.append((start, end))
                cursor = end
                continue
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
                end_of_line = raw.find(b"\n", min(value_end, len(raw)))
                if end_of_line < 0:
                    issues.append("truncated-native-record")
                    intervals.append((start, len(raw)))
                    break
                end = end_of_line + 1
                issues.append("native-framing-malformed")
                intervals.append((start, end))
                cursor = end
                continue
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
                fields = tuple(
                    _decode_utf8(field, label=f"native {kind} field") for field in framed_fields
                )
                records.append(_NativeRecord(kind, fields, raw[value_start:value_end], start, end))
        else:
            end_of_line = raw.find(b"\n", payload_start)
            if end_of_line < 0:
                issues.append("truncated-native-record")
                intervals.append((start, len(raw)))
                break
            end = end_of_line + 1
            authenticated = raw[kind_start:end_of_line]
            logical_record, separator, supplied_mac = authenticated.rpartition(b"|")
            if not separator or len(supplied_mac) != 64:
                issues.append("native-authentication-malformed")
                intervals.append((start, end))
                cursor = end
                continue
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
                    issues.append("native-record-malformed")
                    intervals.append((start, end))
                    cursor = end
                    continue
                if _decode_utf8(logical_kind, label="native logical record kind") != kind:
                    issues.append("native-record-malformed")
                    intervals.append((start, end))
                    cursor = end
                    continue
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
    semantic_stderr = (
        semantic_bytes.decode("utf-8", errors="replace")
        if interrupted
        else _decode_utf8(semantic_bytes, label="semantic stderr")
    )
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


def _load_diagnostic_taxonomy(message: str) -> tuple[str, str]:
    """Classify only allowlisted CLIPS load-diagnostic families."""
    codes = set(_CLIPS_DIAGNOSTIC_CODE_RE.findall(message))
    syntax_codes = {
        code
        for code in codes
        if code in _LOAD_SYNTAX_DIAGNOSTIC_CODES
        or any(code.startswith(family) for family in _LOAD_SYNTAX_DIAGNOSTIC_FAMILIES)
    }
    construct_codes = {
        code
        for code in codes
        if any(code.startswith(family) for family in _LOAD_CONSTRUCT_DIAGNOSTIC_FAMILIES)
    }
    if codes and syntax_codes == codes:
        return "parse", "syntax-error"
    if codes and construct_codes == codes:
        return "load", "construct-error"
    return UNKNOWN_DIAGNOSTIC, UNKNOWN_DIAGNOSTIC


def _parse_native_diagnostic(
    record: _NativeRecord,
    *,
    issues: list[str],
) -> tuple[dict, str, bool] | None:
    if len(record.fields) != 3 or record.payload is None:
        issues.append("native-diagnostic-field-count")
        return None
    version_text, native_phase, continued_text = record.fields
    if version_text != str(DIAGNOSTIC_TAXONOMY_VERSION):
        issues.append("native-diagnostic-taxonomy-version")
        return None
    if native_phase not in _NATIVE_PHASES:
        issues.append("native-diagnostic-phase")
        return None
    if continued_text not in {"0", "1"}:
        issues.append("native-diagnostic-continued")
        return None

    continued = continued_text == "1"
    message = _decode_utf8(record.payload, label="native DIAGNOSTIC payload")
    if native_phase == "load":
        phase, category = _load_diagnostic_taxonomy(message)
    else:
        phase, category = native_phase, "evaluation-error"
    if phase != UNKNOWN_DIAGNOSTIC and phase not in ENGINE_DIAGNOSTIC_PHASES:
        issues.append("native-diagnostic-canonical-phase")
        return None
    if category != UNKNOWN_DIAGNOSTIC and category not in ENGINE_DIAGNOSTIC_CATEGORIES:
        issues.append("native-diagnostic-canonical-category")
        return None
    return (
        {
            "taxonomy_version": DIAGNOSTIC_TAXONOMY_VERSION,
            "phase": phase,
            "category": category,
            "continued": continued,
            "channel": "stderr",
            "message": message,
        },
        native_phase,
        continued,
    )


def _parse_native_phase_records(
    records: list[_NativeRecord],
    *,
    diagnostic_states: list[tuple[dict, str, bool]],
    lifecycle_complete: bool,
    issues: list[str],
) -> tuple[list[dict], str | None]:
    phase_records = [record for record in records if record.kind == "PHASE"]
    phases: list[dict] = []
    active_phase: str | None = None
    next_phase_index = 0
    expected_sequence = 1
    statuses: dict[str, str] = {}

    for record in phase_records:
        if len(record.fields) not in {3, 4}:
            issues.append("native-phase-field-count")
            continue
        sequence_text, phase, event, *status_fields = record.fields
        sequence = _parse_int(
            sequence_text,
            issue="native-phase-sequence",
            issues=issues,
        )
        if sequence is None:
            continue
        if sequence != expected_sequence:
            issues.append("native-phase-sequence-order")
        expected_sequence = sequence + 1
        if phase not in _NATIVE_PHASES:
            issues.append("native-phase-name")
        status = status_fields[0] if status_fields else None

        if event == "BEGIN":
            if status is not None:
                issues.append("native-phase-begin-status")
            if active_phase is not None:
                issues.append("native-phase-overlap")
            if next_phase_index >= len(_NATIVE_PHASES) or phase != _NATIVE_PHASES[next_phase_index]:
                issues.append("native-phase-order")
            active_phase = phase
            phases.append({"sequence": sequence, "phase": phase, "event": "begin"})
        elif event == "END":
            if status not in _NATIVE_PHASE_STATUSES:
                issues.append("native-phase-status")
            if active_phase != phase:
                issues.append("native-phase-end-without-begin")
            else:
                active_phase = None
                next_phase_index += 1
            if status is not None:
                statuses[phase] = status
            phases.append(
                {
                    "sequence": sequence,
                    "phase": phase,
                    "event": "end",
                    "status": status,
                }
            )
        else:
            issues.append("native-phase-event")

    if lifecycle_complete and active_phase is not None:
        issues.append("native-phase-incomplete")

    diagnostics_by_phase: dict[str, list[bool]] = {}
    for _diagnostic, native_phase, continued in diagnostic_states:
        diagnostics_by_phase.setdefault(native_phase, []).append(continued)
    if any(len(values) != 1 for values in diagnostics_by_phase.values()):
        issues.append("native-diagnostic-cardinality")
    for phase, status in statuses.items():
        continued_values = diagnostics_by_phase.get(phase, [])
        expected_continued = status == "CONTINUED"
        if (status == "OK" and continued_values) or (
            status in {"CONTINUED", "ERROR"} and continued_values != [expected_continued]
        ):
            issues.append("native-phase-diagnostic-status")
    for phase in diagnostics_by_phase:
        if phase not in statuses and not (not lifecycle_complete and active_phase == phase):
            issues.append("native-diagnostic-outside-phase")

    return phases, active_phase


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
    interrupted: bool = False,
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
        interrupted=interrupted,
    )
    diagnostic_protocol_issues = [
        issue
        for issue in protocol_issues
        if issue
        in {
            "native-authentication-failed",
            "native-authentication-malformed",
            "native-framing-malformed",
            "native-record-malformed",
            "unexpected-native-reserved-prefix",
        }
    ]

    def add_protocol_issue(issue: str, *, diagnostic: bool = False) -> None:
        protocol_issues.append(issue)
        if diagnostic:
            diagnostic_protocol_issues.append(issue)

    allowed_native_kinds = {"LIFECYCLE", "PHASE", "RUN", "PROBE", "DIAGNOSTIC", "ISSUE"}
    unknown_native_kinds = [
        record.kind for record in native_records if record.kind not in allowed_native_kinds
    ]
    if unknown_native_kinds:
        add_protocol_issue("unknown-native-record-kind", diagnostic=True)

    for record in native_records:
        if record.kind == "ISSUE":
            if len(record.fields) != 1 or not record.fields[0]:
                add_protocol_issue("native-issue-field-count", diagnostic=True)
            else:
                add_protocol_issue(f"native-{record.fields[0]}", diagnostic=True)

    lifecycle_records = [record for record in native_records if record.kind == "LIFECYCLE"]
    lifecycle: list[dict] = []
    expected_binding = (fixture_id, source_sha256, composed_sha256)
    for record in lifecycle_records:
        if len(record.fields) != 5:
            add_protocol_issue("lifecycle-field-count", diagnostic=True)
            continue
        sequence_text, event, bound_fixture, bound_source, bound_composed = record.fields
        lifecycle_issues: list[str] = []
        sequence = _parse_int(
            sequence_text,
            issue="lifecycle-sequence",
            issues=lifecycle_issues,
        )
        protocol_issues.extend(lifecycle_issues)
        diagnostic_protocol_issues.extend(lifecycle_issues)
        if sequence is None:
            continue
        if (bound_fixture, bound_source, bound_composed) != expected_binding:
            add_protocol_issue("lifecycle-binding", diagnostic=True)
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

    lifecycle_complete = [entry["event"] for entry in lifecycle] == ["start", "complete"] and [
        entry["sequence"] for entry in lifecycle
    ] == [0, 3]
    parsed_diagnostic_records: list[tuple[_NativeRecord, tuple[dict, str, bool]]] = []
    for record in native_records:
        if record.kind != "DIAGNOSTIC":
            continue
        diagnostic_issues: list[str] = []
        state = _parse_native_diagnostic(record, issues=diagnostic_issues)
        protocol_issues.extend(diagnostic_issues)
        diagnostic_protocol_issues.extend(diagnostic_issues)
        if state is not None:
            parsed_diagnostic_records.append((record, state))
    diagnostic_states = [state for _record, state in parsed_diagnostic_records]
    diagnostics = [state[0] for state in diagnostic_states]
    run_diagnostic_present = any(state[1] == "run" for state in diagnostic_states)
    terminal_diagnostics = [state for state in diagnostic_states if not state[2]]
    if len(terminal_diagnostics) > 1:
        add_protocol_issue("native-terminal-diagnostic-cardinality", diagnostic=True)
    terminal_diagnostic = terminal_diagnostics[0] if terminal_diagnostics else None

    phase_issues: list[str] = []
    native_phases, active_phase = _parse_native_phase_records(
        native_records,
        diagnostic_states=diagnostic_states,
        lifecycle_complete=lifecycle_complete,
        issues=phase_issues,
    )
    protocol_issues.extend(phase_issues)
    diagnostic_protocol_issues.extend(phase_issues)
    if not native_phases:
        add_protocol_issue("native-phase-records-missing", diagnostic=bool(diagnostics))
    lifecycle_start_only = [entry["event"] for entry in lifecycle] == ["start"] and [
        entry["sequence"] for entry in lifecycle
    ] == [0]
    partial_active_path = active_phase is not None and lifecycle_start_only
    partial_terminal_path = (
        terminal_diagnostic is not None and active_phase is None and lifecycle_start_only
    )
    partial_path = partial_active_path or partial_terminal_path
    if not lifecycle_complete and not partial_path:
        if [entry["event"] for entry in lifecycle] != ["start", "complete"]:
            protocol_issues.append("lifecycle-cardinality-or-order")
        if lifecycle and [entry["sequence"] for entry in lifecycle] != [0, 3]:
            protocol_issues.append("lifecycle-sequence-order")
    ended_phases = [phase["phase"] for phase in native_phases if phase["event"] == "end"]
    if terminal_diagnostic is None or terminal_diagnostic[1] == "run":
        expected_ended_phases = list(_NATIVE_PHASES)
    else:
        terminal_phase_index = _NATIVE_PHASES.index(terminal_diagnostic[1])
        expected_ended_phases = list(_NATIVE_PHASES[: terminal_phase_index + 1])
    if (lifecycle_complete or partial_terminal_path) and ended_phases != expected_ended_phases:
        add_protocol_issue("native-phase-terminal-path", diagnostic=True)

    native_run_records = [record for record in native_records if record.kind == "RUN"]
    native_run: dict[str, int] | None = None
    run_expected = not partial_path and (
        terminal_diagnostic is None or terminal_diagnostic[1] == "run"
    )
    partial_terminal_run = (
        partial_path and terminal_diagnostic is not None and terminal_diagnostic[1] == "run"
    )
    if partial_terminal_run and not native_run_records:
        add_protocol_issue("native-run-metadata-missing", diagnostic=True)
    elif len(native_run_records) != (1 if run_expected or partial_terminal_run else 0):
        add_protocol_issue(
            "native-run-metadata-missing" if not native_run_records else "native-run-cardinality",
            diagnostic=run_diagnostic_present,
        )
    elif native_run_records:
        record = native_run_records[0]
        if len(record.fields) != 7:
            add_protocol_issue("native-run-field-count", diagnostic=run_diagnostic_present)
        else:
            run_issues: list[str] = []
            numeric_fields = [
                _parse_int(field, issue="native-run-numeric-field", issues=run_issues)
                for field in record.fields
            ]
            protocol_issues.extend(run_issues)
            if run_diagnostic_present:
                diagnostic_protocol_issues.extend(run_issues)
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
                    add_protocol_issue("native-run-value", diagnostic=run_diagnostic_present)
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
                        add_protocol_issue("native-observer-violation", diagnostic=True)

    record_positions = {id(record): index for index, record in enumerate(native_records)}
    if lifecycle_records:
        start_position = record_positions[id(lifecycle_records[0])]
        if start_position != 0:
            add_protocol_issue("native-record-order", diagnostic=bool(diagnostics))
    if len(lifecycle_records) == 2:
        complete_position = record_positions[id(lifecycle_records[1])]
        if complete_position != len(native_records) - 1:
            add_protocol_issue("native-record-order", diagnostic=bool(diagnostics))
    if len(lifecycle_records) == 2 and len(native_run_records) == 1:
        run_position = record_positions[id(native_run_records[0])]
        complete_position = record_positions[id(lifecycle_records[1])]
        probe_positions = [
            record_positions[id(record)] for record in native_records if record.kind == "PROBE"
        ]
        if not (all(run_position < position < complete_position for position in probe_positions)):
            protocol_issues.append("native-record-order")

    phase_boundaries: dict[str, tuple[int, int]] = {}
    for phase in _NATIVE_PHASES:
        matching = [
            record
            for record in native_records
            if record.kind == "PHASE" and len(record.fields) >= 2 and record.fields[1] == phase
        ]
        if len(matching) == 2:
            phase_boundaries[phase] = (
                record_positions[id(matching[0])],
                record_positions[id(matching[1])],
            )
    for record, (_diagnostic, native_phase, _continued) in parsed_diagnostic_records:
        boundary = phase_boundaries.get(native_phase)
        active_begin_positions = [
            record_positions[id(phase_record)]
            for phase_record in native_records
            if phase_record.kind == "PHASE"
            and len(phase_record.fields) >= 3
            and phase_record.fields[1:3] == (native_phase, "BEGIN")
        ]
        inside_completed_phase = boundary is not None and (
            boundary[0] < record_positions[id(record)] < boundary[1]
        )
        inside_interrupted_phase = (
            not lifecycle_complete
            and active_phase == native_phase
            and len(active_begin_positions) == 1
            and active_begin_positions[0] < record_positions[id(record)]
        )
        if not (inside_completed_phase or inside_interrupted_phase):
            add_protocol_issue("native-diagnostic-record-order", diagnostic=True)
    if len(native_run_records) == 1:
        run_boundary = phase_boundaries.get("run")
        run_position = record_positions[id(native_run_records[0])]
        if run_boundary is None or run_position <= run_boundary[1]:
            add_protocol_issue("native-run-record-order", diagnostic=run_diagnostic_present)

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
    probes_expected = terminal_diagnostic is None and not partial_path
    if terminal_diagnostic is not None and terminal_diagnostic[1] == "run":
        # The native observer deliberately stops after authenticated RUN
        # metadata on an evaluation failure. Empty fact/module/global
        # placeholders are not a final-state snapshot.
        protocol_issues.append("post-run-state-missing")
    if probes_expected and phase_values != [("1", "RESET_COMPLETE"), ("2", "RUN_COMPLETE")]:
        protocol_issues.append("phase-cardinality-or-order")
    elif not probes_expected and records:
        protocol_issues.append("probe-after-terminal-diagnostic")

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

    slots_by_sequence: dict[int, list[dict]] = {}
    for (fact_sequence, _slot_index), slot in slots_by_key.items():
        slots_by_sequence.setdefault(fact_sequence, []).append(slot)
    for fact_slots in slots_by_sequence.values():
        fact_slots.sort(key=lambda slot: slot["index"])

    facts: list[dict] = []
    for sequence, fact in sorted(facts_by_sequence.items()):
        fact_slots = slots_by_sequence.get(sequence, [])
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
    focus_records = [record for record in records if record.kind == "FOCUS"]
    if probes_expected:
        if len(module_records) == 1 and len(module_records[0].fields) == 1:
            modules["current"] = module_records[0].fields[0]
        else:
            protocol_issues.append("module-cardinality")
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

    semantic_stdout = (
        stdout_bytes.decode("utf-8", errors="replace")
        if interrupted
        else _decode_utf8(stdout_bytes, label="semantic stdout")
    )
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

    if (
        terminal_diagnostic is not None
        and terminal_diagnostic[1] == "run"
        and (
            native_run is None
            or not (
                native_run["evaluation_error"]
                or (native_run["halt_execution"] and not native_run["halt_rules"])
            )
        )
    ):
        add_protocol_issue("native-run-diagnostic-state", diagnostic=True)

    complete = not protocol_issues and lifecycle_complete and probes_expected
    if complete:
        phase_reached = "post_run"
    elif terminal_diagnostic is not None:
        canonical_phase = terminal_diagnostic[0]["phase"]
        phase_reached = (
            terminal_diagnostic[1] if canonical_phase == UNKNOWN_DIAGNOSTIC else canonical_phase
        )
    elif active_phase is not None:
        phase_reached = active_phase
    elif native_run is not None:
        phase_reached = "run"
    elif ended_phases:
        phase_reached = ended_phases[-1]
    else:
        phase_reached = "load"

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
        "active_phase": active_phase,
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
            "native_phases": native_phases,
            "active_phase": active_phase,
        },
        "capabilities": {
            "fact_modules": True,
            "fired_rule_names": False,
            "rules_fired": True,
            "native_run_metadata": True,
        },
        "protocol_issues": protocol_issues,
        "diagnostic_protocol_issues": diagnostic_protocol_issues,
    }
