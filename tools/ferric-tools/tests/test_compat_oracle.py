"""Pure contract and anti-vacuity tests for compatibility oracles."""

from __future__ import annotations

from copy import deepcopy

import pytest

from ferric_tools.compat.oracle import (
    EvidenceStatus,
    evaluate_oracle,
    evaluation_to_dict,
    validate_declaration,
    validate_observation,
)

SOURCE_DIGEST = "a" * 64
OTHER_DIGEST = "b" * 64
COMPOSED_DIGEST = "c" * 64
FIXTURE_ID = "rules.salience.orders-high-first"
NONCE = "0123456789abcdef0123456789abcdef"


def _symbol(value: str) -> dict[str, object]:
    return {"type": "symbol", "value": value}


def _float(value: str) -> dict[str, object]:
    return {"type": "float", "value": value}


def _ordered_fact(
    *,
    fact_id: int = 1,
    value: str = "done",
    relation: str = "result",
) -> dict[str, object]:
    return {
        "kind": "ordered",
        "id": fact_id,
        "origin": "fixture",
        "module": "MAIN",
        "relation": relation,
        "fields": [_symbol(value)],
    }


def _declaration(
    *,
    facts: list[dict[str, object]] | None = None,
    stdout: str = "feature-fired\n",
    normalizers: list[str] | None = None,
) -> dict[str, object]:
    return {
        "version": 1,
        "id": FIXTURE_ID,
        "feature": "rules.salience",
        "source_sha256": SOURCE_DIGEST,
        "composed_sha256": COMPOSED_DIGEST,
        "nonce": NONCE,
        "setup": ["load", "reset", "run"],
        "expectations": {
            "phase": "run-complete",
            "firings": {
                "count": 1,
                "names": ["MAIN::high-priority"],
            },
            "effects": [
                {
                    "name": "priority-result",
                    "value": _symbol("high-first"),
                }
            ],
            "facts": deepcopy(facts if facts is not None else [_ordered_fact()]),
            "channels": {
                "stdout": stdout,
                "stderr": "",
            },
            "diagnostic": {
                "phase": "none",
                "category": "none",
                "continued": True,
            },
            "run": {
                "limit": None,
                "halt_reason": "agenda-empty",
            },
            "focus_stack": None,
            "globals": None,
        },
        "normalizers": list(normalizers or []),
    }


def _markers() -> list[dict[str, object]]:
    return [
        {
            "kind": "START",
            "id": FIXTURE_ID,
            "source_sha256": SOURCE_DIGEST,
            "composed_sha256": COMPOSED_DIGEST,
            "nonce": NONCE,
        },
        {
            "kind": "COMPLETE",
            "id": FIXTURE_ID,
            "source_sha256": SOURCE_DIGEST,
            "composed_sha256": COMPOSED_DIGEST,
            "nonce": NONCE,
        },
    ]


def _observation(
    *,
    facts: list[dict[str, object]] | None = None,
    stdout: str = "feature-fired\n",
) -> dict[str, object]:
    return {
        "version": 1,
        "id": FIXTURE_ID,
        "source_sha256": SOURCE_DIGEST,
        "composed_sha256": COMPOSED_DIGEST,
        "nonce": NONCE,
        "markers": _markers(),
        "phase": "run-complete",
        "firings": [
            {
                "rule": "MAIN::high-priority",
                "origin": "fixture",
            }
        ],
        "effects": [
            {
                "name": "priority-result",
                "value": _symbol("high-first"),
                "origin": "fixture",
            }
        ],
        "facts": deepcopy(facts if facts is not None else [_ordered_fact()]),
        "channels": {
            "stdout": stdout,
            "stderr": "",
        },
        "diagnostic": {
            "phase": "none",
            "category": "none",
            "continued": True,
        },
        "run": {
            "limit": None,
            "halt_reason": "agenda-empty",
        },
        "focus_stack": None,
        "globals": None,
    }


def _evaluate(
    declaration: object | None,
    ferric: object | None,
    clips: object | None,
):
    return evaluate_oracle(
        declaration,
        ferric,
        clips,
        expected_source_sha256=SOURCE_DIGEST,
        expected_composed_sha256=COMPOSED_DIGEST,
    )


def _mismatch_fields(result) -> set[tuple[str, str]]:
    return {(mismatch.scope, mismatch.field) for mismatch in result.mismatches}


def test_valid_non_vacuous_observations_are_equivalent():
    result = _evaluate(_declaration(), _observation(), _observation())

    assert result.status is EvidenceStatus.VALID
    assert result.equivalent
    assert result.mismatches == ()


def test_missing_declaration_is_represented_without_an_exception():
    result = _evaluate(None, _observation(), _observation())

    assert result.status is EvidenceStatus.MISSING
    assert not result.equivalent
    assert result.declaration.status is EvidenceStatus.MISSING
    assert evaluation_to_dict(result)["status"] == "missing"


@pytest.mark.parametrize(("ferric", "clips"), [(None, _observation()), (_observation(), None)])
def test_missing_engine_observation_is_invalid_not_missing(ferric, clips):
    result = _evaluate(_declaration(), ferric, clips)

    assert result.status is EvidenceStatus.INVALID
    assert not result.equivalent
    missing_engine = result.ferric if ferric is None else result.clips
    assert missing_engine.status is EvidenceStatus.INVALID
    assert missing_engine.issues[0].message == "engine observation is missing"


@pytest.mark.parametrize("target", ["declaration", "observation"])
def test_unknown_fields_fail_strict_validation(target):
    declaration = _declaration()
    observation = _observation()
    if target == "declaration":
        declaration["surprise"] = True
        evidence = validate_declaration(
            declaration,
            expected_source_sha256=SOURCE_DIGEST,
            expected_composed_sha256=COMPOSED_DIGEST,
        )
    else:
        observation["surprise"] = True
        declaration_evidence = validate_declaration(
            declaration,
            expected_source_sha256=SOURCE_DIGEST,
            expected_composed_sha256=COMPOSED_DIGEST,
        )
        assert declaration_evidence.value is not None
        evidence = validate_observation(
            observation,
            declaration=declaration_evidence.value,
        )

    assert evidence.status is EvidenceStatus.INVALID
    assert evidence.issues[0].field == "$"
    assert "unknown fields" in evidence.issues[0].message


@pytest.mark.parametrize("target", ["declaration", "observation"])
def test_declaration_and_observation_versions_are_independently_strict(target):
    declaration = _declaration()
    observation = _observation()
    if target == "declaration":
        declaration["version"] = 2
        evidence = validate_declaration(
            declaration,
            expected_source_sha256=SOURCE_DIGEST,
            expected_composed_sha256=COMPOSED_DIGEST,
        )
    else:
        observation["version"] = 2
        declaration_evidence = validate_declaration(
            declaration,
            expected_source_sha256=SOURCE_DIGEST,
            expected_composed_sha256=COMPOSED_DIGEST,
        )
        assert declaration_evidence.value is not None
        evidence = validate_observation(
            observation,
            declaration=declaration_evidence.value,
        )

    assert evidence.status is EvidenceStatus.INVALID
    assert evidence.issues[0].field == "version"


@pytest.mark.parametrize("target", ["declaration", "observation"])
@pytest.mark.parametrize(
    ("phase", "category", "continued", "issue_suffix"),
    [
        ("unknown", "unknown", False, ""),
        ("process", "timeout", False, ""),
        ("harness", "harness-error", False, ""),
        ("run", "syntax-error", False, ""),
        ("parse", "none", False, ""),
        ("none", "none", False, ".continued"),
    ],
)
def test_diagnostic_taxonomy_fails_closed_for_declarations_and_observations(
    target,
    phase,
    category,
    continued,
    issue_suffix,
):
    declaration = _declaration()
    observation = _observation()
    if target == "declaration":
        diagnostic = declaration["expectations"]["diagnostic"]
        field = "expectations.diagnostic"
    else:
        diagnostic = observation["diagnostic"]
        field = "diagnostic"
    diagnostic.update(
        {
            "phase": phase,
            "category": category,
            "continued": continued,
        }
    )

    declaration_evidence = validate_declaration(
        declaration,
        expected_source_sha256=SOURCE_DIGEST,
        expected_composed_sha256=COMPOSED_DIGEST,
    )
    if target == "declaration":
        evidence = declaration_evidence
    else:
        assert declaration_evidence.value is not None
        evidence = validate_observation(
            observation,
            declaration=declaration_evidence.value,
        )

    assert evidence.status is EvidenceStatus.INVALID
    assert evidence.issues[0].field == f"{field}{issue_suffix}"


def test_unknown_normalizer_fails_closed():
    declaration = _declaration(normalizers=["whitespace"])

    evidence = validate_declaration(
        declaration,
        expected_source_sha256=SOURCE_DIGEST,
        expected_composed_sha256=COMPOSED_DIGEST,
    )

    assert evidence.status is EvidenceStatus.INVALID
    assert evidence.issues[0].field == "normalizers"
    assert "unsupported normalizers" in evidence.issues[0].message


def test_v1_declaration_requires_protocol_safe_id_and_exact_setup():
    unsafe_id = _declaration()
    unsafe_id["id"] = "fixture with spaces"
    unsupported_setup = _declaration()
    unsupported_setup["setup"] = ["load", "run"]

    id_evidence = validate_declaration(
        unsafe_id,
        expected_source_sha256=SOURCE_DIGEST,
        expected_composed_sha256=COMPOSED_DIGEST,
    )
    setup_evidence = validate_declaration(
        unsupported_setup,
        expected_source_sha256=SOURCE_DIGEST,
        expected_composed_sha256=COMPOSED_DIGEST,
    )

    assert id_evidence.status is EvidenceStatus.INVALID
    assert id_evidence.issues[0].field == "id"
    assert setup_evidence.status is EvidenceStatus.INVALID
    assert setup_evidence.issues[0].field == "setup"


def test_v1_declaration_rejects_contradictory_firing_count_and_names():
    declaration = _declaration()
    declaration["expectations"]["firings"] = {
        "count": 2,
        "names": ["MAIN::only-one"],
    }

    evidence = validate_declaration(
        declaration,
        expected_source_sha256=SOURCE_DIGEST,
        expected_composed_sha256=COMPOSED_DIGEST,
    )

    assert evidence.status is EvidenceStatus.INVALID
    assert evidence.issues[0].field == "expectations.firings"
    assert "count must equal" in evidence.issues[0].message


@pytest.mark.parametrize(
    ("field", "value", "issue_field"),
    [
        ("limit", 1, "expectations.run.limit"),
        ("halt_reason", "limit-reached", "expectations.run.halt_reason"),
    ],
)
def test_v1_declaration_rejects_unsupported_run_configuration(field, value, issue_field):
    declaration = _declaration()
    declaration["expectations"]["run"][field] = value

    evidence = validate_declaration(
        declaration,
        expected_source_sha256=SOURCE_DIGEST,
        expected_composed_sha256=COMPOSED_DIGEST,
    )

    assert evidence.status is EvidenceStatus.INVALID
    assert evidence.issues[0].field == issue_field


def test_v1_declaration_accepts_native_observable_halt_requested_completion():
    declaration = _declaration()
    declaration["expectations"]["run"]["halt_reason"] = "halt-requested"

    evidence = validate_declaration(
        declaration,
        expected_source_sha256=SOURCE_DIGEST,
        expected_composed_sha256=COMPOSED_DIGEST,
    )

    assert evidence.status is EvidenceStatus.VALID


def test_v1_declaration_accepts_action_error_completion():
    declaration = _declaration()
    declaration["expectations"]["run"]["halt_reason"] = "action-error"

    evidence = validate_declaration(
        declaration,
        expected_source_sha256=SOURCE_DIGEST,
        expected_composed_sha256=COMPOSED_DIGEST,
    )

    assert evidence.status is EvidenceStatus.VALID


def test_v1_declaration_rejects_channels_the_adapters_cannot_capture():
    declaration = _declaration()
    declaration["expectations"]["channels"]["wtrace"] = ""

    evidence = validate_declaration(
        declaration,
        expected_source_sha256=SOURCE_DIGEST,
        expected_composed_sha256=COMPOSED_DIGEST,
    )

    assert evidence.status is EvidenceStatus.INVALID
    assert evidence.issues[0].field == "expectations.channels"


def test_unknown_typed_value_fails_closed():
    declaration = _declaration()
    declaration["expectations"]["effects"][0]["value"]["type"] = "number"

    evidence = validate_declaration(
        declaration,
        expected_source_sha256=SOURCE_DIGEST,
        expected_composed_sha256=COMPOSED_DIGEST,
    )

    assert evidence.status is EvidenceStatus.INVALID
    assert evidence.issues[0].field.endswith(".type")


@pytest.mark.parametrize(
    "nonce",
    [
        "a" * 30,
        "a" * 33,
        "A" * 32,
        "g" * 32,
        "a" * 130,
    ],
)
def test_nonce_requires_16_to_64_lowercase_hex_bytes(nonce):
    declaration = _declaration()
    declaration["nonce"] = nonce

    evidence = validate_declaration(
        declaration,
        expected_source_sha256=SOURCE_DIGEST,
        expected_composed_sha256=COMPOSED_DIGEST,
    )

    assert evidence.status is EvidenceStatus.INVALID
    assert evidence.issues[0].field == "nonce"


# Issue #91 red case: equal output but different final facts.
def test_equal_output_with_different_final_facts_is_not_equivalent():
    ferric = _observation()
    clips = _observation(facts=[_ordered_fact(value="wrong")])

    result = _evaluate(_declaration(), ferric, clips)

    assert result.status is EvidenceStatus.VALID
    assert not result.equivalent
    assert ("clips", "facts") in _mismatch_fields(result)
    assert ("engines", "facts") in _mismatch_fields(result)


# Issue #91 red case: a verifier firing is not the feature-specific effect.
def test_instrumentation_only_noop_cannot_satisfy_fixture_effect_or_firing():
    ferric = _observation()
    clips = _observation()
    for observation in (ferric, clips):
        observation["firings"][0]["origin"] = "instrumentation"
        observation["effects"][0]["origin"] = "instrumentation"

    result = _evaluate(_declaration(), ferric, clips)

    assert result.status is EvidenceStatus.VALID
    assert not result.equivalent
    assert ("ferric", "firings.count") in _mismatch_fields(result)
    assert ("ferric", "firings.names") in _mismatch_fields(result)
    assert ("ferric", "effects") in _mismatch_fields(result)
    assert ("clips", "effects") in _mismatch_fields(result)


# Issue #91 red case: completion proof must exist exactly once.
def test_missing_completion_marker_is_invalid_evidence():
    ferric = _observation()
    ferric["markers"] = ferric["markers"][:1]

    result = _evaluate(_declaration(), ferric, _observation())

    assert result.status is EvidenceStatus.INVALID
    assert not result.equivalent
    assert ("ferric", "markers") in _mismatch_fields(result)


# Issue #91 red case: matching output cannot conceal zero fixture firings.
def test_zero_fixture_firings_do_not_satisfy_expected_firing():
    ferric = _observation()
    clips = _observation()
    ferric["firings"] = []
    clips["firings"] = []

    result = _evaluate(_declaration(), ferric, clips)

    assert result.status is EvidenceStatus.VALID
    assert not result.equivalent
    assert ("ferric", "firings.count") in _mismatch_fields(result)
    assert ("clips", "firings.count") in _mismatch_fields(result)


# Issue #91 red case: channels are exact unless a future contract says otherwise.
def test_one_character_output_drift_is_not_equivalent():
    declaration = _declaration(stdout="ok\n")
    ferric = _observation(stdout="ok\n")
    clips = _observation(stdout="ok!\n")

    result = _evaluate(declaration, ferric, clips)

    assert result.status is EvidenceStatus.VALID
    assert not result.equivalent
    assert ("clips", "channels.stdout") in _mismatch_fields(result)
    assert ("engines", "channels.stdout") in _mismatch_fields(result)


# Issue #91 red case: fact IDs differ unless this fixture explicitly allows it.
def test_fact_id_drift_requires_fixture_declared_normalization():
    ferric = _observation(facts=[_ordered_fact(fact_id=1)])
    clips = _observation(facts=[_ordered_fact(fact_id=99)])

    strict = _evaluate(_declaration(), ferric, clips)
    allowed = _evaluate(
        _declaration(normalizers=["fact-ids"]),
        ferric,
        clips,
    )

    assert not strict.equivalent
    assert ("clips", "facts") in _mismatch_fields(strict)
    assert allowed.status is EvidenceStatus.VALID
    assert allowed.equivalent


# Issue #91 red case: empty channels pass only alongside non-vacuous state/effect proof.
def test_declared_empty_output_with_nonempty_state_oracle_can_be_equivalent():
    declaration = _declaration(stdout="")
    ferric = _observation(stdout="")
    clips = _observation(stdout="")

    result = _evaluate(declaration, ferric, clips)

    assert result.status is EvidenceStatus.VALID
    assert result.equivalent
    assert declaration["expectations"]["facts"]
    assert declaration["expectations"]["effects"]


def test_empty_output_declaration_without_a_semantic_effect_is_invalid():
    declaration = _declaration(stdout="")
    declaration["expectations"]["effects"] = []

    result = _evaluate(declaration, _observation(stdout=""), _observation(stdout=""))

    assert result.status is EvidenceStatus.INVALID
    assert not result.equivalent
    assert result.declaration.issues[0].field == "expectations.effects"


@pytest.mark.parametrize(
    "bound_field",
    ["id", "source_sha256", "composed_sha256", "nonce"],
)
@pytest.mark.parametrize("marker_index", [0, 1])
def test_spoofed_marker_identity_is_invalid(bound_field, marker_index):
    ferric = _observation()
    spoofed_value = {
        "id": "spoofed-fixture",
        "source_sha256": OTHER_DIGEST,
        "composed_sha256": OTHER_DIGEST,
        "nonce": "f" * 32,
    }[bound_field]
    ferric["markers"][marker_index][bound_field] = spoofed_value

    result = _evaluate(_declaration(), ferric, _observation())

    assert result.status is EvidenceStatus.INVALID
    assert not result.equivalent
    assert ("ferric", f"markers[{marker_index}].{bound_field}") in _mismatch_fields(result)


def test_duplicate_start_or_completion_marker_is_invalid():
    ferric = _observation()
    ferric["markers"].insert(1, deepcopy(ferric["markers"][0]))

    result = _evaluate(_declaration(), ferric, _observation())

    assert result.status is EvidenceStatus.INVALID
    assert not result.equivalent
    assert ("ferric", "markers") in _mismatch_fields(result)
    assert "exactly one" in result.ferric.issues[0].message


def test_completion_before_start_is_invalid():
    ferric = _observation()
    ferric["markers"].reverse()

    result = _evaluate(_declaration(), ferric, _observation())

    assert result.status is EvidenceStatus.INVALID
    assert not result.equivalent
    assert ("ferric", "markers") in _mismatch_fields(result)
    assert "precede" in result.ferric.issues[0].message


def test_stale_declaration_source_digest_is_invalid():
    declaration = _declaration()
    declaration["source_sha256"] = OTHER_DIGEST

    evidence = validate_declaration(
        declaration,
        expected_source_sha256=SOURCE_DIGEST,
        expected_composed_sha256=COMPOSED_DIGEST,
    )

    assert evidence.status is EvidenceStatus.INVALID
    assert evidence.issues[0].field == "source_sha256"
    assert "stale" in evidence.issues[0].message


def test_stale_declaration_composed_digest_is_invalid():
    declaration = _declaration()
    declaration["composed_sha256"] = OTHER_DIGEST

    evidence = validate_declaration(
        declaration,
        expected_source_sha256=SOURCE_DIGEST,
        expected_composed_sha256=COMPOSED_DIGEST,
    )

    assert evidence.status is EvidenceStatus.INVALID
    assert evidence.issues[0].field == "composed_sha256"
    assert "stale" in evidence.issues[0].message


def test_fact_order_drift_requires_fixture_declared_normalization():
    facts = [
        _ordered_fact(fact_id=1, value="one"),
        _ordered_fact(fact_id=2, value="two"),
    ]
    reversed_facts = list(reversed(deepcopy(facts)))

    strict = _evaluate(
        _declaration(facts=facts),
        _observation(facts=facts),
        _observation(facts=reversed_facts),
    )
    allowed = _evaluate(
        _declaration(facts=facts, normalizers=["fact-order"]),
        _observation(facts=facts),
        _observation(facts=reversed_facts),
    )

    assert not strict.equivalent
    assert allowed.equivalent


def test_fact_order_normalization_also_orders_fact_derived_effects():
    facts = [
        _ordered_fact(fact_id=1, value="one", relation="first"),
        _ordered_fact(fact_id=2, value="two", relation="second"),
    ]
    effects = [
        {
            "name": f"fact:MAIN::{fact['relation']}",
            "value": {
                "type": "multifield",
                "value": deepcopy(fact["fields"]),
            },
        }
        for fact in facts
    ]
    declaration = _declaration(facts=facts, normalizers=["fact-order"])
    declaration["expectations"]["effects"] = deepcopy(effects)
    ferric = _observation(facts=facts)
    clips = _observation(facts=list(reversed(deepcopy(facts))))
    ferric["effects"] = [{**effect, "origin": "fixture"} for effect in deepcopy(effects)]
    clips["effects"] = [{**effect, "origin": "fixture"} for effect in reversed(deepcopy(effects))]

    result = _evaluate(declaration, ferric, clips)

    assert result.status is EvidenceStatus.VALID
    assert result.equivalent


def test_float_format_drift_requires_fixture_declared_normalization():
    expected_fact = _ordered_fact()
    expected_fact["fields"] = [_float("1.0")]
    clips_fact = deepcopy(expected_fact)
    clips_fact["fields"] = [_float("1.00")]

    strict = _evaluate(
        _declaration(facts=[expected_fact]),
        _observation(facts=[expected_fact]),
        _observation(facts=[clips_fact]),
    )
    allowed = _evaluate(
        _declaration(facts=[expected_fact], normalizers=["float-format"]),
        _observation(facts=[expected_fact]),
        _observation(facts=[clips_fact]),
    )

    assert not strict.equivalent
    assert allowed.equivalent


def test_float_format_normalization_preserves_digits_beyond_decimal_context_precision():
    expected_fact = _ordered_fact()
    expected_fact["fields"] = [_float("1.123456789012345678901234567801")]
    drifted_fact = deepcopy(expected_fact)
    drifted_fact["fields"] = [_float("1.123456789012345678901234567802")]

    result = _evaluate(
        _declaration(facts=[expected_fact], normalizers=["float-format"]),
        _observation(facts=[expected_fact]),
        _observation(facts=[drifted_fact]),
    )

    assert result.status is EvidenceStatus.VALID
    assert not result.equivalent
    assert ("clips", "facts") in _mismatch_fields(result)


@pytest.mark.parametrize("exponent", ["9" * 5000, f"-{'9' * 5000}"])
def test_float_format_normalization_accepts_extreme_finite_decimal_exponents(exponent):
    expected_fact = _ordered_fact()
    expected_fact["fields"] = [_float(f"1e{exponent}")]
    alternate_fact = deepcopy(expected_fact)
    alternate_fact["fields"] = [_float(f"1.0e{exponent}")]

    result = _evaluate(
        _declaration(facts=[expected_fact], normalizers=["float-format"]),
        _observation(facts=[expected_fact]),
        _observation(facts=[alternate_fact]),
    )

    assert result.status is EvidenceStatus.VALID
    assert result.equivalent


def test_fact_normalization_preserves_duplicate_multiplicity():
    first = _ordered_fact(fact_id=1, value="same")
    second = _ordered_fact(fact_id=2, value="same")
    declaration = _declaration(
        facts=[first, second],
        normalizers=["fact-ids", "fact-order"],
    )
    ferric = _observation(facts=[first, second])
    clips = _observation(facts=[deepcopy(first)])

    result = _evaluate(declaration, ferric, clips)

    assert result.status is EvidenceStatus.VALID
    assert not result.equivalent
    assert ("clips", "facts") in _mismatch_fields(result)


def test_recursive_multifields_and_template_slot_names_are_compared():
    template_fact = {
        "kind": "template",
        "id": 4,
        "origin": "fixture",
        "module": "MAIN",
        "template": "nested-result",
        "slots": [
            {
                "name": "items",
                "value": {
                    "type": "multifield",
                    "value": [
                        _symbol("head"),
                        {
                            "type": "multifield",
                            "value": [
                                {"type": "integer", "value": 2},
                                _float("3.0"),
                            ],
                        },
                    ],
                },
            }
        ],
    }
    clips_fact = deepcopy(template_fact)
    clips_fact["slots"][0]["name"] = "other-items"

    result = _evaluate(
        _declaration(facts=[template_fact]),
        _observation(facts=[template_fact]),
        _observation(facts=[clips_fact]),
    )

    assert result.status is EvidenceStatus.VALID
    assert not result.equivalent
    assert ("clips", "facts") in _mismatch_fields(result)


def test_both_engines_being_identically_wrong_does_not_satisfy_expectation():
    wrong = _observation(facts=[_ordered_fact(value="same-wrong-state")])

    result = _evaluate(_declaration(), wrong, deepcopy(wrong))

    assert result.status is EvidenceStatus.VALID
    assert not result.equivalent
    assert ("ferric", "facts") in _mismatch_fields(result)
    assert ("clips", "facts") in _mismatch_fields(result)
    assert ("engines", "facts") not in _mismatch_fields(result)


def test_evaluation_serialization_is_compact_and_json_ready():
    result = _evaluate(_declaration(), _observation(), _observation())

    serialized = evaluation_to_dict(result)

    assert serialized == {
        "status": "valid",
        "equivalent": True,
        "declaration": {"status": "valid", "issues": []},
        "ferric": {"status": "valid", "issues": []},
        "clips": {"status": "valid", "issues": []},
        "mismatches": [],
    }
