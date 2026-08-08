"""Focused tests for engine-specific compatibility observation projection."""

from __future__ import annotations

from copy import deepcopy

import pytest

from ferric_tools.compat.oracle import EvidenceStatus, evaluate_oracle
from ferric_tools.compat.projection import (
    ObservationProjectionError,
    project_clips_observation,
    project_ferric_observation,
    project_observation_diagnostic,
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


def _raw_diagnostic(
    engine: str,
    *,
    phase: str,
    category: str,
    continued: bool,
    message: str = "fixture diagnostic",
) -> dict[str, object]:
    diagnostic: dict[str, object] = {
        "taxonomy_version": 1,
        "phase": phase,
        "category": category,
        "continued": continued,
        "message": message,
    }
    if engine == "ferric":
        diagnostic["severity"] = "warning" if continued else "error"
    else:
        diagnostic["channel"] = "stderr"
    return diagnostic


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


def test_action_error_halt_reason_is_canonical_across_adapters():
    ferric_raw = _raw_observation("ferric")
    ferric_raw["run"]["halt_reason"] = "action-error"
    ferric_raw["phase_reached"] = "run"
    ferric_raw["diagnostics"] = [
        _raw_diagnostic(
            "ferric",
            phase="run",
            category="evaluation-error",
            continued=False,
        )
    ]
    clips_raw = _raw_observation("clips")
    clips_raw["run"]["halt_reason"] = "error"
    clips_raw["phase_reached"] = "run"
    clips_raw["diagnostics"] = [
        _raw_diagnostic(
            "clips",
            phase="run",
            category="evaluation-error",
            continued=False,
        )
    ]

    ferric = project_ferric_observation(ferric_raw, harnessed=False)
    clips = project_clips_observation(clips_raw, harnessed=False)

    assert ferric["run"]["halt_reason"] == clips["run"]["halt_reason"] == "action-error"
    assert clips["firings"] == [{"rule": "counted-firing-1", "origin": "fixture"}]

    declaration = _declaration()
    declaration["expectations"]["phase"] = "run"
    declaration["expectations"]["diagnostic"] = {
        "phase": "run",
        "category": "evaluation-error",
        "continued": False,
    }
    declaration["expectations"]["run"]["halt_reason"] = "action-error"
    evaluation = evaluate_oracle(
        declaration,
        ferric,
        clips,
        expected_source_sha256=SOURCE_DIGEST,
        expected_composed_sha256=COMPOSED_DIGEST,
    )

    assert evaluation.status is EvidenceStatus.VALID
    assert evaluation.equivalent
    assert evaluation.mismatches == ()


@pytest.mark.parametrize("engine", ["ferric", "clips"])
def test_terminal_run_diagnostic_requires_action_error_halt(engine):
    raw = _raw_observation(engine)
    raw["phase_reached"] = "run"
    raw["diagnostics"] = [
        _raw_diagnostic(
            engine,
            phase="run",
            category="evaluation-error",
            continued=False,
        )
    ]
    projector = project_ferric_observation if engine == "ferric" else project_clips_observation

    with pytest.raises(ObservationProjectionError, match="lacks an action-error halt"):
        projector(raw, harnessed=False)


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
    with pytest.raises(ObservationProjectionError, match="protocol violations"):
        project_observation_diagnostic(
            raw,
            engine="clips",
            expected_fixture=_identity(),
        )


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


def test_multi_module_ferric_without_facts_does_not_require_fact_ownership_capability():
    raw = _raw_observation("ferric")
    raw["facts"] = []
    raw["capabilities"]["fact_modules"] = False
    raw["modules"] = {
        "current": "SECONDARY",
        "focus": "MAIN",
        "focus_stack": ["MAIN", "SECONDARY"],
    }

    projected = project_ferric_observation(raw, harnessed=False)

    assert projected["facts"] == []


def test_ferric_with_any_fact_still_requires_authenticated_module_ownership():
    raw = _raw_observation("ferric")
    raw["capabilities"]["fact_modules"] = False

    with pytest.raises(ObservationProjectionError, match="fact_modules"):
        project_ferric_observation(raw, harnessed=False)


@pytest.mark.parametrize("engine", ["ferric", "clips"])
@pytest.mark.parametrize(
    ("phase", "category"),
    [
        ("parse", "syntax-error"),
        ("load", "construct-error"),
        ("reset", "evaluation-error"),
    ],
)
def test_completed_pre_run_diagnostic_projects_without_partial_state(
    engine,
    phase,
    category,
):
    raw = _raw_observation(engine)
    raw["phase_reached"] = phase
    raw["run"] = None
    raw["diagnostics"] = [
        _raw_diagnostic(
            engine,
            phase=phase,
            category=category,
            continued=False,
        )
    ]
    raw["channels"][0]["text"] = "diagnostic output\n"
    if engine == "ferric":
        raw["channels"][0]["present"] = True

    projector = project_ferric_observation if engine == "ferric" else project_clips_observation
    projected = projector(raw, harnessed=False)

    assert projected["phase"] == phase
    assert projected["diagnostic"] == {
        "phase": phase,
        "category": category,
        "continued": False,
    }
    assert projected["run"] == {"limit": None, "halt_reason": "not-run"}
    assert projected["firings"] == []
    assert projected["effects"] == [
        {
            "name": "channel:stdout",
            "value": {"type": "string", "value": "diagnostic output\n"},
            "origin": "fixture",
        }
    ]
    assert projected["facts"] == []
    assert projected["channels"]["stdout"] == "diagnostic output\n"
    assert [marker["kind"] for marker in projected["markers"]] == ["START", "COMPLETE"]


@pytest.mark.parametrize("engine", ["ferric", "clips"])
def test_homogeneous_diagnostics_collapse_but_heterogeneous_diagnostics_fail(engine):
    raw = _raw_observation(engine)
    raw["diagnostics"] = [
        _raw_diagnostic(
            engine,
            phase="load",
            category="construct-error",
            continued=True,
            message="first construct diagnostic",
        ),
        _raw_diagnostic(
            engine,
            phase="load",
            category="construct-error",
            continued=True,
            message="second construct diagnostic",
        ),
    ]
    projector = project_ferric_observation if engine == "ferric" else project_clips_observation

    projected = projector(raw, harnessed=False)
    assert projected["diagnostic"] == {
        "phase": "load",
        "category": "construct-error",
        "continued": True,
    }

    raw["diagnostics"][1] = _raw_diagnostic(
        engine,
        phase="run",
        category="evaluation-error",
        continued=True,
    )
    with pytest.raises(ObservationProjectionError, match="heterogeneous"):
        projector(raw, harnessed=False)


@pytest.mark.parametrize("engine", ["ferric", "clips"])
@pytest.mark.parametrize(
    ("field", "value", "message"),
    [
        ("taxonomy_version", 2, "taxonomy version"),
        ("phase", "unknown", "phase is unsupported"),
        ("category", "unknown", "category is unsupported"),
        ("category", "evaluation-error", "phase/category pair"),
        ("continued", "unknown", "continuation is malformed"),
        ("message", 7, "message is malformed"),
    ],
)
def test_unknown_or_malformed_diagnostic_taxonomy_fails_closed(
    engine,
    field,
    value,
    message,
):
    raw = _raw_observation(engine)
    diagnostic = _raw_diagnostic(
        engine,
        phase="parse",
        category="syntax-error",
        continued=True,
    )
    diagnostic[field] = value
    raw["diagnostics"] = [diagnostic]
    projector = project_ferric_observation if engine == "ferric" else project_clips_observation

    with pytest.raises(ObservationProjectionError, match=message):
        projector(raw, harnessed=False)


@pytest.mark.parametrize(
    ("engine", "field", "value", "message"),
    [
        ("ferric", "severity", "fatal", "severity is unsupported"),
        ("clips", "channel", "t", "channel is unsupported"),
    ],
)
def test_engine_specific_diagnostic_metadata_fails_closed(engine, field, value, message):
    raw = _raw_observation(engine)
    diagnostic = _raw_diagnostic(
        engine,
        phase="run",
        category="evaluation-error",
        continued=False,
    )
    diagnostic[field] = value
    raw["diagnostics"] = [diagnostic]
    projector = project_ferric_observation if engine == "ferric" else project_clips_observation

    with pytest.raises(ObservationProjectionError, match=message):
        projector(raw, harnessed=False)


@pytest.mark.parametrize("engine", ["ferric", "clips"])
def test_harness_diagnostic_is_not_projected_as_semantic_evidence(engine):
    raw = _raw_observation(engine)
    raw["phase_reached"] = "load"
    raw["run"] = None
    raw["diagnostics"] = [
        _raw_diagnostic(
            engine,
            phase="harness",
            category="harness-error",
            continued=False,
        )
    ]
    projector = project_ferric_observation if engine == "ferric" else project_clips_observation

    with pytest.raises(ObservationProjectionError, match="phase is unsupported"):
        projector(raw, harnessed=False)


@pytest.mark.parametrize("engine", ["ferric", "clips"])
def test_bound_diagnostic_subset_survives_unrelated_projection_failure(engine):
    raw = _raw_observation(engine)
    raw["phase_reached"] = "parse"
    raw["run"] = None
    raw["facts"] = "malformed partial state"
    raw["diagnostics"] = [
        _raw_diagnostic(
            engine,
            phase="parse",
            category="syntax-error",
            continued=False,
        )
    ]
    projector = project_ferric_observation if engine == "ferric" else project_clips_observation

    assert project_observation_diagnostic(
        raw,
        engine=engine,
        expected_fixture=_identity(),
    ) == {
        "phase": "parse",
        "category": "syntax-error",
        "continued": False,
    }
    with pytest.raises(ObservationProjectionError, match="facts are malformed"):
        projector(raw, harnessed=False)


@pytest.mark.parametrize("engine", ["ferric", "clips"])
def test_diagnostic_subset_rejects_wrong_invocation_binding(engine):
    raw = _raw_observation(engine)
    raw["diagnostics"] = [
        _raw_diagnostic(
            engine,
            phase="load",
            category="construct-error",
            continued=True,
        )
    ]
    expected_fixture = _identity()
    expected_fixture["nonce"] = "f" * 32

    with pytest.raises(ObservationProjectionError, match=r"nonce.*does not match"):
        project_observation_diagnostic(
            raw,
            engine=engine,
            expected_fixture=expected_fixture,
        )


@pytest.mark.parametrize("engine", ["ferric", "clips"])
def test_diagnostic_subset_preserves_unknown_evidence_for_fail_closed_classification(engine):
    raw = _raw_observation(engine)
    raw["diagnostics"] = [
        _raw_diagnostic(
            engine,
            phase="unknown",
            category="unknown",
            continued=False,
        )
    ]

    assert project_observation_diagnostic(
        raw,
        engine=engine,
        expected_fixture=_identity(),
    ) == {"phase": "unknown", "category": "unknown", "continued": False}


@pytest.mark.parametrize("engine", ["ferric", "clips"])
def test_diagnostic_subset_rejects_harness_evidence(engine):
    raw = _raw_observation(engine)
    raw["diagnostics"] = [
        _raw_diagnostic(
            engine,
            phase="harness",
            category="harness-error",
            continued=False,
        )
    ]

    with pytest.raises(ObservationProjectionError, match="phase is unsupported"):
        project_observation_diagnostic(raw, engine=engine, expected_fixture=_identity())


@pytest.mark.parametrize("engine", ["ferric", "clips"])
def test_diagnostic_subset_rejects_spoofed_lifecycle_binding(engine):
    raw = _raw_observation(engine)
    raw["lifecycle"][0]["nonce"] = "f" * 32
    raw["diagnostics"] = [
        _raw_diagnostic(
            engine,
            phase="run",
            category="evaluation-error",
            continued=False,
        )
    ]

    with pytest.raises(ObservationProjectionError, match=r"lifecycle marker 0.*nonce"):
        project_observation_diagnostic(raw, engine=engine, expected_fixture=_identity())


def test_interrupted_run_diagnostic_rejects_contradictory_native_run_state():
    raw = _raw_observation("clips")
    raw["lifecycle"] = raw["lifecycle"][:1]
    raw["phase_reached"] = "run"
    raw["diagnostics"] = [
        _raw_diagnostic(
            "clips",
            phase="run",
            category="evaluation-error",
            continued=False,
        )
    ]
    raw["protocol_issues"] = ["native-run-diagnostic-state"]

    with pytest.raises(ObservationProjectionError, match="native-run-diagnostic-state"):
        project_observation_diagnostic(
            raw,
            engine="clips",
            expected_fixture=_identity(),
            interrupted=True,
        )

    raw["protocol_issues"].append("native-run-metadata-missing")
    raw["run"] = None
    assert project_observation_diagnostic(
        raw,
        engine="clips",
        expected_fixture=_identity(),
        interrupted=True,
    ) == {"phase": "run", "category": "evaluation-error", "continued": False}


def test_interrupted_diagnostic_subset_ignores_only_unrelated_partial_probe_tail():
    raw = _raw_observation("clips")
    raw["lifecycle"] = raw["lifecycle"][:1]
    raw["phase_reached"] = "run"
    raw["diagnostics"] = [
        _raw_diagnostic(
            "clips",
            phase="reset",
            category="evaluation-error",
            continued=True,
        )
    ]
    raw["protocol_issues"] = [
        "lifecycle-cardinality-or-order",
        "fact-slot-count",
        "slot-item-positions",
    ]
    raw["diagnostic_protocol_issues"] = []

    assert project_observation_diagnostic(
        raw,
        engine="clips",
        expected_fixture=_identity(),
        interrupted=True,
    ) == {"phase": "reset", "category": "evaluation-error", "continued": True}


def test_interrupted_terminal_diagnostic_is_trusted_before_phase_end():
    raw = _raw_observation("clips")
    raw["lifecycle"] = raw["lifecycle"][:1]
    raw["phase_reached"] = "load"
    raw["run"] = None
    raw["diagnostics"] = [
        _raw_diagnostic(
            "clips",
            phase="load",
            category="construct-error",
            continued=False,
        )
    ]
    raw["protocol_issues"] = []
    raw["diagnostic_protocol_issues"] = []

    assert project_observation_diagnostic(
        raw,
        engine="clips",
        expected_fixture=_identity(),
        interrupted=True,
    ) == {"phase": "load", "category": "construct-error", "continued": False}


def test_terminal_run_diagnostic_without_post_run_state_is_not_complete_oracle():
    raw = _raw_observation("clips")
    raw["phase_reached"] = "run"
    raw["run"]["halt_reason"] = "error"
    raw["diagnostics"] = [
        _raw_diagnostic(
            "clips",
            phase="run",
            category="evaluation-error",
            continued=False,
        )
    ]
    raw["protocol_issues"] = ["post-run-state-missing"]
    raw["diagnostic_protocol_issues"] = []

    assert project_observation_diagnostic(
        raw,
        engine="clips",
        expected_fixture=_identity(),
    ) == {"phase": "run", "category": "evaluation-error", "continued": False}
    with pytest.raises(ObservationProjectionError, match="post-run-state-missing"):
        project_clips_observation(raw, harnessed=False)


def test_ferric_and_clips_diagnostic_phase_category_mismatch_remains_divergent():
    ferric_raw = _raw_observation("ferric")
    ferric_raw["diagnostics"] = [
        _raw_diagnostic(
            "ferric",
            phase="parse",
            category="syntax-error",
            continued=True,
        )
    ]
    clips_raw = _raw_observation("clips")
    clips_raw["diagnostics"] = [
        _raw_diagnostic(
            "clips",
            phase="load",
            category="construct-error",
            continued=True,
        )
    ]

    ferric = project_ferric_observation(ferric_raw, harnessed=False)
    clips = project_clips_observation(clips_raw, harnessed=False)
    declaration = _declaration()
    declaration["expectations"]["diagnostic"] = {
        "phase": "parse",
        "category": "syntax-error",
        "continued": True,
    }
    evaluation = evaluate_oracle(
        declaration,
        ferric,
        clips,
        expected_source_sha256=SOURCE_DIGEST,
        expected_composed_sha256=COMPOSED_DIGEST,
    )

    mismatch_fields = {(mismatch.scope, mismatch.field) for mismatch in evaluation.mismatches}
    assert evaluation.status is EvidenceStatus.VALID
    assert not evaluation.equivalent
    assert ("engines", "diagnostic.phase") in mismatch_fields
    assert ("engines", "diagnostic.category") in mismatch_fields
