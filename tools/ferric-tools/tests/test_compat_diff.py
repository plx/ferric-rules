"""Tests for ferric_tools.compat.diff.

Covers compute_diff() and format_markdown().
"""

from __future__ import annotations

import csv
import json
from pathlib import Path

import pytest
from typer.testing import CliRunner

from ferric_tools.compat.diagnostics import diagnostic
from ferric_tools.compat.diff import (
    app,
    compute_diff,
    compute_scanner_diff,
    format_markdown,
    format_scanner_markdown,
    write_scanner_json,
    write_scanner_tsv,
    write_tsv,
)
from ferric_tools.compat.report import compute_oracle_coverage

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _manifest(files: dict) -> dict:
    """Build a minimal manifest dict with the given files mapping."""
    return {"version": 1, "files": files}


def _oracle(
    status: str = "valid",
    *,
    version: int = 1,
    declaration: bool = True,
    reached: bool = True,
    completed: bool = True,
    effect: bool = True,
    normalizations: list[str] | None = None,
) -> dict:
    return {
        "status": status,
        "version": version,
        "declaration": declaration,
        "reached": reached,
        "completed": completed,
        "effect": effect,
        "normalizations": normalizations or [],
        "violations": [],
    }


def _missing_oracle() -> dict:
    return _oracle(
        status="missing",
        declaration=False,
        reached=False,
        completed=False,
        effect=False,
    )


def _file_entry(classification: str, reason: str = "", *, oracle: dict | None = None) -> dict:
    entry = {"classification": classification, "reason": reason}
    if oracle is not None:
        entry["oracle_evidence"] = oracle
    return entry


def _engine_result(
    phase: str,
    category: str,
    *,
    continued: bool,
    exit_code: int = 1,
) -> dict:
    return {
        "exit_code": exit_code,
        "diagnostic": diagnostic(phase, category, continued=continued),
        "termination": {"kind": "exit", "exit_code": exit_code, "signal": None},
    }


def _span(start_byte: int = 0, end_byte: int = 4) -> dict:
    return {
        "start_byte": start_byte,
        "end_byte": end_byte,
        "start_line": 1,
        "start_column": start_byte + 1,
        "end_line": 1,
        "end_column": end_byte + 1,
    }


def _detection(
    feature: str = "defrule",
    *,
    category: str = "supported-construct",
    reason: str = "supported-form",
    head_span: dict | None = None,
    form_span: dict | None = None,
) -> dict:
    return {
        "feature": feature,
        "category": category,
        "reason": reason,
        "head_span": head_span or _span(1, 8),
        "form_span": form_span or _span(0, 20),
    }


def _feature_scan(
    *,
    status: str = "valid",
    detections: list[dict] | None = None,
    issues: list[dict] | None = None,
) -> dict:
    return {
        "version": 1,
        "status": status,
        "detections": [_detection()] if detections is None else detections,
        "issues": issues or [],
    }


def _scanner_entry(
    *,
    features: list[str] | None = None,
    unsupported_features: list[str] | None = None,
    classification: str = "pending",
    reason: str = "testable",
    runability: str = "standalone",
    feature_scan: dict | None = None,
) -> dict:
    entry = {
        "features": ["defrule"] if features is None else features,
        "unsupported_features": [] if unsupported_features is None else unsupported_features,
        "classification": classification,
        "reason": reason,
        "runability": runability,
    }
    if feature_scan is not None:
        entry["feature_scan"] = feature_scan
    return entry


# ---------------------------------------------------------------------------
# scanner-only retained diff
# ---------------------------------------------------------------------------


def test_scanner_diff_treats_new_structured_evidence_as_legacy_neutral():
    base = _manifest({"same.clp": _scanner_entry()})
    head = _manifest({"same.clp": _scanner_entry(feature_scan=_feature_scan())})

    result = compute_scanner_diff(base, head)

    assert result["summary"] == {
        "files_compared": 1,
        "changed_files": 0,
        "added_files": 0,
        "removed_files": 0,
        "legacy_base_structured_evidence": 1,
        "head_structured_evidence": 1,
        "head_invalid_structured_evidence": 0,
        "head_scan_issues": 0,
    }
    assert result["changes"] == []


def test_scanner_diff_retains_effective_field_changes_and_structured_spans():
    base = _manifest({"string.clp": _scanner_entry()})
    detection = {
        "feature": "load",
        "category": "loading-command",
        "reason": "unsupported-command",
        "head_span": _span(12, 16),
        "form_span": _span(11, 24),
    }
    head = _manifest(
        {
            "string.clp": _scanner_entry(
                features=["deffacts", "defrule"],
                unsupported_features=["load"],
                classification="incompatible",
                reason="unsupported-command",
                runability="batch",
                feature_scan=_feature_scan(
                    detections=[_detection(), _detection("deffacts"), detection]
                ),
            )
        }
    )

    result = compute_scanner_diff(base, head)

    assert result["summary"]["changed_files"] == 1
    assert result["summary"]["legacy_base_structured_evidence"] == 1
    assert len(result["changes"]) == 1
    change = result["changes"][0]
    assert change["change"] == "changed"
    assert change["structured_evidence_change"] == "legacy-base"
    assert change["changed_fields"] == [
        "features",
        "unsupported_features",
        "classification",
        "reason",
        "runability",
    ]
    assert change["head"]["feature_scan"]["detections"][2]["head_span"] == _span(12, 16)


def test_scanner_diff_retains_invalid_head_evidence_without_counting_legacy_as_change():
    issue = {
        "kind": "unterminated-string",
        "reason": "string literal reaches end of input",
        "span": _span(8, 19),
    }
    malformed_entry = {
        "classification": "incompatible",
        "reason": "malformed-source",
        "runability": "unknown",
    }
    base = _manifest({"malformed.clp": _scanner_entry(**malformed_entry)})
    head = _manifest(
        {
            "malformed.clp": _scanner_entry(
                **malformed_entry,
                feature_scan=_feature_scan(status="invalid", issues=[issue]),
            )
        }
    )

    result = compute_scanner_diff(base, head)

    assert result["summary"]["changed_files"] == 0
    assert result["summary"]["legacy_base_structured_evidence"] == 1
    assert result["summary"]["head_invalid_structured_evidence"] == 1
    assert result["summary"]["head_scan_issues"] == 1
    assert result["changes"] == [
        {
            "path": "malformed.clp",
            "change": "structured-evidence",
            "changed_fields": [],
            "structured_evidence_change": "legacy-base",
            "base": _scanner_entry(**malformed_entry),
            "head": _scanner_entry(
                **malformed_entry,
                feature_scan=_feature_scan(status="invalid", issues=[issue]),
            ),
        }
    ]


def test_scanner_diff_compares_structured_evidence_once_both_revisions_have_it():
    base_scan = _feature_scan()
    head_scan = _feature_scan(
        detections=[
            {
                "feature": "defrule",
                "category": "supported-construct",
                "reason": "supported-form",
                "head_span": _span(2, 9),
                "form_span": _span(0, 20),
            }
        ]
    )
    base = _manifest({"evidence.clp": _scanner_entry(feature_scan=base_scan)})
    head = _manifest({"evidence.clp": _scanner_entry(feature_scan=head_scan)})

    result = compute_scanner_diff(base, head)

    assert result["summary"]["changed_files"] == 1
    assert result["summary"]["legacy_base_structured_evidence"] == 0
    assert result["changes"][0]["changed_fields"] == ["feature_scan"]
    assert result["changes"][0]["structured_evidence_change"] == "changed"


def test_scanner_diff_does_not_label_a_new_file_as_legacy_structured_evidence():
    result = compute_scanner_diff(
        _manifest({}),
        _manifest({"new.clp": _scanner_entry(feature_scan=_feature_scan())}),
    )

    assert result["summary"]["added_files"] == 1
    assert result["summary"]["legacy_base_structured_evidence"] == 0
    assert result["changes"][0]["structured_evidence_change"] == "added"


def test_scanner_diff_machine_outputs_and_markdown_retain_review_evidence(tmp_path):
    base = _manifest({"changed.clp": _scanner_entry()})
    head = _manifest(
        {
            "changed.clp": _scanner_entry(
                features=["deffacts", "defrule"],
                unsupported_features=["load"],
                feature_scan=_feature_scan(
                    detections=[
                        _detection(),
                        _detection("deffacts"),
                        _detection(
                            "load",
                            category="loading-command",
                            reason="unsupported-command",
                        ),
                    ]
                ),
            )
        }
    )
    result = compute_scanner_diff(base, head)
    tsv_path = tmp_path / "scanner.tsv"
    json_path = tmp_path / "scanner.json"

    write_scanner_tsv(result, str(tsv_path))
    write_scanner_json(result, str(json_path))
    markdown = "\n".join(format_scanner_markdown(result))

    with tsv_path.open(newline="", encoding="utf-8") as stream:
        row = next(csv.DictReader(stream, delimiter="\t"))
    assert row["changed_fields"] == "features;unsupported_features"
    assert row["structured_evidence_change"] == "legacy-base"
    assert row["head_feature_scan_status"] == "valid"
    assert json.loads(json_path.read_text(encoding="utf-8")) == result
    assert "Static Compatibility Scanner Diff" in markdown
    assert "legacy schema boundary, not as scanner changes" in markdown
    assert "`changed.clp`" in markdown


def test_scanner_only_cli_exits_zero_for_observed_changes(tmp_path):
    base_path = tmp_path / "base.json"
    head_path = tmp_path / "head.json"
    report_path = tmp_path / "scanner.md"
    tsv_path = tmp_path / "scanner.tsv"
    json_path = tmp_path / "scanner-diff.json"
    base_path.write_text(json.dumps(_manifest({"changed.clp": _scanner_entry()})), encoding="utf-8")
    head_path.write_text(
        json.dumps(
            _manifest(
                {
                    "changed.clp": _scanner_entry(
                        classification="incompatible",
                        reason="unsupported-command",
                        runability="batch",
                        feature_scan=_feature_scan(),
                    )
                }
            )
        ),
        encoding="utf-8",
    )

    result = CliRunner().invoke(
        app,
        [
            str(base_path),
            str(head_path),
            "--scanner-only",
            "--report",
            str(report_path),
            "--tsv",
            str(tsv_path),
            "--json",
            str(json_path),
        ],
    )

    assert result.exit_code == 0, result.output
    assert report_path.is_file()
    assert tsv_path.is_file()
    assert json_path.is_file()


def test_scanner_only_cli_fails_on_malformed_structured_evidence(tmp_path):
    base_path = tmp_path / "base.json"
    head_path = tmp_path / "head.json"
    base_path.write_text(json.dumps(_manifest({"bad.clp": _scanner_entry()})), encoding="utf-8")
    malformed = _feature_scan()
    malformed["status"] = "maybe"
    head_path.write_text(
        json.dumps(_manifest({"bad.clp": _scanner_entry(feature_scan=malformed)})),
        encoding="utf-8",
    )

    result = CliRunner().invoke(
        app,
        [str(base_path), str(head_path), "--scanner-only"],
    )

    assert result.exit_code == 2
    assert "cannot generate scanner diff" in result.output


def test_scanner_only_cli_fails_when_head_lacks_required_structured_evidence(tmp_path):
    base_path = tmp_path / "base.json"
    head_path = tmp_path / "head.json"
    base_path.write_text(json.dumps(_manifest({"missing.clp": _scanner_entry()})), encoding="utf-8")
    head_path.write_text(json.dumps(_manifest({"missing.clp": _scanner_entry()})), encoding="utf-8")

    result = CliRunner().invoke(
        app,
        [str(base_path), str(head_path), "--scanner-only"],
    )

    assert result.exit_code == 2
    assert "missing.clp" in result.output
    assert "feature_scan is required" in result.output


def test_scanner_only_cli_fails_when_head_aggregates_do_not_project_detections(tmp_path):
    base_path = tmp_path / "base.json"
    head_path = tmp_path / "head.json"
    base_path.write_text(
        json.dumps(_manifest({"mismatch.clp": _scanner_entry()})), encoding="utf-8"
    )
    head_path.write_text(
        json.dumps(
            _manifest(
                {
                    "mismatch.clp": _scanner_entry(
                        features=[],
                        feature_scan=_feature_scan(),
                    )
                }
            )
        ),
        encoding="utf-8",
    )

    result = CliRunner().invoke(
        app,
        [str(base_path), str(head_path), "--scanner-only"],
    )

    assert result.exit_code == 2
    assert "features must exactly project feature_scan detections" in result.output


def test_scanner_only_cli_fails_when_detection_metadata_is_not_canonical(tmp_path):
    base_path = tmp_path / "base.json"
    head_path = tmp_path / "head.json"
    base_path.write_text(json.dumps(_manifest({"bad.clp": _scanner_entry()})), encoding="utf-8")
    bad_detection = _detection(reason="unsupported-form")
    head_path.write_text(
        json.dumps(
            _manifest(
                {"bad.clp": _scanner_entry(feature_scan=_feature_scan(detections=[bad_detection]))}
            )
        ),
        encoding="utf-8",
    )

    result = CliRunner().invoke(
        app,
        [str(base_path), str(head_path), "--scanner-only"],
    )

    assert result.exit_code == 2
    assert "category/reason must be" in result.output


def test_scanner_diff_rejects_structured_status_disposition_mismatches():
    issue = {
        "kind": "unmatched-close",
        "reason": "unmatched-close",
        "span": _span(),
    }
    mismatched_entries = (
        _scanner_entry(feature_scan=_feature_scan(status="invalid", issues=[issue])),
        _scanner_entry(
            classification="incompatible",
            reason="malformed-source",
            runability="unknown",
            feature_scan=_feature_scan(),
        ),
    )

    for entry in mismatched_entries:
        try:
            compute_scanner_diff(_manifest({}), _manifest({"bad.clp": entry}))
        except ValueError as error:
            assert "feature_scan" in str(error)
        else:
            raise AssertionError("feature_scan disposition mismatch must fail closed")


def test_scanner_diff_allows_head_read_error_without_structured_evidence():
    read_error = _scanner_entry(
        features=[],
        classification="incompatible",
        reason="read-error",
        runability="unknown",
    )

    result = compute_scanner_diff(
        _manifest({"binary.clp": read_error}),
        _manifest({"binary.clp": read_error}),
    )

    assert result["changes"] == []
    assert result["summary"]["head_structured_evidence"] == 0


def test_scanner_diff_rejects_head_read_error_with_feature_aggregates():
    read_error = _scanner_entry(
        classification="incompatible",
        reason="read-error",
        runability="unknown",
    )

    try:
        compute_scanner_diff(_manifest({}), _manifest({"binary.clp": read_error}))
    except ValueError as error:
        assert "read-error entries cannot claim detected features" in str(error)
    else:
        raise AssertionError("read-error feature aggregates must fail closed")


def test_local_assessment_recipe_runs_the_complete_blocking_lane():
    justfile = (Path(__file__).parents[3] / "justfile").read_text(encoding="utf-8")
    recipe = justfile.split("\nassess-compatibility:", maxsplit=1)[1]
    recipe = recipe.split("\n# ── Bat processing", maxsplit=1)[0]

    ordered = [
        "just build-cli-release",
        "docker build",
        "just compat-scan",
        "just harness-gen --output-dir",
        "--check",
        "just compat-run --all --require-selected --candidate-sha",
        "just compat-ci-gate --expected-commit-sha",
        "just compat-report",
    ]
    positions = [recipe.index(fragment) for fragment in ordered]
    assert positions == sorted(positions)


# ---------------------------------------------------------------------------
# compute_diff — classification changes
# ---------------------------------------------------------------------------


def _executed(classification="equivalent", *, version=1):
    entry = _file_entry(classification, "oracle-match", oracle=_oracle(version=version))
    entry["oracle"] = {
        "version": version,
        "source_sha256": "a" * 64,
        "composed_sha256": "b" * 64,
        "nonce": "0" * 32,
        "expectations": {"facts": []},
    }
    for engine in ("ferric", "clips"):
        entry[engine] = {"canonical_observation": {"run": {"halt_reason": "agenda-empty"}}}
    return entry


@pytest.mark.parametrize("version", [1, 2])
def test_only_executed_divergence_to_equivalence_is_an_improvement(version):
    base = _manifest({"case.clp": _executed("divergent", version=version)})
    head = _manifest({"case.clp": _executed("equivalent", version=version)})
    bc, hc, regressions, improvements, changes = compute_diff(base, head)
    assert bc["divergent"] == hc["equivalent"] == 1
    assert [item[0] for item in improvements] == ["case.clp"]
    assert regressions == changes == []


@pytest.mark.parametrize(
    "classification", ["incompatible", "pending", "divergent", "equivalent", "unassessed"]
)
def test_unexecuted_legacy_labels_are_neutral_inventory(classification):
    base = _manifest({"case.clp": _file_entry(classification, "scanner-feature")})
    head = {"version": 4, "files": {"case.clp": _file_entry("unassessed", "oracle-missing")}}
    bc, hc, regressions, improvements, changes = compute_diff(base, head)
    assert bc["unassessed"] == hc["unassessed"] == 1
    assert regressions == improvements == []
    assert changes


def test_new_oracle_execution_is_coverage_not_a_fixed_divergence():
    base = _manifest({"case.clp": _file_entry("divergent", "legacy-timeout")})
    head = _manifest({"case.clp": _executed()})
    _, hc, regressions, improvements, changes = compute_diff(base, head)
    assert hc["equivalent"] == 1
    assert regressions == improvements == []
    assert changes[0][0] == "case.clp"


def test_additions_and_removals_are_separate_from_improvements():
    base = _manifest({"old.clp": _executed("divergent")})
    head = _manifest({"new.clp": _executed()})
    _, _, regressions, improvements, changes = compute_diff(base, head)
    assert regressions == improvements == []
    assert {row[0] for row in changes} == {"old.clp", "new.clp"}


@pytest.mark.parametrize(
    "damage", ["completion", "version", "missing-reference", "projection-error", "invalid"]
)
def test_invalid_or_lost_executed_evidence_remains_a_regression(damage):
    base = _manifest({"case.clp": _executed()})
    bad = _executed()
    if damage == "completion":
        bad["oracle_evidence"]["completed"] = False
    elif damage == "version":
        bad["oracle_evidence"]["version"] = 99
    elif damage == "missing-reference":
        bad["clips"] = None
    elif damage == "projection-error":
        bad["ferric"]["projection_error"] = "broken protocol"
    else:
        bad["oracle_evidence"]["status"] = "invalid"
    _, hc, regressions, improvements, _ = compute_diff(base, _manifest({"case.clp": bad}))
    assert hc["evidence-failure"] == 1
    assert len(regressions) == 1
    assert improvements == []


def test_equivalence_to_valid_divergence_is_regression():
    _, _, regressions, improvements, _ = compute_diff(
        _manifest({"case.clp": _executed()}),
        _manifest({"case.clp": _executed("divergent")}),
    )
    assert len(regressions) == 1
    assert improvements == []


def test_diagnostic_changes_remain_visible_without_semantic_claims():
    before = _file_entry("divergent", "diagnostic-phase-mismatch")
    after = _file_entry("divergent", "diagnostic-phase-mismatch")
    before["ferric"] = _engine_result("load", "construct-error", continued=False)
    after["ferric"] = _engine_result("run", "evaluation-error", continued=False)
    _, _, regressions, improvements, changes = compute_diff(
        _manifest({"case.clp": before}),
        _manifest({"case.clp": after}),
    )
    assert regressions == improvements == []
    assert "ferric=load/construct-error" in changes[0][2]
    assert "ferric=run/evaluation-error" in changes[0][4]


def test_identical_executed_manifests_have_no_changes():
    manifest = _manifest({"case.clp": _executed()})
    bc, hc, regressions, improvements, changes = compute_diff(manifest, manifest)
    assert bc == hc
    assert regressions == improvements == changes == []


def test_tsv_never_labels_static_inventory_change_as_improvement(tmp_path):
    output = tmp_path / "change.tsv"
    write_tsv(
        _manifest({"case.clp": _file_entry("incompatible", "unsupported")}),
        _manifest({"case.clp": _file_entry("pending", "testable")}),
        str(output),
    )
    with output.open() as stream:
        row = next(csv.DictReader(stream, delimiter="\t"))
    assert row["base_classification"] == row["head_classification"] == "unassessed"
    assert row["change"] == "inventory-or-coverage-change"


def test_write_tsv_includes_diagnostic_and_termination_evidence(tmp_path):
    base_entry = {**_file_entry("divergent", "same"), "source": "fixtures"}
    base_entry["ferric"] = _engine_result("load", "construct-error", continued=False)
    base_entry["clips"] = _engine_result("run", "evaluation-error", continued=False)
    head_entry = {**_file_entry("divergent", "same"), "source": "fixtures"}
    head_entry["ferric"] = _engine_result("run", "evaluation-error", continued=False)
    head_entry["clips"] = {
        "exit_code": -9,
        "diagnostic": diagnostic("process", "signal", continued=False),
        "termination": {
            "kind": "signal",
            "exit_code": None,
            "signal": 9,
            "active_phase": "run",
        },
    }
    output = tmp_path / "diagnostics.tsv"

    write_tsv(
        _manifest({"phase.clp": base_entry}),
        _manifest({"phase.clp": head_entry}),
        str(output),
    )

    with output.open(newline="", encoding="utf-8") as stream:
        row = next(csv.DictReader(stream, delimiter="\t"))
    assert row["change"] == "inventory-or-coverage-change"
    assert row["base_ferric_diagnostic_phase"] == "load"
    assert row["head_ferric_diagnostic_phase"] == "run"
    assert row["head_clips_diagnostic_category"] == "signal"
    assert row["head_clips_termination"] == "signal"
    assert row["head_clips_signal"] == "9"
    assert row["head_clips_active_phase"] == "run"


# ---------------------------------------------------------------------------
# format_markdown
# ---------------------------------------------------------------------------


def test_format_markdown_returns_list_of_strings():
    base_counts = {"equivalent": 1, "divergent": 0, "incompatible": 0, "pending": 0}
    head_counts = {"equivalent": 1, "divergent": 0, "incompatible": 0, "pending": 0}

    lines = format_markdown(base_counts, head_counts, [], [], [])

    assert isinstance(lines, list)
    assert all(isinstance(line, str) for line in lines)


def test_format_markdown_contains_report_heading():
    # The very first content line must be the standard heading.
    base_counts = {"equivalent": 0, "divergent": 0, "incompatible": 0, "pending": 1}
    head_counts = {"equivalent": 0, "divergent": 0, "incompatible": 0, "pending": 1}

    lines = format_markdown(base_counts, head_counts, [], [], [])

    assert "## CLIPS Compatibility Report" in lines


def test_format_markdown_lists_regression_file():
    # When there is a regression, the offending file name should appear in the
    # output so readers can identify what broke.
    base_counts = {"equivalent": 1, "divergent": 0, "incompatible": 0, "pending": 0}
    head_counts = {"equivalent": 0, "divergent": 1, "incompatible": 0, "pending": 0}
    regressions = [("my-test.clp", "equivalent", "", "divergent", "")]

    lines = format_markdown(base_counts, head_counts, regressions, [], [])

    full_output = "\n".join(lines)
    assert "my-test.clp" in full_output


def test_format_markdown_no_regressions_says_none():
    # When there are no regressions, the report must include the word "None"
    # under the Regressions heading.
    base_counts = {"equivalent": 1, "divergent": 0, "incompatible": 0, "pending": 0}
    head_counts = {"equivalent": 1, "divergent": 0, "incompatible": 0, "pending": 0}

    lines = format_markdown(base_counts, head_counts, [], [], [])

    full_output = "\n".join(lines)
    assert "None" in full_output


def test_format_markdown_exposes_oracle_coverage_and_normalizations():
    base = _manifest({"a.clp": _file_entry("pending")})
    head = _manifest(
        {
            "a.clp": _file_entry(
                "equivalent",
                oracle=_oracle(normalizations=["fact-ids"]),
            )
        }
    )
    base_counts = {"equivalent": 0, "divergent": 0, "incompatible": 0, "pending": 1}
    head_counts = {"equivalent": 1, "divergent": 0, "incompatible": 0, "pending": 0}

    lines = format_markdown(
        base_counts,
        head_counts,
        [],
        [],
        [],
        base_oracle=compute_oracle_coverage(base),
        head_oracle=compute_oracle_coverage(head),
    )

    output = "\n".join(lines)
    assert "### Oracle evidence coverage" in output
    assert "| selected | 0 | 1 | +1 |" in output
    assert "Versions \u2014 base: (none); head: 1: 1" in output
    assert "Normalizations \u2014 base: (none); head: fact-ids: 1" in output


@pytest.mark.parametrize("change", ["source", "harness", "expectations", "missing"])
def test_changed_legacy_oracle_identity_is_not_an_engine_improvement(change):
    before = _executed("divergent")
    after = _executed()
    if change == "source":
        after["source_sha256"] = "c" * 64
    elif change == "harness":
        after["oracle"]["composed_sha256"] = "c" * 64
    elif change == "expectations":
        after["oracle"]["expectations"]["facts"] = ["new expected fact"]
    else:
        del after["oracle"]
    _, _, regressions, improvements, changes = compute_diff(
        _manifest({"case.clp": before}),
        _manifest({"case.clp": after}),
    )
    assert regressions == improvements == []
    assert "source/oracle identity changed or unavailable" in changes[0][4]


def test_execution_nonce_and_feature_prose_do_not_change_legacy_oracle_identity():
    before = _executed("divergent")
    after = _executed()
    after["oracle"].update(nonce="1" * 32, feature="edited documentation")
    _, _, regressions, improvements, _ = compute_diff(
        _manifest({"case.clp": before}),
        _manifest({"case.clp": after}),
    )
    assert len(improvements) == 1
    assert regressions == []
