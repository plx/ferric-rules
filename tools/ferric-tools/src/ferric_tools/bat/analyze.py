"""Analyze CLIPS test-suite .bat files for extractable segments."""

from __future__ import annotations

from collections import defaultdict
from pathlib import Path
from typing import Annotated

import typer
from rich.console import Console

from ferric_tools._clips_parser import (
    classify_keyword,
    extract_top_level_forms,
    first_keyword,
)
from ferric_tools._manifest import load_manifest, save_manifest
from ferric_tools._paths import examples_dir as default_examples_dir

app = typer.Typer(help="Analyze CLIPS .bat test files for extractable segments.")
console = Console(stderr=True)

# Benchmark directories to skip
BENCHMARK_SKIP_SEGMENTS = {"manners", "waltz", "sudoku"}


def split_into_cycles(text: str) -> list[dict]:
    """Split *text* into (clear)-delimited cycles."""
    forms = extract_top_level_forms(text)

    cycles: list[dict] = []
    current_forms: list[tuple[str, str]] = []
    current_label: str | None = None

    for form_text, start_offset in forms:
        kw = first_keyword(form_text)
        if kw == "clear":
            if current_forms or cycles:
                cycles.append({"label": current_label, "forms": current_forms})
            current_forms = []
            current_label = _extract_clear_label(text, start_offset)
        else:
            current_forms.append((form_text, kw))

    if current_forms:
        cycles.append({"label": current_label, "forms": current_forms})

    return cycles


def _extract_clear_label(text: str, clear_start: int) -> str | None:
    """Look for a comment on the (clear) line for use as a label."""
    depth = 0
    i = clear_start
    n = len(text)
    while i < n:
        if text[i] == "(":
            depth += 1
        elif text[i] == ")":
            depth -= 1
            if depth == 0:
                i += 1
                break
        i += 1

    while i < n and text[i] in (" ", "\t"):
        i += 1

    if i < n and text[i] == ";":
        j = i + 1
        while j < n and text[j] != "\n":
            j += 1
        label = text[i + 1 : j].strip()
        if label:
            return label

    return None


def classify_cycle(cycle: dict) -> tuple[bool, bool, set[str]]:
    """Determine whether a cycle is extractable.

    Returns (extractable, has_cool, repl_cmds).
    """
    has_cool = False
    repl_cmds: set[str] = set()
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


def should_skip_file(rel_path: str) -> bool:
    """Return True if the .bat file is a benchmark we should skip."""
    parts = Path(rel_path).parts
    return any(seg in parts for seg in BENCHMARK_SKIP_SEGMENTS)


def analyze_file(examples_path: Path, rel_path: str) -> dict:
    """Analyze a single .bat file."""
    filepath = examples_path / rel_path

    try:
        text = filepath.read_text(encoding="utf-8", errors="replace")
    except OSError as e:
        return {"error": str(e)}

    cycles = split_into_cycles(text)

    file_has_cool = False
    all_repl_cmds: set[str] = set()
    cycle_reports: list[dict] = []

    for idx, cycle in enumerate(cycles):
        extractable, has_cool, repl_cmds = classify_cycle(cycle)
        if has_cool:
            file_has_cool = True
        all_repl_cmds |= repl_cmds

        commands = [kw for _ft, kw in cycle["forms"]]
        cycle_reports.append(
            {
                "index": idx,
                "extractable": extractable,
                "commands": commands,
                "label": cycle["label"],
            }
        )

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


@app.command()
def main(
    examples_dir: Annotated[
        Path | None, typer.Option(help="Path to tests/examples directory")
    ] = None,
    manifest_opt: Annotated[
        Path | None, typer.Option("--manifest", help="Path to compat-manifest.json")
    ] = None,
    output: Annotated[Path | None, typer.Option(help="Output analysis JSON path")] = None,
) -> None:
    """Analyze CLIPS .bat test files for extractable segments."""
    examples_path = Path(examples_dir) if examples_dir else default_examples_dir()
    manifest_path = manifest_opt or (examples_path / "compat-manifest.json")
    output_path = output or (examples_path / "bat-analysis.json")

    if not examples_path.is_dir():
        console.print(f"[red]error:[/] examples directory not found: {examples_path}")
        raise typer.Exit(1)

    if not Path(manifest_path).is_file():
        console.print(f"[red]error:[/] manifest not found: {manifest_path}")
        raise typer.Exit(1)

    mdata = load_manifest(manifest_path)

    bat_files: list[str] = []
    for rel_path, info in sorted(mdata["files"].items()):
        if not rel_path.endswith(".bat"):
            continue
        if info.get("reason") == "duplicate-batch":
            continue
        if should_skip_file(rel_path):
            continue
        bat_files.append(rel_path)

    print(f"Analyzing {len(bat_files)} unique non-benchmark .bat files ...")

    file_reports: dict[str, dict] = {}
    total_extractable = 0
    total_non_extractable = 0
    files_with_extractable = 0
    repl_cmd_counts: dict[str, int] = defaultdict(int)

    for rel_path in bat_files:
        report = analyze_file(examples_path, rel_path)
        file_reports[rel_path] = report

        if "error" in report:
            continue

        if report["extractable_cycles"] > 0:
            files_with_extractable += 1
        total_extractable += report["extractable_cycles"]
        total_non_extractable += report["non_extractable_cycles"]

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
        "repl_commands_needed": dict(sorted(repl_cmd_counts.items(), key=lambda x: -x[1])),
    }

    result = {"files": file_reports, "summary": summary}

    save_manifest(output_path, result)

    print(f"\nAnalysis written to {output_path}")
    print("\nSummary:")
    print(f"  Total files analysed:           {summary['total_files']}")
    print(f"  Files with extractable cycles:  {summary['files_with_extractable_cycles']}")
    print(f"  Total extractable cycles:       {summary['total_extractable_cycles']}")
    print(f"  Total non-extractable cycles:   {summary['total_non_extractable_cycles']}")

    if repl_cmd_counts:
        print("\n  REPL commands blocking extraction (by occurrence):")
        for cmd, count in sorted(repl_cmd_counts.items(), key=lambda x: -x[1]):
            print(f"    {cmd:30s}: {count}")


if __name__ == "__main__":
    app()
