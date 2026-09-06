"""Shared fixtures for ferric Python binding tests."""

from tempfile import TemporaryDirectory

import pytest
import ferric


@pytest.hookimpl(tryfirst=True)
def pytest_configure(config):
    """Avoid pytest 8's predictable shared temp base on Python 3.9.

    GHSA-6w46-j5rx-g56g is fixed in pytest 9, which requires Python >=3.10.
    Keep a private parent alive for the session, including pytest's cleanup.
    An explicit --basetemp remains the caller's choice and responsibility.
    """
    if config.option.basetemp is None:
        parent = TemporaryDirectory(prefix="ferric-pytest-")
        config.option.basetemp = f"{parent.name}/tests"
        config.add_cleanup(parent.cleanup)


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
