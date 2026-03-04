"""Tests for value conversion round-trips."""

import pytest
import ferric


class TestIntRoundTrip:
    def test_int_value(self, engine):
        fid = engine.assert_fact("count", 42)
        fact = engine.get_fact(fid)
        assert fact.fields[0] == 42
        assert isinstance(fact.fields[0], int)

    def test_negative_int(self, engine):
        fid = engine.assert_fact("temp", -10)
        fact = engine.get_fact(fid)
        assert fact.fields[0] == -10

    def test_zero(self, engine):
        fid = engine.assert_fact("zero", 0)
        fact = engine.get_fact(fid)
        assert fact.fields[0] == 0


class TestFloatRoundTrip:
    def test_float_value(self, engine):
        fid = engine.assert_fact("temp", 98.6)
        fact = engine.get_fact(fid)
        assert fact.fields[0] == pytest.approx(98.6)
        assert isinstance(fact.fields[0], float)

    def test_negative_float(self, engine):
        fid = engine.assert_fact("temp", -273.15)
        fact = engine.get_fact(fid)
        assert fact.fields[0] == pytest.approx(-273.15)


class TestStringRoundTrip:
    def test_string_becomes_symbol(self, engine):
        fid = engine.assert_fact("color", "red")
        fact = engine.get_fact(fid)
        assert fact.fields[0] == "red"
        assert isinstance(fact.fields[0], str)


class TestBoolConversion:
    def test_true_becomes_symbol(self, engine):
        fid = engine.assert_fact("flag", True)
        fact = engine.get_fact(fid)
        assert fact.fields[0] == "TRUE"

    def test_false_becomes_symbol(self, engine):
        fid = engine.assert_fact("flag", False)
        fact = engine.get_fact(fid)
        assert fact.fields[0] == "FALSE"


class TestNoneConversion:
    def test_none_roundtrip(self, engine):
        fid = engine.assert_fact("empty", None)
        fact = engine.get_fact(fid)
        assert fact.fields[0] is None


class TestListConversion:
    def test_list_to_multifield(self, engine):
        fid = engine.assert_fact("data", [1, 2, 3])
        fact = engine.get_fact(fid)
        field = fact.fields[0]
        assert isinstance(field, list)
        assert field == [1, 2, 3]

    def test_nested_list(self, engine):
        fid = engine.assert_fact("nested", [1, [2, 3]])
        fact = engine.get_fact(fid)
        assert fact.fields[0] == [1, [2, 3]]

    def test_tuple_to_multifield(self, engine):
        fid = engine.assert_fact("data", (1, 2))
        fact = engine.get_fact(fid)
        assert isinstance(fact.fields[0], list)
        assert fact.fields[0] == [1, 2]
