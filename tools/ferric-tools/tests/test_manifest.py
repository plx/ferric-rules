"""Tests for ferric_tools._manifest.

Verifies load/save round-trip fidelity and the ISO timestamp helper.
"""

from __future__ import annotations

import json

from ferric_tools._manifest import load_manifest, save_manifest, utc_now_iso

# ---------------------------------------------------------------------------
# load_manifest / save_manifest round-trip
# ---------------------------------------------------------------------------


def test_save_then_load_preserves_data(tmp_path):
    # A manifest written by save_manifest should be readable by load_manifest
    # and produce an equal dict — the full round-trip must be lossless.
    manifest_path = tmp_path / "manifest.json"
    original = {
        "version": 1,
        "files": {
            "examples/foo.clp": {
                "classification": "equivalent",
                "reason": "",
            }
        },
    }

    save_manifest(manifest_path, original)
    loaded = load_manifest(manifest_path)

    assert loaded == original


def test_save_creates_parent_directories(tmp_path):
    # save_manifest should create any missing parent directories rather than
    # raising FileNotFoundError.
    deep_path = tmp_path / "a" / "b" / "c" / "manifest.json"

    save_manifest(deep_path, {"version": 1})

    assert deep_path.exists()


def test_save_produces_valid_json(tmp_path):
    # The file written by save_manifest must be parseable by the standard
    # json module independently of load_manifest.
    manifest_path = tmp_path / "manifest.json"
    data = {"key": "value", "numbers": [1, 2, 3]}

    save_manifest(manifest_path, data)

    raw = manifest_path.read_text(encoding="utf-8")
    parsed = json.loads(raw)
    assert parsed == data


def test_save_produces_pretty_printed_json(tmp_path):
    # save_manifest uses indent=2 so the output is human-readable; verify
    # the file contains newlines (i.e. is not collapsed onto a single line).
    manifest_path = tmp_path / "manifest.json"
    save_manifest(manifest_path, {"a": {"b": 1}})

    raw = manifest_path.read_text(encoding="utf-8")
    assert "\n" in raw, "save_manifest should write indented (multi-line) JSON"


def test_save_preserves_unicode(tmp_path):
    # ensure_ascii=False is set; non-ASCII characters must not be escaped.
    manifest_path = tmp_path / "manifest.json"
    data = {"greeting": "héllo wörld"}

    save_manifest(manifest_path, data)

    raw = manifest_path.read_text(encoding="utf-8")
    assert "héllo wörld" in raw, "save_manifest must not escape non-ASCII characters"


def test_load_manifest_returns_dict(tmp_path):
    # load_manifest should return a dict for a JSON object file.
    manifest_path = tmp_path / "simple.json"
    manifest_path.write_text('{"x": 42}\n', encoding="utf-8")

    result = load_manifest(manifest_path)

    assert isinstance(result, dict)
    assert result["x"] == 42


def test_load_manifest_accepts_string_path(tmp_path):
    # The type annotation allows str | Path; a plain string must work.
    manifest_path = tmp_path / "manifest.json"
    manifest_path.write_text('{"ok": true}\n', encoding="utf-8")

    result = load_manifest(str(manifest_path))

    assert result == {"ok": True}


# ---------------------------------------------------------------------------
# utc_now_iso
# ---------------------------------------------------------------------------


def test_utc_now_iso_returns_string():
    result = utc_now_iso()
    assert isinstance(result, str)


def test_utc_now_iso_contains_timezone_offset():
    # datetime.now(UTC).isoformat() always includes a timezone component such
    # as "+00:00"; the result must not be a naive (no-TZ) timestamp.
    result = utc_now_iso()
    assert "+" in result or result.endswith("Z"), (
        f"utc_now_iso() must include a UTC offset, got: {result!r}"
    )


def test_utc_now_iso_is_parseable_as_iso8601():
    # Round-trip back through datetime.fromisoformat() to confirm the format
    # is valid ISO 8601.
    from datetime import datetime

    result = utc_now_iso()
    dt = datetime.fromisoformat(result)
    assert dt.tzinfo is not None, "Parsed datetime must be timezone-aware"
