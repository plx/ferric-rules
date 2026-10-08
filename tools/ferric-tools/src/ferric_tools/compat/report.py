"""Report generator for CLIPS compatibility assessment."""

from __future__ import annotations

import csv
import os
from pathlib import Path
from typing import Annotated

import typer
from rich.console import Console

from ferric_tools._paths import examples_dir as default_examples_dir
from ferric_tools.compat.assessment import (
    ASSESSMENT_CATEGORIES,
    ORACLE_COVERAGE_FIELDS,
    load_legacy_manifest,
    oracle_evidence_view,
    project_manifest,
)
from ferric_tools.compat.corpus_render import corpus_lines
from ferric_tools.compat.corpus_summary import load_summary
from ferric_tools.compat.diagnostics import result_diagnostic_view

app = typer.Typer(help="Generate compatibility assessment reports.")
console = Console(stderr=True)


def compute_oracle_coverage(manifest: dict) -> dict:
    """Aggregate oracle evidence coverage for a compatibility manifest."""
    files = manifest.get("files", {})
    coverage: dict = {
        "total": len(files),
        "selected": 0,
        "declaration": 0,
        "valid": 0,
        "missing": 0,
        "invalid": 0,
        "reached": 0,
        "completed": 0,
        "effect": 0,
        "refused_equivalent": 0,
        "versions": {},
        "normalizations": {},
    }

    for info in files.values():
        evidence = oracle_evidence_view(info)
        if evidence["selected"]:
            coverage["selected"] += 1
            version = evidence["version"]
            version_label = "(unspecified)" if version is None else str(version)
            coverage["versions"][version_label] = coverage["versions"].get(version_label, 0) + 1

        coverage[evidence["status"]] += 1
        for field in ORACLE_COVERAGE_FIELDS:
            if evidence[field]:
                coverage[field] += 1
        if evidence["refused_equivalent"]:
            coverage["refused_equivalent"] += 1
        for normalization in evidence["normalizations"]:
            coverage["normalizations"][normalization] = (
                coverage["normalizations"].get(normalization, 0) + 1
            )

    return coverage


_ORACLE_METRICS = [
    ("selected", "selected"),
    ("declaration", "declaration"),
    ("valid", "valid"),
    ("missing", "missing"),
    ("invalid", "invalid"),
    ("reached", "reached"),
    ("completed", "completed"),
    ("effect", "effect"),
    ("refused_equivalent", "refused equivalent"),
]


def _format_counter(counter: dict[str, int]) -> str:
    if not counter:
        return "(none)"
    return ", ".join(f"{name}: {count:,}" for name, count in sorted(counter.items()))


def _provenance_view(manifest: dict) -> dict[str, str]:
    """Return report-safe candidate and reference identity fields."""
    candidate = manifest.get("candidate")
    reference = manifest.get("reference")
    candidate = candidate if isinstance(candidate, dict) else {}
    reference = reference if isinstance(reference, dict) else {}
    return {
        "candidate_commit": str(candidate.get("commit_sha", "")),
        "candidate_binary": str(candidate.get("binary_sha256", "")),
        "reference_platform": str(reference.get("platform", "")),
        "reference_binary": str(reference.get("binary_sha256", "")),
        "reference_library": str(reference.get("library_sha256", "")),
        "reference_image": str(reference.get("image_id", "")),
        "reference_base": str(reference.get("base_image", "")),
    }


def _print_provenance(manifest: dict) -> None:
    provenance = _provenance_view(manifest)
    print()
    print("Provenance:")
    print(f"  Candidate commit:       {provenance['candidate_commit'] or '(missing)'}")
    print(f"  Candidate binary SHA:   {provenance['candidate_binary'] or '(missing)'}")
    print(f"  Reference platform:     {provenance['reference_platform'] or '(missing)'}")
    print(f"  Reference binary SHA:   {provenance['reference_binary'] or '(missing)'}")
    print(f"  Reference library SHA:  {provenance['reference_library'] or '(missing)'}")
    print(f"  Reference image ID:     {provenance['reference_image'] or '(missing)'}")
    print(f"  Reference base image:   {provenance['reference_base'] or '(missing)'}")


def _termination_view(result: dict) -> dict:
    raw = result.get("termination")
    if not isinstance(raw, dict):
        return {
            "kind": "",
            "exit_code": result.get("exit_code"),
            "signal": None,
            "active_phase": None,
        }
    return {
        "kind": raw.get("kind", ""),
        "exit_code": raw.get("exit_code"),
        "signal": raw.get("signal"),
        "active_phase": raw.get("active_phase"),
    }


def _diagnostic_line(engine: str, result: dict) -> str | None:
    diagnostic = result_diagnostic_view(result)
    if diagnostic["phase"] == "none" and diagnostic["category"] == "none":
        return None
    process = _termination_view(result)
    line = (
        f"{engine}: diagnostic v{diagnostic['version']} "
        f"{diagnostic['phase']}/{diagnostic['category']} "
        f"continued={str(diagnostic['continued']).lower()}; "
        f"termination={process['kind'] or 'unknown'}"
    )
    if process["signal"] is not None:
        line += f" signal={process['signal']}"
    elif process["exit_code"] is not None:
        line += f" exit={process['exit_code']}"
    if process["active_phase"] is not None:
        line += f" active-phase={process['active_phase']}"
    return line


def _diagnostic_entries(manifest: dict) -> list[tuple[str, dict, list[str]]]:
    entries: list[tuple[str, dict, list[str]]] = []
    for path, info in sorted(manifest["files"].items()):
        details = [
            line
            for engine in ("ferric", "clips")
            if (line := _diagnostic_line(engine, info.get(engine) or {})) is not None
        ]
        if details:
            entries.append((path, info, details))
    return entries


def print_summary(manifest: dict | None, corpus_summary: dict | None = None) -> None:
    """Print granular evidence first, then the assessed legacy inventory."""
    print("CLIPS Compatibility Assessment Report")
    print("=" * 40)
    print("\n".join(corpus_lines(corpus_summary)))
    print("\n".join(_legacy_lines(manifest)))


def _legacy_lines(manifest: dict | None) -> list[str]:
    if manifest is None:
        return ["### Legacy assessment", "", "Legacy assessment not produced.", ""]
    projected = project_manifest(manifest)
    summary = projected["summary"]
    lines = [
        "### Legacy executed oracles and unassessed inventory",
        "",
        "Only valid, completed oracle executions are equivalent or divergent. "
        "Unexecuted scanner findings are inventory, not compatibility results.",
        "",
        "| Assessment | Count |",
        "|---|---:|",
    ]
    assessed = [category for category in ASSESSMENT_CATEGORIES if category != "unassessed"]
    lines += [f"| {category} | {summary[category]:,} |" for category in assessed]
    count = sum(summary[category] for category in assessed)
    lines += [f"| **oracle rows** | **{count:,}** |", ""]
    lines += [
        f"Unassessed inventory: **{summary['unassessed']:,}** canonical rows. "
        "These are excluded from oracle assessment totals.",
        "",
    ]
    for key in ("physical_paths", "unique_contents", "duplicate_aliases"):
        if key in summary:
            lines.append(f"- {key.replace('_', ' ')}: {summary[key]:,}")
    lines += ["", "### Candidate and reference provenance", ""]
    lines += [
        f"- {key.replace('_', ' ')}: `{value or '(missing)'}`"
        for key, value in _provenance_view(manifest).items()
    ]
    oracle = compute_oracle_coverage(manifest)
    lines += ["", "### Oracle evidence coverage", "", "| Metric | Count |", "|---|---:|"]
    lines += [f"| {label} | {oracle[key]:,} |" for key, label in _ORACLE_METRICS]
    lines += [
        "",
        f"Versions: {_format_counter(oracle['versions'])}",
        f"Normalizations: {_format_counter(oracle['normalizations'])}",
    ]
    for category in ASSESSMENT_CATEGORIES:
        entries = [
            (path, info)
            for path, info in projected["files"].items()
            if info["classification"] == category
        ]
        if not entries:
            continue
        lines += ["", f"### {category.title()} ({len(entries)})", ""]
        for path, info in sorted(entries):
            lines.append(f"- `{path}` ({info['reason']})")
    diagnostics = _diagnostic_entries(projected)
    if diagnostics:
        lines += ["", f"### Diagnostic evidence ({len(diagnostics)})", ""]
        for path, info, details in diagnostics:
            lines.append(f"- `{path}` ({info['classification']}: {info['reason']})")
            lines += [f"  - {detail}" for detail in details]
    return lines


_FIELDNAMES = [
    "path",
    "source",
    "classification",
    "reason",
    "raw_classification",
    "raw_reason",
    "runability",
    "features",
    "unsupported_features",
    "ferric_exit",
    "ferric_duration_ms",
    "ferric_timed_out",
    "ferric_termination",
    "ferric_signal",
    "ferric_active_phase",
    "ferric_diagnostic_version",
    "ferric_diagnostic_phase",
    "ferric_diagnostic_category",
    "ferric_diagnostic_continued",
    "clips_exit",
    "clips_duration_ms",
    "clips_timed_out",
    "clips_termination",
    "clips_signal",
    "clips_active_phase",
    "clips_diagnostic_version",
    "clips_diagnostic_phase",
    "clips_diagnostic_category",
    "clips_diagnostic_continued",
    "oracle_selected",
    "oracle_declaration",
    "oracle_status",
    "oracle_version",
    "oracle_reached",
    "oracle_completed",
    "oracle_effect",
    "oracle_normalizations",
    "oracle_violations",
    "oracle_refused_equivalent",
    "notes",
]


def _write_delimited(manifest: dict, out_path: str, delimiter: str) -> None:
    """Write CSV or TSV export."""
    manifest = project_manifest(manifest)
    with open(out_path, "w", newline="", encoding="utf-8") as f:
        writer = csv.DictWriter(f, fieldnames=_FIELDNAMES, delimiter=delimiter)
        writer.writeheader()
        for path, info in sorted(manifest["files"].items()):
            ferric = info.get("ferric") or {}
            clips = info.get("clips") or {}
            ferric_diagnostic = result_diagnostic_view(ferric)
            clips_diagnostic = result_diagnostic_view(clips)
            ferric_termination = _termination_view(ferric)
            clips_termination = _termination_view(clips)
            oracle = oracle_evidence_view(info)
            writer.writerow(
                {
                    "path": path,
                    "source": info.get("source", ""),
                    "classification": info.get("classification", ""),
                    "reason": info.get("reason", ""),
                    "raw_classification": info.get("raw_classification", ""),
                    "raw_reason": info.get("raw_reason", ""),
                    "runability": info.get("runability", ""),
                    "features": ";".join(info.get("features", [])),
                    "unsupported_features": ";".join(info.get("unsupported_features", [])),
                    "ferric_exit": ferric.get("exit_code", ""),
                    "ferric_duration_ms": ferric.get("duration_ms", ""),
                    "ferric_timed_out": ferric.get("timed_out", ""),
                    "ferric_termination": ferric_termination["kind"],
                    "ferric_signal": ferric_termination["signal"],
                    "ferric_active_phase": ferric_termination["active_phase"],
                    "ferric_diagnostic_version": ferric_diagnostic["version"],
                    "ferric_diagnostic_phase": ferric_diagnostic["phase"],
                    "ferric_diagnostic_category": ferric_diagnostic["category"],
                    "ferric_diagnostic_continued": ferric_diagnostic["continued"],
                    "clips_exit": clips.get("exit_code", ""),
                    "clips_duration_ms": clips.get("duration_ms", ""),
                    "clips_timed_out": clips.get("timed_out", ""),
                    "clips_termination": clips_termination["kind"],
                    "clips_signal": clips_termination["signal"],
                    "clips_active_phase": clips_termination["active_phase"],
                    "clips_diagnostic_version": clips_diagnostic["version"],
                    "clips_diagnostic_phase": clips_diagnostic["phase"],
                    "clips_diagnostic_category": clips_diagnostic["category"],
                    "clips_diagnostic_continued": clips_diagnostic["continued"],
                    "oracle_selected": oracle["selected"],
                    "oracle_declaration": oracle["declaration"],
                    "oracle_status": oracle["status"],
                    "oracle_version": ("" if oracle["version"] is None else str(oracle["version"])),
                    "oracle_reached": oracle["reached"],
                    "oracle_completed": oracle["completed"],
                    "oracle_effect": oracle["effect"],
                    "oracle_normalizations": ";".join(oracle["normalizations"]),
                    "oracle_violations": ";".join(oracle["violations"]),
                    "oracle_refused_equivalent": oracle["refused_equivalent"],
                    "notes": info.get("notes", ""),
                }
            )
    print(f"{'TSV' if delimiter == '\t' else 'CSV'} written to {out_path}")


def write_report(
    manifest: dict | None,
    report_path: str,
    repo: str | None = None,
    commit_sha: str | None = None,
    corpus_summary: dict | None = None,
) -> None:
    """Write self-contained corpus and legacy evidence without inferring missing runs."""
    lines = ["## CLIPS Compatibility Report", ""]
    lines += corpus_lines(corpus_summary)
    if repo and commit_sha:
        lines += [
            f"Commit: [`{commit_sha[:10]}`](https://github.com/{repo}/commit/{commit_sha})",
            "",
        ]
    lines += _legacy_lines(manifest)
    Path(report_path).write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"Report written to {report_path}")


def create_symlinks(manifest: dict, symlinks_dir: str, examples_path: Path) -> None:
    """Create a symlink directory view."""
    manifest = project_manifest(manifest)
    import shutil

    symlinks_path = Path(symlinks_dir)
    if symlinks_path.exists():
        shutil.rmtree(symlinks_path)

    created = 0
    for rel_path, info in sorted(manifest["files"].items()):
        cls = info["classification"]
        dest_dir = symlinks_path / cls if cls in ASSESSMENT_CATEGORIES else symlinks_path / "other"

        dest = dest_dir / rel_path
        dest.parent.mkdir(parents=True, exist_ok=True)

        actual = examples_path / rel_path
        if actual.exists():
            rel_target = os.path.relpath(actual, dest.parent)
            dest.symlink_to(rel_target)
            created += 1

    print(f"Symlink view created at {symlinks_dir} ({created:,} links)")


@app.command()
def main(
    manifest_opt: Annotated[
        Path | None, typer.Option("--manifest", help="Path to manifest file")
    ] = None,
    corpus_summary: Annotated[
        Path | None, typer.Option(help="Granular corpus summary JSON")
    ] = None,
    csv_path: Annotated[str | None, typer.Option("--csv", help="Export as CSV")] = None,
    tsv_path: Annotated[str | None, typer.Option("--tsv", help="Export as TSV")] = None,
    report: Annotated[str | None, typer.Option(help="Write self-contained Markdown report")] = None,
    repo: Annotated[str | None, typer.Option(help="GitHub repository for commit links")] = None,
    commit_sha: Annotated[str | None, typer.Option(help="Commit SHA for report links")] = None,
    symlinks: Annotated[str | None, typer.Option(help="Create symlink directory view")] = None,
) -> None:
    """Generate compatibility assessment reports."""
    ed = default_examples_dir()
    manifest_path = Path(manifest_opt) if manifest_opt else ed / "compat-manifest.json"

    try:
        corpus = (
            load_summary(corpus_summary) if corpus_summary and corpus_summary.exists() else None
        )
        mdata = load_legacy_manifest(manifest_path) if manifest_path.exists() else None
    except (OSError, ValueError) as error:
        console.print(f"[red]error:[/] {error}")
        raise typer.Exit(1) from error
    if mdata is None and corpus is None:
        console.print(f"[red]error:[/] manifest not found: {manifest_path}")
        raise typer.Exit(1)
    print_summary(mdata, corpus)

    if csv_path and mdata is not None:
        print()
        _write_delimited(mdata, csv_path, ",")

    if tsv_path and mdata is not None:
        print()
        _write_delimited(mdata, tsv_path, "\t")

    if report:
        print()
        write_report(mdata, report, repo=repo, commit_sha=commit_sha, corpus_summary=corpus)

    if symlinks and mdata is not None:
        print()
        create_symlinks(mdata, symlinks, ed)


if __name__ == "__main__":
    app()
