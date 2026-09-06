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

    def close_admitted_run():
        deadline = time.monotonic() + 5
        while not ferric.engine_run_active(engine):
            assert time.monotonic() < deadline, "native run was not admitted"
            time.sleep(0)
        return engine.close()

    result, worker_results = _with_worker_handoff(engine.run, close_admitted_run)

    assert worker_results == [None]
    assert result.halt_reason == ferric.HaltReason.HALT_REQUESTED
    if baseline is not None:
        assert ferric.engine_instance_count() == baseline
    assert engine.close() is None


def _waiting_read_during_run():
    engine = ferric.Engine.from_source(
        "(defrule consume ?fact <- (work ?value) => (retract ?fact))"
    )
    fact_count = 20_000
    engine.assert_string(" ".join(f"(work {index})" for index in range(fact_count)))

    result, worker_results = _with_worker_handoff(engine.run, lambda: engine.fact_count)
    assert result.rules_fired == fact_count
    assert result.halt_reason == ferric.HaltReason.AGENDA_EMPTY
    assert worker_results == [0], "the waiting read must observe the completed run"


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


def _contended_load_fifo(directory, *, close):
    """Hold admitted I/O until readers/closers have detached while waiting."""
    path = directory / "contended-source.fifo"
    os.mkfifo(path)
    baseline = ferric.engine_instance_count()
    engine = ferric.Engine()
    admitted = threading.Event()
    release = threading.Event()
    results = []
    errors = []

    def capture(call):
        try:
            results.append(call())
        except BaseException as exc:
            errors.append(exc)

    def write_source():
        # Opening the writer can finish only after the native reader has
        # entered load_file with the engine mutex held. Keep EOF withheld.
        with path.open("w", encoding="utf-8") as stream:
            admitted.set()
            assert release.wait(timeout=5)
            stream.write("(defrule loaded (payload ?value) =>)")

    loader = threading.Thread(
        target=lambda: capture(lambda: engine.load_file(path)), daemon=True
    )
    writer = threading.Thread(target=lambda: capture(write_source), daemon=True)
    loader.start()
    writer.start()
    assert admitted.wait(timeout=5), "native load did not enter FIFO read"

    waiting = []
    previous_interval = sys.getswitchinterval()
    sys.setswitchinterval(_BLOCKING_SWITCH_INTERVAL_SECONDS)
    try:
        for _ in range(2 if close else 1):
            attempted = threading.Event()

            def wait_for_engine(attempted=attempted):
                attempted.set()
                # No Python wait after publishing: with the long switch
                # interval the coordinator progresses only once this call
                # releases the GIL while waiting for native admission.
                capture(engine.close if close else lambda: len(engine.rules()))

            thread = threading.Thread(target=wait_for_engine, daemon=True)
            thread.start()
            assert attempted.wait(timeout=5), "contending thread did not enter"
            waiting.append(thread)
        assert all(thread.is_alive() for thread in waiting)
        assert ferric.engine_instance_count() == baseline + 1
        assert results == [], "admitted I/O must finish before waiters return"
    finally:
        release.set()
        sys.setswitchinterval(previous_interval)
        for thread in [writer, loader, *waiting]:
            thread.join(timeout=5)

    assert all(not thread.is_alive() for thread in [writer, loader, *waiting])
    assert errors == []
    if close:
        assert results == [None] * 4
        assert ferric.engine_instance_count() == baseline
        try:
            engine.rules()
        except ferric.FerricRuntimeError as exc:
            assert "engine has been closed" in str(exc)
        else:
            raise AssertionError("closed engine remained accessible")
    else:
        assert results.count(None) == 2
        assert results.count(1) == 1, "waiting read must observe the loaded rule"
        engine.close()


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


def _close_during_save_fifo(directory):
    path = directory / "closing-snapshot.fifo"
    os.mkfifo(path)
    engine = ferric.Engine()
    engine.assert_fact("saved", 7)
    admitted = threading.Event()
    release = threading.Event()
    closing_started = threading.Event()
    snapshots = []
    errors = []

    def read_snapshot():
        with path.open("rb") as stream:
            # The writer opens only after serialization, with the native
            # mutex held across file I/O. Withhold reads until close waits.
            admitted.set()
            assert release.wait(timeout=5)
            snapshots.append(stream.read())

    def close_admitted_save():
        assert admitted.wait(timeout=5)
        try:
            closing_started.set()
            engine.close()
        except BaseException as exc:
            errors.append(exc)

    # Make the serialized write exceed the FIFO capacity so EOF stays pending.
    engine.assert_fact("large", "payload" * 100_000)
    previous_interval = sys.getswitchinterval()
    sys.setswitchinterval(_BLOCKING_SWITCH_INTERVAL_SECONDS)
    reader = threading.Thread(target=read_snapshot, daemon=True)
    reader.start()
    closer = threading.Thread(target=close_admitted_save, daemon=True)
    closer.start()

    def release_reader():
        assert closing_started.wait(timeout=5)
        release.set()

    releaser = threading.Thread(target=release_reader, daemon=True)
    releaser.start()
    try:
        assert engine.save_snapshot(path) is None
    finally:
        sys.setswitchinterval(previous_interval)
    for thread in [reader, closer, releaser]:
        thread.join(timeout=5)
        assert not thread.is_alive()
    assert errors == []
    restored = ferric.Engine.from_snapshot(snapshots[0])
    assert restored.fact_count == 2
    restored.close()
    assert engine.close() is None


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


def _conversion_reentry():
    engine = ferric.Engine()
    results = []

    class Meta(type):
        def __getattribute__(cls, name):
            # Unsupported-value errors inspect the Python type's name. A
            # metaclass makes that ordinary conversion path reenter publicly.
            if name == "__name__":
                for operation in (
                    lambda: engine.fact_count,
                    lambda: engine.load("(defrule next =>)"),
                    engine.close,
                ):
                    try:
                        operation()
                    except ferric.FerricRuntimeError as exc:
                        results.append(str(exc))
                    else:
                        results.append("unexpected success")
            return super().__getattribute__(name)

    class Unsupported(metaclass=Meta):
        pass

    try:
        engine.assert_fact("item", Unsupported())
    except TypeError as exc:
        assert "cannot convert Unsupported" in str(exc)
    else:
        raise AssertionError("unsupported value was accepted")
    assert len(results) == 3
    assert all("reentrant" in message for message in results)
    assert engine.fact_count == 0
    engine.assert_fact("still-live", 7)
    assert engine.fact_count == 1
    engine.close()


def main():
    scenario = sys.argv[1]
    if scenario == "conversion_reentry":
        _conversion_reentry()
    elif scenario == "foreign_halt":
        _foreign_halt()
    elif scenario == "foreign_close":
        _foreign_close()
    elif scenario == "waiting_read_during_run":
        _waiting_read_during_run()
    else:
        if not hasattr(os, "mkfifo"):
            raise RuntimeError("FIFO scenarios require os.mkfifo")
        with tempfile.TemporaryDirectory() as raw_directory:
            directory = Path(raw_directory)
            if scenario == "load_fifo":
                _load_fifo(directory)
            elif scenario == "waiting_read_load_fifo":
                _contended_load_fifo(directory, close=False)
            elif scenario == "concurrent_close_load_fifo":
                _contended_load_fifo(directory, close=True)
            elif scenario == "close_save_fifo":
                _close_during_save_fifo(directory)
            elif scenario == "save_fifo":
                _save_fifo(directory)
            elif scenario == "from_snapshot_fifo":
                _from_snapshot_fifo(directory)
            else:
                raise AssertionError(f"unknown scenario: {scenario}")
    print(f"ok:{scenario}")


if __name__ == "__main__":
    main()
