"""Tests for revision-aware Criterion benchmark collection."""

from __future__ import annotations

from pathlib import Path

import pytest

from ferric_tools.perf.collect import _benchmark_crate


def test_benchmark_crate_selects_renamed_facade(tmp_path: Path):
    bench_dir = tmp_path / "crates" / "ferric-rules" / "benches"
    bench_dir.mkdir(parents=True)

    assert _benchmark_crate(tmp_path) == ("ferric-rules", bench_dir)


def test_benchmark_crate_selects_legacy_facade(tmp_path: Path):
    bench_dir = tmp_path / "crates" / "ferric" / "benches"
    bench_dir.mkdir(parents=True)

    assert _benchmark_crate(tmp_path) == ("ferric", bench_dir)


def test_benchmark_crate_rejects_unknown_layout(tmp_path: Path):
    with pytest.raises(FileNotFoundError, match="could not find facade benchmarks"):
        _benchmark_crate(tmp_path)
