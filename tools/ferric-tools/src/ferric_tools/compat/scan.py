"""Static scanner for CLIPS compatibility assessment.

Scans all .clp files under tests/examples/ and produces a JSON manifest
classifying each file by detected features and ferric compatibility.
"""

from __future__ import annotations

import hashlib
import json
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
from ferric_tools._harness import attach_harness_contracts, sha256_bytes
from ferric_tools._manifest import save_manifest, utc_now_iso
from ferric_tools._paths import examples_dir as default_examples_dir
from ferric_tools._paths import repo_root
from ferric_tools.compat.oracle import (
    DECLARATION_VERSION,
    EvidenceStatus,
    validate_declaration,
)

app = typer.Typer(help="Scan CLIPS examples for compatibility assessment.")
console = Console(stderr=True)
MANIFEST_VERSION = 3
ORACLE_REGISTRY_VERSION = 1


class OracleRegistryError(ValueError):
    """Raised when a checked-in compatibility oracle registry is invalid."""


def _reject_duplicate_json_fields(pairs: list[tuple[str, object]]) -> dict:
    result: dict = {}
    for key, value in pairs:
        if key in result:
            raise OracleRegistryError(f"duplicate JSON field in oracle registry: {key!r}")
        result[key] = value
    return result


def _load_oracle_registry(path: Path) -> dict[str, dict]:
    """Load the strict, tracked per-fixture oracle registry."""
    if not path.exists():
        return {}
    try:
        raw = json.loads(
            path.read_text(encoding="utf-8"),
            object_pairs_hook=_reject_duplicate_json_fields,
        )
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise OracleRegistryError(f"cannot read oracle registry {path}: {error}") from error
    if type(raw) is not dict or set(raw) != {"version", "fixtures"}:
        raise OracleRegistryError("oracle registry must contain exactly 'version' and 'fixtures'")
    if raw["version"] != ORACLE_REGISTRY_VERSION:
        raise OracleRegistryError(f"unsupported oracle registry version: {raw['version']!r}")
    fixtures = raw["fixtures"]
    if type(fixtures) is not dict:
        raise OracleRegistryError("oracle registry fixtures must be an object")

    declarations: dict[str, dict] = {}
    fixture_ids: dict[str, str] = {}
    for raw_path, declaration in fixtures.items():
        if type(raw_path) is not str or type(declaration) is not dict:
            raise OracleRegistryError("oracle registry paths must map to declaration objects")
        normalized = Path(raw_path)
        if (
            normalized.is_absolute()
            or normalized.as_posix() != raw_path
            or any(part in {"", ".", ".."} for part in normalized.parts)
        ):
            raise OracleRegistryError(
                f"oracle registry path must be normalized and relative: {raw_path!r}"
            )
        fixture_id = declaration.get("id")
        if type(fixture_id) is not str:
            raise OracleRegistryError(f"{raw_path}: oracle id must be a string")
        if previous := fixture_ids.get(fixture_id):
            raise OracleRegistryError(
                f"duplicate oracle id {fixture_id!r}: {previous} and {raw_path}"
            )
        fixture_ids[fixture_id] = raw_path
        declarations[raw_path] = declaration
    return declarations


def _attach_oracle_declarations(
    files: dict[str, dict],
    *,
    examples_path: Path,
    root: Path,
) -> None:
    """Validate tracked declarations and attach them to generated entries."""
    declarations = _load_oracle_registry(examples_path / "compat-oracles.json")
    unknown_paths = sorted(set(declarations) - set(files))
    if unknown_paths:
        raise OracleRegistryError(
            "oracle registry references files absent from the scan: " + ", ".join(unknown_paths)
        )

    for rel_path, entry in files.items():
        source_path = examples_path / rel_path
        try:
            source_bytes = source_path.read_bytes()
        except OSError as error:
            entry["source_sha256"] = None
            if rel_path in declarations:
                raise OracleRegistryError(
                    f"{rel_path}: declared oracle source cannot be read: {error}"
                ) from error
            continue

        source_sha256 = sha256_bytes(source_bytes)
        entry["source_sha256"] = source_sha256
        declaration = declarations.get(rel_path)
        if declaration is None:
            continue

        composed_sha256 = source_sha256
        harness = entry.get("harness")
        if harness is not None:
            if harness.get("executable") is not True:
                raise OracleRegistryError(
                    f"{rel_path}: declared library oracle has no executable harness"
                )
            harness_path = harness.get("path")
            if type(harness_path) is not str:
                raise OracleRegistryError(
                    f"{rel_path}: declared library oracle has no harness path"
                )
            try:
                harness_bytes = (root / harness_path).read_bytes()
            except OSError as error:
                raise OracleRegistryError(
                    f"{rel_path}: declared harness cannot be read: {error}"
                ) from error
            composed_sha256 = sha256_bytes(source_bytes + b"\n" + harness_bytes)

        evidence = validate_declaration(
            declaration,
            expected_source_sha256=source_sha256,
            expected_composed_sha256=composed_sha256,
        )
        if evidence.status is not EvidenceStatus.VALID:
            detail = "; ".join(f"{issue.field}: {issue.message}" for issue in evidence.issues)
            raise OracleRegistryError(f"{rel_path}: invalid oracle declaration: {detail}")

        entry["oracle"] = declaration
        entry["oracle_evidence"] = {
            "status": "missing",
            "version": DECLARATION_VERSION,
            "declaration": True,
            "reached": False,
            "completed": False,
            "effect": False,
            "normalizations": list(declaration["normalizers"]),
            "violations": [],
        }


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


def scan_examples(
    examples_path: Path,
    *,
    root: Path | None = None,
    harness_dir: Path | None = None,
) -> dict:
    """Scan all .clp and .bat files under examples_path."""
    files: dict[str, dict] = {}
    all_files = sorted(examples_path.rglob("*.clp")) + sorted(examples_path.rglob("*.bat"))

    for filepath in all_files:
        rel = filepath.relative_to(examples_path)
        rel_str = rel.as_posix()
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

    if root is None:
        if examples_path.name == "examples" and examples_path.parent.name == "tests":
            root = examples_path.parent.parent
        else:
            root = repo_root()
    output_dir = harness_dir or root / "tests" / "harnesses"
    attach_harness_contracts(
        files,
        examples_dir=examples_path,
        output_dir=output_dir,
        root=root,
    )
    _attach_oracle_declarations(files, examples_path=examples_path, root=root)
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
    root = repo_root()
    try:
        files = scan_examples(
            examples_path,
            root=root,
            harness_dir=root / "tests" / "harnesses",
        )
    except OracleRegistryError as error:
        console.print(f"[red]error:[/] {error}")
        raise typer.Exit(1) from error
    dup_count = dedup_batch_files(files, examples_path)
    summary = build_summary(files)

    manifest = {
        "version": MANIFEST_VERSION,
        "oracle_protocol_version": DECLARATION_VERSION,
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
