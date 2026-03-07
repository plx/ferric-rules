"""Generate harness .clp files for library-only files in the compatibility manifest."""

from __future__ import annotations

import re
from pathlib import Path
from typing import Annotated

import typer
from rich.console import Console

from ferric_tools._manifest import load_manifest, save_manifest
from ferric_tools._paths import repo_root

app = typer.Typer(help="Generate harness .clp files for library-only files.")
console = Console(stderr=True)

EXTERNAL_DEP_KEYWORDS = ["ros-", "ament-", "blackboard-", "pb-", "navgraph-", "protobuf-"]


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

    for m in re.finditer(r"\(\s*deffacts\s+([\w:.-]+)", cleaned):
        constructs["deffacts"].append(m.group(1))

    for m in re.finditer(r"\(\s*deftemplate\s+([\w:.-]+)", cleaned):
        constructs["deftemplate"].append(m.group(1))

    for m in re.finditer(r"\?\*[\w-]+\*", cleaned):
        var = m.group(0)
        if var not in constructs["defglobal"]:
            constructs["defglobal"].append(var)

    for m in re.finditer(r"\(\s*deffunction\s+([\w:.-]+)\s*\(([^)]*)\)", cleaned):
        name = m.group(1)
        params_str = m.group(2).strip()
        param_count = len(re.findall(r"[\$]?\?\w+", params_str)) if params_str else 0
        constructs["deffunction"].append((name, param_count))

    for m in re.finditer(r"\(\s*defgeneric\s+([\w:.-]+)", cleaned):
        constructs["defgeneric"].append(m.group(1))

    for m in re.finditer(r"\(\s*defmethod\s+([\w:.-]+)", cleaned):
        name = m.group(1)
        if name not in constructs["defmethod"]:
            constructs["defmethod"].append(name)

    for m in re.finditer(r"\(\s*defmodule\s+([\w:.-]+)", cleaned):
        constructs["defmodule"].append(m.group(1))

    return constructs


def has_any_constructs(constructs: dict) -> bool:
    """Check if any constructs were detected."""
    return any(len(v) > 0 for v in constructs.values())


def generate_harness(source_relpath: str, constructs: dict) -> str:
    """Generate a harness .clp file content for the given source file."""
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
                names = [f"{name}/{count}" for name, count in items]
                summary_parts.append(f"{kind}: {', '.join(names)}")
            else:
                summary_parts.append(f"{kind}: {', '.join(items)}")

    summary = "; ".join(summary_parts) if summary_parts else "no named constructs"

    lines = [
        f"; Harness for {source_relpath}",
        f"; Detected constructs: {summary}",
        ";",
        "; Strategy: verify file loads and reset succeeds.",
        "; The source file is loaded via (load ...) before this harness.",
        "",
        "(defrule harness-verify",
        "   (initial-fact)",
        "   =>",
        '   (printout t "HARNESS: loaded" crlf))',
        "",
    ]
    return "\n".join(lines)


def compute_harness_path(output_dir: Path, manifest_key: str) -> Path:
    """Compute the harness output path from the manifest key."""
    p = Path(manifest_key)
    harness_name = f"{p.stem}-harness.clp"
    return output_dir / p.parent / harness_name


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
) -> None:
    """Generate harness .clp files for library-only files."""
    root = repo_root()
    manifest_path = (
        Path(manifest_opt) if manifest_opt else root / "tests" / "examples" / "compat-manifest.json"
    )
    out_dir = Path(output_dir) if output_dir else root / "tests" / "harnesses"
    examples_path = manifest_path.parent

    if not manifest_path.exists():
        console.print(f"[red]error:[/] manifest not found: {manifest_path}")
        raise typer.Exit(1)

    manifest = load_manifest(manifest_path)
    files = manifest.get("files", {})

    library_only = {k: v for k, v in files.items() if v.get("reason") == "library-only"}
    print(f"Found {len(library_only)} library-only files in manifest.")

    stats = {
        "generated": 0,
        "skipped_external": 0,
        "skipped_empty": 0,
        "skipped_missing": 0,
    }

    for manifest_key in sorted(library_only.keys()):
        entry = files[manifest_key]
        source_path = examples_path / manifest_key

        if not source_path.exists():
            if dry_run:
                print(f"  SKIP (missing): {manifest_key}")
            stats["skipped_missing"] += 1
            continue

        try:
            content = source_path.read_text(encoding="utf-8", errors="replace")
        except Exception as e:
            console.print(f"[red]ERROR[/] reading {manifest_key}: {e}")
            stats["skipped_missing"] += 1
            continue

        if has_external_deps(content):
            if dry_run:
                print(f"  SKIP (external-deps): {manifest_key}")
            entry["harness_skip"] = "external-deps"
            stats["skipped_external"] += 1
            continue

        constructs = detect_constructs(content)

        if not has_any_constructs(constructs):
            if dry_run:
                print(f"  SKIP (empty): {manifest_key}")
            entry["harness_skip"] = "empty"
            stats["skipped_empty"] += 1
            continue

        harness_content = generate_harness(manifest_key, constructs)
        harness_path = compute_harness_path(out_dir, manifest_key)

        try:
            harness_relpath = str(harness_path.relative_to(root))
        except ValueError:
            harness_relpath = str(harness_path)

        if dry_run:
            print(f"  GENERATE: {manifest_key}")
            print(f"    -> {harness_relpath}")
        else:
            harness_path.parent.mkdir(parents=True, exist_ok=True)
            harness_path.write_text(harness_content, encoding="utf-8")

        entry["harness"] = harness_relpath
        entry.pop("harness_skip", None)
        stats["generated"] += 1

    if not dry_run:
        save_manifest(manifest_path, manifest)
        print(f"\nManifest updated: {manifest_path}")

    print("\nResults:")
    print(f"  Generated:        {stats['generated']}")
    print(f"  Skipped (ext):    {stats['skipped_external']}")
    print(f"  Skipped (empty):  {stats['skipped_empty']}")
    print(f"  Skipped (missing):{stats['skipped_missing']}")
    print(f"  Total:            {sum(stats.values())}")


if __name__ == "__main__":
    app()
