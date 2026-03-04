"""Tests for the exception hierarchy."""

import pytest
import ferric


class TestExceptionHierarchy:
    def test_base_error_exists(self):
        assert ferric.FerricError is not None

    def test_parse_error_is_ferric_error(self):
        assert issubclass(ferric.FerricParseError, ferric.FerricError)

    def test_compile_error_is_ferric_error(self):
        assert issubclass(ferric.FerricCompileError, ferric.FerricError)

    def test_runtime_error_is_ferric_error(self):
        assert issubclass(ferric.FerricRuntimeError, ferric.FerricError)

    def test_fact_not_found_is_ferric_error(self):
        assert issubclass(ferric.FerricFactNotFoundError, ferric.FerricError)

    def test_module_not_found_is_ferric_error(self):
        assert issubclass(ferric.FerricModuleNotFoundError, ferric.FerricError)

    def test_encoding_error_is_ferric_error(self):
        assert issubclass(ferric.FerricEncodingError, ferric.FerricError)


class TestParseError:
    def test_invalid_source_raises_parse_error(self):
        engine = ferric.Engine()
        with pytest.raises(ferric.FerricParseError):
            engine.load("(defrule incomplete")

    def test_from_source_invalid(self):
        with pytest.raises(ferric.FerricParseError):
            ferric.Engine.from_source("(defrule bad")


class TestFactNotFoundError:
    def test_retract_missing(self):
        engine = ferric.Engine()
        fid = engine.assert_fact("x", "y")
        engine.retract(fid)
        with pytest.raises(ferric.FerricFactNotFoundError):
            engine.retract(fid)


class TestCatchBase:
    def test_catch_parse_as_base(self):
        with pytest.raises(ferric.FerricError):
            ferric.Engine.from_source("(defrule bad")

    def test_catch_fact_not_found_as_base(self):
        engine = ferric.Engine()
        fid = engine.assert_fact("x", "y")
        engine.retract(fid)
        with pytest.raises(ferric.FerricError):
            engine.retract(fid)
