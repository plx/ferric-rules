"""Report generator for CLIPS compatibility assessment."""

from __future__ import annotations

import csv
import os
from pathlib import Path
from typing import Annotated

import typer
from rich.console import Console

from ferric_tools._manifest import load_manifest
from ferric_tools._paths import examples_dir as default_examples_dir

app = typer.Typer(help="Generate compatibility assessment reports.")
console = Console(stderr=True)


def print_summary(manifest: dict) -> None:
    """Print summary to stdout."""
    summary = manifest["summary"]
    total = summary["total"]
    generated = manifest.get("generated", "unknown")

    print("CLIPS Compatibility Assessment Report")
    print("=" * 40)
    print(f"Generated: {generated}")
    print()

    print(f"Total files: {total:,}")
    print()
    print("Classification:")
    for cls in ["equivalent", "divergent", "incompatible", "pending"]:
        count = summary.get(cls, 0)
        pct = (count / total * 100) if total else 0
        print(f"  {cls:15s}: {count:6,} ({pct:5.1f}%)")

    reason_counts: dict[str, int] = {}
    for info in manifest["files"].values():
        if info["classification"] == "incompatible":
            reason = info["reason"]
            reason_counts[reason] = reason_counts.get(reason, 0) + 1

    if reason_counts:
        print()
        print("Incompatible breakdown:")
        for reason, count in sorted(reason_counts.items(), key=lambda x: -x[1]):
            print(f"  {reason:25s}: {count:6,}")

    source_stats: dict[str, dict] = {}
    for info in manifest["files"].values():
        src = info["source"] or "(root)"
        if src not in source_stats:
            source_stats[src] = {
                "equivalent": 0,
                "divergent": 0,
                "incompatible": 0,
                "pending": 0,
                "total": 0,
            }
        source_stats[src]["total"] += 1
        cls = info["classification"]
        if cls in source_stats[src]:
            source_stats[src][cls] += 1

    print()
    print("By source:")
    header = (
        f"  {'Source':<30s} {'Total':>6s} {'Equiv':>6s} {'Diverg':>6s}"
        f" {'Incompat':>8s} {'Pending':>7s}"
    )
    print(header)
    print(f"  {'-' * 30} {'-' * 6} {'-' * 6} {'-' * 6} {'-' * 8} {'-' * 7}")
    for src in sorted(source_stats, key=lambda s: -source_stats[s]["total"]):
        s = source_stats[src]
        print(
            f"  {src:<30s} {s['total']:6,} {s['equivalent']:6,} "
            f"{s['divergent']:6,} {s['incompatible']:8,} {s['pending']:7,}"
        )

    equivalent = [
        (k, v) for k, v in manifest["files"].items() if v["classification"] == "equivalent"
    ]
    if equivalent:
        print()
        print(f"Equivalent files ({len(equivalent)}):")
        for k, v in sorted(equivalent):
            print(f"  {k} ({v['reason']})")

    divergent = [(k, v) for k, v in manifest["files"].items() if v["classification"] == "divergent"]
    if divergent:
        print()
        print(f"Divergent files ({len(divergent)}):")
        for k, v in sorted(divergent):
            print(f"  {k} ({v['reason']})")
            if v["reason"] == "output-mismatch":
                f_out = (v.get("ferric") or {}).get("stdout", "")[:120]
                c_out = (v.get("clips") or {}).get("stdout", "")[:120]
                if f_out or c_out:
                    print(f"    ferric: {f_out!r}")
                    print(f"    clips:  {c_out!r}")


_FIELDNAMES = [
    "path",
    "source",
    "classification",
    "reason",
    "runability",
    "features",
    "unsupported_features",
    "ferric_exit",
    "ferric_duration_ms",
    "ferric_timed_out",
    "clips_exit",
    "clips_duration_ms",
    "clips_timed_out",
    "notes",
]


def _write_delimited(manifest: dict, out_path: str, delimiter: str) -> None:
    """Write CSV or TSV export."""
    with open(out_path, "w", newline="", encoding="utf-8") as f:
        writer = csv.DictWriter(f, fieldnames=_FIELDNAMES, delimiter=delimiter)
        writer.writeheader()
        for path, info in sorted(manifest["files"].items()):
            ferric = info.get("ferric") or {}
            clips = info.get("clips") or {}
            writer.writerow(
                {
                    "path": path,
                    "source": info["source"],
                    "classification": info["classification"],
                    "reason": info["reason"],
                    "runability": info["runability"],
                    "features": ";".join(info.get("features", [])),
                    "unsupported_features": ";".join(info.get("unsupported_features", [])),
                    "ferric_exit": ferric.get("exit_code", ""),
                    "ferric_duration_ms": ferric.get("duration_ms", ""),
                    "ferric_timed_out": ferric.get("timed_out", ""),
                    "clips_exit": clips.get("exit_code", ""),
                    "clips_duration_ms": clips.get("duration_ms", ""),
                    "clips_timed_out": clips.get("timed_out", ""),
                    "notes": info.get("notes", ""),
                }
            )
    print(f"{'TSV' if delimiter == '\t' else 'CSV'} written to {out_path}")


def write_report(
    manifest: dict, report_path: str, repo: str | None = None, commit_sha: str | None = None
) -> None:
    """Write a self-contained Markdown report."""
    summary = manifest["summary"]
    total = summary["total"]
    generated = manifest.get("generated", "unknown")

    lines: list[str] = []
    lines.append("## CLIPS Compatibility Report")
    lines.append("")
    lines.append("Assessment of ferric's compatibility with CLIPS across a corpus of")
    lines.append("example `.clp` files. Each file is classified as **equivalent**")
    lines.append("(output matches CLIPS), **divergent** (runs but output differs),")
    lines.append("**incompatible** (cannot run), or **pending** (not yet tested).")
    lines.append("")

    if repo and commit_sha:
        commit_link = f"[`{commit_sha[:10]}`](https://github.com/{repo}/commit/{commit_sha})"
        lines.append(f"Commit: {commit_link} | Generated: {generated}")
    else:
        lines.append(f"Generated: {generated}")
    lines.append("")

    lines.append("| Classification | Count | % |")
    lines.append("|---|---:|---:|")
    for cls in ["equivalent", "divergent", "incompatible", "pending"]:
        count = summary.get(cls, 0)
        pct = (count / total * 100) if total else 0
        lines.append(f"| {cls} | {count:,} | {pct:.1f}% |")
    lines.append(f"| **total** | **{total:,}** | |")

    source_stats: dict[str, dict] = {}
    for info in manifest["files"].values():
        src = info["source"] or "(root)"
        if src not in source_stats:
            source_stats[src] = {
                "equivalent": 0,
                "divergent": 0,
                "incompatible": 0,
                "pending": 0,
                "total": 0,
            }
        source_stats[src]["total"] += 1
        cls = info["classification"]
        if cls in source_stats[src]:
            source_stats[src][cls] += 1

    lines.append("")
    lines.append("### By source")
    lines.append("")
    lines.append("| Source | Total | Equiv | Diverg | Incompat | Pending |")
    lines.append("|---|---:|---:|---:|---:|---:|")
    for src in sorted(source_stats, key=lambda s: -source_stats[s]["total"]):
        s = source_stats[src]
        lines.append(
            f"| {src} | {s['total']:,} | {s['equivalent']:,} | "
            f"{s['divergent']:,} | {s['incompatible']:,} | {s['pending']:,} |"
        )

    equivalent = sorted(
        (k, v) for k, v in manifest["files"].items() if v["classification"] == "equivalent"
    )
    if equivalent:
        lines.append("")
        lines.append(f"### Equivalent files ({len(equivalent)})")
        lines.append("")
        for k, v in equivalent:
            lines.append(f"- `{k}` ({v['reason']})")

    divergent = sorted(
        (k, v) for k, v in manifest["files"].items() if v["classification"] == "divergent"
    )
    if divergent:
        lines.append("")
        lines.append(f"### Divergent files ({len(divergent)})")
        lines.append("")
        for k, v in divergent:
            lines.append(f"- `{k}` ({v['reason']})")

    with open(report_path, "w", encoding="utf-8") as f:
        f.write("\n".join(lines))
        f.write("\n")

    print(f"Report written to {report_path}")


def create_symlinks(manifest: dict, symlinks_dir: str, examples_path: Path) -> None:
    """Create a symlink directory view."""
    import shutil

    symlinks_path = Path(symlinks_dir)
    if symlinks_path.exists():
        shutil.rmtree(symlinks_path)

    created = 0
    for rel_path, info in sorted(manifest["files"].items()):
        cls = info["classification"]
        if cls in ("equivalent", "divergent", "incompatible", "pending"):
            dest_dir = symlinks_path / cls
        else:
            dest_dir = symlinks_path / "other"

        dest = dest_dir / rel_path
        dest.parent.mkdir(parents=True, exist_ok=True)

        actual = examples_path / rel_path
        if actual.exists():
            rel_target = os.path.relpath(actual, dest.parent)
            dest.symlink_to(rel_target)
            created += 1

    print(f"Symlink view created at {symlinks_dir} ({created:,} links)")


@app.command()
def main(
    manifest_opt: Annotated[
        Path | None, typer.Option("--manifest", help="Path to manifest file")
    ] = None,
    csv_path: Annotated[str | None, typer.Option("--csv", help="Export as CSV")] = None,
    tsv_path: Annotated[str | None, typer.Option("--tsv", help="Export as TSV")] = None,
    report: Annotated[str | None, typer.Option(help="Write self-contained Markdown report")] = None,
    repo: Annotated[str | None, typer.Option(help="GitHub repository for commit links")] = None,
    commit_sha: Annotated[str | None, typer.Option(help="Commit SHA for report links")] = None,
    symlinks: Annotated[str | None, typer.Option(help="Create symlink directory view")] = None,
) -> None:
    """Generate compatibility assessment reports."""
    ed = default_examples_dir()
    manifest_path = Path(manifest_opt) if manifest_opt else ed / "compat-manifest.json"

    if not manifest_path.exists():
        console.print(f"[red]error:[/] manifest not found: {manifest_path}")
        console.print("Run ferric-compat-scan first.")
        raise typer.Exit(1)

    mdata = load_manifest(manifest_path)
    print_summary(mdata)

    if csv_path:
        print()
        _write_delimited(mdata, csv_path, ",")

    if tsv_path:
        print()
        _write_delimited(mdata, tsv_path, "\t")

    if report:
        print()
        write_report(mdata, report, repo=repo, commit_sha=commit_sha)

    if symlinks:
        print()
        create_symlinks(mdata, symlinks, ed)


if __name__ == "__main__":
    app()
