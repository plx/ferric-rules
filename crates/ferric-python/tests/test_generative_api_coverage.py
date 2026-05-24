"""Deterministic generative/property-style coverage for the Python binding."""

from __future__ import annotations

import math
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
    TESTING_ONLY_TOP_LEVEL,
    TOP_LEVEL_EXPORTS,
    covers_generative,
)


def _worker_exception(fn):
    result = {}

    def target():
        try:
            fn()
        except Exception as exc:
            result["exc"] = exc

    thread = threading.Thread(target=target)
    thread.start()
    thread.join()
    return result.get("exc")


def _normalize(value):
    if isinstance(value, ferric.Symbol):
        return ("Symbol", value.value)
    if isinstance(value, ferric.String):
        return ("String", value.value)
    if isinstance(value, bool):
        return ("Symbol", "TRUE" if value else "FALSE")
    if isinstance(value, str):
        return ("Symbol", value)
    if isinstance(value, (tuple, list)):
        return [_normalize(item) for item in value]
    return value


def _ordered_source(count):
    facts = " ".join(f"(item {idx})" for idx in range(count))
    return f"""
        (deffacts startup {facts})
        (defrule mark (item ?x) => (assert (seen ?x)))
    """


@covers_generative(*(TOP_LEVEL_EXPORTS | EXCEPTION_EXPORTS | ENUM_VALUES))
def test_generated_public_api_contract_from_runtime_introspection():
    # Drop `testing`-only exports so this contract works in both build configs.
    public_names = {
        name
        for name in dir(ferric)
        if not name.startswith("_") and name not in TESTING_ONLY_TOP_LEVEL
    }
    assert TOP_LEVEL_EXPORTS == {f"ferric.{name}" for name in public_names}

    for item in EXCEPTION_EXPORTS - {"ferric.FerricError"}:
        exc_type = getattr(ferric, item.removeprefix("ferric."))
        assert issubclass(exc_type, ferric.FerricError)

    enum_groups = [
        (ferric.Strategy, ["DEPTH", "BREADTH", "LEX", "MEA"]),
        (ferric.Encoding, ["ASCII", "UTF8", "ASCII_SYMBOLS_UTF8_STRINGS"]),
        (ferric.FactType, ["ORDERED", "TEMPLATE"]),
        (ferric.HaltReason, ["AGENDA_EMPTY", "LIMIT_REACHED", "HALT_REQUESTED"]),
        (ferric.Format, ["BINCODE", "JSON", "CBOR", "MSGPACK", "POSTCARD"]),
    ]
    for enum_type, names in enum_groups:
        values = [getattr(enum_type, name) for name in names]
        assert all(
            left != right
            for index, left in enumerate(values)
            for right in values[index + 1 :]
        )
        assert enum_type.__module__ == "ferric"

    assert ferric.ferric.Engine is ferric.Engine
    assert ferric.ferric.Format is ferric.Format


@covers_generative(
    *(
        FACT_MEMBERS
        | FACT_PROTOCOLS
        | SYMBOL_MEMBERS
        | SYMBOL_PROTOCOLS
        | STRING_MEMBERS
        | STRING_PROTOCOLS
    ),
    "Engine.__contains__",
    "Engine.__init__",
    "Engine.__len__",
    "Engine.__repr__",
    "Engine.assert_fact",
    "Engine.fact_count",
    "Engine.facts",
    "Engine.find_facts",
    "Engine.get_fact",
)
def test_generated_ordered_fact_value_and_protocol_roundtrips():
    engine = ferric.Engine()
    expected_by_id = {}

    for idx in range(12):
        relation = f"rel_{idx % 3}"
        values = (
            idx,
            -idx,
            idx + 0.25,
            idx % 2 == 0,
            None,
            f"plain_{idx}",
            ferric.Symbol(f"symbol_{idx}"),
            ferric.String(f"string_{idx}"),
            [idx, f"nested_{idx}", ferric.String(f"deep_{idx}")],
            (idx + 1, ferric.Symbol(f"tuple_{idx}")),
        )
        fid = engine.assert_fact(relation, *values)
        expected_by_id[fid] = (relation, [_normalize(value) for value in values])

    assert engine.fact_count == len(expected_by_id)
    assert len(engine) == len(expected_by_id)
    assert "Engine(" in repr(engine)

    for fid, (relation, expected) in expected_by_id.items():
        assert fid in engine
        fact = engine.get_fact(fid)
        assert fact is not None
        assert fact.id == fid
        assert fact.engine_id > 0
        assert fact.fact_type == ferric.FactType.ORDERED
        assert fact.relation == relation
        assert fact.template_name is None
        assert fact.slots is None
        assert [_normalize(value) for value in fact.fields] == expected
        assert fact == engine.get_fact(fid)
        assert hash(fact) == hash(engine.get_fact(fid))
        assert relation in repr(fact)

    for relation in {relation for relation, _ in expected_by_id.values()}:
        found = engine.find_facts(relation)
        assert {fact.id for fact in found} == {
            fid
            for fid, (fact_relation, _) in expected_by_id.items()
            if fact_relation == relation
        }

    for idx in range(8):
        text = f"key_{idx}"
        assert ferric.Symbol(text).value == text
        assert ferric.String(text).value == text
        assert str(ferric.Symbol(text)) == text
        assert str(ferric.String(text)) == text
        assert repr(ferric.Symbol(text)) == f'Symbol("{text}")'
        assert repr(ferric.String(text)) == f'String("{text}")'
        assert ferric.Symbol(text) == text
        assert ferric.String(text) == text
        assert hash(ferric.Symbol(text)) == hash(text)
        assert hash(ferric.String(text)) == hash(text)


@covers_generative(
    "Engine.assert_string",
    "Engine.fact_count",
    "Engine.get_fact",
    "Engine.retract",
    "ferric.FerricFactNotFoundError",
)
def test_generated_assert_string_and_retract_lifecycle_properties():
    for count in range(1, 8):
        engine = ferric.Engine()
        source = " ".join(f"(generated{idx} value{idx})" for idx in range(count))
        ids = engine.assert_string(source)
        assert len(ids) == count
        assert len(set(ids)) == count
        assert engine.fact_count == count

        for idx, fid in enumerate(ids):
            fact = engine.get_fact(fid)
            assert fact is not None
            assert fact.relation == f"generated{idx}"

        for fid in ids:
            engine.retract(fid)
            assert engine.get_fact(fid) is None
            with pytest.raises(ferric.FerricFactNotFoundError):
                engine.retract(fid)

        assert engine.fact_count == 0


@covers_generative(
    "Engine.assert_template",
    "Engine.get_fact",
    "Engine.get_fact_slot",
    "Engine.templates",
    "ferric.FerricSlotNotFoundError",
    "ferric.FerricTemplateNotFoundError",
)
def test_generated_template_slot_properties():
    engine = ferric.Engine.from_source("""
        (deftemplate generated
            (slot label)
            (slot count (type INTEGER) (default 0))
            (slot state (default ready)))
    """)
    assert engine.templates() == ["generated"]

    for idx in range(10):
        fid = engine.assert_template(
            "generated",
            label=f"label_{idx}",
            count=idx,
            state=f"state_{idx}",
        )
        fact = engine.get_fact(fid)
        assert fact.fact_type == ferric.FactType.TEMPLATE
        assert fact.template_name == "generated"
        assert fact.relation is None
        assert fact.slots["label"] == f"label_{idx}"
        assert fact.slots["count"] == idx
        assert fact.fields[0] == f"label_{idx}"
        assert engine.get_fact_slot(fid, "state") == f"state_{idx}"

    default_id = engine.assert_template("generated", label="defaulted")
    assert engine.get_fact_slot(default_id, "count") == 0
    assert engine.get_fact_slot(default_id, "state") == "ready"

    with pytest.raises(ferric.FerricTemplateNotFoundError):
        engine.assert_template("missing", label="x")
    with pytest.raises(ferric.FerricSlotNotFoundError):
        engine.get_fact_slot(default_id, "missing")


@covers_generative(
    *(RUN_RESULT_MEMBERS | RUN_RESULT_PROTOCOLS | FIRED_RULE_MEMBERS | FIRED_RULE_PROTOCOLS),
    "Engine.agenda_size",
    "Engine.clear",
    "Engine.find_facts",
    "Engine.halt",
    "Engine.is_halted",
    "Engine.reset",
    "Engine.rules",
    "Engine.run",
    "Engine.step",
)
def test_generated_execution_result_and_lifecycle_properties():
    for count in range(2, 7):
        engine = ferric.Engine.from_source(_ordered_source(count))
        assert len(engine.rules()) == 1
        assert engine.agenda_size == count

        fired = engine.step()
        assert isinstance(fired, ferric.FiredRule)
        assert fired.rule_name == "mark"
        assert "mark" in repr(fired)

        result = engine.run()
        assert isinstance(result, ferric.RunResult)
        assert result.rules_fired == count - 1
        assert result.halt_reason == ferric.HaltReason.AGENDA_EMPTY
        assert "RunResult(" in repr(result)
        assert len(engine.find_facts("seen")) == count

    limited = ferric.Engine.from_source(_ordered_source(4)).run(limit=1)
    assert limited.rules_fired == 1
    assert limited.halt_reason == ferric.HaltReason.LIMIT_REACHED

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
    assert halt_engine.rules() == []
    assert halt_engine.fact_count == 0


@covers_generative(
    "Engine.current_module",
    "Engine.focus",
    "Engine.focus_stack",
    "Engine.get_global",
    "Engine.modules",
    "Engine.push_focus",
    "Engine.set_focus",
    "ferric.FerricModuleNotFoundError",
)
def test_generated_module_focus_and_global_properties():
    module_names = [f"M{idx}" for idx in range(6)]
    module_source = "\n".join(f"(defmodule {name})" for name in module_names)
    engine = ferric.Engine.from_source(f"""
        (defglobal ?*seed* = 91)
        {module_source}
    """)

    assert engine.current_module == "MAIN"
    assert set(engine.modules()) == {"MAIN", *module_names}
    assert engine.get_global("seed") == 91
    assert engine.get_global("missing") is None

    for name in module_names:
        engine.set_focus(name)
        assert engine.focus == name
        assert engine.focus_stack[-1] == name

    for name in module_names[:3]:
        engine.push_focus(name)
        assert engine.focus == name
        assert engine.focus_stack[-1] == name

    with pytest.raises(ferric.FerricModuleNotFoundError):
        engine.push_focus("MISSING")


@covers_generative(
    "Engine.clear_diagnostics",
    "Engine.clear_output",
    "Engine.diagnostics",
    "Engine.get_output",
    "Engine.push_input",
    "Engine.run",
)
def test_generated_io_and_diagnostics_properties():
    for idx in range(6):
        message = f"msg_{idx}"
        engine = ferric.Engine.from_source(f"""
            (deffacts startup (go))
            (defrule say (go) => (printout t "{message}" crlf))
        """)
        assert engine.diagnostics == []
        engine.run()
        assert engine.get_output("t") == f"{message}\n"
        engine.clear_output("t")
        assert engine.get_output("t") is None
        engine.push_input(f"input_{idx}")
        engine.clear_diagnostics()
        assert engine.diagnostics == []


@covers_generative(
    "Engine.from_snapshot",
    "Engine.from_snapshot_file",
    "Engine.facts",
    "Engine.get_global",
    "Engine.save_snapshot",
    "Engine.serialize",
    "Engine.templates",
)
def test_generated_serialization_roundtrip_properties(tmp_path):
    formats = [
        ferric.Format.BINCODE,
        ferric.Format.JSON,
        ferric.Format.CBOR,
        ferric.Format.MSGPACK,
        ferric.Format.POSTCARD,
    ]
    for idx, fmt in enumerate(formats):
        engine = ferric.Engine.from_source(f"""
            (deftemplate record (slot label) (slot count (type INTEGER)))
            (defglobal ?*seed* = {idx})
        """)
        engine.assert_template("record", label=f"label_{idx}", count=idx)

        data = engine.serialize(format=fmt)
        assert isinstance(data, bytes)
        assert data

        restored = ferric.Engine.from_snapshot(data, format=fmt)
        assert restored.templates() == ["record"]
        assert restored.get_global("seed") == idx
        facts = restored.facts()
        assert len(facts) == 1
        assert facts[0].slots["count"] == idx

        path = tmp_path / f"snapshot_{idx}.bin"
        engine.save_snapshot(path, format=fmt)
        from_file = ferric.Engine.from_snapshot_file(path, format=fmt)
        assert from_file.get_global("seed") == idx


@covers_generative(
    "Engine.__enter__",
    "Engine.__exit__",
    "Engine.close",
    "Engine.from_source",
    "Engine.load",
    "Engine.load_file",
    "Engine.thread_affinity",
    "ferric.FerricParseError",
    "ferric.FerricRuntimeError",
)
def test_generated_loading_context_close_and_error_properties(tmp_path):
    for idx in range(5):
        source = f"(defrule loaded{idx} (go{idx}) => (assert (done{idx})))"

        loaded = ferric.Engine()
        loaded.load(source)
        assert loaded.rules()[0][0] == f"loaded{idx}"

        path = tmp_path / f"rules_{idx}.clp"
        path.write_text(source)
        file_engine = ferric.Engine()
        file_engine.load_file(path)
        assert file_engine.rules()[0][0] == f"loaded{idx}"

        from_source = ferric.Engine.from_source(source)
        assert from_source.rules()[0][0] == f"loaded{idx}"

        with ferric.Engine.from_source(source) as managed:
            assert managed.rules()[0][0] == f"loaded{idx}"
        with pytest.raises(ferric.FerricRuntimeError, match="closed"):
            managed.rules()

        exc = _worker_exception(lambda: from_source.rules())
        assert isinstance(exc, ferric.FerricRuntimeError)
        assert "wrong thread" in str(exc)

        from_source.close()
        from_source.close()
        with pytest.raises(ferric.FerricRuntimeError, match="closed"):
            from_source.rules()

        with pytest.raises(ferric.FerricParseError):
            ferric.Engine.from_source(f"(defrule incomplete{idx}")


@covers_generative(
    "ferric.FerricEncodingError",
    "ferric.FerricFactNotFoundError",
    "ferric.FerricRuntimeError",
    "ferric.FerricSlotNotFoundError",
    "ferric.FerricTemplateNotFoundError",
)
def test_generated_specific_error_mapping_properties():
    for idx in range(5):
        engine = ferric.Engine()
        fid = engine.assert_fact(f"item{idx}", idx)
        engine.retract(fid)
        with pytest.raises(ferric.FerricFactNotFoundError):
            engine.retract(fid)

        template_engine = ferric.Engine.from_source("(deftemplate t (slot value))")
        with pytest.raises(ferric.FerricTemplateNotFoundError):
            template_engine.assert_template("missing", value=idx)
        real_id = template_engine.assert_template("t", value=idx)
        with pytest.raises(ferric.FerricSlotNotFoundError):
            template_engine.get_fact_slot(real_id, "missing")
        with pytest.raises(ferric.FerricRuntimeError):
            engine.get_fact_slot(engine.assert_fact("ordered", idx), "value")

        ascii_engine = ferric.Engine(encoding=ferric.Encoding.ASCII)
        with pytest.raises(ferric.FerricEncodingError):
            ascii_engine.assert_fact(f"accent{idx}", ferric.String(f"é{idx}"))


@covers_generative("Engine.assert_fact")
def test_generated_numeric_values_are_finite_after_float_roundtrip():
    engine = ferric.Engine()
    for idx in range(1, 20):
        value = idx / 7
        fid = engine.assert_fact("float", value)
        fact = engine.get_fact(fid)
        assert math.isfinite(fact.fields[0])
        assert fact.fields[0] == pytest.approx(value)
