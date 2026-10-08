"""Reports separate declared coverage from matching, executed oracle evidence."""

import json
from pathlib import Path

import pytest
from typer.testing import CliRunner

from ferric_tools.compat import corpus_summary as cs
from ferric_tools.compat.corpus_render import corpus_diff_lines, corpus_lines
from ferric_tools.compat.diff import app as diff_app
from ferric_tools.compat.report import app as report_app
from ferric_tools.compat.report import write_report


@pytest.fixture
def summaries(tmp_path, monkeypatch):
    monkeypatch.setattr(
        cs,
        "checkout_identity",
        lambda root, revision=None: {
            "revision": "a" * 40,
            "requested_revision": revision,
            "dirty": False,
        },
    )

    def create(name, cases):
        root = tmp_path / name
        directory = root / Path(cs.MANIFEST).parent
        directory.mkdir(parents=True)
        descriptors = []
        for path, fields in cases.items():
            fields = dict(fields)
            source = fields.pop("source", "(defrule case =>)")
            golden = fields.pop("golden", "result\n")
            (directory / path).write_text(source)
            (directory / path).with_suffix(".out").write_text(golden)
            descriptors.append({"path": path, "level": "basic", **fields})
        cs.write_json(root / cs.MANIFEST, {"schema_version": 1, "cases": descriptors})
        return cs.capture(root, run_id=name)

    return create


def verify(summary, directory):
    for kind in ("ferric", "reference"):
        results = {}
        for path, case in summary["corpus"]["cases"].items():
            conforms = case["expectation"] == "conformance"
            results[path] = (
                {"matches": True, "output": "oracle"}
                if kind == "reference"
                else {
                    "verdict": "conformance" if conforms else "known_gap",
                    "accepted": True,
                    "conforms": conforms,
                }
            )
        evidence = {
            "schema": "ferric.compat-corpus-reference"
            if kind == "reference"
            else "ferric.compat-corpus-status",
            "version": 1,
            "run_id": summary["run_id"],
            "revision": summary["checkout"]["revision"],
            **{key: value for key, value in summary["corpus"].items() if key.endswith("_sha256")},
            "scope": "characterization",
            "status": "passed",
            "complete": True,
            "selection": {"filter": "", "level": None, "paths": list(results)},
            "results": results,
            "provenance": {"image_id": "sha256:" + "b" * 64, "version": "CLIPS (6.30 3/17/15)"},
        }
        report, status = directory / f"{kind}.json", directory / "status.json"
        cs.write_json(
            report,
            {"evidence": evidence, "results": results}
            if kind == "reference"
            else {path: {"phase": "complete"} for path in results},
        )
        if kind == "ferric":
            evidence["observations_sha256"] = cs.digest(report.read_bytes())
        cs.write_json(status, evidence)
        cs.attach_run(summary, kind, report, 0, status)
    assert cs.corpus_verification(summary)["status"] == "verified", summary["runs"]


def test_declared_and_verified_fixed_gaps_are_distinct(summaries, tmp_path):
    base = summaries("base", {"case.clp": {"gap": {"issues": ["issue399"]}}})
    head = summaries("head", {"case.clp": {}})
    output = "\n".join(corpus_diff_lines(base, head))
    assert "| Verified gap → conformance | 0 |" in output
    assert "| Declared gap → conformance (unverified) | 1 |" in output
    verify(head, tmp_path)
    assert "| Declared gap → conformance (unverified) | 1 |" in "\n".join(
        corpus_diff_lines(base, head)
    )
    verify(base, tmp_path)
    assert "| Verified gap → conformance | 1 |" in "\n".join(corpus_diff_lines(base, head))


@pytest.mark.parametrize(
    ("change", "expected"),
    [
        ("source", "Changed scenarios (not same-case fixes)"),
        ("golden", "Changed goldens (oracle changes)"),
    ],
)
def test_changed_test_or_oracle_is_not_a_fixed_gap(summaries, change, expected):
    base = summaries("base", {"case.clp": {"gap": {"issues": ["issue399"]}}})
    head = summaries("head", {"case.clp": {change: "changed"}})
    output = "\n".join(corpus_diff_lines(base, head))
    assert f"| {expected} | 1 |" in output
    assert "| Declared gap → conformance (unverified) | 0 |" in output


def test_coverage_addition_and_gap_removal_are_separate(summaries):
    base = summaries("base", {"old.clp": {"gap": {"issues": ["issue399"]}}})
    head = summaries("head", {"new.clp": {"error": "load"}})
    output = "\n".join(corpus_diff_lines(base, head))
    assert "| Added coverage | 1 |" in output
    assert "| Removed cases (not fixes) | 1 |" in output
    assert "| Conformance | 1 |" in output
    assert "| Expected load errors (conformance subset) | 1 |" in output
    assert "| Declared gap → conformance (unverified) | 0 |" in output


def test_failed_reference_never_produces_verified_label(summaries, tmp_path):
    summary = summaries("head", {"case.clp": {}})
    verify(summary, tmp_path)
    summary["runs"]["reference"] = cs.unavailable_run("image unavailable", 1)
    output = "\n".join(corpus_lines(summary))
    assert "Evidence: **failed**" in output
    assert "image unavailable" in output
    assert "Evidence: **verified**" not in output


def test_report_leads_with_corpus_and_supports_missing_legacy_manifest(summaries, tmp_path):
    artifact, report = tmp_path / "summary.json", tmp_path / "report.md"
    cs.write_json(artifact, summaries("head", {"case.clp": {}}))
    result = CliRunner().invoke(
        report_app,
        [
            "--manifest",
            str(tmp_path / "missing.json"),
            "--corpus-summary",
            str(artifact),
            "--report",
            str(report),
        ],
    )
    assert result.exit_code == 0, result.output
    output = report.read_text()
    assert output.index("### Granular corpus") < output.index("### Legacy assessment")
    assert "Declared counts only" in output
    assert "Legacy assessment not produced" in output


def test_diff_renders_missing_base_summary_without_live_checkout_inference(summaries, tmp_path):
    artifact, report = tmp_path / "head.json", tmp_path / "report.md"
    cs.write_json(artifact, summaries("head", {"case.clp": {}}))
    result = CliRunner().invoke(
        diff_app,
        [
            str(tmp_path / "base-manifest.json"),
            str(tmp_path / "head-manifest.json"),
            "--base-corpus-summary",
            str(tmp_path / "missing.json"),
            "--head-corpus-summary",
            str(artifact),
            "--report",
            str(report),
        ],
    )
    assert result.exit_code == 0, result.output
    assert "Before/after corpus evidence unavailable" in report.read_text()
    assert "Legacy assessment not produced" in report.read_text()


@pytest.mark.parametrize("bad", [[], {"files": []}, {"files": {"case": []}}])
def test_invalid_supplied_legacy_manifest_does_not_become_empty_success(summaries, tmp_path, bad):
    legacy, artifact = tmp_path / "legacy.json", tmp_path / "summary.json"
    legacy.write_text(json.dumps(bad))
    cs.write_json(artifact, summaries("head", {"case.clp": {}}))
    result = CliRunner().invoke(
        report_app, ["--manifest", str(legacy), "--corpus-summary", str(artifact)]
    )
    assert result.exit_code != 0
    assert "error" in result.output


def test_report_without_corpus_does_not_infer_checkout_counts(tmp_path):
    report = tmp_path / "report.md"
    write_report({"files": {}}, str(report))
    assert "Corpus evidence unavailable" in report.read_text()
