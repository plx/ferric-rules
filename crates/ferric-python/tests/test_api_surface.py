"""Tests for PYB-006: additional runtime API surface."""

import pytest
import ferric


class TestFocusMutation:
    def test_set_focus(self):
        engine = ferric.Engine()
        engine.load('(defmodule A) (defmodule B)')
        engine.reset()
        engine.set_focus("A")
        assert engine.focus == "A"

    def test_set_focus_nonexistent_raises(self):
        engine = ferric.Engine()
        with pytest.raises(ferric.FerricModuleNotFoundError):
            engine.set_focus("NONEXISTENT")

    def test_push_focus(self):
        engine = ferric.Engine()
        engine.load('(defmodule A) (defmodule B)')
        engine.reset()
        engine.push_focus("A")
        engine.push_focus("B")
        assert engine.focus == "B"
        assert "A" in engine.focus_stack

    def test_push_focus_nonexistent_raises(self):
        engine = ferric.Engine()
        with pytest.raises(ferric.FerricModuleNotFoundError):
            engine.push_focus("NONEXISTENT")


class TestModuleEnumeration:
    def test_modules_default(self):
        engine = ferric.Engine()
        mods = engine.modules()
        assert "MAIN" in mods

    def test_modules_after_defmodule(self):
        engine = ferric.Engine()
        engine.load('(defmodule FOO)')
        mods = engine.modules()
        assert "MAIN" in mods
        assert "FOO" in mods


class TestClearDiagnostics:
    def test_clear_diagnostics(self):
        engine = ferric.Engine()
        # Diagnostics should be empty initially
        assert engine.diagnostics == []
        engine.clear_diagnostics()
        assert engine.diagnostics == []


class TestGetFactSlot:
    def test_get_slot_value(self):
        engine = ferric.Engine()
        engine.load('(deftemplate person (slot name) (slot age))')
        engine.reset()
        fid = engine.assert_template("person", name="Alice", age=30)
        val = engine.get_fact_slot(fid, "name")
        assert val == "Alice"

    def test_get_slot_integer(self):
        engine = ferric.Engine()
        engine.load('(deftemplate person (slot name) (slot age))')
        engine.reset()
        fid = engine.assert_template("person", name="Alice", age=30)
        val = engine.get_fact_slot(fid, "age")
        assert val == 30

    def test_get_slot_not_found(self):
        engine = ferric.Engine()
        engine.load('(deftemplate person (slot name))')
        engine.reset()
        fid = engine.assert_template("person", name="Alice")
        with pytest.raises(ferric.FerricSlotNotFoundError):
            engine.get_fact_slot(fid, "nonexistent")

    def test_get_slot_not_template_fact(self):
        engine = ferric.Engine()
        fid = engine.assert_fact("color", "red")
        with pytest.raises(ferric.FerricRuntimeError):
            engine.get_fact_slot(fid, "name")

    def test_get_slot_fact_not_found(self):
        engine = ferric.Engine()
        fid = engine.assert_fact("color", "red")
        engine.retract(fid)
        with pytest.raises(ferric.FerricFactNotFoundError):
            engine.get_fact_slot(fid, "name")
