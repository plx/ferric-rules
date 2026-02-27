#!/usr/bin/env python3
"""Analyze CLIPS test-suite .bat files for extractable segments.

Parses each unique (non-duplicate, non-benchmark) .bat file into (clear)-delimited
cycles and classifies each top-level form.  Produces a JSON report summarising
which cycles contain only constructs + control commands (and are therefore
extractable as standalone .clp files).

Usage:
    python scripts/bat-analyze.py [--examples-dir DIR] [--manifest FILE] [--output FILE]
"""

import argparse
import json
import sys
from collections import defaultdict
from pathlib import Path


# ---------------------------------------------------------------------------
# Classification keywords
# ---------------------------------------------------------------------------

CONSTRUCT_KEYWORDS = {
    "defrule", "deftemplate", "deffacts", "deffunction",
    "defglobal", "defmodule", "defgeneric", "defmethod",
}

COOL_KEYWORDS = {
    "defclass", "definstances", "defmessage-handler",
}

CONTROL_KEYWORDS = {
    "reset", "run",
}

WATCH_KEYWORDS = {
    "watch", "unwatch",
}

REPL_KEYWORDS = {
    "assert", "retract", "facts", "agenda", "matches", "refresh",
    "set-strategy", "get-strategy", "set-break", "remove-break",
    "halt", "ppdefrule", "ppdeffacts", "ppdeftemplate",
    "list-defrules", "list-deffacts", "list-deftemplates",
    "undefrule", "assert-string", "load-facts", "save-facts",
    "bind", "set-salience-evaluation",
}

NOISE_KEYWORDS = {
    "clear",
}

# Benchmark directories to skip (relative path segments)
BENCHMARK_SKIP_SEGMENTS = {"manners", "waltz", "sudoku"}


# ---------------------------------------------------------------------------
# Parenthesis-depth top-level form extractor
# ---------------------------------------------------------------------------

def extract_top_level_forms(text):
    """Extract top-level parenthesised forms from *text*.

    Returns a list of (form_text, start_offset) tuples.
    Handles:
      - String literals (ignores parens inside "...")
      - CLIPS comments (ignores everything after ; outside strings)
      - Escaped quotes inside strings (\")
    """
    forms = []
    depth = 0
    in_string = False
    i = 0
    n = len(text)
    form_start = None

    while i < n:
        ch = text[i]

        # Handle escape inside string
        if in_string:
            if ch == '\\' and i + 1 < n:
                i += 2
                continue
            if ch == '"':
                in_string = False
            i += 1
            continue

        # Outside string
        if ch == '"':
            in_string = True
            i += 1
            continue

        if ch == ';':
            # Skip to end of line (comment)
            while i < n and text[i] != '\n':
                i += 1
            continue

        if ch == '(':
            if depth == 0:
                form_start = i
            depth += 1
        elif ch == ')':
            depth -= 1
            if depth <= 0:
                depth = 0
                if form_start is not None:
                    forms.append((text[form_start:i + 1], form_start))
                    form_start = None

        i += 1

    return forms


def first_keyword(form_text):
    """Return the first symbol after the opening paren of *form_text*.

    E.g. "(defrule foo ...)" -> "defrule"
    """
    i = 0
    n = len(form_text)
    # Skip past opening paren
    while i < n and form_text[i] != '(':
        i += 1
    i += 1  # skip '('
    # Skip whitespace
    while i < n and form_text[i] in (' ', '\t', '\n', '\r'):
        i += 1
    # Collect keyword characters
    start = i
    while i < n and form_text[i] not in (' ', '\t', '\n', '\r', ')', '(', '"'):
        i += 1
    return form_text[start:i].lower()


# ---------------------------------------------------------------------------
# Cycle splitting
# ---------------------------------------------------------------------------

def split_into_cycles(text):
    """Split *text* into (clear)-delimited cycles.

    Returns a list of dicts:
        {
            "raw": <text of this cycle>,
            "label": <optional label from the (clear) line>,
            "forms": [(form_text, keyword), ...]
        }

    The first cycle is everything before the first (clear).
    Each subsequent cycle is everything between two (clear) calls.
    """
    forms = extract_top_level_forms(text)

    cycles = []
    current_forms = []
    current_label = None
    # Before the first clear, there's an implicit cycle
    # (which may be empty — that's fine)

    for form_text, start_offset in forms:
        kw = first_keyword(form_text)
        if kw == "clear":
            # Emit current cycle (if any forms, or if it's not the first)
            if current_forms or cycles:
                cycles.append({
                    "label": current_label,
                    "forms": current_forms,
                })
            current_forms = []
            # Extract label from the (clear) line — look for "; ..." on same line
            current_label = _extract_clear_label(text, start_offset)
        else:
            current_forms.append((form_text, kw))

    # Trailing cycle after last (clear)
    if current_forms:
        cycles.append({
            "label": current_label,
            "forms": current_forms,
        })

    return cycles


def _extract_clear_label(text, clear_start):
    """Look for a comment on the (clear) line for use as a label."""
    # Find end of the (clear) form
    depth = 0
    i = clear_start
    n = len(text)
    while i < n:
        if text[i] == '(':
            depth += 1
        elif text[i] == ')':
            depth -= 1
            if depth == 0:
                i += 1
                break
        i += 1

    # Now scan the rest of the line for a comment
    while i < n and text[i] in (' ', '\t'):
        i += 1

    if i < n and text[i] == ';':
        # Collect until end of line
        j = i + 1
        while j < n and text[j] != '\n':
            j += 1
        label = text[i + 1:j].strip()
        if label:
            return label

    return None


# ---------------------------------------------------------------------------
# Classify forms within a cycle
# ---------------------------------------------------------------------------

def classify_keyword(kw):
    """Return the classification category for a keyword string."""
    if kw in CONSTRUCT_KEYWORDS:
        return "construct"
    if kw in COOL_KEYWORDS:
        return "cool"
    if kw in CONTROL_KEYWORDS:
        return "control"
    if kw in WATCH_KEYWORDS:
        return "watch"
    if kw in NOISE_KEYWORDS:
        return "noise"
    if kw in REPL_KEYWORDS:
        return "repl"
    # Anything else is also repl-only (unrecognised command)
    return "repl"


def classify_cycle(cycle):
    """Determine whether a cycle is extractable.

    Returns (extractable: bool, has_cool: bool, repl_cmds: set).
    """
    has_cool = False
    repl_cmds = set()
    extractable = True

    for _form_text, kw in cycle["forms"]:
        cat = classify_keyword(kw)
        if cat == "cool":
            has_cool = True
            extractable = False
        elif cat == "repl":
            repl_cmds.add(kw)
            extractable = False

    return extractable, has_cool, repl_cmds


# ---------------------------------------------------------------------------
# File-level analysis
# ---------------------------------------------------------------------------

def should_skip_file(rel_path):
    """Return True if the .bat file is a benchmark we should skip."""
    parts = Path(rel_path).parts
    for seg in BENCHMARK_SKIP_SEGMENTS:
        if seg in parts:
            return True
    return False


def analyze_file(examples_dir, rel_path):
    """Analyze a single .bat file.

    Returns a dict suitable for the JSON report, or None to skip.
    """
    filepath = Path(examples_dir) / rel_path

    try:
        text = filepath.read_text(encoding="utf-8", errors="replace")
    except OSError as e:
        return {"error": str(e)}

    cycles = split_into_cycles(text)

    file_has_cool = False
    all_repl_cmds = set()
    cycle_reports = []

    for idx, cycle in enumerate(cycles):
        extractable, has_cool, repl_cmds = classify_cycle(cycle)
        if has_cool:
            file_has_cool = True
        all_repl_cmds |= repl_cmds

        commands = [kw for _ft, kw in cycle["forms"]]

        cycle_reports.append({
            "index": idx,
            "extractable": extractable,
            "commands": commands,
            "label": cycle["label"],
        })

    extractable_count = sum(1 for c in cycle_reports if c["extractable"])
    non_extractable_count = len(cycle_reports) - extractable_count

    return {
        "total_cycles": len(cycle_reports),
        "extractable_cycles": extractable_count,
        "non_extractable_cycles": non_extractable_count,
        "has_cool": file_has_cool,
        "repl_commands_used": sorted(all_repl_cmds),
        "cycles": cycle_reports,
    }


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main():
    parser = argparse.ArgumentParser(
        description="Analyze CLIPS .bat test files for extractable segments"
    )
    parser.add_argument(
        "--examples-dir",
        default=None,
        help="Path to tests/examples directory (default: auto-detect)",
    )
    parser.add_argument(
        "--manifest",
        default=None,
        help="Path to compat-manifest.json (default: tests/examples/compat-manifest.json)",
    )
    parser.add_argument(
        "--output",
        default=None,
        help="Output analysis JSON path (default: tests/examples/bat-analysis.json)",
    )
    args = parser.parse_args()

    # Resolve repo root
    script_dir = Path(__file__).resolve().parent
    repo_root = script_dir.parent

    examples_dir = args.examples_dir or str(repo_root / "tests" / "examples")
    manifest_path = args.manifest or str(
        repo_root / "tests" / "examples" / "compat-manifest.json"
    )
    output_path = args.output or str(
        repo_root / "tests" / "examples" / "bat-analysis.json"
    )

    if not Path(examples_dir).is_dir():
        print(f"error: examples directory not found: {examples_dir}", file=sys.stderr)
        sys.exit(1)

    if not Path(manifest_path).is_file():
        print(f"error: manifest not found: {manifest_path}", file=sys.stderr)
        sys.exit(1)

    # Load the manifest to find unique (non-duplicate) .bat files
    with open(manifest_path, encoding="utf-8") as f:
        manifest = json.load(f)

    bat_files = []
    for rel_path, info in sorted(manifest["files"].items()):
        if not rel_path.endswith(".bat"):
            continue
        # Skip duplicates
        if info.get("reason") == "duplicate-batch":
            continue
        # Skip benchmarks
        if should_skip_file(rel_path):
            continue
        bat_files.append(rel_path)

    print(f"Analyzing {len(bat_files)} unique non-benchmark .bat files ...")

    # Analyze each file
    file_reports = {}
    total_extractable = 0
    total_non_extractable = 0
    files_with_extractable = 0
    repl_cmd_counts = defaultdict(int)

    for rel_path in bat_files:
        report = analyze_file(examples_dir, rel_path)
        file_reports[rel_path] = report

        if "error" in report:
            continue

        if report["extractable_cycles"] > 0:
            files_with_extractable += 1
        total_extractable += report["extractable_cycles"]
        total_non_extractable += report["non_extractable_cycles"]

        # Count repl commands across all files (per-cycle occurrences)
        for cycle in report["cycles"]:
            if not cycle["extractable"]:
                for cmd in cycle["commands"]:
                    cat = classify_keyword(cmd)
                    if cat == "repl":
                        repl_cmd_counts[cmd] += 1

    summary = {
        "total_files": len(bat_files),
        "files_with_extractable_cycles": files_with_extractable,
        "total_extractable_cycles": total_extractable,
        "total_non_extractable_cycles": total_non_extractable,
        "repl_commands_needed": dict(
            sorted(repl_cmd_counts.items(), key=lambda x: -x[1])
        ),
    }

    result = {
        "files": file_reports,
        "summary": summary,
    }

    # Write output
    output = Path(output_path)
    output.parent.mkdir(parents=True, exist_ok=True)
    with open(output, "w", encoding="utf-8") as f:
        json.dump(result, f, indent=2, ensure_ascii=False)
        f.write("\n")

    print(f"\nAnalysis written to {output_path}")
    print(f"\nSummary:")
    print(f"  Total files analysed:           {summary['total_files']}")
    print(f"  Files with extractable cycles:  {summary['files_with_extractable_cycles']}")
    print(f"  Total extractable cycles:       {summary['total_extractable_cycles']}")
    print(f"  Total non-extractable cycles:   {summary['total_non_extractable_cycles']}")

    if repl_cmd_counts:
        print(f"\n  REPL commands blocking extraction (by occurrence):")
        for cmd, count in sorted(repl_cmd_counts.items(), key=lambda x: -x[1]):
            print(f"    {cmd:30s}: {count}")


if __name__ == "__main__":
    main()
