"""Tests for cross-thread access raising FerricRuntimeError."""

import gc
import threading

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
        engine.close()
        engine.close()  # should not raise

    def test_context_manager_closes(self):
        with ferric.Engine() as engine:
            engine.assert_fact("x", 1)
        with pytest.raises(ferric.FerricRuntimeError, match="closed"):
            engine.fact_count

    def test_close_from_wrong_thread(self):
        engine = ferric.Engine()
        with pytest.raises(ferric.FerricRuntimeError, match="wrong thread"):
            _run_in_thread(lambda: engine.close())


_has_testing = hasattr(ferric, "engine_instance_count")


@pytest.mark.skipif(not _has_testing, reason="testing feature not enabled")
class TestInstanceCount:
    """Tests for engine instance counting instrumentation."""

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

    def test_thread_exit_cleans_up(self):
        """Engine created on a worker thread is cleaned up when thread exits."""
        baseline = ferric.engine_instance_count()

        def create_engine_on_thread():
            _eng = ferric.Engine()
            # engine lives in this thread's TLS; thread exit destroys TLS

        t = threading.Thread(target=create_engine_on_thread)
        t.start()
        t.join()
        gc.collect()
        assert ferric.engine_instance_count() == baseline
