"""Coverage contract helpers for the Python binding test suite."""

from __future__ import annotations

from collections.abc import Callable
from typing import TypeVar


TOP_LEVEL_EXPORTS = frozenset(
    {
        "ferric.Encoding",
        "ferric.Engine",
        "ferric.Fact",
        "ferric.FactType",
        "ferric.FerricCompileError",
        "ferric.FerricEncodingError",
        "ferric.FerricError",
        "ferric.FerricFactNotFoundError",
        "ferric.FerricModuleNotFoundError",
        "ferric.FerricParseError",
        "ferric.FerricRuntimeError",
        "ferric.FerricSlotNotFoundError",
        "ferric.FerricTemplateNotFoundError",
        "ferric.FiredRule",
        "ferric.Format",
        "ferric.HaltReason",
        "ferric.RunResult",
        "ferric.Strategy",
        "ferric.String",
        "ferric.Symbol",
        "ferric.ferric",
    }
)

# Names exported by the ferric module only when built with the `testing`
# feature (`crates/ferric-python/src/lib.rs` adds them under
# `#[cfg(feature = "testing")]`). Contract tests treat these as optional so the
# Python suite can run against either build configuration.
TESTING_ONLY_TOP_LEVEL = frozenset(
    {
        "engine_instance_count",
    }
)

ENGINE_MEMBERS = frozenset(
    {
        "Engine.agenda_size",
        "Engine.assert_fact",
        "Engine.assert_string",
        "Engine.assert_template",
        "Engine.clear",
        "Engine.clear_diagnostics",
        "Engine.clear_output",
        "Engine.close",
        "Engine.current_module",
        "Engine.diagnostics",
        "Engine.fact_count",
        "Engine.facts",
        "Engine.find_facts",
        "Engine.focus",
        "Engine.focus_stack",
        "Engine.from_snapshot",
        "Engine.from_snapshot_file",
        "Engine.from_source",
        "Engine.get_fact",
        "Engine.get_fact_slot",
        "Engine.get_global",
        "Engine.get_output",
        "Engine.halt",
        "Engine.is_halted",
        "Engine.load",
        "Engine.load_file",
        "Engine.modules",
        "Engine.push_focus",
        "Engine.push_input",
        "Engine.reset",
        "Engine.retract",
        "Engine.rules",
        "Engine.run",
        "Engine.save_snapshot",
        "Engine.serialize",
        "Engine.set_focus",
        "Engine.step",
        "Engine.templates",
    }
)

ENGINE_PROTOCOLS = frozenset(
    {
        "Engine.__contains__",
        "Engine.__enter__",
        "Engine.__exit__",
        "Engine.__init__",
        "Engine.__len__",
        "Engine.__repr__",
        "Engine.thread_affinity",
    }
)

FACT_MEMBERS = frozenset(
    {
        "Fact.engine_id",
        "Fact.fact_type",
        "Fact.fields",
        "Fact.id",
        "Fact.relation",
        "Fact.slots",
        "Fact.template_name",
    }
)

FACT_PROTOCOLS = frozenset(
    {
        "Fact.__eq__",
        "Fact.__hash__",
        "Fact.__repr__",
    }
)

SYMBOL_MEMBERS = frozenset(
    {
        "Symbol.value",
    }
)

SYMBOL_PROTOCOLS = frozenset(
    {
        "Symbol.__eq__",
        "Symbol.__hash__",
        "Symbol.__init__",
        "Symbol.__repr__",
        "Symbol.__str__",
    }
)

STRING_MEMBERS = frozenset(
    {
        "String.value",
    }
)

STRING_PROTOCOLS = frozenset(
    {
        "String.__eq__",
        "String.__hash__",
        "String.__init__",
        "String.__repr__",
        "String.__str__",
    }
)

RUN_RESULT_MEMBERS = frozenset(
    {
        "RunResult.halt_reason",
        "RunResult.rules_fired",
    }
)

RUN_RESULT_PROTOCOLS = frozenset(
    {
        "RunResult.__repr__",
    }
)

FIRED_RULE_MEMBERS = frozenset(
    {
        "FiredRule.rule_name",
    }
)

FIRED_RULE_PROTOCOLS = frozenset(
    {
        "FiredRule.__repr__",
    }
)

ENUM_VALUES = frozenset(
    {
        "Encoding.ASCII",
        "Encoding.ASCII_SYMBOLS_UTF8_STRINGS",
        "Encoding.UTF8",
        "FactType.ORDERED",
        "FactType.TEMPLATE",
        "Format.BINCODE",
        "Format.CBOR",
        "Format.JSON",
        "Format.MSGPACK",
        "Format.POSTCARD",
        "HaltReason.AGENDA_EMPTY",
        "HaltReason.HALT_REQUESTED",
        "HaltReason.LIMIT_REACHED",
        "Strategy.BREADTH",
        "Strategy.DEPTH",
        "Strategy.LEX",
        "Strategy.MEA",
    }
)

EXCEPTION_EXPORTS = frozenset(
    item
    for item in TOP_LEVEL_EXPORTS
    if item.startswith("ferric.Ferric") and item != "ferric.ferric"
)

PUBLIC_API = frozenset(
    TOP_LEVEL_EXPORTS
    | ENGINE_MEMBERS
    | ENGINE_PROTOCOLS
    | FACT_MEMBERS
    | FACT_PROTOCOLS
    | SYMBOL_MEMBERS
    | SYMBOL_PROTOCOLS
    | STRING_MEMBERS
    | STRING_PROTOCOLS
    | RUN_RESULT_MEMBERS
    | RUN_RESULT_PROTOCOLS
    | FIRED_RULE_MEMBERS
    | FIRED_RULE_PROTOCOLS
    | ENUM_VALUES
)

PUBLIC_TOP_LEVEL_NAMES = frozenset(
    item.removeprefix("ferric.") for item in TOP_LEVEL_EXPORTS
)

PUBLIC_CLASS_MEMBERS = {
    "Engine": frozenset(item.removeprefix("Engine.") for item in ENGINE_MEMBERS),
    "Fact": frozenset(item.removeprefix("Fact.") for item in FACT_MEMBERS),
    "Symbol": frozenset(item.removeprefix("Symbol.") for item in SYMBOL_MEMBERS),
    "String": frozenset(item.removeprefix("String.") for item in STRING_MEMBERS),
    "RunResult": frozenset(item.removeprefix("RunResult.") for item in RUN_RESULT_MEMBERS),
    "FiredRule": frozenset(item.removeprefix("FiredRule.") for item in FIRED_RULE_MEMBERS),
}

PUBLIC_ENUM_MEMBERS = {
    "Encoding": frozenset(
        item.removeprefix("Encoding.") for item in ENUM_VALUES if item.startswith("Encoding.")
    ),
    "FactType": frozenset(
        item.removeprefix("FactType.") for item in ENUM_VALUES if item.startswith("FactType.")
    ),
    "Format": frozenset(
        item.removeprefix("Format.") for item in ENUM_VALUES if item.startswith("Format.")
    ),
    "HaltReason": frozenset(
        item.removeprefix("HaltReason.")
        for item in ENUM_VALUES
        if item.startswith("HaltReason.")
    ),
    "Strategy": frozenset(
        item.removeprefix("Strategy.") for item in ENUM_VALUES if item.startswith("Strategy.")
    ),
}

_MANUAL_COVERAGE: set[str] = set()
_GENERATIVE_COVERAGE: set[str] = set()
_MANUAL_TESTS: list[Callable[..., object]] = []

T = TypeVar("T", bound=Callable[..., object])


def _validate_items(items: tuple[str, ...]) -> None:
    unknown = set(items) - PUBLIC_API
    if unknown:
        raise AssertionError(f"unknown coverage item(s): {sorted(unknown)}")


def covers_manual(*items: str) -> Callable[[T], T]:
    """Register explicit, hand-authored coverage for public API items."""

    _validate_items(items)

    def decorator(fn: T) -> T:
        _MANUAL_COVERAGE.update(items)
        _MANUAL_TESTS.append(fn)
        return fn

    return decorator


def covers_generative(*items: str) -> Callable[[T], T]:
    """Register generated/property-style coverage for public API items."""

    _validate_items(items)

    def decorator(fn: T) -> T:
        _GENERATIVE_COVERAGE.update(items)
        return fn

    return decorator


def manual_coverage_items() -> frozenset[str]:
    return frozenset(_MANUAL_COVERAGE)


def manual_coverage_tests() -> tuple[Callable[..., object], ...]:
    return tuple(_MANUAL_TESTS)


def generative_coverage_items() -> frozenset[str]:
    return frozenset(_GENERATIVE_COVERAGE)
