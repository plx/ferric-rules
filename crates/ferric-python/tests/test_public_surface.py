"""Public-API surface freeze for the ferric Python binding.

This is a drift detector. It pins the *exact* set of public names the
extension exposes at the package, class, and enum level so that an accidental
addition or removal of a public symbol (easy to do with PyO3's `#[pymethods]`)
fails loudly. It also checks the Python protocols each type implements and
verifies thread-affinity enforcement behaviorally (affinity is a runtime
behavior in `PyEngine::with_engine`, not an attribute, so it cannot be
witnessed with `hasattr`).

Feature-gated surface is treated as optional so the freeze holds across build
configurations: `Format` and the serialization methods are `#[cfg(serde)]`,
and `engine_instance_count` is `#[cfg(testing)]` (see src/lib.rs).
"""

from __future__ import annotations

import threading

import ferric


# `Format` is registered only under the `serde` feature and
# `engine_instance_count` only under `testing`; use their presence as the
# sentinel for whether each feature was compiled in.
_HAS_SERDE = hasattr(ferric, "Format")
_HAS_TESTING = hasattr(ferric, "engine_instance_count")


# -- Expected top-level package surface --

_TOP_LEVEL = {
    "Encoding",
    "Engine",
    "Fact",
    "FactType",
    "FerricCompileError",
    "FerricEncodingError",
    "FerricError",
    "FerricFactNotFoundError",
    "FerricModuleNotFoundError",
    "FerricParseError",
    "FerricRuntimeError",
    "FerricSlotNotFoundError",
    "FerricTemplateNotFoundError",
    "FiredRule",
    "HaltReason",
    "RunResult",
    "Strategy",
    "String",
    "Symbol",
    "ferric",  # the compiled extension module, re-exposed under the package
}
if _HAS_SERDE:
    _TOP_LEVEL.add("Format")
if _HAS_TESTING:
    _TOP_LEVEL.add("engine_instance_count")


# -- Expected class members (non-dunder) --

_ENGINE_MEMBERS = {
    "agenda_size",
    "assert_fact",
    "assert_string",
    "assert_template",
    "clear",
    "clear_diagnostics",
    "clear_output",
    "close",
    "current_module",
    "diagnostics",
    "fact_count",
    "facts",
    "find_facts",
    "focus",
    "focus_stack",
    "from_source",
    "get_fact",
    "get_fact_slot",
    "get_global",
    "get_output",
    "halt",
    "is_halted",
    "load",
    "load_file",
    "modules",
    "push_focus",
    "push_input",
    "reset",
    "retract",
    "rules",
    "run",
    "set_focus",
    "step",
    "templates",
}
if _HAS_SERDE:
    # Snapshot serialization is serde-gated.
    _ENGINE_MEMBERS |= {
        "from_snapshot",
        "from_snapshot_file",
        "save_snapshot",
        "serialize",
    }

_CLASS_MEMBERS = {
    "Engine": _ENGINE_MEMBERS,
    "Fact": {
        "engine_id",
        "fact_type",
        "fields",
        "id",
        "relation",
        "slots",
        "template_name",
    },
    "Symbol": {"value"},
    "String": {"value"},
    "RunResult": {"halt_reason", "rules_fired"},
    "FiredRule": {"rule_name"},
}


# -- Expected enum members --

_ENUM_MEMBERS = {
    "Encoding": {"ASCII", "ASCII_SYMBOLS_UTF8_STRINGS", "UTF8"},
    "FactType": {"ORDERED", "TEMPLATE"},
    "HaltReason": {"AGENDA_EMPTY", "HALT_REQUESTED", "LIMIT_REACHED"},
    "Strategy": {"BREADTH", "DEPTH", "LEX", "MEA"},
}
if _HAS_SERDE:
    _ENUM_MEMBERS["Format"] = {"BINCODE", "CBOR", "JSON", "MSGPACK", "POSTCARD"}


# -- Expected Python protocols (dunder methods) --
#
# Only the protocols PyO3 actually wires up are meaningful here; the universal
# object dunders (__init__/__repr__/__eq__/__hash__/__str__) are listed for
# documentation but the real signal is Engine's container/context-manager
# protocols (__contains__/__enter__/__exit__/__len__), which are not on object.
_PROTOCOLS = {
    "Engine": {"__contains__", "__enter__", "__exit__", "__init__", "__len__", "__repr__"},
    "Fact": {"__eq__", "__hash__", "__repr__"},
    "Symbol": {"__eq__", "__hash__", "__init__", "__repr__", "__str__"},
    "String": {"__eq__", "__hash__", "__init__", "__repr__", "__str__"},
    "RunResult": {"__repr__"},
    "FiredRule": {"__repr__"},
}


def test_top_level_surface_is_frozen():
    exported = {name for name in dir(ferric) if not name.startswith("_")}
    assert exported == _TOP_LEVEL


def test_class_members_are_frozen():
    for class_name, expected in _CLASS_MEMBERS.items():
        cls = getattr(ferric, class_name)
        exported = {name for name in dir(cls) if not name.startswith("_")}
        assert exported == expected, class_name


def test_enum_members_are_frozen():
    for enum_name, expected in _ENUM_MEMBERS.items():
        enum_type = getattr(ferric, enum_name)
        exported = {name for name in dir(enum_type) if not name.startswith("_")}
        assert exported == expected, enum_name


def test_protocol_methods_exist():
    for class_name, members in _PROTOCOLS.items():
        cls = getattr(ferric, class_name)
        for member in members:
            assert hasattr(cls, member), f"{class_name}.{member}"


def test_thread_affinity_is_enforced():
    # thread_affinity is a behavior, not an attribute, so prove it by calling
    # across threads and asserting the cross-thread guard raises a
    # FerricRuntimeError (rather than panicking or silently succeeding).
    engine = ferric.Engine()
    try:
        captured: dict[str, BaseException] = {}

        def call_from_worker():
            try:
                engine.rules()
            except BaseException as exc:  # noqa: BLE001
                captured["exc"] = exc

        worker = threading.Thread(target=call_from_worker)
        worker.start()
        worker.join()
        exc = captured.get("exc")
        assert isinstance(exc, ferric.FerricRuntimeError), (
            f"thread affinity not enforced; got {exc!r}"
        )
        assert "wrong thread" in str(exc)
    finally:
        engine.close()
