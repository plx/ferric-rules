"""Execution runner for CLIPS compatibility assessment.

Runs files classified as testable/pending through both the ferric CLI and
Docker-based reference CLIPS, compares normalized outputs, and updates
the manifest with classification results.
"""

from __future__ import annotations

import contextlib
import os
import re
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Annotated

import typer
from rich.console import Console

from ferric_tools._formatting import normalize_floats, normalize_output
from ferric_tools._manifest import load_manifest, save_manifest
from ferric_tools._paths import (
    examples_dir as default_examples_dir,
)
from ferric_tools._paths import (
    ferric_bin as default_ferric_bin,
)
from ferric_tools._paths import (
    harness_script as default_harness_script,
)
from ferric_tools._paths import (
    repo_root,
)
from ferric_tools._subprocess import parallel_run

app = typer.Typer(help="Run CLIPS compatibility assessment.")
console = Console(stderr=True)


# ---------------------------------------------------------------------------
# Engine runners
# ---------------------------------------------------------------------------


def run_ferric(file_path: str, ferric_bin: str, timeout_secs: int) -> dict:
    """Run a .clp file through the ferric CLI."""
    start = time.monotonic()
    try:
        proc = subprocess.run(
            [ferric_bin, "run", file_path],
            capture_output=True,
            text=True,
            timeout=timeout_secs,
        )
        duration_ms = int((time.monotonic() - start) * 1000)
        return {
            "exit_code": proc.returncode,
            "stdout": proc.stdout,
            "stderr": proc.stderr,
            "duration_ms": duration_ms,
            "timed_out": False,
        }
    except subprocess.TimeoutExpired:
        duration_ms = int((time.monotonic() - start) * 1000)
        return {
            "exit_code": -1,
            "stdout": "",
            "stderr": f"timeout after {timeout_secs}s",
            "duration_ms": duration_ms,
            "timed_out": True,
        }
    except FileNotFoundError:
        return {
            "exit_code": -1,
            "stdout": "",
            "stderr": f"ferric binary not found: {ferric_bin}",
            "duration_ms": 0,
            "timed_out": False,
        }


def run_clips_docker(file_path: str, root: str, script: str, timeout_secs: int) -> dict:
    """Run a .clp file through the Docker CLIPS harness."""
    start = time.monotonic()
    try:
        proc = subprocess.run(
            [script, "run", "--file", file_path],
            capture_output=True,
            text=True,
            timeout=timeout_secs,
            cwd=root,
        )
        duration_ms = int((time.monotonic() - start) * 1000)
        return {
            "exit_code": proc.returncode,
            "stdout": proc.stdout,
            "stderr": proc.stderr,
            "duration_ms": duration_ms,
            "timed_out": False,
        }
    except subprocess.TimeoutExpired:
        duration_ms = int((time.monotonic() - start) * 1000)
        return {
            "exit_code": -1,
            "stdout": "",
            "stderr": f"timeout after {timeout_secs}s",
            "duration_ms": duration_ms,
            "timed_out": True,
        }
    except FileNotFoundError:
        return {
            "exit_code": -1,
            "stdout": "",
            "stderr": f"harness script not found: {script}",
            "duration_ms": 0,
            "timed_out": False,
        }


# ---------------------------------------------------------------------------
# Classification
# ---------------------------------------------------------------------------


def classify_results(ferric_result: dict, clips_result: dict | None) -> tuple[str, str]:
    """Classify based on ferric and CLIPS results."""
    f = ferric_result
    c = clips_result

    if f["timed_out"] and (c is None or c["timed_out"]):
        return "incompatible", "timeout-both"

    if c is None:
        if f["timed_out"]:
            return "incompatible", "timeout-ferric"
        if f["exit_code"] != 0:
            return "incompatible", "ferric-error"
        return "pending", "ferric-only-clean"

    if f["timed_out"] and not c["timed_out"]:
        return "divergent", "timeout-ferric"

    if not f["timed_out"] and c["timed_out"]:
        return "divergent", "timeout-clips"

    if c["exit_code"] == 0:
        clips_has_error = bool(re.search(r"\[[A-Z]+\d+\]", c["stdout"]))
        if clips_has_error and f["exit_code"] != 0:
            return "incompatible", "both-error"
        if clips_has_error:
            return "incompatible", "clips-load-error"

    if f["exit_code"] != 0 and c["exit_code"] == 0:
        return "divergent", "ferric-error"

    if f["exit_code"] == 0 and c["exit_code"] != 0:
        return "divergent", "clips-error"

    if f["exit_code"] != 0 and c["exit_code"] != 0:
        return "incompatible", "both-error"

    f_out = normalize_output(f["stdout"], "ferric")
    c_out = normalize_output(c["stdout"], "clips")

    if f_out == c_out:
        if f_out.strip() == "":
            return "equivalent", "empty-match"
        return "equivalent", "exact-match"

    if normalize_floats(f_out) == normalize_floats(c_out):
        return "equivalent", "float-normalized-match"

    return "divergent", "output-mismatch"


# ---------------------------------------------------------------------------
# Worker function (for parallel execution)
# ---------------------------------------------------------------------------


def process_file(args: tuple) -> tuple:
    """Process a single file through both engines."""
    rel_path, abs_path, ferric, root, script, timeout, skip_clips, harness_path = args

    run_path = abs_path
    tmp_path = None
    try:
        if harness_path and os.path.exists(harness_path):
            with open(abs_path) as f_orig:
                original_content = f_orig.read()
            with open(harness_path) as f_harness:
                harness_content = f_harness.read()
            with tempfile.NamedTemporaryFile(suffix=".clp", delete=False, mode="w") as tmp:
                tmp.write(original_content)
                tmp.write("\n")
                tmp.write(harness_content)
            tmp_path = tmp.name
            run_path = tmp_path

        ferric_result = run_ferric(run_path, ferric, timeout)
        clips_result = None
        if not skip_clips:
            clips_result = run_clips_docker(run_path, root, script, timeout)
        classification, reason = classify_results(ferric_result, clips_result)
        return rel_path, ferric_result, clips_result, classification, reason
    finally:
        if tmp_path:
            with contextlib.suppress(OSError):
                os.unlink(tmp_path)


def _resolve_harness(entry: dict, root: Path) -> str | None:
    """Resolve harness path from manifest entry, or None."""
    harness = entry.get("harness")
    if not harness:
        return None
    path = root / harness
    return str(path) if path.exists() else None


@app.command()
def main(
    all_files: Annotated[bool, typer.Option("--all", help="Run all testable files")] = False,
    only_pending: Annotated[bool, typer.Option(help="Only run pending files")] = False,
    only_divergent: Annotated[bool, typer.Option(help="Re-run divergent files")] = False,
    source: Annotated[str | None, typer.Option(help="Filter by source directory")] = None,
    file: Annotated[
        str | None, typer.Option(help="Run a single file (relative to tests/examples/)")
    ] = None,
    timeout: Annotated[int, typer.Option(help="Per-engine timeout in seconds")] = 120,
    workers: Annotated[int, typer.Option(help="Parallel worker count")] = 4,
    manifest: Annotated[Path | None, typer.Option(help="Path to manifest file")] = None,
    ferric_bin_path: Annotated[
        str | None, typer.Option("--ferric-bin", help="Path to ferric binary")
    ] = None,
    skip_clips: Annotated[bool, typer.Option(help="Skip Docker CLIPS (ferric-only)")] = False,
    dry_run: Annotated[bool, typer.Option(help="Show files without running")] = False,
) -> None:
    """Run CLIPS compatibility assessment."""
    root = repo_root()
    ed = default_examples_dir()
    manifest_path = Path(manifest) if manifest else ed / "compat-manifest.json"
    ferric = ferric_bin_path or str(default_ferric_bin())
    script = str(default_harness_script())

    if not manifest_path.exists():
        console.print(f"[red]error:[/] manifest not found: {manifest_path}")
        console.print("Run ferric-compat-scan first.")
        raise typer.Exit(1)

    mdata = load_manifest(manifest_path)

    if not dry_run and not Path(ferric).exists():
        console.print(f"[red]error:[/] ferric binary not found: {ferric}")
        console.print("Run: cargo build --release -p ferric-cli")
        raise typer.Exit(1)

    # Check Docker CLIPS availability
    if not skip_clips and not dry_run:
        try:
            result = subprocess.run(
                ["docker", "image", "inspect", "ferric-rules/clips-reference:latest"],
                capture_output=True,
                timeout=10,
            )
            if result.returncode != 0:
                console.print(
                    "[yellow]warning:[/] Docker CLIPS image not found. Using --skip-clips mode."
                )
                skip_clips = True
        except (FileNotFoundError, subprocess.TimeoutExpired):
            console.print("[yellow]warning:[/] Docker not available. Using --skip-clips mode.")
            skip_clips = True

    # Select files to run
    files_to_run: list[tuple[str, str]] = []
    for rel_path, info in mdata["files"].items():
        if file:
            if rel_path != file:
                continue
        elif only_pending:
            if info["classification"] != "pending":
                continue
        elif only_divergent:
            if info["classification"] != "divergent":
                continue
        elif all_files:
            if (  # noqa: SIM102
                info["reason"] not in ("testable", "ferric-only-clean", "library-only")
                and info["classification"] != "pending"
            ):
                if info["classification"] == "incompatible" and info["reason"] != "testable":
                    continue
        else:
            if info["classification"] != "pending":
                continue

        if source and info["source"] != source:
            continue

        generated = info.get("generated")
        if generated:
            abs_path = root / generated
        elif rel_path.startswith("generated/"):
            abs_path = root / "tests" / rel_path
        else:
            abs_path = ed / rel_path
        if not abs_path.exists():
            continue

        files_to_run.append((rel_path, str(abs_path)))

    if not files_to_run:
        print("No files to run.")
        raise typer.Exit(0)

    print(f"Files to run: {len(files_to_run)}")
    print(f"Timeout: {timeout}s per engine")
    print(f"Workers: {workers}")
    print(f"Skip CLIPS: {skip_clips}")
    print()

    if dry_run:
        for rel_path, _ in files_to_run:
            print(f"  {rel_path}")
        raise typer.Exit(0)

    work_items = [
        (
            rel,
            abs_p,
            ferric,
            str(root),
            script,
            timeout,
            skip_clips,
            _resolve_harness(mdata["files"].get(rel, {}), root),
        )
        for rel, abs_p in files_to_run
    ]

    completed = 0
    results: dict[str, tuple] = {}
    start_time = time.monotonic()

    for result_tuple in parallel_run(process_file, work_items, workers=workers):
        try:
            rel, ferric_result, clips_result, classification, reason = result_tuple
            results[rel] = (ferric_result, clips_result, classification, reason)
            completed += 1
            status_char = {
                "equivalent": ".",
                "divergent": "D",
                "incompatible": "X",
                "pending": "?",
            }.get(classification, "?")
            sys.stdout.write(status_char)
            if completed % 80 == 0:
                sys.stdout.write(f" [{completed}/{len(files_to_run)}]\n")
            sys.stdout.flush()
        except Exception:
            completed += 1
            sys.stdout.write("E")
            sys.stdout.flush()

    elapsed = time.monotonic() - start_time
    print(f"\n\nCompleted {completed} files in {elapsed:.1f}s")

    # Update manifest
    for rel_path, (ferric_result, clips_result, classification, reason) in results.items():
        if rel_path in mdata["files"]:
            entry = mdata["files"][rel_path]
            entry["ferric"] = ferric_result
            entry["clips"] = clips_result
            entry["classification"] = classification
            entry["reason"] = reason

    # Recompute summary
    summary = {"total": 0, "equivalent": 0, "divergent": 0, "incompatible": 0, "pending": 0}
    for info in mdata["files"].values():
        summary["total"] += 1
        cls = info["classification"]
        if cls in summary:
            summary[cls] += 1
    mdata["summary"] = summary

    save_manifest(manifest_path, mdata)

    print(f"\nManifest updated: {manifest_path}")
    print("\nResults:")

    run_summary = {"equivalent": 0, "divergent": 0, "incompatible": 0, "pending": 0}
    for _, (_, _, cls, _reason) in results.items():
        if cls in run_summary:
            run_summary[cls] += 1

    for cls, count in sorted(run_summary.items()):
        if count > 0:
            print(f"  {cls:15s}: {count}")

    divergent = [(k, v) for k, v in results.items() if v[2] == "divergent"]
    if divergent:
        print(f"\nDivergent files ({len(divergent)}):")
        for rel_path, (ferric_r, clips_r, _cls, reason) in divergent[:20]:
            print(f"  {rel_path} ({reason})")
            if reason == "output-mismatch" and clips_r:
                f_out = normalize_output(ferric_r["stdout"], "ferric")[:100]
                c_out = normalize_output(clips_r["stdout"], "clips")[:100]
                print(f"    ferric: {f_out!r}")
                print(f"    clips:  {c_out!r}")
        if len(divergent) > 20:
            print(f"  ... and {len(divergent) - 20} more")


if __name__ == "__main__":
    app()
