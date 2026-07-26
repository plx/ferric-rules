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
from ferric_tools._harness import (
    HarnessContractError,
    ResolvedHarness,
    atomic_write_bytes,
    resolve_harness_contract,
    sha256_bytes,
)
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
VALID_RUNABILITY = {"batch", "interactive", "library", "standalone", "unknown"}
FAILED_CLASSIFICATIONS = {"divergent", "incompatible"}
COMPAT_WORKSPACE = Path(".ferric-compat")
IS_WINDOWS = os.name == "nt"


class CompatibilityWorkspaceError(RuntimeError):
    """Raised when composed compatibility inputs cannot be staged safely."""


def _make_read_only(path: Path) -> None:
    """Make retained or staged inputs immutable where chmod does not block unlink."""
    if not IS_WINDOWS:
        path.chmod(0o444)


def _require_contained(path: Path, *, root: Path, label: str) -> Path:
    """Resolve *path* and require it to remain physically inside *root*."""
    try:
        resolved_root = root.resolve(strict=True)
        resolved_path = path.resolve(strict=True)
    except OSError as error:
        raise CompatibilityWorkspaceError(f"{label} cannot be resolved: {error}") from error

    try:
        resolved_path.relative_to(resolved_root)
    except ValueError as error:
        raise CompatibilityWorkspaceError(
            f"{label} escapes repository root: {resolved_path}"
        ) from error
    return resolved_path


def _require_contained_directory(path: Path, *, root: Path, label: str) -> Path:
    """Validate an existing repository-contained directory."""
    resolved_path = _require_contained(path, root=root, label=label)
    if not resolved_path.is_dir():
        raise CompatibilityWorkspaceError(f"{label} is not a directory: {resolved_path}")
    return resolved_path


def _require_contained_file(path: Path, *, root: Path, label: str) -> Path:
    """Validate an existing regular file without following a leaf symlink."""
    if path.is_symlink():
        raise CompatibilityWorkspaceError(f"{label} must not be a symlink: {path}")
    resolved_path = _require_contained(path, root=root, label=label)
    if not resolved_path.is_file():
        raise CompatibilityWorkspaceError(f"{label} is not a regular file: {resolved_path}")
    return resolved_path


def _ensure_workspace_directory(root: Path, relative_path: Path, *, label: str) -> Path:
    """Create a fixed relative directory without traversing outside *root*."""
    if relative_path.is_absolute() or any(part in {"", ".", ".."} for part in relative_path.parts):
        raise CompatibilityWorkspaceError(f"{label} must be a normalized relative path")

    try:
        resolved_root = root.resolve(strict=True)
    except OSError as error:
        raise CompatibilityWorkspaceError(f"repository root cannot be resolved: {error}") from error
    if not resolved_root.is_dir():
        raise CompatibilityWorkspaceError(f"repository root is not a directory: {resolved_root}")

    current = resolved_root
    for part in relative_path.parts:
        candidate = current / part
        if candidate.is_symlink():
            raise CompatibilityWorkspaceError(f"{label} must not contain a symlink: {candidate}")
        try:
            candidate.mkdir(mode=0o755, exist_ok=True)
        except OSError as error:
            raise CompatibilityWorkspaceError(f"cannot create {label}: {error}") from error
        if candidate.is_symlink():
            raise CompatibilityWorkspaceError(f"{label} must not contain a symlink: {candidate}")
        current = _require_contained_directory(candidate, root=resolved_root, label=label)
        try:
            current.chmod(0o755)
        except OSError as error:
            raise CompatibilityWorkspaceError(
                f"cannot set traversal permissions on {label}: {error}"
            ) from error
    return current


@contextlib.contextmanager
def _compatibility_run_workspace(root: Path):
    """Yield one repository-visible run directory and the retained-failure directory."""
    resolved_root = root.resolve(strict=True)
    runs_dir = _ensure_workspace_directory(
        resolved_root,
        COMPAT_WORKSPACE / "runs",
        label="compatibility run workspace",
    )
    failures_dir = _ensure_workspace_directory(
        resolved_root,
        COMPAT_WORKSPACE / "failures",
        label="compatibility failure workspace",
    )
    try:
        temporary_directory = tempfile.TemporaryDirectory(prefix="run-", dir=runs_dir)
    except OSError as error:
        message = f"cannot create compatibility run directory: {error}"
        raise CompatibilityWorkspaceError(message) from error

    with temporary_directory as temp_name:
        run_dir = _require_contained_directory(
            Path(temp_name),
            root=resolved_root,
            label="compatibility run directory",
        )
        try:
            run_dir.chmod(0o755)
        except OSError as error:
            raise CompatibilityWorkspaceError(
                f"cannot set traversal permissions on compatibility run directory: {error}"
            ) from error
        yield run_dir, failures_dir


def _materialize_composed_source(content: bytes, *, workspace: Path, root: Path) -> Path:
    """Write one closed, fsynced composed source in the invocation workspace."""
    resolved_workspace = _require_contained_directory(
        workspace,
        root=root,
        label="compatibility run directory",
    )
    file_descriptor = -1
    temp_path: Path | None = None
    materialized = False
    try:
        file_descriptor, temp_name = tempfile.mkstemp(
            prefix="composed-",
            suffix=".clp",
            dir=resolved_workspace,
        )
        temp_path = Path(temp_name)
        with os.fdopen(file_descriptor, "wb") as composed_file:
            file_descriptor = -1
            composed_file.write(content)
            composed_file.flush()
            os.fsync(composed_file.fileno())
        _make_read_only(temp_path)
        resolved_path = _require_contained_file(
            temp_path,
            root=root,
            label="composed compatibility source",
        )
        if resolved_path.read_bytes() != content:
            raise CompatibilityWorkspaceError(
                f"composed compatibility source changed after write: {resolved_path}"
            )
        materialized = True
        return resolved_path
    except OSError as error:
        raise CompatibilityWorkspaceError(
            f"cannot materialize composed compatibility source: {error}"
        ) from error
    finally:
        if file_descriptor >= 0:
            os.close(file_descriptor)
        if temp_path is not None and not materialized:
            temp_path.unlink(missing_ok=True)


def _verify_composed_source(
    path: Path,
    expected_content: bytes,
    *,
    root: Path,
    boundary: str,
) -> Path:
    """Fail closed if a staged source changes across an engine boundary."""
    resolved_path = _require_contained_file(
        path,
        root=root,
        label="composed compatibility source",
    )
    try:
        actual_content = resolved_path.read_bytes()
    except OSError as error:
        raise CompatibilityWorkspaceError(
            f"cannot verify composed compatibility source {boundary}: {error}"
        ) from error
    if actual_content != expected_content:
        raise CompatibilityWorkspaceError(
            f"composed compatibility source changed {boundary}: {resolved_path}"
        )
    return resolved_path


def _retain_failure_artifact(
    content: bytes,
    digest: str,
    *,
    failures_dir: Path,
    root: Path,
) -> Path:
    """Atomically retain exact failed input bytes under their SHA-256 digest."""
    resolved_failures_dir = _require_contained_directory(
        failures_dir,
        root=root,
        label="compatibility failure workspace",
    )
    artifact_path = resolved_failures_dir / f"{digest}.clp"
    if artifact_path.is_symlink():
        raise CompatibilityWorkspaceError(
            f"compatibility failure artifact must not be a symlink: {artifact_path}"
        )

    try:
        if artifact_path.exists():
            resolved_artifact = _require_contained_file(
                artifact_path,
                root=root,
                label="compatibility failure artifact",
            )
            if resolved_artifact.read_bytes() != content:
                raise CompatibilityWorkspaceError(
                    f"compatibility failure artifact digest collision: {artifact_path}"
                )
            _make_read_only(resolved_artifact)
            return resolved_artifact

        atomic_write_bytes(artifact_path, content)
        _make_read_only(artifact_path)
        resolved_artifact = _require_contained_file(
            artifact_path,
            root=root,
            label="compatibility failure artifact",
        )
        if sha256_bytes(resolved_artifact.read_bytes()) != digest:
            raise CompatibilityWorkspaceError(
                f"compatibility failure artifact changed after retention: {resolved_artifact}"
            )
        return resolved_artifact
    except OSError as error:
        raise CompatibilityWorkspaceError(
            f"cannot retain compatibility failure artifact: {error}"
        ) from error


# ---------------------------------------------------------------------------
# Engine runners
# ---------------------------------------------------------------------------


def run_ferric(file_path: str, ferric_bin: str, root: str, timeout_secs: int) -> dict:
    """Run a .clp file through the ferric CLI."""
    start = time.monotonic()
    try:
        proc = subprocess.run(
            [ferric_bin, "run", file_path],
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
            "stderr": f"ferric binary not found: {ferric_bin}",
            "duration_ms": 0,
            "timed_out": False,
        }


def run_clips_docker(file_path: str, root: str, script: str, timeout_secs: int) -> dict:
    """Run a .clp file through the Docker CLIPS harness."""
    resolved_path = _require_contained_file(
        Path(file_path),
        root=Path(root),
        label="CLIPS input",
    )
    start = time.monotonic()
    try:
        proc = subprocess.run(
            [script, "run", "--file", str(resolved_path)],
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
    (
        rel_path,
        abs_path,
        ferric,
        root,
        script,
        timeout,
        skip_clips,
        harness,
        run_workspace,
        failures_dir,
    ) = args

    run_path = abs_path
    composed_path: Path | None = None
    composed_content: bytes | None = None
    composed_digest: str | None = None
    retained_artifact: Path | None = None
    resolved_root = Path(root).resolve(strict=True)
    try:
        if harness is not None:
            if run_workspace is None or failures_dir is None:
                raise CompatibilityWorkspaceError(
                    f"{rel_path}: harness execution requires an invocation workspace"
                )
            composed_content = harness.source_bytes + b"\n" + harness.harness_bytes
            composed_digest = sha256_bytes(composed_content)
            composed_path = _materialize_composed_source(
                composed_content,
                workspace=Path(run_workspace),
                root=resolved_root,
            )
            run_path = str(composed_path)

        if composed_path is not None and composed_content is not None:
            _verify_composed_source(
                composed_path,
                composed_content,
                root=resolved_root,
                boundary="before Ferric execution",
            )
        ferric_result = run_ferric(run_path, ferric, root, timeout)
        if harness is not None:
            ferric_result["harness"] = dict(harness.metadata)
        clips_result = None
        if not skip_clips:
            if composed_path is not None and composed_content is not None:
                _verify_composed_source(
                    composed_path,
                    composed_content,
                    root=resolved_root,
                    boundary="before CLIPS execution",
                )
            clips_result = run_clips_docker(run_path, root, script, timeout)
            if harness is not None:
                clips_result["harness"] = dict(harness.metadata)
        if composed_path is not None and composed_content is not None:
            _verify_composed_source(
                composed_path,
                composed_content,
                root=resolved_root,
                boundary="after engine execution",
            )
        classification, reason = classify_results(ferric_result, clips_result)

        if composed_content is not None and composed_digest is not None:
            composed_metadata: dict[str, str | int] = {
                "sha256": composed_digest,
                "size_bytes": len(composed_content),
            }
            if classification in FAILED_CLASSIFICATIONS:
                retained_artifact = _retain_failure_artifact(
                    composed_content,
                    composed_digest,
                    failures_dir=Path(failures_dir),
                    root=resolved_root,
                )
                composed_metadata["artifact_path"] = retained_artifact.relative_to(
                    resolved_root
                ).as_posix()
            ferric_result["composed_source"] = dict(composed_metadata)
            if clips_result is not None:
                clips_result["composed_source"] = dict(composed_metadata)

        return rel_path, ferric_result, clips_result, classification, reason
    except BaseException as error:
        if (
            composed_content is not None
            and composed_digest is not None
            and failures_dir is not None
            and retained_artifact is None
        ):
            try:
                retained_artifact = _retain_failure_artifact(
                    composed_content,
                    composed_digest,
                    failures_dir=Path(failures_dir),
                    root=resolved_root,
                )
            except Exception as retention_error:
                error.add_note(f"could not retain composed failure artifact: {retention_error}")
            else:
                artifact_relpath = retained_artifact.relative_to(resolved_root).as_posix()
                error.add_note(f"composed failure artifact retained at {artifact_relpath}")
        raise
    finally:
        if composed_path is not None:
            try:
                composed_path.unlink(missing_ok=True)
            except OSError as cleanup_error:
                active_error = sys.exception()
                message = (
                    f"could not remove composed temporary source {composed_path}: {cleanup_error}"
                )
                if active_error is None:
                    raise CompatibilityWorkspaceError(message) from cleanup_error
                active_error.add_note(message)


def _resolve_harness(
    entry: dict,
    *,
    source_path: Path,
    root: Path,
    manifest_key: str,
) -> ResolvedHarness | None:
    """Validate and resolve a library entry's structured harness contract."""
    return resolve_harness_contract(
        entry,
        source_path=source_path,
        root=root,
        manifest_key=manifest_key,
    )


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

    # Select files and validate all library harnesses before starting engines.
    files_to_run: list[tuple[str, str]] = []
    resolved_harnesses: dict[str, ResolvedHarness | None] = {}
    harness_targets: dict[Path, str] = {}
    for rel_path, info in mdata["files"].items():
        runability = info.get("runability")
        if runability not in VALID_RUNABILITY:
            console.print(
                f"[red]error:[/] {rel_path}: invalid or missing runability: {runability!r}"
            )
            raise typer.Exit(1)
        if "harness" in info and runability != "library":
            console.print(
                f"[red]error:[/] {rel_path}: harness contract requires library runability"
            )
            raise typer.Exit(1)

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
        if runability == "library":
            try:
                resolved_harness = _resolve_harness(
                    info,
                    source_path=abs_path,
                    root=root,
                    manifest_key=rel_path,
                )
            except HarnessContractError as error:
                console.print(f"[red]error:[/] {error}")
                raise typer.Exit(1) from error

            if resolved_harness is None:
                if file:
                    skip_reason = info["harness"].get("skip_reason", "not executable")
                    console.print(
                        f"[red]error:[/] {rel_path}: harness is not executable ({skip_reason})"
                    )
                    raise typer.Exit(1)
                continue

            target = resolved_harness.path
            if previous := harness_targets.get(target):
                console.print(
                    "[red]error:[/] duplicate harness mapping: "
                    f"{previous} and {rel_path} -> {target}"
                )
                raise typer.Exit(1)
            harness_targets[target] = rel_path
            resolved_harnesses[rel_path] = resolved_harness
        elif not abs_path.exists():
            continue
        else:
            resolved_harnesses[rel_path] = None

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

    completed = 0
    results: dict[str, tuple] = {}
    start_time = time.monotonic()

    with _compatibility_run_workspace(root) as (run_workspace, failures_dir):
        work_items = [
            (
                rel,
                abs_p,
                ferric,
                str(root),
                script,
                timeout,
                skip_clips,
                resolved_harnesses[rel],
                str(run_workspace),
                str(failures_dir),
            )
            for rel, abs_p in files_to_run
        ]

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
            artifact_path = (ferric_r.get("composed_source") or {}).get("artifact_path")
            if artifact_path:
                print(f"    composed artifact: {artifact_path}")
            if reason == "output-mismatch" and clips_r:
                f_out = normalize_output(ferric_r["stdout"], "ferric")[:100]
                c_out = normalize_output(clips_r["stdout"], "clips")[:100]
                print(f"    ferric: {f_out!r}")
                print(f"    clips:  {c_out!r}")
        if len(divergent) > 20:
            print(f"  ... and {len(divergent) - 20} more")


if __name__ == "__main__":
    app()
