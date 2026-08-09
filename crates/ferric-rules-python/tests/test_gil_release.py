"""FR-PY-002 conformance tests for releasing the GIL around native work."""

import os
import subprocess
import sys
import threading
from pathlib import Path

import pytest
import ferric


_BLOCKING_SWITCH_INTERVAL_SECONDS = 1_000.0
_HANDOFF_FIXTURE = Path(__file__).with_name("fixtures") / "gil_release_handoff.py"


class _RedirectablePath:
    """PathLike whose target can change while the native call is detached."""

    def __init__(self, path):
        self.path = path
        self.fspath_calls = 0

    def __fspath__(self):
        self.fspath_calls += 1
        return str(self.path)


def _large_deffacts_source(fact_count):
    return (
        "(deffacts bulk "
        + " ".join(f"(payload {index})" for index in range(fact_count))
        + ")"
    )


def _large_json_snapshot(fact_count):
    engine = ferric.Engine()
    engine.assert_string(
        " ".join(
            f"(payload {index} {index % 97} token-{index})"
            for index in range(fact_count)
        )
    )
    return engine.serialize(format=ferric.Format.JSON)


def _call_with_parked_python_thread(call, *, worker_call=None):
    """Call native work and report whether a parked Python thread ran inside it."""
    ready = threading.Event()
    gate = threading.Lock()
    gate.acquire()
    progressed = threading.Event()
    worker_errors = []

    def worker():
        ready.set()
        gate.acquire()
        try:
            if worker_call is not None:
                worker_call()
        except BaseException as exc:
            worker_errors.append(exc)
        finally:
            progressed.set()
            gate.release()

    thread = threading.Thread(target=worker)
    thread.start()
    assert ready.wait(timeout=5), "progress worker did not park"

    previous_interval = sys.getswitchinterval()
    sys.setswitchinterval(_BLOCKING_SWITCH_INTERVAL_SECONDS)
    try:
        gate.release()
        progressed_before_call = progressed.is_set()
        result = call()
        progressed_during_call = progressed.is_set()
    finally:
        sys.setswitchinterval(previous_interval)
        thread.join(timeout=5)

    assert not thread.is_alive(), "progress worker did not finish"
    assert not progressed_before_call, "worker ran before the native call"
    assert worker_errors == []
    assert progressed_during_call, "native call retained the GIL"
    return result


def test_long_run_releases_gil_and_preserves_result():
    engine = ferric.Engine.from_source(
        "(defrule consume ?fact <- (work ?value) => (retract ?fact))"
    )
    fact_count = 20_000
    engine.assert_string(" ".join(f"(work {index})" for index in range(fact_count)))

    result = _call_with_parked_python_thread(engine.run)

    assert result.rules_fired == fact_count
    assert result.halt_reason == ferric.HaltReason.AGENDA_EMPTY
    assert engine.fact_count == 0


def test_large_serialization_releases_gil_and_roundtrips():
    engine = ferric.Engine()
    fact_count = 20_000
    engine.assert_string(
        " ".join(
            f"(payload {index} {index % 97} token-{index})"
            for index in range(fact_count)
        )
    )

    snapshot = _call_with_parked_python_thread(
        lambda: engine.serialize(format=ferric.Format.JSON)
    )

    assert isinstance(snapshot, bytes)
    restored = ferric.Engine.from_snapshot(snapshot, format=ferric.Format.JSON)
    assert restored.fact_count == fact_count


def test_large_from_source_releases_gil_and_preserves_state():
    fact_count = 50_000
    source = _large_deffacts_source(fact_count)

    engine = _call_with_parked_python_thread(lambda: ferric.Engine.from_source(source))

    assert engine.fact_count == fact_count


def test_large_load_releases_gil_and_preserves_state():
    fact_count = 50_000
    source = _large_deffacts_source(fact_count)
    engine = ferric.Engine()

    _call_with_parked_python_thread(lambda: engine.load(source))

    engine.reset()
    assert engine.fact_count == fact_count


def test_large_load_file_releases_gil_and_preserves_state(tmp_path):
    fact_count = 50_000
    source = _large_deffacts_source(fact_count)
    path = tmp_path / "large.clp"
    path.write_text(source, encoding="utf-8")
    path_argument = _RedirectablePath(path)
    engine = ferric.Engine()

    _call_with_parked_python_thread(
        lambda: engine.load_file(path_argument),
        worker_call=lambda: setattr(path_argument, "path", tmp_path / "missing.clp"),
    )

    assert path_argument.fspath_calls == 1
    engine.reset()
    assert engine.fact_count == fact_count


def test_large_from_snapshot_releases_gil_and_preserves_state():
    fact_count = 20_000
    snapshot = _large_json_snapshot(fact_count)

    restored = _call_with_parked_python_thread(
        lambda: ferric.Engine.from_snapshot(snapshot, format=ferric.Format.JSON)
    )

    assert restored.fact_count == fact_count


def test_large_save_snapshot_releases_gil_and_preserves_file(tmp_path):
    fact_count = 20_000
    engine = ferric.Engine.from_snapshot(
        _large_json_snapshot(fact_count), format=ferric.Format.JSON
    )
    path = tmp_path / "large.json"
    alternate_path = tmp_path / "wrong.json"
    path_argument = _RedirectablePath(path)

    _call_with_parked_python_thread(
        lambda: engine.save_snapshot(path_argument, format=ferric.Format.JSON),
        worker_call=lambda: setattr(path_argument, "path", alternate_path),
    )

    assert path_argument.fspath_calls == 1
    assert not alternate_path.exists()
    restored = ferric.Engine.from_snapshot_file(path, format=ferric.Format.JSON)
    assert restored.fact_count == fact_count


def test_large_from_snapshot_file_releases_gil_and_preserves_state(tmp_path):
    fact_count = 20_000
    path = tmp_path / "large.json"
    path.write_bytes(_large_json_snapshot(fact_count))
    path_argument = _RedirectablePath(path)

    restored = _call_with_parked_python_thread(
        lambda: ferric.Engine.from_snapshot_file(
            path_argument, format=ferric.Format.JSON
        ),
        worker_call=lambda: setattr(path_argument, "path", tmp_path / "missing.json"),
    )

    assert path_argument.fspath_calls == 1
    assert restored.fact_count == fact_count


def test_detached_operations_preserve_result_and_error_mapping(tmp_path):
    malformed_source = "(defrule incomplete"
    malformed_source_path = tmp_path / "malformed.clp"
    malformed_source_path.write_text(malformed_source, encoding="utf-8")
    malformed_snapshot_path = tmp_path / "malformed.bin"
    malformed_snapshot_path.write_bytes(b"not a snapshot")
    missing_parent = tmp_path / "missing" / "snapshot.bin"

    with pytest.raises(ferric.FerricParseError, match="unclosed parenthesis"):
        ferric.Engine.from_source(malformed_source)
    with pytest.raises(ferric.FerricParseError, match="unclosed parenthesis"):
        ferric.Engine().load(malformed_source)
    with pytest.raises(ferric.FerricParseError, match="unclosed parenthesis"):
        ferric.Engine().load_file(malformed_source_path)
    with pytest.raises(ferric.FerricError, match="deserialization failed"):
        ferric.Engine.from_snapshot(b"not a snapshot")
    with pytest.raises(ferric.FerricError, match="deserialization failed"):
        ferric.Engine.from_snapshot_file(malformed_snapshot_path)
    with pytest.raises(OSError):
        ferric.Engine().save_snapshot(missing_parent)

    action_error = ferric.Engine.from_source("(defrule fail => (/ 1 0))").run()
    assert action_error.rules_fired == 1
    assert action_error.halt_reason == ferric.HaltReason.ACTION_ERROR


def test_same_rule_action_error_wins_over_native_halt_state():
    engine = ferric.Engine.from_source("(defrule halt-then-fault => (halt) (/ 1 0))")

    result = engine.run()

    assert engine.is_halted
    assert result.rules_fired == 1
    assert result.halt_reason == ferric.HaltReason.ACTION_ERROR
    assert len(engine.diagnostics) == 1
    assert "division by zero" in engine.diagnostics[0]


def test_zero_limit_run_clears_prior_halt_and_action_diagnostics():
    engine = ferric.Engine.from_source("(defrule halt-then-fault => (halt) (/ 1 0))")
    first = engine.run()
    assert first.halt_reason == ferric.HaltReason.ACTION_ERROR
    assert engine.is_halted
    assert len(engine.diagnostics) == 1

    result = engine.run(limit=0)

    assert result.rules_fired == 0
    assert result.halt_reason == ferric.HaltReason.LIMIT_REACHED
    assert not engine.is_halted
    assert engine.diagnostics == []


def test_rule_halt_on_detached_chunk_boundary_repairs_limit_result():
    engine = ferric.Engine.from_source("""
        (defrule count-to-boundary
            ?fact <- (counter ?value&:(< ?value 65))
            =>
            (retract ?fact)
            (assert (counter (+ ?value 1)))
            (if (= ?value 63) then (halt)))
        (deffacts initial (counter 0))
    """)

    result = engine.run(limit=64)

    assert result.rules_fired == 64
    assert result.halt_reason == ferric.HaltReason.HALT_REQUESTED


def test_idle_and_closed_halt_are_idempotent_and_do_not_latch():
    engine = ferric.Engine.from_source(
        "(deffacts initial (ready)) (defrule fire (ready) => (assert (done)))"
    )

    assert engine.halt() is None
    assert engine.halt() is None
    assert not engine.is_halted
    result = engine.run()
    assert result.rules_fired == 1
    assert result.halt_reason == ferric.HaltReason.AGENDA_EMPTY
    assert not engine.is_halted

    engine.close()
    assert engine.halt() is None


def test_foreign_idle_and_closed_halt_are_noops():
    engine = ferric.Engine()
    results = []
    errors = []

    def halt_from_foreign_thread():
        try:
            results.append(engine.halt())
        except BaseException as exc:
            errors.append(exc)

    thread = threading.Thread(target=halt_from_foreign_thread)
    thread.start()
    thread.join(timeout=5)

    assert not thread.is_alive()
    assert errors == []
    assert results == [None]
    assert not engine.is_halted

    engine.close()
    thread = threading.Thread(target=halt_from_foreign_thread)
    thread.start()
    thread.join(timeout=5)

    assert not thread.is_alive()
    assert errors == []
    assert results == [None, None]


@pytest.mark.parametrize(
    "scenario",
    [
        "foreign_halt",
        "foreign_close",
        "close_serialize",
        "wrong_thread_during_run",
    ],
)
def test_active_native_phase_lifecycle_handoff_is_bounded(scenario):
    result = subprocess.run(
        [sys.executable, str(_HANDOFF_FIXTURE), scenario],
        check=False,
        capture_output=True,
        text=True,
        timeout=10,
    )

    assert result.returncode == 0, result.stdout + result.stderr
    assert result.stdout == f"ok:{scenario}\n"
    assert result.stderr == ""


@pytest.mark.skipif(not hasattr(os, "mkfifo"), reason="requires POSIX FIFOs")
@pytest.mark.parametrize(
    "scenario",
    ["load_fifo", "save_fifo", "from_snapshot_fifo"],
)
def test_file_operation_releases_gil_for_fifo_handoff(scenario):
    result = subprocess.run(
        [sys.executable, str(_HANDOFF_FIXTURE), scenario],
        check=False,
        capture_output=True,
        text=True,
        timeout=10,
    )

    assert result.returncode == 0, result.stdout + result.stderr
    assert result.stdout == f"ok:{scenario}\n"
    assert result.stderr == ""
