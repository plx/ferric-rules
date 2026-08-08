"""Execution runner for CLIPS compatibility assessment.

Runs files classified as testable/pending through both the ferric CLI and
Docker-based reference CLIPS, compares normalized outputs, and updates
the manifest with classification results.
"""

from __future__ import annotations

import base64
import contextlib
import copy
import json
import os
import re
import secrets
import signal
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Annotated

import typer
from rich.console import Console

from ferric_tools._formatting import normalize_output
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
from ferric_tools.compat.clips_oracle import (
    ClipsOracleProtocolError,
    build_probe_operations,
    parse_probe_output,
)
from ferric_tools.compat.diagnostics import (
    diagnostic,
    diagnostic_evidence_state,
    process_diagnostic,
    termination,
)
from ferric_tools.compat.oracle import (
    DECLARATION_VERSION,
    ORACLE_PROTOCOL_VERSION,
    SCENARIO_DECLARATION_VERSION,
    EvidenceStatus,
    OracleDeclaration,
    canonical_scenario_plan,
    evaluate_oracle,
    evaluation_to_dict,
    scenario_native_phases,
    validate_declaration,
    validate_scenario_source_sizes,
)
from ferric_tools.compat.projection import (
    ObservationProjectionError,
    project_clips_observation,
    project_ferric_observation,
    project_observation_diagnostic,
)

app = typer.Typer(help="Run CLIPS compatibility assessment.")
console = Console(stderr=True)
VALID_RUNABILITY = {"batch", "interactive", "library", "standalone", "unknown"}
FAILED_CLASSIFICATIONS = {"divergent", "incompatible"}
INVALID_RUNTIME_EVIDENCE_REASONS = frozenset(
    {
        "diagnostic-invalid",
        "diagnostic-missing",
        "harness-failure",
        "termination-invalid",
        "termination-missing",
    }
)
COMPAT_WORKSPACE = Path(".ferric-compat")
IS_WINDOWS = os.name == "nt"
_EMPTY_INTERRUPTION_PROTOCOL_ISSUES = frozenset(
    {
        "native-phase-records-missing",
        "lifecycle-cardinality-or-order",
        "native-run-metadata-missing",
        "phase-cardinality-or-order",
        "module-cardinality",
        "truncated-native-record",
    }
)
_REFERENCE_SCHEMA = "ferric.clips-reference-provenance"
_REFERENCE_FIELDS = frozenset(
    {
        "schema",
        "version",
        "engine",
        "engine_version",
        "package",
        "package_version",
        "platform",
        "binary_sha256",
        "library_sha256",
        "base_image",
        "image_id",
    }
)
_REFERENCE_DIGEST_RE = re.compile(r"[0-9a-f]{64}")
_REFERENCE_BASE_IMAGE_RE = re.compile(r"debian:bookworm-slim@sha256:[0-9a-f]{64}")
_REFERENCE_IMAGE_ID_RE = re.compile(r"sha256:[0-9a-f]{64}")
_CANDIDATE_SCHEMA = "ferric.compat-candidate-provenance"
_COMMIT_SHA_RE = re.compile(r"[0-9a-f]{40}")


class CompatibilityWorkspaceError(RuntimeError):
    """Raised when composed compatibility inputs cannot be staged safely."""


class ReferenceProvenanceError(RuntimeError):
    """Raised when the CLIPS reference identity is unavailable or malformed."""


class CandidateProvenanceError(RuntimeError):
    """Raised when the Ferric candidate identity is unavailable or changes."""


@dataclass(frozen=True)
class ResolvedScenarioBundle:
    """One fully validated runtime view of a v2 scenario bundle."""

    plan: bytes
    paths: tuple[Path, ...]
    native_phases: tuple[str, ...]


def candidate_provenance(binary: Path, *, commit_sha: str) -> dict[str, object]:
    """Measure the exact Ferric executable used by a compatibility run."""
    if _COMMIT_SHA_RE.fullmatch(commit_sha) is None:
        raise CandidateProvenanceError(
            "candidate commit SHA must be exactly 40 lowercase hexadecimal characters"
        )
    if binary.is_symlink():
        raise CandidateProvenanceError(f"candidate binary must not be a symlink: {binary}")
    try:
        resolved = binary.resolve(strict=True)
        if not resolved.is_file():
            raise CandidateProvenanceError(f"candidate binary is not a regular file: {binary}")
        content = resolved.read_bytes()
    except CandidateProvenanceError:
        raise
    except OSError as error:
        raise CandidateProvenanceError(f"cannot read candidate binary {binary}: {error}") from error
    return {
        "schema": _CANDIDATE_SCHEMA,
        "version": 1,
        "commit_sha": commit_sha,
        "binary_sha256": sha256_bytes(content),
    }


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


def _materialize_read_only_input(
    content: bytes,
    *,
    workspace: Path,
    root: Path,
    prefix: str,
    suffix: str,
    label: str,
) -> Path:
    """Write one closed, fsynced input in the invocation workspace."""
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
            prefix=prefix,
            suffix=suffix,
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
            label=label,
        )
        if resolved_path.read_bytes() != content:
            raise CompatibilityWorkspaceError(f"{label} changed after write: {resolved_path}")
        materialized = True
        return resolved_path
    except OSError as error:
        raise CompatibilityWorkspaceError(f"cannot materialize {label}: {error}") from error
    finally:
        if file_descriptor >= 0:
            os.close(file_descriptor)
        if temp_path is not None and not materialized:
            temp_path.unlink(missing_ok=True)


def _materialize_composed_source(content: bytes, *, workspace: Path, root: Path) -> Path:
    """Write one closed, fsynced composed source in the invocation workspace."""
    return _materialize_read_only_input(
        content,
        workspace=workspace,
        root=root,
        prefix="composed-",
        suffix=".clp",
        label="composed compatibility source",
    )


def _materialize_scenario_plan(content: bytes, *, workspace: Path, root: Path) -> Path:
    """Write one closed, fsynced canonical scenario plan in the run workspace."""
    return _materialize_read_only_input(
        content,
        workspace=workspace,
        root=root,
        prefix="scenario-",
        suffix=".plan",
        label="compatibility scenario plan",
    )


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


def _verify_scenario_plan(
    path: Path,
    expected_content: bytes,
    *,
    root: Path,
    boundary: str,
) -> Path:
    """Fail closed if a staged scenario plan changes across a boundary."""
    resolved_path = _require_contained_file(
        path,
        root=root,
        label="compatibility scenario plan",
    )
    try:
        actual_content = resolved_path.read_bytes()
    except OSError as error:
        raise CompatibilityWorkspaceError(
            f"cannot verify compatibility scenario plan {boundary}: {error}"
        ) from error
    if actual_content != expected_content:
        raise CompatibilityWorkspaceError(
            f"compatibility scenario plan changed {boundary}: {resolved_path}"
        )
    return resolved_path


def _resolve_scenario_bundle(
    declaration: OracleDeclaration,
    *,
    registry_key: str,
    primary_path: Path,
    root: Path,
) -> ResolvedScenarioBundle:
    """Resolve and digest-check every v2 source before engine execution."""
    if declaration.version != SCENARIO_DECLARATION_VERSION or declaration.sources is None:
        raise CompatibilityWorkspaceError("scenario bundle requires a valid v2 declaration")
    if declaration.sources[0].path != registry_key:
        raise CompatibilityWorkspaceError(
            f"{registry_key}: primary source path does not match the manifest key"
        )
    examples = _require_contained_directory(
        root / "tests" / "examples",
        root=root,
        label="scenario examples directory",
    )
    expected_primary = _require_contained_file(
        primary_path,
        root=root,
        label=f"{registry_key} primary source",
    )
    paths: list[Path] = []
    sizes: list[int] = []
    for index, source in enumerate(declaration.sources):
        candidate = examples.joinpath(*Path(source.path).parts)
        resolved = _require_contained_file(
            candidate,
            root=examples,
            label=f"{registry_key} scenario source {index}",
        )
        try:
            validate_scenario_source_sizes((resolved.stat().st_size,))
            content = resolved.read_bytes()
            validate_scenario_source_sizes((len(content),))
        except ValueError as error:
            raise CompatibilityWorkspaceError(f"{registry_key}: {error}") from error
        except OSError as error:
            raise CompatibilityWorkspaceError(
                f"{registry_key}: cannot read scenario source {index}: {error}"
            ) from error
        if sha256_bytes(content) != source.sha256:
            raise CompatibilityWorkspaceError(
                f"{registry_key}: scenario source {index} digest is stale"
            )
        paths.append(resolved)
        sizes.append(len(content))
    try:
        validate_scenario_source_sizes(tuple(sizes))
    except ValueError as error:
        raise CompatibilityWorkspaceError(f"{registry_key}: {error}") from error
    if paths[0] != expected_primary:
        raise CompatibilityWorkspaceError(
            f"{registry_key}: primary scenario source differs from the selected input"
        )
    return ResolvedScenarioBundle(
        plan=canonical_scenario_plan(declaration),
        paths=tuple(paths),
        native_phases=scenario_native_phases(declaration),
    )


def _verify_scenario_bundle(
    bundle: ResolvedScenarioBundle,
    declaration: OracleDeclaration,
    *,
    root: Path,
    boundary: str,
) -> None:
    """Revalidate every source across each independently invoked engine boundary."""
    assert declaration.sources is not None
    if len(bundle.paths) != len(declaration.sources):
        raise CompatibilityWorkspaceError("scenario source cardinality changed")
    sizes: list[int] = []
    for index, (path, source) in enumerate(zip(bundle.paths, declaration.sources, strict=True)):
        resolved = _require_contained_file(
            path,
            root=root / "tests" / "examples",
            label=f"scenario source {index}",
        )
        try:
            validate_scenario_source_sizes((resolved.stat().st_size,))
            content = resolved.read_bytes()
            validate_scenario_source_sizes((len(content),))
        except ValueError as error:
            raise CompatibilityWorkspaceError(f"scenario source {index}: {error}") from error
        except OSError as error:
            raise CompatibilityWorkspaceError(
                f"cannot verify scenario source {index} {boundary}: {error}"
            ) from error
        if sha256_bytes(content) != source.sha256:
            raise CompatibilityWorkspaceError(
                f"scenario source {index} changed {boundary}: {resolved}"
            )
        sizes.append(len(content))
    try:
        validate_scenario_source_sizes(tuple(sizes))
    except ValueError as error:
        raise CompatibilityWorkspaceError(f"scenario bundle {boundary}: {error}") from error


def _retain_failure_artifact(
    content: bytes,
    digest: str,
    *,
    failures_dir: Path,
    root: Path,
    suffix: str = ".clp",
) -> Path:
    """Atomically retain exact failed input bytes under their SHA-256 digest."""
    resolved_failures_dir = _require_contained_directory(
        failures_dir,
        root=root,
        label="compatibility failure workspace",
    )
    if suffix not in {".clp", ".plan"}:
        raise CompatibilityWorkspaceError("unsupported compatibility artifact suffix")
    artifact_path = resolved_failures_dir / f"{digest}{suffix}"
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


def _output_bytes(value: str | bytes | None) -> bytes:
    if value is None:
        return b""
    if isinstance(value, bytes):
        return value
    return value.encode("utf-8")


def _display_output(value: bytes) -> str:
    return value.decode("utf-8", errors="replace")


def load_reference_provenance(
    script: str,
    *,
    root: str,
    timeout_secs: int = 30,
) -> dict[str, object]:
    """Invoke and strictly validate the reference wrapper's measured identity."""
    try:
        process = subprocess.run(
            [script, "provenance"],
            capture_output=True,
            text=False,
            timeout=timeout_secs,
            cwd=root,
        )
    except (OSError, subprocess.SubprocessError) as error:
        raise ReferenceProvenanceError(
            f"cannot obtain CLIPS reference provenance: {error}"
        ) from error
    stdout = _output_bytes(process.stdout)
    stderr = _output_bytes(process.stderr)
    if process.returncode != 0:
        detail = _display_output(stderr).strip() or f"status {process.returncode}"
        raise ReferenceProvenanceError(f"CLIPS reference provenance failed: {detail}")
    if stderr:
        raise ReferenceProvenanceError("CLIPS reference provenance emitted stderr")
    if stdout.count(b"\n") != 1 or not stdout.endswith(b"\n"):
        raise ReferenceProvenanceError(
            "CLIPS reference provenance must be one LF-terminated JSON object"
        )

    def reject_duplicate_fields(pairs: list[tuple[str, object]]) -> dict[str, object]:
        result: dict[str, object] = {}
        for key, value in pairs:
            if key in result:
                raise ValueError(f"duplicate field: {key}")
            result[key] = value
        return result

    try:
        decoded = stdout.decode("utf-8", errors="strict")
        raw = json.loads(decoded, object_pairs_hook=reject_duplicate_fields)
    except (UnicodeError, json.JSONDecodeError, ValueError) as error:
        raise ReferenceProvenanceError(
            f"CLIPS reference provenance is malformed: {error}"
        ) from error
    if type(raw) is not dict or set(raw) != _REFERENCE_FIELDS:
        raise ReferenceProvenanceError("CLIPS reference provenance has an unexpected field set")
    constants = {
        "schema": _REFERENCE_SCHEMA,
        "version": 1,
        "engine": "clips",
        "engine_version": "6.30",
        "package": "clips",
        "package_version": "6.30-4.1",
    }
    for field, expected in constants.items():
        if type(raw[field]) is not type(expected) or raw[field] != expected:
            raise ReferenceProvenanceError(f"CLIPS reference provenance field {field!r} is invalid")
    platform = raw["platform"]
    if type(platform) is not str or platform not in {"linux/amd64", "linux/arm64"}:
        raise ReferenceProvenanceError("CLIPS reference provenance platform is invalid")
    for field in ("binary_sha256", "library_sha256"):
        value = raw[field]
        if type(value) is not str or _REFERENCE_DIGEST_RE.fullmatch(value) is None:
            raise ReferenceProvenanceError(f"CLIPS reference provenance field {field!r} is invalid")
    base_image = raw["base_image"]
    if type(base_image) is not str or _REFERENCE_BASE_IMAGE_RE.fullmatch(base_image) is None:
        raise ReferenceProvenanceError("CLIPS reference provenance base_image is invalid")
    image_id = raw["image_id"]
    if type(image_id) is not str or _REFERENCE_IMAGE_ID_RE.fullmatch(image_id) is None:
        raise ReferenceProvenanceError("CLIPS reference provenance image_id is invalid")
    return raw


def _attach_raw_output(result: dict, *, stdout: bytes, stderr: bytes) -> dict:
    """Persist exact process bytes alongside their human-readable views."""
    result["raw_output"] = {
        "encoding": "base64",
        "stdout": base64.b64encode(stdout).decode("ascii"),
        "stderr": base64.b64encode(stderr).decode("ascii"),
    }
    return result


def _with_termination(result: dict, *, spawn_error: bool = False) -> dict:
    """Attach process state without treating it as an engine diagnostic."""
    if spawn_error:
        result["spawn_error"] = True
    result["termination"] = termination(
        exit_code=result["exit_code"],
        timed_out=result["timed_out"],
        spawn_error=spawn_error,
    )
    return result


def _clips_wrapper_termination(exit_code: int) -> dict[str, object]:
    """Normalize Docker's documented 128+signal transport status."""
    # The observer runs in a Linux container even when the compatibility tool
    # runs on another host. Linux signal numbers occupy 1..64, including the
    # realtime range that is absent from some host-side Python enums.
    if 129 <= exit_code <= 192:
        return {"kind": "signal", "exit_code": None, "signal": exit_code - 128}
    return termination(exit_code=exit_code, timed_out=False)


def _terminate_process_tree(process: subprocess.Popen) -> None:
    """Terminate the isolated CLIPS wrapper and all local pipeline children."""
    if IS_WINDOWS:
        try:
            subprocess.run(
                ["taskkill", "/PID", str(process.pid), "/T", "/F"],
                capture_output=True,
                check=False,
                timeout=5,
            )
        except (OSError, subprocess.SubprocessError):
            process.kill()
        return
    with contextlib.suppress(ProcessLookupError):
        os.killpg(process.pid, signal.SIGKILL)


def _remove_clips_container(container_name: str) -> None:
    """Force-remove the uniquely named observer container after interruption."""
    if re.fullmatch(r"ferric-compat-[0-9a-f]{32}", container_name) is None:
        raise ValueError("refusing to remove an unrecognized CLIPS container name")
    # The local process group is already dead. A missing daemon/container is
    # expected when startup lost the race with the timeout.
    with contextlib.suppress(OSError, subprocess.SubprocessError):
        subprocess.run(
            ["docker", "rm", "--force", container_name],
            capture_output=True,
            check=False,
            timeout=5,
        )


def _run_clips_process(
    command: list[str],
    *,
    timeout_secs: int,
    root: str,
    container_name: str | None = None,
) -> subprocess.CompletedProcess:
    """Run the shell/Docker observer with a wall-clock process-tree timeout."""
    popen_options: dict[str, object] = {
        "stdout": subprocess.PIPE,
        "stderr": subprocess.PIPE,
        "text": False,
        "cwd": root,
    }
    if IS_WINDOWS:
        popen_options["creationflags"] = subprocess.CREATE_NEW_PROCESS_GROUP
    else:
        popen_options["start_new_session"] = True
    process = subprocess.Popen(command, **popen_options)
    try:
        stdout, stderr = process.communicate(timeout=timeout_secs)
    except subprocess.TimeoutExpired as error:
        _terminate_process_tree(process)
        if container_name is not None:
            _remove_clips_container(container_name)
        try:
            stdout, stderr = process.communicate(timeout=5)
        except subprocess.TimeoutExpired:
            stdout, stderr = error.output, error.stderr
        raise subprocess.TimeoutExpired(
            command,
            timeout_secs,
            output=stdout,
            stderr=stderr,
        ) from error
    return subprocess.CompletedProcess(command, process.returncode, stdout, stderr)


def run_ferric_observer(
    file_path: str,
    ferric_bin: str,
    root: str,
    timeout_secs: int,
    *,
    fixture_id: str,
    nonce: str,
    source_sha256: str,
    composed_sha256: str,
    scenario_path: str | None = None,
) -> dict:
    """Run the dedicated Ferric compatibility observer."""
    start = time.monotonic()
    command = [
        ferric_bin,
        "compat-observe",
        "--fixture-id",
        fixture_id,
        "--nonce",
        nonce,
        "--source-sha256",
        source_sha256,
        "--composed-sha256",
        composed_sha256,
    ]
    if scenario_path is None:
        command.append(file_path)
    else:
        command.extend(["--scenario", scenario_path])
    try:
        proc = subprocess.run(
            command,
            capture_output=True,
            text=False,
            timeout=timeout_secs,
            cwd=root,
        )
        raw_stdout = _output_bytes(proc.stdout)
        raw_stderr = _output_bytes(proc.stderr)
        duration_ms = int((time.monotonic() - start) * 1000)
        result = _attach_raw_output(
            _with_termination(
                {
                    "exit_code": proc.returncode,
                    "stdout": _display_output(raw_stdout),
                    "stderr": _display_output(raw_stderr),
                    "duration_ms": duration_ms,
                    "timed_out": False,
                }
            ),
            stdout=raw_stdout,
            stderr=raw_stderr,
        )
    except subprocess.TimeoutExpired as error:
        raw_stdout = _output_bytes(error.stdout)
        raw_stderr = _output_bytes(error.stderr)
        duration_ms = int((time.monotonic() - start) * 1000)
        return _attach_raw_output(
            _with_termination(
                {
                    "exit_code": None,
                    "stdout": _display_output(raw_stdout),
                    "stderr": _display_output(raw_stderr) or f"timeout after {timeout_secs}s",
                    "duration_ms": duration_ms,
                    "timed_out": True,
                    "observation_error": "observer timed out before terminal evidence",
                }
            ),
            stdout=raw_stdout,
            stderr=raw_stderr,
        )
    except OSError as error:
        return _attach_raw_output(
            _with_termination(
                {
                    "exit_code": None,
                    "stdout": "",
                    "stderr": f"cannot start ferric observer {ferric_bin}: {error}",
                    "duration_ms": 0,
                    "timed_out": False,
                    "observation_error": "observer executable could not be started",
                },
                spawn_error=True,
            ),
            stdout=b"",
            stderr=b"",
        )

    try:
        if result["stdout"].count("\n") != 1 or not result["stdout"].endswith("\n"):
            raise ValueError("stdout must contain exactly one newline-terminated JSON object")
        observation = json.loads(result["stdout"])
        if type(observation) is not dict:
            raise ValueError("observation must be a JSON object")
    except (json.JSONDecodeError, ValueError) as error:
        if (result.get("termination") or {}).get("kind") == "signal":
            result["observation_error"] = "observer was signaled before terminal evidence"
        else:
            result["observation_error"] = str(error)
            result["harness_error"] = True
    else:
        result["observation"] = observation
    return result


def run_clips_observer(
    file_path: str,
    root: str,
    script: str,
    timeout_secs: int,
    *,
    fixture_id: str,
    nonce: str,
    source_sha256: str,
    composed_sha256: str,
    globals_to_capture: tuple[str, ...],
    harness_identity: str | None = None,
    scenario_path: str | None = None,
    expected_phases: tuple[str, ...] | None = None,
) -> dict:
    """Run reference CLIPS quietly and parse its nonce-bound post-run probe."""
    resolved_path: Path | None = None
    resolved_scenario: Path | None = None
    if scenario_path is None:
        resolved_path = _require_contained_file(
            Path(file_path),
            root=Path(root),
            label="CLIPS input",
        )
    else:
        resolved_scenario = _require_contained_file(
            Path(scenario_path),
            root=Path(root),
            label="CLIPS scenario plan",
        )
    operations = build_probe_operations(
        fixture_id=fixture_id,
        nonce=nonce,
        source_sha256=source_sha256,
        composed_sha256=composed_sha256,
        globals_to_capture=globals_to_capture,
    )
    auth_key = secrets.token_hex(32)
    container_name = f"ferric-compat-{auth_key[:32]}"
    command = [
        script,
        "run",
        "--quiet",
        "--observer-nonce",
        nonce,
        "--observer-fixture-id",
        fixture_id,
        "--observer-source-sha256",
        source_sha256,
        "--observer-composed-sha256",
        composed_sha256,
        "--observer-auth-key",
        auth_key,
        "--observer-container-name",
        container_name,
    ]
    if resolved_scenario is None:
        assert resolved_path is not None
        command.extend(["--file", str(resolved_path)])
    else:
        command.extend(["--scenario", str(resolved_scenario)])
    for operation in operations:
        command.extend(["--op", operation])

    start = time.monotonic()
    raw_stdout = b""
    raw_stderr = b""
    try:
        proc = _run_clips_process(
            command,
            timeout_secs=timeout_secs,
            root=root,
            container_name=container_name,
        )
        raw_stdout = _output_bytes(proc.stdout)
        raw_stderr = _output_bytes(proc.stderr)
        duration_ms = int((time.monotonic() - start) * 1000)
        result = _attach_raw_output(
            _with_termination(
                {
                    "exit_code": proc.returncode,
                    "stdout": _display_output(raw_stdout),
                    "stderr": _display_output(raw_stderr),
                    "duration_ms": duration_ms,
                    "timed_out": False,
                }
            ),
            stdout=raw_stdout,
            stderr=raw_stderr,
        )
        result["termination"] = _clips_wrapper_termination(proc.returncode)
        if proc.returncode in {125, 126, 127}:
            result["harness_error"] = True
            result["observation_error"] = (
                f"reference observer transport failed with status {proc.returncode}"
            )
    except subprocess.TimeoutExpired as error:
        duration_ms = int((time.monotonic() - start) * 1000)
        raw_stdout = _output_bytes(error.stdout)
        raw_stderr = _output_bytes(error.stderr)
        partial_stdout = _display_output(raw_stdout)
        partial_stderr = _display_output(raw_stderr)
        result = _attach_raw_output(
            _with_termination(
                {
                    "exit_code": None,
                    "stdout": partial_stdout,
                    "stderr": partial_stderr or f"timeout after {timeout_secs}s",
                    "duration_ms": duration_ms,
                    "timed_out": True,
                    "observation_error": "reference observer timed out before terminal evidence",
                }
            ),
            stdout=raw_stdout,
            stderr=raw_stderr,
        )
    except OSError as error:
        return _attach_raw_output(
            _with_termination(
                {
                    "exit_code": None,
                    "stdout": "",
                    "stderr": f"cannot start reference observer {script}: {error}",
                    "duration_ms": 0,
                    "timed_out": False,
                    "observation_error": "reference observer executable could not be started",
                },
                spawn_error=True,
            ),
            stdout=b"",
            stderr=b"",
        )

    try:
        observation = parse_probe_output(
            raw_stdout,
            raw_stderr=raw_stderr,
            fixture_id=fixture_id,
            nonce=nonce,
            source_sha256=source_sha256,
            composed_sha256=composed_sha256,
            auth_key=auth_key,
            expected_harness_identity=harness_identity,
            interrupted=(result.get("termination") or {}).get("kind") in {"signal", "timeout"},
            expected_phases=expected_phases,
        )
    except ClipsOracleProtocolError as error:
        result["observation_error"] = str(error)
        result["harness_error"] = True
    except (ValueError, RuntimeError) as error:
        if (result.get("termination") or {}).get("kind") in {"signal", "timeout"}:
            result.setdefault(
                "observation_error",
                "reference observer was interrupted before parsable terminal evidence",
            )
        else:
            result["observation_error"] = str(error)
            result["harness_error"] = True
    else:
        result["observation"] = observation
    result["observer_auth_key"] = auth_key
    return result


# ---------------------------------------------------------------------------
# Classification
# ---------------------------------------------------------------------------


def oracle_outcome(evaluation) -> tuple[str, str, dict]:
    """Translate one strict oracle evaluation into manifest fields."""
    evaluation_dict = evaluation_to_dict(evaluation)
    mismatches = evaluation_dict["mismatches"]
    declaration_valid = evaluation.declaration.status is EvidenceStatus.VALID
    observations_valid = (
        evaluation.ferric.status is EvidenceStatus.VALID
        and evaluation.clips.status is EvidenceStatus.VALID
    )
    observations_completed = observations_valid and all(
        evidence.value is not None and evidence.value.run.halt_reason != "not-run"
        for evidence in (evaluation.ferric, evaluation.clips)
    )
    effect_mismatch = any(mismatch["field"] == "effects" for mismatch in mismatches)
    normalizations = (
        list(evaluation.declaration.value.normalizers)
        if declaration_valid and evaluation.declaration.value is not None
        else []
    )
    violations = [
        f"{mismatch['scope']}.{mismatch['field']}: {mismatch['message']}" for mismatch in mismatches
    ]
    evidence = {
        "status": evaluation.status.value,
        "version": (
            evaluation.declaration.value.version
            if declaration_valid and evaluation.declaration.value is not None
            else DECLARATION_VERSION
        ),
        "declaration": declaration_valid,
        "reached": observations_valid,
        "completed": observations_completed,
        "effect": observations_completed and not effect_mismatch,
        "normalizations": normalizations,
        "violations": violations,
        "evaluation": evaluation_dict,
    }

    if evaluation.status is EvidenceStatus.MISSING:
        return "pending", "oracle-missing", evidence
    if evaluation.status is EvidenceStatus.INVALID:
        field = mismatches[0]["field"] if mismatches else "evidence"
        reason_field = re.sub(r"[^a-z0-9]+", "-", field.lower()).strip("-") or "evidence"
        return "pending", f"oracle-invalid:{reason_field}", evidence
    if evaluation.equivalent:
        assert evaluation.declaration.value is not None
        return (
            "equivalent",
            f"oracle-v{evaluation.declaration.value.version}-match",
            evidence,
        )

    field = mismatches[0]["field"] if mismatches else "semantic"
    reason_field = re.sub(r"[^a-z0-9]+", "-", field.lower()).strip("-") or "semantic"
    return "divergent", f"oracle-{reason_field}-mismatch", evidence


# Retained for callers testing the historical internal boundary.
_oracle_outcome = oracle_outcome


def classify_results(
    ferric_result: dict,
    clips_result: dict | None,
    evaluation=None,
) -> tuple[str, str]:
    """Classify only from independently validated structured evidence."""
    diagnostic_outcome = _diagnostic_outcome(ferric_result, clips_result)
    if evaluation is None:
        return diagnostic_outcome or ("pending", "oracle-missing")
    if diagnostic_outcome is not None:
        classification, reason = diagnostic_outcome
        complete_terminal_oracle = (
            classification == "incompatible"
            and reason == "diagnostic-match-without-complete-oracle"
            and all(
                evidence.status is EvidenceStatus.VALID
                and evidence.value is not None
                and evidence.value.run.halt_reason != "not-run"
                for evidence in (evaluation.ferric, evaluation.clips)
            )
        )
        if not complete_terminal_oracle:
            return diagnostic_outcome
    classification, reason, _evidence = oracle_outcome(evaluation)
    return classification, reason


def _termination_state(result: dict | None) -> tuple[str, tuple[object, ...] | None]:
    if not isinstance(result, dict) or "termination" not in result:
        return "missing", None
    raw = result.get("termination")
    if not isinstance(raw, dict):
        return "invalid", None
    kind = raw.get("kind")
    exit_code = raw.get("exit_code")
    signal = raw.get("signal")
    active_phase = raw.get("active_phase")
    if active_phase is not None and active_phase not in {"load", "reset", "run"}:
        return "invalid", None
    if kind == "exit" and type(exit_code) is int and exit_code >= 0 and signal is None:
        return "valid", (kind, exit_code, None, active_phase)
    if kind == "signal" and exit_code is None and type(signal) is int and signal > 0:
        return "valid", (kind, None, signal, active_phase)
    if kind in {"timeout", "spawn-error"} and exit_code is None and signal is None:
        return "valid", (kind, None, None, active_phase)
    return "invalid", None


def _diagnostic_outcome(
    ferric_result: dict,
    clips_result: dict | None,
) -> tuple[str, str] | None:
    """Classify trusted terminal diagnostics when full oracle evidence is unavailable."""
    ferric_status, ferric = diagnostic_evidence_state(ferric_result)
    clips_status, clips = diagnostic_evidence_state(clips_result)
    statuses = {ferric_status, clips_status}
    if "harness" in statuses:
        return "pending", "harness-failure"
    if "invalid" in statuses:
        return "pending", "diagnostic-invalid"
    if ferric_status == clips_status == "missing":
        return None
    if "missing" in statuses:
        known = clips if ferric is None else ferric
        if known is not None and known[0] == "none":
            return None
        return "pending", "diagnostic-missing"
    assert ferric is not None and clips is not None
    for index, field in enumerate(("phase", "category", "continued")):
        if ferric[index] != clips[index]:
            return "divergent", f"diagnostic-{field}-mismatch"

    ferric_termination_status, ferric_termination = _termination_state(ferric_result)
    clips_termination_status, clips_termination = _termination_state(clips_result)
    termination_statuses = {ferric_termination_status, clips_termination_status}
    if "invalid" in termination_statuses:
        return "pending", "termination-invalid"
    if termination_statuses == {"missing"}:
        pass
    elif "missing" in termination_statuses:
        return "pending", "termination-missing"
    else:
        assert ferric_termination is not None and clips_termination is not None
        for index, field in enumerate(("kind", "exit-code", "signal", "active-phase")):
            if ferric_termination[index] != clips_termination[index]:
                return "divergent", f"termination-{field}-mismatch"
        if ferric_termination[0] in {"timeout", "signal"}:
            return "incompatible", "termination-match-without-complete-oracle"
    if ferric[0] != "none" and ferric[2] is False:
        return "incompatible", "diagnostic-match-without-complete-oracle"
    if ferric_termination is not None and (
        ferric_termination[0] == "exit" and ferric_termination[1] != 0
    ):
        return "incompatible", "termination-nonzero-exit-match"
    return None


def _missing_oracle_evidence() -> dict:
    """Return the canonical manifest view for an undeclared fixture."""
    return {
        "status": "missing",
        "version": DECLARATION_VERSION,
        "declaration": False,
        "reached": False,
        "completed": False,
        "effect": False,
        "normalizations": [],
        "violations": [],
    }


def _invalid_observation(error: str) -> dict[str, str]:
    """Create deliberately invalid input for the strict oracle validator."""
    return {"observation_error": error}


def _invalid_preflight_evidence(error: str) -> dict:
    """Return fail-closed evidence for an oracle input rejected before execution."""
    return {
        "status": "invalid",
        "version": DECLARATION_VERSION,
        "declaration": False,
        "reached": False,
        "completed": False,
        "effect": False,
        "normalizations": [],
        "violations": [error],
    }


def _is_empty_clips_interruption(observation: object, *, engine: str) -> bool:
    """Recognize a kill before the native observer emitted authenticated evidence."""
    if engine != "clips" or type(observation) is not dict:
        return False
    protocol_issues = observation.get("protocol_issues")
    return (
        observation.get("lifecycle") == []
        and observation.get("diagnostics") == []
        and observation.get("active_phase") is None
        and type(protocol_issues) is list
        and all(type(issue) is str for issue in protocol_issues)
        and set(protocol_issues).issubset(_EMPTY_INTERRUPTION_PROTOCOL_ISSUES)
    )


def _project_result(
    result: dict,
    *,
    engine: str,
    harness_identity: str | None,
    expected_fixture: dict[str, str] | None = None,
    require_firing_names: bool = False,
    require_globals: bool = False,
) -> dict:
    """Project one successful observer result or return invalid evidence."""
    observation = result.get("observation")
    process = result.get("termination")
    if not isinstance(process, dict):
        process = termination(
            exit_code=result.get("exit_code"),
            timed_out=result.get("timed_out") is True,
            spawn_error=result.get("spawn_error") is True,
        )
        result["termination"] = process
    process_kind = process.get("kind")
    interrupted = process_kind in {"timeout", "signal"}
    if process_kind in {"timeout", "signal", "spawn-error"} and type(observation) is dict:
        instrumentation = observation.get("instrumentation")
        active_phase = (
            instrumentation.get("active_phase") if isinstance(instrumentation, dict) else None
        )
        if active_phase in {"load", "reset", "run"}:
            process["active_phase"] = active_phase

    if result.get("harness_error") is True:
        error = result.get("observation_error", f"{engine} observer harness failed")
        result["diagnostic"] = diagnostic("harness", "harness-error", continued=False)
        result["projection_error"] = error
        return _invalid_observation(error)

    trusted_diagnostic: dict | None = None
    validate_diagnostic_subset = not interrupted or not _is_empty_clips_interruption(
        observation,
        engine=engine,
    )
    if (
        expected_fixture is not None
        and type(observation) is dict
        and observation.get("schema") == "ferric.compat-observation"
        and validate_diagnostic_subset
    ):
        try:
            trusted_diagnostic = project_observation_diagnostic(
                observation,
                engine=engine,
                expected_fixture=expected_fixture,
                interrupted=interrupted,
            )
        except ObservationProjectionError as error:
            result["diagnostic"] = diagnostic("harness", "harness-error", continued=False)
            result["projection_error"] = str(error)
            return _invalid_observation(str(error))
    if process_kind in {"timeout", "signal", "spawn-error"}:
        error = result.get(
            "observation_error",
            f"{engine} observer terminated as {process_kind}",
        )
        if trusted_diagnostic is not None and trusted_diagnostic["phase"] != "none":
            result["diagnostic"] = diagnostic(
                trusted_diagnostic["phase"],
                trusted_diagnostic["category"],
                continued=trusted_diagnostic["continued"],
            )
        else:
            result["diagnostic"] = process_diagnostic(result)
        result["projection_error"] = error
        return _invalid_observation(error)
    if engine == "ferric" and result.get("stderr"):
        error = "ferric observer emitted out-of-band stderr"
        result["diagnostic"] = diagnostic("harness", "harness-error", continued=False)
        result["projection_error"] = error
        return _invalid_observation(error)
    if type(observation) is not dict:
        error = result.get("observation_error", f"{engine} observation is missing")
        result["diagnostic"] = process_diagnostic(result) or diagnostic(
            "harness", "harness-error", continued=False
        )
        result["projection_error"] = error
        return _invalid_observation(error)

    try:
        if engine == "ferric":
            projected = project_ferric_observation(
                observation,
                harness_identity=harness_identity,
                require_firing_names=require_firing_names,
                require_globals=require_globals,
            )
        else:
            projected = project_clips_observation(
                observation,
                harness_identity=harness_identity,
                require_firing_names=require_firing_names,
            )
    except ObservationProjectionError as error:
        if trusted_diagnostic is not None and trusted_diagnostic["phase"] != "none":
            result["diagnostic"] = diagnostic(
                trusted_diagnostic["phase"],
                trusted_diagnostic["category"],
                continued=trusted_diagnostic["continued"],
            )
        else:
            result["diagnostic"] = diagnostic("harness", "harness-error", continued=False)
        result["projection_error"] = str(error)
        return _invalid_observation(str(error))

    canonical_diagnostic = projected.get("diagnostic")
    if type(canonical_diagnostic) is not dict:
        error = f"{engine} projected diagnostic is malformed"
        result["diagnostic"] = diagnostic("harness", "harness-error", continued=False)
        result["projection_error"] = error
        return _invalid_observation(error)
    result["diagnostic"] = diagnostic(
        canonical_diagnostic["phase"],
        canonical_diagnostic["category"],
        continued=canonical_diagnostic["continued"],
    )
    if result.get("exit_code") != 0 and canonical_diagnostic.get("phase") == "none":
        error = f"{engine} observer exited with status {result.get('exit_code')!r}"
        result["diagnostic"] = process_diagnostic(result)
        result["projection_error"] = error
        return _invalid_observation(error)

    result["canonical_observation"] = projected
    return projected


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
        *oracle_args,
    ) = args
    if len(oracle_args) != 1 or oracle_args[0] is None:
        raise TypeError("compatibility work item requires one structured oracle declaration")
    declaration = oracle_args[0]
    harness_identity = harness.verifier_identity if harness is not None else None

    run_path = abs_path
    composed_path: Path | None = None
    scenario_path: Path | None = None
    scenario_bundle: ResolvedScenarioBundle | None = None
    composed_content: bytes | None = None
    composed_digest: str | None = None
    retained_artifact: Path | None = None
    resolved_root = Path(root).resolve(strict=True)
    runtime_declaration = copy.deepcopy(declaration)
    runtime_declaration["nonce"] = secrets.token_hex(16)
    is_scenario = runtime_declaration.get("version") == SCENARIO_DECLARATION_VERSION
    try:
        if is_scenario:
            if harness is not None:
                raise CompatibilityWorkspaceError(
                    f"{rel_path}: v2 scenario cannot also use a legacy generated harness"
                )
            if run_workspace is None:
                raise CompatibilityWorkspaceError(
                    f"{rel_path}: scenario execution requires an invocation workspace"
                )
            try:
                composed_content = canonical_scenario_plan(runtime_declaration)
            except ValueError as error:
                raise CompatibilityWorkspaceError(
                    f"{rel_path}: invalid scenario declaration: {error}"
                ) from error
            composed_digest = sha256_bytes(composed_content)
            primary_path = _require_contained_file(
                Path(abs_path),
                root=resolved_root / "tests" / "examples",
                label=f"{rel_path} primary source",
            )
            try:
                validate_scenario_source_sizes((primary_path.stat().st_size,))
                source_content = primary_path.read_bytes()
                validate_scenario_source_sizes((len(source_content),))
            except ValueError as error:
                raise CompatibilityWorkspaceError(f"{rel_path}: {error}") from error
            except OSError as error:
                raise CompatibilityWorkspaceError(
                    f"{rel_path}: cannot read primary source: {error}"
                ) from error
            source_digest = sha256_bytes(source_content)
            declaration_evidence = validate_declaration(
                runtime_declaration,
                expected_source_sha256=source_digest,
                expected_composed_sha256=composed_digest,
            )
            if declaration_evidence.status is EvidenceStatus.VALID:
                validated_declaration = declaration_evidence.value
                assert validated_declaration is not None
                scenario_bundle = _resolve_scenario_bundle(
                    validated_declaration,
                    registry_key=rel_path,
                    primary_path=primary_path,
                    root=resolved_root,
                )
                scenario_path = _materialize_scenario_plan(
                    scenario_bundle.plan,
                    workspace=Path(run_workspace),
                    root=resolved_root,
                )
                run_path = str(primary_path)
        elif harness is not None:
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
            source_digest = sha256_bytes(harness.source_bytes)
            declaration_evidence = validate_declaration(
                runtime_declaration,
                expected_source_sha256=source_digest,
                expected_composed_sha256=composed_digest,
            )
        else:
            if run_workspace is None:
                raise CompatibilityWorkspaceError(
                    f"{rel_path}: oracle execution requires an invocation workspace"
                )
            source_path = _require_contained_file(
                Path(abs_path),
                root=resolved_root,
                label=f"{rel_path} source",
            )
            try:
                composed_content = source_path.read_bytes()
            except OSError as error:
                raise CompatibilityWorkspaceError(
                    f"{rel_path}: cannot read compatibility source: {error}"
                ) from error
            composed_digest = sha256_bytes(composed_content)
            composed_path = _materialize_composed_source(
                composed_content,
                workspace=Path(run_workspace),
                root=resolved_root,
            )
            run_path = str(composed_path)
            source_digest = sha256_bytes(composed_content)
            declaration_evidence = validate_declaration(
                runtime_declaration,
                expected_source_sha256=source_digest,
                expected_composed_sha256=composed_digest,
            )

        if composed_path is not None and composed_content is not None:
            _verify_composed_source(
                composed_path,
                composed_content,
                root=resolved_root,
                boundary="before Ferric execution",
            )
        if scenario_path is not None and scenario_bundle is not None:
            _verify_scenario_plan(
                scenario_path,
                scenario_bundle.plan,
                root=resolved_root,
                boundary="before Ferric execution",
            )
            assert declaration_evidence.value is not None
            _verify_scenario_bundle(
                scenario_bundle,
                declaration_evidence.value,
                root=resolved_root,
                boundary="before Ferric execution",
            )
        assert composed_content is not None
        assert composed_digest is not None

        if declaration_evidence.status is EvidenceStatus.VALID:
            fixture_id = runtime_declaration["id"]
            nonce = runtime_declaration["nonce"]
            expected_fixture = {
                "id": fixture_id,
                "nonce": nonce,
                "source_sha256": source_digest,
                "composed_sha256": composed_digest,
            }
            ferric_result = run_ferric_observer(
                run_path,
                ferric,
                root,
                timeout,
                fixture_id=fixture_id,
                nonce=nonce,
                source_sha256=source_digest,
                composed_sha256=composed_digest,
                **({"scenario_path": str(scenario_path)} if scenario_path is not None else {}),
            )
            ferric_observation = _project_result(
                ferric_result,
                engine="ferric",
                harness_identity=harness_identity,
                expected_fixture=expected_fixture,
                require_firing_names=(
                    runtime_declaration["expectations"]["firings"]["names"] is not None
                ),
                require_globals=(runtime_declaration["expectations"]["globals"] is not None),
            )
        else:
            ferric_result = {
                "exit_code": None,
                "stdout": "",
                "stderr": "oracle declaration is invalid for the current source",
                "duration_ms": 0,
                "timed_out": False,
                "observation_error": "oracle declaration validation failed",
                "not_run": True,
            }
            ferric_observation = None

        if composed_path is not None:
            _verify_composed_source(
                composed_path,
                composed_content,
                root=resolved_root,
                boundary="before CLIPS execution",
            )
        if scenario_path is not None and scenario_bundle is not None:
            _verify_scenario_plan(
                scenario_path,
                scenario_bundle.plan,
                root=resolved_root,
                boundary="before CLIPS execution",
            )
            assert declaration_evidence.value is not None
            _verify_scenario_bundle(
                scenario_bundle,
                declaration_evidence.value,
                root=resolved_root,
                boundary="before CLIPS execution",
            )

        if declaration_evidence.status is EvidenceStatus.VALID and not skip_clips:
            globals_expectation = runtime_declaration["expectations"]["globals"]
            globals_to_capture = (
                ()
                if globals_expectation is None
                else tuple(item["name"] for item in globals_expectation)
            )
            clips_result = run_clips_observer(
                run_path,
                root,
                script,
                timeout,
                fixture_id=fixture_id,
                nonce=nonce,
                source_sha256=source_digest,
                composed_sha256=composed_digest,
                globals_to_capture=globals_to_capture,
                harness_identity=harness_identity,
                **(
                    {
                        "scenario_path": str(scenario_path),
                        "expected_phases": scenario_bundle.native_phases,
                    }
                    if scenario_path is not None and scenario_bundle is not None
                    else {}
                ),
            )
            clips_observation = _project_result(
                clips_result,
                engine="clips",
                harness_identity=harness_identity,
                expected_fixture=expected_fixture,
                require_firing_names=(
                    runtime_declaration["expectations"]["firings"]["names"] is not None
                ),
            )
        else:
            clips_result = {
                "exit_code": None,
                "stdout": "",
                "stderr": (
                    "reference observer was skipped"
                    if skip_clips
                    else "oracle declaration is invalid for the current source"
                ),
                "duration_ms": 0,
                "timed_out": False,
                "observation_error": (
                    "reference observer was skipped"
                    if skip_clips
                    else "oracle declaration validation failed"
                ),
                "not_run": True,
            }
            clips_observation = (
                _invalid_observation("reference observer was skipped") if skip_clips else None
            )

        evaluation = evaluate_oracle(
            runtime_declaration,
            ferric_observation,
            clips_observation,
            expected_source_sha256=source_digest,
            expected_composed_sha256=composed_digest,
        )
        _oracle_classification, _oracle_reason, evidence = oracle_outcome(evaluation)
        classification, reason = classify_results(
            ferric_result,
            clips_result,
            evaluation=evaluation,
        )
        for engine_name, engine_result in (
            ("ferric", ferric_result),
            ("clips", clips_result),
        ):
            if projection_error := engine_result.get("projection_error"):
                evidence["violations"].append(f"{engine_name}.projection: {projection_error}")
        ferric_result["oracle_evidence"] = evidence
        clips_result["oracle_evidence"] = evidence

        if harness is not None:
            ferric_result["harness"] = dict(harness.metadata)
            if clips_result is not None:
                clips_result["harness"] = dict(harness.metadata)
        if composed_path is not None and composed_content is not None:
            _verify_composed_source(
                composed_path,
                composed_content,
                root=resolved_root,
                boundary="after engine execution",
            )
        if scenario_path is not None and scenario_bundle is not None:
            _verify_scenario_plan(
                scenario_path,
                scenario_bundle.plan,
                root=resolved_root,
                boundary="after engine execution",
            )
            assert declaration_evidence.value is not None
            _verify_scenario_bundle(
                scenario_bundle,
                declaration_evidence.value,
                root=resolved_root,
                boundary="after engine execution",
            )

        retain_failure = classification in FAILED_CLASSIFICATIONS or reason.startswith(
            ("oracle-invalid", "diagnostic-", "termination-", "harness-")
        )
        if composed_content is not None and composed_digest is not None:
            composed_metadata: dict[str, str | int] = {
                "sha256": composed_digest,
                "size_bytes": len(composed_content),
            }
            if retain_failure:
                retained_artifact = _retain_failure_artifact(
                    composed_content,
                    composed_digest,
                    failures_dir=Path(failures_dir),
                    root=resolved_root,
                    suffix=".plan" if is_scenario else ".clp",
                )
                composed_metadata["artifact_path"] = retained_artifact.relative_to(
                    resolved_root
                ).as_posix()
            metadata_field = "scenario_plan" if is_scenario else "composed_source"
            ferric_result[metadata_field] = dict(composed_metadata)
            if clips_result is not None:
                clips_result[metadata_field] = dict(composed_metadata)

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
                    suffix=".plan" if is_scenario else ".clp",
                )
            except Exception as retention_error:
                error.add_note(f"could not retain composed failure artifact: {retention_error}")
            else:
                artifact_relpath = retained_artifact.relative_to(resolved_root).as_posix()
                error.add_note(f"composed failure artifact retained at {artifact_relpath}")
        raise
    finally:
        for temporary_path, label in (
            (composed_path, "composed temporary source"),
            (scenario_path, "temporary scenario plan"),
        ):
            if temporary_path is None:
                continue
            try:
                temporary_path.unlink(missing_ok=True)
            except OSError as cleanup_error:
                active_error = sys.exception()
                message = f"could not remove {label} {temporary_path}: {cleanup_error}"
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


def _recompute_summary(manifest_data: dict) -> dict[str, int]:
    """Recompute compatibility totals after selection or execution updates."""
    summary = {
        "total": 0,
        "equivalent": 0,
        "divergent": 0,
        "incompatible": 0,
        "pending": 0,
    }
    for info in manifest_data["files"].values():
        summary["total"] += 1
        classification = info.get("classification")
        if classification in summary:
            summary[classification] += 1
    manifest_data["summary"] = summary
    return summary


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
    candidate_sha: Annotated[
        str | None,
        typer.Option(help="Exact 40-character Git commit SHA represented by the Ferric candidate"),
    ] = None,
    require_selected: Annotated[
        bool,
        typer.Option(help="Fail when no structured-oracle fixture is selected"),
    ] = False,
    skip_clips: Annotated[bool, typer.Option(help="Skip Docker CLIPS (ferric-only)")] = False,
    dry_run: Annotated[bool, typer.Option(help="Show files without running")] = False,
) -> None:
    """Run CLIPS compatibility assessment."""
    root = repo_root()
    ed = default_examples_dir()
    manifest_path = Path(manifest) if manifest else ed / "compat-manifest.json"
    ferric = ferric_bin_path or str(default_ferric_bin())
    script = str(default_harness_script())

    if require_selected and candidate_sha is None:
        console.print("[red]error:[/] --require-selected also requires --candidate-sha")
        raise typer.Exit(1)
    if candidate_sha is not None and _COMMIT_SHA_RE.fullmatch(candidate_sha) is None:
        console.print(
            "[red]error:[/] --candidate-sha must be exactly 40 lowercase hexadecimal characters"
        )
        raise typer.Exit(1)

    if not manifest_path.exists():
        console.print(f"[red]error:[/] manifest not found: {manifest_path}")
        console.print("Run ferric-compat-scan first.")
        raise typer.Exit(1)

    mdata = load_manifest(manifest_path)
    if (
        mdata.get("version") != 3
        or mdata.get("oracle_protocol_version") != ORACLE_PROTOCOL_VERSION
        or type(mdata.get("files")) is not dict
    ):
        console.print(
            "[red]error:[/] manifest is not structured-oracle schema v3; "
            "run ferric-compat-scan first"
        )
        raise typer.Exit(1)

    # Select files and validate all library harnesses before starting engines.
    files_to_run: list[tuple[str, str]] = []
    resolved_harnesses: dict[str, ResolvedHarness | None] = {}
    declarations: dict[str, dict] = {}
    harness_targets: dict[Path, str] = {}
    missing_selected: list[str] = []
    invalid_selected: list[tuple[str, str]] = []
    file_was_found = False
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
            file_was_found = True
            if runability == "unknown":
                console.print(
                    f"[red]error:[/] {rel_path}: cannot explicitly run a file "
                    "with unknown runability"
                )
                raise typer.Exit(1)
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

        declaration = info.get("oracle")
        if declaration is None:
            missing_selected.append(rel_path)
            if not dry_run:
                info["classification"] = "pending"
                info["reason"] = "oracle-missing"
                info["oracle_evidence"] = _missing_oracle_evidence()
                info["ferric"] = None
                info["clips"] = None
            continue
        if type(declaration) is not dict:
            message = "oracle declaration must be an object"
            invalid_selected.append((rel_path, message))
            if not dry_run:
                info["classification"] = "pending"
                info["reason"] = "oracle-invalid:declaration"
                info["oracle_evidence"] = _invalid_preflight_evidence(message)
                info["ferric"] = None
                info["clips"] = None
            continue

        generated = info.get("generated")
        if generated:
            abs_path = root / generated
        elif rel_path.startswith("generated/"):
            abs_path = root / "tests" / rel_path
        else:
            abs_path = ed / rel_path
        if runability == "library" and declaration.get("version") != SCENARIO_DECLARATION_VERSION:
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
        else:
            try:
                _require_contained_file(
                    abs_path,
                    root=root,
                    label=f"{rel_path} source",
                )
            except CompatibilityWorkspaceError as error:
                message = str(error)
                invalid_selected.append((rel_path, message))
                if not dry_run:
                    info["classification"] = "pending"
                    info["reason"] = "oracle-invalid:source"
                    info["oracle_evidence"] = _invalid_preflight_evidence(message)
                    info["ferric"] = None
                    info["clips"] = None
                continue
            resolved_harnesses[rel_path] = None

        files_to_run.append((rel_path, str(abs_path)))
        declarations[rel_path] = declaration

    if file and not file_was_found:
        console.print(f"[red]error:[/] file is absent from the manifest: {file}")
        raise typer.Exit(1)
    if file and missing_selected:
        if not dry_run:
            _recompute_summary(mdata)
            save_manifest(manifest_path, mdata)
        console.print(
            f"[red]error:[/] {file}: no structured oracle declaration; the fixture remains pending"
        )
        if not dry_run:
            console.print(f"Manifest updated: {manifest_path}")
        raise typer.Exit(1)
    if invalid_selected:
        if not dry_run:
            _recompute_summary(mdata)
            save_manifest(manifest_path, mdata)
        for rel_path, message in invalid_selected:
            console.print(f"[red]error:[/] {rel_path}: {message}")
        if not dry_run:
            console.print(f"Manifest updated: {manifest_path}")
        raise typer.Exit(1)

    if not files_to_run:
        if not dry_run and missing_selected:
            _recompute_summary(mdata)
            save_manifest(manifest_path, mdata)
            print(
                f"No oracle-backed files to run; "
                f"{len(missing_selected)} selected file(s) remain pending."
            )
            print(f"Manifest updated: {manifest_path}")
        else:
            print("No oracle-backed files to run.")
        if require_selected:
            console.print("[red]error:[/] required compatibility selection is empty")
            raise typer.Exit(1)
        raise typer.Exit(0)

    print(f"Files to run: {len(files_to_run)}")
    if missing_selected:
        print(f"Pending without oracle: {len(missing_selected)}")
    print(f"Timeout: {timeout}s per engine")
    print(f"Workers: {workers}")
    print()

    if dry_run:
        for rel_path, _ in files_to_run:
            print(f"  {rel_path}")
        raise typer.Exit(0)

    try:
        ferric_path = Path(ferric)
        if ferric_path.is_symlink():
            raise CandidateProvenanceError(f"candidate binary must not be a symlink: {ferric}")
        resolved_ferric = ferric_path.resolve(strict=True)
        if not resolved_ferric.is_file():
            raise CandidateProvenanceError(f"candidate binary is not a regular file: {ferric}")
        candidate = (
            candidate_provenance(resolved_ferric, commit_sha=candidate_sha)
            if candidate_sha is not None
            else None
        )
        ferric = str(resolved_ferric)
    except (OSError, CandidateProvenanceError) as error:
        console.print(f"[red]error:[/] {error}")
        console.print("Run: cargo build --release -p ferric-rules-cli")
        raise typer.Exit(1) from error
    persisted_candidate = mdata.get("candidate")
    if persisted_candidate is not None and candidate is None:
        console.print("[red]error:[/] manifest candidate requires --candidate-sha verification")
        raise typer.Exit(1)
    if persisted_candidate is not None and persisted_candidate != candidate:
        console.print("[red]error:[/] manifest candidate provenance does not match the binary")
        raise typer.Exit(1)
    if candidate is not None:
        mdata["candidate"] = candidate
        # Persist candidate identity before the Docker/reference boundary so a
        # missing or broken reference still leaves an attributable artifact.
        save_manifest(manifest_path, mdata)
    if skip_clips:
        console.print(
            "[red]error:[/] --skip-clips cannot produce structured compatibility evidence"
        )
        raise typer.Exit(1)
    try:
        docker_result = subprocess.run(
            ["docker", "image", "inspect", "ferric-rules/clips-reference:latest"],
            capture_output=True,
            timeout=10,
        )
    except (FileNotFoundError, subprocess.TimeoutExpired) as error:
        console.print(f"[red]error:[/] Docker CLIPS is unavailable: {error}")
        raise typer.Exit(1) from error
    if docker_result.returncode != 0:
        console.print(
            "[red]error:[/] Docker CLIPS image not found: ferric-rules/clips-reference:latest"
        )
        raise typer.Exit(1)
    try:
        reference = load_reference_provenance(script, root=str(root))
    except ReferenceProvenanceError as error:
        console.print(f"[red]error:[/] {error}")
        raise typer.Exit(1) from error
    mdata["reference"] = reference

    completed = 0
    results: dict[str, tuple] = {}
    start_time = time.monotonic()

    execution_error: Exception | None = None
    try:
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
                    declarations[rel],
                )
                for rel, abs_p in files_to_run
            ]

            for result_tuple in parallel_run(process_file, work_items, workers=workers):
                if type(result_tuple) is not tuple or len(result_tuple) != 5:
                    raise RuntimeError("compatibility worker returned a malformed result")
                rel, ferric_result, clips_result, classification, reason = result_tuple
                if rel not in declarations or rel in results:
                    raise RuntimeError(
                        f"compatibility worker returned an unexpected result for {rel!r}"
                    )
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
    except Exception as error:
        execution_error = error
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
            entry["oracle_evidence"] = ferric_result["oracle_evidence"]

    # Recompute summary
    _recompute_summary(mdata)

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

    invalid = [
        (rel_path, values)
        for rel_path, values in results.items()
        if values[3].startswith("oracle-invalid")
    ]
    if invalid:
        print(f"\nInvalid oracle evidence ({len(invalid)}):")
        for rel_path, (ferric_r, _clips_r, _classification, reason) in invalid[:20]:
            print(f"  {rel_path} ({reason})")
            for violation in ferric_r["oracle_evidence"].get("violations", [])[:3]:
                print(f"    {violation}")

    invalid_runtime = [
        (rel_path, values)
        for rel_path, values in results.items()
        if values[3] in INVALID_RUNTIME_EVIDENCE_REASONS
    ]
    if invalid_runtime:
        print(f"\nInvalid runtime evidence ({len(invalid_runtime)}):")
        for rel_path, (_ferric_r, _clips_r, _classification, reason) in invalid_runtime[:20]:
            print(f"  {rel_path} ({reason})")

    if candidate is not None:
        try:
            final_candidate = candidate_provenance(Path(ferric), commit_sha=candidate["commit_sha"])
        except CandidateProvenanceError as error:
            console.print(f"[red]error:[/] {error}")
            raise typer.Exit(1) from error
        if final_candidate != candidate:
            console.print("[red]error:[/] candidate binary changed during compatibility execution")
            raise typer.Exit(1)

    if execution_error is not None:
        console.print(f"[red]error:[/] compatibility execution failed: {execution_error}")
        raise typer.Exit(1) from execution_error

    if completed != len(files_to_run):
        console.print(
            "[red]error:[/] compatibility execution was incomplete: "
            f"completed {completed} of {len(files_to_run)}"
        )
        raise typer.Exit(1)

    if invalid or invalid_runtime:
        raise typer.Exit(1)


if __name__ == "__main__":
    app()
