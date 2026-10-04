"""Evidence-based projection of legacy compatibility inventory.

Scanner labels remain available as raw fields; they are never execution evidence.
This presentation view does not replace the CI gate's physical revalidation.
"""

from __future__ import annotations

from ferric_tools.compat.oracle import (
    DECLARATION_VERSION,
    SCENARIO_DECLARATION_VERSION,
    SUPPORTED_NORMALIZERS,
)

ORACLE_STATUSES = ("valid", "missing", "invalid")
ORACLE_COVERAGE_FIELDS = ("declaration", "reached", "completed", "effect")
ORACLE_EVIDENCE_VERSIONS = frozenset({DECLARATION_VERSION, SCENARIO_DECLARATION_VERSION})


def oracle_evidence_view(info: dict) -> dict:
    """Return a defensive, report-ready view of one file's oracle evidence.

    Legacy entries have no ``oracle_evidence`` member and are treated as
    missing. An equivalent claim without valid evidence is refused and counted
    as invalid, while the rest of a legacy manifest remains reportable.
    """
    raw = info.get("oracle_evidence")
    selected = raw is not None
    malformed = selected and not isinstance(raw, dict)
    evidence = raw if isinstance(raw, dict) else {}

    status = evidence.get("status", "missing")
    if status not in ORACLE_STATUSES:
        status = "invalid"
        malformed = True

    version = evidence.get("version")
    if selected and (type(version) is not int or version not in ORACLE_EVIDENCE_VERSIONS):
        malformed = True

    coverage: dict[str, bool] = {}
    for field in ORACLE_COVERAGE_FIELDS:
        value = evidence.get(field, False)
        if selected and (field not in evidence or type(value) is not bool):
            malformed = True
        coverage[field] = value is True

    raw_normalizations = evidence.get("normalizations", [])
    if not isinstance(raw_normalizations, list) or not all(
        isinstance(item, str) for item in raw_normalizations
    ):
        normalizations: list[str] = []
        malformed = True
    else:
        normalizations = sorted(set(raw_normalizations))
        if (
            selected
            and version in ORACLE_EVIDENCE_VERSIONS
            and any(normalization not in SUPPORTED_NORMALIZERS for normalization in normalizations)
        ):
            malformed = True

    raw_violations = evidence.get("violations", [])
    if not isinstance(raw_violations, list) or not all(
        isinstance(item, str) for item in raw_violations
    ):
        violations = ["malformed violations"]
        malformed = True
    else:
        violations = list(raw_violations)

    invalid_valid_shape = status == "valid" and not all(
        coverage[field] for field in ("declaration", "reached", "completed")
    )
    invalid_missing_shape = status == "missing" and any(
        coverage[field] for field in ("reached", "completed", "effect")
    )
    if malformed or invalid_valid_shape or invalid_missing_shape:
        status = "invalid"

    refused_equivalent = info.get(
        "raw_classification", info.get("classification")
    ) == "equivalent" and (status != "valid" or not all(coverage.values()) or bool(violations))
    if refused_equivalent:
        status = "invalid"

    return {
        "selected": selected,
        "status": status,
        "version": version,
        **coverage,
        "normalizations": normalizations,
        "violations": violations,
        "refused_equivalent": refused_equivalent,
    }


ASSESSMENT_CATEGORIES = ("equivalent", "divergent", "unassessed", "evidence-failure")


def assessment_view(info: dict) -> dict:
    """Return a classification supported by retained execution evidence."""
    evidence = oracle_evidence_view(info)
    declared = info.get("oracle") is not None or evidence["declaration"]
    classification = info.get("raw_classification", info.get("classification"))
    reason = info.get("raw_reason", info.get("reason", ""))
    engines = [info.get(name) for name in ("ferric", "clips")]
    executed = all(
        isinstance(result, dict)
        and isinstance(result.get("canonical_observation"), dict)
        and not result.get("not_run")
        and not result.get("projection_error")
        for result in engines
    )
    if (info.get("oracle") is not None and not isinstance(info["oracle"], dict)) or (
        declared and evidence["status"] == "invalid"
    ):
        return {"classification": "evidence-failure", "reason": reason or "oracle-invalid"}
    if (
        classification in ("equivalent", "divergent")
        and evidence["status"] == "valid"
        and evidence["completed"]
        and executed
    ):
        return {"classification": classification, "reason": reason}
    if declared and any(result is not None for result in engines):
        return {"classification": "evidence-failure", "reason": reason or "execution-incomplete"}
    inventory_reason = reason or "oracle-missing"
    if not declared and not inventory_reason.startswith("oracle-missing"):
        inventory_reason = f"oracle-missing; scanner: {inventory_reason}"
    elif declared:
        inventory_reason = "oracle-not-executed"
    return {"classification": "unassessed", "reason": inventory_reason}


def project_manifest(manifest: dict) -> dict:
    """Copy a manifest into a consistent semantic view without losing raw labels."""
    files = {}
    for path, info in manifest.get("files", {}).items():
        files[path] = {
            **info,
            "raw_classification": info.get("raw_classification", info.get("classification")),
            "raw_reason": info.get("raw_reason", info.get("reason", "")),
            **assessment_view(info),
        }
    summary = {category: 0 for category in ASSESSMENT_CATEGORIES}
    for info in files.values():
        summary[info["classification"]] += 1
    summary["total"] = len(files)
    for field in ("physical_paths", "unique_contents", "duplicate_aliases"):
        if field in manifest.get("summary", {}):
            summary[field] = manifest["summary"][field]
    return {**manifest, "files": files, "summary": summary}


def load_legacy_manifest(path):
    """Reject malformed supplied manifests instead of rendering empty success."""
    import json
    from pathlib import Path

    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError(f"duplicate manifest field: {key}")
            result[key] = value
        return result

    manifest = json.loads(Path(path).read_text(encoding="utf-8"), object_pairs_hook=unique)
    if not isinstance(manifest, dict) or not isinstance(manifest.get("files"), dict):
        raise ValueError("legacy manifest files must be an object")
    for path, info in manifest["files"].items():
        if not isinstance(path, str) or not isinstance(info, dict):
            raise ValueError("legacy manifest entries must be objects")
        if not isinstance(info.get("classification"), str):
            raise ValueError(f"legacy manifest classification missing for {path}")
    return manifest
