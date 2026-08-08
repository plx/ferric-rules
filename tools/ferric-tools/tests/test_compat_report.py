"""Tests for structured-oracle compatibility reporting."""

from __future__ import annotations

import csv

import pytest

from ferric_tools.compat.diagnostics import diagnostic
from ferric_tools.compat.report import (
    _write_delimited,
    compute_oracle_coverage,
    oracle_evidence_view,
    print_summary,
    write_report,
)


def _oracle(
    status: str,
    *,
    version: int,
    declaration: bool,
    reached: bool,
    completed: bool,
    effect: bool,
    normalizations: list[str] | None = None,
    violations: list[str] | None = None,
) -> dict:
    return {
        "status": status,
        "version": version,
        "declaration": declaration,
        "reached": reached,
        "completed": completed,
        "effect": effect,
        "normalizations": normalizations or [],
        "violations": violations or [],
    }


def _entry(classification: str, reason: str, oracle: dict | None = None) -> dict:
    entry = {
        "source": "fixtures",
        "classification": classification,
        "reason": reason,
        "runability": "standalone",
        "features": [],
        "unsupported_features": [],
    }
    if oracle is not None:
        entry["oracle_evidence"] = oracle
    return entry


def _manifest() -> dict:
    files = {
        "valid.clp": _entry(
            "equivalent",
            "oracle-match",
            _oracle(
                "valid",
                version=1,
                declaration=True,
                reached=True,
                completed=True,
                effect=True,
                normalizations=["fact-ids"],
            ),
        ),
        "missing.clp": _entry(
            "divergent",
            "oracle-missing",
            _oracle(
                "missing",
                version=1,
                declaration=False,
                reached=False,
                completed=False,
                effect=False,
            ),
        ),
        "invalid.clp": _entry(
            "pending",
            "oracle-invalid",
            _oracle(
                "invalid",
                version=2,
                declaration=True,
                reached=True,
                completed=False,
                effect=False,
                normalizations=["float-format"],
                violations=["completion marker missing"],
            ),
        ),
        "legacy.clp": _entry("pending", "testable"),
        "legacy-equivalent.clp": _entry("equivalent", "empty-match"),
    }
    return {
        "version": 3,
        "generated": "2026-07-26T00:00:00+00:00",
        "summary": {
            "total": 5,
            "equivalent": 2,
            "divergent": 1,
            "incompatible": 0,
            "pending": 2,
        },
        "files": files,
    }


def test_compute_oracle_coverage_handles_legacy_and_refuses_unverified_equivalence():
    coverage = compute_oracle_coverage(_manifest())

    assert coverage == {
        "total": 5,
        "selected": 3,
        "declaration": 2,
        "valid": 1,
        "missing": 2,
        "invalid": 2,
        "reached": 2,
        "completed": 1,
        "effect": 1,
        "refused_equivalent": 1,
        "versions": {"1": 2, "2": 1},
        "normalizations": {"fact-ids": 1, "float-format": 1},
    }

    legacy = oracle_evidence_view(_manifest()["files"]["legacy.clp"])
    refused = oracle_evidence_view(_manifest()["files"]["legacy-equivalent.clp"])
    assert legacy["status"] == "missing"
    assert legacy["selected"] is False
    assert refused["status"] == "invalid"
    assert refused["refused_equivalent"] is True


def test_valid_scenario_v2_evidence_is_reported_as_valid():
    info = _entry(
        "equivalent",
        "oracle-v2-match",
        _oracle(
            "valid",
            version=2,
            declaration=True,
            reached=True,
            completed=True,
            effect=True,
            normalizations=["fact-ids"],
        ),
    )

    view = oracle_evidence_view(info)

    assert view["status"] == "valid"
    assert view["version"] == 2
    assert view["refused_equivalent"] is False


def test_print_summary_exposes_oracle_coverage_versions_and_normalizations(capsys):
    print_summary(_manifest())

    output = capsys.readouterr().out
    assert "Oracle evidence:" in output
    assert "selected            :      3 ( 60.0%)" in output
    assert "refused equivalent  :      1 ( 20.0%)" in output
    assert "versions            : 1: 2, 2: 1" in output
    assert "normalizations      : fact-ids: 1, float-format: 1" in output
    assert "legacy-equivalent.clp (empty-match) [REFUSED: invalid oracle evidence]" in output


def test_markdown_report_exposes_oracle_coverage_and_refused_claim(tmp_path):
    report = tmp_path / "compat.md"

    write_report(_manifest(), str(report))

    output = report.read_text(encoding="utf-8")
    assert "### Oracle evidence coverage" in output
    assert "| valid | 1 | 20.0% |" in output
    assert "| missing | 2 | 40.0% |" in output
    assert "| invalid | 2 | 40.0% |" in output
    assert "Versions: 1: 2, 2: 1" in output
    assert "Normalizations: fact-ids: 1, float-format: 1" in output
    assert (
        "`legacy-equivalent.clp` (empty-match) \u2014 **REFUSED: invalid oracle evidence**"
        in output
    )


def test_reports_retain_candidate_and_reference_digests(tmp_path, capsys):
    manifest = _manifest()
    manifest["candidate"] = {
        "schema": "ferric.compat-candidate-provenance",
        "version": 1,
        "commit_sha": "a" * 40,
        "binary_sha256": "b" * 64,
    }
    manifest["reference"] = {
        "platform": "linux/amd64",
        "binary_sha256": "c" * 64,
        "library_sha256": "d" * 64,
        "image_id": "sha256:" + "e" * 64,
        "base_image": "debian:bookworm-slim@sha256:" + "f" * 64,
    }
    report = tmp_path / "compat.md"

    print_summary(manifest)
    write_report(manifest, str(report))

    summary = capsys.readouterr().out
    assert f"Candidate commit:       {'a' * 40}" in summary
    assert f"Reference binary SHA:   {'c' * 64}" in summary
    markdown = report.read_text(encoding="utf-8")
    assert "### Candidate and reference provenance" in markdown
    assert f"| Ferric candidate binary SHA-256 | `{'b' * 64}` |" in markdown
    assert f"| CLIPS image ID | `sha256:{'e' * 64}` |" in markdown


@pytest.mark.parametrize(("delimiter", "suffix"), [(",", "csv"), ("\t", "tsv")])
def test_delimited_report_exposes_per_file_oracle_evidence(tmp_path, delimiter, suffix):
    output = tmp_path / f"compat.{suffix}"

    _write_delimited(_manifest(), str(output), delimiter)

    with output.open(newline="", encoding="utf-8") as stream:
        rows = {row["path"]: row for row in csv.DictReader(stream, delimiter=delimiter)}

    valid = rows["valid.clp"]
    assert valid["oracle_selected"] == "True"
    assert valid["oracle_status"] == "valid"
    assert valid["oracle_version"] == "1"
    assert valid["oracle_normalizations"] == "fact-ids"

    legacy = rows["legacy.clp"]
    assert legacy["oracle_selected"] == "False"
    assert legacy["oracle_status"] == "missing"

    refused = rows["legacy-equivalent.clp"]
    assert refused["oracle_status"] == "invalid"
    assert refused["oracle_refused_equivalent"] == "True"


def test_reports_expose_phase_diagnostic_and_independent_termination(tmp_path, capsys):
    manifest = _manifest()
    divergent = manifest["files"]["missing.clp"]
    divergent["reason"] = "diagnostic-phase-mismatch"
    divergent["ferric"] = {
        "exit_code": 1,
        "diagnostic": diagnostic("load", "construct-error", continued=False),
        "termination": {"kind": "exit", "exit_code": 1, "signal": None},
    }
    divergent["clips"] = {
        "exit_code": -9,
        "diagnostic": diagnostic("run", "evaluation-error", continued=False),
        "termination": {
            "kind": "signal",
            "exit_code": None,
            "signal": 9,
            "active_phase": "run",
        },
    }
    report = tmp_path / "compat.md"
    table = tmp_path / "compat.csv"

    print_summary(manifest)
    write_report(manifest, str(report))
    _write_delimited(manifest, str(table), ",")

    summary = capsys.readouterr().out
    assert "Diagnostic evidence (1):" in summary
    assert "ferric: diagnostic v1 load/construct-error" in summary
    assert "clips: diagnostic v1 run/evaluation-error" in summary
    assert "termination=signal signal=9 active-phase=run" in summary
    markdown = report.read_text(encoding="utf-8")
    assert "### Diagnostic evidence (1)" in markdown
    assert "ferric: diagnostic v1 load/construct-error" in markdown
    with table.open(newline="", encoding="utf-8") as stream:
        row = {item["path"]: item for item in csv.DictReader(stream)}["missing.clp"]
    assert row["ferric_diagnostic_phase"] == "load"
    assert row["ferric_diagnostic_category"] == "construct-error"
    assert row["clips_diagnostic_phase"] == "run"
    assert row["clips_termination"] == "signal"
    assert row["clips_signal"] == "9"
    assert row["clips_active_phase"] == "run"


@pytest.mark.parametrize(
    "evidence",
    [
        "not-a-mapping",
        {"status": "unknown"},
        {"status": "valid", "declaration": "yes"},
        {"status": "valid", "normalizations": "fact-ids"},
        _oracle(
            "valid",
            version=3,
            declaration=True,
            reached=True,
            completed=True,
            effect=True,
        ),
        _oracle(
            "valid",
            version=1,
            declaration=False,
            reached=True,
            completed=True,
            effect=True,
        ),
        _oracle(
            "valid",
            version=1,
            declaration=True,
            reached=False,
            completed=True,
            effect=True,
        ),
        _oracle(
            "valid",
            version=1,
            declaration=True,
            reached=True,
            completed=False,
            effect=True,
        ),
    ],
)
def test_malformed_oracle_evidence_is_invalid_without_crashing(evidence):
    info = _entry("pending", "oracle-invalid")
    info["oracle_evidence"] = evidence

    view = oracle_evidence_view(info)

    assert view["selected"] is True
    assert view["status"] == "invalid"


def test_v1_oracle_evidence_rejects_unsupported_normalizer_names():
    info = _entry(
        "pending",
        "oracle-invalid",
        _oracle(
            "valid",
            version=1,
            declaration=True,
            reached=True,
            completed=True,
            effect=True,
            normalizations=["string-whitespace"],
        ),
    )

    view = oracle_evidence_view(info)

    assert view["status"] == "invalid"
    assert view["normalizations"] == ["string-whitespace"]


@pytest.mark.parametrize("field", ["declaration", "reached", "completed", "effect"])
def test_missing_oracle_evidence_requires_all_coverage_flags_false(field):
    evidence = _oracle(
        "missing",
        version=1,
        declaration=False,
        reached=False,
        completed=False,
        effect=False,
    )
    evidence[field] = True
    info = _entry("pending", "oracle-missing", evidence)

    view = oracle_evidence_view(info)

    assert view["status"] == "invalid"


def test_equivalent_claim_requires_complete_supported_oracle_evidence():
    info = _entry(
        "equivalent",
        "oracle-v1-match",
        _oracle(
            "valid",
            version=1,
            declaration=False,
            reached=False,
            completed=False,
            effect=False,
        ),
    )

    view = oracle_evidence_view(info)

    assert view["status"] == "invalid"
    assert view["refused_equivalent"] is True


def test_valid_semantic_divergence_remains_valid_evidence():
    info = _entry(
        "divergent",
        "oracle-facts-mismatch",
        _oracle(
            "valid",
            version=1,
            declaration=True,
            reached=True,
            completed=True,
            effect=False,
            violations=["ferric.effects: does not match the declared expectation"],
        ),
    )

    view = oracle_evidence_view(info)

    assert view["status"] == "valid"
    assert view["effect"] is False
    assert view["refused_equivalent"] is False


def test_equivalent_claim_with_semantic_mismatch_is_refused():
    info = _entry(
        "equivalent",
        "oracle-v1-match",
        _oracle(
            "valid",
            version=1,
            declaration=True,
            reached=True,
            completed=True,
            effect=True,
            violations=["engines.facts: differs by engine"],
        ),
    )

    view = oracle_evidence_view(info)

    assert view["status"] == "invalid"
    assert view["refused_equivalent"] is True
