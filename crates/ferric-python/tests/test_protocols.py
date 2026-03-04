"""Tests for Python protocols: repr, len, contains."""

import ferric


class TestRepr:
    def test_engine_repr(self):
        engine = ferric.Engine()
        r = repr(engine)
        assert "Engine(" in r
        assert "facts=" in r
        assert "rules=" in r
        assert "halted=" in r

    def test_fact_repr_ordered(self):
        engine = ferric.Engine()
        fid = engine.assert_fact("color", "red")
        fact = engine.get_fact(fid)
        r = repr(fact)
        assert "Fact(" in r
        assert "ORDERED" in r
        assert "color" in r

    def test_run_result_repr(self):
        engine = ferric.Engine()
        result = engine.run()
        r = repr(result)
        assert "RunResult(" in r

    def test_fired_rule_repr(self):
        engine = ferric.Engine.from_source("""
            (deffacts startup (go))
            (defrule r1 (go) => (assert (done)))
        """)
        fired = engine.step()
        assert fired is not None
        r = repr(fired)
        assert "FiredRule(" in r
        assert "r1" in r


class TestLen:
    def test_len_empty(self):
        engine = ferric.Engine()
        assert len(engine) == 0

    def test_len_with_facts(self):
        engine = ferric.Engine()
        engine.assert_fact("a", "b")
        engine.assert_fact("c", "d")
        assert len(engine) == 2


class TestContains:
    def test_contains_existing(self):
        engine = ferric.Engine()
        fid = engine.assert_fact("color", "red")
        assert fid in engine

    def test_contains_missing(self):
        engine = ferric.Engine()
        fid = engine.assert_fact("color", "red")
        engine.retract(fid)
        assert fid not in engine


class TestFactEquality:
    def test_same_id_equal(self):
        engine = ferric.Engine()
        fid = engine.assert_fact("color", "red")
        f1 = engine.get_fact(fid)
        f2 = engine.get_fact(fid)
        assert f1 == f2

    def test_different_id_not_equal(self):
        engine = ferric.Engine()
        fid1 = engine.assert_fact("color", "red")
        fid2 = engine.assert_fact("color", "blue")
        f1 = engine.get_fact(fid1)
        f2 = engine.get_fact(fid2)
        assert f1 != f2

    def test_fact_hashable(self):
        engine = ferric.Engine()
        fid = engine.assert_fact("color", "red")
        fact = engine.get_fact(fid)
        s = {fact}
        assert len(s) == 1
