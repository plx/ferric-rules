"""Compare two compatibility manifests and produce a Markdown summary."""

from __future__ import annotations

import csv
from typing import Annotated

import typer
from rich.console import Console

from ferric_tools._manifest import load_manifest
from ferric_tools.compat.diagnostics import result_diagnostic_view
from ferric_tools.compat.report import compute_oracle_coverage, oracle_evidence_view

app = typer.Typer(help="Compare two compat manifests.")
console = Console(stderr=True)

DISPLAY_ORDER = ["equivalent", "divergent", "incompatible", "pending"]

# Ordered from best to worst for determining regressions vs improvements.
RANK = {"equivalent": 0, "divergent": 1, "pending": 2, "incompatible": 3}
ORACLE_STATUS_RANK = {"invalid": 0, "missing": 1, "valid": 2}
ORACLE_BOOLEAN_COVERAGE = ("selected", "declaration", "reached", "completed", "effect")
STRUCTURED_ORACLE_MANIFEST_VERSION = 3
ABSENT_CLASSIFICATION = "absent"
ABSENT_REASON = "not present"
LEGACY_RUNNER_CLASSIFICATIONS = {
    "timeout-both": frozenset({"incompatible"}),
    "timeout-ferric": frozenset({"divergent", "incompatible"}),
    "ferric-only-clean": frozenset({"pending"}),
    "timeout-clips": frozenset({"divergent"}),
    "clips-load-error": frozenset({"incompatible"}),
    "both-error": frozenset({"incompatible"}),
    "ferric-error": frozenset({"divergent", "incompatible"}),
    "clips-error": frozenset({"divergent"}),
    "empty-match": frozenset({"equivalent"}),
    "exact-match": frozenset({"equivalent"}),
    "float-normalized-match": frozenset({"equivalent"}),
    "output-mismatch": frozenset({"divergent"}),
}


def _termination_snapshot(result: object) -> tuple[object, object, object, object]:
    if not isinstance(result, dict):
        return ("unknown", None, None, None)
    raw = result.get("termination")
    if not isinstance(raw, dict):
        return ("unknown", result.get("exit_code"), None, None)
    return (
        raw.get("kind"),
        raw.get("exit_code"),
        raw.get("signal"),
        raw.get("active_phase"),
    )


def _engine_diagnostic_snapshot(result: object) -> tuple[object, ...]:
    view = result_diagnostic_view(result)
    return (
        view["version"],
        view["phase"],
        view["category"],
        view["continued"],
        *_termination_snapshot(result),
    )


def _diagnostic_snapshot(info: object) -> tuple[tuple[object, ...], ...]:
    if not isinstance(info, dict):
        return tuple(_engine_diagnostic_snapshot(None) for _engine in ("ferric", "clips"))
    return tuple(_engine_diagnostic_snapshot(info.get(engine)) for engine in ("ferric", "clips"))


def _diagnostic_label(info: dict) -> str:
    labels: list[str] = []
    for engine in ("ferric", "clips"):
        result = info.get(engine)
        view = result_diagnostic_view(result)
        termination_kind, exit_code, signal, active_phase = _termination_snapshot(result)
        termination_label = str(termination_kind)
        if signal is not None:
            termination_label += f"({signal})"
        elif exit_code is not None:
            termination_label += f"({exit_code})"
        if active_phase is not None:
            termination_label += f"@{active_phase}"
        labels.append(
            f"{engine}={view['phase']}/{view['category']}/"
            f"continued:{str(view['continued']).lower()};termination:{termination_label}"
        )
    return ", ".join(labels)


def _reason_with_diagnostics(reason: str, info: dict) -> str:
    detail = f"diagnostics: {_diagnostic_label(info)}"
    return f"{reason}; {detail}" if reason else detail


def fmt_delta(n: int) -> str:
    if n > 0:
        return f"+{n}"
    if n < 0:
        return str(n)
    return "0"


def _oracle_loss_details(
    base_info: dict | None,
    head_info: dict | None,
    *,
    require_verified_head: bool,
) -> list[str]:
    """Describe oracle-evidence losses from one file entry to another."""
    if base_info is None:
        assert head_info is not None
        head = oracle_evidence_view(head_info)
        if head_info.get("classification") == "equivalent" and head["status"] != "valid":
            return ["unverified equivalent claim"]
        return []

    if head_info is None:
        base = oracle_evidence_view(base_info)
        if base["selected"] and base["status"] == "valid":
            return ["valid oracle-backed fixture removed"]
        return []

    base = oracle_evidence_view(base_info)
    head = oracle_evidence_view(head_info)
    losses: list[str] = []

    for field in ORACLE_BOOLEAN_COVERAGE:
        if base[field] and not head[field]:
            losses.append(f"{field} true\u2192false")

    if ORACLE_STATUS_RANK[head["status"]] < ORACLE_STATUS_RANK[base["status"]]:
        losses.append(f"status {base['status']}\u2192{head['status']}")

    if base["version"] is not None and head["version"] is None:
        losses.append(f"version {base['version']}\u2192unspecified")

    added_violations = sorted(set(head["violations"]) - set(base["violations"]))
    if added_violations:
        losses.append(f"violations added: {', '.join(added_violations)}")

    unverified_equivalent = (
        head_info.get("classification") == "equivalent"
        and head["status"] != "valid"
        and (
            require_verified_head
            or base_info.get("classification") != "equivalent"
            or base["status"] == "valid"
        )
    )
    if unverified_equivalent:
        losses.append("unverified equivalent claim")

    return losses


def _reason_with_oracle_loss(reason: str, losses: list[str]) -> str:
    detail = f"oracle regression: {', '.join(losses)}"
    return f"{reason}; {detail}" if reason else detail


def _is_v3_oracle_reset(
    base_version: object,
    head_version: object,
    base_info: dict,
    head_info: dict,
) -> bool:
    """Return whether a legacy result was reset for a missing v3 oracle."""
    if (
        type(base_version) is not int
        or type(head_version) is not int
        or base_version >= STRUCTURED_ORACLE_MANIFEST_VERSION
        or head_version < STRUCTURED_ORACLE_MANIFEST_VERSION
    ):
        return False

    if base_info.get("oracle") is not None or base_info.get("oracle_evidence") is not None:
        return False

    if (
        head_info.get("classification") != "pending"
        or head_info.get("reason") != "oracle-missing"
        or head_info.get("oracle") is not None
        or not isinstance(head_info.get("oracle_evidence"), dict)
    ):
        return False

    head_evidence = oracle_evidence_view(head_info)
    return (
        head_evidence["selected"]
        and head_evidence["status"] == "missing"
        and not any(
            head_evidence[field] for field in ("declaration", "reached", "completed", "effect")
        )
    )


def _is_legacy_runner_migration(
    base_version: object,
    head_version: object,
    base_info: dict,
    head_info: dict,
) -> bool:
    """Recognize approved resets of classifications produced by the legacy runner."""
    if not _is_v3_oracle_reset(base_version, head_version, base_info, head_info):
        return False

    allowed_classifications = LEGACY_RUNNER_CLASSIFICATIONS.get(base_info.get("reason"))
    return (
        allowed_classifications is not None
        and base_info.get("classification") in allowed_classifications
    )


def compute_diff(base: dict, head: dict) -> tuple[dict, dict, list, list, list]:
    """Compute counts and per-file changes between two manifests.

    Returns (base_counts, head_counts, regressions, real_improvements, reason_changes).
    """
    base_files = base.get("files", {})
    head_files = head.get("files", {})

    base_counts = {cls: 0 for cls in DISPLAY_ORDER}
    head_counts = {cls: 0 for cls in DISPLAY_ORDER}

    for info in base_files.values():
        cls = info["classification"]
        if cls in base_counts:
            base_counts[cls] += 1

    for info in head_files.values():
        cls = info["classification"]
        if cls in head_counts:
            head_counts[cls] += 1

    real_improvements: list[tuple] = []
    regressions: list[tuple] = []
    reason_changes: list[tuple] = []
    require_verified_head = (
        type(head.get("version")) is int and head["version"] >= STRUCTURED_ORACLE_MANIFEST_VERSION
    )

    all_keys = sorted(set(base_files) | set(head_files))
    for key in all_keys:
        b = base_files.get(key)
        h = head_files.get(key)

        if b is None or h is None:
            oracle_losses = _oracle_loss_details(
                b,
                h,
                require_verified_head=require_verified_head,
            )
            if oracle_losses:
                b_cls = ABSENT_CLASSIFICATION if b is None else b["classification"]
                b_reason = ABSENT_REASON if b is None else b.get("reason", "")
                h_cls = ABSENT_CLASSIFICATION if h is None else h["classification"]
                h_reason = ABSENT_REASON if h is None else h.get("reason", "")
                regressions.append(
                    (
                        key,
                        b_cls,
                        b_reason,
                        h_cls,
                        _reason_with_oracle_loss(h_reason, oracle_losses),
                    )
                )
            continue

        b_cls = b["classification"]
        h_cls = h["classification"]
        b_reason = b.get("reason", "")
        h_reason = h.get("reason", "")

        if _is_legacy_runner_migration(
            base.get("version"),
            head.get("version"),
            b,
            h,
        ):
            continue

        diagnostics_changed = _diagnostic_snapshot(b) != _diagnostic_snapshot(h)
        entry = (
            key,
            b_cls,
            _reason_with_diagnostics(b_reason, b) if diagnostics_changed else b_reason,
            h_cls,
            _reason_with_diagnostics(h_reason, h) if diagnostics_changed else h_reason,
        )
        oracle_losses = _oracle_loss_details(
            b,
            h,
            require_verified_head=require_verified_head,
        )

        if oracle_losses:
            regressions.append(
                (
                    key,
                    b_cls,
                    entry[2],
                    h_cls,
                    _reason_with_oracle_loss(entry[4], oracle_losses),
                )
            )
            continue

        if b_cls == h_cls:
            if b_reason != h_reason or diagnostics_changed:
                # A reason change within the same semantic classification is
                # neutral. This is especially important during manifest and
                # oracle schema migrations, where legacy reasons are replaced.
                reason_changes.append(
                    (
                        key,
                        b_cls,
                        entry[2],
                        h_cls,
                        entry[4],
                    )
                )
            continue

        if _is_v3_oracle_reset(
            base.get("version"),
            head.get("version"),
            b,
            h,
        ):
            regressions.append(entry)
            continue

        b_rank = RANK.get(b_cls, 99)
        h_rank = RANK.get(h_cls, 99)

        if h_rank < b_rank:
            real_improvements.append(entry)
        else:
            regressions.append(entry)

    return base_counts, head_counts, regressions, real_improvements, reason_changes


def format_markdown(
    base_counts: dict,
    head_counts: dict,
    regressions: list,
    real_improvements: list,
    reason_changes: list,
    *,
    repo: str | None = None,
    base_sha: str | None = None,
    head_sha: str | None = None,
    base_oracle: dict | None = None,
    head_oracle: dict | None = None,
) -> list[str]:
    """Build the full Markdown report as a list of lines."""
    lines: list[str] = []
    lines.append("## CLIPS Compatibility Report")
    lines.append("")
    lines.append("Compares ferric's compatibility with CLIPS across a corpus of")
    lines.append("example `.clp` files. Each file is classified as **equivalent**")
    lines.append("(a valid structured oracle matches CLIPS), **divergent** (semantic")
    lines.append("observations differ),")
    lines.append("**incompatible** (cannot run), or **pending** (not yet tested).")
    lines.append("")

    if repo and base_sha and head_sha:
        base_link = f"[`{base_sha[:10]}`](https://github.com/{repo}/commit/{base_sha})"
        head_link = f"[`{head_sha[:10]}`](https://github.com/{repo}/commit/{head_sha})"
        lines.append(f"Base: {base_link} | Head: {head_link}")
        lines.append("")

    base_total = sum(base_counts.values())
    head_total = sum(head_counts.values())

    lines.append("| Classification | Base | Head | Delta |")
    lines.append("|---|---:|---:|---|")
    for cls in DISPLAY_ORDER:
        b = base_counts[cls]
        h = head_counts[cls]
        d = h - b
        delta_str = f"**{fmt_delta(d)}**" if d != 0 else "\u2014"
        lines.append(f"| {cls} | {b} | {h} | {delta_str} |")
    d_total = head_total - base_total
    delta_total = f"**{fmt_delta(d_total)}**" if d_total != 0 else "\u2014"
    lines.append(f"| **total** | **{base_total}** | **{head_total}** | {delta_total} |")

    if base_oracle is not None and head_oracle is not None:
        lines.append("")
        lines.append("### Oracle evidence coverage")
        lines.append("")
        lines.append("| Metric | Base | Head | Delta |")
        lines.append("|---|---:|---:|---:|")
        for key in [
            "selected",
            "declaration",
            "valid",
            "missing",
            "invalid",
            "reached",
            "completed",
            "effect",
            "refused_equivalent",
        ]:
            b = base_oracle[key]
            h = head_oracle[key]
            lines.append(f"| {key.replace('_', ' ')} | {b} | {h} | {fmt_delta(h - b)} |")
        lines.append("")
        lines.append(
            f"Versions — base: {_format_counter(base_oracle['versions'])}; "
            f"head: {_format_counter(head_oracle['versions'])}"
        )
        lines.append("")
        lines.append(
            "Normalizations — "
            f"base: {_format_counter(base_oracle['normalizations'])}; "
            f"head: {_format_counter(head_oracle['normalizations'])}"
        )

    lines.append("")
    if regressions:
        lines.append(f"### Regressions ({len(regressions)})")
        lines.append("")
        lines.append("| File | Before | After |")
        lines.append("|---|---|---|")
        for path, b_cls, b_reason, h_cls, h_reason in regressions:
            lines.append(f"| `{path}` | {b_cls} ({b_reason}) | {h_cls} ({h_reason}) |")
    else:
        lines.append("### Regressions")
        lines.append("")
        lines.append("None")

    lines.append("")
    if real_improvements:
        lines.append(f"### Improvements ({len(real_improvements)})")
        lines.append("")
        lines.append("| File | Before | After |")
        lines.append("|---|---|---|")
        for path, b_cls, b_reason, h_cls, h_reason in real_improvements:
            lines.append(f"| `{path}` | {b_cls} ({b_reason}) | {h_cls} ({h_reason}) |")
    else:
        lines.append("### Improvements")
        lines.append("")
        lines.append("None")

    if reason_changes:
        lines.append("")
        lines.append(
            "<details><summary>Reason changes within same classification"
            f" ({len(reason_changes)})</summary>"
        )
        lines.append("")
        lines.append("| File | Classification | Before | After |")
        lines.append("|---|---|---|---|")
        for path, b_cls, b_reason, _h_cls, h_reason in reason_changes:
            lines.append(f"| `{path}` | {b_cls} | {b_reason} | {h_reason} |")
        lines.append("")
        lines.append("</details>")

    return lines


def _format_counter(counter: dict[str, int]) -> str:
    if not counter:
        return "(none)"
    return ", ".join(f"{name}: {count}" for name, count in sorted(counter.items()))


def _change_kind(
    base_info: dict | None,
    head_info: dict | None,
    *,
    base_version: object = None,
    head_version: object = None,
) -> tuple[str, list[str]]:
    """Return the TSV change label and any oracle regression details."""
    require_verified_head = (
        type(head_version) is int and head_version >= STRUCTURED_ORACLE_MANIFEST_VERSION
    )
    if base_info is None:
        oracle_losses = _oracle_loss_details(
            None,
            head_info,
            require_verified_head=require_verified_head,
        )
        if oracle_losses:
            return "regression", oracle_losses
        return "added", []
    if head_info is None:
        oracle_losses = _oracle_loss_details(
            base_info,
            None,
            require_verified_head=require_verified_head,
        )
        if oracle_losses:
            return "regression", oracle_losses
        return "removed", []

    if _is_legacy_runner_migration(
        base_version,
        head_version,
        base_info,
        head_info,
    ):
        return "schema-migration", []

    oracle_losses = _oracle_loss_details(
        base_info,
        head_info,
        require_verified_head=require_verified_head,
    )
    if oracle_losses:
        return "regression", oracle_losses

    base_classification = base_info["classification"]
    head_classification = head_info["classification"]
    if base_classification == head_classification:
        if base_info.get("reason", "") != head_info.get("reason", "") or _diagnostic_snapshot(
            base_info
        ) != _diagnostic_snapshot(head_info):
            return "reason-changed", []
        return "unchanged", []

    if _is_v3_oracle_reset(
        base_version,
        head_version,
        base_info,
        head_info,
    ):
        return "regression", []

    base_rank = RANK.get(base_classification, 99)
    head_rank = RANK.get(head_classification, 99)
    change = "improvement" if head_rank < base_rank else "regression"
    return change, []


def _tsv_diagnostic_fields(prefix: str, info: dict | None) -> dict[str, object]:
    fields: dict[str, object] = {}
    for engine in ("ferric", "clips"):
        result = info.get(engine) if info is not None else None
        view = result_diagnostic_view(result)
        termination_kind, exit_code, signal, active_phase = _termination_snapshot(result)
        stem = f"{prefix}_{engine}"
        fields.update(
            {
                f"{stem}_diagnostic_version": view["version"],
                f"{stem}_diagnostic_phase": view["phase"],
                f"{stem}_diagnostic_category": view["category"],
                f"{stem}_diagnostic_continued": view["continued"],
                f"{stem}_termination": termination_kind,
                f"{stem}_exit": exit_code,
                f"{stem}_signal": signal,
                f"{stem}_active_phase": active_phase,
            }
        )
    return fields


def write_tsv(base: dict, head: dict, tsv_path: str) -> None:
    """Write per-file raw data as TSV."""
    base_files = base.get("files", {})
    head_files = head.get("files", {})
    all_keys = sorted(set(base_files) | set(head_files))

    fieldnames = [
        "path",
        "source",
        "base_classification",
        "base_reason",
        "head_classification",
        "head_reason",
        "base_ferric_diagnostic_version",
        "base_ferric_diagnostic_phase",
        "base_ferric_diagnostic_category",
        "base_ferric_diagnostic_continued",
        "base_ferric_termination",
        "base_ferric_exit",
        "base_ferric_signal",
        "base_ferric_active_phase",
        "head_ferric_diagnostic_version",
        "head_ferric_diagnostic_phase",
        "head_ferric_diagnostic_category",
        "head_ferric_diagnostic_continued",
        "head_ferric_termination",
        "head_ferric_exit",
        "head_ferric_signal",
        "head_ferric_active_phase",
        "base_clips_diagnostic_version",
        "base_clips_diagnostic_phase",
        "base_clips_diagnostic_category",
        "base_clips_diagnostic_continued",
        "base_clips_termination",
        "base_clips_exit",
        "base_clips_signal",
        "base_clips_active_phase",
        "head_clips_diagnostic_version",
        "head_clips_diagnostic_phase",
        "head_clips_diagnostic_category",
        "head_clips_diagnostic_continued",
        "head_clips_termination",
        "head_clips_exit",
        "head_clips_signal",
        "head_clips_active_phase",
        "base_oracle_status",
        "head_oracle_status",
        "base_oracle_version",
        "head_oracle_version",
        "base_oracle_normalizations",
        "head_oracle_normalizations",
        "oracle_regression",
        "change",
    ]

    with open(tsv_path, "w", newline="", encoding="utf-8") as f:
        writer = csv.DictWriter(f, fieldnames=fieldnames, delimiter="\t")
        writer.writeheader()

        for key in all_keys:
            b = base_files.get(key)
            h = head_files.get(key)

            b_cls = b["classification"] if b else ""
            b_reason = b.get("reason", "") if b else ""
            h_cls = h["classification"] if h else ""
            h_reason = h.get("reason", "") if h else ""
            source_val = (h or b).get("source", "")
            base_oracle = oracle_evidence_view(b) if b else None
            head_oracle = oracle_evidence_view(h) if h else None
            change, oracle_losses = _change_kind(
                b,
                h,
                base_version=base.get("version"),
                head_version=head.get("version"),
            )

            writer.writerow(
                {
                    "path": key,
                    "source": source_val,
                    "base_classification": b_cls,
                    "base_reason": b_reason,
                    "head_classification": h_cls,
                    "head_reason": h_reason,
                    **_tsv_diagnostic_fields("base", b),
                    **_tsv_diagnostic_fields("head", h),
                    "base_oracle_status": base_oracle["status"] if base_oracle else "",
                    "head_oracle_status": head_oracle["status"] if head_oracle else "",
                    "base_oracle_version": (
                        ""
                        if base_oracle is None or base_oracle["version"] is None
                        else str(base_oracle["version"])
                    ),
                    "head_oracle_version": (
                        ""
                        if head_oracle is None or head_oracle["version"] is None
                        else str(head_oracle["version"])
                    ),
                    "base_oracle_normalizations": (
                        ";".join(base_oracle["normalizations"]) if base_oracle else ""
                    ),
                    "head_oracle_normalizations": (
                        ";".join(head_oracle["normalizations"]) if head_oracle else ""
                    ),
                    "oracle_regression": ";".join(oracle_losses),
                    "change": change,
                }
            )


@app.command()
def main(
    base_manifest: Annotated[str, typer.Argument(help="Base manifest JSON")],
    head_manifest: Annotated[str, typer.Argument(help="Head manifest JSON")],
    tsv: Annotated[str | None, typer.Option(help="Write per-file data as TSV")] = None,
    report: Annotated[str | None, typer.Option(help="Write self-contained Markdown report")] = None,
    repo: Annotated[str | None, typer.Option(help="GitHub repository for commit links")] = None,
    base_sha: Annotated[str | None, typer.Option(help="Base commit SHA")] = None,
    head_sha: Annotated[str | None, typer.Option(help="Head commit SHA")] = None,
) -> None:
    """Compare two compat manifests."""
    base = load_manifest(base_manifest)
    head = load_manifest(head_manifest)

    base_counts, head_counts, regressions, real_improvements, reason_changes = compute_diff(
        base, head
    )

    md_lines = format_markdown(
        base_counts,
        head_counts,
        regressions,
        real_improvements,
        reason_changes,
        repo=repo,
        base_sha=base_sha,
        head_sha=head_sha,
        base_oracle=compute_oracle_coverage(base),
        head_oracle=compute_oracle_coverage(head),
    )
    print("\n".join(md_lines))

    if report:
        with open(report, "w", encoding="utf-8") as f:
            f.write("\n".join(md_lines))
            f.write("\n")

    if tsv:
        write_tsv(base, head, tsv)

    if regressions:
        raise typer.Exit(1)


if __name__ == "__main__":
    app()
