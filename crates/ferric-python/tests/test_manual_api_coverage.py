"""Explicit double-coverage tests for the public Python binding API."""

from __future__ import annotations

import threading

import pytest
import ferric

from double_coverage import (
    ENUM_VALUES,
    EXCEPTION_EXPORTS,
    FACT_MEMBERS,
    FACT_PROTOCOLS,
    FIRED_RULE_MEMBERS,
    FIRED_RULE_PROTOCOLS,
    RUN_RESULT_MEMBERS,
    RUN_RESULT_PROTOCOLS,
    STRING_MEMBERS,
    STRING_PROTOCOLS,
    SYMBOL_MEMBERS,
    SYMBOL_PROTOCOLS,
    TOP_LEVEL_EXPORTS,
    covers_manual,
)


def _call_from_worker(fn):
    result = {}

    def target():
        try:
            fn()
        except Exception as exc:
            result["exc"] = exc

    thread = threading.Thread(target=target)
    thread.start()
    thread.join()
    if "exc" in result:
        raise result["exc"]


@covers_manual(*(TOP_LEVEL_EXPORTS | EXCEPTION_EXPORTS))
def test_manual_module_exports_and_exception_hierarchy():
    # The package-level API is the import contract embedders rely on, so this
    # checks the concrete names instead of only exercising them indirectly.
    for item in TOP_LEVEL_EXPORTS:
        name = item.removeprefix("ferric.")
        assert hasattr(ferric, name)

    # All Ferric-specific failures must remain catchable through the base class
    # so callers can choose either coarse or specific error handling.
    for item in EXCEPTION_EXPORTS - {"ferric.FerricError"}:
        exc_type = getattr(ferric, item.removeprefix("ferric."))
        assert issubclass(exc_type, ferric.FerricError)

    # The extension implementation module is visible; verify it re-exports the
    # same concrete classes as the package facade.
    assert ferric.ferric.Engine is ferric.Engine
    assert ferric.ferric.Symbol is ferric.Symbol


@covers_manual(*ENUM_VALUES)
def test_manual_enum_values_are_distinct_and_usable():
    # Enum members are passed straight into Rust config and result paths, so the
    # explicit case checks every advertised member exists and is unique.
    enum_groups = [
        [
            ferric.Strategy.DEPTH,
            ferric.Strategy.BREADTH,
            ferric.Strategy.LEX,
            ferric.Strategy.MEA,
        ],
        [
            ferric.Encoding.ASCII,
            ferric.Encoding.UTF8,
            ferric.Encoding.ASCII_SYMBOLS_UTF8_STRINGS,
        ],
        [ferric.FactType.ORDERED, ferric.FactType.TEMPLATE],
        [
            ferric.HaltReason.AGENDA_EMPTY,
            ferric.HaltReason.LIMIT_REACHED,
            ferric.HaltReason.HALT_REQUESTED,
        ],
        [
            ferric.Format.BINCODE,
            ferric.Format.JSON,
            ferric.Format.CBOR,
            ferric.Format.MSGPACK,
            ferric.Format.POSTCARD,
        ],
    ]
    for values in enum_groups:
        assert all(
            left != right
            for index, left in enumerate(values)
            for right in values[index + 1 :]
        )

    # Config enum values must be accepted by the constructor without changing
    # the baseline empty-engine state.
    engine = ferric.Engine(
        strategy=ferric.Strategy.MEA,
        encoding=ferric.Encoding.ASCII_SYMBOLS_UTF8_STRINGS,
    )
    assert engine.fact_count == 0


@covers_manual(
    "Engine.__init__",
    "Engine.__enter__",
    "Engine.__exit__",
    "Engine.__repr__",
    "Engine.__len__",
    "Engine.__contains__",
    "Engine.close",
    "Engine.from_source",
    "Engine.load",
    "Engine.load_file",
    "Engine.fact_count",
    "Engine.rules",
    "Engine.thread_affinity",
)
def test_manual_engine_lifecycle_loading_protocols_and_thread_affinity(tmp_path):
    # Constructor, load(), and load_file() are separate entry points into the
    # runtime loader, so this checks each with a small rule that is introspected.
    engine = ferric.Engine()
    engine.load("(defrule loaded (go) => (assert (done)))")
    assert len(engine.rules()) == 1

    path = tmp_path / "rule.clp"
    path.write_text("(defrule from-file (start) => (assert (end)))")
    file_engine = ferric.Engine()
    file_engine.load_file(path)
    assert len(file_engine.rules()) == 1

    source_engine = ferric.Engine.from_source("(deffacts startup (seed yes))")
    assert source_engine.fact_count == 1
    assert "Engine(" in repr(source_engine)

    # The context manager must close the Rust engine even when Python keeps the
    # handle object alive after the with-block.
    with ferric.Engine() as managed:
        fid = managed.assert_fact("marker", "inside")
        assert len(managed) == 1
        assert fid in managed
    with pytest.raises(ferric.FerricRuntimeError, match="closed"):
        managed.fact_count

    # Handles are intentionally thread-affine; crossing threads should produce
    # a Python exception instead of a panic or undefined behavior.
    with pytest.raises(ferric.FerricRuntimeError, match="wrong thread"):
        _call_from_worker(lambda: source_engine.rules())

    source_engine.close()
    source_engine.close()
    with pytest.raises(ferric.FerricRuntimeError, match="closed"):
        source_engine.fact_count


@covers_manual(
    *(FACT_MEMBERS | FACT_PROTOCOLS),
    "Engine.assert_fact",
    "Engine.assert_string",
    "Engine.get_fact",
    "Engine.facts",
    "Engine.find_facts",
    "Engine.retract",
)
def test_manual_ordered_fact_api_and_protocols():
    # Ordered facts are the smallest data path through the binding; this checks
    # both CLIPS-source assertion and structured assertion produce snapshots.
    engine = ferric.Engine()
    ids = engine.assert_string("(color red) (color blue)")
    structured_id = engine.assert_fact("shape", "circle", 3)
    assert len(ids) == 2
    assert structured_id not in ids

    fact = engine.get_fact(structured_id)
    assert fact is not None
    assert fact.id == structured_id
    assert fact.engine_id > 0
    assert fact.fact_type == ferric.FactType.ORDERED
    assert fact.relation == "shape"
    assert fact.template_name is None
    assert fact.fields == ["circle", 3]
    assert fact.slots is None

    # Snapshot equality and hashing use engine_id plus fact id; this catches
    # accidental comparison by field contents alone.
    same_fact = engine.get_fact(structured_id)
    assert fact == same_fact
    assert hash(fact) == hash(same_fact)
    assert "ORDERED" in repr(fact)

    assert len(engine.facts()) == 3
    assert [f.relation for f in engine.find_facts("color")] == ["color", "color"]
    engine.retract(structured_id)
    assert engine.get_fact(structured_id) is None
    with pytest.raises(ferric.FerricFactNotFoundError):
        engine.retract(structured_id)


@covers_manual(
    "Engine.assert_template",
    "Engine.get_fact_slot",
    "Engine.templates",
    "ferric.FerricTemplateNotFoundError",
    "ferric.FerricSlotNotFoundError",
)
def test_manual_template_fact_api_and_slot_errors():
    # Template facts have named slot conversion and defaulting behavior, so the
    # test inspects both the generic fields list and the slot dictionary.
    engine = ferric.Engine.from_source("""
        (deftemplate person
            (slot name)
            (slot age (type INTEGER) (default 0))
            (slot active (default TRUE)))
    """)
    fid = engine.assert_template("person", name="Ada", age=37)
    fact = engine.get_fact(fid)
    assert fact.fact_type == ferric.FactType.TEMPLATE
    assert fact.template_name == "person"
    assert fact.relation is None
    assert fact.fields[0] == "Ada"
    assert fact.slots["name"] == "Ada"
    assert fact.slots["age"] == 37
    assert "TEMPLATE" in repr(fact)
    assert engine.get_fact_slot(fid, "active") == "TRUE"
    assert engine.templates() == ["person"]

    # Specific Python exception classes are part of the binding contract for
    # callers that need to distinguish schema errors from missing facts.
    with pytest.raises(ferric.FerricTemplateNotFoundError):
        engine.assert_template("missing", name="Ada")
    with pytest.raises(ferric.FerricSlotNotFoundError):
        engine.assert_template("person", missing_slot="value")
    with pytest.raises(ferric.FerricSlotNotFoundError):
        engine.get_fact_slot(fid, "missing_slot")


@covers_manual(
    *(RUN_RESULT_MEMBERS | RUN_RESULT_PROTOCOLS | FIRED_RULE_MEMBERS | FIRED_RULE_PROTOCOLS),
    "Engine.agenda_size",
    "Engine.clear",
    "Engine.halt",
    "Engine.is_halted",
    "Engine.reset",
    "Engine.run",
    "Engine.step",
)
def test_manual_execution_halt_reset_clear_and_result_protocols():
    # step() returns the fired rule name while run() returns aggregate execution
    # state; both result wrappers need stable attributes and reprs.
    step_engine = ferric.Engine.from_source("""
        (deffacts startup (go))
        (defrule mark (go) => (assert (done)))
    """)
    assert step_engine.agenda_size == 1
    fired = step_engine.step()
    assert isinstance(fired, ferric.FiredRule)
    assert fired.rule_name == "mark"
    assert "mark" in repr(fired)

    limit_engine = ferric.Engine.from_source("""
        (deffacts startup (item 1) (item 2) (item 3))
        (defrule mark (item ?x) => (assert (seen ?x)))
    """)
    limited = limit_engine.run(limit=1)
    assert isinstance(limited, ferric.RunResult)
    assert limited.rules_fired == 1
    assert limited.halt_reason == ferric.HaltReason.LIMIT_REACHED
    assert "RunResult(" in repr(limited)

    # Halt, reset, and clear mutate global engine state; the assertions pin the
    # externally visible lifecycle after each operation.
    halt_engine = ferric.Engine.from_source("""
        (deffacts startup (go))
        (defrule stop (go) => (halt))
    """)
    halted = halt_engine.run()
    assert halted.halt_reason == ferric.HaltReason.HALT_REQUESTED
    assert halt_engine.is_halted
    halt_engine.reset()
    assert not halt_engine.is_halted
    halt_engine.clear()
    assert halt_engine.fact_count == 0
    assert halt_engine.rules() == []


@covers_manual(
    "Engine.current_module",
    "Engine.focus",
    "Engine.focus_stack",
    "Engine.get_global",
    "Engine.modules",
    "Engine.push_focus",
    "Engine.set_focus",
)
def test_manual_focus_modules_and_globals():
    # Module focus is process-like mutable state, so the manual case checks the
    # default MAIN module and both replacement and push focus operations.
    engine = ferric.Engine.from_source("""
        (defglobal ?*threshold* = 17)
        (defmodule A)
        (defmodule B)
    """)
    assert engine.current_module == "MAIN"
    assert set(engine.modules()) == {"MAIN", "A", "B"}
    assert engine.get_global("threshold") == 17
    assert engine.get_global("missing") is None

    engine.set_focus("A")
    assert engine.focus == "A"
    assert engine.focus_stack[-1] == "A"
    engine.push_focus("B")
    assert engine.focus == "B"
    assert engine.focus_stack[-2:] == ["A", "B"]

    with pytest.raises(ferric.FerricModuleNotFoundError):
        engine.set_focus("MISSING")


@covers_manual(
    "Engine.clear_diagnostics",
    "Engine.clear_output",
    "Engine.diagnostics",
    "Engine.get_output",
    "Engine.push_input",
)
def test_manual_io_and_diagnostics():
    # Captured output is how printout reaches Python callers; this checks both
    # retrieval and explicit channel clearing.
    engine = ferric.Engine.from_source("""
        (deffacts startup (go))
        (defrule say (go) => (printout t "hello" crlf))
    """)
    assert engine.diagnostics == []
    engine.run()
    assert engine.get_output("t") == "hello\n"
    engine.clear_output("t")
    assert engine.get_output("t") is None

    # push_input currently queues input for read/readline paths; the public
    # method should accept a line even when no rule consumes it.
    engine.push_input("queued line")
    engine.clear_diagnostics()
    assert engine.diagnostics == []


@covers_manual(
    "Engine.from_snapshot",
    "Engine.from_snapshot_file",
    "Engine.save_snapshot",
    "Engine.serialize",
)
def test_manual_serialization_api(tmp_path):
    # Serialization supports in-memory bytes and file convenience APIs; using
    # JSON here gives an explicit human-readable format distinct from defaults.
    engine = ferric.Engine.from_source("""
        (deftemplate sensor (slot id (type INTEGER)))
        (defglobal ?*version* = 3)
    """)
    data = engine.serialize(format=ferric.Format.JSON)
    restored = ferric.Engine.from_snapshot(data, format=ferric.Format.JSON)
    assert restored.templates() == ["sensor"]
    assert restored.get_global("version") == 3

    # The default format is also part of the call signature, so exercise the
    # no-argument snapshot path separately.
    default_data = engine.serialize()
    default_restored = ferric.Engine.from_snapshot(default_data)
    assert default_restored.get_global("version") == 3

    path = tmp_path / "engine.snapshot"
    engine.save_snapshot(path, format=ferric.Format.BINCODE)
    from_file = ferric.Engine.from_snapshot_file(path, format=ferric.Format.BINCODE)
    assert from_file.templates() == ["sensor"]


@covers_manual(
    *(SYMBOL_MEMBERS | SYMBOL_PROTOCOLS | STRING_MEMBERS | STRING_PROTOCOLS),
    "ferric.FerricEncodingError",
)
def test_manual_value_wrappers_and_conversion():
    # Symbol and String intentionally compare equal to Python str for ergonomic
    # use while still preserving distinct CLIPS value types through round-trip.
    symbol = ferric.Symbol("alpha")
    string = ferric.String("alpha")
    assert symbol.value == "alpha"
    assert string.value == "alpha"
    assert str(symbol) == "alpha"
    assert str(string) == "alpha"
    assert repr(symbol) == 'Symbol("alpha")'
    assert repr(string) == 'String("alpha")'
    assert symbol == "alpha"
    assert string == "alpha"
    assert hash(symbol) == hash("alpha")
    assert hash(string) == hash("alpha")
    assert {symbol, string} == {symbol, string}

    # Conversion covers primitive values plus sequence-to-multifield behavior so
    # callers can pass natural Python values into assert_fact().
    engine = ferric.Engine()
    fid = engine.assert_fact(
        "values",
        42,
        -1.5,
        True,
        False,
        None,
        [1, "two"],
        (3, ferric.String("four")),
    )
    fact = engine.get_fact(fid)
    assert fact.fields[0] == 42
    assert fact.fields[1] == pytest.approx(-1.5)
    assert fact.fields[2] == "TRUE"
    assert fact.fields[3] == "FALSE"
    assert fact.fields[4] is None
    assert fact.fields[5] == [1, "two"]
    assert fact.fields[6][0] == 3
    assert isinstance(fact.fields[6][1], ferric.String)
    assert fact.fields[6][1] == "four"

    # Encoding errors should be exposed as FerricEncodingError, not a generic
    # runtime or type error, when the configured encoding rejects a value.
    ascii_engine = ferric.Engine(encoding=ferric.Encoding.ASCII)
    with pytest.raises(ferric.FerricEncodingError):
        ascii_engine.assert_fact("accent", ferric.String("é"))
