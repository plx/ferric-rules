"""Corpus counts are declarations; only matching complete runs verify them."""

import copy
import json
from pathlib import Path

import pytest

from ferric_tools.compat import corpus_summary as cs

REVISION = "a" * 40


@pytest.fixture
def checkout(tmp_path, monkeypatch):
    monkeypatch.setattr(
        cs,
        "checkout_identity",
        lambda root, revision=None: {
            "revision": REVISION,
            "requested_revision": revision,
            "dirty": False,
        },
    )
    directory = tmp_path / Path(cs.MANIFEST).parent
    directory.mkdir(parents=True)
    manifest = {
        "schema_version": 1,
        "reference": {},
        "cases": [
            {"path": "one.clp", "level": "basic", "error": "load"},
            {"path": "two.clp", "level": "boundary", "gap": {"issues": ["issue1"]}},
        ],
    }
    cs.write_json(tmp_path / cs.MANIFEST, manifest)
    for case in manifest["cases"]:
        (directory / case["path"]).write_bytes(b"(defrule r =>)\r\n")
        (directory / case["path"]).with_suffix(".out").write_bytes(b"\xff\r\n")
    return tmp_path


def producer(summary, kind, paths=None):
    paths = paths or list(summary["corpus"]["cases"])
    results = {}
    for path in paths:
        if kind == "reference":
            results[path] = {"matches": True, "output": "oracle"}
        else:
            conforms = summary["corpus"]["cases"][path]["expectation"] == "conformance"
            results[path] = {
                "verdict": "conformance" if conforms else "known_gap",
                "conforms": conforms,
                "accepted": True,
            }
    return {
        "schema": "ferric.compat-corpus-status"
        if kind == "ferric"
        else "ferric.compat-corpus-reference",
        "version": 1,
        "run_id": summary["run_id"],
        "revision": REVISION,
        "manifest_sha256": summary["corpus"]["manifest_sha256"],
        "files_sha256": summary["corpus"]["files_sha256"],
        "scope": "characterization",
        "status": "passed",
        "complete": True,
        "selection": {"filter": "one" if len(paths) == 1 else "", "level": None, "paths": paths},
        "results": results,
        "provenance": {"image_id": "sha256:" + "b" * 64, "version": "CLIPS (6.30 3/17/15)"},
    }


def attach(summary, kind, directory, evidence=None, code=0):
    evidence = evidence or producer(summary, kind)
    report = directory / f"{kind}.json"
    status = directory / "status.json"
    if kind == "reference":
        cs.write_json(report, {"evidence": evidence, "results": evidence["results"]})
    else:
        cs.write_json(report, {path: {"phase": "complete"} for path in evidence["results"]})
        evidence["observations_sha256"] = cs.digest(report.read_bytes())
        cs.write_json(status, evidence)
    cs.attach_run(summary, kind, report, code, status)


def test_counts_and_byte_identity(checkout):
    summary = cs.capture(checkout, REVISION, "run")
    assert cs.corpus_counts(summary) == {
        "total": 2,
        "conformance": 1,
        "gaps": 1,
        "expected_errors": {"load": 1, "run": 0},
    }
    assert cs.corpus_verification(summary)["status"] == "unavailable"
    source = checkout / Path(cs.MANIFEST).parent / "one.clp"
    before = summary["corpus"]["cases"]["one.clp"]
    source.with_suffix(".in").write_bytes(b"")
    after = cs.capture(checkout)["corpus"]["cases"]["one.clp"]
    assert before["scenario_sha256"] != after["scenario_sha256"]
    assert before["golden_sha256"] == cs.digest(b"\xff\r\n")


def test_default_settings_and_reference_stamp_do_not_change_scenario(checkout):
    before = cs.capture(checkout)
    path = checkout / cs.MANIFEST
    manifest = json.loads(path.read_text())
    manifest["reference"]["verified_on"] = "later"
    manifest["cases"][0].update(resets=1, strategy="depth", **dict.fromkeys(cs.NOTICE_FLAGS, False))
    cs.write_json(path, manifest)
    after = cs.capture(checkout)
    assert before["corpus"]["cases"] == after["corpus"]["cases"]
    assert before["corpus"]["manifest_sha256"] != after["corpus"]["manifest_sha256"]


def _edit_case(root, **fields):
    path = root / cs.MANIFEST
    manifest = json.loads(path.read_text())
    manifest["cases"][1].update(fields)
    manifest["cases"][1] = {k: v for k, v in manifest["cases"][1].items() if v is not None}
    cs.write_json(path, manifest)


@pytest.mark.parametrize("flag", cs.NOTICE_FLAGS)
def test_notice_flags_are_part_of_the_scenario(checkout, flag):
    base = cs.capture(checkout)
    _edit_case(checkout, **{flag: False})
    assert cs.capture(checkout)["corpus"]["cases"] == base["corpus"]["cases"]
    _edit_case(checkout, **{flag: True})
    flipped = cs.capture(checkout)
    assert (
        flipped["corpus"]["cases"]["two.clp"]["scenario_sha256"]
        != base["corpus"]["cases"]["two.clp"]["scenario_sha256"]
    )
    # Allowing a notice while dropping the gap is a changed scenario, never a fix.
    _edit_case(checkout, gap=None)
    delta = cs.corpus_delta(base, cs.capture(checkout))
    assert delta["changed_scenarios"] == ["two.clp"]
    assert delta["fixed_gaps"] == []


@pytest.mark.parametrize("flag", cs.NOTICE_FLAGS)
def test_notice_flags_must_be_booleans(checkout, flag):
    _edit_case(checkout, **{flag: 1})
    with pytest.raises(ValueError, match=flag):
        cs.capture(checkout)


def test_unknown_case_fields_are_rejected(checkout):
    _edit_case(checkout, recoverable_future_notices=True)
    with pytest.raises(ValueError, match="unknown corpus case fields"):
        cs.capture(checkout)


def test_real_manifest_is_captured():
    assert cs.corpus_identity(cs.repo_root())["declared"]["total"] > 0


@pytest.mark.parametrize(
    "path", ["../one.clp", "/one.clp", "a//one.clp", "a/./one.clp", "a\\one.clp"]
)
def test_rejects_unsafe_paths(checkout, path):
    manifest = json.loads((checkout / cs.MANIFEST).read_text())
    manifest["cases"][0]["path"] = path
    cs.write_json(checkout / cs.MANIFEST, manifest)
    with pytest.raises(ValueError, match="unsafe"):
        cs.capture(checkout)


def test_full_matching_evidence_verifies_and_roundtrips(checkout):
    summary = cs.capture(checkout, REVISION, "run")
    attach(summary, "ferric", checkout)
    attach(summary, "reference", checkout)
    assert cs.corpus_verification(summary)["status"] == "verified"
    assert summary["runs"]["ferric"]["accepted"] == 2
    assert cs.corpus_counts(summary)["conformance"] == 1
    path = checkout / "summary.json"
    cs.write_json(path, summary)
    assert cs.load_summary(path) == summary


@pytest.mark.parametrize(
    "mutation", ["id", "revision", "manifest", "extra", "missing", "duplicate", "verdict", "image"]
)
def test_stale_or_invalid_evidence_cannot_verify(checkout, mutation):
    summary = cs.capture(checkout, REVISION, "run")
    kind = "reference" if mutation == "image" else "ferric"
    evidence = producer(summary, kind)
    if mutation == "id":
        evidence["run_id"] = "old"
    elif mutation == "revision":
        evidence["revision"] = "c" * 40
    elif mutation == "manifest":
        evidence["manifest_sha256"] = "c" * 64
    elif mutation == "extra":
        evidence["results"]["extra.clp"] = evidence["results"]["one.clp"]
    elif mutation == "missing":
        del evidence["results"]["one.clp"]
    elif mutation == "duplicate":
        evidence["selection"]["paths"].append("one.clp")
    elif mutation == "verdict":
        evidence["results"]["two.clp"]["verdict"] = "conformance"
    elif mutation == "image":
        evidence["provenance"]["image_id"] = "mutable:tag"
    attach(summary, kind, checkout, evidence)
    assert summary["runs"][kind]["status"] != "passed"


def test_partial_or_failed_process_is_not_full_verification(checkout):
    summary = cs.capture(checkout, REVISION, "run")
    attach(summary, "reference", checkout)
    attach(summary, "ferric", checkout, producer(summary, "ferric", ["one.clp"]))
    assert cs.corpus_verification(summary)["status"] == "partial"
    attach(summary, "ferric", checkout, code=101)
    assert cs.corpus_verification(summary)["status"] == "failed"


def test_missing_or_old_reports_remain_explicitly_incomplete(checkout):
    summary = cs.capture(checkout)
    cs.attach_run(summary, "ferric", checkout / "absent.json", 101)
    assert summary["runs"]["ferric"]["status"] == "failed"
    cs.write_json(checkout / "raw.json", {"one.clp": {"phase": "complete"}})
    cs.attach_run(summary, "ferric", checkout / "raw.json", 0)
    assert summary["runs"]["ferric"]["status"] == "incomplete"


def test_summary_cannot_forge_counts_or_full_selection(checkout):
    summary = cs.capture(checkout)
    attach(summary, "ferric", checkout, producer(summary, "ferric", ["one.clp"]))
    path = checkout / "summary.json"
    summary["runs"]["ferric"]["selection"]["full"] = True
    cs.write_json(path, summary)
    with pytest.raises(ValueError, match="selection"):
        cs.load_summary(path)
    summary["runs"]["ferric"] = None
    summary["corpus"]["declared"]["conformance"] = 2
    cs.write_json(path, summary)
    with pytest.raises(ValueError, match="counts"):
        cs.load_summary(path)


def test_delta_separates_fixes_from_removal_and_changed_oracles(checkout):
    base = cs.capture(checkout)
    head = copy.deepcopy(base)
    head["corpus"]["cases"]["two.clp"]["expectation"] = "conformance"
    delta = cs.corpus_delta(base, head)
    assert delta["delta"] == {"total": 0, "conformance": 1, "gaps": -1}
    assert delta["fixed_gaps"] == [{"path": "two.clp", "verified": False}]
    head["corpus"]["cases"]["two.clp"]["golden_sha256"] = "c" * 64
    assert cs.corpus_delta(base, head)["fixed_gaps"] == []
    assert cs.corpus_delta(base, head)["changed_goldens"] == ["two.clp"]
    del head["corpus"]["cases"]["two.clp"]
    head["corpus"]["cases"]["new.clp"] = copy.deepcopy(head["corpus"]["cases"]["one.clp"])
    delta = cs.corpus_delta(base, head)
    assert delta["removed"] == ["two.clp"] and delta["added"] == ["new.clp"]
    assert delta["fixed_gaps"] == []


def test_update_writes_failure_before_require_verified_exit(checkout, monkeypatch):
    path = checkout / "summary.json"
    cs.write_json(path, cs.capture(checkout, REVISION, "run"))
    monkeypatch.setattr(
        "sys.argv",
        [
            "summary",
            "--update",
            str(path),
            "--root",
            str(checkout),
            "--ferric-report",
            str(checkout / "missing"),
            "--ferric-exit-code",
            "101",
            "--require-verified",
        ],
    )
    with pytest.raises(SystemExit):
        cs.main()
    assert cs.load_summary(path)["runs"]["ferric"]["status"] == "failed"


def test_missing_exit_code_preserves_results_without_assuming_success(checkout):
    summary = cs.capture(checkout, REVISION, "run")
    attach(summary, "ferric", checkout, code=None)
    assert summary["runs"]["ferric"]["status"] == "incomplete"
    assert summary["runs"]["ferric"]["accepted"] == 2
    assert summary["runs"]["ferric"]["exit_code"] is None


def test_old_file_contents_and_corrupted_observations_cannot_verify(checkout):
    summary = cs.capture(checkout, REVISION, "run")
    evidence = producer(summary, "ferric")
    (checkout / Path(cs.MANIFEST).parent / "one.out").write_text("new oracle")
    changed = cs.capture(checkout, REVISION, "run")
    attach(changed, "ferric", checkout, evidence)
    assert changed["runs"]["ferric"]["status"] != "passed"
    attach(summary, "ferric", checkout)
    (checkout / "ferric.json").write_text('{"one.clp": null,"two.clp": null}')
    cs.attach_run(summary, "ferric", checkout / "ferric.json", 0, checkout / "status.json")
    assert "digest" in summary["runs"]["ferric"]["error"]["message"]


def test_failing_verdicts_are_retained_without_becoming_conformance(checkout):
    summary = cs.capture(checkout, REVISION, "run")
    evidence = producer(summary, "ferric")
    evidence["results"]["one.clp"] = {"verdict": "mismatch", "conforms": False, "accepted": False}
    evidence["status"] = "failed"
    attach(summary, "ferric", checkout, evidence, 101)
    assert summary["runs"]["ferric"]["failures"] == ["one.clp"]
    assert summary["runs"]["ferric"]["accepted"] == 1


@pytest.mark.parametrize(
    "field,value",
    [
        ("selection", "bad"),
        ("error", "bad"),
        ("accepted", True),
        ("results", []),
        ("failures", "one.clp"),
    ],
)
def test_failed_and_incomplete_records_are_safe_to_render(checkout, field, value):
    summary = cs.capture(checkout)
    summary["runs"]["ferric"] = cs.unavailable_run("interrupted")
    summary["runs"]["ferric"][field] = value
    path = checkout / "summary.json"
    cs.write_json(path, summary)
    with pytest.raises(ValueError):
        cs.load_summary(path)


def test_reset_runs_rejects_wrong_id_then_clears_previous_results(checkout, monkeypatch):
    summary = cs.capture(checkout, REVISION, "run")
    summary["runs"]["ferric"] = cs.unavailable_run("previous attempt", 1)
    path = checkout / "summary.json"
    cs.write_json(path, summary)
    args = ["summary", "--update", str(path), "--root", str(checkout), "--reset-runs", "--run-id"]
    monkeypatch.setattr("sys.argv", [*args, "wrong"])
    with pytest.raises(SystemExit):
        cs.main()
    assert cs.load_summary(path)["runs"]["ferric"] is not None
    monkeypatch.setattr("sys.argv", [*args, "run"])
    cs.main()
    assert cs.load_summary(path)["runs"] == {"ferric": None, "reference": None}
