"""Presentation of self-contained granular corpus evidence."""

from __future__ import annotations

from ferric_tools.compat.corpus_summary import corpus_counts, corpus_delta, corpus_verification


def _cell(value: object) -> str:
    return str(value).replace("|", "\\|").replace("\n", " ")


def corpus_lines(summary: dict | None, *, label: str = "Granular corpus") -> list[str]:
    lines = [f"### {label}", ""]
    if summary is None:
        return [*lines, "Corpus evidence unavailable; no counts or verification inferred.", ""]
    counts = corpus_counts(summary)
    verification = corpus_verification(summary)
    errors = counts["expected_errors"]
    lines += [
        "| Declared cases | Count |",
        "|---|---:|",
        f"| Registered | {counts['total']:,} |",
        f"| Conformance | {counts['conformance']:,} |",
        f"| Known gaps | {counts['gaps']:,} |",
        f"| Expected load errors (conformance subset) | {errors['load']:,} |",
        f"| Expected run errors (conformance subset) | {errors['run']:,} |",
        "",
        f"Evidence: **{verification['status']}** — {_cell(verification['reason'])}",
        "",
    ]
    checkout = summary["checkout"]
    state = "dirty working tree" if checkout.get("dirty") else "clean checkout"
    lines += [f"Revision: `{checkout['revision']}` ({state}).", ""]
    for name in ("ferric", "reference"):
        run = summary["runs"].get(name)
        if run is None:
            lines.append(f"- {name.title()}: unavailable / not run.")
            continue
        selection = run["selection"]
        scope = "full" if selection["full"] else "selected"
        lines.append(
            f"- {name.title()}: **{run['status']}**, {scope} selection "
            f"{len(selection['paths'])}/{counts['total']} cases; "
            f"{run['accepted']} accepted, {run['reported']} reported; exit {run['exit_code']}."
        )
        if run.get("error"):
            error = run["error"]
            lines.append(f"  - {_cell(error['stage'])}: {_cell(error['message'])}")
        if run.get("failures"):
            lines.append("  - Failed cases: " + ", ".join(f"`{_cell(p)}`" for p in run["failures"]))
    lines += ["", "Accepted known gaps remain gaps; acceptance is not conformance.", ""]
    return lines


def corpus_diff_lines(base: dict | None, head: dict | None) -> list[str]:
    lines = ["### Granular corpus changes", ""]
    if base is None or head is None:
        lines += ["Before/after corpus evidence unavailable; no fixed-gap claim inferred.", ""]
    else:
        delta = corpus_delta(base, head)
        fixed = delta["fixed_gaps"]
        verified = [item for item in fixed if item["verified"]]
        unverified = [item for item in fixed if not item["verified"]]
        groups = [
            ("Verified gap → conformance", verified),
            ("Declared gap → conformance (unverified)", unverified),
            ("Conformance → gap", delta["regressions"]),
            ("Added coverage", delta["added"]),
            ("Removed cases (not fixes)", delta["removed"]),
            ("Changed scenarios (not same-case fixes)", delta["changed_scenarios"]),
            ("Changed goldens (oracle changes)", delta["changed_goldens"]),
        ]
        lines += ["| Change | Cases |", "|---|---:|"]
        lines += [f"| {label} | {len(items)} |" for label, items in groups]
        lines.append("")
        for label, items in groups:
            if items:
                paths = [item["path"] if isinstance(item, dict) else item for item in items]
                lines += [f"- {label}: " + ", ".join(f"`{_cell(path)}`" for path in paths)]
        lines.append("")
    lines += corpus_lines(base, label="Base granular corpus")
    lines += corpus_lines(head, label="Head granular corpus")
    return lines
