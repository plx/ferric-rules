"""Tests for cross-thread access raising FerricRuntimeError."""

import gc
import queue
import subprocess
import sys
import threading
from pathlib import Path

import pytest
import ferric


def _run_in_thread(fn):
    """Run `fn` in a new thread and propagate any exception."""
    result = {}

    def target():
        try:
            fn()
        except Exception as exc:
            result["exc"] = exc

    t = threading.Thread(target=target)
    t.start()
    t.join()
    if "exc" in result:
        raise result["exc"]


class TestCrossThreadProperty:
    """Cross-thread reads of properties must raise FerricRuntimeError."""

    def test_fact_count(self, engine):
        with pytest.raises(ferric.FerricRuntimeError, match="wrong thread"):
            _run_in_thread(lambda: engine.fact_count)

    def test_is_halted(self, engine):
        with pytest.raises(ferric.FerricRuntimeError, match="wrong thread"):
            _run_in_thread(lambda: engine.is_halted)

    def test_agenda_size(self, engine):
        with pytest.raises(ferric.FerricRuntimeError, match="wrong thread"):
            _run_in_thread(lambda: engine.agenda_size)

    def test_current_module(self, engine):
        with pytest.raises(ferric.FerricRuntimeError, match="wrong thread"):
            _run_in_thread(lambda: engine.current_module)


class TestCrossThreadMethod:
    """Cross-thread method calls must raise FerricRuntimeError."""

    def test_run(self, engine):
        with pytest.raises(ferric.FerricRuntimeError, match="wrong thread"):
            _run_in_thread(lambda: engine.run())

    def test_reset(self, engine):
        with pytest.raises(ferric.FerricRuntimeError, match="wrong thread"):
            _run_in_thread(lambda: engine.reset())

    def test_load(self, engine):
        with pytest.raises(ferric.FerricRuntimeError, match="wrong thread"):
            _run_in_thread(lambda: engine.load("(assert (x))"))

    def test_assert_fact(self, engine):
        with pytest.raises(ferric.FerricRuntimeError, match="wrong thread"):
            _run_in_thread(lambda: engine.assert_fact("color", "red"))

    def test_facts(self, engine):
        with pytest.raises(ferric.FerricRuntimeError, match="wrong thread"):
            _run_in_thread(lambda: engine.facts())


class TestCrossThreadDrop:
    """Dropping engine on wrong thread must not panic."""

    def test_drop_on_foreign_thread_no_panic(self, capsys):
        """Engine dropped on foreign thread: no panic, no stderr output."""
        engine = ferric.Engine()
        engine.assert_fact("color", "red")

        def drop_it(eng):
            del eng

        t = threading.Thread(target=drop_it, args=(engine,))
        del engine
        t.start()
        t.join()
        # No panic/segfault, and no stderr noise
        captured = capsys.readouterr()
        assert "leaked" not in captured.err
        assert "ferric" not in captured.err


class TestCrossThreadIsNotPanic:
    """Ensure the exception is FerricRuntimeError, NOT PanicException."""

    def test_not_panic_exception(self, engine):
        exc = None

        def target():
            nonlocal exc
            try:
                engine.run()
            except Exception as e:
                exc = e

        t = threading.Thread(target=target)
        t.start()
        t.join()

        assert exc is not None
        assert isinstance(exc, ferric.FerricRuntimeError)
        assert not isinstance(exc, BaseException) or isinstance(exc, Exception)
        assert "wrong thread" in str(exc)


class TestClose:
    """Tests for explicit engine close/lifecycle."""

    def test_close_releases_engine(self):
        engine = ferric.Engine()
        engine.assert_fact("x", 1)
        engine.close()
        with pytest.raises(ferric.FerricRuntimeError, match="closed"):
            engine.fact_count

    def test_close_idempotent(self):
        engine = ferric.Engine()
        assert engine.close() is None
        assert engine.close() is None

    def test_context_manager_closes(self):
        with ferric.Engine() as engine:
            engine.assert_fact("x", 1)
        with pytest.raises(ferric.FerricRuntimeError, match="closed"):
            engine.fact_count

    @pytest.mark.parametrize("closed", [False, True])
    def test_context_enter_preserves_wrong_thread_precedence(self, closed):
        engine = ferric.Engine()
        if closed:
            engine.close()

        with pytest.raises(ferric.FerricRuntimeError, match="wrong thread"):
            _run_in_thread(lambda: engine.__enter__())

    def test_context_enter_rejects_closed_on_creator_thread(self):
        engine = ferric.Engine()
        engine.close()

        with pytest.raises(ferric.FerricRuntimeError, match="closed"):
            engine.__enter__()

    def test_context_exit_closes_from_wrong_thread(self):
        engine = ferric.Engine()
        results = []

        _run_in_thread(lambda: results.append(engine.__exit__(None, None, None)))

        assert results == [False]
        with pytest.raises(ferric.FerricRuntimeError, match="closed"):
            engine.fact_count

    def test_close_from_wrong_thread(self):
        engine = ferric.Engine()
        results = []

        _run_in_thread(lambda: results.append(engine.close()))

        assert results == [None]
        with pytest.raises(ferric.FerricRuntimeError, match="closed"):
            engine.fact_count

    def test_wrong_thread_precedes_closed_for_non_close_operations(self):
        engine = ferric.Engine()
        engine.close()

        with pytest.raises(ferric.FerricRuntimeError, match="wrong thread"):
            _run_in_thread(lambda: engine.fact_count)


_has_testing = hasattr(ferric, "engine_instance_count")


@pytest.mark.skipif(not _has_testing, reason="testing feature not enabled")
class TestInstanceCount:
    """FR-PY-001 lifecycle tests using test-only instance instrumentation."""

    def test_create_and_drop(self):
        gc.collect()  # flush pending deallocations from earlier tests
        baseline = ferric.engine_instance_count()
        engine = ferric.Engine()
        assert ferric.engine_instance_count() == baseline + 1
        del engine
        gc.collect()
        assert ferric.engine_instance_count() == baseline

    def test_close_decrements(self):
        baseline = ferric.engine_instance_count()
        engine = ferric.Engine()
        assert ferric.engine_instance_count() == baseline + 1
        engine.close()
        assert ferric.engine_instance_count() == baseline

    def test_foreign_thread_close_is_synchronous_and_exactly_once(self):
        gc.collect()
        baseline = ferric.engine_instance_count()
        engines = [ferric.Engine()]
        fact_id = engines[0].assert_fact("owner", "foreign-close")
        assert engines[0].get_fact(fact_id).relation == "owner"
        results = []

        def close_twice():
            results.append(engines[0].close())
            assert ferric.engine_instance_count() == baseline
            results.append(engines[0].close())

        _run_in_thread(close_twice)

        assert results == [None, None]
        assert ferric.engine_instance_count() == baseline
        assert engines[0].close() is None
        engines.clear()
        gc.collect()
        assert ferric.engine_instance_count() == baseline

    def test_foreign_context_exit_decrements_synchronously(self):
        gc.collect()
        baseline = ferric.engine_instance_count()
        engines = [ferric.Engine()]
        results = []

        _run_in_thread(lambda: results.append(engines[0].__exit__(None, None, None)))

        assert results == [False]
        assert ferric.engine_instance_count() == baseline
        engines.clear()
        gc.collect()
        assert ferric.engine_instance_count() == baseline

    def test_concurrent_foreign_close_callers_destroy_exactly_once(self):
        gc.collect()
        baseline = ferric.engine_instance_count()
        engines = [ferric.Engine()]
        assert ferric.engine_instance_count() == baseline + 1
        start = threading.Barrier(3)
        results = []
        errors = []

        def close_after_barrier():
            start.wait()
            try:
                results.append(engines[0].close())
            except Exception as exc:
                errors.append(exc)

        workers = [threading.Thread(target=close_after_barrier) for _ in range(2)]
        for worker in workers:
            worker.start()
        start.wait()
        for worker in workers:
            worker.join()

        assert errors == []
        assert results == [None, None]
        assert ferric.engine_instance_count() == baseline
        assert engines[0].close() is None
        engines.clear()
        gc.collect()
        assert ferric.engine_instance_count() == baseline

    def test_foreign_thread_final_reference_decrements(self):
        """The final Python reference may be released off the creator thread."""
        gc.collect()
        baseline = ferric.engine_instance_count()
        engine = ferric.Engine()
        assert ferric.engine_instance_count() == baseline + 1

        def drop_final_reference(owned_engine):
            del owned_engine
            gc.collect()

        worker = threading.Thread(target=drop_final_reference, args=(engine,))
        del engine
        worker.start()
        worker.join()
        gc.collect()

        assert ferric.engine_instance_count() == baseline

    @pytest.mark.parametrize(
        "constructor",
        ["new", "from_source", "from_snapshot", "from_snapshot_file"],
    )
    def test_all_constructor_paths_support_foreign_final_drop(
        self, constructor, tmp_path
    ):
        """Every constructor owns the same exact foreign-drop lifecycle."""
        gc.collect()
        baseline = ferric.engine_instance_count()

        if constructor == "new":
            engine = ferric.Engine()
        elif constructor == "from_source":
            engine = ferric.Engine.from_source("(deffacts startup (origin source))")
        else:
            source = ferric.Engine()
            source.assert_fact("origin", "snapshot")
            snapshot = source.serialize()
            source.close()
            assert ferric.engine_instance_count() == baseline
            if constructor == "from_snapshot":
                engine = ferric.Engine.from_snapshot(snapshot)
            else:
                snapshot_path = tmp_path / "engine.bin"
                snapshot_path.write_bytes(snapshot)
                engine = ferric.Engine.from_snapshot_file(snapshot_path)

        fact_id = engine.assert_fact("constructor", constructor)
        fact = engine.get_fact(fact_id)
        assert fact is not None
        assert fact.relation == "constructor"
        assert fact.fields[0] == constructor
        assert fact.engine_id > 0
        del fact
        assert ferric.engine_instance_count() == baseline + 1

        def drop_final_reference(value):
            del value
            gc.collect()

        worker = threading.Thread(target=drop_final_reference, args=(engine,))
        del engine
        worker.start()
        worker.join()
        gc.collect()

        assert ferric.engine_instance_count() == baseline

    @pytest.mark.parametrize("cleanup", ["close", "drop"])
    def test_creator_may_exit_before_retained_handle_cleanup(self, cleanup):
        """Ownership follows a live handle after its creator thread exits."""
        gc.collect()
        baseline = ferric.engine_instance_count()
        transferred = queue.Queue()

        def create_and_transfer():
            engine = ferric.Engine()
            fact_id = engine.assert_fact("creator", cleanup)
            fact = engine.get_fact(fact_id)
            assert fact is not None
            transferred.put((engine, fact.engine_id))

        creator = threading.Thread(target=create_and_transfer)
        creator.start()
        creator.join()
        engine, engine_identity = transferred.get_nowait()

        assert engine_identity > 0
        assert ferric.engine_instance_count() == baseline + 1
        with pytest.raises(ferric.FerricRuntimeError, match="wrong thread"):
            engine.fact_count
        if cleanup == "close":
            assert engine.close() is None
        del engine
        gc.collect()

        assert ferric.engine_instance_count() == baseline

    def test_repeated_creator_thread_drops_are_bounded(self):
        gc.collect()
        baseline = ferric.engine_instance_count()
        engine_identities = set()

        for iteration in range(128):
            engine = ferric.Engine()
            assert ferric.engine_instance_count() == baseline + 1
            fact_id = engine.assert_fact("iteration", iteration)
            fact = engine.get_fact(fact_id)
            assert fact is not None
            assert fact.fields[0] == iteration
            engine_identities.add(fact.engine_id)
            del fact
            del engine
            gc.collect()
            assert ferric.engine_instance_count() == baseline

        assert len(engine_identities) == 128

    def test_repeated_drops_on_one_foreign_thread_are_bounded(self):
        """A long-lived foreign worker cannot accumulate abandoned engines."""
        gc.collect()
        baseline = ferric.engine_instance_count()
        work = queue.Queue()
        completed = queue.Queue()
        stop = object()

        def drop_work():
            while True:
                value = work.get()
                if value is stop:
                    return
                del value
                gc.collect()
                completed.put(ferric.engine_instance_count())

        worker = threading.Thread(target=drop_work)
        worker.start()
        observed_counts = []
        engine_identities = set()
        try:
            for iteration in range(128):
                engine = ferric.Engine()
                assert ferric.engine_instance_count() <= baseline + 1
                fact_id = engine.assert_fact("iteration", iteration)
                fact = engine.get_fact(fact_id)
                assert fact is not None
                assert fact.fields[0] == iteration
                engine_identities.add(fact.engine_id)
                del fact
                work.put(engine)
                del engine
                observed_counts.append(completed.get())
        finally:
            work.put(stop)
            worker.join()

        assert observed_counts == [baseline] * 128
        assert len(engine_identities) == 128
        assert ferric.engine_instance_count() == baseline

    def test_interpreter_teardown_reclaims_foreign_thread_drop(self):
        """Foreign-thread finalization remains observable during shutdown."""
        fixture = Path(__file__).with_name("fixtures") / "foreign_drop_teardown.py"
        result = subprocess.run(
            [sys.executable, str(fixture)],
            check=False,
            capture_output=True,
            text=True,
            timeout=15,
        )

        assert result.returncode == 0, result.stdout + result.stderr
        assert result.stdout == "baseline=0 remaining=0\n"
        assert result.stderr == ""

    def test_creator_thread_final_reference_decrements(self):
        """A creator-thread final reference destroys the engine synchronously."""
        baseline = ferric.engine_instance_count()

        def create_engine_on_thread():
            _eng = ferric.Engine()
            assert ferric.engine_instance_count() == baseline + 1

        t = threading.Thread(target=create_engine_on_thread)
        t.start()
        t.join()
        gc.collect()
        assert ferric.engine_instance_count() == baseline
