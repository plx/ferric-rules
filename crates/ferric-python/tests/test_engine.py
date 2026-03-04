"""Tests for Engine lifecycle: create, from_source, context manager, reset, clear."""

import ferric


class TestEngineCreation:
    def test_default_engine(self):
        engine = ferric.Engine()
        assert engine.fact_count == 0
        assert not engine.is_halted

    def test_engine_with_strategy(self):
        engine = ferric.Engine(strategy=ferric.Strategy.LEX)
        assert engine.fact_count == 0

    def test_engine_with_encoding(self):
        engine = ferric.Engine(encoding=ferric.Encoding.ASCII)
        assert engine.fact_count == 0

    def test_engine_with_both(self):
        engine = ferric.Engine(
            strategy=ferric.Strategy.BREADTH,
            encoding=ferric.Encoding.UTF8,
        )
        assert engine.fact_count == 0


class TestFromSource:
    def test_from_source_happy(self):
        engine = ferric.Engine.from_source(
            '(defrule r1 (fact a) => (assert (result b)))'
        )
        assert len(engine.rules()) == 1

    def test_from_source_with_strategy(self):
        engine = ferric.Engine.from_source(
            '(defrule r1 (fact a) => (assert (result b)))',
            strategy=ferric.Strategy.LEX,
        )
        assert len(engine.rules()) == 1

    def test_from_source_parse_error(self):
        import pytest
        with pytest.raises(ferric.FerricParseError):
            ferric.Engine.from_source('(defrule incomplete')


class TestContextManager:
    def test_context_manager_clears_on_exit(self):
        with ferric.Engine.from_source(
            '(deffacts startup (color red))'
        ) as engine:
            assert engine.fact_count >= 1
        # After exit, engine is cleared
        assert engine.fact_count == 0
        assert len(engine.rules()) == 0

    def test_context_manager_clears_on_exception(self):
        try:
            with ferric.Engine.from_source(
                '(deffacts startup (color red))'
            ) as engine:
                raise ValueError("test error")
        except ValueError:
            pass
        assert engine.fact_count == 0


class TestResetClear:
    def test_reset_clears_facts(self, engine_with_deffacts):
        engine = engine_with_deffacts
        initial_count = engine.fact_count
        assert initial_count == 2
        engine.assert_string("(temp x)")
        assert engine.fact_count == 3
        engine.reset()
        assert engine.fact_count == 2  # deffacts re-asserted

    def test_clear_removes_everything(self, engine_with_deffacts):
        engine = engine_with_deffacts
        engine.clear()
        assert engine.fact_count == 0
        assert len(engine.rules()) == 0
