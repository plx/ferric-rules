#!/usr/bin/env python3
"""Convert benchmark .bat files into self-contained .clp files.

Parses CLIPS benchmark .bat files (which use (load ...) and (load-facts ...)
commands to reference companion files), and produces self-contained .clp files
that inline all loaded rule files and wrap loaded fact files in (deffacts ...)
blocks.

Processes clips-official benchmarks only.  Telefonica duplicates are noted in
the manifest but not re-generated (they contain identical .clp and .fct files).

Usage:
    python scripts/bat-convert.py [options]

Options:
    --examples-dir DIR   Base directory for source files
                         (default: tests/examples/ relative to repo root)
    --output-dir DIR     Where to write generated .clp files
                         (default: tests/generated/benchmarks/)
    --manifest FILE      Path to compat-manifest.json
                         (default: tests/examples/compat-manifest.json)
    --dry-run            Show what would be done without writing files
"""

import argparse
import json
import os
import re
import sys
from datetime import datetime, timezone
from pathlib import Path


# ---------------------------------------------------------------------------
# Repo root detection
# ---------------------------------------------------------------------------

def find_repo_root():
    """Walk up from this script's location to find the repo root (has Cargo.toml)."""
    here = Path(__file__).resolve().parent
    for ancestor in [here] + list(here.parents):
        if (ancestor / "Cargo.toml").exists():
            return ancestor
    # Fallback: assume scripts/ is one level below root
    return here.parent


REPO_ROOT = find_repo_root()

# Benchmark directories relative to the examples-dir.
# Each entry is (relative_dir, source_label).
BENCHMARK_DIRS = [
    ("clips-official/examples/manners", "clips-official"),
    ("clips-official/examples/waltz", "clips-official"),
    ("clips-official/examples/sudoku", "clips-official"),
]

# Telefonica duplicate directories that mirror clips-official benchmarks.
TELEFONICA_MIRRORS = [
    "telefonica-clips/branches/63x/examples/manners",
    "telefonica-clips/branches/63x/examples/waltz",
    "telefonica-clips/branches/63x/examples/sudoku",
    "telefonica-clips/branches/64x/examples/manners",
    "telefonica-clips/branches/64x/examples/waltz",
    "telefonica-clips/branches/64x/examples/sudoku",
]

# Commands in .bat files that we ignore (they're session-level, not content).
IGNORED_COMMANDS = {"clear", "unwatch", "watch", "set-strategy", "reset", "run"}


# ---------------------------------------------------------------------------
# .bat file parsing
# ---------------------------------------------------------------------------

def parse_bat_file(bat_path):
    """Parse a .bat file and extract load/load-facts commands plus any extras.

    Returns a dict:
        {
            "loads": [filename, ...],       # from (load ...) commands
            "load_facts": [filename, ...],  # from (load-facts ...) commands
            "extras": [line, ...],          # unrecognised commands (e.g. assert)
        }
    """
    loads = []
    load_facts = []
    extras = []

    text = bat_path.read_text(encoding="utf-8", errors="replace")

    # Match top-level parenthesised commands.  We don't need a full CLIPS
    # parser -- .bat files are simple sequences of top-level forms.
    # Pattern: opening paren, command word, optional args, closing paren.
    for m in re.finditer(r'\(([^()]*(?:\([^()]*\))*[^()]*)\)', text):
        body = m.group(1).strip()
        if not body:
            continue

        parts = body.split(None, 1)
        cmd = parts[0].lower()
        arg = parts[1].strip() if len(parts) > 1 else ""

        if cmd in IGNORED_COMMANDS:
            continue
        elif cmd == "load":
            # Strip optional surrounding quotes and normalise path separators.
            filename = arg.strip('"').strip("'").replace("//", "/")
            loads.append(filename)
        elif cmd == "load-facts":
            filename = arg.strip('"').strip("'").replace("//", "/")
            load_facts.append(filename)
        else:
            # Preserve unrecognised commands (e.g. (assert ...)).
            extras.append(m.group(0))

    return {"loads": loads, "load_facts": load_facts, "extras": extras}


# ---------------------------------------------------------------------------
# Self-contained .clp generation
# ---------------------------------------------------------------------------

def build_combined_clp(bat_path, parsed, bat_dir):
    """Build the text of a self-contained .clp file from parsed bat data.

    Returns (clp_text, warnings) where warnings is a list of strings.
    Returns (None, warnings) if a critical referenced file is missing.
    """
    warnings = []
    sections = []

    # Header
    rel = bat_path.relative_to(REPO_ROOT) if bat_path.is_relative_to(REPO_ROOT) else bat_path
    sections.append(
        f";;; =================================================================\n"
        f";;; Auto-generated self-contained benchmark from {rel}\n"
        f";;; Generated by scripts/bat-convert.py\n"
        f";;; =================================================================\n"
    )

    # Inline each (load ...) file
    for filename in parsed["loads"]:
        file_path = (bat_dir / filename).resolve()
        if not file_path.exists():
            warnings.append(f"Referenced file not found: {filename} (from {bat_path.name})")
            return None, warnings
        content = file_path.read_text(encoding="utf-8", errors="replace")
        sections.append(
            f"\n;;; --- Inlined from {filename} ---\n\n"
            f"{content.rstrip()}\n"
        )

    # Wrap each (load-facts ...) file in a deffacts block
    fact_count = len(parsed["load_facts"])
    for i, filename in enumerate(parsed["load_facts"]):
        file_path = (bat_dir / filename).resolve()
        if not file_path.exists():
            warnings.append(f"Referenced file not found: {filename} (from {bat_path.name})")
            return None, warnings
        content = file_path.read_text(encoding="utf-8", errors="replace")

        if fact_count == 1:
            deffacts_name = "loaded-data"
        else:
            deffacts_name = f"loaded-data-{i + 1}"

        # Indent fact file contents by 2 spaces inside the deffacts block.
        indented = "\n".join(
            f"  {line}" if line.strip() else ""
            for line in content.rstrip().split("\n")
        )
        sections.append(
            f"\n;;; --- Facts from {filename} ---\n\n"
            f"(deffacts {deffacts_name}\n"
            f"{indented}\n"
            f")\n"
        )

    # Include any extra commands as comments + raw forms
    if parsed["extras"]:
        sections.append("\n;;; --- Additional commands from batch file ---\n")
        for extra in parsed["extras"]:
            sections.append(f"\n;;; NOTE: The following command was in the .bat file.\n")
            sections.append(f";;; It may need manual review.\n")
            sections.append(f";;; {extra}\n")

    return "\n".join(sections), warnings


# ---------------------------------------------------------------------------
# Manifest update
# ---------------------------------------------------------------------------

def update_manifest(manifest_path, updates, dry_run=False):
    """Apply updates to the manifest.

    updates is a list of dicts:
        {"key": manifest_key, "generated": relative_path_to_generated_file}
    """
    if not manifest_path.exists():
        print(f"  WARNING: Manifest not found at {manifest_path}, skipping update")
        return

    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    files = manifest.get("files", {})
    changed = False

    for upd in updates:
        key = upd["key"]
        if key not in files:
            print(f"  WARNING: Manifest key not found: {key}")
            continue
        entry = files[key]
        entry["generated"] = upd["generated"]
        entry["classification"] = "pending"
        entry["reason"] = "benchmark-converted"
        changed = True

    if changed:
        # Recount summary
        summary = {"total": 0, "equivalent": 0, "divergent": 0, "incompatible": 0, "pending": 0}
        for entry in files.values():
            summary["total"] += 1
            cls = entry.get("classification", "pending")
            if cls in summary:
                summary[cls] += 1
        manifest["summary"] = summary
        manifest["generated"] = datetime.now(timezone.utc).isoformat()

        if dry_run:
            print(f"  Would update manifest with {len(updates)} entries")
        else:
            manifest_path.write_text(
                json.dumps(manifest, indent=2, ensure_ascii=False) + "\n",
                encoding="utf-8",
            )
            print(f"  Updated manifest ({len(updates)} entries)")


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main():
    parser = argparse.ArgumentParser(
        description="Convert benchmark .bat files into self-contained .clp files."
    )
    parser.add_argument(
        "--examples-dir",
        type=Path,
        default=None,
        help="Base directory for source files (default: tests/examples/ relative to repo root)",
    )
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=None,
        help="Where to write generated .clp files (default: tests/generated/benchmarks/)",
    )
    parser.add_argument(
        "--manifest",
        type=Path,
        default=None,
        help="Path to compat-manifest.json (default: tests/examples/compat-manifest.json)",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Show what would be done without writing files",
    )
    args = parser.parse_args()

    examples_dir = args.examples_dir or (REPO_ROOT / "tests" / "examples")
    output_dir = args.output_dir or (REPO_ROOT / "tests" / "generated" / "benchmarks")
    manifest_path = args.manifest or (examples_dir / "compat-manifest.json")

    examples_dir = examples_dir.resolve()
    output_dir = output_dir.resolve()
    manifest_path = manifest_path.resolve()

    print(f"Examples dir:  {examples_dir}")
    print(f"Output dir:    {output_dir}")
    print(f"Manifest:      {manifest_path}")
    print(f"Dry run:       {args.dry_run}")
    print()

    manifest_updates = []
    generated_count = 0
    skipped_count = 0
    warning_count = 0

    for bench_rel, source_label in BENCHMARK_DIRS:
        bench_dir = examples_dir / bench_rel
        if not bench_dir.is_dir():
            print(f"  WARNING: Benchmark directory not found: {bench_dir}")
            warning_count += 1
            continue

        bat_files = sorted(bench_dir.glob("*.bat"))
        if not bat_files:
            print(f"  No .bat files in {bench_dir}")
            continue

        for bat_path in bat_files:
            bat_stem = bat_path.stem
            print(f"Processing: {bat_path.relative_to(examples_dir)}")

            parsed = parse_bat_file(bat_path)
            if not parsed["loads"] and not parsed["load_facts"]:
                print(f"  SKIP: No load or load-facts commands found")
                skipped_count += 1
                continue

            clp_text, warnings = build_combined_clp(bat_path, parsed, bench_dir)

            for w in warnings:
                print(f"  WARNING: {w}")
                warning_count += 1

            if clp_text is None:
                print(f"  SKIP: Missing referenced file(s)")
                skipped_count += 1
                continue

            # Determine output path.
            dest_dir = output_dir / source_label
            dest_path = dest_dir / f"{bat_stem}.clp"

            if args.dry_run:
                print(f"  Would write: {dest_path.relative_to(REPO_ROOT)}")
                lines = clp_text.count("\n")
                print(f"  ({lines} lines, {len(parsed['loads'])} load(s), "
                      f"{len(parsed['load_facts'])} load-facts, "
                      f"{len(parsed['extras'])} extra(s))")
            else:
                dest_dir.mkdir(parents=True, exist_ok=True)
                dest_path.write_text(clp_text, encoding="utf-8")
                print(f"  Wrote: {dest_path.relative_to(REPO_ROOT)}")

            generated_count += 1

            # Track manifest update for this clips-official bat entry.
            manifest_key = str(bat_path.relative_to(examples_dir))
            generated_rel = str(dest_path.relative_to(REPO_ROOT))
            manifest_updates.append({
                "key": manifest_key,
                "generated": generated_rel,
            })

    # Update manifest
    print()
    if manifest_updates:
        update_manifest(manifest_path, manifest_updates, dry_run=args.dry_run)

    # Summary
    print()
    print(f"Summary:")
    print(f"  Generated:  {generated_count}")
    print(f"  Skipped:    {skipped_count}")
    print(f"  Warnings:   {warning_count}")

    if warning_count > 0:
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
