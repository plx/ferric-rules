#!/usr/bin/env python3
"""Generate harness .clp files for library-only files in the compatibility manifest.

Library-only files define constructs (deffacts, deftemplate, defglobal, deffunction,
etc.) but have no defrule, so they produce no output when run standalone. This script
generates minimal harness files that load each library file and verify the constructs
loaded correctly.

Usage:
    python scripts/harness-gen.py [--manifest FILE] [--output-dir DIR] [--dry-run]

Defaults (relative to repo root):
    --manifest  tests/examples/compat-manifest.json
    --output-dir tests/harnesses/
"""

import argparse
import json
import os
import re
import sys
from pathlib import Path

# External-dependency keywords that indicate a file cannot be loaded standalone.
# Checked case-insensitively against the file content.
EXTERNAL_DEP_KEYWORDS = [
    "ros-",
    "ament-",
    "blackboard-",
    "pb-",
    "navgraph-",
    "protobuf-",
]


def find_repo_root():
    """Find the repository root (parent of scripts/ directory)."""
    script_dir = Path(__file__).resolve().parent
    return script_dir.parent


def load_manifest(manifest_path):
    """Load and return the compatibility manifest."""
    with open(manifest_path) as f:
        return json.load(f)


def save_manifest(manifest_path, manifest):
    """Save the manifest back to disk."""
    with open(manifest_path, "w") as f:
        json.dump(manifest, f, indent=2, ensure_ascii=False)
        f.write("\n")


def has_external_deps(content):
    """Check if file content references external dependency keywords."""
    content_lower = content.lower()
    for keyword in EXTERNAL_DEP_KEYWORDS:
        if keyword in content_lower:
            return True
    return False


def detect_constructs(content):
    """Parse file content with simple regexes to detect CLIPS constructs.

    Returns a dict with keys:
        deffacts: list of fact-set names
        deftemplate: list of template names
        defglobal: list of variable names (e.g. ?*x*)
        deffunction: list of (name, param_count) tuples
        defgeneric: list of generic names
        defmethod: list of method names
        defmodule: list of module names
    """
    constructs = {
        "deffacts": [],
        "deftemplate": [],
        "defglobal": [],
        "deffunction": [],
        "defgeneric": [],
        "defmethod": [],
        "defmodule": [],
    }

    # Strip comments (lines starting with ;) to avoid false matches.
    # We keep inline content after semicolons in case constructs appear mid-line,
    # but for safety we do a simple line-based strip of full-line comments.
    lines = content.split("\n")
    stripped_lines = []
    for line in lines:
        stripped = line.lstrip()
        if stripped.startswith(";"):
            continue
        stripped_lines.append(line)
    cleaned = "\n".join(stripped_lines)

    # deffacts: (deffacts <name> ...)
    for m in re.finditer(r"\(\s*deffacts\s+([\w:.-]+)", cleaned):
        constructs["deffacts"].append(m.group(1))

    # deftemplate: (deftemplate <name> ...)
    for m in re.finditer(r"\(\s*deftemplate\s+([\w:.-]+)", cleaned):
        constructs["deftemplate"].append(m.group(1))

    # defglobal: (defglobal [<module>] ?*<name>* = ...)
    # Variable names look like ?*name*
    for m in re.finditer(r"\?\*[\w-]+\*", cleaned):
        var = m.group(0)
        if var not in constructs["defglobal"]:
            constructs["defglobal"].append(var)

    # deffunction: (deffunction <name> (<params>) ...)
    for m in re.finditer(
        r"\(\s*deffunction\s+([\w:.-]+)\s*\(([^)]*)\)", cleaned
    ):
        name = m.group(1)
        params_str = m.group(2).strip()
        if params_str:
            # Count params: each ?var or $?var is one param
            param_count = len(re.findall(r"[\$]?\?\w+", params_str))
        else:
            param_count = 0
        constructs["deffunction"].append((name, param_count))

    # defgeneric: (defgeneric <name> ...)
    for m in re.finditer(r"\(\s*defgeneric\s+([\w:.-]+)", cleaned):
        constructs["defgeneric"].append(m.group(1))

    # defmethod: (defmethod <name> ...)
    for m in re.finditer(r"\(\s*defmethod\s+([\w:.-]+)", cleaned):
        name = m.group(1)
        if name not in constructs["defmethod"]:
            constructs["defmethod"].append(name)

    # defmodule: (defmodule <name> ...)
    for m in re.finditer(r"\(\s*defmodule\s+([\w:.-]+)", cleaned):
        constructs["defmodule"].append(m.group(1))

    return constructs


def has_any_constructs(constructs):
    """Check if any constructs were detected."""
    return any(len(v) > 0 for v in constructs.values())


def generate_harness(source_relpath, constructs):
    """Generate a harness .clp file content for the given source file.

    All harnesses use the same simple approach: a rule that fires on
    initial-fact and prints a confirmation message. We intentionally do
    not try to exercise the constructs because we do not know argument
    types, required globals, or other context.
    """
    # Build a human-readable summary of what was detected
    summary_parts = []
    for kind in [
        "deffacts",
        "deftemplate",
        "defglobal",
        "deffunction",
        "defgeneric",
        "defmethod",
        "defmodule",
    ]:
        items = constructs[kind]
        if items:
            if kind == "deffunction":
                names = [f"{name}/{count}" for name, count in items]
                summary_parts.append(f"{kind}: {', '.join(names)}")
            else:
                if kind == "defglobal":
                    summary_parts.append(f"{kind}: {', '.join(items)}")
                else:
                    summary_parts.append(f"{kind}: {', '.join(items)}")

    summary = "; ".join(summary_parts) if summary_parts else "no named constructs"

    lines = []
    lines.append(f"; Harness for {source_relpath}")
    lines.append(f"; Detected constructs: {summary}")
    lines.append(";")
    lines.append("; Strategy: verify file loads and reset succeeds.")
    lines.append("; The source file is loaded via (load ...) before this harness.")
    lines.append("")
    lines.append("(defrule harness-verify")
    lines.append("   (initial-fact)")
    lines.append("   =>")
    lines.append('   (printout t "HARNESS: loaded" crlf))')
    lines.append("")

    return "\n".join(lines)


def compute_harness_path(output_dir, manifest_key):
    """Compute the harness output path from the manifest key.

    manifest_key is like: source/path/to/file.clp
    Output: output_dir/source/path/to/file-harness.clp
    """
    p = Path(manifest_key)
    stem = p.stem
    harness_name = f"{stem}-harness.clp"
    harness_path = output_dir / p.parent / harness_name
    return harness_path


def main():
    repo_root = find_repo_root()

    parser = argparse.ArgumentParser(
        description="Generate harness .clp files for library-only files."
    )
    parser.add_argument(
        "--manifest",
        default=str(repo_root / "tests" / "examples" / "compat-manifest.json"),
        help="Path to the compatibility manifest JSON file.",
    )
    parser.add_argument(
        "--output-dir",
        default=str(repo_root / "tests" / "harnesses"),
        help="Output directory for generated harness files.",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Print what would be done without writing files.",
    )
    args = parser.parse_args()

    manifest_path = Path(args.manifest)
    output_dir = Path(args.output_dir)
    examples_dir = manifest_path.parent  # tests/examples/

    if not manifest_path.exists():
        print(f"Error: manifest not found: {manifest_path}", file=sys.stderr)
        sys.exit(1)

    manifest = load_manifest(manifest_path)
    files = manifest.get("files", {})

    # Collect library-only entries
    library_only = {
        k: v for k, v in files.items() if v.get("reason") == "library-only"
    }

    print(f"Found {len(library_only)} library-only files in manifest.")

    stats = {
        "generated": 0,
        "skipped_external": 0,
        "skipped_empty": 0,
        "skipped_missing": 0,
    }

    for manifest_key in sorted(library_only.keys()):
        entry = files[manifest_key]
        source_path = examples_dir / manifest_key

        # Check if source file exists on disk
        if not source_path.exists():
            if args.dry_run:
                print(f"  SKIP (missing): {manifest_key}")
            stats["skipped_missing"] += 1
            continue

        # Read source file content
        try:
            content = source_path.read_text(encoding="utf-8", errors="replace")
        except Exception as e:
            print(f"  ERROR reading {manifest_key}: {e}", file=sys.stderr)
            stats["skipped_missing"] += 1
            continue

        # Check for external dependencies
        if has_external_deps(content):
            if args.dry_run:
                print(f"  SKIP (external-deps): {manifest_key}")
            entry["harness_skip"] = "external-deps"
            stats["skipped_external"] += 1
            continue

        # Detect constructs
        constructs = detect_constructs(content)

        # Check for empty files (no constructs detected)
        if not has_any_constructs(constructs):
            if args.dry_run:
                print(f"  SKIP (empty): {manifest_key}")
            entry["harness_skip"] = "empty"
            stats["skipped_empty"] += 1
            continue

        # Generate the harness
        harness_content = generate_harness(manifest_key, constructs)
        harness_path = compute_harness_path(output_dir, manifest_key)

        # Compute the harness path relative to repo root for the manifest
        try:
            harness_relpath = str(harness_path.relative_to(repo_root))
        except ValueError:
            harness_relpath = str(harness_path)

        if args.dry_run:
            print(f"  GENERATE: {manifest_key}")
            print(f"    -> {harness_relpath}")
        else:
            harness_path.parent.mkdir(parents=True, exist_ok=True)
            harness_path.write_text(harness_content, encoding="utf-8")

        # Update manifest entry
        entry["harness"] = harness_relpath
        # Remove any previous skip marker if we are now generating
        entry.pop("harness_skip", None)

        stats["generated"] += 1

    # Save updated manifest
    if not args.dry_run:
        save_manifest(manifest_path, manifest)
        print(f"\nManifest updated: {manifest_path}")

    print(f"\nResults:")
    print(f"  Generated:        {stats['generated']}")
    print(f"  Skipped (ext):    {stats['skipped_external']}")
    print(f"  Skipped (empty):  {stats['skipped_empty']}")
    print(f"  Skipped (missing):{stats['skipped_missing']}")
    print(f"  Total:            {sum(stats.values())}")


if __name__ == "__main__":
    main()
