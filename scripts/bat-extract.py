#!/usr/bin/env python3
"""Extract standalone .clp segments from CLIPS test-suite .bat files.

Reads the analysis JSON produced by bat-analyze.py and extracts each
extractable cycle as an independent .clp file.  Constructs are kept as-is;
(watch)/(unwatch)/(reset)/(run) are stripped (ferric does load->reset->run
automatically).

Usage:
    python scripts/bat-extract.py [--analysis FILE] [--examples-dir DIR]
                                  [--output-dir DIR] [--manifest FILE]
"""

import argparse
import json
import sys
from collections import Counter
from pathlib import Path


# ---------------------------------------------------------------------------
# Keywords to strip during extraction
# ---------------------------------------------------------------------------

STRIP_KEYWORDS = {
    "watch", "unwatch", "reset", "run",
}

CONSTRUCT_KEYWORDS = {
    "defrule", "deftemplate", "deffacts", "deffunction",
    "defglobal", "defmodule", "defgeneric", "defmethod",
}


# ---------------------------------------------------------------------------
# Top-level form extraction (same logic as bat-analyze.py)
# ---------------------------------------------------------------------------

def extract_top_level_forms(text):
    """Extract top-level parenthesised forms from *text*.

    Returns a list of (form_text, start_offset) tuples.
    """
    forms = []
    depth = 0
    in_string = False
    i = 0
    n = len(text)
    form_start = None

    while i < n:
        ch = text[i]

        if in_string:
            if ch == '\\' and i + 1 < n:
                i += 2
                continue
            if ch == '"':
                in_string = False
            i += 1
            continue

        if ch == '"':
            in_string = True
            i += 1
            continue

        if ch == ';':
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
    """Return the first symbol after the opening paren of *form_text*."""
    i = 0
    n = len(form_text)
    while i < n and form_text[i] != '(':
        i += 1
    i += 1
    while i < n and form_text[i] in (' ', '\t', '\n', '\r'):
        i += 1
    start = i
    while i < n and form_text[i] not in (' ', '\t', '\n', '\r', ')', '(', '"'):
        i += 1
    return form_text[start:i].lower()


# ---------------------------------------------------------------------------
# Cycle splitting (same logic as bat-analyze.py)
# ---------------------------------------------------------------------------

def split_into_cycles(text):
    """Split *text* into (clear)-delimited cycles.

    Returns a list of lists, where each inner list is
    [(form_text, keyword), ...] for that cycle.
    """
    forms = extract_top_level_forms(text)

    cycles = []
    current_forms = []

    for form_text, _start_offset in forms:
        kw = first_keyword(form_text)
        if kw == "clear":
            if current_forms or cycles:
                cycles.append(current_forms)
            current_forms = []
        else:
            current_forms.append((form_text, kw))

    if current_forms:
        cycles.append(current_forms)

    return cycles


# ---------------------------------------------------------------------------
# Extraction
# ---------------------------------------------------------------------------

def extract_cycle(cycle_forms):
    """Given a list of (form_text, keyword) for an extractable cycle,
    return the .clp content string with watch/unwatch/reset/run stripped.
    """
    kept = []
    for form_text, kw in cycle_forms:
        if kw in STRIP_KEYWORDS:
            continue
        kept.append(form_text)

    if not kept:
        return None

    return "\n".join(kept) + "\n"


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main():
    parser = argparse.ArgumentParser(
        description="Extract standalone .clp segments from .bat analysis"
    )
    parser.add_argument(
        "--analysis",
        default=None,
        help="Path to bat-analysis.json (default: tests/examples/bat-analysis.json)",
    )
    parser.add_argument(
        "--examples-dir",
        default=None,
        help="Path to tests/examples directory (default: auto-detect)",
    )
    parser.add_argument(
        "--output-dir",
        default=None,
        help="Output directory for .clp files (default: tests/generated/test-suite-segments)",
    )
    parser.add_argument(
        "--manifest",
        default=None,
        help="Path to compat-manifest.json to update (default: tests/examples/compat-manifest.json)",
    )
    args = parser.parse_args()

    # Resolve repo root
    script_dir = Path(__file__).resolve().parent
    repo_root = script_dir.parent

    analysis_path = args.analysis or str(
        repo_root / "tests" / "examples" / "bat-analysis.json"
    )
    examples_dir = args.examples_dir or str(repo_root / "tests" / "examples")
    output_dir = args.output_dir or str(
        repo_root / "tests" / "generated" / "test-suite-segments"
    )
    manifest_path = args.manifest or str(
        repo_root / "tests" / "examples" / "compat-manifest.json"
    )

    if not Path(analysis_path).is_file():
        print(f"error: analysis file not found: {analysis_path}", file=sys.stderr)
        print("Run bat-analyze.py first.", file=sys.stderr)
        sys.exit(1)

    if not Path(examples_dir).is_dir():
        print(f"error: examples directory not found: {examples_dir}", file=sys.stderr)
        sys.exit(1)

    # Load analysis
    with open(analysis_path, encoding="utf-8") as f:
        analysis = json.load(f)

    # Load manifest
    if Path(manifest_path).is_file():
        with open(manifest_path, encoding="utf-8") as f:
            manifest = json.load(f)
    else:
        print(f"warning: manifest not found: {manifest_path}", file=sys.stderr)
        manifest = None

    # Create output directory
    out_path = Path(output_dir)
    out_path.mkdir(parents=True, exist_ok=True)

    # Build a mapping from rel_path -> output_stem that avoids collisions.
    # When multiple .bat files share the same stem (e.g. rulemisc.bat from
    # different source trees), we prefix with a short disambiguator derived
    # from the source path.
    extractable_files = []
    for rel_path, file_report in sorted(analysis["files"].items()):
        if "error" in file_report:
            continue
        cycles_info = file_report.get("cycles", [])
        extractable_indices = [c["index"] for c in cycles_info if c["extractable"]]
        if extractable_indices:
            extractable_files.append((rel_path, extractable_indices))

    # Detect stem collisions
    stem_counts = Counter(Path(rp).stem for rp, _ in extractable_files)
    colliding_stems = {s for s, c in stem_counts.items() if c > 1}

    def _make_output_stem(rel_path):
        """Build a unique output stem for *rel_path*.

        For non-colliding stems, just use the .bat filename stem.
        For colliding stems, prepend a short source tag derived from the
        relative path (e.g. 'clips-official' -> 'co', 'telefonica-63x' -> 't63x').
        """
        bat_stem = Path(rel_path).stem
        if bat_stem not in colliding_stems:
            return bat_stem
        parts = Path(rel_path).parts
        # Build a short tag from path components before the filename.
        # clips-official/test_suite/foo.bat -> "co"
        # telefonica-clips/branches/63x/test_suite/foo.bat -> "t63x"
        # telefonica-clips/branches/64x/test_suite/foo.bat -> "t64x"
        # telefonica-clips/branches/65x/test_suite/foo.bat -> "t65x"
        tag_parts = []
        for p in parts[:-1]:
            if p in ("test_suite", "examples"):
                continue
            tag_parts.append(p)
        tag = "-".join(tag_parts) if tag_parts else "unknown"
        # Shorten common prefixes
        tag = tag.replace("clips-official", "co")
        tag = tag.replace("telefonica-clips-branches-", "t")
        tag = tag.replace("telefonica-clips", "tc")
        tag = tag.replace("branches-", "")
        tag = tag.replace("/", "-")
        return f"{tag}-{bat_stem}"

    extracted_count = 0
    skipped_empty = 0
    manifest_entries = {}

    for rel_path, extractable_indices in extractable_files:
        # Read the source .bat file and re-parse to get actual form text
        bat_path = Path(examples_dir) / rel_path
        try:
            text = bat_path.read_text(encoding="utf-8", errors="replace")
        except OSError as e:
            print(f"warning: cannot read {bat_path}: {e}", file=sys.stderr)
            continue

        cycles = split_into_cycles(text)

        # Determine the disambiguated stem for output filenames
        output_stem = _make_output_stem(rel_path)

        # Determine source directory from the .bat relative path
        source_parts = Path(rel_path).parts
        source = source_parts[0] if len(source_parts) > 1 else ""

        for idx in extractable_indices:
            if idx >= len(cycles):
                continue

            content = extract_cycle(cycles[idx])
            if content is None or content.strip() == "":
                skipped_empty += 1
                continue

            # Format cycle index as two digits
            out_filename = f"{output_stem}-{idx:02d}.clp"
            out_file = out_path / out_filename
            out_file.write_text(content, encoding="utf-8")
            extracted_count += 1

            # Build manifest entry key relative to tests/
            manifest_key = f"generated/test-suite-segments/{out_filename}"

            manifest_entries[manifest_key] = {
                "source": source,
                "classification": "pending",
                "reason": "extracted-segment",
                "runability": "standalone",
                "features": [],
                "unsupported_features": [],
                "ferric": None,
                "clips": None,
                "notes": "",
                "extracted_from": rel_path,
                "cycle_index": idx,
            }

    # Update manifest if it was loaded
    if manifest is not None and manifest_entries:
        # Remove any previously-generated entries from prior runs
        to_remove = [
            k for k in manifest["files"]
            if k.startswith("generated/test-suite-segments/")
        ]
        for k in to_remove:
            del manifest["files"][k]

        manifest["files"].update(manifest_entries)

        # Recompute summary counts
        counts = {"total": 0, "equivalent": 0, "divergent": 0, "incompatible": 0, "pending": 0}
        for info in manifest["files"].values():
            counts["total"] += 1
            cls = info.get("classification", "")
            if cls in counts:
                counts[cls] += 1
        manifest["summary"] = counts

        with open(manifest_path, "w", encoding="utf-8") as f:
            json.dump(manifest, f, indent=2, ensure_ascii=False)
            f.write("\n")

        print(f"Manifest updated: {manifest_path}")
        print(f"  Added {len(manifest_entries)} entries (removed {len(to_remove)} stale)")

    print(f"\nExtraction complete.")
    print(f"  Output directory: {output_dir}")
    print(f"  Segments extracted: {extracted_count}")
    if skipped_empty:
        print(f"  Empty cycles skipped: {skipped_empty}")


if __name__ == "__main__":
    main()
