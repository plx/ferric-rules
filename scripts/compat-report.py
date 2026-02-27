#!/usr/bin/env python3
"""Report generator for CLIPS compatibility assessment.

Reads the manifest produced by compat-scan.py / compat-run.py and
generates human-readable reports, CSV/TSV exports, and optional symlink views.

Usage:
    python scripts/compat-report.py [--manifest FILE] [--csv FILE] [--tsv FILE]
                                    [--report FILE] [--symlinks DIR]
                                    [--repo OWNER/REPO] [--commit-sha SHA]
"""

import argparse
import csv
import json
import os
import sys
from pathlib import Path


def load_manifest(path):
    with open(path, "r", encoding="utf-8") as f:
        return json.load(f)


# ---------------------------------------------------------------------------
# Summary report (stdout)
# ---------------------------------------------------------------------------

def print_summary(manifest):
    summary = manifest["summary"]
    total = summary["total"]
    generated = manifest.get("generated", "unknown")

    print("CLIPS Compatibility Assessment Report")
    print("=" * 40)
    print(f"Generated: {generated}")
    print()

    # Classification summary
    print(f"Total files: {total:,}")
    print()
    print("Classification:")
    for cls in ["equivalent", "divergent", "incompatible", "pending"]:
        count = summary.get(cls, 0)
        pct = (count / total * 100) if total else 0
        print(f"  {cls:15s}: {count:6,} ({pct:5.1f}%)")

    # Incompatible breakdown by reason
    reason_counts = {}
    for info in manifest["files"].values():
        if info["classification"] == "incompatible":
            reason = info["reason"]
            reason_counts[reason] = reason_counts.get(reason, 0) + 1

    if reason_counts:
        print()
        print("Incompatible breakdown:")
        for reason, count in sorted(reason_counts.items(), key=lambda x: -x[1]):
            print(f"  {reason:25s}: {count:6,}")

    # By source
    source_stats = {}
    for info in manifest["files"].values():
        src = info["source"] or "(root)"
        if src not in source_stats:
            source_stats[src] = {"equivalent": 0, "divergent": 0, "incompatible": 0, "pending": 0, "total": 0}
        source_stats[src]["total"] += 1
        cls = info["classification"]
        if cls in source_stats[src]:
            source_stats[src][cls] += 1

    print()
    print("By source:")
    print(f"  {'Source':<30s} {'Total':>6s} {'Equiv':>6s} {'Diverg':>6s} {'Incompat':>8s} {'Pending':>7s}")
    print(f"  {'-'*30} {'-'*6} {'-'*6} {'-'*6} {'-'*8} {'-'*7}")
    for src in sorted(source_stats, key=lambda s: -source_stats[s]["total"]):
        s = source_stats[src]
        print(
            f"  {src:<30s} {s['total']:6,} {s['equivalent']:6,} "
            f"{s['divergent']:6,} {s['incompatible']:8,} {s['pending']:7,}"
        )

    # Equivalent files
    equivalent = [
        (k, v) for k, v in manifest["files"].items()
        if v["classification"] == "equivalent"
    ]
    if equivalent:
        print()
        print(f"Equivalent files ({len(equivalent)}):")
        for k, v in sorted(equivalent):
            print(f"  {k} ({v['reason']})")

    # Divergent files
    divergent = [
        (k, v) for k, v in manifest["files"].items()
        if v["classification"] == "divergent"
    ]
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


# ---------------------------------------------------------------------------
# CSV export
# ---------------------------------------------------------------------------

def write_csv(manifest, csv_path):
    fieldnames = [
        "path", "source", "classification", "reason", "runability",
        "features", "unsupported_features",
        "ferric_exit", "ferric_duration_ms", "ferric_timed_out",
        "clips_exit", "clips_duration_ms", "clips_timed_out",
        "notes",
    ]

    with open(csv_path, "w", newline="", encoding="utf-8") as f:
        writer = csv.DictWriter(f, fieldnames=fieldnames)
        writer.writeheader()

        for path, info in sorted(manifest["files"].items()):
            ferric = info.get("ferric") or {}
            clips = info.get("clips") or {}
            writer.writerow({
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
            })

    print(f"CSV written to {csv_path}")


def write_tsv(manifest, tsv_path):
    fieldnames = [
        "path", "source", "classification", "reason", "runability",
        "features", "unsupported_features",
        "ferric_exit", "ferric_duration_ms", "ferric_timed_out",
        "clips_exit", "clips_duration_ms", "clips_timed_out",
        "notes",
    ]

    with open(tsv_path, "w", newline="", encoding="utf-8") as f:
        writer = csv.DictWriter(f, fieldnames=fieldnames, delimiter="\t")
        writer.writeheader()

        for path, info in sorted(manifest["files"].items()):
            ferric = info.get("ferric") or {}
            clips = info.get("clips") or {}
            writer.writerow({
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
            })

    print(f"TSV written to {tsv_path}")


# ---------------------------------------------------------------------------
# Markdown report (self-contained file)
# ---------------------------------------------------------------------------

def write_report(manifest, report_path, repo=None, commit_sha=None):
    summary = manifest["summary"]
    total = summary["total"]
    generated = manifest.get("generated", "unknown")

    lines = []
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

    lines.append(f"| Classification | Count | % |")
    lines.append("|---|---:|---:|")
    for cls in ["equivalent", "divergent", "incompatible", "pending"]:
        count = summary.get(cls, 0)
        pct = (count / total * 100) if total else 0
        lines.append(f"| {cls} | {count:,} | {pct:.1f}% |")
    lines.append(f"| **total** | **{total:,}** | |")

    # By-source breakdown
    source_stats = {}
    for info in manifest["files"].values():
        src = info["source"] or "(root)"
        if src not in source_stats:
            source_stats[src] = {"equivalent": 0, "divergent": 0, "incompatible": 0, "pending": 0, "total": 0}
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

    # Equivalent files
    equivalent = sorted(
        (k, v) for k, v in manifest["files"].items()
        if v["classification"] == "equivalent"
    )
    if equivalent:
        lines.append("")
        lines.append(f"### Equivalent files ({len(equivalent)})")
        lines.append("")
        for k, v in equivalent:
            lines.append(f"- `{k}` ({v['reason']})")

    # Divergent files
    divergent = sorted(
        (k, v) for k, v in manifest["files"].items()
        if v["classification"] == "divergent"
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


# ---------------------------------------------------------------------------
# Symlink view
# ---------------------------------------------------------------------------

def create_symlinks(manifest, symlinks_dir, examples_dir):
    symlinks_path = Path(symlinks_dir)

    # Clear existing symlink directory
    if symlinks_path.exists():
        import shutil
        shutil.rmtree(symlinks_path)

    created = 0
    for rel_path, info in sorted(manifest["files"].items()):
        cls = info["classification"]
        # Map classification to directory
        if cls in ("equivalent", "divergent", "incompatible", "pending"):
            dest_dir = symlinks_path / cls
        else:
            dest_dir = symlinks_path / "other"

        dest = dest_dir / rel_path
        dest.parent.mkdir(parents=True, exist_ok=True)

        # Compute relative path from symlink location to actual file
        actual = Path(examples_dir) / rel_path
        if actual.exists():
            rel_target = os.path.relpath(actual, dest.parent)
            dest.symlink_to(rel_target)
            created += 1

    print(f"Symlink view created at {symlinks_dir} ({created:,} links)")


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main():
    parser = argparse.ArgumentParser(description="Generate compatibility assessment reports")
    parser.add_argument("--manifest", default=None, help="Path to manifest file")
    parser.add_argument("--csv", default=None, metavar="FILE", help="Export as CSV")
    parser.add_argument("--tsv", default=None, metavar="FILE", help="Export as TSV")
    parser.add_argument("--report", default=None, metavar="FILE", help="Write self-contained Markdown report")
    parser.add_argument("--repo", default=None, metavar="OWNER/REPO", help="GitHub repository for commit links")
    parser.add_argument("--commit-sha", default=None, metavar="SHA", help="Commit SHA for report links")
    parser.add_argument("--symlinks", default=None, metavar="DIR", help="Create symlink directory view")

    args = parser.parse_args()

    # Resolve paths
    script_dir = Path(__file__).resolve().parent
    repo_root = script_dir.parent
    examples_dir = repo_root / "tests" / "examples"
    manifest_path = Path(args.manifest) if args.manifest else examples_dir / "compat-manifest.json"

    if not manifest_path.exists():
        print(f"error: manifest not found: {manifest_path}", file=sys.stderr)
        print("Run scripts/compat-scan.py first.", file=sys.stderr)
        sys.exit(1)

    manifest = load_manifest(manifest_path)

    # Always print summary
    print_summary(manifest)

    if args.csv:
        print()
        write_csv(manifest, args.csv)

    if args.tsv:
        print()
        write_tsv(manifest, args.tsv)

    if args.report:
        print()
        write_report(manifest, args.report, repo=args.repo, commit_sha=args.commit_sha)

    if args.symlinks:
        print()
        create_symlinks(manifest, args.symlinks, examples_dir)


if __name__ == "__main__":
    main()
