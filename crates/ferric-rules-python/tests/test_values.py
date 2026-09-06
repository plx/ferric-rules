"""Tests for value conversion round-trips."""

import ferric
import pytest


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
    def test_plain_string_becomes_clips_string(self, engine):
        fid = engine.assert_fact("color", "red")
        fact = engine.get_fact(fid)
        assert fact.fields[0] == ferric.String("red")
        assert isinstance(fact.fields[0], ferric.String)
        assert str(fact.fields[0]) == "red"

    def test_nested_plain_strings_and_explicit_symbols(self, engine):
        fid = engine.assert_fact("data", ["red", (ferric.Symbol("red"), "blue")])
        assert engine.get_fact(fid).fields == [
            [ferric.String("red"), [ferric.Symbol("red"), ferric.String("blue")]]
        ]


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
        assert fact.fields[0] == ferric.Symbol("hello")


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
        assert fact.fields[0] == ferric.String("hello")


class TestSymbolStringDistinction:
    def test_symbol_and_string_distinct_types(self):
        sym = ferric.Symbol("x")
        s = ferric.String("x")
        assert type(sym) != type(s)

    def test_rule_matches_symbol_not_string(self):
        """A rule matching symbol 'Alice' fires for Symbol but not String."""
        engine = ferric.Engine()
        engine.load("(defrule match-symbol (name Alice) => (assert (matched symbol)))")
        engine.reset()
        # Assert with explicit Symbol - should match
        engine.assert_fact("name", ferric.Symbol("Alice"))
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
        # Assert with explicit Symbol - should NOT match string pattern
        engine.assert_fact("name", ferric.Symbol("Alice"))
        result = engine.run()
        assert result.rules_fired == 0

    def test_string_in_template(self, engine):
        """ClipsString works in template assertions."""
        engine.load("(deftemplate person (slot name))")
        engine.reset()
        fid = engine.assert_template("person", name=ferric.String("Alice"))
        fact = engine.get_fact(fid)
        assert isinstance(fact.slots["name"], ferric.String)
        assert fact.slots["name"] == ferric.String("Alice")


class TestBoolConversion:
    def test_true_becomes_symbol(self, engine):
        fid = engine.assert_fact("flag", True)
        fact = engine.get_fact(fid)
        assert fact.fields[0] == ferric.Symbol("TRUE")

    def test_false_becomes_symbol(self, engine):
        fid = engine.assert_fact("flag", False)
        fact = engine.get_fact(fid)
        assert fact.fields[0] == ferric.Symbol("FALSE")


class TestNoneConversion:
    @pytest.mark.parametrize("value", [None, [None], [[None]]])
    def test_none_is_rejected_before_fact_installation(self, engine, value):
        with pytest.raises(ValueError, match="None.*cannot be stored"):
            engine.assert_fact("empty", value)
        assert engine.fact_count == 0

    @pytest.mark.parametrize("value", [None, [None], [[None]]])
    def test_none_template_slot_is_rejected(self, engine, value):
        engine.load("(deftemplate empty (multislot items))")
        with pytest.raises(ValueError, match="None.*cannot be stored"):
            engine.assert_template("empty", items=value)
        assert engine.fact_count == 0

    def test_nil_is_an_ordinary_symbol(self, engine):
        fid = engine.assert_fact("empty", ferric.Symbol("nil"))
        assert engine.get_fact(fid).fields == [ferric.Symbol("nil")]


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

    def test_multifield_preserves_symbol_and_string_types(self, engine):
        """Symbol/String wrappers nested inside a multifield keep their distinct
        CLIPS types through the round-trip rather than collapsing to str."""
        fid = engine.assert_fact("data", [ferric.Symbol("sym"), ferric.String("str")])
        field = engine.get_fact(fid).fields[0]
        assert isinstance(field[0], ferric.Symbol)
        assert isinstance(field[1], ferric.String)
        assert field[0] == ferric.Symbol("sym")
        assert field[1] == ferric.String("str")


class TestHashContract:
    """Typed wrappers and host strings form coherent, distinct key classes."""

    @pytest.mark.parametrize("value_type", [ferric.Symbol, ferric.String])
    def test_equal_values_have_equal_hashes(self, value_type):
        assert value_type("hello") == value_type("hello")
        assert hash(value_type("hello")) == hash(value_type("hello"))
        assert value_type("hello") != "hello"
        assert "hello" != value_type("hello")

    def test_mixed_sets_and_dicts_are_order_independent(self):
        from itertools import permutations

        values = [ferric.Symbol("key"), ferric.String("key"), "key"]
        for ordered in permutations(values):
            assert len(set(ordered)) == 3
            mapping = {value: type(value) for value in ordered}
            assert len(mapping) == 3
            assert mapping[ferric.Symbol("key")] is ferric.Symbol
            assert mapping[ferric.String("key")] is ferric.String
            assert mapping["key"] is str
        for left in values:
            for middle in values:
                for right in values:
                    if left == middle and middle == right:
                        assert left == right

    @pytest.mark.parametrize("value_type", [ferric.Symbol, ferric.String])
    def test_hashed_payload_is_readonly(self, value_type):
        value = value_type("stable")
        with pytest.raises(AttributeError):
            value.value = "changed"
        assert {value: 1}[value_type("stable")] == 1
