"""Tests for cross-thread access raising FerricRuntimeError."""

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
