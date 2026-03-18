"""Tests for engine serialization and deserialization."""

import pytest
import ferric


ALL_FORMATS = [
    ferric.Format.BINCODE,
    ferric.Format.JSON,
    ferric.Format.CBOR,
    ferric.Format.MSGPACK,
    ferric.Format.POSTCARD,
]

SOURCE = """
(deftemplate sensor (slot id (type INTEGER)) (slot value (type FLOAT)))
(defrule alert
    (sensor (id ?id) (value ?v&:(> ?v 100.0)))
    =>
    (printout t "ALERT " ?id crlf))
(defglobal ?*threshold* = 42)
"""


class TestSerializeRoundtrip:
    @pytest.mark.parametrize("fmt", ALL_FORMATS)
    def test_roundtrip_preserves_state(self, fmt):
        engine = ferric.Engine.from_source(SOURCE)
        data = engine.serialize(format=fmt)
        assert isinstance(data, bytes)
        assert len(data) > 0

        restored = ferric.Engine.from_snapshot(data, format=fmt)
        assert len(restored.rules()) == 1
        assert restored.get_global("threshold") == 42

    @pytest.mark.parametrize("fmt", ALL_FORMATS)
    def test_deserialized_engine_can_run(self, fmt):
        engine = ferric.Engine.from_source(SOURCE)
        data = engine.serialize(format=fmt)
        restored = ferric.Engine.from_snapshot(data, format=fmt)

        restored.assert_string('(sensor (id 7) (value 200.0))')
        result = restored.run()
        assert result.rules_fired == 1
        output = restored.get_output("t")
        assert output == "ALERT 7\n"

    @pytest.mark.parametrize("fmt", ALL_FORMATS)
    def test_empty_engine_roundtrip(self, fmt):
        engine = ferric.Engine()
        data = engine.serialize(format=fmt)
        restored = ferric.Engine.from_snapshot(data, format=fmt)
        assert restored.fact_count == 0

    def test_default_format_is_bincode(self):
        engine = ferric.Engine.from_source(SOURCE)
        data = engine.serialize()  # no format arg
        restored = ferric.Engine.from_snapshot(data)  # no format arg
        assert len(restored.rules()) == 1


class TestSerializeErrors:
    @pytest.mark.parametrize("fmt", ALL_FORMATS)
    def test_invalid_data_rejected(self, fmt):
        with pytest.raises(ferric.FerricError):
            ferric.Engine.from_snapshot(b"not valid data", format=fmt)

    def test_cross_format_rejected(self):
        engine = ferric.Engine()
        data = engine.serialize(format=ferric.Format.BINCODE)
        with pytest.raises(ferric.FerricError):
            ferric.Engine.from_snapshot(data, format=ferric.Format.JSON)


class TestFileConvenience:
    @pytest.mark.parametrize("fmt", ALL_FORMATS)
    def test_save_and_load_roundtrip(self, fmt, tmp_path):
        engine = ferric.Engine.from_source(SOURCE)
        path = tmp_path / f"snapshot.{fmt}"
        engine.save_snapshot(str(path), format=fmt)

        assert path.exists()
        assert path.stat().st_size > 0

        restored = ferric.Engine.from_snapshot_file(str(path), format=fmt)
        assert len(restored.rules()) == 1
        assert restored.get_global("threshold") == 42

    def test_from_snapshot_file_nonexistent(self):
        with pytest.raises(OSError):
            ferric.Engine.from_snapshot_file("/nonexistent/path.bin")

    def test_save_snapshot_bad_path(self):
        engine = ferric.Engine()
        with pytest.raises(OSError):
            engine.save_snapshot("/nonexistent/dir/snap.bin")
