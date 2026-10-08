"""Reference command failures retain useful artifacts, including startup failures."""

import json
import subprocess
from pathlib import Path
from types import SimpleNamespace

import pytest

from ferric_tools.compat import corpus
from ferric_tools.compat import corpus_summary as cs


@pytest.fixture
def reference_tree(tmp_path, monkeypatch):
    directory = tmp_path / Path(cs.MANIFEST).parent
    directory.mkdir(parents=True)
    cs.write_json(
        tmp_path / cs.MANIFEST,
        {
            "schema_version": 1,
            "cases": [
                {"path": "one.clp", "level": "basic"},
            ],
        },
    )
    (directory / "one.clp").write_text("(defrule go =>)")
    (directory / "one.out").write_text("reference\n")
    monkeypatch.setattr(
        cs,
        "checkout_identity",
        lambda root, revision=None: {
            "revision": "a" * 40,
            "requested_revision": revision,
            "dirty": False,
        },
    )
    return tmp_path


def run_main(root, monkeypatch, failure=None, options=()):
    report = root / "report.json"
    calls = []

    def run(command, **kwargs):
        calls.append((command, kwargs))
        if failure == "image" and command[1:3] == ["image", "inspect"]:
            raise OSError("Docker is unavailable")
        if command[1:3] == ["image", "inspect"]:
            return SimpleNamespace(stdout="sha256:" + "b" * 64)
        if command[1] == "run":
            if failure == "timeout":
                raise subprocess.TimeoutExpired(command, kwargs["timeout"])
            return SimpleNamespace(
                stdout="wrong version" if failure == "version" else "CLIPS (6.30 3/17/15)"
            )
        return SimpleNamespace(stdout="")

    def case(*args):
        if failure == "case":
            raise corpus.ReferenceFailure("case timed out")
        return "wrong\n" if failure == "mismatch" else "reference\n"

    monkeypatch.setattr(corpus.subprocess, "run", run)
    monkeypatch.setattr(corpus, "run_reference", case)
    monkeypatch.setattr(
        "sys.argv",
        [
            "corpus",
            "--root",
            str(root),
            "--report",
            str(report),
            "--run-id",
            "run",
            "--revision",
            "a" * 40,
            *options,
        ],
    )
    with pytest.raises(SystemExit) as exit_info:
        corpus.main()
    return exit_info.value.code, json.loads(report.read_text()), calls


def test_full_success_preserves_old_fields_and_uses_120_second_startup(reference_tree, monkeypatch):
    code, report, calls = run_main(reference_tree, monkeypatch)
    assert code == 0
    assert report["results"] == {"one.clp": {"matches": True, "output": "reference\n"}}
    assert report["version"] == "CLIPS (6.30 3/17/15)"
    assert report["evidence"]["complete"] is True
    assert report["evidence"]["selection"]["paths"] == ["one.clp"]
    assert all(options["timeout"] == 120 for _, options in calls)
    summary = cs.capture(reference_tree, "a" * 40, "run")
    cs.attach_run(summary, "reference", reference_tree / "report.json", code)
    assert summary["runs"]["reference"]["status"] == "passed"


@pytest.mark.parametrize(
    "failure,stage", [("image", "image"), ("version", "version"), ("timeout", "version")]
)
def test_startup_failures_leave_identity_and_failure_artifact(
    reference_tree, monkeypatch, failure, stage
):
    code, report, calls = run_main(reference_tree, monkeypatch, failure)
    assert code == 1 and report["results"] == {}
    assert report["evidence"]["complete"] is False
    assert report["evidence"]["error"]["stage"] == stage
    assert report["evidence"]["revision"] == "a" * 40
    assert report["evidence"]["manifest_sha256"]
    if failure == "timeout":
        assert any(command[1:3] == ["rm", "-f"] for command, _ in calls)


@pytest.mark.parametrize("failure", ["mismatch", "case"])
def test_completed_case_failures_remain_nonzero(reference_tree, monkeypatch, failure):
    code, report, _ = run_main(reference_tree, monkeypatch, failure)
    assert code == 1 and report["evidence"]["complete"] is True
    assert report["results"]["one.clp"]["matches"] is False
    assert report["evidence"]["status"] == "failed"


@pytest.mark.parametrize(
    "options",
    [
        ("--filter", "absent"),
        ("--timeout", "0"),
        ("--timeout", "nan"),
        ("--timeout", "bad"),
        ("--workers", "bad"),
        ("--level", "bad"),
    ],
)
def test_invalid_selection_writes_failure_artifact(reference_tree, monkeypatch, options):
    code, report, calls = run_main(reference_tree, monkeypatch, options=options)
    assert code == 1 and calls == []
    assert report["evidence"]["status"] == "failed"
    assert report["evidence"]["error"]["stage"] == "selection"


def test_malformed_manifest_still_writes_run_identity(reference_tree, monkeypatch):
    (reference_tree / cs.MANIFEST).write_text("not json")
    code, report, calls = run_main(reference_tree, monkeypatch)
    assert code == 1 and calls == []
    assert report["evidence"]["run_id"] == "run"
    assert report["evidence"]["error"]["stage"] == "manifest"


def test_mutated_golden_is_a_real_mismatch(reference_tree, monkeypatch):
    (reference_tree / Path(cs.MANIFEST).parent / "one.out").write_text("changed\n")
    code, report, _ = run_main(reference_tree, monkeypatch)
    assert code == 1
    assert report["results"]["one.clp"] == {"matches": False, "output": "reference\n"}
