"""Typed byte lexemes retain identity, contents, snapshots and output."""

import ferric
import pytest


@pytest.mark.parametrize("wrapper", [ferric.String, ferric.Symbol, ferric.InstanceName])
@pytest.mark.parametrize("payload", [b"", b"a\x00b", b"\xff\xc3(", "résumé".encode()])
def test_wrapper_payloads_are_lossless_and_immutable(engine, wrapper, payload):
    value = wrapper(payload)
    assert value.bytes == payload
    assert value == wrapper(payload)
    assert hash(value) == hash(wrapper(payload))
    with pytest.raises(AttributeError):
        value.bytes = b"changed"
    fid = engine.assert_fact("bytes", value, [value])
    assert engine.get_fact(fid).fields == [value, [value]]
    if payload == b"\xff\xc3(":
        with pytest.raises(UnicodeDecodeError):
            _ = value.value
        with pytest.raises(UnicodeDecodeError):
            str(value)
    else:
        assert value.value == payload.decode()


def test_same_bytes_remain_three_distinct_types(engine):
    values = [
        kind(b"name") for kind in (ferric.String, ferric.Symbol, ferric.InstanceName)
    ]
    assert len(set(values)) == 3
    assert engine.get_fact(engine.assert_fact("typed", *values)).fields == values


def test_raw_output_is_owned_and_text_access_is_checked(engine):
    engine.load("(defrule emit (bytes ?x) => (printout t ?x))")
    engine.reset()
    engine.assert_fact("bytes", ferric.String(b"a\x00\xffb"))
    assert engine.run().rules_fired == 1
    output = engine.get_output_bytes("t")
    assert output == b"a\x00\xffb"
    with pytest.raises(UnicodeDecodeError):
        engine.get_output("t")
    assert engine.get_output_bytes("missing") is None
    engine.clear_output("t")
    engine.close()
    assert output == b"a\x00\xffb"


@pytest.mark.parametrize(
    "fmt",
    [
        ferric.Format.BINCODE,
        ferric.Format.JSON,
        ferric.Format.CBOR,
        ferric.Format.MSGPACK,
        ferric.Format.POSTCARD,
    ],
)
def test_all_codecs_preserve_byte_lexemes(fmt):
    engine = ferric.Engine()
    values = [
        kind(b"a\x00\xff")
        for kind in (ferric.String, ferric.Symbol, ferric.InstanceName)
    ]
    engine.assert_fact("bytes", *values)
    restored = ferric.Engine.from_snapshot(engine.serialize(format=fmt), format=fmt)
    assert restored.find_facts("bytes")[0].fields == values


@pytest.mark.parametrize("wrapper", [ferric.String, ferric.Symbol, ferric.InstanceName])
def test_explicit_bytes_keep_strict_ascii_validation(wrapper):
    engine = ferric.Engine(encoding=ferric.Encoding.ASCII)
    with pytest.raises(ferric.FerricEncodingError):
        engine.assert_fact("bytes", wrapper(b"\xff"))
    assert engine.fact_count == 0


@pytest.mark.parametrize("operation", ["type", "named"])
def test_missing_instance_lookup_halts_without_erasing_name(engine, operation):
    engine.load(
        "(defgeneric named) "
        "(defmethod named ((?x INSTANCE-NAME)) unreachable) "
        "(defrule check (name ?x) => "
        "(printout t (instance-namep ?x) crlf) (assert (before ?x)) "
        f"({operation} ?x) (assert (after)))"
    )
    engine.reset()
    name = ferric.InstanceName(b"missing")
    engine.assert_fact("name", name)
    result = engine.run()
    assert result.halt_reason == ferric.HaltReason.ACTION_ERROR
    assert result.rules_fired == 1
    assert engine.diagnostics
    assert engine.get_output("t") == "TRUE\n"
    assert engine.find_facts("before")[0].fields == [name]
    assert engine.find_facts("after") == []


@pytest.mark.parametrize("wrapper", [ferric.String, ferric.Symbol, ferric.InstanceName])
def test_text_surrogates_are_rejected_while_explicit_bytes_are_preserved(wrapper):
    with pytest.raises(UnicodeEncodeError):
        wrapper("\ud800")
    raw = wrapper(b"\xed\xa0\x80")
    assert raw.bytes == b"\xed\xa0\x80"
    with pytest.raises(UnicodeDecodeError):
        _ = raw.value
