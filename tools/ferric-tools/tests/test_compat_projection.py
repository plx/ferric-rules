"""Focused tests for engine-specific compatibility observation projection."""

from __future__ import annotations

from copy import deepcopy

import pytest

from ferric_tools.compat.oracle import EvidenceStatus, evaluate_oracle
from ferric_tools.compat.projection import (
    ObservationProjectionError,
    project_clips_observation,
    project_ferric_observation,
)

FIXTURE_ID = "ferric-oracle.empty-output-state"
SOURCE_DIGEST = "a" * 64
COMPOSED_DIGEST = "b" * 64
NONCE = "0123456789abcdef0123456789abcdef"


def _identity() -> dict[str, str]:
    return {
        "id": FIXTURE_ID,
        "nonce": NONCE,
        "source_sha256": SOURCE_DIGEST,
        "composed_sha256": COMPOSED_DIGEST,
    }


def _lifecycle(engine: str) -> list[dict[str, object]]:
    complete_sequence = 1 if engine == "ferric" else 3
    return [
        {
            "sequence": 0,
            "event": "START" if engine == "ferric" else "start",
            "fixture_id": FIXTURE_ID,
            "nonce": NONCE,
            "source_sha256": SOURCE_DIGEST,
            "composed_sha256": COMPOSED_DIGEST,
        },
        {
            "sequence": complete_sequence,
            "event": "COMPLETE" if engine == "ferric" else "complete",
            "fixture_id": FIXTURE_ID,
            "nonce": NONCE,
            "source_sha256": SOURCE_DIGEST,
            "composed_sha256": COMPOSED_DIGEST,
        },
    ]


def _raw_fact(
    engine: str,
    *,
    fact_id: object = "2",
    relation: str = "result",
    value: int = 42,
) -> dict[str, object]:
    return {
        "ordinal": 0,
        "fact_id": fact_id,
        "module": "MAIN",
        "kind": "ordered",
        "relation": relation,
        "fields": [
            {
                "type": "integer",
                "value": str(value),
            }
        ],
    }


def _raw_observation(engine: str) -> dict[str, object]:
    channels: list[dict[str, object]]
    run: dict[str, object]
    if engine == "ferric":
        channels = [
            {"name": "t", "present": False, "text": ""},
            {"name": "stderr", "present": False, "text": ""},
            {"name": "stdout", "present": False, "text": ""},
        ]
        run = {
            "rules_fired": 1,
            "fired_rule_names": None,
            "halt_reason": "agenda-empty",
            "agenda_size": 0,
            "halted": False,
        }
    else:
        channels = [
            {"name": "t", "text": ""},
            {"name": "stderr", "text": ""},
        ]
        run = {
            "rules_fired": 1,
            "halt_reason": "agenda_empty",
            "agenda_size": 0,
            "halted": False,
        }

    observation: dict[str, object] = {
        "schema": "ferric.compat-observation",
        "version": 1,
        "engine": {"name": engine, "version": "test"},
        "fixture": _identity(),
        "phase_reached": "post-run" if engine == "ferric" else "post_run",
        "lifecycle": _lifecycle(engine),
        "run": run,
        "facts": [_raw_fact(engine)],
        "channels": channels,
        "diagnostics": [],
        "modules": {
            "current": "MAIN",
            "focus": "MAIN",
            "focus_stack": ["MAIN"],
        },
        "capabilities": {
            "fact_modules": True,
            "composed_digest_verification": engine == "ferric",
            "fired_rule_names": False,
            "rules_fired": engine == "clips",
            "global_values": False,
        },
    }
    if engine == "clips":
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


def _declaration() -> dict[str, object]:
    integer = {"type": "integer", "value": 42}
    return {
        "version": 1,
        "id": FIXTURE_ID,
        "feature": "ordered fact state transition with intentionally empty output",
        "source_sha256": SOURCE_DIGEST,
        "composed_sha256": COMPOSED_DIGEST,
        "nonce": NONCE,
        "setup": ["load", "reset", "run"],
        "expectations": {
            "phase": "run-complete",
            "firings": {"count": 1, "names": None},
            "effects": [
                {
                    "name": "fact:MAIN::result",
                    "value": {"type": "multifield", "value": [integer]},
                }
            ],
            "facts": [
                {
                    "kind": "ordered",
                    "id": 0,
                    "origin": "fixture",
                    "module": "MAIN",
                    "relation": "result",
                    "fields": [integer],
                }
            ],
            "channels": {"stdout": "", "stderr": ""},
            "diagnostic": {
                "phase": "none",
                "category": "none",
                "continued": True,
            },
            "run": {"limit": None, "halt_reason": "agenda-empty"},
            "focus_stack": None,
            "globals": None,
        },
        "normalizers": ["fact-ids"],
    }


def test_seed_ordered_result_projects_to_equivalent_non_vacuous_evidence():
    ferric = project_ferric_observation(_raw_observation("ferric"), harnessed=False)
    clips = project_clips_observation(_raw_observation("clips"), harnessed=False)

    assert (
        ferric["effects"]
        == clips["effects"]
        == [
            {
                "name": "fact:MAIN::result",
                "value": {
                    "type": "multifield",
                    "value": [{"type": "integer", "value": 42}],
                },
                "origin": "fixture",
            }
        ]
    )
    assert ferric["facts"][0]["origin"] == clips["facts"][0]["origin"] == "fixture"
    assert ferric["firings"] == [{"rule": "counted-firing-1", "origin": "fixture"}]
    assert clips["firings"] == [{"rule": "counted-firing-1", "origin": "fixture"}]

    evaluation = evaluate_oracle(
        _declaration(),
        ferric,
        clips,
        expected_source_sha256=SOURCE_DIGEST,
        expected_composed_sha256=COMPOSED_DIGEST,
    )

    assert evaluation.status is EvidenceStatus.VALID
    assert evaluation.equivalent
    assert evaluation.mismatches == ()


def test_generated_clips_harness_activity_fails_when_firings_cannot_be_attributed():
    raw = _raw_observation("clips")
    raw["facts"] = [
        _raw_fact(
            "clips",
            fact_id="1",
            relation="ferric-harness-generated-state",
            value=1,
        ),
        _raw_fact("clips", fact_id="2"),
    ]
    raw["run"]["rules_fired"] = 2
    raw["channels"][0]["text"] = (
        "FERRIC-HARNESS|2|ferric-harness-deadbeef|START\n"
        "FERRIC-HARNESS|2|ferric-harness-deadbeef|STATE|focus=MAIN\n"
        "FERRIC-HARNESS|2|ferric-harness-deadbeef|COMPLETE\n"
        "feature-output\n"
    )

    with pytest.raises(
        ObservationProjectionError,
        match="cannot separate harness firings",
    ):
        project_clips_observation(raw, harnessed=True)


def test_ferric_harness_observation_fails_when_firings_cannot_be_attributed():
    with pytest.raises(
        ObservationProjectionError,
        match="cannot separate harness firings",
    ):
        project_ferric_observation(_raw_observation("ferric"), harnessed=True)


def test_harness_looking_fixture_data_is_semantic_without_a_harness():
    raw = _raw_observation("clips")
    raw["facts"] = [
        _raw_fact(
            "clips",
            relation="ferric-harness-domain-event",
            value=7,
        )
    ]
    raw["channels"][0]["text"] = "FERRIC-HARNESS|1|domain-feature|event\n"

    projected = project_clips_observation(raw, harnessed=False)

    assert projected["facts"][0]["origin"] == "fixture"
    assert projected["effects"][0]["origin"] == "fixture"
    assert projected["firings"][0]["origin"] == "fixture"
    assert projected["channels"]["stdout"] == "FERRIC-HARNESS|1|domain-feature|event\n"


@pytest.mark.parametrize("engine", ["ferric", "clips"])
def test_ambiguous_fact_ownership_fails_closed(engine):
    raw = _raw_observation(engine)
    raw["facts"][0]["module"] = None

    projector = project_ferric_observation if engine == "ferric" else project_clips_observation
    with pytest.raises(ObservationProjectionError, match="fact module is unavailable"):
        projector(raw, harnessed=False)


@pytest.mark.parametrize("engine", ["ferric", "clips"])
def test_lossy_or_duplicate_fact_identity_fails_closed(engine):
    raw = _raw_observation(engine)
    raw["facts"] = [
        _raw_fact(engine, fact_id=1.5),
        _raw_fact(engine, fact_id=1.5, relation="other"),
    ]

    projector = project_ferric_observation if engine == "ferric" else project_clips_observation
    with pytest.raises(ObservationProjectionError, match="fact id"):
        projector(raw, harnessed=False)

    raw = _raw_observation(engine)
    raw["facts"].append(deepcopy(raw["facts"][0]))
    with pytest.raises(ObservationProjectionError, match="duplicate fact id"):
        projector(raw, harnessed=False)


@pytest.mark.parametrize("engine", ["ferric", "clips"])
def test_noncanonical_integer_transport_fails_closed(engine):
    raw = _raw_observation(engine)
    raw["facts"][0]["fields"][0]["value"] = "4_2"

    projector = project_ferric_observation if engine == "ferric" else project_clips_observation
    with pytest.raises(ObservationProjectionError, match="canonical decimal text"):
        projector(raw, harnessed=False)


@pytest.mark.parametrize("engine", ["ferric", "clips"])
def test_duplicate_semantic_channels_fail_closed(engine):
    raw = _raw_observation(engine)
    raw["channels"].append(deepcopy(raw["channels"][0]))

    projector = project_ferric_observation if engine == "ferric" else project_clips_observation
    with pytest.raises(ObservationProjectionError, match="duplicate channel"):
        projector(raw, harnessed=False)


@pytest.mark.parametrize("engine", ["ferric", "clips"])
def test_wrong_raw_envelope_or_lifecycle_sequence_fails_closed(engine):
    projector = project_ferric_observation if engine == "ferric" else project_clips_observation
    raw = _raw_observation(engine)
    raw["schema"] = "fixture-controlled"
    with pytest.raises(ObservationProjectionError, match="schema"):
        projector(raw, harnessed=False)

    raw = _raw_observation(engine)
    raw["lifecycle"][1]["sequence"] = 99
    with pytest.raises(ObservationProjectionError, match="lifecycle sequence"):
        projector(raw, harnessed=False)


def test_clips_protocol_issue_fails_closed():
    raw = _raw_observation("clips")
    raw["protocol_issues"] = ["unexpected-reserved-prefix"]

    with pytest.raises(ObservationProjectionError, match="protocol violations"):
        project_clips_observation(raw, harnessed=False)


def test_unavailable_declared_ferric_capability_fails_closed():
    raw = _raw_observation("ferric")

    with pytest.raises(ObservationProjectionError, match="fired_rule_names"):
        project_ferric_observation(
            raw,
            harnessed=False,
            require_firing_names=True,
        )
    with pytest.raises(ObservationProjectionError, match="global_values"):
        project_ferric_observation(
            raw,
            harnessed=False,
            require_globals=True,
        )

    clips_raw = _raw_observation("clips")
    with pytest.raises(ObservationProjectionError, match="fired_rule_names"):
        project_clips_observation(
            clips_raw,
            harnessed=False,
            require_firing_names=True,
        )


def test_expected_diagnostic_projection_remains_invalid_until_structured_support():
    ferric_raw = _raw_observation("ferric")
    clips_raw = _raw_observation("clips")
    ferric_raw["diagnostics"] = [
        {
            "phase": "load",
            "severity": "error",
            "category": "parse-error",
            "message": "fixture diagnostic",
        }
    ]
    clips_raw["diagnostics"] = [
        {
            "phase": "unknown",
            "category": "[EXPRNPSR3]",
            "continuation": "unknown",
            "channel": "stdout",
        }
    ]

    ferric = project_ferric_observation(ferric_raw, harnessed=False)
    clips = project_clips_observation(clips_raw, harnessed=False)
    assert ferric["diagnostic"]["phase"] == clips["diagnostic"]["phase"] == "unknown"

    evaluation = evaluate_oracle(
        _declaration(),
        ferric,
        clips,
        expected_source_sha256=SOURCE_DIGEST,
        expected_composed_sha256=COMPOSED_DIGEST,
    )

    assert evaluation.status is EvidenceStatus.INVALID
    assert not evaluation.equivalent
    assert evaluation.ferric.issues[0].field == "diagnostic"
    assert evaluation.clips.issues[0].field == "diagnostic"
