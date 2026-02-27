#!/usr/bin/env python3
"""Compare two compatibility manifests and produce a Markdown summary.

Reads a "base" and "head" manifest (as produced by compat-scan.py /
compat-run.py) and emits a Markdown table showing classification
changes between the two.

Usage:
    python scripts/compat-diff.py BASE_MANIFEST HEAD_MANIFEST [options]

Options:
    --tsv FILE          Write per-file raw data as TSV
    --report FILE       Write a self-contained Markdown report with context
    --repo OWNER/REPO   GitHub repository (for commit links)
    --base-sha SHA      Base commit SHA (for commit links)
    --head-sha SHA      Head commit SHA (for commit links)

Stdout is always the comparison table (suitable for $GITHUB_STEP_SUMMARY).
"""

import argparse
import csv
import json
import sys

DISPLAY_ORDER = ["equivalent", "divergent", "incompatible", "pending"]

# Ordered from best to worst for determining regressions vs improvements.
# equivalent (best) > divergent > pending > incompatible (worst)
RANK = {"equivalent": 0, "divergent": 1, "pending": 2, "incompatible": 3}


def load_manifest(path):
    with open(path, "r", encoding="utf-8") as f:
        return json.load(f)


def fmt_delta(n):
    if n > 0:
        return f"+{n}"
    if n < 0:
        return str(n)
    return "0"


def compute_diff(base, head):
    """Compute counts and per-file changes between two manifests.

    Returns (base_counts, head_counts, regressions, real_improvements, reason_changes).
    """
    base_files = base.get("files", {})
    head_files = head.get("files", {})

    base_counts = {cls: 0 for cls in DISPLAY_ORDER}
    head_counts = {cls: 0 for cls in DISPLAY_ORDER}

    for info in base_files.values():
        cls = info["classification"]
        if cls in base_counts:
            base_counts[cls] += 1

    for info in head_files.values():
        cls = info["classification"]
        if cls in head_counts:
            head_counts[cls] += 1

    improvements = []
    regressions = []

    all_keys = sorted(set(base_files) | set(head_files))
    for key in all_keys:
        b = base_files.get(key)
        h = head_files.get(key)

        if b is None or h is None:
            continue

        b_cls = b["classification"]
        h_cls = h["classification"]

        if b_cls == h_cls:
            b_reason = b.get("reason", "")
            h_reason = h.get("reason", "")
            if b_reason != h_reason:
                improvements.append((key, b_cls, b_reason, h_cls, h_reason))
            continue

        entry = (key, b_cls, b.get("reason", ""), h_cls, h.get("reason", ""))
        b_rank = RANK.get(b_cls, 99)
        h_rank = RANK.get(h_cls, 99)

        if h_rank < b_rank:
            improvements.append(entry)
        else:
            regressions.append(entry)

    real_improvements = [e for e in improvements if e[1] != e[3]]
    reason_changes = [e for e in improvements if e[1] == e[3]]

    return base_counts, head_counts, regressions, real_improvements, reason_changes


def format_markdown(base_counts, head_counts, regressions, real_improvements,
                    reason_changes, *, repo=None, base_sha=None, head_sha=None):
    """Build the full Markdown report as a list of lines."""
    lines = []

    lines.append("## CLIPS Compatibility Report")
    lines.append("")

    # Context blurb + commit links
    lines.append("Compares ferric's compatibility with CLIPS across a corpus of")
    lines.append("example `.clp` files. Each file is classified as **equivalent**")
    lines.append("(output matches CLIPS), **divergent** (runs but output differs),")
    lines.append("**incompatible** (cannot run), or **pending** (not yet tested).")
    lines.append("")

    if repo and base_sha and head_sha:
        base_link = f"[`{base_sha[:10]}`](https://github.com/{repo}/commit/{base_sha})"
        head_link = f"[`{head_sha[:10]}`](https://github.com/{repo}/commit/{head_sha})"
        lines.append(f"Base: {base_link} | Head: {head_link}")
        lines.append("")

    base_total = sum(base_counts.values())
    head_total = sum(head_counts.values())

    lines.append("| Classification | Base | Head | Delta |")
    lines.append("|---|---:|---:|---|")
    for cls in DISPLAY_ORDER:
        b = base_counts[cls]
        h = head_counts[cls]
        d = h - b
        delta_str = f"**{fmt_delta(d)}**" if d != 0 else "—"
        lines.append(f"| {cls} | {b} | {h} | {delta_str} |")
    d_total = head_total - base_total
    delta_total = f"**{fmt_delta(d_total)}**" if d_total != 0 else "—"
    lines.append(f"| **total** | **{base_total}** | **{head_total}** | {delta_total} |")

    # Regressions
    lines.append("")
    if regressions:
        lines.append(f"### Regressions ({len(regressions)})")
        lines.append("")
        lines.append("| File | Before | After |")
        lines.append("|---|---|---|")
        for path, b_cls, b_reason, h_cls, h_reason in regressions:
            lines.append(f"| `{path}` | {b_cls} ({b_reason}) | {h_cls} ({h_reason}) |")
    else:
        lines.append("### Regressions")
        lines.append("")
        lines.append("None")

    # Improvements
    lines.append("")
    if real_improvements:
        lines.append(f"### Improvements ({len(real_improvements)})")
        lines.append("")
        lines.append("| File | Before | After |")
        lines.append("|---|---|---|")
        for path, b_cls, b_reason, h_cls, h_reason in real_improvements:
            lines.append(f"| `{path}` | {b_cls} ({b_reason}) | {h_cls} ({h_reason}) |")
    else:
        lines.append("### Improvements")
        lines.append("")
        lines.append("None")

    # Reason-only changes
    if reason_changes:
        lines.append("")
        lines.append(f"<details><summary>Reason changes within same classification ({len(reason_changes)})</summary>")
        lines.append("")
        lines.append("| File | Classification | Before | After |")
        lines.append("|---|---|---|---|")
        for path, b_cls, b_reason, h_cls, h_reason in reason_changes:
            lines.append(f"| `{path}` | {b_cls} | {b_reason} | {h_reason} |")
        lines.append("")
        lines.append("</details>")

    return lines


def write_tsv(base, head, tsv_path):
    """Write per-file raw data as TSV."""
    base_files = base.get("files", {})
    head_files = head.get("files", {})
    all_keys = sorted(set(base_files) | set(head_files))

    fieldnames = [
        "path", "source",
        "base_classification", "base_reason",
        "head_classification", "head_reason",
        "change",
    ]

    with open(tsv_path, "w", newline="", encoding="utf-8") as f:
        writer = csv.DictWriter(f, fieldnames=fieldnames, delimiter="\t")
        writer.writeheader()

        for key in all_keys:
            b = base_files.get(key)
            h = head_files.get(key)

            b_cls = b["classification"] if b else ""
            b_reason = b.get("reason", "") if b else ""
            h_cls = h["classification"] if h else ""
            h_reason = h.get("reason", "") if h else ""
            source = (h or b).get("source", "")

            if not b:
                change = "added"
            elif not h:
                change = "removed"
            elif b_cls != h_cls:
                b_rank = RANK.get(b_cls, 99)
                h_rank = RANK.get(h_cls, 99)
                change = "improvement" if h_rank < b_rank else "regression"
            elif b_reason != h_reason:
                change = "reason-changed"
            else:
                change = "unchanged"

            writer.writerow({
                "path": key,
                "source": source,
                "base_classification": b_cls,
                "base_reason": b_reason,
                "head_classification": h_cls,
                "head_reason": h_reason,
                "change": change,
            })


def main():
    parser = argparse.ArgumentParser(description="Compare two compat manifests")
    parser.add_argument("base_manifest", help="Base manifest JSON")
    parser.add_argument("head_manifest", help="Head manifest JSON")
    parser.add_argument("--tsv", metavar="FILE", help="Write per-file data as TSV")
    parser.add_argument("--report", metavar="FILE", help="Write self-contained Markdown report")
    parser.add_argument("--repo", metavar="OWNER/REPO", help="GitHub repository for commit links")
    parser.add_argument("--base-sha", metavar="SHA", help="Base commit SHA")
    parser.add_argument("--head-sha", metavar="SHA", help="Head commit SHA")

    args = parser.parse_args()

    base = load_manifest(args.base_manifest)
    head = load_manifest(args.head_manifest)

    base_counts, head_counts, regressions, real_improvements, reason_changes = compute_diff(base, head)

    # Always write the comparison table to stdout (for $GITHUB_STEP_SUMMARY)
    md_lines = format_markdown(
        base_counts, head_counts, regressions, real_improvements, reason_changes,
        repo=args.repo, base_sha=args.base_sha, head_sha=args.head_sha,
    )
    print("\n".join(md_lines))

    # Optional: write self-contained Markdown report to file
    if args.report:
        with open(args.report, "w", encoding="utf-8") as f:
            f.write("\n".join(md_lines))
            f.write("\n")

    # Optional: write TSV
    if args.tsv:
        write_tsv(base, head, args.tsv)

    # Exit with code 1 if there are regressions.
    # The CI workflow uses continue-on-error so this surfaces as a
    # visible warning rather than a hard failure.
    if regressions:
        sys.exit(1)


if __name__ == "__main__":
    main()
