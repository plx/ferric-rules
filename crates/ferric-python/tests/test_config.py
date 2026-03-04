"""Tests for configuration enums."""

import ferric


class TestStrategy:
    def test_depth_exists(self):
        assert ferric.Strategy.DEPTH is not None

    def test_breadth_exists(self):
        assert ferric.Strategy.BREADTH is not None

    def test_lex_exists(self):
        assert ferric.Strategy.LEX is not None

    def test_mea_exists(self):
        assert ferric.Strategy.MEA is not None

    def test_values_distinct(self):
        assert ferric.Strategy.DEPTH != ferric.Strategy.BREADTH
        assert ferric.Strategy.BREADTH != ferric.Strategy.LEX
        assert ferric.Strategy.LEX != ferric.Strategy.MEA

    def test_strategy_as_kwarg(self):
        engine = ferric.Engine(strategy=ferric.Strategy.BREADTH)
        assert engine.fact_count == 0


class TestEncoding:
    def test_ascii_exists(self):
        assert ferric.Encoding.ASCII is not None

    def test_utf8_exists(self):
        assert ferric.Encoding.UTF8 is not None

    def test_ascii_symbols_utf8_strings_exists(self):
        assert ferric.Encoding.ASCII_SYMBOLS_UTF8_STRINGS is not None

    def test_values_distinct(self):
        assert ferric.Encoding.ASCII != ferric.Encoding.UTF8
        assert ferric.Encoding.UTF8 != ferric.Encoding.ASCII_SYMBOLS_UTF8_STRINGS

    def test_encoding_as_kwarg(self):
        engine = ferric.Engine(encoding=ferric.Encoding.ASCII)
        assert engine.fact_count == 0
