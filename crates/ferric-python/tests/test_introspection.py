"""Tests for introspection: rules(), templates(), get_global()."""

import ferric


class TestRules:
    def test_no_rules(self, engine):
        assert engine.rules() == []

    def test_rules_after_load(self):
        engine = ferric.Engine.from_source("""
            (defrule r1 (a) => (assert (b)))
            (defrule r2 (declare (salience 10)) (c) => (assert (d)))
        """)
        rules = engine.rules()
        assert len(rules) == 2
        names = {r[0] for r in rules}
        assert "r1" in names
        assert "r2" in names
        # Check salience
        for name, salience in rules:
            if name == "r2":
                assert salience == 10


class TestTemplates:
    def test_no_templates(self, engine):
        assert engine.templates() == []

    def test_templates_after_load(self):
        engine = ferric.Engine.from_source("""
            (deftemplate person (slot name) (slot age))
        """)
        templates = engine.templates()
        assert "person" in templates


class TestGetGlobal:
    def test_global_not_found(self, engine):
        assert engine.get_global("nonexistent") is None

    def test_global_value(self):
        engine = ferric.Engine.from_source("""
            (defglobal ?*count* = 42)
        """)
        val = engine.get_global("count")
        assert val == 42

    def test_global_string(self):
        engine = ferric.Engine.from_source("""
            (defglobal ?*name* = hello)
        """)
        val = engine.get_global("name")
        assert val == "hello"
