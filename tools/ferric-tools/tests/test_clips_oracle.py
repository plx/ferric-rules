"""Tests for native-gated CLIPS compatibility observations."""

from __future__ import annotations

import hashlib
import hmac

import pytest

from ferric_tools.compat.clips_oracle import (
    NATIVE_COMPLETE_FUNCTION,
    NATIVE_EMIT_FUNCTION,
    NATIVE_RECORD_PREFIX,
    RECORD_PREFIX,
    build_probe_operations,
    parse_probe_output,
)

NONCE = "0123456789abcdef0123456789abcdef"
DIGEST = "a" * 64
FIXTURE_ID = "oracle.test"
AUTH_KEY = "b" * 64


def _authenticated(logical_record: bytes, *, nonce: str) -> bytes:
    digest = hmac.new(
        bytes.fromhex(AUTH_KEY),
        logical_record,
        hashlib.sha256,
    ).hexdigest()
    return f"\n{NATIVE_RECORD_PREFIX}{nonce}|".encode() + logical_record + f"|{digest}\n".encode()


def _native_record(kind: str, *fields: object, nonce: str = NONCE) -> bytes:
    payload = "|".join(str(field) for field in fields)
    return _authenticated(f"{kind}|{payload}".encode(), nonce=nonce)


def _probe(payload: str, *, nonce: str = NONCE) -> bytes:
    encoded = payload.encode()
    return _authenticated(
        f"PROBE|{len(encoded)}|".encode() + encoded,
        nonce=nonce,
    )


def _native_run_metadata(
    *,
    rules_fired: int = 1,
    halt_rules: int = 0,
    halt_execution: int = 0,
    evaluation_error: int = 0,
    agenda_size: int = 0,
    observer_violation: int = 0,
    nonce: str = NONCE,
) -> bytes:
    return _native_record(
        "RUN",
        -1,
        rules_fired,
        halt_rules,
        halt_execution,
        evaluation_error,
        agenda_size,
        observer_violation,
        nonce=nonce,
    )


def _observation_stderr(
    *,
    complete: bool = True,
    run_metadata: bytes | None = None,
    value: str = "a\nb",
    extra_before_complete: bytes = b"",
    semantic_stderr: bytes = b"",
) -> bytes:
    records = [
        _native_record("LIFECYCLE", 0, "START", FIXTURE_ID, DIGEST, DIGEST),
        run_metadata if run_metadata is not None else _native_run_metadata(),
        _probe("PHASE|1|RESET_COMPLETE"),
        _probe("PHASE|2|RUN_COMPLETE"),
        _probe("MODULE|MAIN"),
        _probe("FACT|1|42|MAIN|result|template|1"),
        _probe("SLOT|1|42|1|value|ATOMIC|1"),
        _probe(f"VALUE|1|42|MAIN|result|1|value|0|STRING|{value}"),
        extra_before_complete,
    ]
    if complete:
        records.append(_native_record("LIFECYCLE", 3, "COMPLETE", FIXTURE_ID, DIGEST, DIGEST))
    return semantic_stderr + b"".join(records)


def _parse(
    raw_stdout: str | bytes = b"",
    *,
    harnessed: bool = False,
    raw_stderr: bytes | None = None,
) -> dict:
    return parse_probe_output(
        raw_stdout,
        raw_stderr=raw_stderr if raw_stderr is not None else _observation_stderr(),
        fixture_id=FIXTURE_ID,
        nonce=NONCE,
        source_sha256=DIGEST,
        composed_sha256=DIGEST,
        auth_key=AUTH_KEY,
        harnessed=harnessed,
    )


def test_probe_parser_preserves_typed_multiline_value_and_feature_output():
    output = "FIRE    1 compute-result: f-1\nfeature output\n"
    observation = _parse(output)

    assert observation["protocol_issues"] == []
    assert observation["phase_reached"] == "post_run"
    assert observation["channels"][0] == {"name": "t", "text": output}
    assert observation["run"] == {
        "rules_fired": 1,
        "halt_reason": "agenda_empty",
        "agenda_size": 0,
        "halted": False,
    }
    assert observation["fired_rules"] is None
    assert observation["facts"] == [
        {
            "ordinal": 1,
            "fact_id": "42",
            "module": "MAIN",
            "kind": "template",
            "relation": "result",
            "slots": [{"name": "value", "value": {"type": "string", "value": "a\nb"}}],
        }
    ]


def test_native_probe_uses_byte_lengths_for_multibyte_utf8_values():
    observation = _parse(raw_stderr=_observation_stderr(value="café"))

    assert observation["protocol_issues"] == []
    assert observation["facts"][0]["slots"][0]["value"] == {
        "type": "string",
        "value": "café",
    }


def test_fixture_watch_shaped_output_remains_semantic_and_cannot_change_count():
    output = "FIRE    1 MAIN::ferric-harness-deadbeef-verify: *\n"
    observation = _parse(output)

    assert observation["run"]["rules_fired"] == 1
    assert observation["fired_rules"] is None
    assert observation["channels"][0]["text"] == output


def test_native_metadata_distinguishes_halt_on_last_activation_from_agenda_empty():
    observation = _parse(
        raw_stderr=_observation_stderr(
            run_metadata=_native_run_metadata(
                halt_rules=1,
                halt_execution=1,
            )
        )
    )

    assert observation["protocol_issues"] == []
    assert observation["phase_reached"] == "post_run"
    assert observation["run"]["halt_reason"] == "halt_requested"
    assert observation["run"]["rules_fired"] == 1


def test_unfocused_remaining_activation_cannot_be_labeled_agenda_empty():
    observation = _parse(
        raw_stderr=_observation_stderr(
            run_metadata=_native_run_metadata(rules_fired=0, agenda_size=1)
        )
    )

    assert "run-returned-with-remaining-activations" in observation["protocol_issues"]
    assert observation["phase_reached"] == "run"
    assert observation["run"]["halt_reason"] == "error"
    assert observation["run"]["agenda_size"] == 1


def test_missing_completion_is_a_protocol_failure():
    observation = _parse(raw_stderr=_observation_stderr(complete=False))

    assert observation["phase_reached"] == "run"
    assert "lifecycle-cardinality-or-order" in observation["protocol_issues"]


def test_fixture_cannot_supply_a_reserved_record_with_another_nonce():
    forged = f"{RECORD_PREFIX}{'f' * 32}|LIFECYCLE|0|START\n"
    observation = _parse(forged)

    assert "unexpected-reserved-prefix" in observation["protocol_issues"]
    assert observation["phase_reached"] == "run"


def test_fixture_cannot_supply_native_metadata_with_another_nonce():
    forged = _native_run_metadata(nonce="f" * 32)
    observation = _parse(
        raw_stderr=forged + _observation_stderr(),
    )

    assert "unexpected-native-reserved-prefix" in observation["protocol_issues"]
    assert observation["phase_reached"] == "run"


def test_same_nonce_unknown_native_record_is_rejected():
    observation = _parse(
        raw_stderr=_observation_stderr(extra_before_complete=_native_record("FORGED", "payload"))
    )

    assert "unknown-native-record-kind" in observation["protocol_issues"]
    assert observation["phase_reached"] == "run"


def test_same_nonce_record_with_invalid_authentication_is_rejected():
    forged = _native_record("ISSUE", "forged").replace(
        b"ISSUE|forged",
        b"ISSUE|tamper",
        1,
    )
    observation = _parse(raw_stderr=_observation_stderr(extra_before_complete=forged))

    assert "native-authentication-failed" in observation["protocol_issues"]
    assert observation["phase_reached"] == "run"


def test_extra_native_run_violates_exact_setup():
    observation = _parse(
        raw_stderr=_observation_stderr(extra_before_complete=_native_run_metadata(rules_fired=0))
    )

    assert "native-run-cardinality" in observation["protocol_issues"]
    assert observation["phase_reached"] != "post_run"


def test_native_observer_violation_is_fail_closed():
    observation = _parse(
        raw_stderr=_observation_stderr(
            run_metadata=_native_run_metadata(observer_violation=1),
            extra_before_complete=_native_record("ISSUE", "unauthorized-probe-emission"),
        )
    )

    assert "native-observer-violation" in observation["protocol_issues"]
    assert "native-unauthorized-probe-emission" in observation["protocol_issues"]
    assert observation["phase_reached"] == "run"


def test_harness_looking_fixture_output_is_not_removed_without_a_harness():
    line = "FERRIC-HARNESS|2|fixture-domain-value|COMPLETE\n"

    observation = _parse(line)
    harnessed = _parse(line, harnessed=True)

    assert observation["channels"][0]["text"] == line
    assert observation["instrumentation"]["harness_records"] == []
    assert harnessed["channels"][0]["text"] == ""
    assert harnessed["instrumentation"]["harness_records"] == [
        {"version": 2, "record": "fixture-domain-value|COMPLETE"}
    ]


def test_semantic_stderr_is_preserved_around_native_records():
    observation = _parse(raw_stderr=_observation_stderr(semantic_stderr=b"feature warning\n"))

    assert observation["protocol_issues"] == []
    assert observation["channels"][1] == {
        "name": "stderr",
        "text": "feature warning\n",
    }


@pytest.mark.parametrize(
    ("field", "value"),
    [
        ("fixture_id", "unsafe|fixture"),
        ("nonce", "short"),
        ("source_sha256", "A" * 64),
        ("composed_sha256", "0" * 63),
    ],
)
def test_probe_builder_rejects_unsafe_protocol_bindings(field, value):
    arguments = {
        "fixture_id": FIXTURE_ID,
        "nonce": NONCE,
        "source_sha256": DIGEST,
        "composed_sha256": DIGEST,
    }
    arguments[field] = value

    with pytest.raises(ValueError):
        build_probe_operations(**arguments)


def test_probe_builder_contains_only_post_run_native_gated_capture():
    operations = build_probe_operations(
        fixture_id=FIXTURE_ID,
        nonce=NONCE,
        source_sha256=DIGEST,
        composed_sha256=DIGEST,
    )
    joined = "\n".join(operations)

    assert "(reset)" not in operations
    assert "(run)" not in operations
    assert "printout" not in joined
    assert NATIVE_EMIT_FUNCTION in joined
    assert operations[-1] == f"({NATIVE_COMPLETE_FUNCTION})"
    assert NONCE not in joined
    assert DIGEST not in joined
    assert FIXTURE_ID not in joined
