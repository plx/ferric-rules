"""Harness generation and manifest-contract helpers."""

from __future__ import annotations

import hashlib
import os
import re
import tempfile
from dataclasses import dataclass
from pathlib import Path, PurePosixPath, PureWindowsPath

HARNESS_GENERATION_VERSION = 2
EXTERNAL_DEP_KEYWORDS = ["ros-", "ament-", "blackboard-", "pb-", "navgraph-", "protobuf-"]
SHA256_RE = re.compile(r"[0-9a-f]{64}")
SKIP_REASONS = {"empty", "external-deps"}


class HarnessContractError(ValueError):
    """Raised when harness metadata or files violate the manifest contract."""


@dataclass(frozen=True)
class HarnessPlan:
    """A fully validated harness generation plan for one source fixture."""

    source_path: Path
    source_bytes: bytes
    harness_path: Path | None
    harness_bytes: bytes | None
    verifier_identity: str | None
    metadata: dict[str, object]


@dataclass(frozen=True)
class ResolvedHarness:
    """Validated harness bytes and provenance ready for engine execution."""

    path: Path
    source_bytes: bytes
    harness_bytes: bytes
    verifier_identity: str
    metadata: dict[str, object]


def sha256_bytes(content: bytes) -> str:
    """Return the lowercase SHA-256 digest for *content*."""
    return hashlib.sha256(content).hexdigest()


def has_external_deps(content: str) -> bool:
    """Check if file content references external dependency keywords."""
    content_lower = content.lower()
    return any(keyword in content_lower for keyword in EXTERNAL_DEP_KEYWORDS)


def detect_constructs(content: str) -> dict:
    """Parse file content with simple regexes to detect CLIPS constructs."""
    constructs: dict[str, list] = {
        "deffacts": [],
        "deftemplate": [],
        "defglobal": [],
        "deffunction": [],
        "defgeneric": [],
        "defmethod": [],
        "defmodule": [],
    }

    lines = content.split("\n")
    stripped_lines = [line for line in lines if not line.lstrip().startswith(";")]
    cleaned = "\n".join(stripped_lines)

    for match in re.finditer(r"\(\s*deffacts\s+([\w:.-]+)", cleaned):
        constructs["deffacts"].append(match.group(1))

    for match in re.finditer(r"\(\s*deftemplate\s+([\w:.-]+)", cleaned):
        constructs["deftemplate"].append(match.group(1))

    for match in re.finditer(r"\?\*[\w-]+\*", cleaned):
        variable = match.group(0)
        if variable not in constructs["defglobal"]:
            constructs["defglobal"].append(variable)

    for match in re.finditer(r"\(\s*deffunction\s+([\w:.-]+)\s*\(([^)]*)\)", cleaned):
        name = match.group(1)
        params = match.group(2).strip()
        parameter_count = len(re.findall(r"[\$]?\?\w+", params)) if params else 0
        constructs["deffunction"].append((name, parameter_count))

    for match in re.finditer(r"\(\s*defgeneric\s+([\w:.-]+)", cleaned):
        constructs["defgeneric"].append(match.group(1))

    for match in re.finditer(r"\(\s*defmethod\s+([\w:.-]+)", cleaned):
        name = match.group(1)
        if name not in constructs["defmethod"]:
            constructs["defmethod"].append(name)

    for match in re.finditer(r"\(\s*defmodule\s+([\w:.-]+)", cleaned):
        constructs["defmodule"].append(match.group(1))

    return constructs


def has_any_constructs(constructs: dict) -> bool:
    """Check if any constructs were detected."""
    return any(len(items) > 0 for items in constructs.values())


def harness_verifier_identity(source_relpath: str, source_bytes: bytes) -> str:
    """Return a deterministic verifier identity absent from the source bytes."""
    source_relpath = _normalized_relative_path(
        source_relpath,
        label="source path",
    ).as_posix()
    identity_input = (
        f"ferric-harness-v{HARNESS_GENERATION_VERSION}\0".encode()
        + source_relpath.encode("utf-8")
        + b"\0"
        + sha256_bytes(source_bytes).encode("ascii")
    )
    base = f"ferric-harness-{sha256_bytes(identity_input)}"
    source_text = source_bytes.decode("utf-8", errors="replace").casefold()
    candidate = base
    suffix = 1
    while candidate.casefold() in source_text:
        candidate = f"{base}-{suffix}"
        suffix += 1
    return candidate


def _single_line_comment_field(value: object, *, label: str) -> str:
    """Return a comment field only when it cannot escape onto a new CLIPS line."""
    text = str(value)
    if any(ord(character) < 32 or ord(character) == 127 for character in text):
        raise HarnessContractError(f"{label} must not contain control characters")
    return text


def generate_harness(source_relpath: str, source_bytes: bytes, constructs: dict) -> str:
    """Generate a harness .clp file for the given source file."""
    source_relpath = _normalized_relative_path(
        source_relpath,
        label="source path",
    ).as_posix()
    summary_parts: list[str] = []
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
                names = [
                    (
                        f"{_single_line_comment_field(name, label='construct name')}/"
                        f"{_single_line_comment_field(count, label='construct arity')}"
                    )
                    for name, count in items
                ]
                summary_parts.append(f"{kind}: {', '.join(names)}")
            else:
                names = [_single_line_comment_field(item, label="construct name") for item in items]
                summary_parts.append(f"{kind}: {', '.join(names)}")

    summary = "; ".join(summary_parts) if summary_parts else "no named constructs"
    verifier_id = harness_verifier_identity(source_relpath, source_bytes)
    lines = [
        f"; Harness for {source_relpath}",
        f"; Detected constructs: {summary}",
        ";",
        "; Strategy: prove reset/run reaches an isolated MAIN verifier.",
        "; The source and harness are composed and loaded together before reset.",
        "",
        f"(defrule MAIN::{verifier_id}-verify",
        "   (declare (salience 10000))",
        "   (initial-fact)",
        "   =>",
        (f'   (printout t "FERRIC-HARNESS|{HARNESS_GENERATION_VERSION}|{verifier_id}|START" crlf)'),
        (
            f'   (printout t "FERRIC-HARNESS|{HARNESS_GENERATION_VERSION}|'
            f'{verifier_id}|STATE|focus=" (get-focus) crlf)'
        ),
        (
            f'   (printout t "FERRIC-HARNESS|{HARNESS_GENERATION_VERSION}|'
            f'{verifier_id}|COMPLETE" crlf))'
        ),
        "",
    ]
    return "\n".join(lines)


def compute_harness_path(output_dir: Path, manifest_key: str) -> Path:
    """Compute the harness output path from a normalized manifest key."""
    source_relpath = PurePosixPath(manifest_key)
    harness_name = f"{source_relpath.stem}-harness.clp"
    return output_dir.joinpath(*source_relpath.parent.parts, harness_name)


def _normalized_relative_path(value: str, *, label: str) -> PurePosixPath:
    if not value or "\\" in value:
        raise HarnessContractError(f"{label} must be a normalized POSIX relative path")
    if any(ord(character) < 32 or ord(character) == 127 for character in value):
        raise HarnessContractError(f"{label} must not contain control characters")

    path = PurePosixPath(value)
    windows_path = PureWindowsPath(value)
    if path.is_absolute() or windows_path.is_absolute() or windows_path.drive:
        raise HarnessContractError(f"{label} must be repository-relative")
    if ".." in path.parts or path.as_posix() != value:
        raise HarnessContractError(f"{label} must be a normalized POSIX relative path")
    return path


def _resolved_contained_path(
    path: Path,
    root: Path,
    *,
    label: str,
    must_exist: bool,
) -> Path:
    try:
        resolved_root = root.resolve(strict=True)
        resolved_path = path.resolve(strict=must_exist)
        resolved_path.relative_to(resolved_root)
    except (FileNotFoundError, RuntimeError, ValueError) as error:
        if isinstance(error, FileNotFoundError):
            raise HarnessContractError(f"{label} does not exist: {path}") from error
        raise HarnessContractError(f"{label} escapes repository root: {path}") from error

    if must_exist and not resolved_path.is_file():
        raise HarnessContractError(f"{label} is not a regular file: {path}")
    return resolved_path


def _repository_relative_path(path: Path, root: Path, *, label: str) -> str:
    resolved_root = root.resolve(strict=True)
    resolved_path = _resolved_contained_path(path, root, label=label, must_exist=False)
    return resolved_path.relative_to(resolved_root).as_posix()


def build_harness_plan(
    manifest_key: str,
    *,
    examples_dir: Path,
    output_dir: Path,
    root: Path,
) -> HarnessPlan:
    """Build a deterministic harness plan and metadata for one library fixture."""
    source_relpath = _normalized_relative_path(manifest_key, label="source path")
    source_candidate = examples_dir.joinpath(*source_relpath.parts)
    source_path = _resolved_contained_path(
        source_candidate,
        examples_dir,
        label=f"source {manifest_key}",
        must_exist=True,
    )
    source_bytes = source_path.read_bytes()
    source_digest = sha256_bytes(source_bytes)
    content = source_bytes.decode("utf-8", errors="replace")

    skip_reason: str | None = None
    if has_external_deps(content):
        skip_reason = "external-deps"
    else:
        constructs = detect_constructs(content)
        if not has_any_constructs(constructs):
            skip_reason = "empty"

    if skip_reason is not None:
        metadata: dict[str, object] = {
            "path": None,
            "source_sha256": source_digest,
            "harness_sha256": None,
            "generation_version": HARNESS_GENERATION_VERSION,
            "executable": False,
            "skip_reason": skip_reason,
        }
        return HarnessPlan(source_path, source_bytes, None, None, None, metadata)

    harness_text = generate_harness(manifest_key, source_bytes, constructs)
    harness_bytes = harness_text.encode("utf-8")
    harness_path = compute_harness_path(output_dir, manifest_key)
    if harness_path.is_symlink():
        raise HarnessContractError(f"harness output must not be a symlink: {harness_path}")
    harness_relpath = _repository_relative_path(
        harness_path,
        root,
        label=f"harness for {manifest_key}",
    )
    if harness_path.exists() and not harness_path.is_file():
        raise HarnessContractError(f"harness is not a regular file: {harness_path}")

    metadata = {
        "path": harness_relpath,
        "source_sha256": source_digest,
        "harness_sha256": sha256_bytes(harness_bytes),
        "generation_version": HARNESS_GENERATION_VERSION,
        "executable": True,
    }
    return HarnessPlan(
        source_path,
        source_bytes,
        harness_path,
        harness_bytes,
        harness_verifier_identity(manifest_key, source_bytes),
        metadata,
    )


def build_harness_plans(
    files: dict[str, dict],
    *,
    examples_dir: Path,
    output_dir: Path,
    root: Path,
) -> dict[str, HarnessPlan]:
    """Build and validate all library harness plans before any writes occur."""
    plans: dict[str, HarnessPlan] = {}
    targets: dict[Path, str] = {}

    for manifest_key, entry in sorted(files.items()):
        if entry.get("runability") != "library":
            continue

        plan = build_harness_plan(
            manifest_key,
            examples_dir=examples_dir,
            output_dir=output_dir,
            root=root,
        )
        if plan.harness_path is not None:
            target = plan.harness_path.resolve(strict=False)
            if previous := targets.get(target):
                raise HarnessContractError(
                    f"duplicate harness mapping: {previous} and {manifest_key} -> {target}"
                )
            targets[target] = manifest_key
        plans[manifest_key] = plan

    return plans


def attach_harness_contracts(
    files: dict[str, dict],
    *,
    examples_dir: Path,
    output_dir: Path,
    root: Path,
) -> dict[str, HarnessPlan]:
    """Attach deterministic harness metadata to every library entry."""
    plans = build_harness_plans(
        files,
        examples_dir=examples_dir,
        output_dir=output_dir,
        root=root,
    )
    for manifest_key, plan in plans.items():
        files[manifest_key]["harness"] = dict(plan.metadata)
        files[manifest_key].pop("harness_skip", None)
    return plans


def atomic_write_bytes(path: Path, content: bytes) -> None:
    """Atomically replace *path* with *content* using a sibling temp file."""
    path.parent.mkdir(parents=True, exist_ok=True)
    temp_path: Path | None = None
    try:
        file_descriptor, temp_name = tempfile.mkstemp(
            dir=path.parent,
            prefix=f".{path.name}.",
            suffix=".tmp",
        )
        temp_path = Path(temp_name)
        with os.fdopen(file_descriptor, "wb") as temp_file:
            temp_file.write(content)
            temp_file.flush()
            os.fsync(temp_file.fileno())
        os.replace(temp_path, path)
        temp_path = None
    finally:
        if temp_path is not None:
            temp_path.unlink(missing_ok=True)


def _required_digest(value: object, *, field: str, manifest_key: str) -> str:
    if not isinstance(value, str) or SHA256_RE.fullmatch(value) is None:
        raise HarnessContractError(
            f"{manifest_key}: harness.{field} must be a lowercase SHA-256 digest"
        )
    return value


def resolve_harness_contract(
    entry: dict,
    *,
    source_path: Path,
    root: Path,
    manifest_key: str,
) -> ResolvedHarness | None:
    """Validate and resolve one library fixture's harness contract."""
    contract = entry.get("harness")
    if not isinstance(contract, dict):
        raise HarnessContractError(f"{manifest_key}: missing structured harness contract")

    required_fields = {
        "path",
        "source_sha256",
        "harness_sha256",
        "generation_version",
        "executable",
    }
    missing = sorted(required_fields - contract.keys())
    extra = sorted(contract.keys() - (required_fields | {"skip_reason"}))
    if missing:
        raise HarnessContractError(
            f"{manifest_key}: harness contract missing fields: {', '.join(missing)}"
        )
    if extra:
        raise HarnessContractError(
            f"{manifest_key}: harness contract has unknown fields: {', '.join(extra)}"
        )

    generation_version = contract["generation_version"]
    if type(generation_version) is not int or generation_version != HARNESS_GENERATION_VERSION:
        raise HarnessContractError(
            f"{manifest_key}: unsupported harness generation version: {generation_version!r}"
        )
    executable = contract["executable"]
    if type(executable) is not bool:
        raise HarnessContractError(f"{manifest_key}: harness.executable must be a boolean")

    resolved_source = _resolved_contained_path(
        source_path,
        root,
        label=f"source {manifest_key}",
        must_exist=True,
    )
    source_bytes = resolved_source.read_bytes()
    source_digest = _required_digest(
        contract["source_sha256"],
        field="source_sha256",
        manifest_key=manifest_key,
    )
    if sha256_bytes(source_bytes) != source_digest:
        raise HarnessContractError(f"{manifest_key}: source digest is stale")

    if not executable:
        if contract["path"] is not None or contract["harness_sha256"] is not None:
            raise HarnessContractError(
                f"{manifest_key}: non-executable harness must have null path and digest"
            )
        skip_reason = contract.get("skip_reason")
        if skip_reason not in SKIP_REASONS:
            raise HarnessContractError(
                f"{manifest_key}: non-executable harness has invalid skip_reason"
            )
        return None

    if "skip_reason" in contract:
        raise HarnessContractError(
            f"{manifest_key}: executable harness must not define skip_reason"
        )
    harness_relpath = contract["path"]
    if not isinstance(harness_relpath, str):
        raise HarnessContractError(f"{manifest_key}: harness.path must be a string")
    normalized_path = _normalized_relative_path(harness_relpath, label="harness path")
    harness_path = root.joinpath(*normalized_path.parts)
    resolved_harness_path = _resolved_contained_path(
        harness_path,
        root,
        label=f"harness for {manifest_key}",
        must_exist=True,
    )
    harness_bytes = resolved_harness_path.read_bytes()
    harness_digest = _required_digest(
        contract["harness_sha256"],
        field="harness_sha256",
        manifest_key=manifest_key,
    )
    if sha256_bytes(harness_bytes) != harness_digest:
        raise HarnessContractError(f"{manifest_key}: harness digest is stale")

    return ResolvedHarness(
        path=resolved_harness_path,
        source_bytes=source_bytes,
        harness_bytes=harness_bytes,
        verifier_identity=harness_verifier_identity(manifest_key, source_bytes),
        metadata=dict(contract),
    )
