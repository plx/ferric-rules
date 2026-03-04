"""Tests for execution: run, step, halt, run with limit."""

import ferric


class TestRun:
    def test_run_fires_all_rules(self):
        engine = ferric.Engine.from_source("""
            (deffacts startup (greeting world))
            (defrule greet
                (greeting ?x)
                =>
                (assert (greeted ?x)))
        """)
        result = engine.run()
        assert isinstance(result, ferric.RunResult)
        assert result.rules_fired == 1
        assert result.halt_reason == ferric.HaltReason.AGENDA_EMPTY

    def test_run_empty_agenda(self, engine):
        result = engine.run()
        assert result.rules_fired == 0
        assert result.halt_reason == ferric.HaltReason.AGENDA_EMPTY


class TestRunWithLimit:
    def test_run_limit_one(self):
        engine = ferric.Engine.from_source("""
            (deffacts startup (a 1) (a 2) (a 3))
            (defrule count-a
                (a ?x)
                =>
                (assert (counted ?x)))
        """)
        result = engine.run(limit=1)
        assert result.rules_fired == 1
        assert result.halt_reason == ferric.HaltReason.LIMIT_REACHED

    def test_run_limit_exceeds_available(self):
        engine = ferric.Engine.from_source("""
            (deffacts startup (a 1))
            (defrule count-a
                (a ?x)
                =>
                (assert (counted ?x)))
        """)
        result = engine.run(limit=100)
        assert result.rules_fired == 1
        assert result.halt_reason == ferric.HaltReason.AGENDA_EMPTY


class TestStep:
    def test_step_returns_fired_rule(self):
        engine = ferric.Engine.from_source("""
            (deffacts startup (greeting hello))
            (defrule greet
                (greeting ?x)
                =>
                (assert (greeted ?x)))
        """)
        fired = engine.step()
        assert fired is not None
        assert isinstance(fired, ferric.FiredRule)
        assert fired.rule_name == "greet"

    def test_step_returns_none_when_empty(self, engine):
        fired = engine.step()
        assert fired is None


class TestHalt:
    def test_halt_stops_execution(self):
        engine = ferric.Engine.from_source("""
            (deffacts startup (a 1))
            (defrule r1
                (a ?x)
                =>
                (halt)
                (assert (b ?x)))
        """)
        result = engine.run()
        assert result.halt_reason == ferric.HaltReason.HALT_REQUESTED

    def test_is_halted_property(self, engine):
        assert not engine.is_halted
        engine.halt()
        assert engine.is_halted


class TestReset:
    def test_reset_clears_halt(self):
        engine = ferric.Engine()
        engine.halt()
        assert engine.is_halted
        engine.reset()
        assert not engine.is_halted

    def test_reset_reasserts_deffacts(self):
        engine = ferric.Engine.from_source("""
            (deffacts startup (color red) (color blue))
        """)
        assert engine.fact_count == 2
        engine.assert_fact("temp", "value")
        assert engine.fact_count == 3
        engine.reset()
        assert engine.fact_count == 2
