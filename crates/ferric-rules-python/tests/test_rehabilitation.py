"""Useful embedding contracts, bounded inputs, and deliberate pre-1.0 changes."""

import subprocess
import sys
from pathlib import Path

import ferric
import pytest

LAUNCH_SOURCE = (
    Path(__file__).resolve().parents[3]
    / "examples"
    / "embedding"
    / "launch-selection.clp"
)
RECURSION = """
(deffunction go (?x) (if (> ?x 0) then (go (- ?x 1)) else 42))
(defrule execute => (printout t (go 2) crlf))
"""


@pytest.mark.parametrize(
    "factory",
    [ferric.Engine, lambda **options: ferric.Engine.from_source("", **options)],
)
@pytest.mark.parametrize("depth", [-1, 2**32, 2**100])
def test_call_depth_range_is_checked_without_narrowing(factory, depth):
    with pytest.raises(ValueError, match="max_call_depth"):
        factory(max_call_depth=depth)


@pytest.mark.parametrize(
    "factory",
    [ferric.Engine, lambda **options: ferric.Engine.from_source("", **options)],
)
@pytest.mark.parametrize("depth", [True, 1.5, "3", object()])
def test_call_depth_requires_an_actual_integer(factory, depth):
    with pytest.raises(TypeError, match="max_call_depth"):
        factory(max_call_depth=depth)


def test_call_depth_has_runtime_effect_and_survives_snapshot():
    assert ferric.Engine().max_call_depth == 64
    assert ferric.Engine(max_call_depth=2**32 - 1).max_call_depth == 2**32 - 1
    for depth in (0, 1):
        with ferric.Engine.from_source(RECURSION, max_call_depth=depth) as limited:
            assert limited.max_call_depth == depth
            assert limited.run().halt_reason == ferric.HaltReason.ACTION_ERROR
            assert "recursion limit" in limited.diagnostics[0]
    with ferric.Engine.from_source(RECURSION, max_call_depth=3) as engine:
        data = engine.serialize()
    with ferric.Engine.from_snapshot(data) as restored:
        assert restored.max_call_depth == 3
        assert restored.run().rules_fired == 1
        assert restored.get_output("t") == "42\n"
    with pytest.raises(ferric.FerricRuntimeError):
        _ = restored.max_call_depth


@pytest.mark.parametrize(
    "source,exception,message",
    [
        ("(defrule incomplete", ferric.FerricParseError, "unclosed parenthesis"),
        ("(defrule missing (x))", ferric.FerricParseError, "missing =>"),
        ("(defclass Probe (is-a USER))", ferric.FerricCompileError, "defclass"),
    ],
)
def test_load_errors_have_concrete_types_across_constructors_and_files(
    tmp_path, source, exception, message
):
    path = tmp_path / "rules.clp"
    path.write_text(source, encoding="utf-8")
    with pytest.raises(exception, match=message):
        ferric.Engine.from_source(source)
    for operation in (
        lambda engine: engine.load(source),
        lambda engine: engine.load_file(path),
    ):
        with ferric.Engine() as engine, pytest.raises(exception, match=message):
            operation(engine)


def test_load_error_aggregation_and_io_are_distinct(tmp_path):
    with ferric.Engine() as engine:
        with pytest.raises(ferric.FerricCompileError) as raised:
            engine.load("(defclass First (is-a USER)) (defclass Second (is-a USER))")
        assert str(raised.value).count("unsupported top-level form: defclass") == 2
        with pytest.raises(OSError):
            engine.load_file(tmp_path / "missing.clp")
        invalid_utf8 = tmp_path / "invalid.clp"
        invalid_utf8.write_bytes(b"\xff")
        with pytest.raises(OSError):
            engine.load_file(invalid_utf8)


def test_snapshots_are_versioned_cbor_by_default_with_owned_typed_errors(tmp_path):
    assert issubclass(ferric.FerricSerializationError, ferric.FerricError)
    with ferric.Engine.from_source(LAUNCH_SOURCE.read_text()) as engine:
        snapshot = engine.serialize()
        assert snapshot[:8] == b"FERRIC\0S"
        assert snapshot[10] == 2  # The envelope's CBOR discriminator.
        with ferric.Engine.from_snapshot(
            snapshot, format=ferric.Format.CBOR
        ) as restored:
            assert restored.run().rules_fired == 1
    for data, message in [
        (b"legacy", "legacy"),
        (snapshot[:8] + b"\xff\xff" + snapshot[10:], "version 65535"),
        (snapshot[:-1], "length"),
        (b"\0" * (16 * 1024 * 1024 + 1), "16 MiB"),
    ]:
        with pytest.raises(ferric.FerricSerializationError, match=message):
            ferric.Engine.from_snapshot(data)
    with pytest.raises(ferric.FerricSerializationError, match="format"):
        ferric.Engine.from_snapshot(snapshot, format=ferric.Format.BINCODE)
    sparse = tmp_path / "huge.cbor"
    with sparse.open("wb") as handle:
        handle.truncate(1024 * 1024 * 1024)
    with pytest.raises(ferric.FerricSerializationError, match="16 MiB"):
        ferric.Engine.from_snapshot_file(sparse)


def test_multifield_limits_reject_cycles_without_process_failure():
    # A removed depth guard should fail with a bounded subprocess, not hang or
    # terminate the pytest process. Test both ordered and template conversion.
    source = r"""
import ferric
engine = ferric.Engine.from_source('(deftemplate record (multislot data))')
cyclic = []
cyclic.append(cyclic)
deep = 1
for _ in range(32):
    deep = [deep]
allowed = engine.assert_fact('data', deep)
engine.retract(allowed)
deep = [deep]
empty_deep = []
for _ in range(32):
    empty_deep = [empty_deep]
for value in (cyclic, deep, empty_deep):
    for operation in (lambda: engine.assert_fact('data', value), lambda: engine.assert_template('record', data=value)):
        try:
            operation()
        except ValueError as error:
            assert 'nesting' in str(error), error
        else:
            raise AssertionError('unbounded nesting accepted')
assert engine.find_facts('data') == []
assert engine.find_facts('record') == []
try:
    engine.assert_fact('data', [0] * 500_000, [0] * 500_000)
except ValueError as error:
    assert '1000000' in str(error), error
else:
    raise AssertionError('aggregate value limit skipped across fields')
identifier = engine.assert_fact('still-live', 42)
assert engine.get_fact(identifier).fields == [42]
engine.close()
"""
    result = subprocess.run(
        [sys.executable, "-c", source],
        capture_output=True,
        text=True,
        timeout=30,
        check=False,
    )
    assert result.returncode == 0, result.stdout + result.stderr


def test_shared_launch_selection_and_snapshot_resume(tmp_path):
    with ferric.Engine.from_source(LAUNCH_SOURCE.read_text()) as engine:
        assert engine.run(limit=0).rules_fired == 0
        assert engine.find_facts("action") == []
        snapshot = engine.serialize()
        path = tmp_path / "launch.cbor"
        engine.save_snapshot(path)
        assert engine.run(limit=1).rules_fired == 1
        assert engine.run().rules_fired == 0
        expected = [ferric.Symbol("session-42"), ferric.Symbol("sign-in")]
        assert [fact.fields for fact in engine.find_facts("action")] == [expected]
        assert engine.get_output("t") == "action session-42 sign-in\n"
    for restored in (
        ferric.Engine.from_snapshot(snapshot),
        ferric.Engine.from_snapshot_file(path),
    ):
        with restored:
            assert restored.run().rules_fired == 1
            action = restored.find_facts("action")
            assert [fact.fields for fact in action] == [expected]
            assert restored.run().rules_fired == 0
            completed = restored.serialize()
        assert action[0].fields == expected  # Owned after close.
        with ferric.Engine.from_snapshot(completed) as resumed:
            assert resumed.run().rules_fired == 0
            assert [fact.fields for fact in resumed.find_facts("action")] == [expected]


def test_requested_depth_cannot_disable_native_safety_limits():
    source = r"""
import ferric, threading, traceback
try:
    import resource
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
except ImportError:
    pass
threading.stack_size(2 * 1024 * 1024)
errors = []
def worker():
    try:
        for body in ('(recurse)', '(if TRUE then (if TRUE then (if TRUE then (if TRUE then (recurse)))))'):
            engine = ferric.Engine.from_source(f'(deffunction recurse () {body}) (defrule run => (recurse))', max_call_depth=100000)
            assert engine.max_call_depth == 100000
            assert engine.effective_max_call_depth == 32
            restored = ferric.Engine.from_snapshot(engine.serialize())
            assert restored.max_call_depth == 100000
            assert restored.effective_max_call_depth == 32
            result = restored.run()
            assert result.halt_reason == ferric.HaltReason.ACTION_ERROR
            assert any('limit exceeded' in message for message in restored.diagnostics)
            restored.clear()
            restored.load('(defrule recovered => (assert (done)))')
            assert restored.run().rules_fired == 1
            assert len(restored.find_facts('done')) == 1
            engine.close()
            restored.close()
    except BaseException:
        errors.append(traceback.format_exc())
thread = threading.Thread(target=worker)
thread.start()
thread.join()
assert not errors, errors
"""
    result = subprocess.run(
        [sys.executable, "-c", source],
        capture_output=True,
        text=True,
        timeout=30,
        check=False,
    )
    assert result.returncode == 0, result.stdout + result.stderr
