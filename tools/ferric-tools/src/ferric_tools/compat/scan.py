"""Static scanner for CLIPS compatibility assessment.

Scans all .clp files under tests/examples/ and produces a JSON manifest
classifying each file by detected features and ferric compatibility.
"""

from __future__ import annotations

import hashlib
from pathlib import Path
from typing import Annotated

import typer
from rich.console import Console

from ferric_tools._clips_parser import (
    COOL_CONSTRUCTS,
    INTERACTIVE_IO,
    LOADING_COMMANDS,
    UNSUPPORTED_CONTROL,
    UNSUPPORTED_IO,
    detect_features,
    strip_comments,
)
from ferric_tools._manifest import save_manifest, utc_now_iso
from ferric_tools._paths import examples_dir as default_examples_dir

app = typer.Typer(help="Scan CLIPS examples for compatibility assessment.")
console = Console(stderr=True)


def classify_file(path: Path, features: list[str], unsupported: list[str]) -> tuple[str, str, str]:
    """Pre-classify a file based on detected features.

    Returns (classification, reason, runability).
    """
    suffix = path.suffix.lower()

    if suffix == ".bat":
        return "incompatible", "test-suite-batch", "batch"

    cool_features = [f for f in unsupported if f in COOL_CONSTRUCTS]
    if cool_features:
        return "incompatible", "unsupported-form", "standalone"

    control_features = [f for f in unsupported if f in UNSUPPORTED_CONTROL]
    if control_features:
        return "incompatible", "unsupported-control", "standalone"

    io_features = [f for f in unsupported if f in UNSUPPORTED_IO]
    if io_features:
        return "incompatible", "unsupported-io", "standalone"

    interactive_features = [f for f in unsupported if f in INTERACTIVE_IO]
    if interactive_features:
        return "incompatible", "interactive", "interactive"

    loading_features = [f for f in unsupported if f in LOADING_COMMANDS]
    if loading_features:
        return "incompatible", "unsupported-command", "batch"

    if "defrule" not in features:
        return "pending", "library-only", "library"

    return "pending", "testable", "standalone"


def scan_examples(examples_path: Path) -> dict:
    """Scan all .clp and .bat files under examples_path."""
    files: dict[str, dict] = {}
    all_files = sorted(examples_path.rglob("*.clp")) + sorted(examples_path.rglob("*.bat"))

    for filepath in all_files:
        rel = filepath.relative_to(examples_path)
        rel_str = str(rel)
        source = rel.parts[0] if len(rel.parts) > 1 else ""

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

        cleaned = strip_comments(raw_content)
        features, unsupported = detect_features(cleaned)
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


def dedup_batch_files(files: dict, examples_path: Path) -> int:
    """Detect duplicate .bat files via content hashing."""
    hash_to_paths: dict[str, str] = {}

    for rel_path, info in sorted(files.items()):
        if info["reason"] != "test-suite-batch":
            continue
        filepath = examples_path / rel_path
        try:
            content = filepath.read_bytes()
            digest = hashlib.sha256(content).hexdigest()
        except OSError:
            continue

        if digest not in hash_to_paths:
            hash_to_paths[digest] = rel_path
        else:
            canonical = hash_to_paths[digest]
            info["classification"] = "incompatible"
            info["reason"] = "duplicate-batch"
            info["duplicate_of"] = canonical

    return sum(1 for info in files.values() if info.get("duplicate_of"))


def build_summary(files: dict) -> dict:
    """Compute summary counts from the files dict."""
    counts = {"total": 0, "equivalent": 0, "divergent": 0, "incompatible": 0, "pending": 0}
    for info in files.values():
        counts["total"] += 1
        cls = info["classification"]
        if cls in counts:
            counts[cls] += 1
    return counts


@app.command()
def main(
    examples_dir: Annotated[
        Path | None,
        typer.Option(help="Path to tests/examples directory (default: auto-detect)"),
    ] = None,
    output: Annotated[
        Path | None,
        typer.Option(help="Output manifest path (default: tests/examples/compat-manifest.json)"),
    ] = None,
) -> None:
    """Scan CLIPS examples for compatibility assessment."""
    examples_path = Path(examples_dir) if examples_dir else default_examples_dir()
    output_path = output or (examples_path / "compat-manifest.json")

    if not examples_path.is_dir():
        console.print(f"[red]error:[/] examples directory not found: {examples_path}")
        raise typer.Exit(1)

    console.print(f"Scanning {examples_path} ...")
    files = scan_examples(examples_path)
    dup_count = dedup_batch_files(files, examples_path)
    summary = build_summary(files)

    manifest = {
        "version": 1,
        "generated": utc_now_iso(),
        "summary": summary,
        "files": files,
    }

    save_manifest(output_path, manifest)

    print(f"\nManifest written to {output_path}")
    print("\nSummary:")
    print(f"  Total files:    {summary['total']}")
    print(f"  Pending (testable): {summary['pending']}")
    print(f"  Incompatible:   {summary['incompatible']}")
    print(f"  Equivalent:     {summary['equivalent']}")
    print(f"  Divergent:      {summary['divergent']}")
    if dup_count:
        print(f"  Duplicate .bat:  {dup_count}")

    reason_counts: dict[str, int] = {}
    for info in files.values():
        if info["classification"] == "incompatible":
            reason = info["reason"]
            reason_counts[reason] = reason_counts.get(reason, 0) + 1
    if reason_counts:
        print("\n  Incompatible breakdown:")
        for reason, count in sorted(reason_counts.items(), key=lambda x: -x[1]):
            print(f"    {reason:25s}: {count}")


if __name__ == "__main__":
    app()
