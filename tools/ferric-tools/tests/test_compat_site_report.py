"""Documentation must fail closed instead of publishing cached or partial evidence."""

import copy
import json
import os
import subprocess
from pathlib import Path

import pytest

from ferric_tools.compat import site_report

ISSUE = "https://github.com/plx/ferric-rules/issues/324"
MATCH = "facts/001_match.clp"
GAP = "queries/001_expression.clp"


@pytest.fixture
def corpus(tmp_path):
    root = tmp_path
    directory = root / site_report.CORPUS
    (directory / "facts").mkdir(parents=True)
    (directory / "queries").mkdir()
    (directory / MATCH).write_text(
        ";; Level: basic\n; A small fact example.\n(defrule example => (printout t 1 crlf))\n"
    )
    (directory / GAP).write_text("; Query expression.\n(defrule query => (find-all-facts))\n")
    (directory / MATCH).with_suffix(".out").write_text(" 1 \n\n")
    (directory / GAP).with_suffix(".out").write_text("TRUE\n")
    (directory / GAP).with_suffix(".in").write_text("sample\n")
    manifest = {
        "schema_version": 1,
        "cases": [
            {"path": MATCH, "level": "basic", "covers": ["facts"], "resets": 2},
            {
                "path": GAP,
                "level": "boundary",
                "covers": ["queries"],
                "gap": {
                    "summary": "Stored explanation is not evidence",
                    "issues": [ISSUE],
                    "observed": {"phase": "complete", "output": "STALE", "diagnostics": []},
                },
            },
        ],
    }
    policy = {
        "schema_version": 1,
        "issues": {
            ISSUE: {
                "kind": "deliberate-boundary",
                "label": "Deliberate subset boundary",
                "explanation": "Expression queries are outside the supported subset.",
            }
        },
        "overrides": {},
    }
    ferric = {
        MATCH: {"phase": "complete", "output": " 1 \n\n", "diagnostics": []},
        GAP: {"phase": "load", "output": "", "diagnostics": ["expression query unsupported"]},
    }
    reference = {
        "version": "CLIPS (6.30 3/17/15)",
        "image_id": "sha256:" + "a" * 64,
        "results": {
            MATCH: {"matches": True, "output": " 1 \n\n"},
            GAP: {"matches": True, "output": "TRUE\n"},
        },
    }
    (directory / "manifest.json").write_text(json.dumps(manifest))
    (directory / "dispositions.json").write_text(json.dumps(policy))
    (root / "Cargo.toml").write_text("[workspace]\n")
    return root, manifest, policy, ferric, reference


def build(corpus):
    root, manifest, policy, ferric, reference = corpus
    return site_report.build_report(
        root,
        site_report.validate_manifest(root, manifest),
        policy,
        ferric,
        reference,
        {"revision": "b" * 40, "source_digest": "sha256:sample", "generated_at": "now"},
    )


def test_uses_live_outputs_exactly_and_shows_source_input_protocol_and_disposition(corpus):
    report = build(corpus)
    assert report["summary"] == {"total": 2, "matching": 1, "different": 1}
    assert [group["id"] for group in report["groups"]] == ["facts", "queries"]
    match, gap = [group["cases"][0] for group in report["groups"]]
    assert match["clips_output"] == match["ferric_output"] == " 1 \n\n"
    assert match["title"] == "A small fact example"
    assert match["resets"] == 2
    assert match["input"] is None
    assert match["disposition"]["kind"] == "match"
    assert gap["id"] == "queries_001_expression"
    assert gap["ferric_output"] == ""  # Never read the manifest's stale observation.
    assert gap["clips_output"] == "TRUE\n"
    assert gap["ferric_phase"] == "load"
    assert gap["diagnostics"] == ["expression query unsupported"]
    assert gap["input"] == "sample\n"
    assert "(defrule query" in gap["source"]
    assert gap["disposition"] == {**corpus[2]["issues"][ISSUE], "issues": [ISSUE]}
    assert report["provenance"]["reference_image"] == "sha256:" + "a" * 64


@pytest.mark.parametrize("engine", ["ferric", "clips"])
@pytest.mark.parametrize("change", ["missing", "extra"])
def test_rejects_incomplete_or_extra_execution_cases(corpus, engine, change):
    results = corpus[3] if engine == "ferric" else corpus[4]["results"]
    if change == "missing":
        del results[MATCH]
    else:
        results["facts/999_extra.clp"] = results[MATCH]
    with pytest.raises(site_report.ReportFailure, match="report cases"):
        build(corpus)


@pytest.mark.parametrize(
    "result",
    [
        {"matches": False, "output": "TRUE\n"},
        {"matches": True, "output": "false\n"},
        {"matches": False, "error": "reference timed out"},
        {"matches": True, "output": "TRUE\n", "warnings": ["unexpected warning"]},
    ],
)
def test_rejects_reference_errors_diagnostics_and_golden_disagreements(corpus, result):
    corpus[4]["results"][GAP] = result
    with pytest.raises(site_report.ReportFailure, match="CLIPS"):
        build(corpus)


def test_rejects_unclassified_ferric_warning_even_if_stdout_matches(corpus):
    corpus[3][MATCH]["diagnostics"] = ["warning: discarded action"]
    with pytest.raises(site_report.ReportFailure, match="unclassified mismatch"):
        build(corpus)


def test_rejects_an_unexpected_fix_until_characterization_and_policy_are_updated(corpus):
    corpus[3][GAP] = {"phase": "complete", "output": "TRUE\n", "diagnostics": []}
    with pytest.raises(site_report.ReportFailure, match="unexpected match"):
        build(corpus)


@pytest.mark.parametrize("change", ["missing", "unknown", "empty", "stale-issue", "stale-path"])
def test_rejects_missing_unknown_empty_or_stale_dispositions(corpus, change):
    policy = corpus[2]
    if change == "missing":
        del policy["issues"][ISSUE]
    elif change == "unknown":
        policy["issues"][ISSUE]["kind"] = "will-be-fixed-tomorrow"
    elif change == "empty":
        policy["issues"][ISSUE]["explanation"] = " "
    elif change == "stale-issue":
        policy["issues"][ISSUE + "99"] = policy["issues"][ISSUE]
    else:
        policy["overrides"][MATCH] = {**policy["issues"][ISSUE], "issues": [ISSUE]}
    with pytest.raises(site_report.ReportFailure):
        build(corpus)


def test_mixed_issue_kinds_need_an_explicit_probe_override(corpus):
    issue2 = ISSUE + "2"
    corpus[1]["cases"][1]["gap"]["issues"].append(issue2)
    corpus[2]["issues"][issue2] = {
        "kind": "tracked-defect",
        "label": "Tracked for correction",
        "explanation": "A defect.",
    }
    with pytest.raises(site_report.ReportFailure, match="mixed dispositions"):
        build(corpus)
    override = {
        "kind": "deliberate-boundary",
        "label": "Deliberate subset boundary",
        "explanation": "This combination tests a deliberate boundary and a tracked defect.",
        "issues": [ISSUE, issue2],
    }
    corpus[2]["overrides"][GAP] = override
    assert build(corpus)["groups"][1]["cases"][0]["disposition"] == override


def mock_runners(monkeypatch, corpus, *, failure=None, after_reference=None, dirty=False):
    root, _, _, ferric, reference = corpus
    calls = []

    def run(command, **kwargs):
        calls.append((command, copy.deepcopy(kwargs)))
        assert kwargs["cwd"] == root
        assert kwargs["check"] is True
        assert kwargs["timeout"] > 0
        if command[0] == "git":
            stdout = (
                "b" * 40 + "\n" if command[1] == "rev-parse" else "?? new.rs\n" if dirty else ""
            )
            return subprocess.CompletedProcess(command, 0, stdout=stdout)
        is_ferric = command[0] == "cargo"
        output = (
            Path(kwargs["env"]["FERRIC_CORPUS_REPORT"])
            if is_ferric
            else Path(command[command.index("--report") + 1])
        )
        if failure == "timeout":
            raise subprocess.TimeoutExpired(command, kwargs["timeout"])
        if failure == "exit":
            # A partial report from a failing suite must never reach the site.
            output.write_text(json.dumps(ferric))
            raise subprocess.CalledProcessError(1, command)
        if failure != "missing":
            output.write_text(json.dumps(ferric if is_ferric else reference))
        if failure == "stale":
            os.utime(output, (1, 1))
        if not is_ferric and after_reference:
            after_reference()
        return subprocess.CompletedProcess(command, 0)

    monkeypatch.setattr(site_report.subprocess, "run", run)
    return calls


def test_runs_both_suites_without_filters_and_publishes_only_fresh_reports(corpus, monkeypatch):
    root = corpus[0]
    output = root / site_report.DEFAULT_OUTPUT
    monkeypatch.setenv("FERRIC_CORPUS_FILTER", "does-not-exist")
    monkeypatch.setenv("FERRIC_CORPUS_LEVEL", "boundary")
    monkeypatch.setenv("FERRIC_CORPUS_REPORT", "/old/report.json")
    calls = mock_runners(monkeypatch, corpus, dirty=True)
    site_report.generate(root, output)
    first = json.loads(output.read_text())
    assert first["summary"]["total"] == 2
    assert first["provenance"]["revision"] == "b" * 40
    assert first["provenance"]["workspace_dirty"] is True
    assert first["provenance"]["source_digest"].startswith("sha256:")
    assert first["provenance"]["generated_at"].endswith("Z")
    assert calls[2][0] == [
        "cargo",
        "test",
        "--locked",
        "-p",
        "ferric-rules",
        "--test",
        "compat_corpus",
        "--",
        "--nocapture",
    ]
    for command, kwargs in calls[2:]:
        assert "FERRIC_CORPUS_FILTER" not in kwargs["env"]
        assert "FERRIC_CORPUS_LEVEL" not in kwargs["env"]
        assert "--filter" not in command and "--level" not in command
    assert "ferric_tools.compat.corpus" in calls[3][0]
    assert not Path(calls[2][1]["env"]["FERRIC_CORPUS_REPORT"]).exists()
    site_report.generate(root, output)
    assert calls[2][1]["env"]["FERRIC_CORPUS_REPORT"] != calls[6][1]["env"]["FERRIC_CORPUS_REPORT"]
    assert list(output.parent.iterdir()) == [output]


@pytest.mark.parametrize("failure", ["exit", "timeout", "missing", "stale"])
def test_runner_failure_removes_old_site_data_and_never_publishes_partial_report(
    corpus, monkeypatch, failure
):
    root = corpus[0]
    output = root / "existing.json"
    output.write_text("stale data")
    mock_runners(monkeypatch, corpus, failure=failure)
    with pytest.raises((site_report.ReportFailure, subprocess.SubprocessError)):
        site_report.generate(root, output)
    assert not output.exists()


def test_concurrent_source_change_prevents_publication(corpus, monkeypatch):
    root = corpus[0]
    output = root / "existing.json"
    output.write_text("stale data")
    mock_runners(
        monkeypatch,
        corpus,
        after_reference=lambda: (root / "Cargo.toml").write_text("[workspace]\n# changed\n"),
    )
    with pytest.raises(site_report.ReportFailure, match="source inputs changed"):
        site_report.generate(root, output)
    assert not output.exists()


@pytest.mark.parametrize(
    "filename",
    [
        "Cargo.lock",
        ".cargo/config.toml",
        "crates/ferric-rules-runtime/src/new.rs",
        "tools/ferric-tools/src/ferric_tools/compat/corpus.py",
        "docker/clips-reference/Dockerfile",
        "tests/clips_compat/corpus/dispositions.json",
        "tests/clips_compat/corpus/facts/001_match.out",
    ],
)
def test_digest_tracks_code_build_fixtures_harness_and_policy(corpus, filename):
    root = corpus[0]
    before = site_report.source_digest(root)
    path = root / filename
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("changed input")
    assert site_report.source_digest(root) != before


def test_digest_ignores_generated_site_data_and_python_bytecode(corpus):
    root = corpus[0]
    before = site_report.source_digest(root)
    for filename in (
        site_report.DEFAULT_OUTPUT,
        Path("tools/ferric-tools/src/ferric_tools/compat/__pycache__/corpus.pyc"),
    ):
        path = root / filename
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("new generated file")
    assert site_report.source_digest(root) == before


def test_rejects_duplicate_json_report_keys(tmp_path):
    path = tmp_path / "report.json"
    path.write_text('{"probe": {}, "probe": {}}')
    with pytest.raises(site_report.ReportFailure, match="duplicate JSON key"):
        site_report.read_json(path)


def test_rejects_unregistered_fixture(corpus):
    root = corpus[0]
    (root / site_report.CORPUS / "facts/002_extra.clp").write_text("; unregistered\n")
    with pytest.raises(site_report.ReportFailure, match="corpus registration"):
        build(corpus)
