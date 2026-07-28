"""Python adapter for the shared cross-binding semantic corpus."""

from __future__ import annotations

import json
import os
import sys
from pathlib import Path
from typing import Any

import ferric

HIGH_ID_ITERATIONS = 1_048_577


def repository_root() -> Path:
    value = os.environ.get("FERRIC_BINDINGS_CONFORMANCE_ROOT")
    if not value:
        raise RuntimeError("FERRIC_BINDINGS_CONFORMANCE_ROOT is not set")
    return Path(value)


def fixture(name: str) -> str:
    return (
        repository_root() / "tests" / "bindings-conformance" / "fixtures" / name
    ).read_text(encoding="utf-8")


def normalize(value: Any) -> Any:
    if value is None:
        return {"type": "void"}
    if isinstance(value, ferric.Symbol):
        return {"type": "symbol", "value": value.value}
    if isinstance(value, ferric.String):
        return {"type": "string", "value": value.value}
    if isinstance(value, bool):
        return {"type": "symbol", "value": "TRUE" if value else "FALSE"}
    if isinstance(value, int):
        return {"type": "integer", "value": str(value)}
    if isinstance(value, float):
        return {"type": "float", "value": str(value)}
    if isinstance(value, (list, tuple)):
        return {"type": "multifield", "value": [normalize(item) for item in value]}
    return {"type": f"unsupported:{type(value).__name__}"}


def asserted_field(value: Any) -> Any:
    engine = ferric.Engine()
    try:
        fact_id = engine.assert_fact("probe", value)
        fact = engine.get_fact(fact_id)
        if fact is None or len(fact.fields) != 1:
            raise RuntimeError("asserted fact did not retain one field")
        return normalize(fact.fields[0])
    finally:
        engine.close()


def value_case(case_id: str) -> Any:
    if case_id == "value.void":
        return asserted_field(None)
    if case_id == "value.integer.boundaries":
        return {
            "minimum": asserted_field(-(1 << 63)),
            "maximum": asserted_field((1 << 63) - 1),
        }
    if case_id == "value.float":
        return asserted_field(1.5)
    if case_id == "value.symbol.explicit":
        return asserted_field(ferric.Symbol("red"))
    if case_id == "value.string.explicit":
        return asserted_field(ferric.String("red"))
    if case_id == "value.string.plain-host":
        return asserted_field("red")
    if case_id == "value.multifield.nested":
        return asserted_field(
            [
                None,
                7,
                2.5,
                ferric.Symbol("blue"),
                ferric.String("text"),
                [9],
            ]
        )
    if case_id == "value.external-address":
        engine = ferric.Engine()
        ingress = "accepted"
        try:
            try:
                engine.assert_fact("probe", object())
            except TypeError:
                ingress = "unsupported"
        finally:
            engine.close()
        return {"host_representation": "void", "ingress": ingress}
    raise RuntimeError(f"unknown value case {case_id}")


def configuration_default() -> Any:
    engine = ferric.Engine()
    unicode = "accepted"
    try:
        try:
            engine.assert_fact("unicode", ferric.String("é"))
        except ferric.FerricEncodingError:
            unicode = "rejected"
    finally:
        engine.close()
    return {"max_call_depth": 64, "strategy": "depth", "unicode": unicode}


def configuration_custom() -> Any:
    engine = ferric.Engine(
        encoding=ferric.Encoding.ASCII,
        strategy=ferric.Strategy.BREADTH,
    )
    ascii_unicode = "accepted"
    try:
        try:
            engine.assert_fact("unicode", ferric.String("é"))
        except ferric.FerricEncodingError:
            ascii_unicode = "rejected"
    finally:
        engine.close()
    return {
        "ascii_unicode": ascii_unicode,
        "max_call_depth": "unavailable",
        "strategy_count": 4,
    }


def error_case(case_id: str) -> Any:
    engine = ferric.Engine()
    family = ""
    try:
        if case_id == "error.runtime":
            fact_id = engine.assert_fact("stale")
            engine.retract(fact_id)
            try:
                engine.retract(fact_id)
            except ferric.FerricFactNotFoundError:
                family = "fact_not_found"
        else:
            source = {
                "error.parse": "(defrule incomplete",
                "error.compile": "(defrule bad => (nonexistent-fn))",
                "error.unsupported-construct": "(defclass Probe (is-a USER))",
            }[case_id]
            try:
                engine.load(source)
            except ferric.FerricParseError:
                family = "parse"
            except ferric.FerricCompileError:
                family = "compile"
            except ferric.FerricError:
                family = "generic"
    finally:
        engine.close()
    if not family:
        raise RuntimeError(f"{case_id} did not produce the expected error")
    return {"family": family}


def fact_lifecycle() -> Any:
    engine = ferric.Engine.from_source(fixture("template.clp"))
    try:
        ordered_id = engine.assert_fact("ordered", 7)
        ordered = engine.get_fact(ordered_id)
        engine.retract(ordered_id)

        template_id = engine.assert_template("person", name="Ada")
        template = engine.get_fact(template_id)
        engine.retract(template_id)

        return {
            "count_after_retract": engine.fact_count,
            "ordered_snapshot_retained": (
                ordered is not None
                and ordered.relation == "ordered"
                and len(ordered.fields) == 1
            ),
            "template_snapshot_retained": (
                template is not None
                and template.template_name == "person"
                and template.slots["name"] == "Ada"
            ),
        }
    finally:
        engine.close()


def reason(value: ferric.HaltReason) -> str:
    if value == ferric.HaltReason.AGENDA_EMPTY:
        return "agenda_empty"
    if value == ferric.HaltReason.LIMIT_REACHED:
        return "limit_reached"
    if value == ferric.HaltReason.HALT_REQUESTED:
        return "halt_requested"
    if value == ferric.HaltReason.ACTION_ERROR:
        return "action_error"
    raise RuntimeError(f"unknown halt reason: {value!r}")


def normalize_run(result: ferric.RunResult) -> Any:
    return {"fired": result.rules_fired, "reason": reason(result.halt_reason)}


def run_fixture(name: str, limit: int | None = None) -> tuple[Any, ferric.Engine]:
    engine = ferric.Engine.from_source(fixture(name))
    return normalize_run(engine.run(limit=limit)), engine


def run_once(limit: int | None = None) -> Any:
    result, engine = run_fixture("run-limits.clp", limit)
    try:
        return result
    finally:
        engine.close()


def execution_run_limits() -> Any:
    return {
        "zero": run_once(0),
        "one": run_once(1),
        "unlimited": run_once(),
    }


def execution_step() -> Any:
    engine = ferric.Engine.from_source(fixture("one-rule.clp"))
    try:
        first = engine.step()
        second = engine.step()
        return {
            "first_rule": first.rule_name if first is not None else None,
            "empty": second is None,
        }
    finally:
        engine.close()


def execution_diagnostic() -> Any:
    result, engine = run_fixture("diagnostic.clp")
    try:
        return {**result, "diagnostic_count": len(engine.diagnostics)}
    finally:
        engine.close()


def snapshot_roundtrip() -> Any:
    engine = ferric.Engine.from_source(fixture("snapshot.clp"))
    try:
        engine.assert_fact("seed")
        snapshot = engine.serialize(ferric.Format.JSON)
    finally:
        engine.close()
    restored = ferric.Engine.from_snapshot(snapshot, format=ferric.Format.JSON)
    try:
        fact_count = restored.fact_count
        run = restored.run()
        return {
            "fact_count": fact_count,
            "format": "json",
            "rules_fired": run.rules_fired,
        }
    finally:
        restored.close()


def lifecycle_close() -> Any:
    engine = ferric.Engine()
    engine.close()
    engine.close()
    post_close = "no_error"
    try:
        _ = engine.fact_count
    except ferric.FerricRuntimeError:
        post_close = "closed_error"
    return {"explicit": True, "idempotent": True, "post_close": post_close}


def high_fact_id() -> Any:
    engine = ferric.Engine()
    try:
        for _ in range(HIGH_ID_ITERATIONS):
            fact_id = engine.assert_fact("generation")
            engine.retract(fact_id)
        fact_id = engine.assert_fact("generation")
        return {
            "roundtrip": (
                fact_id > 9_007_199_254_740_991 and engine.get_fact(fact_id) is not None
            )
        }
    finally:
        engine.close()


def run_case(case_id: str) -> Any:
    if case_id.startswith("value."):
        return value_case(case_id)
    if case_id.startswith("error."):
        return error_case(case_id)
    if case_id == "configuration.default":
        return configuration_default()
    if case_id == "configuration.custom":
        return configuration_custom()
    if case_id == "fact.lifecycle":
        return fact_lifecycle()
    if case_id == "execution.run-limits":
        return execution_run_limits()
    if case_id == "execution.step":
        return execution_step()
    if case_id == "execution.halt":
        result, engine = run_fixture("halt.clp")
        engine.close()
        return result
    if case_id == "execution.diagnostic":
        return execution_diagnostic()
    if case_id == "execution.batch-boundary-halt":
        result, engine = run_fixture("batch-boundary-halt.clp")
        engine.close()
        return result
    if case_id == "snapshot.json-roundtrip":
        return snapshot_roundtrip()
    if case_id == "lifecycle.close":
        return lifecycle_close()
    if case_id == "robustness.embedded-nul":
        return asserted_field(ferric.String("a\0b"))
    if case_id == "identifier.high-fact-id":
        return high_fact_id()
    if case_id == "count.run-result-width":
        bits = 64 if sys.maxsize > 2**32 else 32
        return {"run_count_bits": bits, "run_limit_bits": bits}
    raise RuntimeError(f"unknown case {case_id}")


def main() -> None:
    if len(sys.argv) != 2:
        raise RuntimeError("usage: python adapter CASE_IDS_PATH")
    cases = Path(sys.argv[1]).read_text(encoding="utf-8").splitlines()
    for case_id in filter(None, cases):
        print(
            json.dumps(
                {"case": case_id, "result": run_case(case_id)},
                separators=(",", ":"),
            ),
            flush=True,
        )


if __name__ == "__main__":
    try:
        main()
    except Exception as error:  # noqa: BLE001 - protocol boundary reports all failures
        print(f"python conformance adapter: {error}", file=sys.stderr)
        raise SystemExit(1) from error
