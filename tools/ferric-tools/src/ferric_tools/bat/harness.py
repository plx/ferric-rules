"""Generate harness .clp files for library-only files in the compatibility manifest."""

from __future__ import annotations

import copy
from pathlib import Path
from typing import Annotated

import typer
from rich.console import Console

from ferric_tools._harness import (
    HarnessContractError,
    HarnessPlan,
    atomic_write_bytes,
    build_harness_plans,
    compute_harness_path,
    detect_constructs,
    generate_harness,
    has_any_constructs,
    has_external_deps,
    resolve_harness_contract,
)
from ferric_tools._manifest import load_manifest, save_manifest
from ferric_tools._paths import repo_root

__all__ = [
    "compute_harness_path",
    "detect_constructs",
    "generate_harness",
    "has_any_constructs",
    "has_external_deps",
]

app = typer.Typer(help="Generate harness .clp files for library-only files.")
console = Console(stderr=True)


def _verify_plans(
    files: dict[str, dict],
    plans: dict[str, HarnessPlan],
    *,
    root: Path,
) -> int:
    """Verify manifest contracts and materialized bytes against deterministic plans."""
    verified = 0
    for manifest_key, plan in plans.items():
        entry = files[manifest_key]
        if entry.get("harness") != plan.metadata:
            raise HarnessContractError(
                f"{manifest_key}: manifest harness contract does not match deterministic plan"
            )

        resolved = resolve_harness_contract(
            entry,
            source_path=plan.source_path,
            root=root,
            manifest_key=manifest_key,
        )
        if plan.metadata["executable"] is not True:
            if resolved is not None:
                raise HarnessContractError(
                    f"{manifest_key}: non-executable harness resolved to materialized bytes"
                )
            continue

        if resolved is None or plan.harness_bytes is None:
            raise HarnessContractError(f"{manifest_key}: executable harness did not resolve")
        if resolved.source_bytes != plan.source_bytes:
            raise HarnessContractError(f"{manifest_key}: resolved source bytes differ from plan")
        if resolved.harness_bytes != plan.harness_bytes:
            raise HarnessContractError(f"{manifest_key}: harness output bytes differ from plan")
        if resolved.metadata != plan.metadata:
            raise HarnessContractError(
                f"{manifest_key}: resolved harness contract differs from plan"
            )
        verified += 1
    return verified


@app.command()
def main(
    manifest_opt: Annotated[
        str | None,
        typer.Option("--manifest", help="Path to the compatibility manifest JSON file"),
    ] = None,
    output_dir: Annotated[
        str | None,
        typer.Option(help="Output directory for generated harness files"),
    ] = None,
    dry_run: Annotated[
        bool,
        typer.Option(help="Print what would be done without writing files"),
    ] = False,
    check: Annotated[
        bool,
        typer.Option(help="Verify planned outputs and manifest contracts without writing"),
    ] = False,
) -> None:
    """Generate harness .clp files for library-only files."""
    root = repo_root().resolve()
    manifest_path = (
        Path(manifest_opt) if manifest_opt else root / "tests" / "examples" / "compat-manifest.json"
    )
    out_dir = Path(output_dir) if output_dir else root / "tests" / "harnesses"
    examples_path = manifest_path.parent

    if not manifest_path.exists():
        console.print(f"[red]error:[/] manifest not found: {manifest_path}")
        raise typer.Exit(1)
    if dry_run and check:
        console.print("[red]error:[/] --dry-run and --check are mutually exclusive")
        raise typer.Exit(1)

    manifest = copy.deepcopy(load_manifest(manifest_path))
    files = manifest.get("files", {})
    try:
        plans = build_harness_plans(
            files,
            examples_dir=examples_path,
            output_dir=out_dir,
            root=root,
        )
    except HarnessContractError as error:
        console.print(f"[red]error:[/] {error}")
        raise typer.Exit(1) from error

    print(f"Found {len(plans)} library-only files in manifest.")

    if check:
        try:
            verified = _verify_plans(files, plans, root=root)
        except HarnessContractError as error:
            console.print(f"[red]error:[/] {error}")
            raise typer.Exit(1) from error
        print(f"Verified {verified} executable harnesses.")
        return

    stats = {
        "generated": 0,
        "skipped_external": 0,
        "skipped_empty": 0,
    }

    for manifest_key, plan in plans.items():
        entry = files[manifest_key]
        metadata = plan.metadata
        if not metadata["executable"]:
            skip_reason = metadata["skip_reason"]
            if dry_run:
                print(f"  SKIP ({skip_reason}): {manifest_key}")
            stats[f"skipped_{'external' if skip_reason == 'external-deps' else 'empty'}"] += 1
            entry["harness"] = dict(metadata)
            entry.pop("harness_skip", None)
            continue

        if dry_run:
            print(f"  GENERATE: {manifest_key}")
            print(f"    -> {metadata['path']}")
        else:
            assert plan.harness_path is not None
            assert plan.harness_bytes is not None
            if (
                not plan.harness_path.exists()
                or plan.harness_path.read_bytes() != plan.harness_bytes
            ):
                atomic_write_bytes(plan.harness_path, plan.harness_bytes)

        entry["harness"] = dict(metadata)
        entry.pop("harness_skip", None)
        stats["generated"] += 1

    if not dry_run:
        try:
            verified = _verify_plans(files, plans, root=root)
        except HarnessContractError as error:
            console.print(f"[red]error:[/] {error}")
            raise typer.Exit(1) from error
        current_version = manifest.get("version")
        manifest["version"] = max(current_version, 2) if type(current_version) is int else 2
        save_manifest(manifest_path, manifest)
        print(f"\nManifest updated: {manifest_path}")
        print(f"Verified {verified} executable harnesses.")

    print("\nResults:")
    print(f"  Generated:        {stats['generated']}")
    print(f"  Skipped (ext):    {stats['skipped_external']}")
    print(f"  Skipped (empty):  {stats['skipped_empty']}")
    print(f"  Total:            {sum(stats.values())}")


if __name__ == "__main__":
    app()
