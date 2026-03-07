"""Run `ferric check` across extracted test-suite segments with expectations."""

from __future__ import annotations

import fnmatch
import json
import subprocess
from pathlib import Path
from typing import Annotated

import typer
from rich.console import Console

from ferric_tools._manifest import save_manifest
from ferric_tools._subprocess import parallel_run

app = typer.Typer(help="Sweep generated segments with expected-failure classification.")
console = Console(stderr=True)


def load_expectations(path: Path) -> dict:
    if not path.exists():
        return {"entries": {}, "groups": []}
    with path.open("r", encoding="utf-8") as f:
        payload = json.load(f)
    entries = payload.get("entries", {})
    groups = payload.get("groups", [])
    if not isinstance(entries, dict):
        entries = {}
    if not isinstance(groups, list):
        groups = []
    return {"entries": entries, "groups": groups}


def run_check(file_path: Path, ferric_cmd: list[str]) -> tuple[int, str]:
    cmd = [*ferric_cmd, str(file_path)]
    proc = subprocess.run(cmd, capture_output=True, text=True)
    stderr = (proc.stderr or "").strip()
    stdout = (proc.stdout or "").strip()
    combined = stderr if stderr else stdout
    return proc.returncode, combined


def summarize_signature(diagnostic: str) -> str:
    if not diagnostic:
        return ""
    first = diagnostic.splitlines()[0]
    marker = " at line "
    idx = first.find(marker)
    if idx >= 0:
        return first[:idx]
    return first


def resolve_expectation(filename: str, expectations: dict) -> dict | None:
    entries = expectations.get("entries", {})
    if filename in entries:
        return entries[filename]

    for group in expectations.get("groups", []):
        pattern = group.get("glob")
        if isinstance(pattern, str) and fnmatch.fnmatch(filename, pattern):
            return group
    return None


def classify_result(
    filename: str, exit_code: int, diagnostic: str, expectation: dict | None
) -> str:
    failed = exit_code != 0
    if expectation is None:
        return "unexpected_fail" if failed else "pass"

    contains = expectation.get("contains", [])
    if not isinstance(contains, list):
        contains = []
    matched = all(token in diagnostic for token in contains)

    if failed and matched:
        return "expected_fail"
    if failed and not matched:
        return "expected_mismatch"
    if not failed:
        return "unexpected_pass"
    return "unexpected_fail"


def _worker(args: tuple) -> tuple[str, int, str]:
    file_path, ferric_cmd = args
    exit_code, diagnostic = run_check(file_path, ferric_cmd)
    return file_path.name, exit_code, diagnostic


@app.command()
def main(
    segments_dir: Annotated[
        str,
        typer.Option(help="Directory containing extracted segment .clp files"),
    ] = "tests/generated/test-suite-segments",
    expectations: Annotated[
        str,
        typer.Option(help="JSON file containing expected failing segment diagnostics"),
    ] = "tests/generated/segment-check-expectations.json",
    workers: Annotated[int, typer.Option(help="Parallel worker count")] = 4,
    json_out: Annotated[
        str | None,
        typer.Option(help="Optional JSON output path for detailed results"),
    ] = None,
    ferric_bin: Annotated[
        str | None,
        typer.Option(help="Path to ferric binary"),
    ] = None,
) -> None:
    """Sweep generated segments with expected-failure classification."""
    seg_dir = Path(segments_dir)
    if not seg_dir.is_dir():
        console.print(f"[red]error:[/] segments directory not found: {seg_dir}")
        raise typer.Exit(2)

    expectations_path = Path(expectations)
    expect_data = load_expectations(expectations_path)

    if ferric_bin:
        ferric_cmd = [ferric_bin, "check"]
    else:
        debug_bin = Path("target/debug/ferric")
        if debug_bin.exists():
            ferric_cmd = [str(debug_bin), "check"]
        else:
            ferric_cmd = ["cargo", "run", "-q", "-p", "ferric-cli", "--", "check"]

    files = sorted(seg_dir.glob("*.clp"))
    if not files:
        console.print(f"[red]error:[/] no .clp files found in {seg_dir}")
        raise typer.Exit(2)

    detailed: dict[str, dict] = {}
    counts = {
        "pass": 0,
        "expected_fail": 0,
        "unexpected_fail": 0,
        "expected_mismatch": 0,
        "unexpected_pass": 0,
    }
    sig_counts: dict[str, int] = {}

    work_items = [(f, ferric_cmd) for f in files]
    for filename, exit_code, diagnostic in parallel_run(_worker, work_items, workers=workers):
        expectation = resolve_expectation(filename, expect_data)
        status = classify_result(filename, exit_code, diagnostic, expectation)
        counts[status] += 1
        sig = summarize_signature(diagnostic)
        if status in {"unexpected_fail", "expected_mismatch"} and sig:
            sig_counts[sig] = sig_counts.get(sig, 0) + 1
        detailed[filename] = {
            "status": status,
            "exit_code": exit_code,
            "diagnostic": diagnostic,
            "expected": expectation,
        }

    total = len(files)
    print("Segment Check Summary")
    print("=====================")
    print(f"Segments:          {total}")
    print(f"Pass:              {counts['pass']}")
    print(f"Expected fail:     {counts['expected_fail']}")
    print(f"Unexpected fail:   {counts['unexpected_fail']}")
    print(f"Expected mismatch: {counts['expected_mismatch']}")
    print(f"Unexpected pass:   {counts['unexpected_pass']}")

    if sig_counts:
        print("\nTop Unexpected Failure Signatures")
        print("---------------------------------")
        for sig, n in sorted(sig_counts.items(), key=lambda kv: (-kv[1], kv[0]))[:20]:
            print(f"{n:3d}  {sig}")

    if json_out:
        out_path = Path(json_out)
        out_path.parent.mkdir(parents=True, exist_ok=True)
        payload = {
            "segments_dir": str(seg_dir),
            "expectations": str(expectations_path),
            "ferric_cmd": ferric_cmd,
            "counts": counts,
            "total": total,
            "files": detailed,
        }
        save_manifest(out_path, payload)
        print(f"\nDetailed JSON: {out_path}")

    has_unexpected = (
        counts["unexpected_fail"] > 0
        or counts["expected_mismatch"] > 0
        or counts["unexpected_pass"] > 0
    )
    raise typer.Exit(1 if has_unexpected else 0)


if __name__ == "__main__":
    app()
