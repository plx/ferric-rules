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


def test_compat_compare_workflow_retains_native_pre_run_scans_and_artifacts():
    workflow_path = Path(__file__).parents[3] / ".github" / "workflows" / "compat-compare.yml"
    workflow = workflow_path.read_text(encoding="utf-8")

    base_capture = "cp tests/examples/compat-manifest.json /tmp/base-compat-scan-manifest.json"
    head_overlay = "git checkout ${{ inputs.head_sha }} --"
    head_capture = "cp tests/examples/compat-manifest.json /tmp/head-compat-scan-manifest.json"
    scanner_generation = workflow.index("- name: Generate retained scanner comparison")
    harness_generation = workflow.index("- name: Generate head compatibility harnesses")
    harness_verification = workflow.index("- name: Verify head compatibility harnesses")
    head_run = workflow.index("- name: Run compat assessment")
    head_gate = workflow.index("- name: Enforce head compatibility policy")
    assert workflow.index(base_capture) < workflow.index(head_overlay)
    assert (
        workflow.index(head_capture)
        < scanner_generation
        < harness_generation
        < harness_verification
        < head_run
        < head_gate
    )
    base_step = workflow.split("- name: Assess base branch", maxsplit=1)[1]
    base_step = base_step.split("# ── Head assessment", maxsplit=1)[0]
    assert base_step.index("ferric-compat-scan") < base_step.index("ferric-harness-gen")
    assert base_step.index("ferric-harness-gen") < base_step.index("--check")
    assert base_step.index("--check") < base_step.index("ferric-compat-run")
    assert "--require-selected" in base_step
    assert "crates/ferric-rules-cli" not in base_step
    assert "crates/ferric-rules-runtime" not in base_step
    assert "--scanner-only" in workflow
    assert "cat /tmp/compat-scanner-diff-report.md >> /tmp/compat-diff-report.md" in workflow
    scanner_step = workflow.split("- name: Generate retained scanner comparison", maxsplit=1)[1]
    scanner_step = scanner_step.split("- name: Generate head compatibility harnesses", maxsplit=1)[
        0
    ]
    assert "continue-on-error" not in scanner_step
    comparison_step = workflow.split("- name: Generate comparison report", maxsplit=1)[1]
    comparison_step = comparison_step.split("- name: Finalize compatibility evidence", maxsplit=1)[
        0
    ]
    assert "continue-on-error" not in comparison_step
    finalize_step = workflow.split("- name: Finalize compatibility evidence", maxsplit=1)[1]
    finalize_step = finalize_step.split("- name: Upload artifacts", maxsplit=1)[0]
    assert "if: always()" in finalize_step
    assert "/tmp/head-assessment-checkout" in finalize_step
    assert "base-intermediate-compat-manifest.json" in finalize_step
    assert "compat-ci-gate.json" in finalize_step
    assert "compat-ci-gate.md" in finalize_step
    upload_step = workflow.split("- name: Upload artifacts", maxsplit=1)[1]
    assert "if: always()" in upload_step
    assert "if-no-files-found: error" in upload_step
    for artifact in (
        "/tmp/compat-scanner-diff.tsv",
        "/tmp/compat-scanner-diff.json",
        "/tmp/compat-scanner-diff-report.md",
        "/tmp/base-compat-scan-manifest.json",
        "/tmp/head-compat-scan-manifest.json",
    ):
        assert artifact in workflow


@pytest.mark.parametrize(
    ("workflow_name", "scan_step", "run_step", "gate_step"),
    [
        (
            "compat-standalone.yml",
            "- name: Run compat scan",
            "- name: Run compat assessment",
            "- name: Enforce compatibility policy",
        ),
        (
            "ci.yml",
            "- name: Scan claimed compatibility subset",
            "- name: Run pinned differential assessment",
            "- name: Enforce exact compatibility policy",
        ),
    ],
)
def test_required_compatibility_workflows_generate_verify_gate_and_always_upload(
    workflow_name,
    scan_step,
    run_step,
    gate_step,
):
    workflow_path = Path(__file__).parents[3] / ".github" / "workflows" / workflow_name
    workflow = workflow_path.read_text(encoding="utf-8")
    scan = workflow.index(scan_step)
    generate = workflow.index("- name: Generate compatibility harnesses", scan)
    verify = workflow.index("- name: Verify compatibility harnesses", generate)
    run = workflow.index(run_step, verify)
    gate = workflow.index(gate_step, run)
    finalize = workflow.index("- name: Finalize compatibility evidence", gate)
    upload = workflow.index("- name: Upload", finalize)

    assert scan < generate < verify < run < gate < finalize < upload
    core = workflow[scan:finalize]
    assert "--check" in core
    assert "--all --require-selected" in core
    assert "--candidate-sha" in core
    assert "continue-on-error" not in core
    assert "if: always()" in workflow[finalize:upload]
    assert "compat-ci-gate.json" in workflow[finalize:upload]
    assert "compat-ci-gate.md" in workflow[finalize:upload]
    assert "if: always()" in workflow[upload:]
    assert "if-no-files-found: error" in workflow[upload:]


def test_pr_assessment_exposes_stable_required_compatibility_context():
    workflow_path = Path(__file__).parents[3] / ".github" / "workflows" / "pr-assessment.yml"
    workflow = workflow_path.read_text(encoding="utf-8")
    required = workflow.split("compatibility-required:", maxsplit=1)[1]
    required = required.split("\n  comment:", maxsplit=1)[0]

    assert "name: PR Compatibility Gate" in required
    assert "needs: compat-compare" in required
    assert "if: always()" in required
    assert "${{ needs.compat-compare.result }}" in required
    assert 'test "$COMPAT_RESULT" = success' in required


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


def test_compute_diff_improvement_detected():
    # When a file moves from "divergent" (rank 1) to "equivalent" (rank 0)
    # it must appear in real_improvements, not regressions.
    base = _manifest({"foo.clp": _file_entry("divergent")})
    head = _manifest({"foo.clp": _file_entry("equivalent", oracle=_oracle())})

    _base_counts, _head_counts, regressions, real_improvements, _reason_changes = compute_diff(
        base, head
    )

    assert len(real_improvements) == 1
    assert real_improvements[0][0] == "foo.clp"
    assert len(regressions) == 0


def test_compute_diff_regression_detected():
    # When a file moves from "equivalent" to "divergent" it is a regression.
    base = _manifest({"bar.clp": _file_entry("equivalent")})
    head = _manifest({"bar.clp": _file_entry("divergent")})

    _base_counts, _head_counts, regressions, real_improvements, _reason_changes = compute_diff(
        base, head
    )

    assert len(regressions) == 1
    assert regressions[0][0] == "bar.clp"
    assert len(real_improvements) == 0


def test_compute_diff_no_changes_when_manifests_identical():
    # Identical manifests produce no regressions, no improvements, no reason
    # changes, and identical counts.
    entry = _file_entry("pending", "testable")
    base = _manifest({"a.clp": entry, "b.clp": entry})
    head = _manifest({"a.clp": entry, "b.clp": entry})

    base_counts, head_counts, regressions, real_improvements, reason_changes = compute_diff(
        base, head
    )

    assert regressions == []
    assert real_improvements == []
    assert reason_changes == []
    assert base_counts == head_counts


def test_compute_diff_reason_change_within_same_classification():
    # When the classification stays the same but the reason text changes, the
    # entry must land in reason_changes (not real_improvements or regressions).
    base = _manifest({"c.clp": _file_entry("divergent", "old-reason")})
    head = _manifest({"c.clp": _file_entry("divergent", "new-reason")})

    _bc, _hc, regressions, real_improvements, reason_changes = compute_diff(base, head)

    assert len(reason_changes) == 1
    assert reason_changes[0][0] == "c.clp"
    assert len(regressions) == 0
    assert len(real_improvements) == 0


def test_compute_diff_exposes_phase_change_with_unchanged_classification_and_reason():
    base_entry = _file_entry("divergent", "diagnostic-phase-mismatch")
    base_entry["ferric"] = _engine_result("load", "construct-error", continued=False)
    base_entry["clips"] = _engine_result("run", "evaluation-error", continued=False)
    head_entry = _file_entry("divergent", "diagnostic-phase-mismatch")
    head_entry["ferric"] = _engine_result("run", "evaluation-error", continued=False)
    head_entry["clips"] = _engine_result("load", "construct-error", continued=False)

    _bc, _hc, regressions, improvements, reason_changes = compute_diff(
        _manifest({"phase.clp": base_entry}),
        _manifest({"phase.clp": head_entry}),
    )

    assert regressions == []
    assert improvements == []
    assert len(reason_changes) == 1
    assert "ferric=load/construct-error" in reason_changes[0][2]
    assert "ferric=run/evaluation-error" in reason_changes[0][4]


def test_compute_diff_exposes_diagnostics_when_classification_changes():
    base_entry = _file_entry("pending", "diagnostic-invalid")
    base_entry["ferric"] = _engine_result("load", "construct-error", continued=False)
    base_entry["clips"] = _engine_result("run", "evaluation-error", continued=False)
    head_entry = _file_entry("divergent", "diagnostic-phase-mismatch")
    head_entry["ferric"] = _engine_result("run", "evaluation-error", continued=False)
    head_entry["clips"] = _engine_result("load", "construct-error", continued=False)

    _bc, _hc, regressions, improvements, reason_changes = compute_diff(
        _manifest({"phase.clp": base_entry}),
        _manifest({"phase.clp": head_entry}),
    )

    assert improvements == [
        (
            "phase.clp",
            "pending",
            base_entry["reason"] + "; diagnostics: "
            "ferric=load/construct-error/continued:false;termination:exit(1), "
            "clips=run/evaluation-error/continued:false;termination:exit(1)",
            "divergent",
            head_entry["reason"] + "; diagnostics: "
            "ferric=run/evaluation-error/continued:false;termination:exit(1), "
            "clips=load/construct-error/continued:false;termination:exit(1)",
        )
    ]
    assert regressions == []
    assert reason_changes == []


def test_compute_diff_counts_reflect_head_manifest():
    # head_counts should count classifications from the head manifest, not base.
    base = _manifest({"x.clp": _file_entry("pending")})
    head = _manifest({"x.clp": _file_entry("equivalent")})

    _bc, head_counts, _r, _i, _rc = compute_diff(base, head)

    assert head_counts["equivalent"] == 1
    assert head_counts["pending"] == 0


def test_compute_diff_keeps_ordinary_additions_and_removals_neutral():
    base = _manifest({"old.clp": _file_entry("pending")})
    head = _manifest({"new.clp": _file_entry("pending")})

    _bc, _hc, regressions, real_improvements, _rc = compute_diff(base, head)

    assert regressions == []
    assert real_improvements == []


def test_compute_diff_flags_newly_added_unverified_equivalent_with_absent_base_tuple():
    base = _manifest({})
    head = _manifest({"new.clp": _file_entry("equivalent", "oracle-v1-match")})

    _bc, _hc, regressions, improvements, reason_changes = compute_diff(base, head)

    assert improvements == []
    assert reason_changes == []
    assert regressions == [
        (
            "new.clp",
            "absent",
            "not present",
            "equivalent",
            "oracle-v1-match; oracle regression: unverified equivalent claim",
        )
    ]


def test_compute_diff_rejects_unchanged_legacy_equivalent_in_v3_head():
    entry = _file_entry("equivalent", "empty-match")
    base = {"version": 2, "files": {"legacy.clp": entry}}
    head = {"version": 3, "files": {"legacy.clp": entry}}

    _bc, _hc, regressions, improvements, reason_changes = compute_diff(base, head)

    assert improvements == []
    assert reason_changes == []
    assert regressions == [
        (
            "legacy.clp",
            "equivalent",
            "empty-match",
            "equivalent",
            "empty-match; oracle regression: unverified equivalent claim",
        )
    ]


def test_compute_diff_keeps_newly_added_verified_equivalent_neutral():
    base = _manifest({})
    head = _manifest({"new.clp": _file_entry("equivalent", oracle=_oracle())})

    _bc, _hc, regressions, improvements, reason_changes = compute_diff(base, head)

    assert regressions == []
    assert improvements == []
    assert reason_changes == []


def test_compute_diff_flags_removed_valid_oracle_fixture_with_absent_head_tuple():
    base = _manifest({"removed.clp": _file_entry("divergent", "oracle-mismatch", oracle=_oracle())})
    head = _manifest({})

    _bc, _hc, regressions, improvements, reason_changes = compute_diff(base, head)

    assert improvements == []
    assert reason_changes == []
    assert regressions == [
        (
            "removed.clp",
            "divergent",
            "oracle-mismatch",
            "absent",
            "not present; oracle regression: valid oracle-backed fixture removed",
        )
    ]


def test_compute_diff_oracle_completion_loss_is_regression_with_same_classification():
    base = _manifest({"covered.clp": _file_entry("equivalent", oracle=_oracle())})
    head = _manifest(
        {
            "covered.clp": _file_entry(
                "equivalent",
                oracle=_oracle(completed=False),
            )
        }
    )

    _bc, _hc, regressions, improvements, reason_changes = compute_diff(base, head)

    assert improvements == []
    assert reason_changes == []
    assert len(regressions) == 1
    assert regressions[0][0] == "covered.clp"
    assert "completed true\u2192false" in regressions[0][4]


def test_compute_diff_valid_equivalent_becoming_missing_is_regression():
    base = _manifest({"covered.clp": _file_entry("equivalent", oracle=_oracle())})
    head = _manifest(
        {
            "covered.clp": _file_entry(
                "equivalent",
                oracle=_missing_oracle(),
            )
        }
    )

    _bc, _hc, regressions, improvements, reason_changes = compute_diff(base, head)

    assert improvements == []
    assert reason_changes == []
    assert len(regressions) == 1
    assert "status valid\u2192invalid" in regressions[0][4]
    assert "unverified equivalent claim" in regressions[0][4]


def test_compute_diff_refuses_new_equivalent_claim_without_valid_evidence():
    base = _manifest({"claim.clp": _file_entry("divergent")})
    head = _manifest({"claim.clp": _file_entry("equivalent")})

    _bc, _hc, regressions, improvements, _reason_changes = compute_diff(base, head)

    assert improvements == []
    assert len(regressions) == 1
    assert "unverified equivalent claim" in regressions[0][4]


def test_compute_diff_refuses_new_equivalent_claim_with_false_validity_flags():
    base = _manifest({"claim.clp": _file_entry("divergent")})
    head = _manifest(
        {
            "claim.clp": _file_entry(
                "equivalent",
                oracle=_oracle(
                    declaration=False,
                    reached=False,
                    completed=False,
                    effect=False,
                ),
            )
        }
    )

    _bc, _hc, regressions, improvements, _reason_changes = compute_diff(base, head)

    assert improvements == []
    assert len(regressions) == 1
    assert "unverified equivalent claim" in regressions[0][4]


def test_compute_diff_refuses_new_equivalent_claim_with_unsupported_evidence_version():
    base = _manifest({"claim.clp": _file_entry("divergent")})
    head = _manifest(
        {
            "claim.clp": _file_entry(
                "equivalent",
                oracle=_oracle(version=3),
            )
        }
    )

    _bc, _hc, regressions, improvements, _reason_changes = compute_diff(base, head)

    assert improvements == []
    assert len(regressions) == 1
    assert "unverified equivalent claim" in regressions[0][4]


@pytest.mark.parametrize("version", [1, 2])
def test_compute_diff_accepts_supported_verified_equivalent_versions(version):
    base = _manifest({"claim.clp": _file_entry("divergent")})
    head = _manifest(
        {
            "claim.clp": _file_entry(
                "equivalent",
                oracle=_oracle(version=version),
            )
        }
    )

    _bc, _hc, regressions, improvements, _reason_changes = compute_diff(base, head)

    assert regressions == []
    assert len(improvements) == 1


def test_compute_diff_schema_migration_reason_change_is_neutral():
    base = {
        "version": 2,
        "files": {
            "migrated.clp": _file_entry(
                "divergent",
                "legacy-output-mismatch",
                oracle=_oracle(version=1),
            )
        },
    }
    head = {
        "version": 3,
        "files": {
            "migrated.clp": _file_entry(
                "divergent",
                "oracle-state-mismatch",
                oracle=_oracle(version=1),
            )
        },
    }

    _bc, _hc, regressions, improvements, reason_changes = compute_diff(base, head)

    assert regressions == []
    assert improvements == []
    assert len(reason_changes) == 1
    assert reason_changes[0][0] == "migrated.clp"


def test_compute_diff_legacy_equivalent_oracle_migration_is_neutral():
    base = {
        "version": 2,
        "files": {
            "legacy.clp": _file_entry(
                "equivalent",
                "exact-match",
            )
        },
    }
    head = {
        "version": 3,
        "files": {
            "legacy.clp": _file_entry(
                "pending",
                "oracle-missing",
                oracle=_missing_oracle(),
            )
        },
    }

    _bc, _hc, regressions, improvements, reason_changes = compute_diff(base, head)

    assert regressions == []
    assert improvements == []
    assert reason_changes == []


def test_compute_diff_legacy_migration_without_explicit_missing_evidence_is_regression():
    base = {
        "version": 2,
        "files": {
            "legacy.clp": _file_entry(
                "equivalent",
                "exact-match",
            )
        },
    }
    head = {
        "version": 3,
        "files": {
            "legacy.clp": _file_entry(
                "pending",
                "oracle-missing",
            )
        },
    }

    _bc, _hc, regressions, improvements, reason_changes = compute_diff(base, head)

    assert improvements == []
    assert reason_changes == []
    assert len(regressions) == 1
    assert regressions[0][0] == "legacy.clp"


def test_compute_diff_legacy_migration_requires_all_missing_coverage_flags_false():
    noncanonical_missing = _missing_oracle()
    noncanonical_missing["effect"] = True
    base = {
        "version": 2,
        "files": {
            "legacy.clp": _file_entry(
                "equivalent",
                "exact-match",
            )
        },
    }
    head = {
        "version": 3,
        "files": {
            "legacy.clp": _file_entry(
                "pending",
                "oracle-missing",
                oracle=noncanonical_missing,
            )
        },
    }

    _bc, _hc, regressions, improvements, reason_changes = compute_diff(base, head)

    assert improvements == []
    assert reason_changes == []
    assert len(regressions) == 1
    assert regressions[0][0] == "legacy.clp"


def test_compute_diff_legacy_divergent_oracle_migration_is_neutral():
    base = {
        "version": 2,
        "files": {
            "legacy.clp": _file_entry(
                "divergent",
                "output-mismatch",
            )
        },
    }
    head = {
        "version": 3,
        "files": {
            "legacy.clp": _file_entry(
                "pending",
                "oracle-missing",
                oracle=_missing_oracle(),
            )
        },
    }

    _bc, _hc, regressions, improvements, reason_changes = compute_diff(base, head)

    assert regressions == []
    assert improvements == []
    assert reason_changes == []


def test_compute_diff_legacy_runtime_incompatible_oracle_migration_is_neutral():
    base = {
        "version": 2,
        "files": {
            "legacy.clp": _file_entry(
                "incompatible",
                "both-error",
            )
        },
    }
    head = {
        "version": 3,
        "files": {
            "legacy.clp": _file_entry(
                "pending",
                "oracle-missing",
                oracle=_missing_oracle(),
            )
        },
    }

    _bc, _hc, regressions, improvements, reason_changes = compute_diff(base, head)

    assert regressions == []
    assert improvements == []
    assert reason_changes == []


def test_compute_diff_legacy_pending_reason_migration_is_neutral():
    base = {
        "version": 2,
        "files": {
            "library.clp": _file_entry(
                "pending",
                "library-only",
            )
        },
    }
    head = {
        "version": 3,
        "files": {
            "library.clp": _file_entry(
                "pending",
                "oracle-missing",
            )
        },
    }

    _bc, _hc, regressions, improvements, reason_changes = compute_diff(base, head)

    assert regressions == []
    assert improvements == []
    assert reason_changes == [
        (
            "library.clp",
            "pending",
            "library-only",
            "pending",
            "oracle-missing",
        )
    ]


def test_compute_diff_static_incompatible_oracle_reset_is_a_regression():
    base = {
        "version": 2,
        "files": {
            "static.clp": _file_entry(
                "incompatible",
                "unsupported-form",
            )
        },
    }
    head = {
        "version": 3,
        "files": {
            "static.clp": _file_entry(
                "pending",
                "oracle-missing",
                oracle=_missing_oracle(),
            )
        },
    }

    _bc, _hc, regressions, improvements, reason_changes = compute_diff(base, head)

    assert improvements == []
    assert reason_changes == []
    assert len(regressions) == 1
    assert regressions[0][0] == "static.clp"


def test_compute_diff_v3_oracle_coverage_loss_remains_a_regression():
    base = {
        "version": 3,
        "files": {
            "covered.clp": _file_entry(
                "equivalent",
                "oracle-equivalent",
                oracle=_oracle(),
            )
        },
    }
    head = {
        "version": 3,
        "files": {
            "covered.clp": _file_entry(
                "pending",
                "oracle-missing",
                oracle=_missing_oracle(),
            )
        },
    }

    _bc, _hc, regressions, improvements, reason_changes = compute_diff(base, head)

    assert improvements == []
    assert reason_changes == []
    assert len(regressions) == 1
    assert regressions[0][0] == "covered.clp"
    assert "oracle regression" in regressions[0][4]


def test_write_tsv_labels_legacy_oracle_demotion_as_schema_migration(tmp_path):
    base = {
        "version": 2,
        "files": {
            "legacy.clp": {
                **_file_entry("equivalent", "exact-match"),
                "source": "fixtures",
            }
        },
    }
    head = {
        "version": 3,
        "files": {
            "legacy.clp": {
                **_file_entry("pending", "oracle-missing", oracle=_missing_oracle()),
                "source": "fixtures",
            }
        },
    }
    output = tmp_path / "diff.tsv"

    write_tsv(base, head, str(output))

    with output.open(newline="", encoding="utf-8") as stream:
        rows = list(csv.DictReader(stream, delimiter="\t"))
    assert rows[0]["change"] == "schema-migration"
    assert rows[0]["oracle_regression"] == ""


def test_write_tsv_marks_oracle_coverage_loss_as_regression(tmp_path):
    base = _manifest(
        {
            "covered.clp": {
                **_file_entry("equivalent", oracle=_oracle(normalizations=["fact-ids"])),
                "source": "fixtures",
            }
        }
    )
    head = _manifest(
        {
            "covered.clp": {
                **_file_entry(
                    "equivalent",
                    oracle=_oracle(completed=False, normalizations=["fact-ids"]),
                ),
                "source": "fixtures",
            }
        }
    )
    output = tmp_path / "diff.tsv"

    write_tsv(base, head, str(output))

    with output.open(newline="", encoding="utf-8") as stream:
        rows = list(csv.DictReader(stream, delimiter="\t"))
    assert rows[0]["change"] == "regression"
    assert rows[0]["base_oracle_status"] == "valid"
    assert rows[0]["head_oracle_status"] == "invalid"
    assert rows[0]["head_oracle_normalizations"] == "fact-ids"
    assert "completed true\u2192false" in rows[0]["oracle_regression"]


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
    assert row["change"] == "reason-changed"
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
