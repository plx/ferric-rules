"""Tests for ferric_tools._formatting.

Covers fmt_ns(), fmt_ns_unicode(), normalize_output(), and normalize_floats().

Note on actual fmt_ns() return values (verified from source):
  - None  → "n/a"  (not "-")
  - 1_500_000_000 ns → "1.500 s"  (three decimal places, not two)
"""

from __future__ import annotations

from ferric_tools._formatting import fmt_ns, fmt_ns_unicode, normalize_floats, normalize_output

# ---------------------------------------------------------------------------
# fmt_ns — nanosecond formatting with auto-scaling
# ---------------------------------------------------------------------------


def test_fmt_ns_sub_microsecond():
    # Values below 1 000 ns are displayed in nanoseconds with no decimal.
    assert fmt_ns(123) == "123 ns"


def test_fmt_ns_microsecond_range():
    # 1 500 ns is 1.5 µs; displayed with one decimal place.
    assert fmt_ns(1_500) == "1.5 us"


def test_fmt_ns_millisecond_range():
    # 1 500 000 ns is 1.5 ms; displayed with two decimal places.
    assert fmt_ns(1_500_000) == "1.50 ms"


def test_fmt_ns_second_range():
    # 1 500 000 000 ns is 1.5 s; displayed with three decimal places.
    assert fmt_ns(1_500_000_000) == "1.500 s"


def test_fmt_ns_none_returns_not_available():
    # None represents a missing measurement; the placeholder is "n/a".
    assert fmt_ns(None) == "n/a"


def test_fmt_ns_zero():
    # Zero nanoseconds stays in the ns tier.
    assert fmt_ns(0) == "0 ns"


def test_fmt_ns_boundary_exactly_one_microsecond():
    # Exactly 1 000 ns crosses into the microsecond tier.
    assert fmt_ns(1_000) == "1.0 us"


def test_fmt_ns_boundary_exactly_one_millisecond():
    # Exactly 1 000 000 ns crosses into the millisecond tier.
    assert fmt_ns(1_000_000) == "1.00 ms"


def test_fmt_ns_boundary_exactly_one_second():
    # Exactly 1 000 000 000 ns crosses into the second tier.
    assert fmt_ns(1_000_000_000) == "1.000 s"


# ---------------------------------------------------------------------------
# fmt_ns_unicode — same as fmt_ns but uses Unicode µ for microseconds
# ---------------------------------------------------------------------------


def test_fmt_ns_unicode_microsecond_uses_mu():
    # The Unicode micro sign U+00B5 must appear for µs values.
    result = fmt_ns_unicode(2_500)
    assert "\u00b5s" in result, f"Expected µs in result, got: {result!r}"


def test_fmt_ns_unicode_ns_and_ms_and_s_unchanged():
    # Outside the microsecond tier the output is identical to fmt_ns.
    assert fmt_ns_unicode(500) == "500 ns"
    assert fmt_ns_unicode(1_500_000) == "1.50 ms"
    assert fmt_ns_unicode(2_000_000_000) == "2.000 s"


def test_fmt_ns_unicode_none_returns_not_available():
    assert fmt_ns_unicode(None) == "n/a"


# ---------------------------------------------------------------------------
# normalize_output — strip CLIPS banner/prompt noise
# ---------------------------------------------------------------------------


def test_normalize_output_strips_clips_banner():
    # Lines starting with "CLIPS (" (the CLIPS version banner) must be removed
    # when the engine is "clips".
    raw = "CLIPS (6.4.1 ...)\nhello\n"
    result = normalize_output(raw, engine="clips")
    assert "CLIPS (6" not in result
    assert "hello" in result


def test_normalize_output_strips_clips_prompt():
    # Lines starting with "CLIPS>" are interactive prompts and must be stripped.
    raw = "CLIPS> \nresult\n"
    result = normalize_output(raw, engine="clips")
    assert "CLIPS>" not in result
    assert "result" in result


def test_normalize_output_strips_trailing_empty_lines():
    # The normalizer removes blank lines at the end of the output so that
    # comparisons are not sensitive to trailing whitespace.
    # Note: only *trailing* empty lines are stripped; leading empty lines are
    # preserved (the implementation only pops from the tail of the list).
    raw = "hello\nworld\n\n"
    result = normalize_output(raw, engine="ferric")
    assert result == "hello\nworld\n"


def test_normalize_output_preserves_leading_empty_lines():
    # Leading empty lines are NOT stripped by normalize_output; only trailing
    # empty lines are removed.  This test documents that known behavior.
    raw = "\nhello\n"
    result = normalize_output(raw, engine="ferric")
    assert result.startswith("\n")


def test_normalize_output_does_not_strip_noise_for_ferric_engine():
    # Noise patterns apply only to "clips" engine output; ferric output is
    # passed through (minus trailing whitespace and empty boundary lines).
    raw = "CLIPS> \nhello\n"
    result = normalize_output(raw, engine="ferric")
    assert "CLIPS>" in result


def test_normalize_output_normalizes_line_endings():
    # Windows CRLF line endings must be converted to Unix LF.
    raw = "hello\r\nworld\r\n"
    result = normalize_output(raw, engine="ferric")
    assert "\r" not in result


def test_normalize_output_empty_input_returns_empty_string():
    assert normalize_output("", engine="clips") == ""


# ---------------------------------------------------------------------------
# normalize_floats — collapse redundant trailing zeros in decimal numbers
# ---------------------------------------------------------------------------


def test_normalize_floats_collapses_trailing_zeros():
    # The regex matches runs of 3+ trailing zeros and strips them.
    # "1.0000000" → "1." (all post-dot zeros removed).
    # "1.00000" and "1.0000000" both normalize to the same result.
    assert normalize_floats("1.0000000") == normalize_floats("1.00000")


def test_normalize_floats_collapses_many_zeros_in_longer_decimal():
    # A value like "25.10000" (trailing zeros after a significant digit)
    # collapses its trailing zeros: "25.10000" → "25.1".
    assert normalize_floats("25.10000") == normalize_floats("25.1")


def test_normalize_floats_leaves_short_decimals_alone():
    # A value like "1.23" has no redundant zeros to collapse; it is unchanged.
    assert normalize_floats("1.23") == "1.23"


def test_normalize_floats_handles_non_float_text():
    # Plain text without decimal numbers must pass through unmodified.
    text = "hello world"
    assert normalize_floats(text) == text
