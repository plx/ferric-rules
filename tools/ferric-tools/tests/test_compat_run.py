"""Tests for repository-visible composed compatibility execution."""

from __future__ import annotations

import base64
import os
import subprocess
import sys
import threading
import time
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


@pytest.mark.parametrize(
    ("field", "ferric_value", "clips_value", "reason"),
    [
        ("continued", False, True, "diagnostic-continued-mismatch"),
    ],
)
def test_classify_results_treats_known_diagnostic_mismatches_as_divergent(
    field,
    ferric_value,
    clips_value,
    reason,
):
    ferric_diagnostic = run_module.diagnostic("parse", "syntax-error", continued=False)
    clips_diagnostic = run_module.diagnostic("parse", "syntax-error", continued=False)
    ferric_diagnostic[field] = ferric_value
    clips_diagnostic[field] = clips_value

    result = run_module.classify_results(
        {**_engine_result(), "diagnostic": ferric_diagnostic},
        {**_engine_result(), "diagnostic": clips_diagnostic},
    )

    assert result == ("divergent", reason)


def test_classify_results_treats_known_phase_mismatch_as_divergent():
    result = run_module.classify_results(
        {
            **_engine_result(),
            "diagnostic": run_module.diagnostic("parse", "syntax-error", continued=False),
        },
        {
            **_engine_result(),
            "diagnostic": run_module.diagnostic("run", "evaluation-error", continued=False),
        },
    )

    assert result == ("divergent", "diagnostic-phase-mismatch")


def test_classify_results_never_calls_matching_terminal_diagnostics_equivalent():
    terminal = run_module.diagnostic("run", "evaluation-error", continued=False)

    result = run_module.classify_results(
        {**_engine_result(), "diagnostic": terminal},
        {**_engine_result(), "diagnostic": terminal},
    )

    assert result == ("incompatible", "diagnostic-match-without-complete-oracle")


def test_matching_pre_run_terminal_diagnostics_override_oracle_mismatches():
    digest = "a" * 64
    declaration = _oracle_declaration(digest)
    identity = {
        "fixture_id": declaration["id"],
        "nonce": declaration["nonce"],
        "source_sha256": digest,
        "composed_sha256": digest,
    }
    observation = _canonical_observation(identity, fact_id=1, rule="compute")
    observation.update(
        {
            "phase": "load",
            "firings": [],
            "effects": [],
            "facts": [],
            "diagnostic": {
                "phase": "load",
                "category": "construct-error",
                "continued": False,
            },
            "run": {"limit": None, "halt_reason": "not-run"},
        }
    )
    evaluation = run_module.evaluate_oracle(
        declaration,
        observation,
        observation,
        expected_source_sha256=digest,
        expected_composed_sha256=digest,
    )
    terminal = run_module.diagnostic("load", "construct-error", continued=False)
    result = {**_engine_result(exit_code=1), "diagnostic": terminal}
    result["termination"] = run_module.termination(exit_code=1, timed_out=False)

    assert evaluation.status is run_module.EvidenceStatus.VALID
    assert not evaluation.equivalent
    assert run_module._oracle_outcome(evaluation)[2]["completed"] is False
    assert run_module.classify_results(result, dict(result), evaluation) == (
        "incompatible",
        "diagnostic-match-without-complete-oracle",
    )


def test_complete_action_error_oracle_can_still_be_equivalent():
    digest = "a" * 64
    declaration = _oracle_declaration(digest)
    declaration["expectations"]["phase"] = "run"
    declaration["expectations"]["diagnostic"] = {
        "phase": "run",
        "category": "evaluation-error",
        "continued": False,
    }
    declaration["expectations"]["run"]["halt_reason"] = "action-error"
    identity = {
        "fixture_id": declaration["id"],
        "nonce": declaration["nonce"],
        "source_sha256": digest,
        "composed_sha256": digest,
    }
    observation = _canonical_observation(identity, fact_id=1, rule="compute")
    observation["phase"] = "run"
    observation["diagnostic"] = {
        "phase": "run",
        "category": "evaluation-error",
        "continued": False,
    }
    observation["run"]["halt_reason"] = "action-error"
    evaluation = run_module.evaluate_oracle(
        declaration,
        observation,
        observation,
        expected_source_sha256=digest,
        expected_composed_sha256=digest,
    )
    terminal = run_module.diagnostic("run", "evaluation-error", continued=False)
    result = {**_engine_result(), "diagnostic": terminal}
    result["termination"] = run_module.termination(exit_code=0, timed_out=False)

    assert evaluation.equivalent
    assert run_module._oracle_outcome(evaluation)[2]["completed"] is True
    assert run_module.classify_results(result, dict(result), evaluation) == (
        "equivalent",
        "oracle-v1-match",
    )


def test_matching_continued_diagnostics_still_require_oracle_evidence():
    continued = run_module.diagnostic("reset", "evaluation-error", continued=True)

    result = run_module.classify_results(
        {**_engine_result(), "diagnostic": continued},
        {**_engine_result(), "diagnostic": continued},
    )

    assert result == ("pending", "oracle-missing")


def test_matching_timeout_remains_terminal_after_a_continued_diagnostic():
    continued = run_module.diagnostic("reset", "evaluation-error", continued=True)
    result = {
        **_engine_result(exit_code=-1),
        "diagnostic": continued,
        "termination": {
            **run_module.termination(exit_code=None, timed_out=True),
            "active_phase": "run",
        },
    }

    assert run_module.classify_results(result, dict(result)) == (
        "incompatible",
        "termination-match-without-complete-oracle",
    )


def test_matching_nonzero_exit_cannot_be_equivalent_after_continued_diagnostic():
    continued = run_module.diagnostic("reset", "evaluation-error", continued=True)
    result = {
        **_engine_result(exit_code=7),
        "diagnostic": continued,
        "termination": run_module.termination(exit_code=7, timed_out=False),
    }

    assert run_module.classify_results(result, dict(result)) == (
        "incompatible",
        "termination-nonzero-exit-match",
    )


@pytest.mark.parametrize(
    ("untrusted", "expected"),
    [
        (run_module.diagnostic("unknown", "unknown", continued=False), "diagnostic-invalid"),
        (
            run_module.diagnostic("harness", "harness-error", continued=False),
            "harness-failure",
        ),
    ],
)
def test_untrusted_diagnostic_never_becomes_semantic_divergence(untrusted, expected):
    semantic = run_module.diagnostic("run", "evaluation-error", continued=False)

    result = run_module.classify_results(
        {**_engine_result(), "diagnostic": untrusted},
        {**_engine_result(), "diagnostic": semantic},
    )

    assert result == ("pending", expected)


def test_explicit_harness_failure_takes_precedence_over_semantic_diagnostic():
    semantic = run_module.diagnostic("run", "evaluation-error", continued=False)

    result = run_module.classify_results(
        {**_engine_result(), "diagnostic": semantic, "harness_error": True},
        {**_engine_result(), "diagnostic": semantic},
    )

    assert result == ("pending", "harness-failure")


def test_matching_semantic_diagnostics_preserve_termination_mismatch():
    semantic = run_module.diagnostic("run", "evaluation-error", continued=False)
    ferric = {
        **_engine_result(exit_code=1),
        "diagnostic": semantic,
        "termination": run_module.termination(exit_code=1, timed_out=False),
    }
    clips = {
        **_engine_result(exit_code=-9),
        "diagnostic": semantic,
        "termination": run_module.termination(exit_code=-9, timed_out=False),
    }

    assert run_module.classify_results(ferric, clips) == (
        "divergent",
        "termination-kind-mismatch",
    )


def test_timeout_active_phase_mismatch_is_divergent():
    timeout = run_module.diagnostic("process", "timeout", continued=False)
    ferric = {
        **_engine_result(exit_code=-1),
        "diagnostic": timeout,
        "termination": {
            **run_module.termination(exit_code=-1, timed_out=True),
            "active_phase": "load",
        },
    }
    clips = {
        **_engine_result(exit_code=-1),
        "diagnostic": timeout,
        "termination": {
            **run_module.termination(exit_code=-1, timed_out=True),
            "active_phase": "run",
        },
    }

    assert run_module.classify_results(ferric, clips) == (
        "divergent",
        "termination-active-phase-mismatch",
    )


def test_process_metadata_distinguishes_timeout_signal_exit_and_spawn_failure():
    timeout_result = {
        **_engine_result(exit_code=-1),
        "timed_out": True,
        "termination": run_module.termination(exit_code=-1, timed_out=True),
        "observation_error": "timed out",
    }
    signal_result = {
        **_engine_result(exit_code=-9),
        "termination": run_module.termination(exit_code=-9, timed_out=False),
    }
    exit_result = {
        **_engine_result(exit_code=7),
        "termination": run_module.termination(exit_code=7, timed_out=False),
    }
    spawn_result = {
        **_engine_result(exit_code=-1),
        "spawn_error": True,
        "termination": run_module.termination(
            exit_code=-1,
            timed_out=False,
            spawn_error=True,
        ),
    }

    assert timeout_result["termination"] == {
        "kind": "timeout",
        "exit_code": None,
        "signal": None,
    }
    assert signal_result["termination"] == {
        "kind": "signal",
        "exit_code": None,
        "signal": 9,
    }
    assert exit_result["termination"] == {
        "kind": "exit",
        "exit_code": 7,
        "signal": None,
    }
    assert spawn_result["termination"] == {
        "kind": "spawn-error",
        "exit_code": None,
        "signal": None,
    }
    assert run_module.process_diagnostic(timeout_result)["category"] == "timeout"
    assert run_module.process_diagnostic(signal_result)["category"] == "signal"
    assert run_module.process_diagnostic(exit_result)["category"] == "nonzero-exit"
    assert run_module.process_diagnostic(spawn_result)["category"] == "harness-error"
    assert (
        run_module.process_diagnostic(
            {"exit_code": -1, "timed_out": False, "stderr": "synthetic not-run result"}
        )
        is None
    )
    assert (
        run_module.process_diagnostic(
            {"exit_code": None, "observation_error": "skipped", "not_run": True}
        )
        is None
    )


def test_run_ferric_observer_retains_partial_timeout_output(monkeypatch, tmp_path):
    def timeout(*_args, **_kwargs):
        raise subprocess.TimeoutExpired(
            cmd=["ferric"],
            timeout=1,
            output=b'{"partial":true}',
            stderr=b"partial diagnostic",
        )

    monkeypatch.setattr(run_module.subprocess, "run", timeout)

    result = run_module.run_ferric_observer(
        str(tmp_path / "fixture.clp"),
        "ferric",
        str(tmp_path),
        1,
        fixture_id="fixture.timeout",
        nonce="0" * 32,
        source_sha256="a" * 64,
        composed_sha256="a" * 64,
    )

    assert result["stdout"] == '{"partial":true}'
    assert result["stderr"] == "partial diagnostic"
    assert base64.b64decode(result["raw_output"]["stdout"]) == b'{"partial":true}'
    assert base64.b64decode(result["raw_output"]["stderr"]) == b"partial diagnostic"
    assert result["termination"]["kind"] == "timeout"


def test_run_ferric_observer_converts_permission_error_to_spawn_failure(monkeypatch, tmp_path):
    def denied(*_args, **_kwargs):
        raise PermissionError("permission denied")

    monkeypatch.setattr(run_module.subprocess, "run", denied)

    result = run_module.run_ferric_observer(
        "fixture.clp",
        "./target/debug/ferric",
        str(tmp_path),
        1,
        fixture_id="fixture.spawn",
        nonce="0" * 32,
        source_sha256="a" * 64,
        composed_sha256="a" * 64,
    )

    assert result["termination"] == {
        "kind": "spawn-error",
        "exit_code": None,
        "signal": None,
    }
    assert result["spawn_error"] is True
    assert "permission denied" in result["stderr"]


def test_run_ferric_observer_preserves_signal_without_complete_json(monkeypatch, tmp_path):
    monkeypatch.setattr(
        run_module.subprocess,
        "run",
        lambda *_args, **_kwargs: subprocess.CompletedProcess(
            args=[],
            returncode=-9,
            stdout="",
            stderr="partial native stderr",
        ),
    )

    result = run_module.run_ferric_observer(
        "fixture.clp",
        "./target/debug/ferric",
        str(tmp_path),
        1,
        fixture_id="fixture.signal",
        nonce="0" * 32,
        source_sha256="a" * 64,
        composed_sha256="a" * 64,
    )
    projected = run_module._project_result(result, engine="ferric", harnessed=False)

    assert result["stdout"] == ""
    assert result["stderr"] == "partial native stderr"
    assert result["termination"] == {"kind": "signal", "exit_code": None, "signal": 9}
    assert result.get("harness_error") is None
    assert projected == {"observation_error": "observer was signaled before terminal evidence"}
    assert result["diagnostic"] == run_module.diagnostic("process", "signal", continued=False)


def test_run_clips_observer_converts_permission_error_to_spawn_failure(monkeypatch, tmp_path):
    source = tmp_path / "fixture.clp"
    source.write_text("(deffacts startup (ready))\n")

    def denied(*_args, **_kwargs):
        raise PermissionError("permission denied")

    monkeypatch.setattr(run_module, "_run_clips_process", denied)

    result = run_module.run_clips_observer(
        str(source),
        str(tmp_path),
        "scripts/clips-reference.sh",
        1,
        fixture_id="fixture.spawn",
        nonce="0" * 32,
        source_sha256="a" * 64,
        composed_sha256="a" * 64,
        globals_to_capture=(),
        harnessed=False,
    )

    assert result["termination"] == {
        "kind": "spawn-error",
        "exit_code": None,
        "signal": None,
    }
    assert result["spawn_error"] is True
    assert "permission denied" in result["stderr"]


@pytest.mark.parametrize(
    ("returncode", "expected_kind", "expected_signal"),
    [(137, "signal", 9), (163, "signal", 35), (127, "exit", None)],
)
def test_clips_wrapper_preserves_signal_and_internal_failure_status(
    monkeypatch,
    tmp_path,
    returncode,
    expected_kind,
    expected_signal,
):
    source = tmp_path / "fixture.clp"
    source.write_text("(deffacts startup (ready))\n")

    def completed(*_args, **_kwargs):
        return subprocess.CompletedProcess(
            args=[],
            returncode=returncode,
            stdout=b"partial stdout",
            stderr=b"partial stderr",
        )

    monkeypatch.setattr(run_module, "_run_clips_process", completed)
    monkeypatch.setattr(run_module, "parse_probe_output", lambda *_args, **_kwargs: {})

    result = run_module.run_clips_observer(
        str(source),
        str(tmp_path),
        "scripts/clips-reference.sh",
        1,
        fixture_id="fixture.process",
        nonce="0" * 32,
        source_sha256="a" * 64,
        composed_sha256="a" * 64,
        globals_to_capture=(),
        harnessed=False,
    )

    assert result["stdout"] == "partial stdout"
    assert result["stderr"] == "partial stderr"
    assert base64.b64decode(result["raw_output"]["stdout"]) == b"partial stdout"
    assert base64.b64decode(result["raw_output"]["stderr"]) == b"partial stderr"
    assert result["termination"]["kind"] == expected_kind
    if expected_signal is not None:
        assert result["termination"]["signal"] == expected_signal
        assert result.get("harness_error") is None
    else:
        assert result["harness_error"] is True
        assert "status 127" in result["observation_error"]
        projected = run_module._project_result(result, engine="clips", harnessed=False)
        assert projected == {"observation_error": result["observation_error"]}
        assert result["diagnostic"] == run_module.diagnostic(
            "harness", "harness-error", continued=False
        )


@pytest.mark.skipif(run_module.IS_WINDOWS, reason="POSIX process-group regression")
def test_run_clips_process_kills_descendants_holding_capture_fds(tmp_path):
    child = "import time; time.sleep(30)"
    parent = (
        "import subprocess, sys, time; "
        f"subprocess.Popen([sys.executable, '-c', {child!r}]); "
        "print('ready', flush=True); time.sleep(30)"
    )

    started = time.monotonic()
    with pytest.raises(subprocess.TimeoutExpired) as caught:
        run_module._run_clips_process(
            [sys.executable, "-c", parent],
            timeout_secs=0.1,
            root=str(tmp_path),
        )
    elapsed = time.monotonic() - started

    assert elapsed < 2
    assert b"ready" in (caught.value.stdout or b"")


def test_run_clips_process_force_removes_named_container_on_timeout(monkeypatch, tmp_path):
    class FakeProcess:
        pid = 4242
        returncode = -9

        def __init__(self):
            self.communications = 0

        def communicate(self, *, timeout):
            self.communications += 1
            if self.communications == 1:
                raise subprocess.TimeoutExpired([], timeout, output=b"partial")
            return b"partial", b"diagnostic"

    process = FakeProcess()
    removed: list[str] = []
    monkeypatch.setattr(run_module.subprocess, "Popen", lambda *_args, **_kwargs: process)
    monkeypatch.setattr(run_module, "_terminate_process_tree", lambda _process: None)
    monkeypatch.setattr(run_module, "_remove_clips_container", removed.append)

    with pytest.raises(subprocess.TimeoutExpired):
        run_module._run_clips_process(
            ["clips-reference"],
            timeout_secs=1,
            root=str(tmp_path),
            container_name="ferric-compat-" + "a" * 32,
        )

    assert removed == ["ferric-compat-" + "a" * 32]


@pytest.mark.parametrize(
    ("process_result", "expected_kind", "expected_category"),
    [
        (
            subprocess.CompletedProcess([], 137, b"partial stdout", b"partial stderr"),
            "signal",
            "signal",
        ),
        (
            subprocess.TimeoutExpired(
                [],
                1,
                output=b"partial stdout",
                stderr=b"partial stderr",
            ),
            "timeout",
            "timeout",
        ),
    ],
)
def test_interrupted_clips_parse_failure_preserves_process_termination(
    monkeypatch,
    tmp_path,
    process_result,
    expected_kind,
    expected_category,
):
    source = tmp_path / "fixture.clp"
    source.write_text("(deffacts startup (ready))\n")

    def run_process(*_args, **_kwargs):
        if isinstance(process_result, BaseException):
            raise process_result
        return process_result

    monkeypatch.setattr(run_module, "_run_clips_process", run_process)
    monkeypatch.setattr(
        run_module,
        "parse_probe_output",
        lambda *_args, **_kwargs: (_ for _ in ()).throw(ValueError("truncated record")),
    )

    result = run_module.run_clips_observer(
        str(source),
        str(tmp_path),
        "scripts/clips-reference.sh",
        1,
        fixture_id="fixture.interrupted",
        nonce="0" * 32,
        source_sha256="a" * 64,
        composed_sha256="a" * 64,
        globals_to_capture=(),
        harnessed=False,
    )
    projected = run_module._project_result(result, engine="clips", harnessed=False)

    assert result["stdout"] == "partial stdout"
    assert result["stderr"] == "partial stderr"
    assert result["termination"]["kind"] == expected_kind
    assert result.get("harness_error") is None
    assert projected == {
        "observation_error": "reference observer timed out before terminal evidence"
        if expected_kind == "timeout"
        else "reference observer was interrupted before parsable terminal evidence"
    }
    assert result["diagnostic"] == run_module.diagnostic(
        "process", expected_category, continued=False
    )


def test_interrupted_clips_retains_invalid_utf8_bytes_losslessly(monkeypatch, tmp_path):
    source = tmp_path / "fixture.clp"
    source.write_text("(deffacts startup (ready))\n")

    def timeout(*_args, **_kwargs):
        raise subprocess.TimeoutExpired([], 1, output=b"stdout \xc3", stderr=b"stderr \xff")

    monkeypatch.setattr(run_module, "_run_clips_process", timeout)

    result = run_module.run_clips_observer(
        str(source),
        str(tmp_path),
        "scripts/clips-reference.sh",
        1,
        fixture_id="fixture.invalid-utf8",
        nonce="0" * 32,
        source_sha256="a" * 64,
        composed_sha256="a" * 64,
        globals_to_capture=(),
        harnessed=False,
    )

    assert result["stdout"] == "stdout \ufffd"
    assert result["stderr"] == "stderr \ufffd"
    assert base64.b64decode(result["raw_output"]["stdout"]) == b"stdout \xc3"
    assert base64.b64decode(result["raw_output"]["stderr"]) == b"stderr \xff"
    assert result["termination"]["kind"] == "timeout"


def test_interrupted_clips_hard_protocol_corruption_is_harness_failure(
    monkeypatch,
    tmp_path,
):
    source = tmp_path / "fixture.clp"
    source.write_text("(deffacts startup (ready))\n")

    def timeout(*_args, **_kwargs):
        raise subprocess.TimeoutExpired([], 1, output=b"partial", stderr=b"corrupt")

    def corrupt(*_args, **_kwargs):
        raise run_module.ClipsOracleProtocolError("authenticated payload is invalid UTF-8")

    monkeypatch.setattr(run_module, "_run_clips_process", timeout)
    monkeypatch.setattr(run_module, "parse_probe_output", corrupt)

    result = run_module.run_clips_observer(
        str(source),
        str(tmp_path),
        "scripts/clips-reference.sh",
        1,
        fixture_id="fixture.corrupt",
        nonce="0" * 32,
        source_sha256="a" * 64,
        composed_sha256="a" * 64,
        globals_to_capture=(),
        harnessed=False,
    )
    projected = run_module._project_result(result, engine="clips", harnessed=False)

    assert result["termination"]["kind"] == "timeout"
    assert result["harness_error"] is True
    assert projected == {"observation_error": "authenticated payload is invalid UTF-8"}
    assert result["diagnostic"] == run_module.diagnostic(
        "harness", "harness-error", continued=False
    )


def test_partial_timeout_preserves_authenticated_active_phase():
    result = {
        **_engine_result(exit_code=-1),
        "timed_out": True,
        "observation_error": "observer timed out before terminal evidence",
        "termination": run_module.termination(exit_code=-1, timed_out=True),
        "observation": {
            "instrumentation": {"active_phase": "reset"},
        },
    }

    projected = run_module._project_result(
        result,
        engine="clips",
        harnessed=False,
    )

    assert projected == {"observation_error": "observer timed out before terminal evidence"}
    assert result["diagnostic"]["category"] == "timeout"
    assert result["termination"]["active_phase"] == "reset"


def test_signal_before_native_start_remains_process_signal():
    expected_fixture = {
        "id": "fixture.early-signal",
        "nonce": "0" * 32,
        "source_sha256": "a" * 64,
        "composed_sha256": "b" * 64,
    }
    result = {
        **_engine_result(exit_code=-9),
        "observation": {
            "schema": "ferric.compat-observation",
            "version": 1,
            "engine": {"name": "clips", "version": "test"},
            "fixture": dict(expected_fixture),
            "lifecycle": [],
            "diagnostics": [],
            "active_phase": None,
            "instrumentation": {"active_phase": None},
            "protocol_issues": [
                "native-phase-records-missing",
                "lifecycle-cardinality-or-order",
                "native-run-metadata-missing",
                "phase-cardinality-or-order",
                "module-cardinality",
            ],
        },
        "termination": run_module.termination(exit_code=-9, timed_out=False),
    }

    projected = run_module._project_result(
        result,
        engine="clips",
        harnessed=False,
        expected_fixture=expected_fixture,
    )

    assert projected == {"observation_error": "clips observer terminated as signal"}
    assert result["diagnostic"] == run_module.diagnostic("process", "signal", continued=False)


def test_signal_after_lifecycle_start_before_phase_begin_remains_process_signal():
    expected_fixture = {
        "id": "fixture.start-only",
        "nonce": "0" * 32,
        "source_sha256": "a" * 64,
        "composed_sha256": "b" * 64,
    }
    result = {
        **_engine_result(exit_code=-9),
        "observation": {
            "schema": "ferric.compat-observation",
            "version": 1,
            "engine": {"name": "clips", "version": "test"},
            "fixture": dict(expected_fixture),
            "phase_reached": "load",
            "lifecycle": [
                {
                    "sequence": 0,
                    "event": "start",
                    "fixture_id": expected_fixture["id"],
                    "nonce": expected_fixture["nonce"],
                    "source_sha256": expected_fixture["source_sha256"],
                    "composed_sha256": expected_fixture["composed_sha256"],
                }
            ],
            "diagnostics": [],
            "active_phase": None,
            "instrumentation": {"active_phase": None},
            "protocol_issues": [
                "native-phase-records-missing",
                "lifecycle-cardinality-or-order",
                "lifecycle-sequence-order",
                "native-run-metadata-missing",
                "phase-cardinality-or-order",
                "module-cardinality",
            ],
            "diagnostic_protocol_issues": [],
        },
        "termination": run_module.termination(exit_code=-9, timed_out=False),
    }

    projected = run_module._project_result(
        result,
        engine="clips",
        harnessed=False,
        expected_fixture=expected_fixture,
    )

    assert projected == {"observation_error": "clips observer terminated as signal"}
    assert result["diagnostic"] == run_module.diagnostic("process", "signal", continued=False)


def test_interrupted_authenticated_protocol_corruption_is_harness_failure():
    expected_fixture = {
        "id": "fixture.corrupt",
        "nonce": "0" * 32,
        "source_sha256": "a" * 64,
        "composed_sha256": "b" * 64,
    }
    result = {
        **_engine_result(exit_code=-9),
        "observation": {
            "schema": "ferric.compat-observation",
            "version": 1,
            "engine": {"name": "clips", "version": "test"},
            "fixture": dict(expected_fixture),
            "lifecycle": [
                {
                    "sequence": 0,
                    "event": "start",
                    "fixture_id": expected_fixture["id"],
                    "nonce": expected_fixture["nonce"],
                    "source_sha256": expected_fixture["source_sha256"],
                    "composed_sha256": expected_fixture["composed_sha256"],
                }
            ],
            "diagnostics": [],
            "active_phase": "load",
            "instrumentation": {"active_phase": "load"},
            "protocol_issues": ["truncated-native-record"],
        },
        "termination": run_module.termination(exit_code=-9, timed_out=False),
    }

    projected = run_module._project_result(
        result,
        engine="clips",
        harnessed=False,
        expected_fixture=expected_fixture,
    )

    assert "truncated-native-record" in projected["observation_error"]
    assert result["diagnostic"] == run_module.diagnostic(
        "harness", "harness-error", continued=False
    )
    assert result["termination"]["active_phase"] == "load"


def test_projection_failure_preserves_trusted_bound_semantic_diagnostic(monkeypatch):
    expected_fixture = {
        "id": "fixture.diagnostic",
        "nonce": "0" * 32,
        "source_sha256": "a" * 64,
        "composed_sha256": "b" * 64,
    }
    observation = {
        "schema": "ferric.compat-observation",
        "version": 1,
        "engine": {"name": "ferric", "version": "test"},
        "fixture": dict(expected_fixture),
        "phase_reached": "parse",
        "run": None,
        "lifecycle": [
            {
                "sequence": sequence,
                "event": event,
                "fixture_id": expected_fixture["id"],
                "nonce": expected_fixture["nonce"],
                "source_sha256": expected_fixture["source_sha256"],
                "composed_sha256": expected_fixture["composed_sha256"],
            }
            for sequence, event in ((0, "start"), (1, "complete"))
        ],
        "diagnostics": [
            {
                "taxonomy_version": 1,
                "phase": "parse",
                "category": "syntax-error",
                "continued": False,
                "severity": "error",
                "message": "raw parse message",
            }
        ],
    }
    result = {
        **_engine_result(exit_code=1),
        "observation": observation,
        "termination": run_module.termination(exit_code=1, timed_out=False),
    }

    def incomplete_projection(*_args, **_kwargs):
        raise run_module.ObservationProjectionError("terminal state is incomplete")

    monkeypatch.setattr(run_module, "project_ferric_observation", incomplete_projection)

    projected = run_module._project_result(
        result,
        engine="ferric",
        harnessed=False,
        expected_fixture=expected_fixture,
    )

    assert projected == {"observation_error": "terminal state is incomplete"}
    assert result["diagnostic"] == run_module.diagnostic("parse", "syntax-error", continued=False)
    assert result["observation"]["diagnostics"][0]["message"] == "raw parse message"


def test_projection_does_not_preserve_diagnostic_contradicted_by_run_state():
    expected_fixture = {
        "id": "fixture.contradiction",
        "nonce": "0" * 32,
        "source_sha256": "a" * 64,
        "composed_sha256": "b" * 64,
    }
    observation = {
        "schema": "ferric.compat-observation",
        "version": 1,
        "engine": {"name": "ferric", "version": "test"},
        "fixture": dict(expected_fixture),
        "phase_reached": "run",
        "run": {"halt_reason": "agenda_empty"},
        "lifecycle": [
            {
                "sequence": sequence,
                "event": event,
                "fixture_id": expected_fixture["id"],
                "nonce": expected_fixture["nonce"],
                "source_sha256": expected_fixture["source_sha256"],
                "composed_sha256": expected_fixture["composed_sha256"],
            }
            for sequence, event in ((0, "start"), (1, "complete"))
        ],
        "diagnostics": [
            {
                "taxonomy_version": 1,
                "phase": "run",
                "category": "evaluation-error",
                "continued": False,
                "severity": "error",
                "message": "claimed action failure",
            }
        ],
    }
    result = {
        **_engine_result(),
        "observation": observation,
        "termination": run_module.termination(exit_code=0, timed_out=False),
    }

    projected = run_module._project_result(
        result,
        engine="ferric",
        harnessed=False,
        expected_fixture=expected_fixture,
    )

    assert "lacks an action-error halt" in projected["observation_error"]
    assert result["diagnostic"] == run_module.diagnostic(
        "harness", "harness-error", continued=False
    )


@pytest.mark.parametrize(("exit_code", "timed_out"), [(-1, True), (-15, False)])
def test_interruption_preserves_prior_trusted_semantic_diagnostic(exit_code, timed_out):
    expected_fixture = {
        "id": "fixture.interrupted",
        "nonce": "0" * 32,
        "source_sha256": "a" * 64,
        "composed_sha256": "b" * 64,
    }
    observation = {
        "schema": "ferric.compat-observation",
        "version": 1,
        "engine": {"name": "clips", "version": "test"},
        "fixture": dict(expected_fixture),
        "lifecycle": [
            {
                "sequence": 0,
                "event": "start",
                "fixture_id": expected_fixture["id"],
                "nonce": expected_fixture["nonce"],
                "source_sha256": expected_fixture["source_sha256"],
                "composed_sha256": expected_fixture["composed_sha256"],
            }
        ],
        "diagnostics": [
            {
                "taxonomy_version": 1,
                "phase": "reset",
                "category": "evaluation-error",
                "continued": True,
                "channel": "stderr",
                "message": "reset evaluation error",
            }
        ],
        "phase_reached": "run",
        "active_phase": "run",
        "run": None,
        "instrumentation": {"active_phase": "run"},
        "protocol_issues": ["lifecycle-cardinality-or-order"],
    }
    result = {
        **_engine_result(exit_code=exit_code),
        "timed_out": timed_out,
        "observation": observation,
        "observation_error": "observer was interrupted",
        "termination": run_module.termination(exit_code=exit_code, timed_out=timed_out),
    }

    projected = run_module._project_result(
        result,
        engine="clips",
        harnessed=False,
        expected_fixture=expected_fixture,
    )

    assert projected == {"observation_error": "observer was interrupted"}
    assert result["diagnostic"] == run_module.diagnostic(
        "reset", "evaluation-error", continued=True
    )
    assert result["termination"]["kind"] == ("timeout" if timed_out else "signal")
    assert result["termination"]["active_phase"] == "run"


def test_unknown_trusted_semantic_taxonomy_stays_unknown_not_harness(monkeypatch):
    expected_fixture = {
        "id": "fixture.unknown",
        "nonce": "0" * 32,
        "source_sha256": "a" * 64,
        "composed_sha256": "b" * 64,
    }
    observation = {
        "schema": "ferric.compat-observation",
        "version": 1,
        "engine": {"name": "clips", "version": "test"},
        "fixture": dict(expected_fixture),
        "lifecycle": [
            {
                "sequence": sequence,
                "event": event,
                "fixture_id": expected_fixture["id"],
                "nonce": expected_fixture["nonce"],
                "source_sha256": expected_fixture["source_sha256"],
                "composed_sha256": expected_fixture["composed_sha256"],
            }
            for sequence, event in ((0, "start"), (3, "complete"))
        ],
        "diagnostics": [
            {
                "taxonomy_version": 1,
                "phase": "unknown",
                "category": "unknown",
                "continued": False,
                "channel": "stderr",
                "message": "[NEWCODE1] unclassified native diagnostic",
            }
        ],
        "protocol_issues": [],
    }
    result = {
        **_engine_result(exit_code=1),
        "observation": observation,
        "termination": run_module.termination(exit_code=1, timed_out=False),
    }

    def incomplete_projection(*_args, **_kwargs):
        raise run_module.ObservationProjectionError("unknown diagnostic taxonomy")

    monkeypatch.setattr(run_module, "project_clips_observation", incomplete_projection)

    projected = run_module._project_result(
        result,
        engine="clips",
        harnessed=False,
        expected_fixture=expected_fixture,
    )

    assert projected == {"observation_error": "unknown diagnostic taxonomy"}
    assert result["diagnostic"] == run_module.diagnostic("unknown", "unknown", continued=False)


def test_unbound_semantic_diagnostic_is_harness_failure():
    expected_fixture = {
        "id": "fixture.expected",
        "nonce": "0" * 32,
        "source_sha256": "a" * 64,
        "composed_sha256": "b" * 64,
    }
    observation = {
        "schema": "ferric.compat-observation",
        "version": 1,
        "engine": {"name": "ferric", "version": "test"},
        "fixture": {**expected_fixture, "id": "fixture.spoofed"},
        "diagnostics": [
            {
                "taxonomy_version": 1,
                "phase": "run",
                "category": "evaluation-error",
                "continued": False,
            }
        ],
    }
    result = {
        **_engine_result(exit_code=1),
        "observation": observation,
        "termination": run_module.termination(exit_code=1, timed_out=False),
    }

    projected = run_module._project_result(
        result,
        engine="ferric",
        harnessed=False,
        expected_fixture=expected_fixture,
    )

    assert "does not match the invocation" in projected["observation_error"]
    assert result["diagnostic"] == run_module.diagnostic(
        "harness", "harness-error", continued=False
    )


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
    assert reason == "harness-failure"
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


@pytest.mark.parametrize(
    ("reason", "heading"),
    [
        ("oracle-invalid:markers", "Invalid oracle evidence (1)"),
        ("diagnostic-invalid", "Invalid runtime evidence (1)"),
        ("diagnostic-missing", "Invalid runtime evidence (1)"),
        ("harness-failure", "Invalid runtime evidence (1)"),
        ("termination-invalid", "Invalid runtime evidence (1)"),
        ("termination-missing", "Invalid runtime evidence (1)"),
    ],
)
def test_runner_persists_invalid_evidence_before_nonzero_exit(
    tmp_path,
    monkeypatch,
    reason,
    heading,
):
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
            reason,
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
    assert heading in result.output
    persisted = load_manifest(manifest_path)
    persisted_entry = persisted["files"]["fixture.clp"]
    assert persisted_entry["classification"] == "pending"
    assert persisted_entry["reason"] == reason
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
    observer_container_name = "ferric-compat-" + "d" * 32
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
            "--observer-container-name",
            observer_container_name,
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
    assert observer_args[observer_args.index("--name") + 1] == observer_container_name
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
