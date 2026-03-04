"""Output formatting and normalization helpers."""

from __future__ import annotations

import re

# CLIPS Docker output includes the interactive prompt and banner.
CLIPS_NOISE_PATTERNS = [
    re.compile(r"^CLIPS>"),
    re.compile(r"^CLIPS \("),
    re.compile(r"^\s+CLIPS \("),
    re.compile(r"^         CLIPS"),
    re.compile(r"^\[CLIPS\]"),
]


def fmt_ns(ns: float | int | None) -> str:
    """Format nanoseconds with appropriate unit.

    Examples:
        >>> fmt_ns(500)
        '500 ns'
        >>> fmt_ns(1_500)
        '1.5 us'
        >>> fmt_ns(1_500_000)
        '1.50 ms'
        >>> fmt_ns(1_500_000_000)
        '1.500 s'
    """
    if ns is None:
        return "n/a"
    ns = float(ns)
    if ns < 1_000:
        return f"{ns:.0f} ns"
    if ns < 1_000_000:
        return f"{ns / 1_000:.1f} us"
    if ns < 1_000_000_000:
        return f"{ns / 1_000_000:.2f} ms"
    return f"{ns / 1_000_000_000:.3f} s"


def fmt_ns_unicode(ns: float | int | None) -> str:
    """Like fmt_ns but uses the Unicode micro sign for microseconds."""
    if ns is None:
        return "n/a"
    ns = float(ns)
    if ns < 1_000:
        return f"{ns:.0f} ns"
    if ns < 1_000_000:
        return f"{ns / 1_000:.1f} \u00b5s"
    if ns < 1_000_000_000:
        return f"{ns / 1_000_000:.2f} ms"
    return f"{ns / 1_000_000_000:.3f} s"


def normalize_output(raw: str, engine: str) -> str:
    """Normalize engine output for comparison.

    Strips engine-specific noise, trailing whitespace, and normalizes
    line endings.
    """
    text = raw.replace("\r\n", "\n")
    lines = text.split("\n")
    result = []

    for line in lines:
        if engine == "clips":
            skip = any(pat.match(line) for pat in CLIPS_NOISE_PATTERNS)
            if skip:
                continue
        result.append(line.rstrip())

    # Strip trailing empty lines
    while result and result[-1] == "":
        result.pop()

    return "\n".join(result) + "\n" if result else ""


def normalize_floats(text: str) -> str:
    """Normalize float representations for looser comparison.

    Converts patterns like 25.0000000 to 25.0 for comparison purposes.
    """
    return re.sub(
        r"(\d+\.\d*?)0{3,}\d*",
        lambda m: m.group(1).rstrip("0") or m.group(1) + "0",
        text,
    )
