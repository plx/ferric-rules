"""Compare two performance manifests and produce a Markdown summary."""

from __future__ import annotations

import re
from typing import Annotated

import typer
from rich.console import Console

from ferric_tools._formatting import fmt_ns_unicode
from ferric_tools._manifest import load_manifest

app = typer.Typer(help="Compare two perf manifests.")
console = Console(stderr=True)

REGRESSION_THRESHOLD = 5.0
IMPROVEMENT_THRESHOLD = -5.0


def _natural_sort_key(s: str) -> list:
    """Sort key that orders embedded numbers numerically (natural sort)."""
    return [int(c) if c.isdigit() else c.lower() for c in re.split(r"(\d+)", s)]


def _fmt_delta(pct: float | None) -> str:
    if pct is None:
        return "n/a"
    sign = "+" if pct >= 0 else ""
    return f"{sign}{pct:.1f}%"


def _fmt_ratio(ferric_ns: float | None, clips_ns: float | None) -> str:
    if ferric_ns is None or clips_ns is None or clips_ns == 0:
        return "n/a"
    return f"{ferric_ns / clips_ns:.3f}x"


def _clips_reference_note(clips_ref: dict | None) -> str:
    method = (clips_ref or {}).get("methodology")
    if method == "native_wall_clock_launch_adjusted":
        return (
            "CLIPS reference: native wall-clock on the runner host, with a "
            "matched launch-only invocation subtracted from each sample."
        )
    if method == "docker_wall_clock_launch_adjusted":
        return (
            "CLIPS reference: Docker wall-clock, with a matched launch-only "
            "container invocation subtracted from each sample."
        )
    runner = (clips_ref or {}).get("runner")
    if runner == "native":
        return "CLIPS reference: native wall-clock on the runner host."
    if runner == "docker":
        return "CLIPS reference: Docker wall-clock."
    return "CLIPS reference: external wall-clock timings."


def compute_diff(base: dict, head: dict) -> tuple[list, list, list, list, list]:
    """Compare benchmarks between base and head manifests.

    Returns (regressions, improvements, unchanged, added, removed).
    Each entry is (name, suite, base_median_ns, head_median_ns, delta_pct).
    """
    base_benchmarks = base.get("benchmarks", {})
    head_benchmarks = head.get("benchmarks", {})

    regressions: list[tuple] = []
    improvements: list[tuple] = []
    unchanged: list[tuple] = []
    added: list[tuple] = []
    removed: list[tuple] = []

    all_names = sorted(set(base_benchmarks) | set(head_benchmarks), key=_natural_sort_key)

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

        if b_median is None and h_median is not None:
            added.append((name, suite, None, h_median, None))
            continue
        if b_median is not None and h_median is None:
            removed.append((name, suite, b_median, None, None))
            continue
        if b_median is None:
            unchanged.append((name, suite, b_median, h_median, None))
            continue

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

    regressions.sort(key=lambda e: -(e[4] or 0))
    improvements.sort(key=lambda e: e[4] or 0)

    return regressions, improvements, unchanged, added, removed


def format_markdown(
    regressions: list,
    improvements: list,
    unchanged: list,
    added: list,
    removed: list,
    *,
    repo: str | None = None,
    base_sha: str | None = None,
    head_sha: str | None = None,
    clips_ref: dict | None = None,
) -> list[str]:
    lines: list[str] = []
    clips_section = clips_ref or {}
    clips = clips_section.get("benchmarks") or {}
    has_clips = bool(clips)

    total = len(regressions) + len(improvements) + len(unchanged)

    lines.append("## Performance Report")
    lines.append("")
    lines.append(
        f"Compares Criterion benchmark performance between two commits across {total} benchmarks."
    )
    lines.append(
        f"Regressions: >+{REGRESSION_THRESHOLD:.0f}% slower. "
        f"Improvements: >{-IMPROVEMENT_THRESHOLD:.0f}% faster."
    )
    lines.append("")

    if repo and base_sha and head_sha:
        base_link = f"[`{base_sha[:10]}`](https://github.com/{repo}/commit/{base_sha})"
        head_link = f"[`{head_sha[:10]}`](https://github.com/{repo}/commit/{head_sha})"
        lines.append(f"Base: {base_link} | Head: {head_link}")
        lines.append("")

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

    if has_clips:
        hdr = "| Benchmark | Base | Head | Delta | CLIPS Ref | vs CLIPS |"
        sep = "|---|---:|---:|---|---:|---:|"
    else:
        hdr = "| Benchmark | Base | Head | Delta |"
        sep = "|---|---:|---:|---|"

    def _row(
        name: str,
        b_ns: float | None,
        h_ns: float | None,
        pct: float | None,
        bold_delta: bool = False,
    ) -> str:
        delta = f"**{_fmt_delta(pct)}**" if bold_delta else _fmt_delta(pct)
        base_cols = f"| {name} | {fmt_ns_unicode(b_ns)} | {fmt_ns_unicode(h_ns)} | {delta}"
        if has_clips:
            c = clips.get(name)
            c_ns = c.get("median_ns") if c else None
            return f"{base_cols} | {fmt_ns_unicode(c_ns)} | {_fmt_ratio(h_ns, c_ns)} |"
        return f"{base_cols} |"

    lines.append("")
    if regressions:
        lines.append(f"### Regressions ({len(regressions)})")
        lines.append("")
        lines.append(hdr)
        lines.append(sep)
        for name, _suite, b_ns, h_ns, pct in regressions:
            lines.append(_row(name, b_ns, h_ns, pct, bold_delta=True))
    else:
        lines.append("### Regressions")
        lines.append("")
        lines.append("None")

    lines.append("")
    if improvements:
        lines.append(f"### Improvements ({len(improvements)})")
        lines.append("")
        lines.append(hdr)
        lines.append(sep)
        for name, _suite, b_ns, h_ns, pct in improvements:
            lines.append(_row(name, b_ns, h_ns, pct, bold_delta=True))
    else:
        lines.append("### Improvements")
        lines.append("")
        lines.append("None")

    if unchanged:
        lines.append("")
        lines.append(f"<details><summary>Unchanged ({len(unchanged)})</summary>")
        lines.append("")
        lines.append(hdr)
        lines.append(sep)
        for name, _suite, b_ns, h_ns, pct in unchanged:
            lines.append(_row(name, b_ns, h_ns, pct))
        lines.append("")
        lines.append("</details>")

    if added:
        lines.append("")
        lines.append(f"<details><summary>Added benchmarks ({len(added)})</summary>")
        lines.append("")
        for name, suite, _, h_ns, _ in added:
            lines.append(f"- `{name}` ({suite}): {fmt_ns_unicode(h_ns)}")
        lines.append("")
        lines.append("</details>")

    if removed:
        lines.append("")
        lines.append(f"<details><summary>Removed benchmarks ({len(removed)})</summary>")
        lines.append("")
        for name, suite, b_ns, _, _ in removed:
            lines.append(f"- `{name}` ({suite}): was {fmt_ns_unicode(b_ns)}")
        lines.append("")
        lines.append("</details>")

    if has_clips:
        lines.append("")
        lines.append(
            f"> {_clips_reference_note(clips_section)} "
            'The "vs CLIPS" ratio is ferric/CLIPS \u2014 useful for tracking '
            "relative trends, not absolute speed comparison."
        )

    return lines


@app.command()
def main(
    base_manifest: Annotated[str, typer.Argument(help="Base manifest JSON")],
    head_manifest: Annotated[str, typer.Argument(help="Head manifest JSON")],
    report: Annotated[str | None, typer.Option(help="Write self-contained Markdown report")] = None,
    repo: Annotated[str | None, typer.Option(help="GitHub repository for commit links")] = None,
    base_sha: Annotated[str | None, typer.Option(help="Base commit SHA")] = None,
    head_sha: Annotated[str | None, typer.Option(help="Head commit SHA")] = None,
    fail_on_regression: Annotated[
        bool, typer.Option(help="Exit with code 1 if regressions detected")
    ] = False,
) -> None:
    """Compare two perf manifests."""
    base = load_manifest(base_manifest)
    head = load_manifest(head_manifest)

    regressions, improvements, unchanged, added, removed = compute_diff(base, head)
    clips_ref = head.get("clips_reference")

    md_lines = format_markdown(
        regressions,
        improvements,
        unchanged,
        added,
        removed,
        repo=repo,
        base_sha=base_sha,
        head_sha=head_sha,
        clips_ref=clips_ref,
    )
    print("\n".join(md_lines))

    if report:
        with open(report, "w", encoding="utf-8") as f:
            f.write("\n".join(md_lines))
            f.write("\n")

    if fail_on_regression and regressions:
        raise typer.Exit(1)


if __name__ == "__main__":
    app()
