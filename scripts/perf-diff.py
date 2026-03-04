#!/usr/bin/env python3
"""Compare two performance manifests and produce a Markdown summary.

Reads a "base" and "head" perf manifest (as produced by perf-collect.py)
and emits a Markdown table showing benchmark timing changes between the two.

Usage:
    python scripts/perf-diff.py BASE_MANIFEST HEAD_MANIFEST [options]

Options:
    --report FILE              Write a self-contained Markdown report with context
    --repo OWNER/REPO          GitHub repository (for commit links)
    --base-sha SHA             Base commit SHA (for commit links)
    --head-sha SHA             Head commit SHA (for commit links)
    --fail-on-regression       Exit with code 1 if regressions are detected

Stdout is always the comparison table (suitable for $GITHUB_STEP_SUMMARY).
"""

import argparse
import json
import re
import sys


def _natural_sort_key(s):
    """Sort key that orders embedded numbers numerically (natural sort)."""
    return [int(c) if c.isdigit() else c.lower() for c in re.split(r'(\d+)', s)]

# Threshold for classifying changes (percentage)
REGRESSION_THRESHOLD = 5.0   # >+5% is a regression
IMPROVEMENT_THRESHOLD = -5.0  # <-5% is an improvement


def load_manifest(path):
    with open(path, "r", encoding="utf-8") as f:
        return json.load(f)


# ---------------------------------------------------------------------------
# Duration formatting
# ---------------------------------------------------------------------------

def fmt_ns(ns):
    """Format nanoseconds with appropriate unit."""
    if ns is None:
        return "n/a"
    ns = float(ns)
    if ns < 1_000:
        return f"{ns:.0f} ns"
    if ns < 1_000_000:
        return f"{ns / 1_000:.1f} µs"
    if ns < 1_000_000_000:
        return f"{ns / 1_000_000:.2f} ms"
    return f"{ns / 1_000_000_000:.3f} s"


def fmt_delta(pct):
    """Format a percentage delta with sign."""
    if pct is None:
        return "n/a"
    sign = "+" if pct >= 0 else ""
    return f"{sign}{pct:.1f}%"


def fmt_ratio(ferric_ns, clips_ns):
    """Format ferric/CLIPS ratio."""
    if ferric_ns is None or clips_ns is None or clips_ns == 0:
        return "n/a"
    return f"{ferric_ns / clips_ns:.3f}x"


def clips_reference_note(clips_ref):
    """Return a full sentence describing CLIPS methodology."""
    method = (clips_ref or {}).get("methodology")
    if method == "native_wall_clock_launch_adjusted":
        return ("CLIPS reference: native wall-clock on the runner host, with a "
                "matched launch-only invocation subtracted from each sample.")
    if method == "docker_wall_clock_launch_adjusted":
        return ("CLIPS reference: Docker wall-clock, with a matched launch-only "
                "container invocation subtracted from each sample.")
    if method == "native_wall_clock":
        return "CLIPS reference: native wall-clock on the runner host."
    if method == "docker_wall_clock":
        return "CLIPS reference: Docker wall-clock (includes container startup)."

    runner = (clips_ref or {}).get("runner")
    if runner == "native":
        return "CLIPS reference: native wall-clock on the runner host."
    if runner == "docker":
        return "CLIPS reference: Docker wall-clock."
    return "CLIPS reference: external wall-clock timings."


# ---------------------------------------------------------------------------
# Diff computation
# ---------------------------------------------------------------------------

def compute_diff(base, head):
    """Compare benchmarks between base and head manifests.

    Returns (regressions, improvements, unchanged, added, removed).
    Each entry is (name, suite, base_median_ns, head_median_ns, delta_pct).
    """
    base_benchmarks = base.get("benchmarks", {})
    head_benchmarks = head.get("benchmarks", {})

    regressions = []
    improvements = []
    unchanged = []
    added = []
    removed = []

    all_names = sorted(set(base_benchmarks) | set(head_benchmarks),
                       key=_natural_sort_key)

    for name in all_names:
        b = base_benchmarks.get(name)
        h = head_benchmarks.get(name)

        if b is None:
            suite = h["suite"] if h else "unknown"
            h_median = h.get("median_ns") if h else None
            added.append((name, suite, None, h_median, None))
            continue

        if h is None:
            suite = b["suite"] if b else "unknown"
            b_median = b.get("median_ns") if b else None
            removed.append((name, suite, b_median, None, None))
            continue

        suite = h.get("suite", b.get("suite", "unknown"))
        b_median = b.get("median_ns")
        h_median = h.get("median_ns")

        # Skip if either measurement is missing
        if b_median is None or h_median is None:
            unchanged.append((name, suite, b_median, h_median, None))
            continue

        # Avoid division by zero
        if b_median == 0:
            delta_pct = 0.0 if h_median == 0 else 100.0
        else:
            delta_pct = (h_median - b_median) / b_median * 100.0

        entry = (name, suite, b_median, h_median, delta_pct)

        if delta_pct > REGRESSION_THRESHOLD:
            regressions.append(entry)
        elif delta_pct < IMPROVEMENT_THRESHOLD:
            improvements.append(entry)
        else:
            unchanged.append(entry)

    # Sort regressions by severity (most regressed first)
    regressions.sort(key=lambda e: -(e[4] or 0))
    # Sort improvements by magnitude (most improved first)
    improvements.sort(key=lambda e: (e[4] or 0))

    return regressions, improvements, unchanged, added, removed


# ---------------------------------------------------------------------------
# Markdown formatting
# ---------------------------------------------------------------------------

def format_markdown(regressions, improvements, unchanged, added, removed,
                    *, repo=None, base_sha=None, head_sha=None,
                    clips_ref=None):
    """Build the full Markdown report as a list of lines."""
    lines = []
    clips_section = clips_ref or {}
    clips = clips_section.get("benchmarks") or {}
    has_clips = bool(clips)

    total = len(regressions) + len(improvements) + len(unchanged)

    lines.append("## Performance Report")
    lines.append("")
    lines.append(f"Compares Criterion benchmark performance between two commits "
                 f"across {total} benchmarks.")
    lines.append(f"Regressions: >+{REGRESSION_THRESHOLD:.0f}% slower. "
                 f"Improvements: >{-IMPROVEMENT_THRESHOLD:.0f}% faster.")
    lines.append("")

    if repo and base_sha and head_sha:
        base_link = f"[`{base_sha[:10]}`](https://github.com/{repo}/commit/{base_sha})"
        head_link = f"[`{head_sha[:10]}`](https://github.com/{repo}/commit/{head_sha})"
        lines.append(f"Base: {base_link} | Head: {head_link}")
        lines.append("")

    # Summary table
    lines.append("| Metric | Value |")
    lines.append("|---|---|")
    lines.append(f"| Benchmarks compared | {total} |")
    lines.append(f"| Regressed (>{REGRESSION_THRESHOLD:.0f}% slower) | {len(regressions)} |")
    lines.append(f"| Improved (>{-IMPROVEMENT_THRESHOLD:.0f}% faster) | {len(improvements)} |")
    lines.append(f"| Unchanged | {len(unchanged)} |")
    if added:
        lines.append(f"| Added (new benchmarks) | {len(added)} |")
    if removed:
        lines.append(f"| Removed (deleted benchmarks) | {len(removed)} |")

    # Table headers depend on whether CLIPS data is available
    if has_clips:
        hdr = "| Benchmark | Base | Head | Delta | CLIPS Ref | vs CLIPS |"
        sep = "|---|---:|---:|---|---:|---:|"
    else:
        hdr = "| Benchmark | Base | Head | Delta |"
        sep = "|---|---:|---:|---|"

    def _row(name, b_ns, h_ns, pct, bold_delta=False):
        delta = f"**{fmt_delta(pct)}**" if bold_delta else fmt_delta(pct)
        base_cols = f"| {name} | {fmt_ns(b_ns)} | {fmt_ns(h_ns)} | {delta}"
        if has_clips:
            c = clips.get(name)
            c_ns = c.get("median_ns") if c else None
            return f"{base_cols} | {fmt_ns(c_ns)} | {fmt_ratio(h_ns, c_ns)} |"
        return f"{base_cols} |"

    # Regressions
    lines.append("")
    if regressions:
        lines.append(f"### Regressions ({len(regressions)})")
        lines.append("")
        lines.append(hdr)
        lines.append(sep)
        for name, suite, b_ns, h_ns, pct in regressions:
            lines.append(_row(name, b_ns, h_ns, pct, bold_delta=True))
    else:
        lines.append("### Regressions")
        lines.append("")
        lines.append("None")

    # Improvements
    lines.append("")
    if improvements:
        lines.append(f"### Improvements ({len(improvements)})")
        lines.append("")
        lines.append(hdr)
        lines.append(sep)
        for name, suite, b_ns, h_ns, pct in improvements:
            lines.append(_row(name, b_ns, h_ns, pct, bold_delta=True))
    else:
        lines.append("### Improvements")
        lines.append("")
        lines.append("None")

    # Unchanged (in collapsible details)
    if unchanged:
        lines.append("")
        lines.append(f"<details><summary>Unchanged ({len(unchanged)})</summary>")
        lines.append("")
        lines.append(hdr)
        lines.append(sep)
        for name, suite, b_ns, h_ns, pct in unchanged:
            lines.append(_row(name, b_ns, h_ns, pct))
        lines.append("")
        lines.append("</details>")

    # Added / Removed
    if added:
        lines.append("")
        lines.append(f"<details><summary>Added benchmarks ({len(added)})</summary>")
        lines.append("")
        for name, suite, _, h_ns, _ in added:
            lines.append(f"- `{name}` ({suite}): {fmt_ns(h_ns)}")
        lines.append("")
        lines.append("</details>")

    if removed:
        lines.append("")
        lines.append(f"<details><summary>Removed benchmarks ({len(removed)})</summary>")
        lines.append("")
        for name, suite, b_ns, _, _ in removed:
            lines.append(f"- `{name}` ({suite}): was {fmt_ns(b_ns)}")
        lines.append("")
        lines.append("</details>")

    # CLIPS methodology note
    if has_clips:
        lines.append("")
        lines.append(f"> {clips_reference_note(clips_section)} "
                     "The \"vs CLIPS\" ratio is ferric/CLIPS — useful for tracking "
                     "relative trends, not absolute speed comparison.")

    return lines


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main():
    parser = argparse.ArgumentParser(description="Compare two perf manifests")
    parser.add_argument("base_manifest", help="Base manifest JSON")
    parser.add_argument("head_manifest", help="Head manifest JSON")
    parser.add_argument("--report", metavar="FILE",
                        help="Write self-contained Markdown report")
    parser.add_argument("--repo", metavar="OWNER/REPO",
                        help="GitHub repository for commit links")
    parser.add_argument("--base-sha", metavar="SHA", help="Base commit SHA")
    parser.add_argument("--head-sha", metavar="SHA", help="Head commit SHA")
    parser.add_argument("--fail-on-regression", action="store_true",
                        help="Exit with code 1 if regressions are detected")

    args = parser.parse_args()

    base = load_manifest(args.base_manifest)
    head = load_manifest(args.head_manifest)

    regressions, improvements, unchanged, added, removed = compute_diff(base, head)

    # Extract CLIPS reference from head manifest (if available)
    clips_ref = head.get("clips_reference")

    # Always write comparison to stdout (for $GITHUB_STEP_SUMMARY)
    md_lines = format_markdown(
        regressions, improvements, unchanged, added, removed,
        repo=args.repo, base_sha=args.base_sha, head_sha=args.head_sha,
        clips_ref=clips_ref,
    )
    print("\n".join(md_lines))

    # Optional: write to file
    if args.report:
        with open(args.report, "w", encoding="utf-8") as f:
            f.write("\n".join(md_lines))
            f.write("\n")

    # Optionally exit with code 1 if there are regressions
    if args.fail_on_regression and regressions:
        sys.exit(1)


if __name__ == "__main__":
    main()
