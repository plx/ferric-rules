"""Contract tests for the shared cross-binding conformance corpus."""

from pathlib import Path

import pytest

from ferric_tools.bindings_conformance import (
    REQUIRED_BINDINGS,
    REQUIRED_SEMANTICS,
    Case,
    Corpus,
    CorpusError,
    Deviation,
    _parse_adapter_output,
    compare_observations,
    load_corpus,
)

ROOT = Path(__file__).resolve().parents[3]
CORPUS = ROOT / "tests" / "bindings-conformance" / "corpus.json"
TRACKING_ISSUE = "https://github.com/plx/ferric-rules/issues/112"


def test_corpus_covers_every_binding_and_required_semantic() -> None:
    corpus = load_corpus(CORPUS)

    assert set(corpus.bindings) == set(REQUIRED_BINDINGS)
    assert {case.semantic for case in corpus.cases} >= set(REQUIRED_SEMANTICS)


def test_every_deviation_is_versioned_and_explained() -> None:
    corpus = load_corpus(CORPUS)

    for case in corpus.cases:
        for binding, deviation in case.deviations.items():
            assert binding in REQUIRED_BINDINGS
            assert deviation.rationale
            assert deviation.since
            assert deviation.tracking_issue.startswith(
                "https://github.com/plx/ferric-rules/issues/"
            )


def test_case_ids_are_unique_and_all_adapters_are_required() -> None:
    corpus = load_corpus(CORPUS)
    case_ids = [case.id for case in corpus.cases]

    assert len(case_ids) == len(set(case_ids))
    assert all(case.required_bindings == frozenset(REQUIRED_BINDINGS) for case in corpus.cases)


def test_configuration_isolation_uses_behavioral_observations() -> None:
    corpus = load_corpus(CORPUS)
    case = next(case for case in corpus.cases if case.id == "configuration.isolation")

    assert case.semantic == "configuration.isolation"
    assert case.required_bindings == frozenset(REQUIRED_BINDINGS)
    assert set(case.deviations) == {"python"}
    assert case.canonical["encoding_ascii_only"] == {
        "halt_reason": "action_error",
        "unicode": "rejected",
    }
    assert case.canonical["strategy_breadth_only"] == {
        "halt_reason": "action_error",
        "strategy_fired": 2,
        "unicode": "accepted",
    }
    assert case.canonical["depth_256_only"] == {
        "halt_reason": "agenda_empty",
        "unicode": "accepted",
    }


def test_unknown_semantic_drift_fails() -> None:
    corpus = _single_case_corpus()

    failures, accepted = compare_observations(corpus, {"rust": {"probe": {"value": 2}}})

    assert failures == ['probe/rust: expected canonical {"value": 1}, got {"value": 2}']
    assert accepted == []


def test_exact_deviation_is_accepted_but_a_stale_deviation_fails() -> None:
    deviation = Deviation(
        expected={"value": 2},
        rationale="Known binding behavior.",
        since="1.0.0",
        tracking_issue=TRACKING_ISSUE,
    )
    corpus = _single_case_corpus(deviation)

    failures, accepted = compare_observations(corpus, {"rust": {"probe": {"value": 2}}})
    assert failures == []
    assert accepted == [f"probe/rust (1.0.0, {TRACKING_ISSUE})"]

    failures, accepted = compare_observations(corpus, {"rust": {"probe": {"value": 1}}})
    assert failures == ["probe/rust: now matches canonical; remove the stale deviation"]
    assert accepted == []


def test_adapter_protocol_rejects_missing_duplicate_and_unknown_cases() -> None:
    with pytest.raises(CorpusError, match="omitted cases"):
        _parse_adapter_output("rust", "", {"probe"})
    with pytest.raises(CorpusError, match="duplicate case"):
        _parse_adapter_output(
            "rust",
            "\n".join(
                [
                    '{"case":"probe","result":1}',
                    '{"case":"probe","result":1}',
                ]
            ),
            {"probe"},
        )
    with pytest.raises(CorpusError, match="unknown case"):
        _parse_adapter_output("rust", '{"case":"surprise","result":1}', {"probe"})


def _single_case_corpus(deviation: Deviation | None = None) -> Corpus:
    return Corpus(
        schema_version=1,
        suite_version="1.0.0",
        bindings=("rust",),
        cases=(
            Case(
                id="probe",
                semantic="probe",
                canonical={"value": 1},
                deviations={"rust": deviation} if deviation is not None else {},
                required_bindings=frozenset({"rust"}),
            ),
        ),
    )
