#!/usr/bin/env python3
"""Static scanner for CLIPS compatibility assessment.

Scans all .clp files under tests/examples/ and produces a JSON manifest
classifying each file by detected features and ferric compatibility.

Usage:
    python scripts/compat-scan.py [--examples-dir DIR] [--output FILE]
"""

import argparse
import json
import os
import re
import sys
from datetime import datetime, timezone
from pathlib import Path

# ---------------------------------------------------------------------------
# Feature detection patterns
# ---------------------------------------------------------------------------

# These are matched against comment-stripped file content.
# We look for opening-paren + keyword to avoid matching inside symbol names.

SUPPORTED_CONSTRUCTS = [
    "defrule", "deftemplate", "deffacts", "deffunction",
    "defglobal", "defmodule", "defgeneric", "defmethod",
]

COOL_CONSTRUCTS = [
    "defclass", "definstances", "defmessage-handler",
]

UNSUPPORTED_CONTROL = [
    "switch",
]

UNSUPPORTED_IO = ["open", "close"]

INTERACTIVE_IO = ["read", "readline"]

LOADING_COMMANDS = ["batch", "batch*", "load", "load*"]


def _build_keyword_pattern(keywords):
    """Build a regex that matches (keyword ... at a word boundary."""
    escaped = [re.escape(k) for k in keywords]
    return re.compile(r"\(\s*(?:" + "|".join(escaped) + r")(?:\s|[)\"])", re.IGNORECASE)


PAT_COOL = _build_keyword_pattern(COOL_CONSTRUCTS)
PAT_UNSUPPORTED_CONTROL = _build_keyword_pattern(UNSUPPORTED_CONTROL)
PAT_UNSUPPORTED_IO = _build_keyword_pattern(UNSUPPORTED_IO)
PAT_INTERACTIVE = _build_keyword_pattern(INTERACTIVE_IO)
PAT_LOADING = _build_keyword_pattern(LOADING_COMMANDS)
PAT_DEFRULE = re.compile(r"\(\s*defrule\s", re.IGNORECASE)
PAT_DEFFACTS = re.compile(r"\(\s*deffacts\s", re.IGNORECASE)
PAT_PRINTOUT = re.compile(r"\(\s*printout\s", re.IGNORECASE)

# Patterns for all constructs we want to tag
_ALL_CONSTRUCTS = SUPPORTED_CONSTRUCTS + COOL_CONSTRUCTS
PAT_ALL_CONSTRUCTS = {
    kw: re.compile(r"\(\s*" + re.escape(kw) + r"(?:\s|[)\"])", re.IGNORECASE)
    for kw in _ALL_CONSTRUCTS
}


def strip_comments(text):
    """Remove CLIPS comments from source text.

    Strips full-line comments (lines where first non-whitespace is ;) and
    inline comments (from ; to end of line), with a simple heuristic to
    avoid stripping inside string literals.
    """
    lines = text.split("\n")
    result = []
    for line in lines:
        # Full-line comment
        stripped = line.lstrip()
        if stripped.startswith(";"):
            result.append("")
            continue
        # Inline comment: find ; not inside a string
        # Simple approach: track quote parity
        in_string = False
        clean = []
        for ch in line:
            if ch == '"':
                in_string = not in_string
            if ch == ";" and not in_string:
                break
            clean.append(ch)
        result.append("".join(clean))
    return "\n".join(result)


def detect_features(content):
    """Detect CLIPS language features in (comment-stripped) content.

    Returns:
        features: list of detected feature keywords
        unsupported: list of detected unsupported feature keywords
    """
    features = []
    unsupported = []

    # Supported constructs
    for kw, pat in PAT_ALL_CONSTRUCTS.items():
        if pat.search(content):
            features.append(kw)

    # Check for printout (not a construct, but useful to track)
    if PAT_PRINTOUT.search(content):
        features.append("printout")

    # Unsupported: COOL
    for kw in COOL_CONSTRUCTS:
        if kw in features:
            unsupported.append(kw)

    # Unsupported: control flow
    if PAT_UNSUPPORTED_CONTROL.search(content):
        for kw in UNSUPPORTED_CONTROL:
            pat = re.compile(r"\(\s*" + re.escape(kw) + r"(?:\s|[)\"])", re.IGNORECASE)
            if pat.search(content):
                unsupported.append(kw)

    # Unsupported: file I/O
    if PAT_UNSUPPORTED_IO.search(content):
        for kw in UNSUPPORTED_IO:
            pat = re.compile(r"\(\s*" + re.escape(kw) + r"(?:\s|[)\"])", re.IGNORECASE)
            if pat.search(content):
                unsupported.append(kw)

    # Interactive I/O
    if PAT_INTERACTIVE.search(content):
        for kw in INTERACTIVE_IO:
            pat = re.compile(r"\(\s*" + re.escape(kw) + r"(?:\s|[)\"])", re.IGNORECASE)
            if pat.search(content):
                unsupported.append(kw)

    # Loading commands
    if PAT_LOADING.search(content):
        for kw in LOADING_COMMANDS:
            pat = re.compile(r"\(\s*" + re.escape(kw) + r"(?:\s|[)\"])", re.IGNORECASE)
            if pat.search(content):
                unsupported.append(kw)

    return features, unsupported


def classify_file(path, features, unsupported):
    """Pre-classify a file based on detected features.

    Returns (classification, reason, runability).
    """
    suffix = path.suffix.lower()

    # .bat files are batch scripts
    if suffix == ".bat":
        return "incompatible", "test-suite-batch", "batch"

    # COOL constructs
    cool_features = [f for f in unsupported if f in COOL_CONSTRUCTS]
    if cool_features:
        return "incompatible", "unsupported-form", "standalone"

    # Unsupported control flow
    control_features = [f for f in unsupported if f in UNSUPPORTED_CONTROL]
    if control_features:
        return "incompatible", "unsupported-control", "standalone"

    # File I/O
    io_features = [f for f in unsupported if f in UNSUPPORTED_IO]
    if io_features:
        return "incompatible", "unsupported-io", "standalone"

    # Interactive I/O
    interactive_features = [f for f in unsupported if f in INTERACTIVE_IO]
    if interactive_features:
        return "incompatible", "interactive", "interactive"

    # Loading commands (batch/load within file)
    loading_features = [f for f in unsupported if f in LOADING_COMMANDS]
    if loading_features:
        return "incompatible", "unsupported-command", "batch"

    # No defrule → library only
    if "defrule" not in features:
        return "incompatible", "library-only", "library"

    # Testable
    return "pending", "testable", "standalone"


def scan_examples(examples_dir):
    """Scan all .clp and .bat files under examples_dir.

    Returns a dict of {relative_path: file_info}.
    """
    files = {}
    examples_path = Path(examples_dir)

    # Collect all .clp and .bat files
    all_files = sorted(examples_path.rglob("*.clp")) + sorted(examples_path.rglob("*.bat"))

    for filepath in all_files:
        rel = filepath.relative_to(examples_path)
        rel_str = str(rel)

        # Determine source directory (first path component)
        source = rel.parts[0] if len(rel.parts) > 1 else ""

        # Read file content
        try:
            raw_content = filepath.read_text(encoding="utf-8", errors="replace")
        except OSError as e:
            files[rel_str] = {
                "source": source,
                "classification": "incompatible",
                "reason": "read-error",
                "runability": "unknown",
                "features": [],
                "unsupported_features": [],
                "ferric": None,
                "clips": None,
                "notes": str(e),
            }
            continue

        # Strip comments and detect features
        cleaned = strip_comments(raw_content)
        features, unsupported = detect_features(cleaned)

        # Classify
        classification, reason, runability = classify_file(filepath, features, unsupported)

        files[rel_str] = {
            "source": source,
            "classification": classification,
            "reason": reason,
            "runability": runability,
            "features": sorted(set(features)),
            "unsupported_features": sorted(set(unsupported)),
            "ferric": None,
            "clips": None,
            "notes": "",
        }

    return files


def build_summary(files):
    """Compute summary counts from the files dict."""
    counts = {"total": 0, "equivalent": 0, "divergent": 0, "incompatible": 0, "pending": 0}
    for info in files.values():
        counts["total"] += 1
        cls = info["classification"]
        if cls in counts:
            counts[cls] += 1
    return counts


def main():
    parser = argparse.ArgumentParser(description="Scan CLIPS examples for compatibility assessment")
    parser.add_argument(
        "--examples-dir",
        default=None,
        help="Path to tests/examples directory (default: auto-detect from repo root)",
    )
    parser.add_argument(
        "--output",
        default=None,
        help="Output manifest path (default: tests/examples/compat-manifest.json)",
    )
    args = parser.parse_args()

    # Find repo root
    script_dir = Path(__file__).resolve().parent
    repo_root = script_dir.parent

    examples_dir = args.examples_dir or str(repo_root / "tests" / "examples")
    output_path = args.output or str(repo_root / "tests" / "examples" / "compat-manifest.json")

    if not Path(examples_dir).is_dir():
        print(f"error: examples directory not found: {examples_dir}", file=sys.stderr)
        sys.exit(1)

    print(f"Scanning {examples_dir} ...")
    files = scan_examples(examples_dir)
    summary = build_summary(files)

    manifest = {
        "version": 1,
        "generated": datetime.now(timezone.utc).isoformat(),
        "summary": summary,
        "files": files,
    }

    # Write manifest
    output = Path(output_path)
    output.parent.mkdir(parents=True, exist_ok=True)
    with open(output, "w", encoding="utf-8") as f:
        json.dump(manifest, f, indent=2, ensure_ascii=False)
        f.write("\n")

    print(f"\nManifest written to {output_path}")
    print(f"\nSummary:")
    print(f"  Total files:    {summary['total']}")
    print(f"  Pending (testable): {summary['pending']}")
    print(f"  Incompatible:   {summary['incompatible']}")
    print(f"  Equivalent:     {summary['equivalent']}")
    print(f"  Divergent:      {summary['divergent']}")

    # Breakdown of incompatible reasons
    reason_counts = {}
    for info in files.values():
        if info["classification"] == "incompatible":
            reason = info["reason"]
            reason_counts[reason] = reason_counts.get(reason, 0) + 1
    if reason_counts:
        print(f"\n  Incompatible breakdown:")
        for reason, count in sorted(reason_counts.items(), key=lambda x: -x[1]):
            print(f"    {reason:25s}: {count}")


if __name__ == "__main__":
    main()
