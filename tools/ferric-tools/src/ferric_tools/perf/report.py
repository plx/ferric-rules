"""Generate a standalone performance report from a perf manifest."""

from __future__ import annotations

import re
from pathlib import Path
from typing import Annotated

import typer
from rich.console import Console

from ferric_tools._formatting import fmt_ns_unicode
from ferric_tools._manifest import load_manifest
from ferric_tools._paths import repo_root

app = typer.Typer(help="Generate performance assessment report.")


def _natural_sort_key(s: str) -> list:
    """Sort key that orders embedded numbers numerically (natural sort)."""
    return [int(c) if c.isdigit() else c.lower() for c in re.split(r"(\d+)", s)]


console = Console(stderr=True)


def _fmt_ratio(ferric_ns: float | None, clips_ns: float | None) -> str:
    if ferric_ns is None or clips_ns is None or clips_ns == 0:
        return "n/a"
    return f"{ferric_ns / clips_ns:.3f}x"


def _clips_reference_label(clips_ref: dict | None) -> str:
    method = (clips_ref or {}).get("methodology")
    if method == "native_wall_clock_launch_adjusted":
        return "native wall-clock, launch-adjusted"
    if method == "docker_wall_clock_launch_adjusted":
        return "Docker wall-clock, launch-adjusted"
    runner = (clips_ref or {}).get("runner")
    if runner == "native":
        return "native wall-clock"
    if runner == "docker":
        return "Docker wall-clock"
    return "external reference"


def _clips_reference_note(clips_ref: dict | None) -> str:
    method = (clips_ref or {}).get("methodology")
    if method == "native_wall_clock_launch_adjusted":
        return (
            "Wall-clock times from the native CLIPS binary on the runner host, "
            "with a matched launch-only invocation subtracted from each sample."
        )
    if method == "docker_wall_clock_launch_adjusted":
        return (
            "Wall-clock times from CLIPS in the reference Docker image, with a "
            "matched launch-only container invocation subtracted from each sample."
        )
    runner = (clips_ref or {}).get("runner")
    if runner == "native":
        return "Wall-clock times from the native CLIPS binary on the runner host."
    if runner == "docker":
        return "Wall-clock times from CLIPS via Docker."
    return "Wall-clock times from an external CLIPS reference runner."


def print_summary(manifest: dict) -> None:
    summary = manifest["summary"]
    generated = manifest.get("generated", "unknown")
    commit_sha = manifest.get("commit_sha", "")
    settings = manifest.get("settings", {})

    print("Performance Assessment Report")
    print("=" * 40)
    print(f"Generated: {generated}")
    if commit_sha:
        print(f"Commit: {commit_sha[:10]}")
    print(
        f"Settings: sample_size={settings.get('sample_size', '?')}, "
        f"warm_up={settings.get('warm_up_time_s', '?')}s, "
        f"measurement={settings.get('measurement_time_s', '?')}s"
    )
    print()

    print(
        f"Benchmarks: {summary['collected']}/{summary['total_benchmarks']} collected"
        f" ({summary['missing']} missing)"
    )
    print(f"Suites: {', '.join(summary.get('suites', []))}")
    print()

    suite_stats: dict[str, dict] = {}
    for _name, info in manifest.get("benchmarks", {}).items():
        suite = info["suite"]
        if suite not in suite_stats:
            suite_stats[suite] = {"total": 0, "collected": 0}
        suite_stats[suite]["total"] += 1
        if info.get("median_ns") is not None:
            suite_stats[suite]["collected"] += 1

    print(f"{'Suite':<20s} {'Collected':>10s} {'Total':>6s}")
    print(f"{'-' * 20} {'-' * 10} {'-' * 6}")
    for suite in sorted(suite_stats, key=_natural_sort_key):
        s = suite_stats[suite]
        print(f"{suite:<20s} {s['collected']:>10d} {s['total']:>6d}")

    print()
    print(f"{'Benchmark':<28s} {'Median':>12s} {'Mean':>12s} {'Std Dev':>12s}")
    print(f"{'-' * 28} {'-' * 12} {'-' * 12} {'-' * 12}")
    for name, info in manifest.get("benchmarks", {}).items():
        median = fmt_ns_unicode(info.get("median_ns"))
        mean = fmt_ns_unicode(info.get("mean_ns"))
        stddev = fmt_ns_unicode(info.get("stddev_ns"))
        print(f"{name:<28s} {median:>12s} {mean:>12s} {stddev:>12s}")

    clips_ref = manifest.get("clips_reference")
    if clips_ref and clips_ref.get("benchmarks"):
        clips_benchmarks = clips_ref["benchmarks"]
        print()
        print(f"CLIPS Reference ({_clips_reference_label(clips_ref)})")
        print(f"{'-' * 70}")
        print(_clips_reference_note(clips_ref))
        print(f"{'Benchmark':<28s} {'ferric':>12s} {'CLIPS':>12s} {'Ratio':>10s}")
        print(f"{'-' * 28} {'-' * 12} {'-' * 12} {'-' * 10}")
        for name, clips_info in clips_benchmarks.items():
            if clips_info is None:
                continue
            ferric_info = manifest.get("benchmarks", {}).get(name, {})
            ferric_ns = ferric_info.get("median_ns")
            clips_ns = clips_info.get("median_ns")
            ratio = _fmt_ratio(ferric_ns, clips_ns)
            print(
                f"{name:<28s} {fmt_ns_unicode(ferric_ns):>12s} "
                f"{fmt_ns_unicode(clips_ns):>12s} {ratio:>10s}"
            )


def write_report(
    manifest: dict, report_path: str, repo: str | None = None, commit_sha: str | None = None
) -> None:
    summary = manifest["summary"]
    generated = manifest.get("generated", "unknown")
    settings = manifest.get("settings", {})

    lines: list[str] = []
    lines.append("## Performance Report")
    lines.append("")
    lines.append(
        f"Criterion benchmark results across {summary['total_benchmarks']} benchmarks "
        f"in {len(summary.get('suites', []))} suites."
    )
    lines.append(
        f"Settings: sample\\_size={settings.get('sample_size', '?')}, "
        f"warm\\_up={settings.get('warm_up_time_s', '?')}s, "
        f"measurement={settings.get('measurement_time_s', '?')}s"
    )
    lines.append("")

    sha = commit_sha or manifest.get("commit_sha", "")
    if repo and sha:
        commit_link = f"[`{sha[:10]}`](https://github.com/{repo}/commit/{sha})"
        lines.append(f"Commit: {commit_link} | Generated: {generated}")
    else:
        lines.append(f"Generated: {generated}")
    lines.append("")

    suite_stats: dict[str, dict] = {}
    for _name, info in manifest.get("benchmarks", {}).items():
        suite = info["suite"]
        if suite not in suite_stats:
            suite_stats[suite] = {"total": 0, "collected": 0}
        suite_stats[suite]["total"] += 1
        if info.get("median_ns") is not None:
            suite_stats[suite]["collected"] += 1

    lines.append("### Summary")
    lines.append("")
    lines.append("| Suite | Benchmarks | Status |")
    lines.append("|---|---:|---|")
    for suite in sorted(suite_stats, key=_natural_sort_key):
        s = suite_stats[suite]
        lines.append(f"| {suite} | {s['total']} | {s['collected']}/{s['total']} collected |")
    lines.append(
        f"| **total** | **{summary['total_benchmarks']}** | "
        f"**{summary['collected']}/{summary['total_benchmarks']}** |"
    )

    lines.append("")
    lines.append("### Results")
    lines.append("")
    lines.append("| Benchmark | Suite | Median | Mean | Std Dev |")
    lines.append("|---|---|---:|---:|---:|")
    for name, info in manifest.get("benchmarks", {}).items():
        suite = info["suite"]
        median = fmt_ns_unicode(info.get("median_ns"))
        mean = fmt_ns_unicode(info.get("mean_ns"))
        stddev = fmt_ns_unicode(info.get("stddev_ns"))
        lines.append(f"| {name} | {suite} | {median} | {mean} | {stddev} |")

    clips_ref = manifest.get("clips_reference")
    if clips_ref and clips_ref.get("benchmarks"):
        clips_benchmarks = clips_ref["benchmarks"]
        has_data = any(v is not None for v in clips_benchmarks.values())
        if has_data:
            lines.append("")
            lines.append("### CLIPS Reference")
            lines.append("")
            lines.append(_clips_reference_note(clips_ref))
            lines.append(
                "Useful as a relative frame of reference, not for absolute speed comparison."
            )
            lines.append("")
            lines.append("| Benchmark | ferric (median) | CLIPS (median) | ferric / CLIPS |")
            lines.append("|---|---:|---:|---:|")
            for name, clips_info in clips_benchmarks.items():
                if clips_info is None:
                    continue
                ferric_info = manifest.get("benchmarks", {}).get(name, {})
                ferric_ns = ferric_info.get("median_ns")
                clips_ns = clips_info.get("median_ns")
                ratio = _fmt_ratio(ferric_ns, clips_ns)
                lines.append(
                    f"| {name} | {fmt_ns_unicode(ferric_ns)} | "
                    f"{fmt_ns_unicode(clips_ns)} | {ratio} |"
                )

    with open(report_path, "w", encoding="utf-8") as f:
        f.write("\n".join(lines))
        f.write("\n")

    print(f"Report written to {report_path}")


@app.command()
def main(
    manifest_opt: Annotated[
        str | None, typer.Option("--manifest", help="Path to perf-manifest.json")
    ] = None,
    report: Annotated[str | None, typer.Option(help="Write self-contained Markdown report")] = None,
    repo: Annotated[str | None, typer.Option(help="GitHub repository for commit links")] = None,
    commit_sha: Annotated[str | None, typer.Option(help="Commit SHA for report links")] = None,
) -> None:
    """Generate performance assessment report."""
    root = repo_root()
    manifest_path = Path(manifest_opt) if manifest_opt else root / "target" / "perf-manifest.json"

    if not manifest_path.exists():
        console.print(f"[red]error:[/] manifest not found: {manifest_path}")
        console.print("Run ferric-perf-collect first.")
        raise typer.Exit(1)

    mdata = load_manifest(manifest_path)
    print_summary(mdata)

    if report:
        print()
        write_report(mdata, report, repo=repo, commit_sha=commit_sha)


if __name__ == "__main__":
    app()
