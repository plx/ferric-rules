"""Tests for repository-visible composed compatibility execution."""

from __future__ import annotations

import os
import subprocess
import threading
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import pytest

from ferric_tools._harness import HARNESS_GENERATION_VERSION, ResolvedHarness, sha256_bytes
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


def test_classify_results_preserves_prompt_prefixed_harness_start_as_exact_match():
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

    assert result == ("equivalent", "exact-match")


def _work_item(
    *,
    source: Path,
    harness: ResolvedHarness,
    root: Path,
    run_workspace: Path,
    failures_dir: Path,
) -> tuple:
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
    )


def test_divergence_retains_content_addressed_artifact_and_cleans_temp(tmp_path, monkeypatch):
    root = tmp_path / "repo"
    source, harness = _resolved_harness(root)
    invocations: list[tuple[str, Path, bytes]] = []

    def fake_ferric(path, _ferric, _root, _timeout):
        candidate = Path(path)
        invocations.append(("ferric", candidate, candidate.read_bytes()))
        return _engine_result(stdout="ferric\n")

    def fake_clips(path, _root, _script, _timeout):
        candidate = Path(path)
        invocations.append(("clips", candidate, candidate.read_bytes()))
        return _engine_result(stdout="clips\n")

    monkeypatch.setattr(run_module, "run_ferric", fake_ferric)
    monkeypatch.setattr(run_module, "run_clips_docker", fake_clips)

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
    assert reason == "output-mismatch"
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

    def fake_ferric(path, _ferric, _root, _timeout):
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
        return _engine_result()

    def fake_clips(path, _root, _script, _timeout):
        candidate = Path(path)
        assert candidate.exists()
        content = candidate.read_bytes()
        with lock:
            engine_bytes.setdefault(candidate, []).append(content)
        return _engine_result()

    monkeypatch.setattr(run_module, "run_ferric", fake_ferric)
    monkeypatch.setattr(run_module, "run_clips_docker", fake_clips)

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
    assert all(result[3:] == ("equivalent", "exact-match") for result in results)


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

    def interrupting_ferric(path, _ferric, _root, _timeout):
        nonlocal invocation_path
        invocation_path = Path(path)
        raise KeyboardInterrupt("injected interrupt")

    monkeypatch.setattr(run_module, "run_ferric", interrupting_ferric)

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

    def mutating_ferric(path, _ferric, _root, _timeout):
        nonlocal invocation_path
        invocation_path = Path(path)
        invocation_path.chmod(0o644)
        invocation_path.write_bytes(b"tampered\n")
        return _engine_result()

    def unexpected_clips(*_args, **_kwargs):
        pytest.fail("CLIPS must not run after the composed source changes")

    monkeypatch.setattr(run_module, "run_ferric", mutating_ferric)
    monkeypatch.setattr(run_module, "run_clips_docker", unexpected_clips)

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

    def fake_ferric(path, _ferric, _root, _timeout):
        candidate = Path(path)
        invocation_paths.append(candidate)
        assert candidate.stat().st_mode & 0o200
        return _engine_result()

    def fake_clips(path, _root, _script, _timeout):
        candidate = Path(path)
        invocation_paths.append(candidate)
        assert candidate.stat().st_mode & 0o200
        return _engine_result()

    monkeypatch.setattr(run_module, "IS_WINDOWS", True)
    monkeypatch.setattr(run_module, "run_ferric", fake_ferric)
    monkeypatch.setattr(run_module, "run_clips_docker", fake_clips)

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

    assert result[3:] == ("equivalent", "exact-match")
    assert len(invocation_paths) == 2
    assert invocation_paths[0] == invocation_paths[1]
    assert not invocation_paths[0].exists()


def test_run_clips_docker_rejects_leaf_symlink_escape_before_subprocess(tmp_path, monkeypatch):
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
        run_module.run_clips_docker(str(escaped), str(root), "clips-reference", 5)


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
