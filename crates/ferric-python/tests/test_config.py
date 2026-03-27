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


class TestModuleAttribution:
    """Exported classes must report __module__ == 'ferric'."""

    def test_engine_module(self):
        assert ferric.Engine.__module__ == "ferric"

    def test_fact_module(self):
        assert ferric.Fact.__module__ == "ferric"

    def test_fact_type_module(self):
        assert ferric.FactType.__module__ == "ferric"

    def test_strategy_module(self):
        assert ferric.Strategy.__module__ == "ferric"

    def test_encoding_module(self):
        assert ferric.Encoding.__module__ == "ferric"

    def test_run_result_module(self):
        assert ferric.RunResult.__module__ == "ferric"

    def test_halt_reason_module(self):
        assert ferric.HaltReason.__module__ == "ferric"

    def test_fired_rule_module(self):
        assert ferric.FiredRule.__module__ == "ferric"

    def test_symbol_module(self):
        assert ferric.Symbol.__module__ == "ferric"

    def test_string_module(self):
        assert ferric.String.__module__ == "ferric"
