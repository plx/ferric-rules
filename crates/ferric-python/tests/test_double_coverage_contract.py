"""Meta-tests that keep Python binding coverage complete and double-layered."""

from __future__ import annotations

import inspect

import ferric

# Import the registered suites explicitly so this contract also works when run
# by itself rather than only as part of full pytest collection.
import test_generative_api_coverage  # noqa: F401
import test_manual_api_coverage  # noqa: F401

from double_coverage import (
    ENGINE_PROTOCOLS,
    FACT_PROTOCOLS,
    FIRED_RULE_PROTOCOLS,
    PUBLIC_API,
    PUBLIC_CLASS_MEMBERS,
    PUBLIC_ENUM_MEMBERS,
    PUBLIC_TOP_LEVEL_NAMES,
    RUN_RESULT_PROTOCOLS,
    STRING_PROTOCOLS,
    SYMBOL_PROTOCOLS,
    generative_coverage_items,
    manual_coverage_items,
    manual_coverage_tests,
)


def test_public_top_level_exports_match_contract():
    exported = {name for name in dir(ferric) if not name.startswith("_")}
    assert exported == PUBLIC_TOP_LEVEL_NAMES


def test_public_class_members_match_contract():
    for class_name, expected_members in PUBLIC_CLASS_MEMBERS.items():
        cls = getattr(ferric, class_name)
        exported = {name for name in dir(cls) if not name.startswith("_")}
        assert exported == expected_members


def test_public_enum_members_match_contract():
    for enum_name, expected_members in PUBLIC_ENUM_MEMBERS.items():
        enum_type = getattr(ferric, enum_name)
        exported = {name for name in dir(enum_type) if not name.startswith("_")}
        assert exported == expected_members


def test_public_protocol_members_exist():
    protocol_items = (
        ENGINE_PROTOCOLS
        | FACT_PROTOCOLS
        | SYMBOL_PROTOCOLS
        | STRING_PROTOCOLS
        | RUN_RESULT_PROTOCOLS
        | FIRED_RULE_PROTOCOLS
    )
    for item in protocol_items - {"Engine.thread_affinity"}:
        class_name, member_name = item.split(".", maxsplit=1)
        assert hasattr(getattr(ferric, class_name), member_name)


def test_every_public_item_has_manual_coverage():
    missing = PUBLIC_API - manual_coverage_items()
    assert not missing, f"missing manual coverage for: {sorted(missing)}"


def test_every_public_item_has_generative_coverage():
    missing = PUBLIC_API - generative_coverage_items()
    assert not missing, f"missing generative coverage for: {sorted(missing)}"


def test_manual_coverage_tests_have_explanatory_comments():
    for test_fn in manual_coverage_tests():
        source = inspect.getsource(test_fn)
        comments = [
            line.strip()
            for line in source.splitlines()
            if line.lstrip().startswith("#")
        ]
        assert comments, f"{test_fn.__name__} lacks explanatory comments"
