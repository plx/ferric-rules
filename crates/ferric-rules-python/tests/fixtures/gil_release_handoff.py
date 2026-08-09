"""Subprocess handoffs for GIL release and same-engine lifecycle ordering."""

import os
import sys
import tempfile
import threading
import time
from pathlib import Path

import ferric


_BLOCKING_SWITCH_INTERVAL_SECONDS = 1_000.0


def _with_worker_handoff(
    owner_call, worker_call, *, repeat_worker_until_owner_returns=False
):
    ready = threading.Event()
    gate = threading.Lock()
    gate.acquire()
    entered = threading.Event()
    owner_done = threading.Event()
    worker_results = []
    worker_errors = []

    def worker():
        ready.set()
        gate.acquire()
        entered.set()
        try:
            if repeat_worker_until_owner_returns:
                results = []
                while not owner_done.is_set() or len(results) < 2:
                    results.append(worker_call())
                    time.sleep(0)
                worker_results.append(results)
            else:
                worker_results.append(worker_call())
        except BaseException as exc:  # surfaced on the owner thread below
            worker_errors.append(exc)
        finally:
            gate.release()

    # A failing owner operation must not keep this subprocess alive forever if
    # the worker is blocked on the other end of a FIFO. Successful cases still
    # require the worker to finish below.
    thread = threading.Thread(target=worker, daemon=True)
    thread.start()
    assert ready.wait(timeout=5), "handoff worker did not park"

    previous_interval = sys.getswitchinterval()
    sys.setswitchinterval(_BLOCKING_SWITCH_INTERVAL_SECONDS)
    try:
        gate.release()
        entered_before_call = entered.is_set()
        owner_result = owner_call()
        entered_during_call = entered.is_set()
    finally:
        owner_done.set()
        sys.setswitchinterval(previous_interval)
        thread.join(timeout=5)

    assert not thread.is_alive(), "handoff worker did not finish"
    assert not entered_before_call, "worker ran before the native call"
    assert entered_during_call, "native call retained the GIL"
    assert worker_errors == []
    return owner_result, worker_results


def _looping_engine():
    return ferric.Engine.from_source(
        """
        (deffacts initial (counter 0))
        (defrule loop
            ?fact <- (counter ?value)
            =>
            (retract ?fact)
            (assert (counter (+ ?value 1))))
        """
    )


def _foreign_halt():
    engine = _looping_engine()

    result, worker_results = _with_worker_handoff(
        engine.run, engine.halt, repeat_worker_until_owner_returns=True
    )

    assert len(worker_results) == 1
    assert len(worker_results[0]) >= 2
    assert all(value is None for value in worker_results[0])
    assert result.halt_reason == ferric.HaltReason.HALT_REQUESTED
    assert not engine.is_halted
    continued = engine.run(limit=1)
    assert continued.rules_fired == 1
    assert continued.halt_reason == ferric.HaltReason.LIMIT_REACHED
    engine.close()


def _foreign_close():
    baseline = (
        ferric.engine_instance_count()
        if hasattr(ferric, "engine_instance_count")
        else None
    )
    engine = _looping_engine()
    if baseline is not None:
        assert ferric.engine_instance_count() == baseline + 1

    result, worker_results = _with_worker_handoff(engine.run, engine.close)

    assert worker_results == [None]
    assert result.halt_reason == ferric.HaltReason.HALT_REQUESTED
    if baseline is not None:
        assert ferric.engine_instance_count() == baseline
    assert engine.close() is None


def _close_during_serialize():
    engine = ferric.Engine()
    fact_count = 20_000
    engine.assert_string(
        " ".join(
            f"(payload {index} {index % 97} token-{index})"
            for index in range(fact_count)
        )
    )

    snapshot, worker_results = _with_worker_handoff(
        lambda: engine.serialize(format=ferric.Format.JSON), engine.close
    )

    assert worker_results == [None]
    restored = ferric.Engine.from_snapshot(snapshot, format=ferric.Format.JSON)
    assert restored.fact_count == fact_count
    restored.close()


def _wrong_thread_during_run():
    engine = ferric.Engine.from_source(
        "(defrule consume ?fact <- (work ?value) => (retract ?fact))"
    )
    fact_count = 20_000
    engine.assert_string(" ".join(f"(work {index})" for index in range(fact_count)))

    def call_from_wrong_thread():
        try:
            engine.fact_count
        except ferric.FerricRuntimeError as exc:
            return str(exc)
        raise AssertionError("ordinary foreign call unexpectedly succeeded")

    result, worker_results = _with_worker_handoff(engine.run, call_from_wrong_thread)

    assert result.rules_fired == fact_count
    assert result.halt_reason == ferric.HaltReason.AGENDA_EMPTY
    assert len(worker_results) == 1
    assert "wrong thread" in worker_results[0]


def _load_fifo(directory):
    path = directory / "source.fifo"
    os.mkfifo(path)
    engine = ferric.Engine()
    fact_count = 2_000
    source = (
        "(deffacts fifo "
        + " ".join(f"(payload {index})" for index in range(fact_count))
        + ")"
    )

    def write_source():
        path.write_text(source, encoding="utf-8")

    _, worker_results = _with_worker_handoff(
        lambda: engine.load_file(path), write_source
    )

    assert worker_results == [None]
    engine.reset()
    assert engine.fact_count == fact_count


def _save_fifo(directory):
    path = directory / "snapshot.fifo"
    os.mkfifo(path)
    engine = ferric.Engine()
    engine.assert_fact("fifo", "save")

    def read_snapshot():
        return path.read_bytes()

    _, worker_results = _with_worker_handoff(
        lambda: engine.save_snapshot(path), read_snapshot
    )

    assert len(worker_results) == 1
    restored = ferric.Engine.from_snapshot(worker_results[0])
    assert restored.fact_count == 1


def _from_snapshot_fifo(directory):
    path = directory / "snapshot.fifo"
    os.mkfifo(path)
    source = ferric.Engine()
    source.assert_fact("fifo", "load")
    snapshot = source.serialize()
    source.close()

    def write_snapshot():
        path.write_bytes(snapshot)

    restored, worker_results = _with_worker_handoff(
        lambda: ferric.Engine.from_snapshot_file(path), write_snapshot
    )

    assert worker_results == [None]
    assert restored.fact_count == 1


def main():
    scenario = sys.argv[1]
    if scenario == "foreign_halt":
        _foreign_halt()
    elif scenario == "foreign_close":
        _foreign_close()
    elif scenario == "close_serialize":
        _close_during_serialize()
    elif scenario == "wrong_thread_during_run":
        _wrong_thread_during_run()
    else:
        if not hasattr(os, "mkfifo"):
            raise RuntimeError("FIFO scenarios require os.mkfifo")
        with tempfile.TemporaryDirectory() as raw_directory:
            directory = Path(raw_directory)
            if scenario == "load_fifo":
                _load_fifo(directory)
            elif scenario == "save_fifo":
                _save_fifo(directory)
            elif scenario == "from_snapshot_fifo":
                _from_snapshot_fifo(directory)
            else:
                raise AssertionError(f"unknown scenario: {scenario}")
    print(f"ok:{scenario}")


if __name__ == "__main__":
    main()
