"""Extract standalone .clp segments from CLIPS test-suite .bat files."""

from __future__ import annotations

from collections import Counter
from pathlib import Path
from typing import Annotated

import typer
from rich.console import Console

from ferric_tools._clips_parser import extract_top_level_forms, first_keyword
from ferric_tools._manifest import load_manifest, save_manifest
from ferric_tools._paths import examples_dir as default_examples_dir
from ferric_tools._paths import repo_root

app = typer.Typer(help="Extract standalone .clp segments from .bat analysis.")
console = Console(stderr=True)

STRIP_KEYWORDS = {"watch", "unwatch", "reset", "run"}


def split_into_cycles(text: str) -> list[list[tuple[str, str]]]:
    """Split *text* into (clear)-delimited cycles."""
    forms = extract_top_level_forms(text)
    cycles: list[list[tuple[str, str]]] = []
    current_forms: list[tuple[str, str]] = []

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


def extract_cycle(cycle_forms: list[tuple[str, str]]) -> str | None:
    """Extract .clp content from an extractable cycle, stripping control commands."""
    kept = [form_text for form_text, kw in cycle_forms if kw not in STRIP_KEYWORDS]
    if not kept:
        return None
    return "\n".join(kept) + "\n"


@app.command()
def main(
    analysis: Annotated[
        Path | None,
        typer.Option(help="Path to bat-analysis.json"),
    ] = None,
    examples_dir: Annotated[
        Path | None,
        typer.Option(help="Path to tests/examples directory"),
    ] = None,
    output_dir: Annotated[
        Path | None,
        typer.Option(help="Output directory for .clp files"),
    ] = None,
    manifest_opt: Annotated[
        Path | None,
        typer.Option("--manifest", help="Path to compat-manifest.json to update"),
    ] = None,
) -> None:
    """Extract standalone .clp segments from .bat analysis."""
    root = repo_root()
    examples_path = Path(examples_dir) if examples_dir else default_examples_dir()
    analysis_path = analysis or (examples_path / "bat-analysis.json")
    out_path = output_dir or (root / "tests" / "generated" / "test-suite-segments")
    manifest_path = manifest_opt or (examples_path / "compat-manifest.json")

    if not Path(analysis_path).is_file():
        console.print(f"[red]error:[/] analysis file not found: {analysis_path}")
        console.print("Run ferric-bat-analyze first.")
        raise typer.Exit(1)

    if not examples_path.is_dir():
        console.print(f"[red]error:[/] examples directory not found: {examples_path}")
        raise typer.Exit(1)

    analysis_data = load_manifest(analysis_path)

    manifest = None
    if Path(manifest_path).is_file():
        manifest = load_manifest(manifest_path)
    else:
        console.print(f"[yellow]warning:[/] manifest not found: {manifest_path}")

    out_path = Path(out_path)
    out_path.mkdir(parents=True, exist_ok=True)

    extractable_files: list[tuple[str, list[int]]] = []
    for rel_path, file_report in sorted(analysis_data["files"].items()):
        if "error" in file_report:
            continue
        cycles_info = file_report.get("cycles", [])
        extractable_indices = [c["index"] for c in cycles_info if c["extractable"]]
        if extractable_indices:
            extractable_files.append((rel_path, extractable_indices))

    stem_counts = Counter(Path(rp).stem for rp, _ in extractable_files)
    colliding_stems = {s for s, c in stem_counts.items() if c > 1}

    def _make_output_stem(rel_path: str) -> str:
        bat_stem = Path(rel_path).stem
        if bat_stem not in colliding_stems:
            return bat_stem
        parts = Path(rel_path).parts
        tag_parts = [p for p in parts[:-1] if p not in ("test_suite", "examples")]
        tag = "-".join(tag_parts) if tag_parts else "unknown"
        tag = tag.replace("clips-official", "co")
        tag = tag.replace("telefonica-clips-branches-", "t")
        tag = tag.replace("telefonica-clips", "tc")
        tag = tag.replace("branches-", "")
        tag = tag.replace("/", "-")
        return f"{tag}-{bat_stem}"

    extracted_count = 0
    skipped_empty = 0
    manifest_entries: dict[str, dict] = {}

    for rel_path, extractable_indices in extractable_files:
        bat_path = examples_path / rel_path
        try:
            text = bat_path.read_text(encoding="utf-8", errors="replace")
        except OSError as e:
            console.print(f"[yellow]warning:[/] cannot read {bat_path}: {e}")
            continue

        cycles = split_into_cycles(text)
        output_stem = _make_output_stem(rel_path)
        source_parts = Path(rel_path).parts
        source = source_parts[0] if len(source_parts) > 1 else ""

        for idx in extractable_indices:
            if idx >= len(cycles):
                continue

            content = extract_cycle(cycles[idx])
            if content is None or content.strip() == "":
                skipped_empty += 1
                continue

            out_filename = f"{output_stem}-{idx:02d}.clp"
            out_file = out_path / out_filename
            out_file.write_text(content, encoding="utf-8")
            extracted_count += 1

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

    if manifest is not None and manifest_entries:
        to_remove = [k for k in manifest["files"] if k.startswith("generated/test-suite-segments/")]
        for k in to_remove:
            del manifest["files"][k]

        manifest["files"].update(manifest_entries)

        counts = {"total": 0, "equivalent": 0, "divergent": 0, "incompatible": 0, "pending": 0}
        for info in manifest["files"].values():
            counts["total"] += 1
            cls = info.get("classification", "")
            if cls in counts:
                counts[cls] += 1
        manifest["summary"] = counts

        save_manifest(manifest_path, manifest)

        print(f"Manifest updated: {manifest_path}")
        print(f"  Added {len(manifest_entries)} entries (removed {len(to_remove)} stale)")

    print("\nExtraction complete.")
    print(f"  Output directory: {out_path}")
    print(f"  Segments extracted: {extracted_count}")
    if skipped_empty:
        print(f"  Empty cycles skipped: {skipped_empty}")


if __name__ == "__main__":
    app()
