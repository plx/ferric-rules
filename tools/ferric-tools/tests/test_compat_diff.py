"""Tests for ferric_tools.compat.diff.

Covers compute_diff() and format_markdown().
"""

from __future__ import annotations

from ferric_tools.compat.diff import compute_diff, format_markdown

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _manifest(files: dict) -> dict:
    """Build a minimal manifest dict with the given files mapping."""
    return {"version": 1, "files": files}


def _file_entry(classification: str, reason: str = "") -> dict:
    return {"classification": classification, "reason": reason}


# ---------------------------------------------------------------------------
# compute_diff — classification changes
# ---------------------------------------------------------------------------


def test_compute_diff_improvement_detected():
    # When a file moves from "divergent" (rank 1) to "equivalent" (rank 0)
    # it must appear in real_improvements, not regressions.
    base = _manifest({"foo.clp": _file_entry("divergent")})
    head = _manifest({"foo.clp": _file_entry("equivalent")})

    _base_counts, _head_counts, regressions, real_improvements, _reason_changes = compute_diff(
        base, head
    )

    assert len(real_improvements) == 1
    assert real_improvements[0][0] == "foo.clp"
    assert len(regressions) == 0


def test_compute_diff_regression_detected():
    # When a file moves from "equivalent" to "divergent" it is a regression.
    base = _manifest({"bar.clp": _file_entry("equivalent")})
    head = _manifest({"bar.clp": _file_entry("divergent")})

    _base_counts, _head_counts, regressions, real_improvements, _reason_changes = compute_diff(
        base, head
    )

    assert len(regressions) == 1
    assert regressions[0][0] == "bar.clp"
    assert len(real_improvements) == 0


def test_compute_diff_no_changes_when_manifests_identical():
    # Identical manifests produce no regressions, no improvements, no reason
    # changes, and identical counts.
    entry = _file_entry("pending", "testable")
    base = _manifest({"a.clp": entry, "b.clp": entry})
    head = _manifest({"a.clp": entry, "b.clp": entry})

    base_counts, head_counts, regressions, real_improvements, reason_changes = compute_diff(
        base, head
    )

    assert regressions == []
    assert real_improvements == []
    assert reason_changes == []
    assert base_counts == head_counts


def test_compute_diff_reason_change_within_same_classification():
    # When the classification stays the same but the reason text changes, the
    # entry must land in reason_changes (not real_improvements or regressions).
    base = _manifest({"c.clp": _file_entry("divergent", "old-reason")})
    head = _manifest({"c.clp": _file_entry("divergent", "new-reason")})

    _bc, _hc, regressions, real_improvements, reason_changes = compute_diff(base, head)

    assert len(reason_changes) == 1
    assert reason_changes[0][0] == "c.clp"
    assert len(regressions) == 0
    assert len(real_improvements) == 0


def test_compute_diff_counts_reflect_head_manifest():
    # head_counts should count classifications from the head manifest, not base.
    base = _manifest({"x.clp": _file_entry("pending")})
    head = _manifest({"x.clp": _file_entry("equivalent")})

    _bc, head_counts, _r, _i, _rc = compute_diff(base, head)

    assert head_counts["equivalent"] == 1
    assert head_counts["pending"] == 0


def test_compute_diff_ignores_files_only_in_one_manifest():
    # Files present in only one manifest (added/removed) are not counted as
    # regressions or improvements.
    base = _manifest({"old.clp": _file_entry("equivalent")})
    head = _manifest({"new.clp": _file_entry("equivalent")})

    _bc, _hc, regressions, real_improvements, _rc = compute_diff(base, head)

    assert regressions == []
    assert real_improvements == []


# ---------------------------------------------------------------------------
# format_markdown
# ---------------------------------------------------------------------------


def test_format_markdown_returns_list_of_strings():
    base_counts = {"equivalent": 1, "divergent": 0, "incompatible": 0, "pending": 0}
    head_counts = {"equivalent": 1, "divergent": 0, "incompatible": 0, "pending": 0}

    lines = format_markdown(base_counts, head_counts, [], [], [])

    assert isinstance(lines, list)
    assert all(isinstance(line, str) for line in lines)


def test_format_markdown_contains_report_heading():
    # The very first content line must be the standard heading.
    base_counts = {"equivalent": 0, "divergent": 0, "incompatible": 0, "pending": 1}
    head_counts = {"equivalent": 0, "divergent": 0, "incompatible": 0, "pending": 1}

    lines = format_markdown(base_counts, head_counts, [], [], [])

    assert "## CLIPS Compatibility Report" in lines


def test_format_markdown_lists_regression_file():
    # When there is a regression, the offending file name should appear in the
    # output so readers can identify what broke.
    base_counts = {"equivalent": 1, "divergent": 0, "incompatible": 0, "pending": 0}
    head_counts = {"equivalent": 0, "divergent": 1, "incompatible": 0, "pending": 0}
    regressions = [("my-test.clp", "equivalent", "", "divergent", "")]

    lines = format_markdown(base_counts, head_counts, regressions, [], [])

    full_output = "\n".join(lines)
    assert "my-test.clp" in full_output


def test_format_markdown_no_regressions_says_none():
    # When there are no regressions, the report must include the word "None"
    # under the Regressions heading.
    base_counts = {"equivalent": 1, "divergent": 0, "incompatible": 0, "pending": 0}
    head_counts = {"equivalent": 1, "divergent": 0, "incompatible": 0, "pending": 0}

    lines = format_markdown(base_counts, head_counts, [], [], [])

    full_output = "\n".join(lines)
    assert "None" in full_output
