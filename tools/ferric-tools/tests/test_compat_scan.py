"""Tests for ferric_tools.compat.scan.

Covers classify_file(), which pre-classifies a CLIPS file based on
detected features and the path's file extension.

classify_file(path, features, unsupported) returns
  (classification: str, reason: str, runability: str).
"""

from __future__ import annotations

import copy
from pathlib import Path

import pytest
from typer.testing import CliRunner

from ferric_tools._harness import (
    HARNESS_GENERATION_VERSION,
    HarnessContractError,
    atomic_write_bytes,
    build_harness_plans,
    resolve_harness_contract,
    sha256_bytes,
)
from ferric_tools._manifest import load_manifest, save_manifest
from ferric_tools.bat import harness as harness_module
from ferric_tools.compat import run as run_module
from ferric_tools.compat.scan import build_summary, classify_file, scan_examples

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _clp(name: str = "example.clp") -> Path:
    """Return a synthetic .clp Path."""
    return Path(name)


def _materialized_library_harness(tmp_path):
    root = tmp_path / "repo"
    examples = root / "tests" / "examples"
    source = examples / "libraries" / "facts.clp"
    source.parent.mkdir(parents=True)
    source.write_text("(deffacts sample (value 1))\n", encoding="utf-8")
    files = scan_examples(examples, root=root)
    plans = build_harness_plans(
        files,
        examples_dir=examples,
        output_dir=root / "tests" / "harnesses",
        root=root,
    )
    plan = plans["libraries/facts.clp"]
    assert plan.harness_path is not None
    assert plan.harness_bytes is not None
    atomic_write_bytes(plan.harness_path, plan.harness_bytes)
    return root, source, files["libraries/facts.clp"], plan


# ---------------------------------------------------------------------------
# COOL constructs → incompatible
# ---------------------------------------------------------------------------


def test_classify_file_cool_construct_is_incompatible():
    # A file containing COOL constructs (e.g. defclass) is classified
    # "incompatible" because ferric does not support COOL.
    path = _clp("cool_example.clp")
    features = ["defclass", "defrule"]
    unsupported = ["defclass"]

    classification, reason, _runability = classify_file(path, features, unsupported)

    assert classification == "incompatible"
    assert reason == "unsupported-form"


# ---------------------------------------------------------------------------
# Interactive I/O → incompatible
# ---------------------------------------------------------------------------


def test_classify_file_interactive_io_is_incompatible():
    # Files that use (read) or (readline) require an interactive terminal;
    # they are classified "incompatible" with runability "interactive".
    path = _clp("interactive.clp")
    features = ["defrule"]
    unsupported = ["read"]

    classification, reason, runability = classify_file(path, features, unsupported)

    assert classification == "incompatible"
    assert reason == "interactive"
    assert runability == "interactive"


# ---------------------------------------------------------------------------
# Supported constructs only → pending / testable
# ---------------------------------------------------------------------------


def test_classify_file_supported_constructs_with_defrule_is_pending_testable():
    # A file that uses only supported constructs AND has at least one defrule
    # is classified "pending" with reason "testable".
    path = _clp("simple_rule.clp")
    features = ["defrule", "deftemplate"]
    unsupported = []

    classification, reason, runability = classify_file(path, features, unsupported)

    assert classification == "pending"
    assert reason == "testable"
    assert runability == "standalone"


def test_classify_file_no_defrule_is_library_only():
    # A file without any defrule is a library/setup file, not directly
    # testable as a standalone scenario.
    path = _clp("library.clp")
    features = ["deftemplate", "deffacts"]
    unsupported = []

    classification, reason, _runability = classify_file(path, features, unsupported)

    assert classification == "pending"
    assert reason == "library-only"


# ---------------------------------------------------------------------------
# File I/O → incompatible
# ---------------------------------------------------------------------------


def test_classify_file_open_io_is_incompatible():
    # Files using (open ...) for file I/O are incompatible.
    path = _clp("file_io.clp")
    features = ["defrule"]
    unsupported = ["open"]

    classification, reason, _runability = classify_file(path, features, unsupported)

    assert classification == "incompatible"
    assert reason == "unsupported-io"


# ---------------------------------------------------------------------------
# .bat extension → always incompatible
# ---------------------------------------------------------------------------


def test_classify_file_bat_extension_is_incompatible():
    # .bat files are CLIPS test-suite batch files; they are always classified
    # "incompatible" regardless of their feature set.
    path = Path("testfile.bat")
    features = []
    unsupported = []

    classification, reason, _runability = classify_file(path, features, unsupported)

    assert classification == "incompatible"
    assert reason == "test-suite-batch"


def test_harness_generation_attaches_structured_manifest_contract(tmp_path, monkeypatch):
    root = tmp_path / "repo"
    examples = root / "tests" / "examples"
    source = examples / "libraries" / "facts.clp"
    source.parent.mkdir(parents=True)
    source.write_text("(deffacts sample (value 1))\n", encoding="utf-8")

    files = scan_examples(examples)
    manifest_path = examples / "compat-manifest.json"
    save_manifest(
        manifest_path,
        {
            "version": 1,
            "generated": "2026-07-26T00:00:00+00:00",
            "summary": build_summary(files),
            "files": files,
        },
    )
    monkeypatch.setattr(harness_module, "repo_root", lambda: root)

    result = CliRunner().invoke(
        harness_module.app,
        [
            "--manifest",
            str(manifest_path),
            "--output-dir",
            str(root / "tests" / "harnesses"),
        ],
    )

    assert result.exit_code == 0, result.output
    entry = load_manifest(manifest_path)["files"]["libraries/facts.clp"]
    assert entry["harness"] == {
        "path": "tests/harnesses/libraries/facts-harness.clp",
        "source_sha256": ("9cbd4ed905513641a466371ad9acd658ef729cd071739ad84340b30273cfa088"),
        "harness_sha256": ("813e26ca00f6dd8f89322aca61d1ef07441795aae6fe58dc653e69f969693e57"),
        "generation_version": 1,
        "executable": True,
    }
    assert load_manifest(manifest_path)["version"] == 2


def test_scan_attaches_non_executable_contract_for_empty_library(tmp_path):
    root = tmp_path / "repo"
    examples = root / "tests" / "examples"
    source = examples / "empty.clp"
    source.parent.mkdir(parents=True)
    source.write_text("; comments only\n", encoding="utf-8")

    entry = scan_examples(examples, root=root)["empty.clp"]

    assert entry["runability"] == "library"
    assert entry["harness"] == {
        "path": None,
        "source_sha256": sha256_bytes(b"; comments only\n"),
        "harness_sha256": None,
        "generation_version": HARNESS_GENERATION_VERSION,
        "executable": False,
        "skip_reason": "empty",
    }


def test_harness_generation_is_deterministic(tmp_path, monkeypatch):
    root = tmp_path / "repo"
    examples = root / "tests" / "examples"
    source = examples / "library.clp"
    source.parent.mkdir(parents=True)
    source.write_text("(deftemplate item (slot value))\n", encoding="utf-8")
    files = scan_examples(examples, root=root)
    manifest_path = examples / "compat-manifest.json"
    save_manifest(
        manifest_path,
        {
            "version": 2,
            "generated": "2026-07-26T00:00:00+00:00",
            "summary": build_summary(files),
            "files": files,
        },
    )
    monkeypatch.setattr(harness_module, "repo_root", lambda: root)
    args = [
        "--manifest",
        str(manifest_path),
        "--output-dir",
        str(root / "tests" / "harnesses"),
    ]

    first = CliRunner().invoke(harness_module.app, args)
    assert first.exit_code == 0, first.output
    first_manifest = manifest_path.read_bytes()
    harness_path = root / "tests" / "harnesses" / "library-harness.clp"
    first_harness = harness_path.read_bytes()

    second = CliRunner().invoke(harness_module.app, args)

    assert second.exit_code == 0, second.output
    assert manifest_path.read_bytes() == first_manifest
    assert harness_path.read_bytes() == first_harness


def test_harness_generation_uses_stable_library_identity(tmp_path, monkeypatch):
    root = tmp_path / "repo"
    examples = root / "tests" / "examples"
    source = examples / "library.clp"
    source.parent.mkdir(parents=True)
    source.write_text("(deffacts sample (value 1))\n", encoding="utf-8")
    files = scan_examples(examples, root=root)
    files["library.clp"]["reason"] = "exact-match"
    files["library.clp"]["harness"] = {"legacy": True}
    manifest_path = examples / "compat-manifest.json"
    save_manifest(
        manifest_path,
        {
            "version": 1,
            "generated": "2026-07-26T00:00:00+00:00",
            "summary": build_summary(files),
            "files": files,
        },
    )
    monkeypatch.setattr(harness_module, "repo_root", lambda: root)

    result = CliRunner().invoke(
        harness_module.app,
        [
            "--manifest",
            str(manifest_path),
            "--output-dir",
            str(root / "tests" / "harnesses"),
        ],
    )

    assert result.exit_code == 0, result.output
    entry = load_manifest(manifest_path)["files"]["library.clp"]
    assert entry["reason"] == "exact-match"
    assert entry["harness"]["executable"] is True


def test_harness_generation_clears_stale_mapping_when_fixture_becomes_empty(tmp_path, monkeypatch):
    root, source, entry, plan = _materialized_library_harness(tmp_path)
    examples = root / "tests" / "examples"
    manifest_path = examples / "compat-manifest.json"
    save_manifest(
        manifest_path,
        {
            "version": 2,
            "generated": "2026-07-26T00:00:00+00:00",
            "summary": build_summary({"libraries/facts.clp": entry}),
            "files": {"libraries/facts.clp": entry},
        },
    )
    source.write_text("; no constructs remain\n", encoding="utf-8")
    monkeypatch.setattr(harness_module, "repo_root", lambda: root)

    result = CliRunner().invoke(
        harness_module.app,
        [
            "--manifest",
            str(manifest_path),
            "--output-dir",
            str(root / "tests" / "harnesses"),
        ],
    )

    assert result.exit_code == 0, result.output
    contract = load_manifest(manifest_path)["files"]["libraries/facts.clp"]["harness"]
    assert contract["executable"] is False
    assert contract["path"] is None
    assert contract["harness_sha256"] is None
    assert contract["skip_reason"] == "empty"
    assert plan.harness_path is not None


def test_generation_rejects_duplicate_output_mapping_before_writing(tmp_path):
    root = tmp_path / "repo"
    examples = root / "tests" / "examples"
    examples.mkdir(parents=True)
    content = "(deffacts sample (value 1))\n"
    (examples / "same.clp").write_text(content, encoding="utf-8")
    (examples / "same.bat").write_text(content, encoding="utf-8")
    files = {
        "same.clp": {"runability": "library"},
        "same.bat": {"runability": "library"},
    }

    with pytest.raises(HarnessContractError, match="duplicate harness mapping"):
        build_harness_plans(
            files,
            examples_dir=examples,
            output_dir=root / "tests" / "harnesses",
            root=root,
        )

    assert not (root / "tests" / "harnesses").exists()


def test_generation_rejects_harness_leaf_symlink(tmp_path):
    root = tmp_path / "repo"
    examples = root / "tests" / "examples"
    source = examples / "library.clp"
    source.parent.mkdir(parents=True)
    source.write_text("(deffacts sample (value 1))\n", encoding="utf-8")
    output_dir = root / "tests" / "harnesses"
    output_dir.mkdir(parents=True)
    other_target = output_dir / "other.clp"
    other_target.write_text("; old target\n", encoding="utf-8")
    (output_dir / "library-harness.clp").symlink_to(other_target)

    with pytest.raises(HarnessContractError, match="must not be a symlink"):
        build_harness_plans(
            {"library.clp": {"runability": "library"}},
            examples_dir=examples,
            output_dir=output_dir,
            root=root,
        )

    assert other_target.read_text(encoding="utf-8") == "; old target\n"


def test_generation_failure_does_not_publish_updated_manifest(tmp_path, monkeypatch):
    root = tmp_path / "repo"
    examples = root / "tests" / "examples"
    source = examples / "library.clp"
    source.parent.mkdir(parents=True)
    source.write_text("(deffacts sample (value 1))\n", encoding="utf-8")
    files = scan_examples(examples, root=root)
    manifest_path = examples / "compat-manifest.json"
    old_manifest = {
        "version": 1,
        "generated": "2026-07-26T00:00:00+00:00",
        "summary": build_summary(files),
        "files": files,
    }
    save_manifest(manifest_path, old_manifest)
    old_bytes = manifest_path.read_bytes()
    monkeypatch.setattr(harness_module, "repo_root", lambda: root)

    def fail_write(_path, _content):
        raise OSError("injected harness replacement failure")

    monkeypatch.setattr(harness_module, "atomic_write_bytes", fail_write)

    result = CliRunner().invoke(
        harness_module.app,
        [
            "--manifest",
            str(manifest_path),
            "--output-dir",
            str(root / "tests" / "harnesses"),
        ],
    )

    assert result.exit_code == 1
    assert isinstance(result.exception, OSError)
    assert manifest_path.read_bytes() == old_bytes
    assert load_manifest(manifest_path) == old_manifest


def test_resolver_accepts_digest_matched_harness(tmp_path):
    root, source, entry, plan = _materialized_library_harness(tmp_path)

    resolved = resolve_harness_contract(
        entry,
        source_path=source,
        root=root,
        manifest_key="libraries/facts.clp",
    )

    assert resolved is not None
    assert resolved.path == plan.harness_path.resolve()
    assert resolved.source_bytes == source.read_bytes()
    assert resolved.harness_bytes == plan.harness_bytes
    assert resolved.metadata == entry["harness"]


@pytest.mark.parametrize(
    ("mutation", "message"),
    [
        ("source", "source digest is stale"),
        ("harness", "harness digest is stale"),
        ("missing", "does not exist"),
    ],
)
def test_resolver_rejects_stale_or_missing_harness_contract(tmp_path, mutation, message):
    root, source, entry, plan = _materialized_library_harness(tmp_path)
    assert plan.harness_path is not None
    if mutation == "source":
        source.write_text("(deffacts changed (value 2))\n", encoding="utf-8")
    elif mutation == "harness":
        plan.harness_path.write_text("; stale harness\n", encoding="utf-8")
    else:
        plan.harness_path.unlink()

    with pytest.raises(HarnessContractError, match=message):
        resolve_harness_contract(
            entry,
            source_path=source,
            root=root,
            manifest_key="libraries/facts.clp",
        )


@pytest.mark.parametrize(
    "escaping_path",
    [
        "/tmp/outside-harness.clp",
        "../outside-harness.clp",
        "tests/harnesses/../outside-harness.clp",
        "C:/outside-harness.clp",
    ],
)
def test_resolver_rejects_escaping_or_non_normalized_paths(tmp_path, escaping_path):
    root, source, entry, _plan = _materialized_library_harness(tmp_path)
    escaped_entry = copy.deepcopy(entry)
    escaped_entry["harness"]["path"] = escaping_path

    with pytest.raises(HarnessContractError, match="harness path"):
        resolve_harness_contract(
            escaped_entry,
            source_path=source,
            root=root,
            manifest_key="libraries/facts.clp",
        )


def test_resolver_rejects_symlink_escape(tmp_path):
    root, source, entry, _plan = _materialized_library_harness(tmp_path)
    outside_harness = tmp_path / "outside-harness.clp"
    outside_harness.write_text("; outside\n", encoding="utf-8")
    symlink = root / "tests" / "harnesses" / "escape.clp"
    symlink.symlink_to(outside_harness)
    escaped_entry = copy.deepcopy(entry)
    escaped_entry["harness"]["path"] = "tests/harnesses/escape.clp"
    escaped_entry["harness"]["harness_sha256"] = sha256_bytes(outside_harness.read_bytes())

    with pytest.raises(HarnessContractError, match="escapes repository root"):
        resolve_harness_contract(
            escaped_entry,
            source_path=source,
            root=root,
            manifest_key="libraries/facts.clp",
        )


def test_resolver_rejects_missing_contract_and_unknown_generation(tmp_path):
    root, source, entry, _plan = _materialized_library_harness(tmp_path)
    missing_contract = copy.deepcopy(entry)
    missing_contract.pop("harness")
    unknown_generation = copy.deepcopy(entry)
    unknown_generation["harness"]["generation_version"] = HARNESS_GENERATION_VERSION + 1

    with pytest.raises(HarnessContractError, match="missing structured harness"):
        resolve_harness_contract(
            missing_contract,
            source_path=source,
            root=root,
            manifest_key="libraries/facts.clp",
        )
    with pytest.raises(HarnessContractError, match="unsupported harness generation version"):
        resolve_harness_contract(
            unknown_generation,
            source_path=source,
            root=root,
            manifest_key="libraries/facts.clp",
        )


def test_compat_runner_rejects_duplicate_selected_harness_mapping(tmp_path, monkeypatch):
    root = tmp_path / "repo"
    examples = root / "tests" / "examples"
    examples.mkdir(parents=True)
    content = "(deffacts sample (value 1))\n"
    (examples / "a.clp").write_text(content, encoding="utf-8")
    (examples / "b.clp").write_text(content, encoding="utf-8")
    files = scan_examples(examples, root=root)
    plans = build_harness_plans(
        files,
        examples_dir=examples,
        output_dir=root / "tests" / "harnesses",
        root=root,
    )
    first_plan = plans["a.clp"]
    assert first_plan.harness_path is not None
    assert first_plan.harness_bytes is not None
    atomic_write_bytes(first_plan.harness_path, first_plan.harness_bytes)
    files["b.clp"]["harness"] = copy.deepcopy(files["a.clp"]["harness"])
    manifest_path = examples / "compat-manifest.json"
    save_manifest(
        manifest_path,
        {
            "version": 2,
            "generated": "2026-07-26T00:00:00+00:00",
            "summary": build_summary(files),
            "files": files,
        },
    )
    monkeypatch.setattr(run_module, "repo_root", lambda: root)
    monkeypatch.setattr(run_module, "default_examples_dir", lambda: examples)

    result = CliRunner().invoke(
        run_module.app,
        ["--manifest", str(manifest_path), "--dry-run"],
    )

    assert result.exit_code == 1
    assert "duplicate harness mapping" in result.output


def test_compat_runner_skips_explicitly_non_executable_library(tmp_path, monkeypatch):
    root = tmp_path / "repo"
    examples = root / "tests" / "examples"
    source = examples / "empty.clp"
    source.parent.mkdir(parents=True)
    source.write_text("; comments only\n", encoding="utf-8")
    files = scan_examples(examples, root=root)
    manifest_path = examples / "compat-manifest.json"
    save_manifest(
        manifest_path,
        {
            "version": 2,
            "generated": "2026-07-26T00:00:00+00:00",
            "summary": build_summary(files),
            "files": files,
        },
    )
    monkeypatch.setattr(run_module, "repo_root", lambda: root)
    monkeypatch.setattr(run_module, "default_examples_dir", lambda: examples)

    result = CliRunner().invoke(
        run_module.app,
        ["--manifest", str(manifest_path), "--dry-run"],
    )
    explicit = CliRunner().invoke(
        run_module.app,
        ["--manifest", str(manifest_path), "--file", "empty.clp", "--dry-run"],
    )

    assert result.exit_code == 0
    assert "No files to run." in result.output
    assert explicit.exit_code == 1
    assert "harness is not executable (empty)" in explicit.output


@pytest.mark.parametrize("mutation", ["missing", "standalone"])
def test_compat_runner_rejects_runability_that_bypasses_harness_validation(
    tmp_path, monkeypatch, mutation
):
    root, _source, entry, plan = _materialized_library_harness(tmp_path)
    examples = root / "tests" / "examples"
    malformed_entry = copy.deepcopy(entry)
    if mutation == "missing":
        malformed_entry.pop("runability")
    else:
        malformed_entry["runability"] = "standalone"
    manifest_path = examples / "compat-manifest.json"
    save_manifest(
        manifest_path,
        {
            "version": 2,
            "generated": "2026-07-26T00:00:00+00:00",
            "summary": build_summary({"libraries/facts.clp": malformed_entry}),
            "files": {"libraries/facts.clp": malformed_entry},
        },
    )
    monkeypatch.setattr(run_module, "repo_root", lambda: root)
    monkeypatch.setattr(run_module, "default_examples_dir", lambda: examples)

    result = CliRunner().invoke(
        run_module.app,
        ["--manifest", str(manifest_path), "--dry-run"],
    )

    assert result.exit_code == 1
    if mutation == "missing":
        assert "invalid or missing runability" in result.output
    else:
        assert "harness contract requires library runability" in result.output
    assert plan.harness_path is not None


def test_process_file_records_harness_executed_by_both_engines(tmp_path, monkeypatch):
    root, source, entry, _plan = _materialized_library_harness(tmp_path)
    resolved = resolve_harness_contract(
        entry,
        source_path=source,
        root=root,
        manifest_key="libraries/facts.clp",
    )
    assert resolved is not None
    invocations: list[tuple[str, str, bytes]] = []

    def fake_ferric(path, _ferric, _timeout):
        invocations.append(("ferric", path, Path(path).read_bytes()))
        return {
            "exit_code": 0,
            "stdout": "matched\n",
            "stderr": "",
            "duration_ms": 1,
            "timed_out": False,
        }

    def fake_clips(path, _root, _script, _timeout):
        invocations.append(("clips", path, Path(path).read_bytes()))
        return {
            "exit_code": 0,
            "stdout": "matched\n",
            "stderr": "",
            "duration_ms": 1,
            "timed_out": False,
        }

    monkeypatch.setattr(run_module, "run_ferric", fake_ferric)
    monkeypatch.setattr(run_module, "run_clips_docker", fake_clips)

    result = run_module.process_file(
        (
            "libraries/facts.clp",
            str(source),
            "ferric",
            str(root),
            "clips-reference",
            5,
            False,
            resolved,
        )
    )

    _, ferric_result, clips_result, classification, reason = result
    assert [engine for engine, _, _ in invocations] == ["ferric", "clips"]
    assert invocations[0][1] == invocations[1][1]
    assert invocations[0][2] == invocations[1][2]
    assert invocations[0][2] == resolved.source_bytes + b"\n" + resolved.harness_bytes
    assert ferric_result["harness"] == resolved.metadata
    assert clips_result is not None
    assert clips_result["harness"] == resolved.metadata
    assert classification == "equivalent"
    assert reason == "exact-match"
