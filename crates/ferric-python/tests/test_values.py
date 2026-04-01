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
        assert isinstance(fact.fields[0], ferric.Symbol)

    def test_symbol_equals_str(self, engine):
        """Symbol compares equal to plain str with same value."""
        fid = engine.assert_fact("color", "red")
        fact = engine.get_fact(fid)
        assert fact.fields[0] == "red"
        assert str(fact.fields[0]) == "red"


class TestSymbolType:
    def test_symbol_constructor(self):
        sym = ferric.Symbol("hello")
        assert sym.value == "hello"
        assert str(sym) == "hello"

    def test_symbol_repr(self):
        sym = ferric.Symbol("hello")
        assert repr(sym) == 'Symbol("hello")'

    def test_symbol_equality(self):
        a = ferric.Symbol("x")
        b = ferric.Symbol("x")
        assert a == b

    def test_symbol_hash(self):
        a = ferric.Symbol("x")
        b = ferric.Symbol("x")
        assert hash(a) == hash(b)

    def test_symbol_roundtrip(self, engine):
        fid = engine.assert_fact("data", ferric.Symbol("hello"))
        fact = engine.get_fact(fid)
        assert isinstance(fact.fields[0], ferric.Symbol)
        assert fact.fields[0] == "hello"


class TestClipsStringType:
    def test_string_constructor(self):
        s = ferric.String("hello")
        assert s.value == "hello"
        assert str(s) == "hello"

    def test_string_repr(self):
        s = ferric.String("hello")
        assert repr(s) == 'String("hello")'

    def test_string_equality(self):
        a = ferric.String("x")
        b = ferric.String("x")
        assert a == b

    def test_string_hash(self):
        a = ferric.String("x")
        b = ferric.String("x")
        assert hash(a) == hash(b)

    def test_string_roundtrip(self, engine):
        fid = engine.assert_fact("data", ferric.String("hello"))
        fact = engine.get_fact(fid)
        assert isinstance(fact.fields[0], ferric.String)
        assert fact.fields[0] == "hello"


class TestSymbolStringDistinction:
    def test_symbol_and_string_distinct_types(self):
        sym = ferric.Symbol("x")
        s = ferric.String("x")
        assert type(sym) != type(s)

    def test_rule_matches_symbol_not_string(self):
        """A rule matching symbol 'Alice' fires for Symbol but not String."""
        engine = ferric.Engine()
        engine.load(
            '(defrule match-symbol (name Alice) => (assert (matched symbol)))'
        )
        engine.reset()
        # Assert with plain str (becomes Symbol) - should match
        engine.assert_fact("name", "Alice")
        result = engine.run()
        assert result.rules_fired == 1

    def test_rule_matches_string_literal(self):
        """A rule matching string literal fires for String values."""
        engine = ferric.Engine()
        engine.load(
            '(defrule match-string (name "Alice") => (assert (matched string)))'
        )
        engine.reset()
        # Assert with String wrapper - should match string pattern
        engine.assert_fact("name", ferric.String("Alice"))
        result = engine.run()
        assert result.rules_fired == 1

    def test_symbol_does_not_match_string_pattern(self):
        """A Symbol value should not match a string literal pattern."""
        engine = ferric.Engine()
        engine.load(
            '(defrule match-string (name "Alice") => (assert (matched string)))'
        )
        engine.reset()
        # Assert with plain str (becomes Symbol) - should NOT match string pattern
        engine.assert_fact("name", "Alice")
        result = engine.run()
        assert result.rules_fired == 0

    def test_string_in_template(self, engine):
        """ClipsString works in template assertions."""
        engine.load('(deftemplate person (slot name))')
        engine.reset()
        fid = engine.assert_template("person", name=ferric.String("Alice"))
        fact = engine.get_fact(fid)
        assert isinstance(fact.slots["name"], ferric.String)
        assert fact.slots["name"] == "Alice"


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


class TestHashContract:
    """hash(a) == hash(b) whenever a == b (Python invariant)."""

    def test_symbol_hash_equals_str_hash(self):
        sym = ferric.Symbol("hello")
        assert sym == "hello"
        assert hash(sym) == hash("hello")

    def test_string_hash_equals_str_hash(self):
        s = ferric.String("hello")
        assert s == "hello"
        assert hash(s) == hash("hello")

    def test_symbol_in_dict(self):
        d = {}
        d[ferric.Symbol("key")] = "value"
        assert d["key"] == "value"

    def test_string_in_set(self):
        s = {ferric.String("a"), "a"}
        assert len(s) == 1

    def test_symbol_and_string_distinct_in_set(self):
        s = {ferric.Symbol("x"), ferric.String("x")}
        assert len(s) == 2
