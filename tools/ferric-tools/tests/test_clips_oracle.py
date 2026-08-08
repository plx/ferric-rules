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
    ClipsOracleProtocolError,
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


def _phase(sequence: int, phase: str, event: str, status: str | None = None) -> bytes:
    fields: tuple[object, ...] = (sequence, phase, event)
    if status is not None:
        fields += (status,)
    return _native_record("PHASE", *fields)


def _diagnostic(
    phase: str,
    message: str,
    *,
    continued: bool,
    taxonomy_version: int = 1,
) -> bytes:
    payload = message.encode()
    return _authenticated(
        (
            f"DIAGNOSTIC|{taxonomy_version}|{phase}|{int(continued)}|{len(payload)}|".encode()
            + payload
        ),
        nonce=NONCE,
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
        _phase(1, "load", "BEGIN"),
        _phase(2, "load", "END", "OK"),
        _phase(3, "reset", "BEGIN"),
        _phase(4, "reset", "END", "OK"),
        _phase(5, "run", "BEGIN"),
        _phase(6, "run", "END", "OK"),
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
    interrupted: bool = False,
    expected_phases: tuple[str, ...] | None = None,
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
        interrupted=interrupted,
        expected_phases=expected_phases,
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


def test_probe_parser_authenticates_exact_repeated_scenario_phase_sequence():
    stderr = b"".join(
        [
            _native_record("LIFECYCLE", 0, "START", FIXTURE_ID, DIGEST, DIGEST),
            _phase(1, "load", "BEGIN"),
            _phase(2, "load", "END", "OK"),
            _phase(3, "reset", "BEGIN"),
            _phase(4, "reset", "END", "OK"),
            _phase(5, "load", "BEGIN"),
            _phase(6, "load", "END", "OK"),
            _phase(7, "reset", "BEGIN"),
            _phase(8, "reset", "END", "OK"),
            _phase(9, "run", "BEGIN"),
            _phase(10, "run", "END", "OK"),
            _native_run_metadata(),
            _probe("PHASE|1|RESET_COMPLETE"),
            _probe("PHASE|2|RUN_COMPLETE"),
            _probe("MODULE|MAIN"),
            _native_record("LIFECYCLE", 3, "COMPLETE", FIXTURE_ID, DIGEST, DIGEST),
        ]
    )
    expected = ("load", "reset", "load", "reset", "run")

    observation = _parse(raw_stderr=stderr, expected_phases=expected)
    legacy = _parse(raw_stderr=stderr)

    assert observation["protocol_issues"] == []
    assert [
        phase["phase"]
        for phase in observation["instrumentation"]["native_phases"]
        if phase["event"] == "end"
    ] == list(expected)
    assert "native-phase-order" in legacy["protocol_issues"]
    assert "native-phase-terminal-path" in legacy["protocol_issues"]


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


def test_bracketed_user_output_is_not_inferred_to_be_a_diagnostic():
    output = "[EXPRNPSR3] this is fixture output, not an engine diagnostic\n"

    observation = _parse(output)

    assert observation["protocol_issues"] == []
    assert observation["diagnostics"] == []
    assert observation["channels"][0]["text"] == output

    stderr_observation = _parse(raw_stderr=_observation_stderr(semantic_stderr=output.encode()))
    assert stderr_observation["protocol_issues"] == []
    assert stderr_observation["diagnostics"] == []
    assert stderr_observation["channels"][1]["text"] == output


def test_user_werror_output_remains_semantic_on_a_successful_run():
    output = b"[USER123] fixture-selected werror output\n"

    observation = _parse(raw_stderr=_observation_stderr(semantic_stderr=output))

    assert observation["protocol_issues"] == []
    assert observation["phase_reached"] == "post_run"
    assert observation["diagnostics"] == []
    assert observation["channels"][1] == {
        "name": "stderr",
        "text": output.decode(),
    }


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
    assert "unexpected-native-reserved-prefix" in observation["diagnostic_protocol_issues"]
    assert observation["phase_reached"] == "run"


@pytest.mark.parametrize(
    "raw_stderr",
    [
        f"\n{NATIVE_RECORD_PREFIX}".encode(),
        f"\n{NATIVE_RECORD_PREFIX}{NONCE}".encode(),
    ],
)
def test_interrupted_initial_native_header_prefix_is_truncated(raw_stderr):
    observation = _parse(raw_stderr=raw_stderr, interrupted=True)

    assert "truncated-native-record" in observation["protocol_issues"]
    assert "unexpected-native-reserved-prefix" not in observation["protocol_issues"]
    assert observation["lifecycle"] == []


def test_interrupted_wrong_nonce_header_prefix_remains_protocol_corruption():
    raw_stderr = f"\n{NATIVE_RECORD_PREFIX}{'f' * 32}".encode()

    observation = _parse(raw_stderr=raw_stderr, interrupted=True)

    assert "truncated-native-record" not in observation["protocol_issues"]
    assert "unexpected-native-reserved-prefix" in observation["protocol_issues"]
    assert "unexpected-native-reserved-prefix" in observation["diagnostic_protocol_issues"]


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
    assert "native-authentication-failed" in observation["diagnostic_protocol_issues"]
    assert observation["phase_reached"] == "run"


@pytest.mark.parametrize("kind", ["PROBE", "RUN"])
def test_same_nonce_bare_record_is_authentication_corruption(kind):
    bare = f"\n{NATIVE_RECORD_PREFIX}{NONCE}|{kind}\n".encode()
    observation = _parse(
        raw_stderr=_observation_stderr(complete=False, extra_before_complete=bare),
        interrupted=True,
    )

    assert "native-authentication-malformed" in observation["protocol_issues"]
    assert "native-authentication-malformed" in observation["diagnostic_protocol_issues"]


def test_interrupted_semantic_utf8_tail_is_retained_with_replacement():
    stderr = b"".join(
        [
            _native_record("LIFECYCLE", 0, "START", FIXTURE_ID, DIGEST, DIGEST),
            _phase(1, "load", "BEGIN"),
            b"partial UTF-8: \xe2",
        ]
    )

    observation = _parse(raw_stderr=stderr, interrupted=True)

    assert observation["protocol_issues"] == []
    assert observation["channels"][1]["text"] == "partial UTF-8: \ufffd"


def test_authenticated_invalid_utf8_is_hard_protocol_corruption_when_interrupted():
    payload = b"\xff"
    invalid = _authenticated(
        f"DIAGNOSTIC|1|load|0|{len(payload)}|".encode() + payload,
        nonce=NONCE,
    )
    stderr = b"".join(
        [
            _native_record("LIFECYCLE", 0, "START", FIXTURE_ID, DIGEST, DIGEST),
            _phase(1, "load", "BEGIN"),
            invalid,
        ]
    )

    with pytest.raises(ClipsOracleProtocolError, match="DIAGNOSTIC payload"):
        _parse(raw_stderr=stderr, interrupted=True)


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


def test_diagnostic_channel_separation_uses_only_the_adjacent_authenticated_payload():
    user_output = "[EXPRNPSR3] fixture-authored output outside the diagnostic payload\n"
    message = "\n[EXPRNPSR3] Missing function declaration for missing-function.\n"
    stderr = b"".join(
        [
            _native_record("LIFECYCLE", 0, "START", FIXTURE_ID, DIGEST, DIGEST),
            _phase(1, "load", "BEGIN"),
            user_output.encode(),
            message.encode(),
            _diagnostic("load", message, continued=False),
            _phase(2, "load", "END", "ERROR"),
            _native_record("LIFECYCLE", 3, "COMPLETE", FIXTURE_ID, DIGEST, DIGEST),
        ]
    )

    observation = _parse(raw_stderr=stderr)

    assert observation["protocol_issues"] == []
    assert observation["channels"][1]["text"] == user_output


def test_mixed_user_werror_and_engine_diagnostic_fails_closed_without_dropping_output():
    user_output = "[USER123] fixture-selected werror output\n"
    engine_message = "[PRNTUTIL7] Attempt to divide by zero in / function.\n"
    payload = user_output + engine_message
    stderr = _observation_stderr().replace(
        _phase(4, "reset", "END", "OK"),
        payload.encode()
        + _diagnostic("reset", payload, continued=True)
        + _phase(4, "reset", "END", "CONTINUED"),
        1,
    )

    observation = _parse(raw_stderr=stderr)

    assert "native-diagnostic-channel-ambiguous" in observation["protocol_issues"]
    assert "native-diagnostic-channel-ambiguous" in observation["diagnostic_protocol_issues"]
    assert observation["channels"][1]["text"] == payload


def test_recognized_diagnostic_without_an_exact_raw_mirror_fails_closed():
    message = "[EXPRNPSR3] Missing function declaration for missing-function.\n"
    stderr = b"".join(
        [
            _native_record("LIFECYCLE", 0, "START", FIXTURE_ID, DIGEST, DIGEST),
            _phase(1, "load", "BEGIN"),
            _diagnostic("load", message, continued=False),
            _phase(2, "load", "END", "ERROR"),
            _native_record("LIFECYCLE", 3, "COMPLETE", FIXTURE_ID, DIGEST, DIGEST),
        ]
    )

    observation = _parse(raw_stderr=stderr)

    assert "native-diagnostic-channel-mirror" in observation["protocol_issues"]
    assert "native-diagnostic-channel-mirror" in observation["diagnostic_protocol_issues"]


def test_load_parser_diagnostic_is_a_completed_parse_failure():
    message = "\n[PRNTUTIL2] Syntax Error: Check appropriate syntax for defrule.\n"
    stderr = b"".join(
        [
            _native_record("LIFECYCLE", 0, "START", FIXTURE_ID, DIGEST, DIGEST),
            _phase(1, "load", "BEGIN"),
            message.encode(),
            _diagnostic("load", message, continued=False),
            _phase(2, "load", "END", "ERROR"),
            _native_record("LIFECYCLE", 3, "COMPLETE", FIXTURE_ID, DIGEST, DIGEST),
        ]
    )

    observation = _parse(raw_stderr=stderr)

    assert observation["protocol_issues"] == []
    assert observation["phase_reached"] == "parse"
    assert observation["run"] is None
    assert observation["diagnostics"] == [
        {
            "taxonomy_version": 1,
            "phase": "parse",
            "category": "syntax-error",
            "continued": False,
            "channel": "stderr",
            "message": message,
        }
    ]
    assert observation["channels"][1]["text"] == ""


def test_load_construct_diagnostic_is_distinct_from_parser_failure():
    message = "\n[EXPRNPSR3] Missing function declaration for missing-function.\n"
    stderr = b"".join(
        [
            _native_record("LIFECYCLE", 0, "START", FIXTURE_ID, DIGEST, DIGEST),
            _phase(1, "load", "BEGIN"),
            message.encode(),
            _diagnostic("load", message, continued=False),
            _phase(2, "load", "END", "ERROR"),
            _native_record("LIFECYCLE", 3, "COMPLETE", FIXTURE_ID, DIGEST, DIGEST),
        ]
    )

    observation = _parse(raw_stderr=stderr)

    assert observation["protocol_issues"] == []
    assert observation["phase_reached"] == "load"
    assert observation["diagnostics"][0]["category"] == "construct-error"
    assert observation["diagnostics"][0]["continued"] is False
    assert observation["diagnostics"][0]["message"] == message


def test_prntutil1_is_a_construct_companion_while_prntutil2_remains_syntax():
    message = (
        "\n[MODULPSR1] Module A does not export any constructs.\n"
        "\nERROR:\n"
        "(defmodule B\n"
        "   (import A\n"
        "[PRNTUTIL1] Unable to find defmodule B.\n"
        "\nERROR:\n"
        "(deffacts B::startup\n"
        "[PRNTUTIL1] Unable to find defmodule B.\n"
        "\nERROR:\n"
        "(defrule B::leak\n"
    )
    stderr = b"".join(
        [
            _native_record("LIFECYCLE", 0, "START", FIXTURE_ID, DIGEST, DIGEST),
            _phase(1, "load", "BEGIN"),
            message.encode(),
            _diagnostic("load", message, continued=False),
            _phase(2, "load", "END", "ERROR"),
            _native_record("LIFECYCLE", 3, "COMPLETE", FIXTURE_ID, DIGEST, DIGEST),
        ]
    )

    observation = _parse(raw_stderr=stderr)

    assert observation["protocol_issues"] == []
    assert observation["phase_reached"] == "load"
    assert observation["diagnostics"][0]["category"] == "construct-error"
    assert observation["channels"][1]["text"] == ""


def test_completed_load_diagnostic_survives_interruption_before_lifecycle_complete():
    message = "\n[EXPRNPSR3] Missing function declaration for missing-function.\n"
    stderr = b"".join(
        [
            _native_record("LIFECYCLE", 0, "START", FIXTURE_ID, DIGEST, DIGEST),
            _phase(1, "load", "BEGIN"),
            message.encode(),
            _diagnostic("load", message, continued=False),
            _phase(2, "load", "END", "ERROR"),
        ]
    )

    observation = _parse(raw_stderr=stderr)

    assert observation["protocol_issues"] == []
    assert observation["phase_reached"] == "load"
    assert observation["lifecycle"] == [
        {
            "sequence": 0,
            "event": "start",
            "fixture_id": FIXTURE_ID,
            "nonce": NONCE,
            "source_sha256": DIGEST,
            "composed_sha256": DIGEST,
        }
    ]
    assert observation["diagnostics"][0]["category"] == "construct-error"


def test_authenticated_diagnostic_survives_interruption_before_phase_end():
    message = "\n[EXPRNPSR3] Missing function declaration for missing-function.\n"
    stderr = b"".join(
        [
            _native_record("LIFECYCLE", 0, "START", FIXTURE_ID, DIGEST, DIGEST),
            _phase(1, "load", "BEGIN"),
            message.encode(),
            _diagnostic("load", message, continued=False),
        ]
    )

    observation = _parse(raw_stderr=stderr)

    assert observation["protocol_issues"] == []
    assert observation["active_phase"] == "load"
    assert observation["phase_reached"] == "load"
    assert observation["diagnostics"][0]["category"] == "construct-error"


def test_reset_diagnostic_can_continue_to_a_successful_run():
    message = "\n[PRNTUTIL7] Attempt to divide by zero in / function.\n"
    stderr = _observation_stderr().replace(
        _phase(4, "reset", "END", "OK"),
        message.encode()
        + _diagnostic("reset", message, continued=True)
        + _phase(4, "reset", "END", "CONTINUED"),
        1,
    )

    observation = _parse(raw_stderr=stderr)

    assert observation["protocol_issues"] == []
    assert observation["phase_reached"] == "post_run"
    assert observation["run"]["halt_reason"] == "agenda_empty"
    assert observation["diagnostics"] == [
        {
            "taxonomy_version": 1,
            "phase": "reset",
            "category": "evaluation-error",
            "continued": True,
            "channel": "stderr",
            "message": message,
        }
    ]
    assert observation["channels"][1]["text"] == ""


def test_reset_terminal_diagnostic_is_valid_without_run_records():
    message = "\n[PRNTUTIL7] Reset evaluation failed.\n"
    stderr = b"".join(
        [
            _native_record("LIFECYCLE", 0, "START", FIXTURE_ID, DIGEST, DIGEST),
            _phase(1, "load", "BEGIN"),
            _phase(2, "load", "END", "OK"),
            _phase(3, "reset", "BEGIN"),
            message.encode(),
            _diagnostic("reset", message, continued=False),
            _phase(4, "reset", "END", "ERROR"),
            _native_record("LIFECYCLE", 3, "COMPLETE", FIXTURE_ID, DIGEST, DIGEST),
        ]
    )

    observation = _parse(raw_stderr=stderr)

    assert observation["protocol_issues"] == []
    assert observation["phase_reached"] == "reset"
    assert observation["run"] is None
    assert observation["diagnostics"][0]["category"] == "evaluation-error"
    assert observation["diagnostics"][0]["continued"] is False


def test_run_diagnostic_is_terminal_and_retains_run_metadata():
    message = (
        "[ARGACCES5] Function + expected argument #1 to be of type integer or float\n"
        "[PRCCODE4] Execution halted during the actions of defrule first.\n"
    )
    stderr = b"".join(
        [
            _native_record("LIFECYCLE", 0, "START", FIXTURE_ID, DIGEST, DIGEST),
            _phase(1, "load", "BEGIN"),
            _phase(2, "load", "END", "OK"),
            _phase(3, "reset", "BEGIN"),
            _phase(4, "reset", "END", "OK"),
            _phase(5, "run", "BEGIN"),
            message.encode(),
            _diagnostic("run", message, continued=False),
            _phase(6, "run", "END", "ERROR"),
            _native_run_metadata(evaluation_error=1),
            _native_record("LIFECYCLE", 3, "COMPLETE", FIXTURE_ID, DIGEST, DIGEST),
        ]
    )

    observation = _parse(raw_stderr=stderr)

    assert observation["protocol_issues"] == ["post-run-state-missing"]
    assert observation["phase_reached"] == "run"
    assert observation["run"]["halt_reason"] == "error"
    assert observation["diagnostics"][0] == {
        "taxonomy_version": 1,
        "phase": "run",
        "category": "evaluation-error",
        "continued": False,
        "channel": "stderr",
        "message": message,
    }
    assert observation["channels"][1]["text"] == ""


def test_scenario_run_diagnostic_authenticates_full_post_error_snapshot():
    message = (
        "[ARGACCES5] Function + expected argument #1 to be of type integer or float\n"
        "[PRCCODE4] Execution halted during the actions of defrule first.\n"
    )
    stderr = b"".join(
        [
            _native_record("LIFECYCLE", 0, "START", FIXTURE_ID, DIGEST, DIGEST),
            _phase(1, "load", "BEGIN"),
            _phase(2, "load", "END", "OK"),
            _phase(3, "reset", "BEGIN"),
            _phase(4, "reset", "END", "OK"),
            _phase(5, "run", "BEGIN"),
            message.encode(),
            _diagnostic("run", message, continued=False),
            _phase(6, "run", "END", "ERROR"),
            _native_run_metadata(halt_execution=1, agenda_size=1),
            _probe("PHASE|1|RESET_COMPLETE"),
            _probe("PHASE|2|RUN_COMPLETE"),
            _probe("MODULE|MAIN"),
            _native_record("LIFECYCLE", 3, "COMPLETE", FIXTURE_ID, DIGEST, DIGEST),
        ]
    )

    scenario = _parse(
        raw_stderr=stderr,
        expected_phases=("load", "reset", "run"),
    )
    legacy = _parse(raw_stderr=stderr)

    assert scenario["protocol_issues"] == []
    assert scenario["phase_reached"] == "run"
    assert scenario["run"]["halt_reason"] == "error"
    assert scenario["facts"] == []
    assert scenario["modules"]["current"] == "MAIN"
    assert legacy["protocol_issues"] == [
        "post-run-state-missing",
        "probe-after-terminal-diagnostic",
    ]


def test_completed_run_diagnostic_survives_interruption_before_run_metadata():
    message = "[PRCCODE4] Execution halted during the actions of defrule first.\n"
    stderr = b"".join(
        [
            _native_record("LIFECYCLE", 0, "START", FIXTURE_ID, DIGEST, DIGEST),
            _phase(1, "load", "BEGIN"),
            _phase(2, "load", "END", "OK"),
            _phase(3, "reset", "BEGIN"),
            _phase(4, "reset", "END", "OK"),
            _phase(5, "run", "BEGIN"),
            message.encode(),
            _diagnostic("run", message, continued=False),
            _phase(6, "run", "END", "ERROR"),
        ]
    )

    observation = _parse(raw_stderr=stderr)

    assert observation["protocol_issues"] == [
        "native-run-metadata-missing",
        "post-run-state-missing",
        "native-run-diagnostic-state",
    ]
    assert observation["phase_reached"] == "run"
    assert observation["diagnostics"][0]["category"] == "evaluation-error"


def test_native_run_metadata_must_follow_run_phase_end():
    message = "[PRCCODE4] Execution halted during the actions of defrule first.\n"
    stderr = b"".join(
        [
            _native_record("LIFECYCLE", 0, "START", FIXTURE_ID, DIGEST, DIGEST),
            _native_run_metadata(evaluation_error=1),
            _phase(1, "load", "BEGIN"),
            _phase(2, "load", "END", "OK"),
            _phase(3, "reset", "BEGIN"),
            _phase(4, "reset", "END", "OK"),
            _phase(5, "run", "BEGIN"),
            message.encode(),
            _diagnostic("run", message, continued=False),
            _phase(6, "run", "END", "ERROR"),
            _native_record("LIFECYCLE", 3, "COMPLETE", FIXTURE_ID, DIGEST, DIGEST),
        ]
    )

    observation = _parse(raw_stderr=stderr)

    assert "native-run-record-order" in observation["protocol_issues"]


def test_unknown_load_diagnostic_is_retained_as_fail_closed_evidence():
    message = "\n[MYSTERY1] An unrecognized CLIPS diagnostic family.\n"
    stderr = b"".join(
        [
            _native_record("LIFECYCLE", 0, "START", FIXTURE_ID, DIGEST, DIGEST),
            _phase(1, "load", "BEGIN"),
            message.encode(),
            _diagnostic("load", message, continued=False),
            _phase(2, "load", "END", "ERROR"),
            _native_record("LIFECYCLE", 3, "COMPLETE", FIXTURE_ID, DIGEST, DIGEST),
        ]
    )

    observation = _parse(raw_stderr=stderr)

    assert observation["protocol_issues"] == []
    assert observation["phase_reached"] == "load"
    assert observation["diagnostics"][0]["phase"] == "unknown"
    assert observation["diagnostics"][0]["category"] == "unknown"
    assert observation["diagnostics"][0]["message"] == message
    assert observation["channels"][1]["text"] == message


def test_malformed_diagnostic_metadata_fails_closed():
    payload = b"[CSTRCPSR2] malformed record"
    malformed = _authenticated(
        f"DIAGNOSTIC|1|load|maybe|{len(payload)}|".encode() + payload,
        nonce=NONCE,
    )
    stderr = b"".join(
        [
            _native_record("LIFECYCLE", 0, "START", FIXTURE_ID, DIGEST, DIGEST),
            _phase(1, "load", "BEGIN"),
            malformed,
            _phase(2, "load", "END", "ERROR"),
            _native_record("LIFECYCLE", 3, "COMPLETE", FIXTURE_ID, DIGEST, DIGEST),
        ]
    )

    observation = _parse(raw_stderr=stderr)

    assert "native-diagnostic-continued" in observation["protocol_issues"]
    assert observation["phase_reached"] != "post_run"


def test_diagnostic_payload_length_mismatch_is_a_protocol_error():
    malformed = _authenticated(
        b"DIAGNOSTIC|1|load|0|999|[CSTRCPSR2] truncated",
        nonce=NONCE,
    )

    observation = _parse(raw_stderr=malformed)

    assert "truncated-native-record" in observation["protocol_issues"]
    assert observation["phase_reached"] != "post_run"


@pytest.mark.parametrize("phase", ["load", "reset", "run"])
def test_authenticated_active_phase_is_retained_for_interrupted_invocation(phase):
    phase_index = ("load", "reset", "run").index(phase)
    records = [_native_record("LIFECYCLE", 0, "START", FIXTURE_ID, DIGEST, DIGEST)]
    sequence = 1
    for completed_phase in ("load", "reset", "run")[:phase_index]:
        records.extend(
            [
                _phase(sequence, completed_phase, "BEGIN"),
                _phase(sequence + 1, completed_phase, "END", "OK"),
            ]
        )
        sequence += 2
    records.append(_phase(sequence, phase, "BEGIN"))

    observation = _parse(raw_stderr=b"".join(records))

    assert observation["protocol_issues"] == []
    assert observation["active_phase"] == phase
    assert observation["phase_reached"] == phase
    assert observation["run"] is None


def test_truncated_trailing_record_does_not_erase_authenticated_active_phase():
    authenticated_prefix = b"".join(
        [
            _native_record("LIFECYCLE", 0, "START", FIXTURE_ID, DIGEST, DIGEST),
            _phase(1, "load", "BEGIN"),
        ]
    )
    truncated = f"\n{NATIVE_RECORD_PREFIX}{NONCE}|DIAGNOSTIC|1|load|0|999|partial".encode()

    observation = _parse(raw_stderr=authenticated_prefix + truncated)

    assert observation["active_phase"] == "load"
    assert observation["phase_reached"] == "load"
    assert observation["protocol_issues"] == ["truncated-native-record"]


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
