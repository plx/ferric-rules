"""Tests for ferric_tools.perf.diff.

Covers compute_diff(), which compares benchmark results between a base and
head manifest and categorises each benchmark as a regression, improvement,
or unchanged.

The thresholds are: >+5% slower → regression, <-5% faster → improvement.

Each entry in the returned lists is a tuple:
  (name: str, suite: str, base_median_ns: float, head_median_ns: float,
   delta_pct: float)
"""

from __future__ import annotations

from ferric_tools.perf.diff import compute_diff

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _manifest(benchmarks: dict) -> dict:
    """Build a minimal perf manifest."""
    return {"benchmarks": benchmarks}


def _benchmark(median_ns: float, suite: str = "default") -> dict:
    return {"median_ns": median_ns, "suite": suite}


# ---------------------------------------------------------------------------
# compute_diff — regression detection
# ---------------------------------------------------------------------------


def test_compute_diff_regression_when_more_than_5_percent_slower():
    # A benchmark that takes 10% longer in head than base must be flagged as a
    # regression (threshold is strictly greater than +5%).
    base_ns = 1_000_000.0
    head_ns = base_ns * 1.10  # +10% — clearly over the threshold

    base = _manifest({"bench_a": _benchmark(base_ns)})
    head = _manifest({"bench_a": _benchmark(head_ns)})

    regressions, improvements, unchanged, _added, _removed = compute_diff(base, head)

    names = [e[0] for e in regressions]
    assert "bench_a" in names
    assert not any(e[0] == "bench_a" for e in improvements)
    assert not any(e[0] == "bench_a" for e in unchanged)


# ---------------------------------------------------------------------------
# compute_diff — improvement detection
# ---------------------------------------------------------------------------


def test_compute_diff_improvement_when_more_than_5_percent_faster():
    # A benchmark that takes 10% less time in head than base must be flagged as
    # an improvement (threshold is strictly less than -5%).
    base_ns = 1_000_000.0
    head_ns = base_ns * 0.85  # -15% — clearly under the threshold

    base = _manifest({"bench_b": _benchmark(base_ns)})
    head = _manifest({"bench_b": _benchmark(head_ns)})

    regressions, improvements, unchanged, _added, _removed = compute_diff(base, head)

    names = [e[0] for e in improvements]
    assert "bench_b" in names
    assert not any(e[0] == "bench_b" for e in regressions)
    assert not any(e[0] == "bench_b" for e in unchanged)


# ---------------------------------------------------------------------------
# compute_diff — unchanged
# ---------------------------------------------------------------------------


def test_compute_diff_unchanged_when_within_5_percent():
    # A benchmark that changes by less than 5% in either direction must be
    # placed in the unchanged bucket.
    base_ns = 1_000_000.0
    head_ns = base_ns * 1.03  # +3% — within the ±5% band

    base = _manifest({"bench_c": _benchmark(base_ns)})
    head = _manifest({"bench_c": _benchmark(head_ns)})

    regressions, improvements, unchanged, _added, _removed = compute_diff(base, head)

    names = [e[0] for e in unchanged]
    assert "bench_c" in names
    assert not any(e[0] == "bench_c" for e in regressions)
    assert not any(e[0] == "bench_c" for e in improvements)


def test_compute_diff_exactly_5_percent_slower_is_unchanged():
    # The regression threshold is strictly greater than 5%; exactly +5% must
    # not trigger a regression.
    base_ns = 1_000_000.0
    head_ns = base_ns * 1.05  # exactly +5%

    base = _manifest({"bench_d": _benchmark(base_ns)})
    head = _manifest({"bench_d": _benchmark(head_ns)})

    regressions, _improvements, unchanged, _added, _removed = compute_diff(base, head)

    assert not any(e[0] == "bench_d" for e in regressions)
    assert any(e[0] == "bench_d" for e in unchanged)


# ---------------------------------------------------------------------------
# compute_diff — added / removed benchmarks
# ---------------------------------------------------------------------------


def test_compute_diff_detects_added_benchmark():
    # A benchmark present only in head is "added".
    base = _manifest({})
    head = _manifest({"new_bench": _benchmark(500_000.0)})

    _regressions, _improvements, _unchanged, added, _removed = compute_diff(base, head)

    assert any(e[0] == "new_bench" for e in added)


def test_compute_diff_detects_removed_benchmark():
    # A benchmark present only in base is "removed".
    base = _manifest({"old_bench": _benchmark(500_000.0)})
    head = _manifest({})

    _regressions, _improvements, _unchanged, _added, removed = compute_diff(base, head)

    assert any(e[0] == "old_bench" for e in removed)


# ---------------------------------------------------------------------------
# compute_diff — delta_pct value
# ---------------------------------------------------------------------------


def test_compute_diff_delta_pct_is_correct():
    # Verify the delta_pct calculation: (head - base) / base * 100.
    base_ns = 1_000_000.0
    head_ns = 1_200_000.0  # +20%

    base = _manifest({"bench_e": _benchmark(base_ns)})
    head = _manifest({"bench_e": _benchmark(head_ns)})

    regressions, _improvements, _unchanged, _added, _removed = compute_diff(base, head)

    assert len(regressions) == 1
    name, _suite, _b_ns, _h_ns, delta_pct = regressions[0]
    assert name == "bench_e"
    assert abs(delta_pct - 20.0) < 0.001
