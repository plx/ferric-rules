"""Tests for ferric_tools.compat.scan.

Covers classify_file(), which pre-classifies a CLIPS file based on
detected features and the path's file extension.

classify_file(path, features, unsupported) returns
  (classification: str, reason: str, runability: str).
"""

from __future__ import annotations

import copy
import json
import re
import subprocess
from pathlib import Path

import pytest
from typer.testing import CliRunner

from ferric_tools import _harness as harness_core
from ferric_tools._harness import (
    HARNESS_GENERATION_VERSION,
    HarnessContractError,
    atomic_write_bytes,
    build_harness_plans,
    resolve_harness_contract,
    sha256_bytes,
)
from ferric_tools._manifest import load_manifest, save_manifest
from ferric_tools._paths import repo_root
from ferric_tools.bat import harness as harness_module
from ferric_tools.compat import run as run_module
from ferric_tools.compat.oracle import canonical_scenario_plan
from ferric_tools.compat.scan import (
    OracleRegistryError,
    build_summary,
    classify_file,
    scan_examples,
)

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _clp(name: str = "example.clp") -> Path:
    """Return a synthetic .clp Path."""
    return Path(name)


def _span_text(source: str, span: dict) -> str:
    """Slice source using one serialized byte-oriented feature-scan span."""
    encoded = source.encode("utf-8")
    return encoded[span["start_byte"] : span["end_byte"]].decode("utf-8")


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


def _generated_harness_for_source(
    tmp_path,
    source_text: str,
    manifest_key: str = "fixture.clp",
) -> str:
    root = tmp_path / "repo"
    examples = root / "tests" / "examples"
    source = examples.joinpath(*Path(manifest_key).parts)
    source.parent.mkdir(parents=True, exist_ok=True)
    source.write_text(source_text, encoding="utf-8")
    plans = build_harness_plans(
        {manifest_key: {"runability": "library"}},
        examples_dir=examples,
        output_dir=root / "tests" / "harnesses",
        root=root,
    )
    harness_bytes = plans[manifest_key].harness_bytes
    assert harness_bytes is not None
    return harness_bytes.decode("utf-8")


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


@pytest.mark.parametrize("suffix", [".clp", ".bat"])
def test_scan_preserves_strict_utf8_read_errors_as_unknown(tmp_path, suffix):
    root = tmp_path / "repo"
    examples = root / "tests" / "examples"
    source = examples / f"invalid-utf8{suffix}"
    source.parent.mkdir(parents=True)
    source.write_bytes(b"(defrule visible =>)\r\n\xff")

    entry = scan_examples(examples, root=root)[source.name]

    assert entry["classification"] == "incompatible"
    assert entry["reason"] == "read-error"
    assert entry["runability"] == "unknown"
    assert entry["features"] == []
    assert entry["unsupported_features"] == []
    assert "utf-8" in entry["notes"]


def test_scan_attaches_string_aware_feature_evidence_and_legacy_aggregates(tmp_path):
    root = tmp_path / "repo"
    examples = root / "tests" / "examples"
    source = examples / "string-aware.clp"
    source.parent.mkdir(parents=True)
    source_text = (
        "; (read) in a comment must not count\r\n"
        "(DeFrUlE safe\r\n"
        "  =>\r\n"
        '  (PrInToUt t "π ; \\"(open fake)\\"" crlf)\r\n'
        "  (assert (batch-mode open-file readline-token)))\r\n"
    )
    source.write_bytes(source_text.encode("utf-8"))

    entry = scan_examples(examples, root=root)[source.name]

    assert entry["classification"] == "pending"
    assert entry["reason"] == "testable"
    assert entry["runability"] == "standalone"
    assert entry["features"] == ["defrule", "printout"]
    assert entry["unsupported_features"] == []
    evidence = entry["feature_scan"]
    assert evidence["version"] == 1
    assert evidence["status"] == "valid"
    assert evidence["issues"] == []
    assert [detection["feature"] for detection in evidence["detections"]] == [
        "defrule",
        "printout",
    ]
    for detection in evidence["detections"]:
        assert detection["category"] in {"supported-construct", "output"}
        assert detection["reason"] in {"supported-form", "supported-output"}
        assert _span_text(source_text, detection["head_span"]).lower() == detection["feature"]
        assert _span_text(source_text, detection["form_span"]).startswith("(")


@pytest.mark.parametrize("suffix", [".clp", ".bat"])
def test_scan_marks_lexically_malformed_source_unknown_with_partial_evidence(
    tmp_path,
    suffix,
):
    root = tmp_path / "repo"
    examples = root / "tests" / "examples"
    source = examples / f"malformed{suffix}"
    source.parent.mkdir(parents=True)
    source_text = '(DeFrUlE seen => (PrInToUt t "ok" crlf))\r\n(OpEn "fixture" logical "r")\r\n)'
    source.write_bytes(source_text.encode("utf-8"))

    entry = scan_examples(examples, root=root)[source.name]

    assert entry["classification"] == "incompatible"
    assert entry["reason"] == "malformed-source"
    assert entry["runability"] == "unknown"
    assert entry["features"] == ["defrule", "printout"]
    assert entry["unsupported_features"] == ["open"]
    evidence = entry["feature_scan"]
    assert evidence["status"] == "invalid"
    assert [detection["feature"] for detection in evidence["detections"]] == [
        "defrule",
        "printout",
        "open",
    ]
    assert evidence["detections"][2]["category"] == "file-io"
    assert evidence["detections"][2]["reason"] == "unsupported-io"
    assert _span_text(source_text, evidence["detections"][2]["head_span"]) == "OpEn"
    assert evidence["issues"] == [
        {
            "kind": "unmatched-close",
            "reason": "unmatched-close",
            "span": {
                "start_byte": len(source_text.encode("utf-8")) - 1,
                "end_byte": len(source_text.encode("utf-8")),
                "start_line": 3,
                "start_column": 1,
                "end_line": 3,
                "end_column": 2,
            },
        }
    ]


@pytest.mark.parametrize(("input_version", "expected_version"), [(1, 2), (3, 3)])
def test_harness_generation_attaches_structured_manifest_contract(
    tmp_path,
    monkeypatch,
    input_version,
    expected_version,
):
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
            "version": input_version,
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
        "harness_sha256": ("688b349b774260e05fe36879a6af9a8bb15af3c835fcf601b56a97b201930ce3"),
        "generation_version": HARNESS_GENERATION_VERSION,
        "executable": True,
    }
    assert load_manifest(manifest_path)["version"] == expected_version


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
            "version": 3,
            "oracle_protocol_version": 1,
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


@pytest.mark.parametrize(
    "source_text",
    [
        "(deffacts sample (value 1))\n(defmodule TRAILING)\n",
        "(defmodule MAIN (export ?ALL))\n"
        "(deffacts MAIN::sample (value 1))\n"
        "(defmodule TRAILING (export ?ALL))\n",
        "(defmodule MAIN)\n(deffacts MAIN::sample (value 1))\n(defmodule TRAILING)\n",
        "(defmodule FERRIC-HARNESS-AUDIT)\n(deffacts FERRIC-HARNESS-AUDIT::sample (value 1))\n",
        "(defmodule MAIN)\n"
        "(defmodule FIRST)\n"
        "(defmodule SECOND)\n"
        "(defrule MAIN::seed-focus (initial-fact) => (focus FIRST SECOND))\n"
        "(defmodule TRAILING)\n",
        "(defmodule TRAILING)\n"
        '(defrule harness-verify (initial-fact) => (printout t "fixture" crlf))\n',
    ],
    ids=[
        "ends-non-main",
        "explicit-exports",
        "no-exports",
        "audit-module-collision",
        "nested-focus-stack",
        "legacy-rule-collision",
    ],
)
def test_generated_verifier_has_isolated_main_execution_proof(
    tmp_path,
    source_text,
):
    harness = _generated_harness_for_source(tmp_path, source_text)
    rule_match = re.search(
        r"\(defrule MAIN::(ferric-harness-[0-9a-f]{64}(?:-[0-9]+)?)-verify\b",
        harness,
    )

    assert rule_match is not None
    verifier_id = rule_match.group(1)
    assert verifier_id.casefold() not in source_text.casefold()
    assert "\n(defmodule " not in harness
    assert "\n(defrule harness-verify" not in harness
    assert "(declare (salience 10000))" in harness

    start = f'"FERRIC-HARNESS|{HARNESS_GENERATION_VERSION}|{verifier_id}|START"'
    state = f'"FERRIC-HARNESS|{HARNESS_GENERATION_VERSION}|{verifier_id}|STATE|focus=" (get-focus)'
    complete = f'"FERRIC-HARNESS|{HARNESS_GENERATION_VERSION}|{verifier_id}|COMPLETE"'
    assert harness.index(start) < harness.index(state) < harness.index(complete)
    assert harness.endswith(
        f"(defrule MAIN::{verifier_id}-verify\n"
        "   (declare (salience 10000))\n"
        "   (initial-fact)\n"
        "   =>\n"
        f"   (printout t {start} crlf)\n"
        f"   (printout t {state} crlf)\n"
        f"   (printout t {complete} crlf))\n"
    )


def test_harness_generation_rejects_control_bearing_manifest_path(tmp_path):
    root = tmp_path / "repo"
    examples = root / "tests" / "examples"
    examples.mkdir(parents=True)

    with pytest.raises(HarnessContractError, match="control characters"):
        build_harness_plans(
            {"fixture.clp\n(defrule injected =>)": {"runability": "library"}},
            examples_dir=examples,
            output_dir=root / "tests" / "harnesses",
            root=root,
        )


def test_generate_harness_rejects_comment_injection_at_sink():
    source_bytes = b"(deffacts sample (value 1))\n"
    constructs = harness_core.detect_constructs(source_bytes.decode())

    with pytest.raises(HarnessContractError, match="control characters"):
        harness_core.generate_harness(
            "fixture.clp\n(defrule MAIN::path-injected =>)\n;",
            source_bytes,
            constructs,
        )

    malicious_constructs = copy.deepcopy(constructs)
    malicious_constructs["deffacts"] = ["sample\n(defrule MAIN::construct-injected =>)\n;"]
    with pytest.raises(HarnessContractError, match="control characters"):
        harness_core.generate_harness(
            "fixture.clp",
            source_bytes,
            malicious_constructs,
        )


def test_generated_verifier_identity_is_deterministic_and_path_scoped(tmp_path):
    source_text = "(deffacts sample (value 1))\n(defmodule TRAILING)\n"

    first = _generated_harness_for_source(tmp_path, source_text, "a/library.clp")
    repeated = _generated_harness_for_source(tmp_path, source_text, "a/library.clp")
    second_path = _generated_harness_for_source(tmp_path, source_text, "b/library.clp")

    pattern = r"\(defrule MAIN::(ferric-harness-[0-9a-f]{64})-verify\b"
    first_id = re.search(pattern, first)
    second_id = re.search(pattern, second_path)
    assert first == repeated
    assert first_id is not None
    assert second_id is not None
    assert first_id.group(1) != second_id.group(1)


def test_generated_verifier_advances_past_forced_name_collisions(monkeypatch):
    digest = "a" * 64
    base = f"ferric-harness-{digest}"
    source_bytes = (
        f"(defmodule {base})\n(defmodule {base}-1)\n(deffacts {base}-1::sample (value 1))\n"
    ).encode()
    source_text = source_bytes.decode()
    monkeypatch.setattr(harness_core, "sha256_bytes", lambda _content: digest)

    harness = harness_core.generate_harness(
        "fixture.clp",
        source_bytes,
        harness_core.detect_constructs(source_text),
    )

    assert f"(defrule MAIN::{base}-2-verify\n" in harness


def test_generated_verifier_executes_after_trailing_module_without_changing_feature_output(
    tmp_path,
):
    source_text = (
        "(defmodule MAIN)\n"
        "(defrule MAIN::feature-result\n"
        "   (declare (salience -10000))\n"
        "   (initial-fact)\n"
        "   =>\n"
        '   (printout t "FEATURE|module=MAIN" crlf))\n'
        "(defmodule TRAILING)\n"
    )
    root = tmp_path / "fixture-repo"
    examples = root / "tests" / "examples"
    source = examples / "trailing.clp"
    source.parent.mkdir(parents=True)
    source.write_text(source_text, encoding="utf-8")
    plan = build_harness_plans(
        {"trailing.clp": {"runability": "library"}},
        examples_dir=examples,
        output_dir=root / "tests" / "harnesses",
        root=root,
    )["trailing.clp"]
    assert plan.harness_bytes is not None
    composed = root / "composed.clp"
    composed.write_bytes(plan.source_bytes + b"\n" + plan.harness_bytes)

    command = ["cargo", "run", "--quiet", "-p", "ferric-rules-cli", "--", "run"]
    baseline = subprocess.run(
        [*command, str(source)],
        cwd=repo_root(),
        capture_output=True,
        text=True,
        timeout=300,
    )
    instrumented = subprocess.run(
        [*command, str(composed)],
        cwd=repo_root(),
        capture_output=True,
        text=True,
        timeout=300,
    )

    assert baseline.returncode == 0, baseline.stderr
    assert instrumented.returncode == 0, instrumented.stderr
    rule_match = re.search(
        rb"\(defrule MAIN::(ferric-harness-[0-9a-f]{64})-verify\b",
        plan.harness_bytes,
    )
    assert rule_match is not None
    verifier_id = rule_match.group(1).decode()
    proof_lines = [
        line for line in instrumented.stdout.splitlines() if line.startswith("FERRIC-HARNESS|")
    ]
    assert proof_lines == [
        f"FERRIC-HARNESS|{HARNESS_GENERATION_VERSION}|{verifier_id}|START",
        (f"FERRIC-HARNESS|{HARNESS_GENERATION_VERSION}|{verifier_id}|STATE|focus=MAIN"),
        f"FERRIC-HARNESS|{HARNESS_GENERATION_VERSION}|{verifier_id}|COMPLETE",
    ]
    feature_lines = [
        line for line in instrumented.stdout.splitlines() if not line.startswith("FERRIC-HARNESS|")
    ]
    assert feature_lines == baseline.stdout.splitlines() == ["FEATURE|module=MAIN"]


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
    files["a.clp"]["oracle"] = {}
    files["b.clp"]["oracle"] = {}
    manifest_path = examples / "compat-manifest.json"
    save_manifest(
        manifest_path,
        {
            "version": 3,
            "oracle_protocol_version": 1,
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


def test_compat_runner_demotes_undeclared_library_and_rejects_explicit_run(
    tmp_path,
    monkeypatch,
):
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
            "version": 3,
            "oracle_protocol_version": 1,
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
    assert "No oracle-backed files to run." in result.output
    assert explicit.exit_code == 1
    assert "no structured oracle declaration" in explicit.output


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
            "version": 3,
            "oracle_protocol_version": 1,
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
    composed = resolved.source_bytes + b"\n" + resolved.harness_bytes
    declaration = _oracle_declaration(
        sha256_bytes(resolved.source_bytes),
        composed_digest=sha256_bytes(composed),
    )

    def fake_ferric(path, _ferric, _root, _timeout, **identity):
        invocations.append(("ferric", path, Path(path).read_bytes()))
        return {
            "exit_code": 0,
            "stdout": "matched\n",
            "stderr": "",
            "duration_ms": 1,
            "timed_out": False,
            "observation": {"identity": identity},
        }

    def fake_clips(
        path,
        _root,
        _script,
        _timeout,
        *,
        globals_to_capture,
        harnessed,
        **identity,
    ):
        assert globals_to_capture == ()
        assert harnessed is True
        invocations.append(("clips", path, Path(path).read_bytes()))
        return {
            "exit_code": 0,
            "stdout": "matched\n",
            "stderr": "",
            "duration_ms": 1,
            "timed_out": False,
            "observation": {"identity": identity},
        }

    monkeypatch.setattr(run_module, "run_ferric_observer", fake_ferric)
    monkeypatch.setattr(run_module, "run_clips_observer", fake_clips)
    monkeypatch.setattr(
        run_module,
        "project_ferric_observation",
        lambda raw, **_kwargs: _canonical_observation(
            raw["identity"],
            fact_id=11,
        ),
    )
    monkeypatch.setattr(
        run_module,
        "project_clips_observation",
        lambda raw, **_kwargs: _canonical_observation(
            raw["identity"],
            fact_id=2,
        ),
    )

    with run_module._compatibility_run_workspace(root) as (run_workspace, failures_dir):
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
                str(run_workspace),
                str(failures_dir),
                declaration,
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
    assert ferric_result["composed_source"] == clips_result["composed_source"]
    assert ferric_result["composed_source"] == {
        "sha256": sha256_bytes(invocations[0][2]),
        "size_bytes": len(invocations[0][2]),
    }
    assert classification == "equivalent"
    assert reason == "oracle-v1-match"


def test_process_file_composes_harness_inside_clips_mounted_root(tmp_path, monkeypatch):
    root, source, entry, _plan = _materialized_library_harness(tmp_path)
    resolved = resolve_harness_contract(
        entry,
        source_path=source,
        root=root,
        manifest_key="libraries/facts.clp",
    )
    assert resolved is not None
    composed = resolved.source_bytes + b"\n" + resolved.harness_bytes
    declaration = _oracle_declaration(
        sha256_bytes(resolved.source_bytes),
        composed_digest=sha256_bytes(composed),
    )

    system_temp = tmp_path / "system-temp"
    system_temp.mkdir()
    monkeypatch.setattr(run_module.tempfile, "tempdir", str(system_temp))

    invocations: list[tuple[str, Path, bytes]] = []

    def engine_result() -> dict:
        return {
            "exit_code": 0,
            "stdout": "matched\n",
            "stderr": "",
            "duration_ms": 1,
            "timed_out": False,
        }

    def fake_ferric(path, _ferric, _root, _timeout, **identity):
        candidate = Path(path)
        invocations.append(("ferric", candidate, candidate.read_bytes()))
        result = engine_result()
        result["observation"] = {"identity": identity}
        return result

    def fake_clips(
        path,
        mounted_root,
        _script,
        _timeout,
        *,
        globals_to_capture,
        harnessed,
        **identity,
    ):
        assert globals_to_capture == ()
        assert harnessed is True
        candidate = Path(path)
        invocations.append(("clips", candidate, candidate.read_bytes()))
        if not candidate.resolve().is_relative_to(Path(mounted_root).resolve()):
            result = engine_result()
            result["exit_code"] = 1
            result["stderr"] = "path is outside the mounted repository"
            return result
        result = engine_result()
        result["observation"] = {"identity": identity}
        return result

    monkeypatch.setattr(run_module, "run_ferric_observer", fake_ferric)
    monkeypatch.setattr(run_module, "run_clips_observer", fake_clips)
    monkeypatch.setattr(
        run_module,
        "project_ferric_observation",
        lambda raw, **_kwargs: _canonical_observation(
            raw["identity"],
            fact_id=11,
        ),
    )
    monkeypatch.setattr(
        run_module,
        "project_clips_observation",
        lambda raw, **_kwargs: _canonical_observation(
            raw["identity"],
            fact_id=2,
        ),
    )

    with run_module._compatibility_run_workspace(root) as (run_workspace, failures_dir):
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
                str(run_workspace),
                str(failures_dir),
                declaration,
            )
        )

    _, _ferric_result, _clips_result, classification, reason = result
    assert classification == "equivalent"
    assert reason == "oracle-v1-match"
    assert [engine for engine, _, _ in invocations] == ["ferric", "clips"]
    assert invocations[0][1] == invocations[1][1]
    assert invocations[0][2] == invocations[1][2]
    assert invocations[0][1].resolve().is_relative_to(root.resolve())
    assert not invocations[0][1].exists()


def _oracle_declaration(digest: str, *, composed_digest: str | None = None) -> dict:
    return {
        "version": 1,
        "id": "fixture.oracle",
        "feature": "state effect",
        "source_sha256": digest,
        "composed_sha256": composed_digest or digest,
        "nonce": "0" * 32,
        "setup": ["load", "reset", "run"],
        "expectations": {
            "phase": "run-complete",
            "firings": {"count": 1, "names": None},
            "effects": [
                {
                    "name": "fact:MAIN::result",
                    "value": {
                        "type": "multifield",
                        "value": [{"type": "integer", "value": 1}],
                    },
                }
            ],
            "facts": [
                {
                    "kind": "ordered",
                    "id": 0,
                    "origin": "fixture",
                    "module": "MAIN",
                    "relation": "result",
                    "fields": [{"type": "integer", "value": 1}],
                }
            ],
            "channels": {"stdout": "", "stderr": ""},
            "diagnostic": {"phase": "none", "category": "none", "continued": True},
            "run": {"limit": None, "halt_reason": "agenda-empty"},
            "focus_stack": None,
            "globals": None,
        },
        "normalizers": ["fact-ids"],
    }


def _scenario_declaration(primary_digest: str, library_digest: str) -> dict:
    declaration = _oracle_declaration(primary_digest)
    declaration.update(
        {
            "version": 2,
            "sources": [
                {"name": "primary", "path": "fixture.clp", "sha256": primary_digest},
                {
                    "name": "library",
                    "path": "shared/library.clp",
                    "sha256": library_digest,
                },
            ],
            "setup": {
                "steps": [
                    {"operation": "load", "source": "library", "on_error": "stop"},
                    {"operation": "load", "source": "primary", "on_error": "stop"},
                    {"operation": "reset", "on_error": "stop"},
                    {"operation": "run", "limit": None, "on_error": "stop"},
                ]
            },
        }
    )
    declaration["composed_sha256"] = sha256_bytes(canonical_scenario_plan(declaration))
    return declaration


def _canonical_observation(identity: dict, *, fact_id: int) -> dict:
    integer = {"type": "integer", "value": 1}
    return {
        "version": 1,
        "id": identity["fixture_id"],
        "source_sha256": identity["source_sha256"],
        "composed_sha256": identity["composed_sha256"],
        "nonce": identity["nonce"],
        "markers": [
            {
                "kind": kind,
                "id": identity["fixture_id"],
                "source_sha256": identity["source_sha256"],
                "composed_sha256": identity["composed_sha256"],
                "nonce": identity["nonce"],
            }
            for kind in ("START", "COMPLETE")
        ],
        "phase": "run-complete",
        "firings": [{"rule": "counted-firing-1", "origin": "fixture"}],
        "effects": [
            {
                "name": "fact:MAIN::result",
                "value": {"type": "multifield", "value": [integer]},
                "origin": "fixture",
            }
        ],
        "facts": [
            {
                "kind": "ordered",
                "id": fact_id,
                "origin": "fixture",
                "module": "MAIN",
                "relation": "result",
                "fields": [integer],
            }
        ],
        "channels": {"stdout": "", "stderr": ""},
        "diagnostic": {"phase": "none", "category": "none", "continued": True},
        "run": {"limit": None, "halt_reason": "agenda-empty"},
        "focus_stack": [],
        "globals": None,
    }


def test_scan_attaches_digest_bound_oracle_declaration(tmp_path):
    root = tmp_path / "repo"
    examples = root / "tests" / "examples"
    source = examples / "fixture.clp"
    examples.mkdir(parents=True)
    source.write_text("(defrule effect => (assert (result 1)))\n", encoding="utf-8")
    digest = sha256_bytes(source.read_bytes())
    registry = {
        "version": 1,
        "fixtures": {"fixture.clp": _oracle_declaration(digest)},
    }
    (examples / "compat-oracles.json").write_text(
        json.dumps(registry),
        encoding="utf-8",
    )

    files = scan_examples(examples, root=root)

    assert files["fixture.clp"]["source_sha256"] == digest
    assert files["fixture.clp"]["oracle"] == registry["fixtures"]["fixture.clp"]
    assert files["fixture.clp"]["oracle_evidence"] == {
        "status": "missing",
        "version": 1,
        "declaration": True,
        "reached": False,
        "completed": False,
        "effect": False,
        "normalizations": ["fact-ids"],
        "violations": [],
    }


def test_scan_attaches_v2_scenario_after_validating_every_bundle_source(tmp_path):
    root = tmp_path / "repo"
    examples = root / "tests" / "examples"
    primary = examples / "fixture.clp"
    library = examples / "shared" / "library.clp"
    library.parent.mkdir(parents=True)
    primary.write_bytes(b"(defrule effect => (assert (result 1)))\n")
    library.write_bytes(b"(deftemplate shared (slot value))\n")
    declaration = _scenario_declaration(
        sha256_bytes(primary.read_bytes()),
        sha256_bytes(library.read_bytes()),
    )
    (examples / "compat-oracles.json").write_text(
        json.dumps({"version": 1, "fixtures": {"fixture.clp": declaration}}),
        encoding="utf-8",
    )

    files = scan_examples(examples, root=root)

    assert files["fixture.clp"]["oracle"] == declaration
    assert files["fixture.clp"]["oracle_evidence"]["version"] == 2


def test_scan_accepts_mixed_v1_and_v2_declaration_registry(tmp_path):
    root = tmp_path / "repo"
    examples = root / "tests" / "examples"
    legacy = examples / "legacy.clp"
    primary = examples / "fixture.clp"
    library = examples / "shared" / "library.clp"
    library.parent.mkdir(parents=True)
    legacy.write_bytes(b"(defrule legacy => (assert (result 1)))\n")
    primary.write_bytes(b"(defrule scenario => (assert (result 1)))\n")
    library.write_bytes(b"(deftemplate shared (slot value))\n")
    scenario = _scenario_declaration(
        sha256_bytes(primary.read_bytes()),
        sha256_bytes(library.read_bytes()),
    )
    scenario["id"] = "fixture.scenario"
    declarations = {
        "legacy.clp": _oracle_declaration(sha256_bytes(legacy.read_bytes())),
        "fixture.clp": scenario,
    }
    (examples / "compat-oracles.json").write_text(
        json.dumps({"version": 1, "fixtures": declarations}),
        encoding="utf-8",
    )

    files = scan_examples(examples, root=root)

    assert files["legacy.clp"]["oracle_evidence"]["version"] == 1
    assert files["fixture.clp"]["oracle_evidence"]["version"] == 2


def test_scan_rejects_stale_secondary_v2_source_digest(tmp_path):
    root = tmp_path / "repo"
    examples = root / "tests" / "examples"
    primary = examples / "fixture.clp"
    library = examples / "shared" / "library.clp"
    library.parent.mkdir(parents=True)
    primary.write_bytes(b"(defrule effect =>)\n")
    library.write_bytes(b"(deftemplate shared)\n")
    declaration = _scenario_declaration(sha256_bytes(primary.read_bytes()), "f" * 64)
    (examples / "compat-oracles.json").write_text(
        json.dumps({"version": 1, "fixtures": {"fixture.clp": declaration}}),
        encoding="utf-8",
    )

    with pytest.raises(OracleRegistryError, match=r"sources\[1\].sha256"):
        scan_examples(examples, root=root)


def test_scan_rejects_v2_secondary_source_symlink_escape(tmp_path):
    root = tmp_path / "repo"
    examples = root / "tests" / "examples"
    primary = examples / "fixture.clp"
    library = examples / "shared" / "library.clp"
    outside = tmp_path / "outside.clp"
    library.parent.mkdir(parents=True)
    primary.write_bytes(b"(defrule effect =>)\n")
    outside.write_bytes(b"(defrule escaped =>)\n")
    library.symlink_to(outside)
    declaration = _scenario_declaration(
        sha256_bytes(primary.read_bytes()),
        sha256_bytes(outside.read_bytes()),
    )
    (examples / "compat-oracles.json").write_text(
        json.dumps({"version": 1, "fixtures": {"fixture.clp": declaration}}),
        encoding="utf-8",
    )

    with pytest.raises(OracleRegistryError, match="must not be a symlink"):
        scan_examples(examples, root=root)


def test_scan_rejects_stale_oracle_source_digest(tmp_path):
    root = tmp_path / "repo"
    examples = root / "tests" / "examples"
    examples.mkdir(parents=True)
    (examples / "fixture.clp").write_text(
        "(defrule effect => (assert (result 1)))\n",
        encoding="utf-8",
    )
    registry = {
        "version": 1,
        "fixtures": {"fixture.clp": _oracle_declaration("f" * 64)},
    }
    (examples / "compat-oracles.json").write_text(
        json.dumps(registry),
        encoding="utf-8",
    )

    with pytest.raises(OracleRegistryError, match="source digest is stale"):
        scan_examples(examples, root=root)


def test_scan_rejects_duplicate_registry_json_field(tmp_path):
    root = tmp_path / "repo"
    examples = root / "tests" / "examples"
    examples.mkdir(parents=True)
    (examples / "fixture.clp").write_text("(defrule effect =>)\n", encoding="utf-8")
    (examples / "compat-oracles.json").write_text(
        '{"version":1,"version":1,"fixtures":{}}\n',
        encoding="utf-8",
    )

    with pytest.raises(OracleRegistryError, match="duplicate JSON field"):
        scan_examples(examples, root=root)
