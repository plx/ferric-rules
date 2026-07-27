"""Shared fixtures for ferric Python binding tests."""

import pytest
import ferric


@pytest.fixture
def engine():
    """A fresh default engine."""
    return ferric.Engine()


@pytest.fixture
def engine_with_rule():
    """An engine with a simple rule loaded and reset."""
    return ferric.Engine.from_source(
        '(defrule greet (greeting ?x) => (assert (greeted ?x)))'
    )


@pytest.fixture
def engine_with_deffacts():
    """An engine with deffacts loaded and reset."""
    return ferric.Engine.from_source(
        '(deffacts startup (color red) (color blue))'
    )
