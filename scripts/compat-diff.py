#!/usr/bin/env python3
"""Compare two compatibility manifests and produce a Markdown summary.

Reads a "base" and "head" manifest (as produced by compat-scan.py /
compat-run.py) and emits a Markdown table showing classification
changes between the two.

Usage:
    python scripts/compat-diff.py BASE_MANIFEST HEAD_MANIFEST

The output is written to stdout as GitHub-flavored Markdown, suitable
for appending to $GITHUB_STEP_SUMMARY.
"""

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


def main():
    if len(sys.argv) != 3:
        print(f"usage: {sys.argv[0]} BASE_MANIFEST HEAD_MANIFEST", file=sys.stderr)
        sys.exit(1)

    base_path, head_path = sys.argv[1], sys.argv[2]
    base = load_manifest(base_path)
    head = load_manifest(head_path)

    base_files = base.get("files", {})
    head_files = head.get("files", {})

    # ------------------------------------------------------------------
    # Summary counts
    # ------------------------------------------------------------------
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

    base_total = sum(base_counts.values())
    head_total = sum(head_counts.values())

    # ------------------------------------------------------------------
    # Per-file changes
    # ------------------------------------------------------------------
    improvements = []
    regressions = []

    all_keys = sorted(set(base_files) | set(head_files))
    for key in all_keys:
        b = base_files.get(key)
        h = head_files.get(key)

        if b is None or h is None:
            continue  # new or removed file; not a classification change

        b_cls = b["classification"]
        h_cls = h["classification"]

        if b_cls == h_cls:
            # Check if reason changed within the same classification
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

    # Filter out same-classification reason-only changes from improvements
    # (those are neutral, not real improvements)
    real_improvements = [e for e in improvements if e[1] != e[3]]
    reason_changes = [e for e in improvements if e[1] == e[3]]

    # ------------------------------------------------------------------
    # Output Markdown
    # ------------------------------------------------------------------
    print("## CLIPS Compatibility Report")
    print()
    print("| Classification | Base | Head | Delta |")
    print("|---|---:|---:|---|")
    for cls in DISPLAY_ORDER:
        b = base_counts[cls]
        h = head_counts[cls]
        d = h - b
        delta_str = f"**{fmt_delta(d)}**" if d != 0 else "—"
        print(f"| {cls} | {b} | {h} | {delta_str} |")
    d_total = head_total - base_total
    delta_total = f"**{fmt_delta(d_total)}**" if d_total != 0 else "—"
    print(f"| **total** | **{base_total}** | **{head_total}** | {delta_total} |")

    # Regressions
    print()
    if regressions:
        print(f"### Regressions ({len(regressions)})")
        print()
        print("| File | Before | After |")
        print("|---|---|---|")
        for path, b_cls, b_reason, h_cls, h_reason in regressions:
            print(f"| `{path}` | {b_cls} ({b_reason}) | {h_cls} ({h_reason}) |")
    else:
        print("### Regressions")
        print()
        print("None")

    # Improvements
    print()
    if real_improvements:
        print(f"### Improvements ({len(real_improvements)})")
        print()
        print("| File | Before | After |")
        print("|---|---|---|")
        for path, b_cls, b_reason, h_cls, h_reason in real_improvements:
            print(f"| `{path}` | {b_cls} ({b_reason}) | {h_cls} ({h_reason}) |")
    else:
        print("### Improvements")
        print()
        print("None")

    # Reason-only changes (informational)
    if reason_changes:
        print()
        print(f"<details><summary>Reason changes within same classification ({len(reason_changes)})</summary>")
        print()
        print("| File | Classification | Before | After |")
        print("|---|---|---|---|")
        for path, b_cls, b_reason, h_cls, h_reason in reason_changes:
            print(f"| `{path}` | {b_cls} | {b_reason} | {h_reason} |")
        print()
        print("</details>")

    # Exit with code 1 if there are regressions.
    # The CI workflow uses continue-on-error so this surfaces as a
    # visible warning rather than a hard failure.
    if regressions:
        sys.exit(1)


if __name__ == "__main__":
    main()
