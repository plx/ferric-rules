"""Compare two compatibility manifests and produce a Markdown summary."""

from __future__ import annotations

import csv
import json
from pathlib import Path
from typing import Annotated

import typer
from rich.console import Console

from ferric_tools._clips_parser import (
    COOL_CONSTRUCTS,
    INTERACTIVE_IO,
    LOADING_COMMANDS,
    SUPPORTED_CONSTRUCTS,
    UNSUPPORTED_CONTROL,
    UNSUPPORTED_IO,
)
from ferric_tools.compat.assessment import (
    ASSESSMENT_CATEGORIES,
    load_legacy_manifest,
    project_manifest,
)
from ferric_tools.compat.corpus_render import corpus_diff_lines
from ferric_tools.compat.corpus_summary import corpus_delta, load_summary
from ferric_tools.compat.diagnostics import result_diagnostic_view
from ferric_tools.compat.report import compute_oracle_coverage, oracle_evidence_view

app = typer.Typer(help="Compare two compat manifests.")
console = Console(stderr=True)

DISPLAY_ORDER = list(ASSESSMENT_CATEGORIES)

SCANNER_DIFF_VERSION = 1
FEATURE_SCAN_VERSION = 1
SCANNER_FIELDS = (
    "features",
    "unsupported_features",
    "classification",
    "reason",
    "runability",
)
FEATURE_SCAN_FIELDS = {"version", "status", "detections", "issues"}
FEATURE_SCAN_DETECTION_FIELDS = {"feature", "category", "reason", "head_span", "form_span"}
FEATURE_SCAN_ISSUE_FIELDS = {"kind", "reason", "span"}
FEATURE_SCAN_SPAN_FIELDS = {
    "start_byte",
    "end_byte",
    "start_line",
    "start_column",
    "end_line",
    "end_column",
}
PROJECTED_FEATURES = frozenset((*SUPPORTED_CONSTRUCTS, *COOL_CONSTRUCTS, "printout"))
PROJECTED_UNSUPPORTED_FEATURES = frozenset(
    (*COOL_CONSTRUCTS, *UNSUPPORTED_CONTROL, *UNSUPPORTED_IO, *INTERACTIVE_IO, *LOADING_COMMANDS)
)
FEATURE_SCAN_SPECS = {
    **{feature: ("supported-construct", "supported-form") for feature in SUPPORTED_CONSTRUCTS},
    **{feature: ("cool-construct", "unsupported-form") for feature in COOL_CONSTRUCTS},
    "printout": ("output", "supported-output"),
    **{feature: ("unsupported-control", "unsupported-control") for feature in UNSUPPORTED_CONTROL},
    **{feature: ("file-io", "unsupported-io") for feature in UNSUPPORTED_IO},
    **{feature: ("interactive-io", "interactive") for feature in INTERACTIVE_IO},
    **{feature: ("loading-command", "unsupported-command") for feature in LOADING_COMMANDS},
}
ABSENT_CLASSIFICATION = "absent"
ABSENT_REASON = "not present"


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


def _scanner_string(value: object, *, label: str) -> str:
    if type(value) is not str:
        raise ValueError(f"{label} must be a string")
    return value


def _scanner_string_list(value: object, *, label: str) -> list[str]:
    if type(value) is not list or any(type(item) is not str for item in value):
        raise ValueError(f"{label} must be an array of strings")
    return sorted(value)


def _feature_scan_span(value: object, *, label: str) -> dict[str, int]:
    if type(value) is not dict or set(value) != FEATURE_SCAN_SPAN_FIELDS:
        raise ValueError(
            f"{label} must contain exactly: {', '.join(sorted(FEATURE_SCAN_SPAN_FIELDS))}"
        )

    span: dict[str, int] = {}
    for field in FEATURE_SCAN_SPAN_FIELDS:
        coordinate = value[field]
        minimum = 0 if field in {"start_byte", "end_byte"} else 1
        if type(coordinate) is not int or coordinate < minimum:
            qualifier = "non-negative" if minimum == 0 else "positive"
            raise ValueError(f"{label}.{field} must be a {qualifier} integer")
        span[field] = coordinate
    if span["end_byte"] < span["start_byte"]:
        raise ValueError(f"{label}.end_byte must not precede start_byte")
    return span


def _feature_scan_snapshot(value: object, *, label: str) -> dict:
    if type(value) is not dict or set(value) != FEATURE_SCAN_FIELDS:
        raise ValueError(f"{label} must contain exactly: {', '.join(sorted(FEATURE_SCAN_FIELDS))}")
    if type(value["version"]) is not int or value["version"] != FEATURE_SCAN_VERSION:
        raise ValueError(f"{label}.version must be {FEATURE_SCAN_VERSION}")
    status = value["status"]
    if type(status) is not str or status not in {"valid", "invalid"}:
        raise ValueError(f"{label}.status must be 'valid' or 'invalid'")

    raw_detections = value["detections"]
    if type(raw_detections) is not list:
        raise ValueError(f"{label}.detections must be an array")
    detections: list[dict] = []
    for index, raw_detection in enumerate(raw_detections):
        detection_label = f"{label}.detections[{index}]"
        if type(raw_detection) is not dict or set(raw_detection) != FEATURE_SCAN_DETECTION_FIELDS:
            raise ValueError(
                f"{detection_label} must contain exactly: "
                f"{', '.join(sorted(FEATURE_SCAN_DETECTION_FIELDS))}"
            )
        detections.append(
            {
                "feature": _scanner_string(
                    raw_detection["feature"], label=f"{detection_label}.feature"
                ),
                "category": _scanner_string(
                    raw_detection["category"], label=f"{detection_label}.category"
                ),
                "reason": _scanner_string(
                    raw_detection["reason"], label=f"{detection_label}.reason"
                ),
                "head_span": _feature_scan_span(
                    raw_detection["head_span"], label=f"{detection_label}.head_span"
                ),
                "form_span": _feature_scan_span(
                    raw_detection["form_span"], label=f"{detection_label}.form_span"
                ),
            }
        )

    raw_issues = value["issues"]
    if type(raw_issues) is not list:
        raise ValueError(f"{label}.issues must be an array")
    issues: list[dict] = []
    for index, raw_issue in enumerate(raw_issues):
        issue_label = f"{label}.issues[{index}]"
        if type(raw_issue) is not dict or set(raw_issue) != FEATURE_SCAN_ISSUE_FIELDS:
            raise ValueError(
                f"{issue_label} must contain exactly: "
                f"{', '.join(sorted(FEATURE_SCAN_ISSUE_FIELDS))}"
            )
        issues.append(
            {
                "kind": _scanner_string(raw_issue["kind"], label=f"{issue_label}.kind"),
                "reason": _scanner_string(raw_issue["reason"], label=f"{issue_label}.reason"),
                "span": _feature_scan_span(raw_issue["span"], label=f"{issue_label}.span"),
            }
        )

    if (status == "invalid") != bool(issues):
        raise ValueError(f"{label}.status must be 'invalid' exactly when issues are present")

    return {
        "version": FEATURE_SCAN_VERSION,
        "status": status,
        "detections": detections,
        "issues": issues,
    }


def _validate_scanner_snapshot(snapshot: dict, *, label: str) -> None:
    scan = snapshot.get("feature_scan")
    if scan is None:
        return

    detected_features = {detection["feature"] for detection in scan["detections"]}
    unknown_detections = sorted(detected_features - FEATURE_SCAN_SPECS.keys())
    if unknown_detections:
        raise ValueError(
            f"{label}.feature_scan contains unknown detections: {', '.join(unknown_detections)}"
        )
    for index, detection in enumerate(scan["detections"]):
        expected_category, expected_reason = FEATURE_SCAN_SPECS[detection["feature"]]
        if (detection["category"], detection["reason"]) != (
            expected_category,
            expected_reason,
        ):
            raise ValueError(
                f"{label}.feature_scan.detections[{index}] category/reason must be "
                f"{expected_category!r}/{expected_reason!r} for {detection['feature']!r}"
            )

    projected_features = sorted(detected_features & PROJECTED_FEATURES)
    if snapshot["features"] != projected_features:
        raise ValueError(
            f"{label}.features must exactly project feature_scan detections: "
            f"expected {projected_features!r}, got {snapshot['features']!r}"
        )
    projected_unsupported = sorted(detected_features & PROJECTED_UNSUPPORTED_FEATURES)
    if snapshot["unsupported_features"] != projected_unsupported:
        raise ValueError(
            f"{label}.unsupported_features must exactly project feature_scan detections: "
            f"expected {projected_unsupported!r}, got {snapshot['unsupported_features']!r}"
        )

    malformed_disposition = (
        snapshot["classification"] in ("incompatible", "unassessed")
        and snapshot["reason"] == "malformed-source"
        and snapshot["runability"] == "unknown"
    )
    if scan["status"] == "invalid" and not malformed_disposition:
        raise ValueError(
            f"{label}: invalid feature_scan requires "
            "unassessed (legacy incompatible)/malformed-source/unknown disposition"
        )
    if scan["status"] == "valid" and (
        snapshot["reason"] == "malformed-source" or snapshot["runability"] == "unknown"
    ):
        raise ValueError(f"{label}: valid feature_scan cannot claim a malformed disposition")


def _scanner_snapshot(value: object, *, label: str) -> dict:
    if type(value) is not dict:
        raise ValueError(f"{label} must be an object")
    snapshot = {
        "features": _scanner_string_list(value.get("features"), label=f"{label}.features"),
        "unsupported_features": _scanner_string_list(
            value.get("unsupported_features"), label=f"{label}.unsupported_features"
        ),
        "classification": _scanner_string(
            value.get("classification"), label=f"{label}.classification"
        ),
        "reason": _scanner_string(value.get("reason"), label=f"{label}.reason"),
        "runability": _scanner_string(value.get("runability"), label=f"{label}.runability"),
    }
    if "feature_scan" in value:
        snapshot["feature_scan"] = _feature_scan_snapshot(
            value["feature_scan"], label=f"{label}.feature_scan"
        )
    _validate_scanner_snapshot(snapshot, label=label)
    return snapshot


def _scanner_manifest_files(
    manifest: object,
    *,
    label: str,
    require_structured_evidence: bool = False,
) -> dict[str, dict]:
    if type(manifest) is not dict:
        raise ValueError(f"{label} manifest must be an object")
    raw_files = manifest.get("files")
    if type(raw_files) is not dict:
        raise ValueError(f"{label} manifest files must be an object")

    files: dict[str, dict] = {}
    for path, value in raw_files.items():
        if type(path) is not str or not path:
            raise ValueError(f"{label} manifest file paths must be non-empty strings")
        entry_label = f"{label} manifest files[{path!r}]"
        snapshot = _scanner_snapshot(value, label=entry_label)
        read_error = (
            snapshot["classification"] in ("incompatible", "unassessed")
            and snapshot["reason"] == "read-error"
            and snapshot["runability"] == "unknown"
        )
        if read_error and (snapshot["features"] or snapshot["unsupported_features"]):
            raise ValueError(f"{entry_label}: read-error entries cannot claim detected features")
        if require_structured_evidence and "feature_scan" not in snapshot and not read_error:
            raise ValueError(f"{entry_label}.feature_scan is required")
        files[path] = snapshot
    return files


def _structured_evidence_change(base: dict | None, head: dict | None) -> str:
    base_scan = base.get("feature_scan") if base is not None else None
    head_scan = head.get("feature_scan") if head is not None else None
    if base is None:
        return "added" if head_scan is not None else "absent"
    if head is None:
        return "removed" if base_scan is not None else "absent"
    if base_scan is None and head_scan is None:
        return "absent"
    if base_scan is None and head_scan is not None:
        return "legacy-base"
    if base_scan is not None and head_scan is None:
        return "removed"
    if base_scan != head_scan:
        return "changed"
    return "unchanged"


def _head_scan_requires_retention(head: dict | None) -> bool:
    if head is None or "feature_scan" not in head:
        return False
    scan = head["feature_scan"]
    return scan["status"] != "valid" or bool(scan["issues"])


def compute_scanner_diff(base: dict, head: dict) -> dict:
    """Compare pre-run scanner-owned manifest evidence.

    A base entry without the v1 ``feature_scan`` object is legacy evidence.
    Adding that structured evidence on the head is deliberately neutral by
    itself, while invalid head evidence is retained for review.
    """
    base_files = _scanner_manifest_files(base, label="base")
    head_files = _scanner_manifest_files(head, label="head", require_structured_evidence=True)
    common_paths = sorted(set(base_files) & set(head_files))
    changes: list[dict] = []
    changed_files = 0
    added_files = 0
    removed_files = 0
    legacy_base_structured_evidence = 0
    head_structured_evidence = sum("feature_scan" in snapshot for snapshot in head_files.values())
    head_invalid_structured_evidence = sum(
        snapshot.get("feature_scan", {}).get("status") == "invalid"
        for snapshot in head_files.values()
    )
    head_scan_issues = sum(
        len(snapshot.get("feature_scan", {}).get("issues", [])) for snapshot in head_files.values()
    )

    for path in sorted(set(base_files) | set(head_files)):
        base_snapshot = base_files.get(path)
        head_snapshot = head_files.get(path)
        structured_change = _structured_evidence_change(base_snapshot, head_snapshot)

        if base_snapshot is None:
            added_files += 1
            changes.append(
                {
                    "path": path,
                    "change": "added",
                    "changed_fields": list(SCANNER_FIELDS),
                    "structured_evidence_change": structured_change,
                    "base": None,
                    "head": head_snapshot,
                }
            )
            continue
        if head_snapshot is None:
            removed_files += 1
            changes.append(
                {
                    "path": path,
                    "change": "removed",
                    "changed_fields": list(SCANNER_FIELDS),
                    "structured_evidence_change": structured_change,
                    "base": base_snapshot,
                    "head": None,
                }
            )
            continue

        changed_fields = [
            field for field in SCANNER_FIELDS if base_snapshot[field] != head_snapshot[field]
        ]
        if structured_change == "legacy-base":
            legacy_base_structured_evidence += 1
        elif structured_change in {"changed", "removed"}:
            changed_fields.append("feature_scan")

        retain_head_evidence = _head_scan_requires_retention(head_snapshot)
        if changed_fields:
            changed_files += 1
            change = "changed"
        elif retain_head_evidence:
            change = "structured-evidence"
        else:
            continue
        changes.append(
            {
                "path": path,
                "change": change,
                "changed_fields": changed_fields,
                "structured_evidence_change": structured_change,
                "base": base_snapshot,
                "head": head_snapshot,
            }
        )

    return {
        "version": SCANNER_DIFF_VERSION,
        "base_manifest_version": base.get("version"),
        "head_manifest_version": head.get("version"),
        "summary": {
            "files_compared": len(common_paths),
            "changed_files": changed_files,
            "added_files": added_files,
            "removed_files": removed_files,
            "legacy_base_structured_evidence": legacy_base_structured_evidence,
            "head_structured_evidence": head_structured_evidence,
            "head_invalid_structured_evidence": head_invalid_structured_evidence,
            "head_scan_issues": head_scan_issues,
        },
        "changes": changes,
    }


def _scanner_disposition(snapshot: dict | None) -> str:
    if snapshot is None:
        return "absent"
    return " / ".join(str(snapshot[field]) for field in ("classification", "reason", "runability"))


def _scanner_evidence_label(snapshot: dict | None) -> str:
    if snapshot is None:
        return "absent"
    scan = snapshot.get("feature_scan")
    if scan is None:
        return "legacy"
    issue_count = len(scan["issues"])
    return f"{scan['status']} ({issue_count} issue{'s' if issue_count != 1 else ''})"


def _markdown_cell(value: str) -> str:
    return value.replace("|", "\\|").replace("\n", " ")


def format_scanner_markdown(
    scanner_diff: dict,
    *,
    repo: str | None = None,
    base_sha: str | None = None,
    head_sha: str | None = None,
) -> list[str]:
    """Build a concise retained scanner-evidence report."""
    summary = scanner_diff["summary"]
    lines = [
        "## Static Compatibility Scanner Diff",
        "",
        "Compares scanner-owned fields from manifests captured before either compatibility run.",
        "Observed changes are retained for review and do not fail the comparison job.",
        "",
    ]
    if repo and base_sha and head_sha:
        base_link = f"[`{base_sha[:10]}`](https://github.com/{repo}/commit/{base_sha})"
        head_link = f"[`{head_sha[:10]}`](https://github.com/{repo}/commit/{head_sha})"
        lines.extend([f"Base: {base_link} | Head: {head_link}", ""])

    lines.extend(
        [
            "| Measure | Count |",
            "|---|---:|",
            f"| Files compared | {summary['files_compared']} |",
            f"| Changed files | {summary['changed_files']} |",
            f"| Added files | {summary['added_files']} |",
            f"| Removed files | {summary['removed_files']} |",
            f"| Head files with structured evidence | {summary['head_structured_evidence']} |",
            f"| Head files with invalid scans | {summary['head_invalid_structured_evidence']} |",
            f"| Head lexical issues | {summary['head_scan_issues']} |",
        ]
    )
    legacy_count = summary["legacy_base_structured_evidence"]
    if legacy_count:
        lines.extend(
            [
                "",
                f"The base lacks structured `feature_scan` evidence for {legacy_count} matched "
                "file(s). This is treated as a legacy schema boundary, not as scanner changes.",
            ]
        )

    changes = scanner_diff["changes"]
    lines.extend(["", f"### Retained scanner observations ({len(changes)})", ""])
    if not changes:
        lines.append("None")
        return lines

    lines.extend(
        [
            f"<details><summary>Show {len(changes)} per-file observation(s)</summary>",
            "",
            "| File | Kind | Fields | Base disposition | Head disposition | Head evidence |",
            "|---|---|---|---|---|---|",
        ]
    )
    for change in changes:
        fields = ", ".join(change["changed_fields"]) or "structured evidence"
        lines.append(
            "| "
            f"`{_markdown_cell(change['path'])}` | "
            f"{change['change']} | "
            f"{_markdown_cell(fields)} | "
            f"{_markdown_cell(_scanner_disposition(change['base']))} | "
            f"{_markdown_cell(_scanner_disposition(change['head']))} | "
            f"{_markdown_cell(_scanner_evidence_label(change['head']))} |"
        )
    lines.extend(
        [
            "",
            "</details>",
            "",
            "The JSON artifact retains feature lists, unsupported-feature lists, structured "
            "detections, issues, reasons, and exact spans for every observation above.",
        ]
    )
    return lines


def write_scanner_tsv(scanner_diff: dict, tsv_path: str) -> None:
    """Write retained scanner observations as TSV."""
    fieldnames = [
        "path",
        "change",
        "changed_fields",
        "structured_evidence_change",
        "base_features",
        "head_features",
        "base_unsupported_features",
        "head_unsupported_features",
        "base_classification",
        "head_classification",
        "base_reason",
        "head_reason",
        "base_runability",
        "head_runability",
        "base_feature_scan_status",
        "head_feature_scan_status",
        "base_feature_scan_issues",
        "head_feature_scan_issues",
    ]

    def side_fields(prefix: str, snapshot: dict | None) -> dict[str, str]:
        if snapshot is None:
            return {f"{prefix}_{field}": "" for field in SCANNER_FIELDS} | {
                f"{prefix}_feature_scan_status": "",
                f"{prefix}_feature_scan_issues": "",
            }
        scan = snapshot.get("feature_scan")
        return {
            f"{prefix}_features": ";".join(snapshot["features"]),
            f"{prefix}_unsupported_features": ";".join(snapshot["unsupported_features"]),
            f"{prefix}_classification": snapshot["classification"],
            f"{prefix}_reason": snapshot["reason"],
            f"{prefix}_runability": snapshot["runability"],
            f"{prefix}_feature_scan_status": scan["status"] if scan else "",
            f"{prefix}_feature_scan_issues": (
                json.dumps(scan["issues"], sort_keys=True, separators=(",", ":")) if scan else ""
            ),
        }

    with open(tsv_path, "w", newline="", encoding="utf-8") as stream:
        writer = csv.DictWriter(stream, fieldnames=fieldnames, delimiter="\t")
        writer.writeheader()
        for change in scanner_diff["changes"]:
            writer.writerow(
                {
                    "path": change["path"],
                    "change": change["change"],
                    "changed_fields": ";".join(change["changed_fields"]),
                    "structured_evidence_change": change["structured_evidence_change"],
                    **side_fields("base", change["base"]),
                    **side_fields("head", change["head"]),
                }
            )


def write_scanner_json(scanner_diff: dict, json_path: str) -> None:
    """Write the complete retained scanner diff as deterministic JSON."""
    with open(json_path, "w", encoding="utf-8") as stream:
        json.dump(scanner_diff, stream, indent=2, ensure_ascii=False, sort_keys=True)
        stream.write("\n")


def _oracle_identity(info: dict) -> str | None:
    """Bind legacy comparisons to the same source, harness and oracle contract."""
    declaration = info.get("oracle")
    if not isinstance(declaration, dict):
        return None
    digests = [declaration.get(key) for key in ("source_sha256", "composed_sha256")]
    if not all(isinstance(value, str) and len(value) == 64 for value in digests):
        return None
    contract = {key: value for key, value in declaration.items() if key not in ("nonce", "feature")}
    return json.dumps(
        {"source_sha256": info.get("source_sha256", digests[0]), "oracle": contract},
        sort_keys=True,
        separators=(",", ":"),
    )


def compute_diff(base: dict, head: dict) -> tuple[dict, dict, list, list, list]:
    """Compare executed oracle outcomes; inventory changes are never improvements."""
    base_projection, head_projection = project_manifest(base), project_manifest(head)
    base_counts = {key: base_projection["summary"][key] for key in DISPLAY_ORDER}
    head_counts = {key: head_projection["summary"][key] for key in DISPLAY_ORDER}
    regressions, improvements, changes = [], [], []
    base_files, head_files = base_projection["files"], head_projection["files"]
    for path in sorted(base_files.keys() | head_files.keys()):
        b, h = base_files.get(path), head_files.get(path)
        b_cls = b["classification"] if b else "absent"
        h_cls = h["classification"] if h else "absent"
        b_reason = b.get("reason", "") if b else "not present"
        h_reason = h.get("reason", "") if h else "not present"
        if b is not None and h is not None and _diagnostic_snapshot(b) != _diagnostic_snapshot(h):
            b_reason = _reason_with_diagnostics(b_reason, b)
            h_reason = _reason_with_diagnostics(h_reason, h)
        executed_before = b_cls in ("equivalent", "divergent")
        if executed_before and h is None:
            # Deleting an executed fixture loses at least as much as leaving it
            # unassessed; only inventory without oracle results may disappear.
            h_reason = "not present; oracle-backed fixture removed"
        entry = (path, b_cls, b_reason, h_cls, h_reason)
        if (
            h_cls == "evidence-failure"
            or (b_cls == "equivalent" and h_cls == "divergent")
            or (executed_before and h_cls in ("unassessed", "absent"))
        ):
            regressions.append(entry)
        elif b_cls == "divergent" and h_cls == "equivalent":
            identity = _oracle_identity(b)
            if identity is not None and identity == _oracle_identity(h):
                improvements.append(entry)
            else:
                changes.append(
                    (*entry[:4], entry[4] + "; source/oracle identity changed or unavailable")
                )
        elif (
            b_cls != h_cls
            or b_reason != h_reason
            or (
                b is not None
                and h is not None
                and _diagnostic_snapshot(b) != _diagnostic_snapshot(h)
            )
        ):
            changes.append(entry)
    return base_counts, head_counts, regressions, improvements, changes


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
    base_corpus: dict | None = None,
    head_corpus: dict | None = None,
    legacy_available: bool = True,
) -> list[str]:
    """Build the full Markdown report as a list of lines."""
    lines: list[str] = []
    lines.append("## CLIPS Compatibility Report")
    lines.append("")
    lines.extend(corpus_diff_lines(base_corpus, head_corpus))
    if not legacy_available:
        lines.extend(
            ["### Legacy assessment", "", "Legacy assessment not produced for both revisions.", ""]
        )
        return lines
    lines.extend(
        [
            "### Legacy executed oracles and inventory",
            "",
            "Equivalent/divergent rows require completed oracle evidence. "
            "Unassessed inventory and coverage changes are not engine improvements.",
            "",
        ]
    )

    if repo and base_sha and head_sha:
        base_link = f"[`{base_sha[:10]}`](https://github.com/{repo}/commit/{base_sha})"
        head_link = f"[`{head_sha[:10]}`](https://github.com/{repo}/commit/{head_sha})"
        lines.append(f"Base: {base_link} | Head: {head_link}")
        lines.append("")

    assessed = [category for category in DISPLAY_ORDER if category != "unassessed"]
    base_total = sum(base_counts.get(category, 0) for category in assessed)
    head_total = sum(head_counts.get(category, 0) for category in assessed)

    lines.append("| Classification | Base | Head | Delta |")
    lines.append("|---|---:|---:|---|")
    for cls in assessed:
        b = base_counts.get(cls, 0)
        h = head_counts.get(cls, 0)
        d = h - b
        delta_str = f"**{fmt_delta(d)}**" if d != 0 else "\u2014"
        lines.append(f"| {cls} | {b} | {h} | {delta_str} |")
    d_total = head_total - base_total
    delta_total = f"**{fmt_delta(d_total)}**" if d_total != 0 else "\u2014"
    lines.append(f"| **oracle rows** | **{base_total}** | **{head_total}** | {delta_total} |")
    lines.extend(
        [
            "",
            f"Unassessed inventory: base **{base_counts.get('unassessed', 0)}**, "
            f"head **{head_counts.get('unassessed', 0)}** canonical rows; excluded "
            "from oracle assessment totals.",
        ]
    )

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
            "<details><summary>Coverage, removals and inventory changes"
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
    base = {"files": {"case": base_info} if base_info is not None else {}}
    head = {"files": {"case": head_info} if head_info is not None else {}}
    _, _, regressions, improvements, changes = compute_diff(base, head)
    if regressions:
        return "regression", [regressions[0][4]]
    if improvements:
        return "improvement", []
    if base_info is None:
        return "added", []
    if head_info is None:
        return "removed", []
    return ("inventory-or-coverage-change" if changes else "unchanged"), []


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
    """Write per-file evidence-based classifications as TSV."""
    base, head = project_manifest(base), project_manifest(head)
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
    base_corpus_summary: Annotated[
        Path | None, typer.Option(help="Base granular corpus summary")
    ] = None,
    head_corpus_summary: Annotated[
        Path | None, typer.Option(help="Head granular corpus summary")
    ] = None,
    tsv: Annotated[str | None, typer.Option(help="Write per-file data as TSV")] = None,
    report: Annotated[str | None, typer.Option(help="Write self-contained Markdown report")] = None,
    scanner_only: Annotated[
        bool,
        typer.Option(help="Compare retained pre-run scanner evidence instead of run results"),
    ] = False,
    json_output: Annotated[
        str | None,
        typer.Option("--json", help="Write the retained scanner diff as JSON"),
    ] = None,
    repo: Annotated[str | None, typer.Option(help="GitHub repository for commit links")] = None,
    base_sha: Annotated[str | None, typer.Option(help="Base commit SHA")] = None,
    head_sha: Annotated[str | None, typer.Option(help="Head commit SHA")] = None,
) -> None:
    """Compare two compat manifests."""
    try:
        base_corpus = (
            load_summary(base_corpus_summary)
            if base_corpus_summary and base_corpus_summary.exists()
            else None
        )
        head_corpus = (
            load_summary(head_corpus_summary)
            if head_corpus_summary and head_corpus_summary.exists()
            else None
        )
        base = load_legacy_manifest(base_manifest) if Path(base_manifest).exists() else None
        head = load_legacy_manifest(head_manifest) if Path(head_manifest).exists() else None
    except (OSError, ValueError) as error:
        console.print(f"[red]error:[/] {error}")
        raise typer.Exit(1) from error
    if (base is None or head is None) and (
        scanner_only or (base_corpus is None and head_corpus is None)
    ):
        console.print("[red]error:[/] legacy manifest missing and no corpus summaries supplied")
        raise typer.Exit(1)

    if scanner_only:
        try:
            scanner_diff = compute_scanner_diff(base, head)
        except ValueError as error:
            console.print(f"[red]error:[/] cannot generate scanner diff: {error}")
            raise typer.Exit(2) from error
        scanner_lines = format_scanner_markdown(
            scanner_diff,
            repo=repo,
            base_sha=base_sha,
            head_sha=head_sha,
        )
        print("\n".join(scanner_lines))
        if report:
            with open(report, "w", encoding="utf-8") as f:
                f.write("\n".join(scanner_lines))
                f.write("\n")
        if tsv:
            write_scanner_tsv(scanner_diff, tsv)
        if json_output:
            write_scanner_json(scanner_diff, json_output)
        return

    if json_output:
        console.print("[red]error:[/] --json requires --scanner-only")
        raise typer.Exit(2)

    base_counts, head_counts, regressions, real_improvements, reason_changes = compute_diff(
        base or {"files": {}}, head or {"files": {}}
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
        base_oracle=compute_oracle_coverage(base) if base is not None else None,
        head_oracle=compute_oracle_coverage(head) if head is not None else None,
        base_corpus=base_corpus,
        head_corpus=head_corpus,
        legacy_available=base is not None and head is not None,
    )
    print("\n".join(md_lines))

    if report:
        with open(report, "w", encoding="utf-8") as f:
            f.write("\n".join(md_lines))
            f.write("\n")

    if tsv and base is not None and head is not None:
        write_tsv(base, head, tsv)

    corpus_regressions = (
        corpus_delta(base_corpus, head_corpus)["regressions"]
        if base_corpus is not None and head_corpus is not None
        else []
    )
    if regressions or corpus_regressions:
        raise typer.Exit(1)


if __name__ == "__main__":
    app()
