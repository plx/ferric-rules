"""Tests for repository-visible composed compatibility execution."""

from __future__ import annotations

import os
import subprocess
import threading
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import pytest
from typer.testing import CliRunner

from ferric_tools._harness import HARNESS_GENERATION_VERSION, ResolvedHarness, sha256_bytes
from ferric_tools._manifest import load_manifest, save_manifest
from ferric_tools._paths import repo_root
from ferric_tools.compat import run as run_module


def _resolved_harness(
    root: Path,
    *,
    marker: bytes = b"one",
    name: str = "library",
) -> tuple[Path, ResolvedHarness]:
    source = root / "tests" / "examples" / f"{name}.clp"
    harness_path = root / "tests" / "harnesses" / f"{name}-harness.clp"
    source.parent.mkdir(parents=True, exist_ok=True)
    harness_path.parent.mkdir(parents=True, exist_ok=True)
    source_bytes = b"(deffacts sample (value " + marker + b"))\n"
    harness_bytes = b'(defrule verify => (printout t "matched" crlf))\n'
    source.write_bytes(source_bytes)
    harness_path.write_bytes(harness_bytes)
    harness = ResolvedHarness(
        path=harness_path,
        source_bytes=source_bytes,
        harness_bytes=harness_bytes,
        metadata={
            "path": f"tests/harnesses/{name}-harness.clp",
            "source_sha256": sha256_bytes(source_bytes),
            "harness_sha256": sha256_bytes(harness_bytes),
            "generation_version": HARNESS_GENERATION_VERSION,
            "executable": True,
        },
    )
    return source, harness


def _engine_result(*, stdout: str = "matched\n", exit_code: int = 0) -> dict:
    return {
        "exit_code": exit_code,
        "stdout": stdout,
        "stderr": "",
        "duration_ms": 1,
        "timed_out": False,
    }


def _oracle_declaration(digest: str, *, composed_digest: str | None = None) -> dict:
    integer = {"type": "integer", "value": 42}
    return {
        "version": 1,
        "id": "fixture.structured",
        "feature": "ordered state transition",
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
                    "value": {"type": "multifield", "value": [integer]},
                }
            ],
            "facts": [
                {
                    "kind": "ordered",
                    "id": 0,
                    "origin": "fixture",
                    "module": "MAIN",
                    "relation": "result",
                    "fields": [integer],
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


def _canonical_observation(
    identity: dict,
    *,
    fact_id: int,
    rule: str,
    value: int = 42,
) -> dict:
    integer = {"type": "integer", "value": value}
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
        "firings": [{"rule": rule, "origin": "fixture"}],
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


def _structured_work_item(
    *,
    source: Path,
    root: Path,
    run_workspace: Path,
    failures_dir: Path,
    declaration: dict,
) -> tuple:
    return (
        "fixture.clp",
        str(source),
        "ferric",
        str(root),
        "clips-reference",
        5,
        False,
        None,
        str(run_workspace),
        str(failures_dir),
        declaration,
    )


def test_classify_results_rejects_prompt_prefixed_harness_without_feature_oracle():
    verifier_id = "ferric-harness-" + ("a" * 64)
    output = (
        f"FERRIC-HARNESS|2|{verifier_id}|START\n"
        f"FERRIC-HARNESS|2|{verifier_id}|STATE|focus=MAIN\n"
        f"FERRIC-HARNESS|2|{verifier_id}|COMPLETE\n"
    )
    clips_output = f"         CLIPS (6.30 3/17/15)\nCLIPS> TRUE\nCLIPS> CLIPS> {output}CLIPS> "

    result = run_module.classify_results(
        _engine_result(stdout=output),
        _engine_result(stdout=clips_output),
    )

    assert result == ("pending", "oracle-missing")


def test_classify_results_rejects_matching_empty_output_without_oracle():
    result = run_module.classify_results(
        _engine_result(stdout=""),
        _engine_result(stdout=""),
    )

    assert result == ("pending", "oracle-missing")


def test_structured_process_binds_actual_bytes_and_fresh_nonce(tmp_path, monkeypatch):
    root = tmp_path / "repo"
    source = root / "tests" / "examples" / "fixture.clp"
    source.parent.mkdir(parents=True)
    source_bytes = b"(defrule compute => (assert (result 42)))\n"
    source.write_bytes(source_bytes)
    digest = sha256_bytes(source_bytes)
    declaration = _oracle_declaration(digest)
    invocations: list[tuple[str, Path, bytes, dict]] = []

    def fake_ferric(path, _ferric, _root, _timeout, **identity):
        candidate = Path(path)
        invocations.append(("ferric", candidate, candidate.read_bytes(), identity))
        return {**_engine_result(stdout="{}\n"), "observation": {"identity": identity}}

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
        assert harnessed is False
        candidate = Path(path)
        invocations.append(("clips", candidate, candidate.read_bytes(), identity))
        return {**_engine_result(stdout="protocol\n"), "observation": {"identity": identity}}

    monkeypatch.setattr(run_module, "run_ferric_observer", fake_ferric)
    monkeypatch.setattr(run_module, "run_clips_observer", fake_clips)
    monkeypatch.setattr(
        run_module,
        "project_ferric_observation",
        lambda raw, **_kwargs: _canonical_observation(
            raw["identity"],
            fact_id=11,
            rule="counted-firing-1",
        ),
    )
    monkeypatch.setattr(
        run_module,
        "project_clips_observation",
        lambda raw, **_kwargs: _canonical_observation(
            raw["identity"],
            fact_id=2,
            rule="compute",
        ),
    )

    with run_module._compatibility_run_workspace(root) as (run_workspace, failures_dir):
        item = _structured_work_item(
            source=source,
            root=root,
            run_workspace=run_workspace,
            failures_dir=failures_dir,
            declaration=declaration,
        )
        first = run_module.process_file(item)
        second = run_module.process_file(item)
        assert list(run_workspace.iterdir()) == []

    assert first[3:] == ("equivalent", "oracle-v1-match")
    assert second[3:] == ("equivalent", "oracle-v1-match")
    assert first[1]["oracle_evidence"]["status"] == "valid"
    assert first[1]["oracle_evidence"]["effect"] is True
    assert len(invocations) == 4
    assert all(content == source_bytes for _, _, content, _ in invocations)
    assert invocations[0][1] == invocations[1][1]
    assert invocations[2][1] == invocations[3][1]
    assert all(not path.exists() for _, path, _, _ in invocations)
    first_identity = invocations[0][3]
    second_identity = invocations[2][3]
    assert invocations[1][3] == first_identity
    assert invocations[3][3] == second_identity
    assert first_identity["source_sha256"] == digest
    assert first_identity["composed_sha256"] == digest
    assert first_identity["nonce"] != declaration["nonce"]
    assert first_identity["nonce"] != second_identity["nonce"]
    assert declaration["nonce"] == "0" * 32


def test_nonzero_structured_observer_is_invalid_and_retains_source(tmp_path, monkeypatch):
    root = tmp_path / "repo"
    source = root / "tests" / "examples" / "fixture.clp"
    source.parent.mkdir(parents=True)
    source_bytes = b"(defrule compute => (assert (result 42)))\n"
    source.write_bytes(source_bytes)
    digest = sha256_bytes(source_bytes)
    declaration = _oracle_declaration(digest)

    def failed_ferric(_path, _ferric, _root, _timeout, **_identity):
        return {
            **_engine_result(stdout='{"partial":true}\n', exit_code=1),
            "observation": {"partial": True},
        }

    def successful_clips(
        _path,
        _root,
        _script,
        _timeout,
        *,
        globals_to_capture,
        harnessed,
        **identity,
    ):
        assert globals_to_capture == ()
        assert harnessed is False
        return {**_engine_result(), "observation": {"identity": identity}}

    monkeypatch.setattr(run_module, "run_ferric_observer", failed_ferric)
    monkeypatch.setattr(run_module, "run_clips_observer", successful_clips)
    monkeypatch.setattr(
        run_module,
        "project_clips_observation",
        lambda raw, **_kwargs: _canonical_observation(
            raw["identity"],
            fact_id=2,
            rule="compute",
        ),
    )

    with run_module._compatibility_run_workspace(root) as (run_workspace, failures_dir):
        result = run_module.process_file(
            _structured_work_item(
                source=source,
                root=root,
                run_workspace=run_workspace,
                failures_dir=failures_dir,
                declaration=declaration,
            )
        )

    _, ferric_result, clips_result, classification, reason = result
    assert classification == "pending"
    assert reason == "oracle-invalid:evidence"
    assert ferric_result["oracle_evidence"]["status"] == "invalid"
    assert clips_result is not None
    assert clips_result["oracle_evidence"] == ferric_result["oracle_evidence"]
    artifact_path = root / ferric_result["composed_source"]["artifact_path"]
    assert artifact_path.read_bytes() == source_bytes


def test_runner_persists_missing_oracle_state_without_engine_preflight(
    tmp_path,
    monkeypatch,
):
    root = tmp_path / "repo"
    examples = root / "tests" / "examples"
    source = examples / "fixture.clp"
    source.parent.mkdir(parents=True)
    source.write_text("(defrule noop =>)\n", encoding="utf-8")
    manifest_path = examples / "compat-manifest.json"
    entry = {
        "source": "",
        "classification": "pending",
        "reason": "testable",
        "runability": "standalone",
        "features": ["defrule"],
        "unsupported_features": [],
        "ferric": {"legacy": True},
        "clips": {"legacy": True},
        "notes": "",
        "source_sha256": sha256_bytes(source.read_bytes()),
    }
    save_manifest(
        manifest_path,
        {
            "version": 3,
            "oracle_protocol_version": 1,
            "summary": {
                "total": 1,
                "equivalent": 0,
                "divergent": 0,
                "incompatible": 0,
                "pending": 1,
            },
            "files": {"fixture.clp": entry},
        },
    )
    monkeypatch.setattr(run_module, "repo_root", lambda: root)
    monkeypatch.setattr(run_module, "default_examples_dir", lambda: examples)

    result = CliRunner().invoke(
        run_module.app,
        ["--manifest", str(manifest_path), "--all"],
    )

    assert result.exit_code == 0, result.output
    persisted = load_manifest(manifest_path)
    assert persisted["summary"] == {
        "total": 1,
        "equivalent": 0,
        "divergent": 0,
        "incompatible": 0,
        "pending": 1,
    }
    assert persisted["files"]["fixture.clp"]["reason"] == "oracle-missing"
    assert persisted["files"]["fixture.clp"]["oracle_evidence"]["status"] == "missing"
    assert persisted["files"]["fixture.clp"]["ferric"] is None
    assert persisted["files"]["fixture.clp"]["clips"] is None


def test_runner_persists_explicit_missing_oracle_before_nonzero_exit(
    tmp_path,
    monkeypatch,
):
    root = tmp_path / "repo"
    examples = root / "tests" / "examples"
    source = examples / "fixture.clp"
    source.parent.mkdir(parents=True)
    source.write_text("(defrule noop =>)\n", encoding="utf-8")
    manifest_path = examples / "compat-manifest.json"
    save_manifest(
        manifest_path,
        {
            "version": 3,
            "oracle_protocol_version": 1,
            "summary": {
                "total": 1,
                "equivalent": 1,
                "divergent": 0,
                "incompatible": 0,
                "pending": 0,
            },
            "files": {
                "fixture.clp": {
                    "source": "",
                    "classification": "equivalent",
                    "reason": "exact-match",
                    "runability": "standalone",
                    "features": ["defrule"],
                    "unsupported_features": [],
                    "ferric": {"legacy": True},
                    "clips": {"legacy": True},
                    "notes": "",
                    "source_sha256": sha256_bytes(source.read_bytes()),
                }
            },
        },
    )
    monkeypatch.setattr(run_module, "repo_root", lambda: root)
    monkeypatch.setattr(run_module, "default_examples_dir", lambda: examples)

    result = CliRunner().invoke(
        run_module.app,
        ["--manifest", str(manifest_path), "--file", "fixture.clp"],
    )

    assert result.exit_code == 1
    assert "no structured oracle declaration" in result.output
    persisted = load_manifest(manifest_path)
    persisted_entry = persisted["files"]["fixture.clp"]
    assert persisted["summary"]["equivalent"] == 0
    assert persisted["summary"]["pending"] == 1
    assert persisted_entry["classification"] == "pending"
    assert persisted_entry["reason"] == "oracle-missing"
    assert persisted_entry["oracle_evidence"] == run_module._missing_oracle_evidence()
    assert persisted_entry["ferric"] is None
    assert persisted_entry["clips"] is None


def test_runner_persists_malformed_declaration_before_nonzero_exit(
    tmp_path,
    monkeypatch,
):
    root = tmp_path / "repo"
    examples = root / "tests" / "examples"
    source = examples / "fixture.clp"
    source.parent.mkdir(parents=True)
    source.write_text("(defrule noop =>)\n", encoding="utf-8")
    manifest_path = examples / "compat-manifest.json"
    save_manifest(
        manifest_path,
        {
            "version": 3,
            "oracle_protocol_version": 1,
            "summary": {
                "total": 1,
                "equivalent": 1,
                "divergent": 0,
                "incompatible": 0,
                "pending": 0,
            },
            "files": {
                "fixture.clp": {
                    "source": "",
                    "classification": "equivalent",
                    "reason": "exact-match",
                    "runability": "standalone",
                    "features": ["defrule"],
                    "unsupported_features": [],
                    "ferric": {"legacy": True},
                    "clips": {"legacy": True},
                    "notes": "",
                    "source_sha256": sha256_bytes(source.read_bytes()),
                    "oracle": "not-an-object",
                }
            },
        },
    )
    monkeypatch.setattr(run_module, "repo_root", lambda: root)
    monkeypatch.setattr(run_module, "default_examples_dir", lambda: examples)

    result = CliRunner().invoke(
        run_module.app,
        ["--manifest", str(manifest_path), "--file", "fixture.clp"],
    )

    assert result.exit_code == 1
    assert "oracle declaration must be an object" in result.output
    persisted = load_manifest(manifest_path)
    persisted_entry = persisted["files"]["fixture.clp"]
    assert persisted["summary"]["equivalent"] == 0
    assert persisted["summary"]["pending"] == 1
    assert persisted_entry["classification"] == "pending"
    assert persisted_entry["reason"] == "oracle-invalid:declaration"
    assert persisted_entry["oracle_evidence"]["status"] == "invalid"
    assert persisted_entry["oracle_evidence"]["violations"] == [
        "oracle declaration must be an object"
    ]
    assert persisted_entry["ferric"] is None
    assert persisted_entry["clips"] is None


def test_runner_persists_missing_source_as_invalid_before_engine_preflight(
    tmp_path,
    monkeypatch,
):
    root = tmp_path / "repo"
    examples = root / "tests" / "examples"
    examples.mkdir(parents=True)
    manifest_path = examples / "compat-manifest.json"
    digest = "0" * 64
    entry = {
        "source": "",
        "classification": "pending",
        "reason": "testable",
        "runability": "standalone",
        "features": ["defrule"],
        "unsupported_features": [],
        "ferric": {"legacy": True},
        "clips": {"legacy": True},
        "notes": "",
        "source_sha256": digest,
        "oracle": _oracle_declaration(digest),
    }
    save_manifest(
        manifest_path,
        {
            "version": 3,
            "oracle_protocol_version": 1,
            "summary": {
                "total": 1,
                "equivalent": 0,
                "divergent": 0,
                "incompatible": 0,
                "pending": 1,
            },
            "files": {"gone.clp": entry},
        },
    )
    monkeypatch.setattr(run_module, "repo_root", lambda: root)
    monkeypatch.setattr(run_module, "default_examples_dir", lambda: examples)

    result = CliRunner().invoke(
        run_module.app,
        ["--manifest", str(manifest_path), "--file", "gone.clp"],
    )

    assert result.exit_code == 1
    assert "gone.clp source cannot be resolved" in result.output
    persisted = load_manifest(manifest_path)
    persisted_entry = persisted["files"]["gone.clp"]
    assert persisted_entry["classification"] == "pending"
    assert persisted_entry["reason"] == "oracle-invalid:source"
    assert persisted_entry["oracle_evidence"]["status"] == "invalid"
    assert persisted_entry["oracle_evidence"]["declaration"] is False
    assert (
        "gone.clp source cannot be resolved" in persisted_entry["oracle_evidence"]["violations"][0]
    )
    assert persisted_entry["ferric"] is None
    assert persisted_entry["clips"] is None


def test_runner_persists_invalid_evidence_before_nonzero_exit(tmp_path, monkeypatch):
    root = tmp_path / "repo"
    examples = root / "tests" / "examples"
    source = examples / "fixture.clp"
    source.parent.mkdir(parents=True)
    source.write_text("(defrule noop =>)\n", encoding="utf-8")
    ferric = root / "target" / "release" / "ferric"
    ferric.parent.mkdir(parents=True)
    ferric.write_text("", encoding="utf-8")
    manifest_path = examples / "compat-manifest.json"
    evidence = {
        "status": "invalid",
        "version": 1,
        "declaration": True,
        "reached": False,
        "completed": False,
        "effect": False,
        "normalizations": [],
        "violations": ["ferric.markers: missing COMPLETE"],
    }
    entry = {
        "source": "",
        "classification": "pending",
        "reason": "testable",
        "runability": "standalone",
        "features": ["defrule"],
        "unsupported_features": [],
        "ferric": None,
        "clips": None,
        "notes": "",
        "source_sha256": sha256_bytes(source.read_bytes()),
        "oracle": {},
    }
    save_manifest(
        manifest_path,
        {
            "version": 3,
            "oracle_protocol_version": 1,
            "summary": {
                "total": 1,
                "equivalent": 0,
                "divergent": 0,
                "incompatible": 0,
                "pending": 1,
            },
            "files": {"fixture.clp": entry},
        },
    )

    def fake_parallel(_function, items, *, workers):
        assert workers == 1
        assert len(list(items)) == 1
        yield (
            "fixture.clp",
            {**_engine_result(exit_code=1), "oracle_evidence": evidence},
            {**_engine_result(), "oracle_evidence": evidence},
            "pending",
            "oracle-invalid:markers",
        )

    monkeypatch.setattr(run_module, "repo_root", lambda: root)
    monkeypatch.setattr(run_module, "default_examples_dir", lambda: examples)
    monkeypatch.setattr(run_module, "parallel_run", fake_parallel)
    monkeypatch.setattr(
        run_module.subprocess,
        "run",
        lambda *args, **kwargs: subprocess.CompletedProcess(args[0], 0),
    )

    result = CliRunner().invoke(
        run_module.app,
        [
            "--manifest",
            str(manifest_path),
            "--ferric-bin",
            str(ferric),
            "--workers",
            "1",
        ],
    )

    assert result.exit_code == 1
    assert "Invalid oracle evidence (1)" in result.output
    persisted = load_manifest(manifest_path)
    persisted_entry = persisted["files"]["fixture.clp"]
    assert persisted_entry["classification"] == "pending"
    assert persisted_entry["reason"] == "oracle-invalid:markers"
    assert persisted_entry["oracle_evidence"] == evidence


def _work_item(
    *,
    source: Path,
    harness: ResolvedHarness,
    root: Path,
    run_workspace: Path,
    failures_dir: Path,
) -> tuple:
    composed = harness.source_bytes + b"\n" + harness.harness_bytes
    return (
        "library.clp",
        str(source),
        "ferric",
        str(root),
        "clips-reference",
        5,
        False,
        harness,
        str(run_workspace),
        str(failures_dir),
        _oracle_declaration(
            sha256_bytes(harness.source_bytes),
            composed_digest=sha256_bytes(composed),
        ),
    )


def _install_canonical_projectors(monkeypatch) -> None:
    monkeypatch.setattr(
        run_module,
        "project_ferric_observation",
        lambda raw, **_kwargs: _canonical_observation(
            raw["identity"],
            fact_id=11,
            rule="counted-firing-1",
            value=raw.get("value", 42),
        ),
    )
    monkeypatch.setattr(
        run_module,
        "project_clips_observation",
        lambda raw, **_kwargs: _canonical_observation(
            raw["identity"],
            fact_id=2,
            rule="counted-firing-1",
            value=raw.get("value", 42),
        ),
    )


def test_structured_divergence_retains_artifact_and_cleans_temp(
    tmp_path,
    monkeypatch,
):
    root = tmp_path / "repo"
    source, harness = _resolved_harness(root)
    invocations: list[tuple[str, Path, bytes]] = []

    def fake_ferric(path, _ferric, _root, _timeout, **identity):
        candidate = Path(path)
        invocations.append(("ferric", candidate, candidate.read_bytes()))
        return {
            **_engine_result(stdout="ferric\n"),
            "observation": {"identity": identity, "value": 42},
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
        candidate = Path(path)
        invocations.append(("clips", candidate, candidate.read_bytes()))
        return {
            **_engine_result(stdout="clips\n"),
            "observation": {"identity": identity, "value": 43},
        }

    monkeypatch.setattr(run_module, "run_ferric_observer", fake_ferric)
    monkeypatch.setattr(run_module, "run_clips_observer", fake_clips)
    _install_canonical_projectors(monkeypatch)

    with run_module._compatibility_run_workspace(root) as (run_workspace, failures_dir):
        item = _work_item(
            source=source,
            harness=harness,
            root=root,
            run_workspace=run_workspace,
            failures_dir=failures_dir,
        )
        first = run_module.process_file(item)
        second = run_module.process_file(item)
        assert list(run_workspace.iterdir()) == []

    expected = harness.source_bytes + b"\n" + harness.harness_bytes
    expected_digest = sha256_bytes(expected)
    _, ferric_result, clips_result, classification, reason = first
    assert classification == "divergent"
    assert reason == "oracle-effects-mismatch"
    assert clips_result is not None
    assert ferric_result["composed_source"] == clips_result["composed_source"]
    assert ferric_result["composed_source"] == {
        "sha256": expected_digest,
        "size_bytes": len(expected),
        "artifact_path": f".ferric-compat/failures/{expected_digest}.clp",
    }
    artifact = root / ferric_result["composed_source"]["artifact_path"]
    assert artifact.read_bytes() == expected
    assert sha256_bytes(artifact.read_bytes()) == expected_digest
    if not run_module.IS_WINDOWS:
        assert artifact.stat().st_mode & 0o222 == 0
    assert second[1]["composed_source"] == ferric_result["composed_source"]
    assert len(list(artifact.parent.glob("*.clp"))) == 1
    assert [engine for engine, _, _ in invocations] == [
        "ferric",
        "clips",
        "ferric",
        "clips",
    ]
    assert all(content == expected for _, _, content in invocations)
    assert all(not path.exists() for _, path, _ in invocations)


def test_concurrent_unicode_workspace_uses_unique_files_without_crosstalk(tmp_path, monkeypatch):
    root = tmp_path / "repo space Ω"
    barrier = threading.Barrier(8)
    lock = threading.Lock()
    ferric_paths: list[Path] = []
    engine_bytes: dict[Path, list[bytes]] = {}
    cases = [
        _resolved_harness(
            root,
            marker=str(index).encode(),
            name=f"library-{index}",
        )
        for index in range(8)
    ]
    expected_contents = {
        harness.source_bytes + b"\n" + harness.harness_bytes for _, harness in cases
    }

    def fake_ferric(path, _ferric, _root, _timeout, **identity):
        candidate = Path(path)
        assert candidate.exists()
        if not run_module.IS_WINDOWS:
            assert candidate.stat().st_mode & 0o222 == 0
        assert all(
            directory.stat().st_mode & 0o111 == 0o111
            for directory in (
                candidate.parent,
                candidate.parent.parent,
                candidate.parent.parent.parent,
            )
        )
        content = candidate.read_bytes()
        with lock:
            ferric_paths.append(candidate)
            engine_bytes.setdefault(candidate, []).append(content)
        barrier.wait(timeout=10)
        return {**_engine_result(), "observation": {"identity": identity}}

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
        candidate = Path(path)
        assert candidate.exists()
        content = candidate.read_bytes()
        with lock:
            engine_bytes.setdefault(candidate, []).append(content)
        return {**_engine_result(), "observation": {"identity": identity}}

    monkeypatch.setattr(run_module, "run_ferric_observer", fake_ferric)
    monkeypatch.setattr(run_module, "run_clips_observer", fake_clips)
    _install_canonical_projectors(monkeypatch)

    with run_module._compatibility_run_workspace(root) as (run_workspace, failures_dir):
        items = [
            _work_item(
                source=source,
                harness=harness,
                root=root,
                run_workspace=run_workspace,
                failures_dir=failures_dir,
            )
            for source, harness in cases
        ]
        with ThreadPoolExecutor(max_workers=8) as executor:
            results = list(executor.map(run_module.process_file, items))
        assert list(run_workspace.iterdir()) == []
        assert list(failures_dir.glob("*.clp")) == []

    assert len(set(ferric_paths)) == 8
    assert all(path.resolve().is_relative_to(root.resolve()) for path in ferric_paths)
    assert all(not path.exists() for path in ferric_paths)
    assert set(engine_bytes) == set(ferric_paths)
    assert all(contents[0] == contents[1] for contents in engine_bytes.values())
    assert {contents[0] for contents in engine_bytes.values()} == expected_contents
    assert all(result[3:] == ("equivalent", "oracle-v1-match") for result in results)


def test_concurrent_workspace_creation_is_race_free_and_traversable(tmp_path):
    root = tmp_path / "repo"
    root.mkdir()
    barrier = threading.Barrier(16)

    def open_workspace(_index):
        with run_module._compatibility_run_workspace(root) as (run_workspace, failures_dir):
            assert run_workspace.stat().st_mode & 0o111 == 0o111
            assert failures_dir.stat().st_mode & 0o111 == 0o111
            barrier.wait(timeout=10)
            return run_workspace

    with ThreadPoolExecutor(max_workers=16) as executor:
        run_workspaces = list(executor.map(open_workspace, range(16)))

    assert len(set(run_workspaces)) == 16
    assert all(not workspace.exists() for workspace in run_workspaces)
    assert all(
        directory.stat().st_mode & 0o111 == 0o111
        for directory in (
            root / ".ferric-compat",
            root / ".ferric-compat" / "runs",
            root / ".ferric-compat" / "failures",
        )
    )


def test_workspace_rejects_symlink_escape_before_creating_external_files(tmp_path):
    root = tmp_path / "repo"
    root.mkdir()
    outside = tmp_path / "outside"
    outside.mkdir()
    (root / ".ferric-compat").symlink_to(outside, target_is_directory=True)

    with (
        pytest.raises(
            run_module.CompatibilityWorkspaceError,
            match="compatibility run workspace must not contain a symlink",
        ),
        run_module._compatibility_run_workspace(root),
    ):
        pytest.fail("escaped workspace should not be yielded")

    assert list(outside.iterdir()) == []


def test_keyboard_interrupt_retains_artifact_and_cleans_run_workspace(tmp_path, monkeypatch):
    root = tmp_path / "repo"
    source, harness = _resolved_harness(root)
    invocation_path: Path | None = None
    run_workspace_path: Path | None = None

    def interrupting_ferric(path, _ferric, _root, _timeout, **_identity):
        nonlocal invocation_path
        invocation_path = Path(path)
        raise KeyboardInterrupt("injected interrupt")

    monkeypatch.setattr(run_module, "run_ferric_observer", interrupting_ferric)

    with (
        pytest.raises(KeyboardInterrupt, match="injected interrupt") as caught,
        run_module._compatibility_run_workspace(root) as (run_workspace, failures_dir),
    ):
        run_workspace_path = run_workspace
        run_module.process_file(
            _work_item(
                source=source,
                harness=harness,
                root=root,
                run_workspace=run_workspace,
                failures_dir=failures_dir,
            )
        )

    expected = harness.source_bytes + b"\n" + harness.harness_bytes
    digest = sha256_bytes(expected)
    artifact = root / ".ferric-compat" / "failures" / f"{digest}.clp"
    assert artifact.read_bytes() == expected
    assert any("composed failure artifact retained" in note for note in caught.value.__notes__)
    assert invocation_path is not None
    assert not invocation_path.exists()
    assert run_workspace_path is not None
    assert not run_workspace_path.exists()


def test_mutated_composed_source_fails_closed_before_clips(tmp_path, monkeypatch):
    root = tmp_path / "repo"
    source, harness = _resolved_harness(root)
    invocation_path: Path | None = None

    def mutating_ferric(path, _ferric, _root, _timeout, **identity):
        nonlocal invocation_path
        invocation_path = Path(path)
        invocation_path.chmod(0o644)
        invocation_path.write_bytes(b"tampered\n")
        return {**_engine_result(), "observation": {"identity": identity}}

    def unexpected_clips(*_args, **_kwargs):
        pytest.fail("CLIPS must not run after the composed source changes")

    monkeypatch.setattr(run_module, "run_ferric_observer", mutating_ferric)
    monkeypatch.setattr(run_module, "run_clips_observer", unexpected_clips)
    _install_canonical_projectors(monkeypatch)

    with (
        pytest.raises(
            run_module.CompatibilityWorkspaceError,
            match="changed before CLIPS execution",
        ),
        run_module._compatibility_run_workspace(root) as (run_workspace, failures_dir),
    ):
        run_module.process_file(
            _work_item(
                source=source,
                harness=harness,
                root=root,
                run_workspace=run_workspace,
                failures_dir=failures_dir,
            )
        )

    expected = harness.source_bytes + b"\n" + harness.harness_bytes
    digest = sha256_bytes(expected)
    artifact = root / ".ferric-compat" / "failures" / f"{digest}.clp"
    assert artifact.read_bytes() == expected
    if not run_module.IS_WINDOWS:
        assert artifact.stat().st_mode & 0o222 == 0
    assert invocation_path is not None
    assert not invocation_path.exists()


def test_windows_mode_keeps_composed_file_writable_for_cleanup(tmp_path, monkeypatch):
    root = tmp_path / "repo"
    source, harness = _resolved_harness(root)
    invocation_paths: list[Path] = []

    def fake_ferric(path, _ferric, _root, _timeout, **identity):
        candidate = Path(path)
        invocation_paths.append(candidate)
        assert candidate.stat().st_mode & 0o200
        return {**_engine_result(), "observation": {"identity": identity}}

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
        candidate = Path(path)
        invocation_paths.append(candidate)
        assert candidate.stat().st_mode & 0o200
        return {**_engine_result(), "observation": {"identity": identity}}

    monkeypatch.setattr(run_module, "IS_WINDOWS", True)
    monkeypatch.setattr(run_module, "run_ferric_observer", fake_ferric)
    monkeypatch.setattr(run_module, "run_clips_observer", fake_clips)
    _install_canonical_projectors(monkeypatch)

    with run_module._compatibility_run_workspace(root) as (run_workspace, failures_dir):
        result = run_module.process_file(
            _work_item(
                source=source,
                harness=harness,
                root=root,
                run_workspace=run_workspace,
                failures_dir=failures_dir,
            )
        )
        assert list(run_workspace.iterdir()) == []

    assert result[3:] == ("equivalent", "oracle-v1-match")
    assert len(invocation_paths) == 2
    assert invocation_paths[0] == invocation_paths[1]
    assert not invocation_paths[0].exists()


def test_run_clips_observer_rejects_leaf_symlink_escape_before_subprocess(
    tmp_path,
    monkeypatch,
):
    root = tmp_path / "repo"
    root.mkdir()
    outside = tmp_path / "outside.clp"
    outside.write_text("(reset)\n", encoding="utf-8")
    escaped = root / "escaped.clp"
    escaped.symlink_to(outside)

    def unexpected_run(*_args, **_kwargs):
        pytest.fail("Docker subprocess must not run for an escaped symlink")

    monkeypatch.setattr(run_module.subprocess, "run", unexpected_run)

    with pytest.raises(
        run_module.CompatibilityWorkspaceError,
        match="CLIPS input must not be a symlink",
    ):
        run_module.run_clips_observer(
            str(escaped),
            str(root),
            "clips-reference",
            5,
            fixture_id="fixture.symlink",
            nonce="0" * 32,
            source_sha256="0" * 64,
            composed_sha256="0" * 64,
            globals_to_capture=(),
            harnessed=False,
        )


def test_clips_reference_script_preserves_unicode_quotes_and_rejects_symlink(
    tmp_path,
):
    synthetic_repo = tmp_path / "repo space Ω"
    synthetic_repo.mkdir()
    subprocess.run(["git", "init", "-q"], cwd=synthetic_repo, check=True)
    fixture = synthetic_repo / "fixtures" / 'rules space Ω "quoted".clp'
    fixture.parent.mkdir()
    fixture.write_text("(reset)\n", encoding="utf-8")

    fake_bin = tmp_path / "fake-bin"
    fake_bin.mkdir()
    fake_docker = fake_bin / "docker"
    fake_docker.write_text(
        '#!/usr/bin/env bash\nprintf "%s\\n" "$@" > "$CAPTURE_ARGS"\ncat > "$CAPTURE_STDIN"\n',
        encoding="utf-8",
    )
    fake_docker.chmod(0o755)
    captured_args = tmp_path / "docker-args"
    captured_stdin = tmp_path / "docker-stdin"
    env = {
        **os.environ,
        "PATH": f"{fake_bin}{os.pathsep}{os.environ['PATH']}",
        "CAPTURE_ARGS": str(captured_args),
        "CAPTURE_STDIN": str(captured_stdin),
    }
    script = repo_root() / "scripts" / "clips-reference.sh"

    result = subprocess.run(
        [str(script), "run", "--file", str(fixture)],
        cwd=synthetic_repo,
        env=env,
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0, result.stderr
    assert (
        captured_stdin.read_text(encoding="utf-8")
        == '(batch* "/workspace/fixtures/rules space Ω \\"quoted\\".clp")\n'
        "(reset)\n(run)\n(exit)\n"
    )
    mount = f"{synthetic_repo.resolve()}:/workspace:ro"
    assert mount in captured_args.read_text(encoding="utf-8").splitlines()

    quiet_result = subprocess.run(
        [str(script), "run", "--quiet", "--file", str(fixture)],
        cwd=synthetic_repo,
        env=env,
        capture_output=True,
        text=True,
    )

    assert quiet_result.returncode == 0, quiet_result.stderr
    quiet_args = captured_args.read_text(encoding="utf-8").splitlines()
    assert quiet_args[-2:] == ["-f2", "/dev/stdin"]
    assert (
        captured_stdin.read_text(encoding="utf-8")
        == '(batch* "/workspace/fixtures/rules space Ω \\"quoted\\".clp")\n'
        "(reset)\n(run)\n(exit)\n"
    )

    observer_nonce = "0123456789abcdef0123456789abcdef"
    observer_auth_key = "c" * 64
    observer_result = subprocess.run(
        [
            str(script),
            "run",
            "--quiet",
            "--observer-nonce",
            observer_nonce,
            "--observer-fixture-id",
            "oracle.test",
            "--observer-source-sha256",
            "a" * 64,
            "--observer-composed-sha256",
            "b" * 64,
            "--observer-auth-key",
            observer_auth_key,
            "--file",
            str(fixture),
        ],
        cwd=synthetic_repo,
        env=env,
        capture_output=True,
        text=True,
    )

    assert observer_result.returncode == 0, observer_result.stderr
    observer_args = captured_args.read_text(encoding="utf-8").splitlines()
    assert "--ferric-observer" in observer_args
    assert "--source" in observer_args
    assert '/workspace/fixtures/rules space Ω "quoted".clp' in observer_args
    assert observer_nonce not in observer_args
    assert observer_auth_key not in observer_args
    assert not any("FERRIC_COMPAT_OBSERVER_NONCE" in argument for argument in observer_args)
    assert not any("LD_PRELOAD" in argument for argument in observer_args)
    assert captured_stdin.read_text(encoding="utf-8") == (
        f"{observer_nonce}|oracle.test|{'a' * 64}|{'b' * 64}|{observer_auth_key}\n"
    )

    invalid_observer_result = subprocess.run(
        [
            str(script),
            "run",
            "--observer-nonce",
            "not-hex",
            "--file",
            str(fixture),
        ],
        cwd=synthetic_repo,
        env=env,
        capture_output=True,
        text=True,
    )

    assert invalid_observer_result.returncode == 1
    assert "--observer-nonce must encode" in invalid_observer_result.stderr

    outside = tmp_path / "outside.clp"
    outside.write_text("(reset)\n", encoding="utf-8")
    escaped = synthetic_repo / "fixtures" / "escaped.clp"
    escaped.symlink_to(outside)
    captured_args.unlink()
    captured_stdin.unlink()

    escaped_result = subprocess.run(
        [str(script), "run", "--file", str(escaped)],
        cwd=synthetic_repo,
        env=env,
        capture_output=True,
        text=True,
    )

    assert escaped_result.returncode == 1
    assert "file path must not be a symlink" in escaped_result.stderr
    assert not captured_args.exists()
    assert not captured_stdin.exists()
