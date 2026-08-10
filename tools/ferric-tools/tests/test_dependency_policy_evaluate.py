"""End-to-end offline evidence tests for ``dependency-policy.py evaluate``."""

from __future__ import annotations

import base64
import copy
import hashlib
import importlib.util
import json
import pathlib
import shutil
import subprocess
import sys
import tomllib
from typing import Any

import pytest

REPO_ROOT = pathlib.Path(__file__).resolve().parents[3]
EVALUATOR = REPO_ROOT / "scripts" / "dependency-policy.py"
COMMITTED_POLICY = REPO_ROOT / "dependency-policy.json"
COMMITTED_DENY = REPO_ROOT / "deny.toml"
FIXTURES = pathlib.Path(__file__).with_name("fixtures") / "dependency-policy"
TODAY = "2026-08-09"
CANDIDATE_SHA = "a" * 40
CARGO_WORKSPACE_MANIFESTS_SHA256 = (
    "4c270476c840bec13f14fa531496268ce2e26aef1a450881c82231d903023753"
)

PROJECTS = {
    "rust-workspace": ("cargo", "Cargo.toml", "Cargo.lock"),
    "node-package": ("npm", "packages/ferric/package.json", "packages/ferric/package-lock.json"),
    "node-addon": (
        "npm",
        "crates/ferric-rules-napi/package.json",
        "crates/ferric-rules-napi/package-lock.json",
    ),
    "documentation": ("npm", "documentation/package.json", "documentation/package-lock.json"),
    "site": ("npm", "site/package.json", "site/package-lock.json"),
    "python-package": (
        "pypi",
        "crates/ferric-rules-python/pyproject.toml",
        "crates/ferric-rules-python/uv.lock",
    ),
    "python-tools": (
        "pypi",
        "tools/ferric-tools/pyproject.toml",
        "tools/ferric-tools/uv.lock",
    ),
}

TOOL_PINS = {
    "rustc": "1.93.0",
    "cargo_audit": "0.22.2",
    "cargo_deny": "0.20.2",
    "cargo_cyclonedx": "0.5.9",
    "cargo_about": "0.9.0",
    "uv": "0.11.16",
    "pip_audit": "2.10.1",
    "node": "22.18.0",
    "npm": "11.12.1",
    "cyclonedx_npm": "4.2.1",
}

SCAN_FILES = {
    ("rust-workspace", "cargo-audit"): "rust-workspace.cargo-audit.json",
    ("rust-workspace", "cargo-deny"): "cargo-deny.json",
    ("node-package", "npm-audit"): "node-package.npm-audit.json",
    ("node-addon", "npm-audit"): "node-addon.npm-audit.json",
    ("documentation", "npm-audit"): "documentation.npm-audit.json",
    ("site", "npm-audit"): "site.npm-audit.json",
    ("python-package", "pip-audit"): "python-package.pip-audit.json",
    ("python-tools", "pip-audit"): "python-tools.pip-audit.json",
}


def _load_evaluator():
    spec = importlib.util.spec_from_file_location("dependency_policy_evaluate_contract", EVALUATOR)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def _json_bytes(value: object) -> bytes:
    return (json.dumps(value, indent=2, sort_keys=True) + "\n").encode()


def _write_json(path: pathlib.Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(_json_bytes(value))


def _sha256(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def _workspace_manifest_paths(root: pathlib.Path) -> list[pathlib.Path]:
    workspace = tomllib.loads((root / "Cargo.toml").read_text())["workspace"]
    paths = {pathlib.Path("Cargo.toml")}
    for pattern in workspace["members"]:
        paths.update(
            member.relative_to(root) / "Cargo.toml"
            for member in root.glob(pattern)
            if member.is_dir()
        )
    return sorted(paths)


def _component_hashes(hashes: list[str]) -> list[dict[str, str]]:
    result = []
    for value in hashes:
        algorithm, content = value.split(":", 1)
        result.append(
            {
                "alg": {"sha256": "SHA-256", "sha512": "SHA-512"}[algorithm],
                "content": content,
            }
        )
    return result


def _cargo_lock_union_sbom(components: list[dict[str, Any]]) -> dict[str, Any]:
    return {
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "version": 1,
        "components": [
            {
                "type": "library",
                "name": component["name"],
                "version": component["version"],
                **(
                    {"hashes": _component_hashes(sorted(component["hashes"]))}
                    if component.get("hashes")
                    else {}
                ),
            }
            for component in sorted(
                components,
                key=lambda component: (component["name"], component["version"]),
            )
        ],
    }


def _sbom(project_id: str, ecosystem: str, components: list[dict[str, Any]]) -> dict[str, Any]:
    values = []
    for index, component in enumerate(components):
        name, version = component["name"], component["version"]
        path = component.get("path", str(index))
        values.append(
            {
                "type": "library",
                "bom-ref": f"{ecosystem}:{path}:{name}@{version}",
                "name": name,
                "version": version,
                "hashes": _component_hashes(component.get("hashes", [])),
            }
        )
    return {
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "version": 1,
        "metadata": {"properties": [{"name": "ferric:project-id", "value": project_id}]},
        "components": values,
    }


def _adversarial_sbom_component(mutation: str) -> dict[str, Any]:
    component: dict[str, Any] = {
        "type": "library",
        "bom-ref": f"fixture:adversarial:{mutation}",
        "name": "unexpected-component",
        "version": "9.0.0",
        "hashes": [],
    }
    field = "name" if mutation.endswith("name") else "version"
    if mutation.startswith("missing-"):
        component.pop(field)
    elif mutation.startswith("empty-"):
        component[field] = ""
    elif mutation.startswith("blank-"):
        component[field] = "   "
    elif mutation.startswith("wrong-type-"):
        component[field] = ["not", "a", "string"]
    return component


def _cargo_components(lock_bytes: bytes) -> list[dict[str, Any]]:
    lock = tomllib.loads(lock_bytes.decode())
    result = []
    for package in lock["package"]:
        hashes = [f"sha256:{package['checksum']}"] if "checksum" in package else []
        result.append({"name": package["name"], "version": package["version"], "hashes": hashes})
    return result


def _npm_components(lock_bytes: bytes) -> list[dict[str, Any]]:
    lock = json.loads(lock_bytes)
    result = []
    for path, package in lock["packages"].items():
        name = package.get("name")
        version = package.get("version")
        if not name or not version:
            continue
        hashes = []
        integrity = package.get("integrity")
        if integrity:
            algorithm, encoded = integrity.split("-", 1)
            hashes.append(f"{algorithm}:{base64.b64decode(encoded).hex()}")
        result.append({"name": name, "version": version, "path": path, "hashes": hashes})
    return result


def _uv_components(lock_bytes: bytes) -> list[dict[str, Any]]:
    lock = tomllib.loads(lock_bytes.decode())
    result = []
    for package in lock["package"]:
        if "registry" not in package.get("source", {}):
            continue
        hashes = []
        if "sdist" in package:
            hashes.append(package["sdist"]["hash"])
        hashes.extend(wheel["hash"] for wheel in package.get("wheels", []))
        result.append(
            {"name": package["name"], "version": package["version"], "hashes": sorted(hashes)}
        )
    return result


def _lock_components(ecosystem: str, lock_bytes: bytes) -> list[dict[str, Any]]:
    if ecosystem == "cargo":
        return _cargo_components(lock_bytes)
    if ecosystem == "npm":
        return _npm_components(lock_bytes)
    return _uv_components(lock_bytes)


def _npm_lock(project_id: str, digit: str) -> bytes:
    dependency = f"{project_id}-dependency"
    digest = bytes.fromhex(digit * 64)
    return _json_bytes(
        {
            "name": f"{project_id}-root",
            "version": "0.1.0",
            "lockfileVersion": 3,
            "requires": True,
            "packages": {
                "": {
                    "name": f"{project_id}-root",
                    "version": "0.1.0",
                    "dependencies": {dependency: "1.0.0"},
                },
                f"node_modules/{dependency}": {
                    "name": dependency,
                    "version": "1.0.0",
                    "resolved": f"https://registry.npmjs.org/{dependency}/-/{dependency}-1.0.0.tgz",
                    "integrity": f"sha256-{base64.b64encode(digest).decode()}",
                    "license": "MIT",
                },
            },
        }
    )


def _uv_lock(project_id: str, digit: str) -> bytes:
    dependency = f"{project_id}-dependency"
    build_binding = (
        "\n[package.dev-dependencies]\n"
        f'build = [{{ name = "{dependency}", version = "1.0.0", '
        'source = { registry = "https://pypi.org/simple" } }]\n\n'
        "[package.metadata]\n\n"
        "[package.metadata.requires-dev]\n"
        f'build = [{{ name = "{dependency}", specifier = "==1.0.0" }}]\n'
        if project_id == "python-package"
        else ""
    )
    return (
        "version = 1\n"
        "revision = 3\n"
        'requires-python = ">=3.9"\n\n'
        "[[package]]\n"
        f'name = "{project_id}"\n'
        'version = "0.1.0"\n'
        'source = { editable = "." }\n'
        f'dependencies = [{{ name = "{dependency}" }}]\n'
        f"{build_binding}\n"
        "[[package]]\n"
        f'name = "{dependency}"\n'
        'version = "1.0.0"\n'
        'source = { registry = "https://pypi.org/simple" }\n'
        f'sdist = {{ url = "https://files.pythonhosted.org/{dependency}-1.0.0.tar.gz", '
        f'hash = "sha256:{digit * 64}", size = 1 }}\n'
    ).encode()


def _clean_cargo_audit() -> dict[str, Any]:
    return {
        "database": {
            "advisory-count": 1,
            "last-commit": "1" * 40,
            "last-updated": "2026-08-09T00:00:00Z",
        },
        "lockfile": {"dependency-count": 6},
        "settings": {
            "target_arch": [],
            "target_os": [],
            "severity": None,
            "ignore": [],
            "informational_warnings": ["unmaintained", "unsound", "notice"],
        },
        "vulnerabilities": {"found": False, "count": 0, "list": []},
        "warnings": {"unmaintained": [], "unsound": [], "notice": [], "yanked": []},
    }


def _cargo_audit_with_vulnerability(
    name: str = "fixture-build",
    version: str = "2.0.0",
    finding_id: str = "RUSTSEC-2099-0100",
) -> dict[str, Any]:
    report = _clean_cargo_audit()
    report["vulnerabilities"] = {
        "found": True,
        "count": 1,
        "list": [
            {
                "advisory": {
                    "id": finding_id,
                    "package": name,
                    "title": "cross-scanner reconciliation fixture",
                    "informational": None,
                    "cvss": "7.5",
                    "aliases": [],
                },
                "versions": {"patched": [], "unaffected": []},
                "affected": None,
                "package": {
                    "name": name,
                    "version": version,
                    "source": "registry+https://github.com/rust-lang/crates.io-index",
                    "checksum": "2" * 64,
                    "dependencies": [],
                    "replace": None,
                },
            }
        ],
    }
    return report


def _cargo_deny_summary(
    *, errors: int = 0, warnings: int = 0, notes: int = 0, helps: int = 0
) -> dict[str, Any]:
    empty = {"errors": 0, "warnings": 0, "notes": 0, "helps": 0}
    return {
        "type": "summary",
        "fields": {
            "advisories": {
                "errors": errors,
                "warnings": warnings,
                "notes": notes,
                "helps": helps,
            },
            "bans": dict(empty),
            "licenses": dict(empty),
            "sources": dict(empty),
        },
    }


def _clean_cargo_deny() -> list[dict[str, Any]]:
    return [_cargo_deny_summary()]


def _cargo_deny_advisory(
    name: str = "fixture-build",
    version: str = "2.0.0",
    finding_id: str = "RUSTSEC-2099-0100",
) -> list[dict[str, Any]]:
    return [
        {
            "type": "diagnostic",
            "fields": {
                "code": "vulnerability",
                "severity": "error",
                "message": f"advisory found for {name} {version}",
                "graphs": [{"Krate": {"name": name, "version": version}}],
                "advisory": {"id": finding_id},
            },
        },
        _cargo_deny_summary(errors=1),
    ]


def _cargo_deny_license_finding() -> list[dict[str, Any]]:
    summary = _cargo_deny_summary()
    summary["fields"]["licenses"]["errors"] = 1
    return [
        {
            "type": "diagnostic",
            "fields": {
                "code": "unlicensed",
                "graphs": [
                    {
                        "Krate": {"name": "fixture-build", "version": "2.0.0"},
                    }
                ],
                "message": "fixture license finding",
                "severity": "error",
            },
        },
        summary,
    ]


def _clean_npm_audit() -> dict[str, Any]:
    return {
        "auditReportVersion": 2,
        "vulnerabilities": {},
        "metadata": {
            "vulnerabilities": {
                "info": 0,
                "low": 0,
                "moderate": 0,
                "high": 0,
                "critical": 0,
                "total": 0,
            },
            "dependencies": {
                "prod": 2,
                "dev": 0,
                "optional": 0,
                "peer": 0,
                "peerOptional": 0,
                "total": 1,
            },
        },
    }


def _npm_audit_with_vulnerability(
    project_id: str,
    *,
    severity: str = "low",
    advisory_ids: tuple[str, ...] = ("GHSA-abcd-1234-efgh",),
) -> dict[str, Any]:
    dependency = f"{project_id}-dependency"
    report = _clean_npm_audit()
    report["vulnerabilities"] = {
        dependency: {
            "name": dependency,
            "severity": severity,
            "isDirect": False,
            "via": [
                {
                    "source": 42 + index,
                    "name": dependency,
                    "dependency": dependency,
                    "title": f"fixture finding {index}",
                    "url": f"https://github.com/advisories/{advisory_id}",
                    "severity": severity,
                    "range": "<=1.0.0",
                }
                for index, advisory_id in enumerate(advisory_ids)
            ],
            "effects": [],
            "range": "<=1.0.0",
            "nodes": [f"node_modules/{dependency}"],
            "fixAvailable": False,
        }
    }
    report["metadata"]["vulnerabilities"].update({severity: 1, "total": 1})
    return report


def _npm_exception(project_id: str, finding_id: str) -> dict[str, Any]:
    _, _, lockfile = PROJECTS[project_id]
    return {
        "exception_id": "fixture-npm-exception",
        "ecosystem": "npm",
        "project_id": project_id,
        "lockfile": lockfile,
        "kind": "vulnerability",
        "finding_id": finding_id,
        "package": {"name": f"{project_id}-dependency", "version": "1.0.0"},
        "scanner_severity": "low",
        "dependency_scopes": ["runtime"],
        "dev_only": False,
        "affected_surfaces": ["fixture"],
        "reachability": "unknown",
        "evidence": [{"kind": "fixture", "reference": "offline node-resolution adversary"}],
        "owner": "release-engineering",
        "tracking_issue": "https://github.com/plx/ferric-rules/issues/215",
        "rationale": "Prove one excepted node cannot mask an unresolved sibling node.",
        "remediation": "Remove the fixture.",
        "issued_on": TODAY,
        "expires_on": "2026-09-08",
    }


def _cargo_graph_sha256(
    module: object,
    repo: pathlib.Path,
    **overrides: object,
) -> str:
    policy = json.loads((repo / "dependency-policy.json").read_text())
    project = next(item for item in policy["projects"] if item["project_id"] == "rust-workspace")
    values: dict[str, object] = {
        "project_id": project["project_id"],
        "lockfile": project["lockfile"],
        "lockfile_sha256": _sha256(repo / project["lockfile"]),
        "workspace_manifests_sha256": CARGO_WORKSPACE_MANIFESTS_SHA256,
        "cargo_config_sha256": _sha256(repo / ".cargo/config.toml"),
        "deny_config_sha256": _sha256(repo / "deny.toml"),
        "dependency_groups": project["dependency_groups"],
        "targets": project["targets"],
        "features": project["features"],
    }
    values.update(overrides)
    return module._cargo_graph_sha256(**values)


def _cargo_exception(cargo_graph_sha256: str) -> dict[str, Any]:
    return {
        "exception_id": "fixture-cargo-exception",
        "ecosystem": "cargo",
        "project_id": "rust-workspace",
        "lockfile": "Cargo.lock",
        "cargo_graph_sha256": cargo_graph_sha256,
        "kind": "vulnerability",
        "finding_id": "RUSTSEC-2099-0100",
        "package": {"name": "fixture-build", "version": "2.0.0"},
        "scanner_severity": "high",
        "dependency_scopes": ["build"],
        "dev_only": False,
        "affected_surfaces": ["fixture-published-artifact"],
        "reachability": "unknown",
        "evidence": [{"kind": "fixture", "reference": "offline Cargo graph adversary"}],
        "owner": "release-engineering",
        "tracking_issue": "https://github.com/plx/ferric-rules/issues/215",
        "rationale": "Exercise an exact graph-bound Cargo exception.",
        "remediation": "Remove the fixture.",
        "issued_on": TODAY,
        "expires_on": "2026-09-08",
    }


def _clean_pip_audit(components: list[dict[str, Any]]) -> dict[str, Any]:
    return {
        "dependencies": [
            {"name": component["name"], "version": component["version"], "vulns": []}
            for component in components
        ],
        "fixes": [],
    }


def _uv_export_command() -> list[str]:
    return [
        "uv",
        "--no-config",
        "--no-cache",
        "export",
        "--locked",
        "--all-groups",
        "--all-extras",
        "--no-emit-project",
        "--format",
        "requirements.txt",
    ]


def _pip_envelope(*reports: dict[str, Any]) -> dict[str, Any]:
    return {
        "schema": "ferric.pip-audit-batches",
        "version": 1,
        "export_command": _uv_export_command(),
        "audit_command": _scan_command("pip-audit"),
        "reports": list(reports),
    }


def _scan_command(scanner: str) -> list[str]:
    if scanner == "cargo-audit":
        return ["cargo-audit", "audit", "--file", "Cargo.lock", "--format", "json"]
    if scanner == "cargo-deny":
        return [
            "cargo-deny",
            "--locked",
            "--all-features",
            "--format",
            "json",
            "check",
            "advisories",
            "bans",
            "licenses",
            "sources",
        ]
    if scanner == "npm-audit":
        return [
            "npm",
            "audit",
            "--package-lock-only",
            "--json",
            "--audit-level=info",
            "--registry=https://registry.npmjs.org/",
            "--include=dev",
            "--include=optional",
            "--include=peer",
        ]
    return [
        "uvx",
        "--isolated",
        "--no-env-file",
        "--no-config",
        "--no-cache",
        "--default-index",
        "https://pypi.org/simple",
        "--from",
        "pip-audit==2.10.1",
        "pip-audit",
        "--strict",
        "--no-deps",
        "--require-hashes",
        "--disable-pip",
        "--vulnerability-service",
        "pypi",
        "--format",
        "json",
        "-r",
        "<marker-free-requirements-batch>",
    ]


def _scan_environment(scanner: str) -> dict[str, str]:
    cargo_aliases = {
        "CARGO_ALIAS_AUDIT": "<cleared>",
        "CARGO_ALIAS_DENY": "<cleared>",
        "CARGO_ALIAS_CYCLONEDX": "<cleared>",
        "CARGO_ALIAS_ABOUT": "<cleared>",
    }
    if scanner == "cargo-audit":
        return {
            "CARGO_HOME": "<isolated-empty-directory>",
            "CARGO_AUDIT_*": "<cleared>",
            "RUSTSEC_*": "<cleared>",
            **cargo_aliases,
        }
    if scanner == "cargo-deny":
        return cargo_aliases
    if scanner == "npm-audit":
        return {
            "NPM_CONFIG_*": "<cleared>",
            "NPM_CONFIG_USERCONFIG": "<isolated-empty-file>",
            "NPM_CONFIG_GLOBALCONFIG": "<isolated-empty-file>",
            "NPM_CONFIG_CACHE": "<isolated-directory>",
        }
    if scanner == "pip-audit":
        return {"PIP_AUDIT_*": "<cleared>", "UV_*": "<cleared>"}
    return {}


def _sbom_environment(ecosystem: str) -> dict[str, str]:
    if ecosystem == "cargo":
        return {
            "CARGO_ALIAS_AUDIT": "<cleared>",
            "CARGO_ALIAS_DENY": "<cleared>",
            "CARGO_ALIAS_CYCLONEDX": "<cleared>",
            "CARGO_ALIAS_ABOUT": "<cleared>",
        }
    if ecosystem == "npm":
        return _scan_environment("npm-audit")
    return {"UV_*": "<cleared>"}


def _sbom_command(ecosystem: str) -> list[str]:
    if ecosystem == "cargo":
        return [
            "cargo-cyclonedx",
            "cyclonedx",
            "--format",
            "json",
            "--spec-version",
            "1.5",
            "--all-features",
            "--target",
            "all",
            "--describe",
            "all-cargo-targets",
            "--all",
        ]
    if ecosystem == "npm":
        return [
            "cyclonedx-npm",
            "--package-lock-only",
            "--spec-version",
            "1.5",
            "--output-format",
            "JSON",
            "--output-reproducible",
            "--flatten-components",
            "--output-file",
            "<output>",
            "<manifest>",
        ]
    return [
        "uv",
        "--no-config",
        "--no-cache",
        "--preview-features",
        "sbom-export",
        "export",
        "--locked",
        "--all-groups",
        "--all-extras",
        "--no-emit-project",
        "--format",
        "cyclonedx1.5",
    ]


def _working_directory(project_id: str) -> str:
    manifest = pathlib.PurePosixPath(PROJECTS[project_id][1])
    return "." if str(manifest.parent) == "." else str(manifest.parent)


def _refresh_raw_manifest(evidence: pathlib.Path) -> None:
    raw = evidence / "raw"
    manifest_path = raw / "scan-manifest.json"
    manifest = json.loads(manifest_path.read_text())
    raw_files = []
    for path in sorted(raw.iterdir()):
        if path.name != "scan-manifest.json":
            raw_files.append({"path": path.name, "sha256": _sha256(path)})
    manifest["raw_files"] = raw_files
    raw_hashes = {item["path"]: item["sha256"] for item in raw_files}
    for scan in manifest["scans"]:
        scan["raw_sha256"] = raw_hashes[scan["report"]]
    manifest["license_notice"]["sha256"] = raw_hashes["license-notices.json"]
    _write_json(manifest_path, manifest)


def _refresh_sbom_manifest(evidence: pathlib.Path, project_id: str) -> None:
    scan_path = evidence / "raw" / "scan-manifest.json"
    scan_manifest = json.loads(scan_path.read_text())
    sbom_dir = evidence / "sboms"
    if project_id == "rust-workspace":
        rust_manifest_path = sbom_dir / "rust-workspace.sbom-manifest.json"
        rust_manifest = json.loads(rust_manifest_path.read_text())
        for member in rust_manifest["members"]:
            member["sha256"] = _sha256(sbom_dir / member["path"])
        _write_json(rust_manifest_path, rust_manifest)
        changed = rust_manifest_path
    else:
        changed = sbom_dir / f"{project_id}.cdx.json"
    for item in scan_manifest["sboms"]:
        if item["project_id"] == project_id:
            item["sha256"] = _sha256(changed)
    _write_json(scan_path, scan_manifest)


def _refresh_lock_bindings(
    repo: pathlib.Path,
    evidence: pathlib.Path,
    project_id: str,
) -> None:
    lockfile = PROJECTS[project_id][2]
    manifest = PROJECTS[project_id][1]
    lock_sha = _sha256(repo / lockfile)
    manifest_sha = _sha256(repo / manifest)
    manifest_path = evidence / "raw" / "scan-manifest.json"
    manifest = json.loads(manifest_path.read_text())
    input_record = next(item for item in manifest["inputs"] if item["project_id"] == project_id)
    input_record["sha256"] = lock_sha
    input_record["manifest_sha256"] = manifest_sha
    for scan in manifest["scans"]:
        if scan["project_id"] == project_id:
            scan["lockfile_sha256"] = lock_sha
    next(item for item in manifest["sboms"] if item["project_id"] == project_id)[
        "lockfile_sha256"
    ] = lock_sha
    _write_json(manifest_path, manifest)


def _refresh_cargo_lock_evidence(
    repo: pathlib.Path,
    evidence: pathlib.Path,
    cargo_audit: dict[str, Any],
) -> None:
    lock_path = repo / "Cargo.lock"
    lock_sha = _sha256(lock_path)
    components = _cargo_components(lock_path.read_bytes())
    cargo_audit["lockfile"]["dependency-count"] = len(components)
    _replace_raw(evidence, "rust-workspace.cargo-audit.json", cargo_audit)
    _refresh_lock_bindings(repo, evidence, "rust-workspace")

    license_path = evidence / "raw/license-notices.json"
    license_evidence = json.loads(license_path.read_text())
    license_evidence["cargo_lock_sha256"] = lock_sha
    _replace_raw(evidence, license_path.name, license_evidence)
    scan_path = evidence / "raw/scan-manifest.json"
    scan_manifest = json.loads(scan_path.read_text())
    scan_manifest["license_notice"]["cargo_lock_sha256"] = lock_sha
    _write_json(scan_path, scan_manifest)

    sbom_dir = evidence / "sboms"
    supplement_path = sbom_dir / "rust-workspace/cargo-lock-union.cdx.json"
    _write_json(supplement_path, _cargo_lock_union_sbom(components))
    rust_manifest_path = sbom_dir / "rust-workspace.sbom-manifest.json"
    rust_manifest = json.loads(rust_manifest_path.read_text())
    rust_manifest["lockfile_sha256"] = lock_sha
    _write_json(rust_manifest_path, rust_manifest)
    _refresh_sbom_manifest(evidence, "rust-workspace")


def _build_bundle(
    tmp_path: pathlib.Path,
    *,
    marker_python_tools: bool = False,
    host_only_python_tools: bool = False,
    current_python_package_lock: bool = False,
) -> tuple[pathlib.Path, pathlib.Path]:
    repo = tmp_path / "repo"
    evidence = tmp_path / "evidence"
    raw = evidence / "raw"
    sboms = evidence / "sboms"
    raw.mkdir(parents=True)
    sboms.mkdir(parents=True)

    policy = json.loads(COMMITTED_POLICY.read_text())
    policy["exceptions"] = []
    _write_json(repo / "dependency-policy.json", policy)
    (repo / "deny.toml").write_bytes(COMMITTED_DENY.read_bytes())
    for relative in (
        ".cargo/config.toml",
        "about.toml",
        "licenses/third-party-notices.hbs",
        "scripts/license-notices.sh",
        "crates/ferric-rules-core/Cargo.toml",
        "crates/ferric-rules-ffi-macros/Cargo.toml",
        "crates/ferric-rules-parser/Cargo.toml",
        "crates/ferric-rules-pinned/Cargo.toml",
        "crates/ferric-rules-runtime/Cargo.toml",
    ):
        destination = repo / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes((REPO_ROOT / relative).read_bytes())
    for relative in _workspace_manifest_paths(REPO_ROOT):
        destination = repo / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes((REPO_ROOT / relative).read_bytes())

    lock_bytes: dict[str, bytes] = {}
    for index, (project_id, (ecosystem, manifest, lockfile)) in enumerate(PROJECTS.items(), 1):
        manifest_path = repo / manifest
        manifest_path.parent.mkdir(parents=True, exist_ok=True)
        if ecosystem == "cargo":
            data = (FIXTURES / "cargo-all-scope.lock").read_bytes()
        elif ecosystem == "npm":
            data = _npm_lock(project_id, str(index))
            root_package = json.loads(data)["packages"][""]
            manifest_path.write_text(
                json.dumps(
                    {
                        field: root_package[field]
                        for field in (
                            "name",
                            "version",
                            "dependencies",
                            "devDependencies",
                            "optionalDependencies",
                            "peerDependencies",
                            "peerDependenciesMeta",
                        )
                        if field in root_package
                    }
                ),
                encoding="utf-8",
            )
        else:
            if project_id == "python-package" and current_python_package_lock:
                manifest_path.write_bytes((REPO_ROOT / manifest).read_bytes())
                data = (REPO_ROOT / lockfile).read_bytes()
            else:
                build_requirement = (
                    f"{project_id}-dependency==1.0.0"
                    if project_id == "python-package"
                    else "hatchling"
                )
                manifest_path.write_text(
                    "[build-system]\n"
                    f'requires = ["{build_requirement}"]\n'
                    'build-backend = "fixture_backend"\n\n'
                    '[project]\nname = "fixture"\nversion = "0.1.0"\n'
                    'requires-python = ">=3.9"\n',
                    encoding="utf-8",
                )
                data = (
                    (FIXTURES / "uv-marker-variants.lock").read_bytes()
                    if project_id == "python-tools" and marker_python_tools
                    else _uv_lock(project_id, str(index))
                )
        lock_path = repo / lockfile
        lock_path.parent.mkdir(parents=True, exist_ok=True)
        lock_path.write_bytes(data)
        lock_bytes[project_id] = data

    notices = repo / "THIRD_PARTY_NOTICES.md"
    notices.write_text("fixture notices\n", encoding="utf-8")

    reports: dict[str, object] = {
        "rust-workspace.cargo-audit.json": _clean_cargo_audit(),
        "cargo-deny.json": _clean_cargo_deny(),
    }
    components: dict[str, list[dict[str, Any]]] = {}
    for project_id, (ecosystem, _, _) in PROJECTS.items():
        values = _lock_components(ecosystem, lock_bytes[project_id])
        components[project_id] = values
        if ecosystem == "npm":
            reports[f"{project_id}.npm-audit.json"] = _clean_npm_audit()
        elif ecosystem == "pypi":
            if project_id == "python-tools" and marker_python_tools and host_only_python_tools:
                reports[f"{project_id}.pip-audit.json"] = _pip_envelope(
                    json.loads((FIXTURES / "pip-audit-host-only-clean.json").read_text())
                )
            elif project_id == "python-tools" and marker_python_tools:
                reports[f"{project_id}.pip-audit.json"] = _pip_envelope(
                    json.loads((FIXTURES / "pip-audit-all-markers-clean.json").read_text())
                )
            else:
                reports[f"{project_id}.pip-audit.json"] = _pip_envelope(_clean_pip_audit(values))
    for filename, value in reports.items():
        _write_json(raw / filename, value)

    _write_json(raw / "tool-versions.json", TOOL_PINS)
    license_notice = {
        "schema": "ferric.license-notices-evidence",
        "version": 1,
        "candidate_sha": CANDIDATE_SHA,
        "evaluated_on": TODAY,
        "command": ["./scripts/license-notices.sh", "check"],
        "working_directory": ".",
        "exit_code": 0,
        "status": "pass",
        "path": "THIRD_PARTY_NOTICES.md",
        "sha256": _sha256(notices),
        "cargo_about": "0.9.0",
        "about_config_sha256": _sha256(repo / "about.toml"),
        "template_sha256": _sha256(repo / "licenses/third-party-notices.hbs"),
        "script_sha256": _sha256(repo / "scripts/license-notices.sh"),
        "cargo_lock_sha256": _sha256(repo / "Cargo.lock"),
    }
    _write_json(raw / "license-notices.json", license_notice)

    sbom_entries = []
    for project_id, (ecosystem, _, lockfile) in PROJECTS.items():
        lock_sha = _sha256(repo / lockfile)
        document = _sbom(project_id, ecosystem, components[project_id])
        if project_id == "rust-workspace":
            member_path = sboms / "rust-workspace" / "fixture.cdx.json"
            tool_document = _sbom(project_id, ecosystem, components[project_id][:1])
            tool_document["metadata"]["component"] = {
                "type": "application",
                "name": "fixture-rust-target",
                "version": "0.1.0",
            }
            _write_json(member_path, tool_document)
            supplement_path = sboms / "rust-workspace" / "cargo-lock-union.cdx.json"
            _write_json(supplement_path, _cargo_lock_union_sbom(components[project_id]))
            rust_manifest_path = sboms / "rust-workspace.sbom-manifest.json"
            _write_json(
                rust_manifest_path,
                {
                    "schema": "ferric.rust-sbom-manifest",
                    "version": 1,
                    "project_id": project_id,
                    "candidate_sha": CANDIDATE_SHA,
                    "lockfile_sha256": lock_sha,
                    "members": [
                        {
                            "kind": "cargo-cyclonedx",
                            "path": "rust-workspace/fixture.cdx.json",
                            "sha256": _sha256(member_path),
                        },
                        {
                            "kind": "cargo-lock-union",
                            "path": "rust-workspace/cargo-lock-union.cdx.json",
                            "sha256": _sha256(supplement_path),
                        },
                    ],
                },
            )
            sbom_path = rust_manifest_path
            relative = "rust-workspace.sbom-manifest.json"
        else:
            sbom_path = sboms / f"{project_id}.cdx.json"
            _write_json(sbom_path, document)
            relative = f"{project_id}.cdx.json"
        sbom_entries.append(
            {
                "project_id": project_id,
                "path": relative,
                "command": _sbom_command(ecosystem),
                "environment": _sbom_environment(ecosystem),
                "working_directory": _working_directory(project_id),
                "exit_code": 0,
                "lockfile_sha256": lock_sha,
                "normalization": "canonical-json-v1",
                "source_date_epoch": 1_786_233_600,
                "sha256": _sha256(sbom_path),
            }
        )

    project_policy = {project["project_id"]: project for project in policy["projects"]}
    inputs = []
    for project_id, (ecosystem, manifest, lockfile) in PROJECTS.items():
        inputs.append(
            {
                "project_id": project_id,
                "ecosystem": ecosystem,
                "manifest": manifest,
                "manifest_sha256": _sha256(repo / manifest),
                "lockfile": lockfile,
                "sha256": _sha256(repo / lockfile),
            }
        )
    input_hashes = {item["project_id"]: item["sha256"] for item in inputs}
    scans = []
    for (project_id, scanner), filename in SCAN_FILES.items():
        record = {
            "project_id": project_id,
            "scanner": scanner,
            "report": filename,
            "command": _scan_command(scanner),
            "environment": _scan_environment(scanner),
            "working_directory": _working_directory(project_id),
            "exit_code": 0,
            "exit_classification": "clean",
            "raw_sha256": _sha256(raw / filename),
            "lockfile_sha256": input_hashes[project_id],
            "target_scope": "all",
        }
        if scanner == "cargo-audit":
            record["advisory_database"] = {
                "url": "https://github.com/RustSec/advisory-db",
                "commit": "1" * 40,
                "fresh_fetch": True,
            }
        scans.append(record)
    scan_manifest = {
        "schema": "ferric.dependency-scan-manifest",
        "version": 1,
        "candidate_sha": CANDIDATE_SHA,
        "evaluated_on": TODAY,
        "source_date_epoch": 1_786_233_600,
        "target_scope": "all",
        "policy_sha256": _sha256(repo / "dependency-policy.json"),
        "deny_config_sha256": _sha256(repo / "deny.toml"),
        "cargo_config_sha256": _sha256(repo / ".cargo/config.toml"),
        "cargo_workspace_manifests_sha256": CARGO_WORKSPACE_MANIFESTS_SHA256,
        "tool_versions": TOOL_PINS,
        "inputs": inputs,
        "scans": scans,
        "sboms": sbom_entries,
        "license_notice": {
            "command": ["./scripts/license-notices.sh", "check"],
            "working_directory": ".",
            "path": "license-notices.json",
            "exit_code": 0,
            "status": "pass",
            "sha256": _sha256(raw / "license-notices.json"),
            "about_config_sha256": _sha256(repo / "about.toml"),
            "template_sha256": _sha256(repo / "licenses/third-party-notices.hbs"),
            "script_sha256": _sha256(repo / "scripts/license-notices.sh"),
            "cargo_lock_sha256": _sha256(repo / "Cargo.lock"),
        },
        "normalization": {
            "schema": "canonical-json-v1",
            "uv_preview_feature": "sbom-export",
            "uv_removed_fields": ["serialNumber", "metadata.timestamp"],
            "uv_checksum_enrichment": "all-registry-artifact-sha256-from-uv-lock",
        },
        "raw_files": [],
    }
    _write_json(raw / "scan-manifest.json", scan_manifest)
    _refresh_raw_manifest(evidence)
    assert set(project_policy) == set(PROJECTS)
    return repo, evidence


def _run_evaluate(repo: pathlib.Path, evidence: pathlib.Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [
            sys.executable,
            str(EVALUATOR),
            "evaluate",
            "--policy",
            str(repo / "dependency-policy.json"),
            "--reports-dir",
            str(evidence / "raw"),
            "--sbom-dir",
            str(evidence / "sboms"),
            "--candidate-sha",
            CANDIDATE_SHA,
            "--today",
            TODAY,
            "--output-dir",
            str(evidence),
        ],
        cwd=repo,
        text=True,
        capture_output=True,
        check=False,
    )


def _run_orchestrator(
    repo: pathlib.Path, output_dir: pathlib.Path
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [
            sys.executable,
            str(EVALUATOR),
            "run",
            "--policy",
            str(repo / "dependency-policy.json"),
            "--candidate-sha",
            CANDIDATE_SHA,
            "--today",
            TODAY,
            "--output-dir",
            str(output_dir),
        ],
        cwd=repo,
        text=True,
        capture_output=True,
        check=False,
    )


def _report(evidence: pathlib.Path) -> dict[str, Any]:
    return json.loads((evidence / "dependency-policy-report.json").read_text())


def _replace_raw(evidence: pathlib.Path, filename: str, value: object) -> None:
    _write_json(evidence / "raw" / filename, value)
    _refresh_raw_manifest(evidence)


def _set_scan_disposition(
    evidence: pathlib.Path,
    project_id: str,
    scanner: str,
    *,
    exit_code: int,
    classification: str,
) -> None:
    manifest_path = evidence / "raw" / "scan-manifest.json"
    manifest = json.loads(manifest_path.read_text())
    scan = next(
        item
        for item in manifest["scans"]
        if item["project_id"] == project_id and item["scanner"] == scanner
    )
    scan["exit_code"] = exit_code
    scan["exit_classification"] = classification
    _write_json(manifest_path, manifest)


def _set_policy_exceptions(
    repo: pathlib.Path,
    evidence: pathlib.Path,
    exceptions: list[dict[str, Any]],
) -> None:
    policy_path = repo / "dependency-policy.json"
    policy = json.loads(policy_path.read_text())
    policy["exceptions"] = copy.deepcopy(exceptions)
    _write_json(policy_path, policy)
    manifest_path = evidence / "raw" / "scan-manifest.json"
    manifest = json.loads(manifest_path.read_text())
    manifest["policy_sha256"] = _sha256(policy_path)
    _write_json(manifest_path, manifest)


def _python_package_pytest_batches(
    repo: pathlib.Path,
    *,
    vulnerable_versions: set[str],
) -> dict[str, Any]:
    lock = repo / PROJECTS["python-package"][2]
    components = _uv_components(lock.read_bytes())
    common = [component for component in components if component["name"] != "pytest"]
    reports = []
    for version in ("8.4.2", "9.0.2"):
        dependencies = [
            {"name": component["name"], "version": component["version"], "vulns": []}
            for component in common
        ]
        dependencies.append(
            {
                "name": "pytest",
                "version": version,
                "vulns": (
                    [
                        {
                            "id": "GHSA-6W46-J5RX-G56G",
                            "fix_versions": [],
                            "aliases": [],
                            "description": "pytest temporary-directory cleanup advisory",
                        }
                    ]
                    if version in vulnerable_versions
                    else []
                ),
            }
        )
        reports.append({"dependencies": dependencies, "fixes": []})
    return _pip_envelope(*reports)


def _synthetic_pypi_alias_batches(alias_rows: list[list[str]]) -> dict[str, Any]:
    reports = []
    for aliases in alias_rows:
        reports.append(
            {
                "dependencies": [
                    {
                        "name": "python-package-dependency",
                        "version": "1.0.0",
                        "vulns": [
                            {
                                "id": "PYSEC-2099-0001",
                                "fix_versions": [],
                                "aliases": aliases,
                                "description": "duplicate marker-batch advisory",
                            }
                        ],
                    }
                ],
                "fixes": [],
            }
        )
    return _pip_envelope(*reports)


def _assert_failed(
    result: subprocess.CompletedProcess[str], evidence: pathlib.Path
) -> dict[str, Any]:
    assert result.returncode != 0, result.stdout + result.stderr
    report = _report(evidence)
    assert report["verdict"] == "fail"
    assert (
        report["errors"]
        or any(finding.get("status") == "blocked" for finding in report["findings"])
        or any(exception.get("status") != "active" for exception in report["exceptions"])
    )
    return report


def test_offline_evaluate_passes_complete_exact_seven_project_evidence(tmp_path):
    repo, evidence = _build_bundle(tmp_path)
    result = _run_evaluate(repo, evidence)
    assert result.returncode == 0, result.stdout + result.stderr
    report = _report(evidence)
    assert report.keys() >= {
        "schema",
        "version",
        "candidate_sha",
        "evaluated_on",
        "verdict",
        "tool_versions",
        "inputs",
        "findings",
        "exceptions",
        "sboms",
        "license_notice",
        "errors",
    }
    assert report["schema"] == "ferric.dependency-policy-report"
    assert report["version"] == 1
    assert report["candidate_sha"] == CANDIDATE_SHA
    assert report["evaluated_on"] == TODAY
    assert report["verdict"] == "pass"
    assert report["errors"] == []
    assert report["findings"] == []
    assert report["exceptions"] == []
    assert len(report["inputs"]) == len(report["sboms"]) == 7
    assert all(
        item.keys()
        >= {
            "project_id",
            "ecosystem",
            "manifest",
            "manifest_sha256",
            "lockfile",
            "sha256",
        }
        for item in report["inputs"]
    )
    assert all(
        item.keys()
        >= {
            "project_id",
            "path",
            "sha256",
            "spec_version",
            "lockfile_sha256",
            "component_count",
            "verified",
        }
        and item["verified"] is True
        for item in report["sboms"]
    )
    assert report["license_notice"].keys() >= {"status", "path", "sha256"}
    assert report["license_notice"]["status"] == "pass"
    assert (evidence / "dependency-policy-report.md").is_file()


def test_live_like_python_package_marker_fork_exactly_excepts_both_pytest_versions(tmp_path):
    repo, evidence = _build_bundle(tmp_path, current_python_package_lock=True)
    committed = json.loads(COMMITTED_POLICY.read_text())
    exception_ids = {
        "pypi-python-package-pytest-8-4-2-ghsa-6w46-j5rx-g56g",
        "pypi-python-package-pytest-ghsa-6w46-j5rx-g56g",
    }
    exceptions = [
        exception
        for exception in committed["exceptions"]
        if exception["exception_id"] in exception_ids
    ]
    assert {exception["exception_id"] for exception in exceptions} == exception_ids
    _set_policy_exceptions(repo, evidence, exceptions)
    _replace_raw(
        evidence,
        "python-package.pip-audit.json",
        _python_package_pytest_batches(
            repo,
            vulnerable_versions={"8.4.2", "9.0.2"},
        ),
    )
    _set_scan_disposition(
        evidence,
        "python-package",
        "pip-audit",
        exit_code=1,
        classification="findings",
    )

    result = _run_evaluate(repo, evidence)

    assert result.returncode == 0, result.stdout + result.stderr
    report = _report(evidence)
    pytest_findings = [
        finding
        for finding in report["findings"]
        if finding["project_id"] == "python-package" and finding["package"]["name"] == "pytest"
    ]
    assert {
        (finding["package"]["version"], finding["status"], finding["exception_id"])
        for finding in pytest_findings
    } == {
        (
            "8.4.2",
            "excepted",
            "pypi-python-package-pytest-8-4-2-ghsa-6w46-j5rx-g56g",
        ),
        ("9.0.2", "excepted", "pypi-python-package-pytest-ghsa-6w46-j5rx-g56g"),
    }
    assert {item["status"] for item in report["exceptions"]} == {"active"}
    assert all(item["matched_finding_count"] == 1 for item in report["exceptions"])


@pytest.mark.parametrize("mutation", ["unused", "version-drift"])
def test_python_package_pytest_marker_exception_unused_or_version_drift_fails(tmp_path, mutation):
    repo, evidence = _build_bundle(tmp_path, current_python_package_lock=True)
    committed = json.loads(COMMITTED_POLICY.read_text())
    exception_ids = {
        "pypi-python-package-pytest-8-4-2-ghsa-6w46-j5rx-g56g",
        "pypi-python-package-pytest-ghsa-6w46-j5rx-g56g",
    }
    exceptions = [
        copy.deepcopy(exception)
        for exception in committed["exceptions"]
        if exception["exception_id"] in exception_ids
    ]
    assert len(exceptions) == 2
    vulnerable_versions = {"9.0.2"} if mutation == "unused" else {"8.4.2", "9.0.2"}
    if mutation == "version-drift":
        legacy = next(
            exception for exception in exceptions if exception["package"]["version"] == "8.4.2"
        )
        legacy["package"]["version"] = "8.4.1"
    _set_policy_exceptions(repo, evidence, exceptions)
    _replace_raw(
        evidence,
        "python-package.pip-audit.json",
        _python_package_pytest_batches(repo, vulnerable_versions=vulnerable_versions),
    )
    _set_scan_disposition(
        evidence,
        "python-package",
        "pip-audit",
        exit_code=1,
        classification="findings",
    )

    report = _assert_failed(_run_evaluate(repo, evidence), evidence)

    assert any(item["status"] == "unused" for item in report["exceptions"])
    if mutation == "version-drift":
        assert any(
            finding["package"] == {"name": "pytest", "version": "8.4.2"}
            and finding["status"] == "blocked"
            for finding in report["findings"]
        )


def test_duplicate_pypi_batch_alias_order_produces_identical_canonical_reports(tmp_path):
    reports = []
    alias_orders = [
        [
            ["GHSA-7777-8888-9999", "CVE-2099-0001", "GHSA-7777-8888-9999"],
            ["CVE-2099-0001", "GHSA-7777-8888-9999"],
        ],
        [
            ["CVE-2099-0001", "GHSA-7777-8888-9999", "CVE-2099-0001"],
            ["GHSA-7777-8888-9999", "CVE-2099-0001"],
        ],
    ]
    for index, aliases in enumerate(alias_orders):
        repo, evidence = _build_bundle(tmp_path / str(index))
        _replace_raw(
            evidence,
            "python-package.pip-audit.json",
            _synthetic_pypi_alias_batches(aliases),
        )
        _set_scan_disposition(
            evidence,
            "python-package",
            "pip-audit",
            exit_code=1,
            classification="findings",
        )
        report = _assert_failed(_run_evaluate(repo, evidence), evidence)
        assert report["errors"] == []
        reports.append(report)

    assert _json_bytes(reports[0]) == _json_bytes(reports[1])
    matching = [
        finding for finding in reports[0]["findings"] if finding["project_id"] == "python-package"
    ]
    assert len(matching) == 1
    assert matching[0]["aliases"] == ["CVE-2099-0001", "GHSA-7777-8888-9999"]


def test_write_report_atomically_replaces_the_authoritative_json_last(tmp_path, monkeypatch):
    module = _load_evaluator()
    output = tmp_path / "evidence"

    def report(verdict: str) -> dict[str, Any]:
        return {
            "schema": "ferric.dependency-policy-report",
            "version": 1,
            "candidate_sha": CANDIDATE_SHA,
            "evaluated_on": TODAY,
            "tool_versions": {},
            "inputs": [],
            "findings": [],
            "exceptions": [],
            "sboms": [],
            "license_notice": {"status": "fail" if verdict == "fail" else "pass"},
            "errors": (
                [{"code": "gate-not-completed", "message": "still running", "project_id": None}]
                if verdict == "fail"
                else []
            ),
            "verdict": verdict,
        }

    module._write_report(output, report("fail"))
    real_replace = module.os.replace
    observed: list[tuple[str, str]] = []
    replacement_order: list[str] = []

    def observe_replace(source, destination):
        destination_path = pathlib.Path(destination)
        replacement_order.append(destination_path.name)
        if destination_path.name == "dependency-policy-report.json":
            current = json.loads(destination_path.read_text())
            replacement = json.loads(pathlib.Path(source).read_text())
            observed.append((current["verdict"], replacement["verdict"]))
        real_replace(source, destination)

    monkeypatch.setattr(module.os, "replace", observe_replace)
    module._write_report(output, report("pass"))

    assert observed == [("fail", "pass")]
    assert replacement_order[-2:] == [
        "dependency-policy-report.md",
        "dependency-policy-report.json",
    ]
    assert _report(output)["verdict"] == "pass"
    assert not list(output.glob(".*.tmp"))


@pytest.mark.parametrize("target", ["dot", "repo-root", "parent", "arbitrary-existing"])
def test_run_rejects_unscoped_output_directories_without_deleting_them(tmp_path, target):
    repo, _ = _build_bundle(tmp_path / target)
    if target == "dot":
        output = pathlib.Path(".")
        protected = repo
    elif target == "repo-root":
        output = repo
        protected = repo
    elif target == "parent":
        output = repo.parent
        protected = repo.parent
    else:
        output = repo / "arbitrary-existing"
        output.mkdir()
        protected = output
    sentinel = protected / "dependency-policy-output-safety-sentinel.txt"
    sentinel.write_text("must survive\n", encoding="utf-8")

    result = _run_orchestrator(repo, output)

    assert result.returncode != 0
    assert "dependency-policy-evidence" in (result.stdout + result.stderr)
    assert sentinel.read_text(encoding="utf-8") == "must survive\n"


def test_run_failure_artifact_refuses_exact_evidence_path_symlink_without_writing_through(
    tmp_path,
):
    repo, _ = _build_bundle(tmp_path)
    outside = tmp_path / "outside"
    outside.mkdir()
    sentinel = outside / "dependency-policy-output-safety-sentinel.txt"
    sentinel.write_text("must survive alone\n", encoding="utf-8")
    output = repo / "dependency-policy-evidence"
    output.symlink_to(outside, target_is_directory=True)

    result = _run_orchestrator(repo, output)

    assert result.returncode != 0
    assert sentinel.read_text(encoding="utf-8") == "must survive alone\n"
    assert list(outside.iterdir()) == [sentinel]


@pytest.mark.parametrize("child_name", ["raw", "sboms"])
def test_run_failure_artifact_refuses_child_directory_symlinks_without_writing_through(
    tmp_path, child_name
):
    repo, _ = _build_bundle(tmp_path)
    output = repo / "dependency-policy-evidence"
    output.mkdir()
    child = output / child_name
    if child.is_dir():
        shutil.rmtree(child)
    outside = tmp_path / f"outside-{child_name}"
    outside.mkdir()
    sentinel = outside / "dependency-policy-output-safety-sentinel.txt"
    sentinel.write_text("must survive alone\n", encoding="utf-8")
    child.symlink_to(outside, target_is_directory=True)

    result = _run_orchestrator(repo, output)

    assert result.returncode != 0
    assert sentinel.read_text(encoding="utf-8") == "must survive alone\n"
    assert list(outside.iterdir()) == [sentinel]


@pytest.mark.parametrize("target_scanner", ["cargo-audit", "npm-audit"])
def test_run_isolates_ambient_scanner_configuration_before_invocation(
    tmp_path, monkeypatch, target_scanner
):
    module = _load_evaluator()
    repo, _ = _build_bundle(tmp_path)
    output = repo / "dependency-policy-evidence"
    assert module._write_incomplete_run_evidence(
        policy_path=repo / "dependency-policy.json",
        candidate_sha=CANDIDATE_SHA,
        today=TODAY,
        output_dir=output,
        code="gate-not-completed",
        message="Dependency Policy aggregate has not completed",
    )
    monkeypatch.setenv("CARGO_HOME", "/host/cargo-home")
    monkeypatch.setenv("CARGO_AUDIT_IGNORE", "RUSTSEC-2099-0001")
    monkeypatch.setenv("RUSTSEC_ADVISORY_DB", "https://attacker.invalid/")
    monkeypatch.setenv("NPM_CONFIG_REGISTRY", "https://attacker.invalid/")
    observed: dict[str, Any] = {}

    class ReachedTarget(Exception):
        pass

    version_outputs = {
        ("rustc", "--version"): b"rustc 1.93.0",
        ("cargo-audit", "--version"): b"cargo-audit 0.22.2",
        ("cargo-deny", "--version"): b"cargo-deny 0.20.2",
        ("cargo-cyclonedx", "cyclonedx", "--version"): b"cargo-cyclonedx 0.5.9",
        ("cargo-about", "--version"): b"cargo-about 0.9.0",
        ("uv", "--no-config", "--no-cache", "--version"): b"uv 0.11.16",
        (
            "uvx",
            "--isolated",
            "--no-env-file",
            "--no-config",
            "--no-cache",
            "--default-index",
            "https://pypi.org/simple",
            "--from",
            "pip-audit==2.10.1",
            "pip-audit",
            "--version",
        ): b"pip-audit 2.10.1",
        ("node", "--version"): b"v22.18.0",
        ("npm", "--version"): b"11.12.1",
        ("cyclonedx-npm", "--version"): b"cyclonedx-npm 4.2.1",
    }

    def fake_run(command, *, cwd, env=None, clear_env_prefixes=()):
        argv = list(command)
        if argv == ["git", "rev-parse", "HEAD"]:
            return subprocess.CompletedProcess(argv, 0, CANDIDATE_SHA.encode(), b"")
        if argv == ["git", "show", "-s", "--format=%ct", CANDIDATE_SHA]:
            return subprocess.CompletedProcess(argv, 0, b"1786233600", b"")
        version = version_outputs.get(tuple(argv))
        if version is not None:
            return subprocess.CompletedProcess(argv, 0, version, b"")
        if argv == module.CARGO_AUDIT_ARGV:
            if target_scanner == "cargo-audit":
                initialized_report = _report(output)
                assert initialized_report["verdict"] == "fail"
                assert initialized_report["errors"][0]["code"] == "gate-not-completed"
                observed.update(
                    {
                        "env": dict(env or {}),
                        "clear": tuple(clear_env_prefixes),
                        "cargo_home_empty": (
                            pathlib.Path(str(env["CARGO_HOME"])).is_dir()
                            and not any(pathlib.Path(str(env["CARGO_HOME"])).iterdir())
                        ),
                        "initialized_report_present": True,
                    }
                )
                raise ReachedTarget
            return subprocess.CompletedProcess(
                argv,
                0,
                _json_bytes(_clean_cargo_audit()),
                b"",
            )
        if argv == module.CARGO_DENY_ARGV:
            return subprocess.CompletedProcess(argv, 0, b"", _json_bytes(_clean_cargo_deny()))
        if argv == module.NPM_AUDIT_ARGV:
            initialized_report = _report(output)
            assert initialized_report["verdict"] == "fail"
            assert initialized_report["errors"][0]["code"] == "gate-not-completed"
            observed.update(
                {
                    "argv": argv,
                    "env": dict(env or {}),
                    "clear": tuple(clear_env_prefixes),
                    "empty_configs": all(
                        pathlib.Path(str(env[key])).is_file()
                        and pathlib.Path(str(env[key])).read_bytes() == b""
                        for key in ("NPM_CONFIG_USERCONFIG", "NPM_CONFIG_GLOBALCONFIG")
                    ),
                    "initialized_report_present": True,
                }
            )
            raise ReachedTarget
        raise AssertionError(f"unexpected command before {target_scanner}: {argv}")

    monkeypatch.setattr(module, "_run_command", fake_run)
    monkeypatch.setattr(module, "_current_utc_date", lambda: TODAY)
    monkeypatch.setattr(
        module,
        "_cargo_audit_database_provenance",
        lambda *_args, **_kwargs: {
            "url": "https://github.com/RustSec/advisory-db",
            "commit": "1" * 40,
            "fresh_fetch": True,
        },
    )

    with pytest.raises(ReachedTarget):
        module.run_policy(
            policy_path=repo / "dependency-policy.json",
            candidate_sha=CANDIDATE_SHA,
            today=TODAY,
            output_dir=output,
        )

    assert observed["initialized_report_present"] is True
    preserved_report = _report(output)
    assert preserved_report["verdict"] == "fail"
    assert preserved_report["errors"][0]["code"] == "gate-not-completed"

    if target_scanner == "cargo-audit":
        assert observed["cargo_home_empty"] is True
        assert observed["clear"] == (
            "CARGO_HOME",
            "CARGO_AUDIT_",
            "RUSTSEC_",
            "CARGO_ALIAS_AUDIT",
            "CARGO_ALIAS_DENY",
            "CARGO_ALIAS_CYCLONEDX",
            "CARGO_ALIAS_ABOUT",
        )
        assert observed["env"]["CARGO_HOME"] != "/host/cargo-home"
    else:
        assert "--registry=https://registry.npmjs.org/" in observed["argv"]
        assert observed["clear"] == ("NPM_CONFIG_",)
        assert observed["empty_configs"] is True
        assert observed["env"]["NPM_CONFIG_CACHE"].startswith(
            str(pathlib.Path(observed["env"]["NPM_CONFIG_USERCONFIG"]).parent)
        )


def test_run_fails_closed_when_utc_day_rolls_over_before_final_evaluation(tmp_path, monkeypatch):
    module = _load_evaluator()
    repo, _ = _build_bundle(tmp_path)
    output = repo / "dependency-policy-evidence"
    dates = iter([TODAY, "2026-08-10"])
    monkeypatch.setattr(module, "_current_utc_date", lambda: next(dates))
    monkeypatch.setattr(module, "_assert_tool_versions", lambda _repo: dict(TOOL_PINS))
    monkeypatch.setattr(
        module,
        "_cargo_audit_database_provenance",
        lambda *_args, **_kwargs: {
            "url": "https://github.com/RustSec/advisory-db",
            "commit": "1" * 40,
            "fresh_fetch": True,
        },
    )

    def fake_run(command, *, cwd, env=None, clear_env_prefixes=()):
        argv = list(command)
        if argv == ["git", "rev-parse", "HEAD"]:
            return subprocess.CompletedProcess(argv, 0, CANDIDATE_SHA.encode(), b"")
        if argv == ["git", "show", "-s", "--format=%ct", CANDIDATE_SHA]:
            return subprocess.CompletedProcess(argv, 0, b"1786233600", b"")
        if argv == module.CARGO_AUDIT_ARGV:
            return subprocess.CompletedProcess(argv, 0, _json_bytes(_clean_cargo_audit()), b"")
        if argv == module.CARGO_DENY_ARGV:
            return subprocess.CompletedProcess(argv, 0, b"", _json_bytes(_clean_cargo_deny()))
        if argv == module.NPM_AUDIT_ARGV:
            return subprocess.CompletedProcess(argv, 0, _json_bytes(_clean_npm_audit()), b"")
        if argv == ["./scripts/license-notices.sh", "check"]:
            return subprocess.CompletedProcess(argv, 0, b"", b"")
        raise AssertionError(f"unexpected rollover fixture command: {argv}")

    def fake_pip_audit(*, project, cwd, lock_data, temporary_dir):
        parsed = _pip_envelope(_clean_pip_audit(module._uv_components(lock_data)))
        return (
            subprocess.CompletedProcess(module.PIP_AUDIT_ARGV, 0, _json_bytes(parsed), b""),
            parsed,
        )

    monkeypatch.setattr(module, "_run_command", fake_run)
    monkeypatch.setattr(module, "_run_pip_audit", fake_pip_audit)
    monkeypatch.setattr(module, "_generate_sboms", lambda **_kwargs: [])

    with pytest.raises(module.PolicyError, match=r"UTC date changed|rerun"):
        module.run_policy(
            policy_path=repo / "dependency-policy.json",
            candidate_sha=CANDIDATE_SHA,
            today=TODAY,
            output_dir=output,
        )

    assert not (output / "raw" / "scan-manifest.json").exists()
    report_path = output / "dependency-policy-report.json"
    assert not report_path.exists() or json.loads(report_path.read_text())["verdict"] != "pass"


def test_run_normalizes_workflow_relative_output_dir_before_changing_project_cwd(
    tmp_path, monkeypatch
):
    module = _load_evaluator()
    repo, _ = _build_bundle(tmp_path)
    monkeypatch.chdir(repo)
    monkeypatch.setattr(module, "_current_utc_date", lambda: TODAY)
    observed: dict[str, pathlib.Path] = {}

    class ReachedOutputPreparation(Exception):
        pass

    def fake_run(command, *, cwd, env=None, clear_env_prefixes=()):
        argv = list(command)
        if argv == ["git", "rev-parse", "HEAD"]:
            return subprocess.CompletedProcess(argv, 0, CANDIDATE_SHA.encode(), b"")
        if argv == ["git", "show", "-s", "--format=%ct", CANDIDATE_SHA]:
            return subprocess.CompletedProcess(argv, 0, b"1786233600", b"")
        raise AssertionError(f"unexpected command before output preparation: {argv}")

    def capture_output(output_dir):
        observed["output_dir"] = output_dir
        raise ReachedOutputPreparation

    monkeypatch.setattr(module, "_run_command", fake_run)
    monkeypatch.setattr(module, "_prepare_run_output", capture_output)

    with pytest.raises(ReachedOutputPreparation):
        module.run_policy(
            policy_path=repo / "dependency-policy.json",
            candidate_sha=CANDIDATE_SHA,
            today=TODAY,
            output_dir=pathlib.Path("dependency-policy-evidence"),
        )

    assert observed["output_dir"] == repo / "dependency-policy-evidence"
    assert observed["output_dir"].is_absolute()


def test_evaluate_remains_deterministic_for_explicit_date_independent_of_wall_clock(
    tmp_path, monkeypatch
):
    module = _load_evaluator()
    repo, evidence = _build_bundle(tmp_path)
    monkeypatch.setattr(module, "_current_utc_date", lambda: "2099-12-31")

    report = module.evaluate_evidence(
        policy_path=repo / "dependency-policy.json",
        reports_dir=evidence / "raw",
        sbom_dir=evidence / "sboms",
        candidate_sha=CANDIDATE_SHA,
        today=TODAY,
        output_dir=evidence,
    )

    assert report["evaluated_on"] == TODAY
    assert report["verdict"] == "pass"


def test_clean_host_only_pip_report_cannot_omit_marker_only_locked_package(tmp_path):
    repo, evidence = _build_bundle(
        tmp_path,
        marker_python_tools=True,
        host_only_python_tools=True,
    )
    report = _assert_failed(_run_evaluate(repo, evidence), evidence)
    assert "marker-only" in json.dumps(report["errors"]).lower()


def test_pip_report_must_cover_both_locked_versions_of_one_normalized_name(tmp_path):
    repo, evidence = _build_bundle(tmp_path, marker_python_tools=True)
    path = evidence / "raw" / "python-tools.pip-audit.json"
    pip_report = json.loads(path.read_text())
    batch = pip_report["reports"][0]
    batch["dependencies"] = [
        dependency
        for dependency in batch["dependencies"]
        if not (dependency["name"] == "duplicate-package" and dependency["version"] == "4.0.0")
    ]
    _replace_raw(evidence, path.name, pip_report)
    report = _assert_failed(_run_evaluate(repo, evidence), evidence)
    assert "duplicate-package" in json.dumps(report["errors"]).lower()
    assert "4.0.0" in json.dumps(report["errors"])


def test_pip_report_rejects_an_extra_identity_absent_from_uv_lock(tmp_path):
    repo, evidence = _build_bundle(tmp_path, marker_python_tools=True)
    path = evidence / "raw" / "python-tools.pip-audit.json"
    pip_report = json.loads(path.read_text())
    pip_report["reports"][0]["dependencies"].append(
        {"name": "not-locked", "version": "1.0.0", "vulns": []}
    )
    _replace_raw(evidence, path.name, pip_report)
    report = _assert_failed(_run_evaluate(repo, evidence), evidence)
    assert "absent from uv.lock" in json.dumps(report["errors"])


@pytest.mark.parametrize("failure", ["missing", "malformed", "error-payload", "unknown-schema"])
def test_missing_malformed_error_and_unknown_schema_reports_fail_closed(tmp_path, failure):
    repo, evidence = _build_bundle(tmp_path)
    path = evidence / "raw" / "node-package.npm-audit.json"
    if failure == "missing":
        path.unlink()
    elif failure == "malformed":
        path.write_text("{", encoding="utf-8")
        _refresh_raw_manifest(evidence)
    elif failure == "error-payload":
        _replace_raw(evidence, path.name, {"error": {"code": "EAUDIT", "summary": "offline"}})
    else:
        report = _clean_npm_audit()
        report["auditReportVersion"] = 99
        _replace_raw(evidence, path.name, report)
    _assert_failed(_run_evaluate(repo, evidence), evidence)


@pytest.mark.parametrize(
    "mutation",
    [
        "missing-metadata",
        "metadata-not-object",
        "missing-counts",
        "counts-not-object",
        "missing-bucket",
        "malformed-count",
        "negative-count",
        "total-not-sum",
        "object-count",
        "severity-histogram",
    ],
)
def test_npm_metadata_count_and_severity_inconsistencies_fail_through_evaluate_cli(
    tmp_path, mutation
):
    repo, evidence = _build_bundle(tmp_path)
    report = _clean_npm_audit()
    counts = report["metadata"]["vulnerabilities"]
    if mutation == "missing-metadata":
        report.pop("metadata")
    elif mutation == "metadata-not-object":
        report["metadata"] = []
    elif mutation == "missing-counts":
        report["metadata"].pop("vulnerabilities")
    elif mutation == "counts-not-object":
        report["metadata"]["vulnerabilities"] = []
    elif mutation == "missing-bucket":
        counts.pop("moderate")
    elif mutation == "malformed-count":
        counts["high"] = "1"
    elif mutation == "negative-count":
        counts["high"] = -1
        counts["total"] = -1
    elif mutation == "total-not-sum":
        counts["total"] = 1
    elif mutation == "object-count":
        counts.update({"high": 1, "total": 1})
    else:
        report = _npm_audit_with_vulnerability("node-package", severity="low")
        report["metadata"]["vulnerabilities"].update({"low": 0, "high": 1})
    _replace_raw(evidence, "node-package.npm-audit.json", report)
    _assert_failed(_run_evaluate(repo, evidence), evidence)


def test_valid_npm_metadata_counts_one_object_with_multiple_via_advisories(tmp_path):
    repo, evidence = _build_bundle(tmp_path)
    report = _npm_audit_with_vulnerability(
        "node-package",
        advisory_ids=("GHSA-abcd-1234-efgh", "GHSA-1111-2222-3333"),
    )
    _replace_raw(evidence, "node-package.npm-audit.json", report)
    manifest_path = evidence / "raw" / "scan-manifest.json"
    manifest = json.loads(manifest_path.read_text())
    scan = next(
        item
        for item in manifest["scans"]
        if item["project_id"] == "node-package" and item["scanner"] == "npm-audit"
    )
    scan["exit_code"] = 1
    scan["exit_classification"] = "findings"
    _write_json(manifest_path, manifest)

    normalized = _assert_failed(_run_evaluate(repo, evidence), evidence)

    assert normalized["errors"] == []
    assert len(normalized["findings"]) == 2
    assert {finding["status"] for finding in normalized["findings"]} == {"blocked"}


def test_duplicate_npm_identity_union_matches_one_reviewed_exception(tmp_path):
    module = _load_evaluator()
    repo, evidence = _build_bundle(tmp_path)
    project_id = "node-package"
    dependency = f"{project_id}-dependency"
    finding_id = "GHSA-ABCD-1234-EFGH"
    lock_path = repo / PROJECTS[project_id][2]
    lock = json.loads(lock_path.read_text())
    duplicate_path = f"node_modules/dev-parent/node_modules/{dependency}"
    duplicate = copy.deepcopy(lock["packages"][f"node_modules/{dependency}"])
    duplicate.update({"dev": True, "devOptional": True, "optional": True})
    lock["packages"][duplicate_path] = duplicate
    _write_json(lock_path, lock)

    report = _npm_audit_with_vulnerability(project_id, advisory_ids=(finding_id,))
    report["vulnerabilities"][dependency]["nodes"].append(duplicate_path)
    report["metadata"]["dependencies"] = module._npm_audit_dependency_counts(lock)
    _replace_raw(evidence, f"{project_id}.npm-audit.json", report)
    _refresh_lock_bindings(repo, evidence, project_id)

    sbom_path = evidence / "sboms" / f"{project_id}.cdx.json"
    _write_json(sbom_path, _sbom(project_id, "npm", _npm_components(_json_bytes(lock))))
    _refresh_sbom_manifest(evidence, project_id)

    policy_path = repo / "dependency-policy.json"
    policy = json.loads(policy_path.read_text())
    exception = _npm_exception(project_id, finding_id)
    exception["dependency_scopes"] = ["runtime", "development", "optional"]
    policy["exceptions"] = [exception]
    _write_json(policy_path, policy)
    manifest_path = evidence / "raw" / "scan-manifest.json"
    manifest = json.loads(manifest_path.read_text())
    manifest["policy_sha256"] = _sha256(policy_path)
    _write_json(manifest_path, manifest)
    _set_scan_disposition(
        evidence,
        project_id,
        "npm-audit",
        exit_code=1,
        classification="findings",
    )

    result = _run_evaluate(repo, evidence)

    assert result.returncode == 0, result.stdout + result.stderr
    evaluated = _report(evidence)
    assert evaluated["verdict"] == "pass"
    assert evaluated["errors"] == []
    assert len(evaluated["findings"]) == 1
    assert evaluated["findings"][0]["status"] == "excepted"
    assert evaluated["findings"][0]["dependency_scopes"] == [
        "runtime",
        "development",
        "optional",
    ]
    assert evaluated["findings"][0]["dev_only"] is False
    assert evaluated["exceptions"] == [
        {
            "exception_id": "fixture-npm-exception",
            "status": "active",
            "matched_finding_count": 1,
        }
    ]


def test_dev_optional_npm_node_cannot_match_stale_dev_only_exception(tmp_path):
    module = _load_evaluator()
    repo, evidence = _build_bundle(tmp_path)
    project_id = "node-package"
    dependency = f"{project_id}-dependency"
    finding_id = "GHSA-ABCD-1234-EFGH"
    lock_path = repo / PROJECTS[project_id][2]
    lock = json.loads(lock_path.read_text())
    lock["packages"][f"node_modules/{dependency}"]["devOptional"] = True
    _write_json(lock_path, lock)

    report = _npm_audit_with_vulnerability(project_id, advisory_ids=(finding_id,))
    report["metadata"]["dependencies"] = module._npm_audit_dependency_counts(lock)
    _replace_raw(evidence, f"{project_id}.npm-audit.json", report)
    _refresh_lock_bindings(repo, evidence, project_id)

    policy_path = repo / "dependency-policy.json"
    policy = json.loads(policy_path.read_text())
    stale_exception = _npm_exception(project_id, finding_id)
    stale_exception["dependency_scopes"] = ["development", "optional"]
    stale_exception["dev_only"] = True
    policy["exceptions"] = [stale_exception]
    _write_json(policy_path, policy)
    manifest_path = evidence / "raw" / "scan-manifest.json"
    manifest = json.loads(manifest_path.read_text())
    manifest["policy_sha256"] = _sha256(policy_path)
    _write_json(manifest_path, manifest)
    _set_scan_disposition(
        evidence,
        project_id,
        "npm-audit",
        exit_code=1,
        classification="findings",
    )

    evaluated = _assert_failed(_run_evaluate(repo, evidence), evidence)

    assert evaluated["errors"] == []
    assert len(evaluated["findings"]) == 1
    assert evaluated["findings"][0]["status"] == "blocked"
    assert evaluated["findings"][0]["dependency_scopes"] == [
        "runtime",
        "development",
        "optional",
    ]
    assert evaluated["findings"][0]["dev_only"] is False
    assert evaluated["exceptions"] == [
        {
            "exception_id": "fixture-npm-exception",
            "status": "unused",
            "matched_finding_count": 0,
        }
    ]


def test_node_addon_build_tool_cannot_match_stale_development_only_exception(tmp_path):
    module = _load_evaluator()
    repo, evidence = _build_bundle(tmp_path)
    project_id = "node-addon"
    dependency = f"{project_id}-dependency"
    finding_id = "GHSA-ABCD-1234-EFGH"
    manifest_path = repo / PROJECTS[project_id][1]
    lock_path = repo / PROJECTS[project_id][2]
    package_manifest = json.loads(manifest_path.read_text())
    package_lock = json.loads(lock_path.read_text())
    package_manifest["scripts"] = {"build": "fixture-build"}
    package_manifest["devDependencies"] = package_manifest.pop("dependencies")
    lock_root = package_lock["packages"][""]
    lock_root["devDependencies"] = lock_root.pop("dependencies")
    package_lock["packages"][f"node_modules/{dependency}"]["dev"] = True
    _write_json(manifest_path, package_manifest)
    _write_json(lock_path, package_lock)

    report = _npm_audit_with_vulnerability(project_id, advisory_ids=(finding_id,))
    report["metadata"]["dependencies"] = module._npm_audit_dependency_counts(package_lock)
    _replace_raw(evidence, f"{project_id}.npm-audit.json", report)
    _refresh_lock_bindings(repo, evidence, project_id)
    _set_scan_disposition(
        evidence,
        project_id,
        "npm-audit",
        exit_code=1,
        classification="findings",
    )

    stale_exception = _npm_exception(project_id, finding_id)
    stale_exception["dependency_scopes"] = ["development"]
    stale_exception["dev_only"] = True
    _set_policy_exceptions(repo, evidence, [stale_exception])

    evaluated = _assert_failed(_run_evaluate(repo, evidence), evidence)

    assert evaluated["errors"] == []
    assert len(evaluated["findings"]) == 1
    assert evaluated["findings"][0]["status"] == "blocked"
    assert evaluated["findings"][0]["dependency_scopes"] == ["build", "development"]
    assert evaluated["findings"][0]["dev_only"] is False
    assert evaluated["exceptions"] == [
        {
            "exception_id": "fixture-npm-exception",
            "status": "unused",
            "matched_finding_count": 0,
        }
    ]


def test_python_build_backend_cannot_match_stale_development_only_exception(tmp_path):
    repo, evidence = _build_bundle(tmp_path, current_python_package_lock=True)
    project_id = "python-package"
    manifest_path = repo / PROJECTS[project_id][1]
    manifest_path.write_bytes((REPO_ROOT / PROJECTS[project_id][1]).read_bytes())
    _refresh_lock_bindings(repo, evidence, project_id)

    report_path = evidence / "raw" / f"{project_id}.pip-audit.json"
    report = json.loads(report_path.read_text())
    dependency = next(
        item
        for batch in report["reports"]
        for item in batch["dependencies"]
        if item["name"] == "maturin"
    )
    dependency["vulns"] = [
        {
            "id": "PYSEC-2099-0001",
            "aliases": ["GHSA-ABCD-1234-EFGH"],
            "fix_versions": [],
            "description": "authenticated Python build-backend fixture",
        }
    ]
    _replace_raw(evidence, report_path.name, report)
    _set_scan_disposition(
        evidence,
        project_id,
        "pip-audit",
        exit_code=1,
        classification="findings",
    )

    stale_exception = {
        "exception_id": "fixture-python-build-exception",
        "ecosystem": "pypi",
        "project_id": project_id,
        "lockfile": PROJECTS[project_id][2],
        "kind": "vulnerability",
        "finding_id": "GHSA-ABCD-1234-EFGH",
        "package": {"name": "maturin", "version": "1.12.6"},
        "scanner_severity": "unknown",
        "dependency_scopes": ["development"],
        "dev_only": True,
        "affected_surfaces": ["fixture"],
        "reachability": "unknown",
        "evidence": [{"kind": "fixture", "reference": "offline PEP 517 adversary"}],
        "owner": "release-engineering",
        "tracking_issue": "https://github.com/plx/ferric-rules/issues/215",
        "rationale": "Prove a stale development-only waiver cannot mask build reachability.",
        "remediation": "Remove the fixture.",
        "issued_on": TODAY,
        "expires_on": "2026-09-08",
    }
    _set_policy_exceptions(repo, evidence, [stale_exception])

    evaluated = _assert_failed(_run_evaluate(repo, evidence), evidence)

    assert evaluated["errors"] == []
    assert len(evaluated["findings"]) == 1
    assert evaluated["findings"][0]["status"] == "blocked"
    assert evaluated["findings"][0]["dependency_scopes"] == ["build", "development"]
    assert evaluated["findings"][0]["dev_only"] is False
    assert evaluated["exceptions"] == [
        {
            "exception_id": "fixture-python-build-exception",
            "status": "unused",
            "matched_finding_count": 0,
        }
    ]


def test_cargo_report_carries_observed_graph_digest_and_matches_exact_exception(tmp_path):
    module = _load_evaluator()
    repo, evidence = _build_bundle(tmp_path)
    cargo_graph_sha256 = _cargo_graph_sha256(module, repo)
    cargo_audit = _cargo_audit_with_vulnerability()
    _replace_raw(evidence, "rust-workspace.cargo-audit.json", cargo_audit)
    _set_scan_disposition(
        evidence,
        "rust-workspace",
        "cargo-audit",
        exit_code=1,
        classification="findings",
    )
    _set_policy_exceptions(repo, evidence, [_cargo_exception(cargo_graph_sha256)])

    result = _run_evaluate(repo, evidence)

    assert result.returncode == 0, result.stdout + result.stderr
    report = _report(evidence)
    assert report["verdict"] == "pass"
    assert report["errors"] == []
    assert report["findings"] == [
        {
            "ecosystem": "cargo",
            "project_id": "rust-workspace",
            "lockfile": "Cargo.lock",
            "kind": "vulnerability",
            "finding_id": "RUSTSEC-2099-0100",
            "package": {"name": "fixture-build", "version": "2.0.0"},
            "scanner_severity": "high",
            "title": "cross-scanner reconciliation fixture",
            "cargo_graph_sha256": cargo_graph_sha256,
            "dependency_scopes": ["build"],
            "dev_only": False,
            "affected_surfaces": ["fixture-published-artifact"],
            "reachability": "unknown",
            "owner": "release-engineering",
            "tracking_issue": "https://github.com/plx/ferric-rules/issues/215",
            "status": "excepted",
            "exception_id": "fixture-cargo-exception",
        }
    ]
    assert report["exceptions"] == [
        {
            "exception_id": "fixture-cargo-exception",
            "status": "active",
            "matched_finding_count": 1,
        }
    ]


def test_cargo_exception_cannot_survive_lock_graph_drift(tmp_path):
    module = _load_evaluator()
    repo, evidence = _build_bundle(tmp_path)
    cargo_graph_sha256 = _cargo_graph_sha256(module, repo)
    cargo_audit = _cargo_audit_with_vulnerability()
    _replace_raw(evidence, "rust-workspace.cargo-audit.json", cargo_audit)
    _set_scan_disposition(
        evidence,
        "rust-workspace",
        "cargo-audit",
        exit_code=1,
        classification="findings",
    )
    _set_policy_exceptions(repo, evidence, [_cargo_exception(cargo_graph_sha256)])

    lock_path = repo / "Cargo.lock"
    lock_text = lock_path.read_text()
    assert ' "fixture-build",' in lock_text
    lock_text = lock_text.replace(
        ' "fixture-build",',
        ' "fixture-build",\n "fixture-parent",',
        1,
    )
    lock_text += (
        "\n[[package]]\n"
        'name = "fixture-parent"\n'
        'version = "1.0.0"\n'
        'dependencies = ["fixture-build"]\n'
    )
    lock_path.write_text(lock_text, encoding="utf-8")
    _refresh_cargo_lock_evidence(repo, evidence, cargo_audit)
    assert _cargo_graph_sha256(module, repo) != cargo_graph_sha256

    evaluated = _assert_failed(_run_evaluate(repo, evidence), evidence)

    assert evaluated["findings"] == []
    errors = json.dumps(evaluated["errors"])
    assert "cargo_graph_sha256" in errors
    assert "authenticated Cargo graph" in errors


def test_cargo_exception_cannot_survive_workspace_manifest_aggregate_drift(tmp_path):
    module = _load_evaluator()
    repo, evidence = _build_bundle(tmp_path)
    cargo_graph_sha256 = _cargo_graph_sha256(module, repo)
    cargo_audit = _cargo_audit_with_vulnerability()
    _replace_raw(evidence, "rust-workspace.cargo-audit.json", cargo_audit)
    _set_scan_disposition(
        evidence,
        "rust-workspace",
        "cargo-audit",
        exit_code=1,
        classification="findings",
    )
    _set_policy_exceptions(repo, evidence, [_cargo_exception(cargo_graph_sha256)])

    manifest_path = repo / "crates/ferric-rules-runtime/Cargo.toml"
    original = "bincode = { workspace = true, optional = true }"
    assert original in manifest_path.read_text()
    manifest_path.write_text(
        manifest_path.read_text().replace(original, "bincode = { workspace = true }"),
        encoding="utf-8",
    )

    evaluated = _assert_failed(_run_evaluate(repo, evidence), evidence)

    assert evaluated["findings"] == []
    errors = json.dumps(evaluated["errors"])
    assert "Cargo workspace manifest aggregate" in errors
    assert "reviewed contract" in errors


@pytest.mark.parametrize("project_id", ["node-addon", "python-package"])
def test_evaluator_rejects_stale_authenticated_build_manifest_bytes(tmp_path, project_id):
    repo, evidence = _build_bundle(tmp_path)
    path = repo / PROJECTS[project_id][1]
    path.write_bytes(path.read_bytes() + b"\n")

    evaluated = _assert_failed(_run_evaluate(repo, evidence), evidence)

    errors = json.dumps(evaluated["errors"]).lower()
    assert "manifest" in errors
    assert "sha256" in errors
    assert "mismatch" in errors


@pytest.mark.parametrize(
    "bad_node_kind",
    ["malformed", "nonstring", "unknown", "versionless"],
)
def test_one_valid_excepted_npm_node_cannot_mask_an_invalid_sibling_node(tmp_path, bad_node_kind):
    repo, evidence = _build_bundle(tmp_path)
    project_id = "node-package"
    finding_id = "GHSA-ABCD-1234-EFGH"
    report = _npm_audit_with_vulnerability(project_id, advisory_ids=(finding_id,))
    if bad_node_kind == "malformed":
        bad_node: Any = ""
    elif bad_node_kind == "nonstring":
        bad_node = 7
    elif bad_node_kind == "unknown":
        bad_node = "node_modules/not-in-lock"
    else:
        bad_node = "node_modules/versionless"
        lock_path = repo / PROJECTS[project_id][2]
        lock = json.loads(lock_path.read_text())
        lock["packages"][bad_node] = {"name": "versionless"}
        _write_json(lock_path, lock)
        _refresh_lock_bindings(repo, evidence, project_id)
    report["vulnerabilities"][f"{project_id}-dependency"]["nodes"].append(bad_node)
    _replace_raw(evidence, f"{project_id}.npm-audit.json", report)

    policy_path = repo / "dependency-policy.json"
    policy = json.loads(policy_path.read_text())
    policy["exceptions"] = [_npm_exception(project_id, finding_id)]
    _write_json(policy_path, policy)
    manifest_path = evidence / "raw" / "scan-manifest.json"
    manifest = json.loads(manifest_path.read_text())
    manifest["policy_sha256"] = _sha256(policy_path)
    _write_json(manifest_path, manifest)
    _set_scan_disposition(
        evidence,
        project_id,
        "npm-audit",
        exit_code=1,
        classification="findings",
    )

    evaluated = _assert_failed(_run_evaluate(repo, evidence), evidence)

    assert evaluated["errors"]
    assert any(
        token in json.dumps(evaluated["errors"]).lower()
        for token in ["node", "lock", "version", "string"]
    )


@pytest.mark.parametrize("mutation", ["cargo-list", "npm-via", "npm-nodes", "pip-vulns"])
def test_truncated_nested_scanner_results_fail_closed_through_evaluate_cli(tmp_path, mutation):
    repo, evidence = _build_bundle(tmp_path)
    if mutation == "cargo-list":
        filename = "rust-workspace.cargo-audit.json"
        report = _clean_cargo_audit()
        report["vulnerabilities"].pop("list")
    elif mutation.startswith("npm-"):
        filename = "node-package.npm-audit.json"
        dependency = "node-package-dependency"
        report = _clean_npm_audit()
        report["vulnerabilities"] = {
            dependency: {
                "name": dependency,
                "severity": "low",
                "isDirect": False,
                "via": [],
                "effects": [],
                "range": "<=1.0.0",
                "nodes": [f"node_modules/{dependency}"],
                "fixAvailable": False,
            }
        }
        report["vulnerabilities"][dependency].pop(mutation.removeprefix("npm-"))
    else:
        filename = "python-tools.pip-audit.json"
        report = json.loads((evidence / "raw" / filename).read_text())
        report["reports"][0]["dependencies"][0].pop("vulns")
    _replace_raw(evidence, filename, report)
    _assert_failed(_run_evaluate(repo, evidence), evidence)


@pytest.mark.parametrize("classification", ["operational-error", "clean"])
def test_scanner_operational_failure_blocks_even_if_misclassified_as_clean(
    tmp_path, classification
):
    repo, evidence = _build_bundle(tmp_path)
    manifest_path = evidence / "raw" / "scan-manifest.json"
    manifest = json.loads(manifest_path.read_text())
    scan = next(item for item in manifest["scans"] if item["scanner"] == "npm-audit")
    scan["exit_code"] = 2
    scan["exit_classification"] = classification
    _write_json(manifest_path, manifest)
    report = _assert_failed(_run_evaluate(repo, evidence), evidence)
    assert any(
        token in json.dumps(report["errors"]).lower()
        for token in ["operational", "exit", "classification"]
    )


@pytest.mark.parametrize(
    ("project_id", "scanner"),
    [
        ("rust-workspace", "cargo-audit"),
        ("rust-workspace", "cargo-deny"),
        ("node-package", "npm-audit"),
        ("python-tools", "pip-audit"),
    ],
)
def test_findings_exit_and_manifest_classification_cannot_describe_a_clean_report(
    tmp_path, project_id, scanner
):
    repo, evidence = _build_bundle(tmp_path)
    manifest_path = evidence / "raw" / "scan-manifest.json"
    manifest = json.loads(manifest_path.read_text())
    scan = next(
        item
        for item in manifest["scans"]
        if item["project_id"] == project_id and item["scanner"] == scanner
    )
    scan["exit_code"] = 1
    scan["exit_classification"] = "findings"
    _write_json(manifest_path, manifest)

    report = _assert_failed(_run_evaluate(repo, evidence), evidence)

    assert any(
        token in json.dumps(report["errors"]).lower()
        for token in ["classification", "finding", "clean", "exit"]
    )


def test_cargo_deny_license_bitset_exit_is_a_valid_findings_disposition(tmp_path):
    repo, evidence = _build_bundle(tmp_path)
    _replace_raw(evidence, "cargo-deny.json", _cargo_deny_license_finding())
    _set_scan_disposition(
        evidence,
        "rust-workspace",
        "cargo-deny",
        exit_code=4,
        classification="findings",
    )

    report = _assert_failed(_run_evaluate(repo, evidence), evidence)

    assert report["errors"] == []
    finding = next(item for item in report["findings"] if item["kind"] == "license")
    assert finding["status"] == "blocked"


def test_cargo_deny_exit_bitset_must_match_its_terminal_summary(tmp_path):
    repo, evidence = _build_bundle(tmp_path)
    _replace_raw(evidence, "cargo-deny.json", _cargo_deny_license_finding())
    _set_scan_disposition(
        evidence,
        "rust-workspace",
        "cargo-deny",
        exit_code=1,
        classification="findings",
    )

    report = _assert_failed(_run_evaluate(repo, evidence), evidence)

    assert "terminal summary" in json.dumps(report["errors"]).lower()


@pytest.mark.parametrize(
    "mutation",
    [
        "scanner-argv",
        "npm-registry",
        "pip-vulnerability-service",
        "scanner-environment",
        "advisory-database",
        "sbom-argv",
        "working-directory",
        "tool-version",
    ],
)
def test_scan_manifest_rejects_command_or_tool_pin_drift(tmp_path, mutation):
    repo, evidence = _build_bundle(tmp_path)
    manifest_path = evidence / "raw" / "scan-manifest.json"
    manifest = json.loads(manifest_path.read_text())
    if mutation == "scanner-argv":
        scan = next(item for item in manifest["scans"] if item["scanner"] == "npm-audit")
        scan["command"].append("--omit=dev")
    elif mutation == "npm-registry":
        scan = next(item for item in manifest["scans"] if item["scanner"] == "npm-audit")
        index = next(
            i for i, value in enumerate(scan["command"]) if value.startswith("--registry=")
        )
        scan["command"][index] = "--registry=https://attacker.invalid/"
    elif mutation == "pip-vulnerability-service":
        scan = next(item for item in manifest["scans"] if item["scanner"] == "pip-audit")
        index = scan["command"].index("--vulnerability-service")
        scan["command"][index + 1] = "osv"
    elif mutation == "scanner-environment":
        scan = next(item for item in manifest["scans"] if item["scanner"] == "cargo-audit")
        scan["environment"]["CARGO_HOME"] = "<host-cargo-home>"
    elif mutation == "advisory-database":
        scan = next(item for item in manifest["scans"] if item["scanner"] == "cargo-audit")
        scan["advisory_database"]["url"] = "https://attacker.invalid/advisory-db"
    elif mutation == "sbom-argv":
        sbom = next(item for item in manifest["sboms"] if item["project_id"] == "rust-workspace")
        flag_index = sbom["command"].index("all-cargo-targets")
        sbom["command"].pop(flag_index)
        sbom["command"].pop(flag_index - 1)
    elif mutation == "working-directory":
        scan = next(item for item in manifest["scans"] if item["project_id"] == "site")
        scan["working_directory"] = "."
    else:
        manifest["tool_versions"]["npm"] = "999.0.0"
    _write_json(manifest_path, manifest)
    _assert_failed(_run_evaluate(repo, evidence), evidence)


def test_license_failure_blocks_even_when_all_dependency_scans_are_clean(tmp_path):
    repo, evidence = _build_bundle(tmp_path)
    path = evidence / "raw" / "license-notices.json"
    notice = json.loads(path.read_text())
    notice["status"] = "fail"
    _replace_raw(evidence, path.name, notice)

    manifest_path = evidence / "raw" / "scan-manifest.json"
    manifest = json.loads(manifest_path.read_text())
    manifest["license_notice"]["status"] = "fail"
    _write_json(manifest_path, manifest)

    report = _assert_failed(_run_evaluate(repo, evidence), evidence)
    assert "license" in json.dumps(report["errors"]).lower()


def test_cargo_deny_advisory_absent_from_cargo_audit_cannot_vanish(tmp_path):
    repo, evidence = _build_bundle(tmp_path)
    finding_id = "RUSTSEC-2099-0100"
    _replace_raw(
        evidence,
        "cargo-deny.json",
        _cargo_deny_advisory(finding_id=finding_id),
    )
    _set_scan_disposition(
        evidence,
        "rust-workspace",
        "cargo-deny",
        exit_code=1,
        classification="findings",
    )

    report = _assert_failed(_run_evaluate(repo, evidence), evidence)

    assert finding_id in json.dumps({"errors": report["errors"], "findings": report["findings"]})


def test_exact_cargo_audit_and_deny_advisory_overlap_coalesces_without_disappearing(
    tmp_path,
):
    repo, evidence = _build_bundle(tmp_path)
    finding_id = "RUSTSEC-2099-0100"
    _replace_raw(
        evidence,
        "rust-workspace.cargo-audit.json",
        _cargo_audit_with_vulnerability(finding_id=finding_id),
    )
    _replace_raw(
        evidence,
        "cargo-deny.json",
        _cargo_deny_advisory(finding_id=finding_id),
    )
    for scanner in ("cargo-audit", "cargo-deny"):
        _set_scan_disposition(
            evidence,
            "rust-workspace",
            scanner,
            exit_code=1,
            classification="findings",
        )

    report = _assert_failed(_run_evaluate(repo, evidence), evidence)
    matching = [finding for finding in report["findings"] if finding["finding_id"] == finding_id]

    assert report["errors"] == []
    assert len(matching) == 1
    assert matching[0]["status"] == "blocked"


def test_new_unexcepted_finding_blocks_and_is_not_hidden_by_low_or_dev_context(tmp_path):
    repo, evidence = _build_bundle(tmp_path)
    project_id = "node-package"
    dependency = f"{project_id}-dependency"
    report = _clean_npm_audit()
    report["vulnerabilities"] = {
        dependency: {
            "name": dependency,
            "severity": "low",
            "isDirect": False,
            "via": [
                {
                    "source": 42,
                    "name": dependency,
                    "dependency": dependency,
                    "title": "new fixture finding",
                    "url": "https://github.com/advisories/GHSA-abcd-1234-efgh",
                    "severity": "low",
                    "range": "<=1.0.0",
                }
            ],
            "effects": [],
            "range": "<=1.0.0",
            "nodes": [f"node_modules/{dependency}"],
            "fixAvailable": False,
        }
    }
    report["metadata"]["vulnerabilities"].update({"low": 1, "total": 1})
    _replace_raw(evidence, f"{project_id}.npm-audit.json", report)
    manifest_path = evidence / "raw" / "scan-manifest.json"
    manifest = json.loads(manifest_path.read_text())
    scan = next(
        item
        for item in manifest["scans"]
        if item["project_id"] == project_id and item["scanner"] == "npm-audit"
    )
    scan["exit_code"] = 1
    scan["exit_classification"] = "findings"
    _write_json(manifest_path, manifest)
    normalized = _assert_failed(_run_evaluate(repo, evidence), evidence)
    assert normalized["errors"] == []
    assert normalized["findings"][0]["status"] == "blocked"
    assert normalized["findings"][0]["exception_id"] is None


def test_unused_reviewed_exception_is_a_failing_disposition(tmp_path):
    repo, evidence = _build_bundle(tmp_path)
    policy_path = repo / "dependency-policy.json"
    policy = json.loads(policy_path.read_text())
    committed = json.loads(COMMITTED_POLICY.read_text())
    policy["exceptions"] = [copy.deepcopy(committed["exceptions"][0])]
    policy["exceptions"][0]["cargo_graph_sha256"] = _cargo_graph_sha256(_load_evaluator(), repo)
    _write_json(policy_path, policy)
    manifest_path = evidence / "raw" / "scan-manifest.json"
    manifest = json.loads(manifest_path.read_text())
    manifest["policy_sha256"] = _sha256(policy_path)
    _write_json(manifest_path, manifest)
    report = _assert_failed(_run_evaluate(repo, evidence), evidence)
    assert report["errors"] == []
    assert report["exceptions"] == [
        {
            "exception_id": policy["exceptions"][0]["exception_id"],
            "status": "unused",
            "matched_finding_count": 0,
        }
    ]


@pytest.mark.parametrize("mutation", ["missing", "extra", "version", "checksum"])
def test_non_rust_sbom_component_drift_fails_even_when_manifest_hash_is_updated(tmp_path, mutation):
    repo, evidence = _build_bundle(tmp_path)
    project_id = "python-tools"
    path = evidence / "sboms" / f"{project_id}.cdx.json"
    sbom = json.loads(path.read_text())
    if mutation == "missing":
        sbom["components"].pop()
    elif mutation == "extra":
        extra = copy.deepcopy(sbom["components"][0])
        extra.update({"bom-ref": "pypi:extra", "name": "extra", "version": "9.0.0"})
        sbom["components"].append(extra)
    elif mutation == "version":
        sbom["components"][0]["version"] = "999.0.0"
    else:
        sbom["components"][0]["hashes"][0]["content"] = "f" * 64
    _write_json(path, sbom)
    _refresh_sbom_manifest(evidence, project_id)
    _assert_failed(_run_evaluate(repo, evidence), evidence)


@pytest.mark.parametrize("project_id", ["python-tools", "node-package"])
@pytest.mark.parametrize(
    "mutation",
    [
        "extra",
        "missing-name",
        "empty-name",
        "blank-name",
        "wrong-type-name",
        "missing-version",
        "empty-version",
        "blank-version",
        "wrong-type-version",
    ],
)
def test_non_rust_sbom_extra_or_malformed_component_never_disappears_from_parity(
    tmp_path, project_id, mutation
):
    repo, evidence = _build_bundle(tmp_path)
    path = evidence / "sboms" / f"{project_id}.cdx.json"
    sbom = json.loads(path.read_text())
    sbom["components"].append(_adversarial_sbom_component(mutation))
    _write_json(path, sbom)
    _refresh_sbom_manifest(evidence, project_id)

    report = _assert_failed(_run_evaluate(repo, evidence), evidence)

    assert report["errors"], "a rehashed malformed/extra component must be a policy error"


def test_rust_lock_union_supplement_is_minimal_exact_sorted_cargo_lock_inventory(tmp_path):
    repo, evidence = _build_bundle(tmp_path)
    sbom_dir = evidence / "sboms"
    manifest = json.loads((sbom_dir / "rust-workspace.sbom-manifest.json").read_text())
    assert [member["kind"] for member in manifest["members"]] == [
        "cargo-cyclonedx",
        "cargo-lock-union",
    ]
    assert all(set(member) == {"kind", "path", "sha256"} for member in manifest["members"])
    supplement = manifest["members"][-1]
    assert supplement["path"] == "rust-workspace/cargo-lock-union.cdx.json"
    supplement_path = sbom_dir / supplement["path"]
    expected = _cargo_lock_union_sbom(_cargo_components((repo / "Cargo.lock").read_bytes()))
    assert json.loads(supplement_path.read_text()) == expected
    assert supplement_path.read_bytes() == _json_bytes(expected)
    assert supplement["sha256"] == _sha256(supplement_path)

    tool_path = sbom_dir / manifest["members"][0]["path"]
    tool = json.loads(tool_path.read_text())
    assert 0 < len(tool["components"]) < len(expected["components"])
    assert tool["metadata"]["component"] == {
        "type": "application",
        "name": "fixture-rust-target",
        "version": "0.1.0",
    }
    result = _run_evaluate(repo, evidence)
    assert result.returncode == 0, result.stdout + result.stderr


@pytest.mark.parametrize(
    "mutation",
    ["missing-supplement", "duplicate-supplement", "wrong-kind", "missing-tool-member"],
)
def test_rust_manifest_requires_one_lock_union_supplement_and_at_least_one_tool_member(
    tmp_path, mutation
):
    repo, evidence = _build_bundle(tmp_path)
    sbom_dir = evidence / "sboms"
    manifest_path = sbom_dir / "rust-workspace.sbom-manifest.json"
    manifest = json.loads(manifest_path.read_text())
    tool = next(member for member in manifest["members"] if member["kind"] == "cargo-cyclonedx")
    supplement = next(
        member for member in manifest["members"] if member["kind"] == "cargo-lock-union"
    )
    if mutation == "missing-supplement":
        manifest["members"].remove(supplement)
        (sbom_dir / supplement["path"]).unlink()
    elif mutation == "duplicate-supplement":
        duplicate_path = sbom_dir / "rust-workspace" / "duplicate-lock-union.cdx.json"
        shutil.copyfile(sbom_dir / supplement["path"], duplicate_path)
        manifest["members"].append(
            {
                "kind": "cargo-lock-union",
                "path": "rust-workspace/duplicate-lock-union.cdx.json",
                "sha256": _sha256(duplicate_path),
            }
        )
    elif mutation == "wrong-kind":
        supplement["kind"] = "cargo-cyclonedx"
    else:
        manifest["members"].remove(tool)
        (sbom_dir / tool["path"]).unlink()
    _write_json(manifest_path, manifest)
    _refresh_sbom_manifest(evidence, "rust-workspace")

    report = _assert_failed(_run_evaluate(repo, evidence), evidence)

    error_text = json.dumps(report["errors"]).lower()
    assert any(
        token in error_text for token in ["cargo.lock union", "cargo-lock-union", "cargo-cyclonedx"]
    )


@pytest.mark.parametrize(
    "mutation",
    ["missing-component", "version", "checksum", "extra-component"],
)
def test_rehashed_rust_lock_union_supplement_rejects_exact_inventory_drift(tmp_path, mutation):
    repo, evidence = _build_bundle(tmp_path)
    sbom_dir = evidence / "sboms"
    manifest = json.loads((sbom_dir / "rust-workspace.sbom-manifest.json").read_text())
    supplement = next(
        member for member in manifest["members"] if member["kind"] == "cargo-lock-union"
    )
    path = sbom_dir / supplement["path"]
    document = json.loads(path.read_text())
    if mutation == "missing-component":
        document["components"].pop()
    elif mutation == "version":
        document["components"][0]["version"] = "999.0.0"
    elif mutation == "checksum":
        component = next(item for item in document["components"] if item["hashes"])
        component["hashes"][0]["content"] = "f" * 64
    else:
        document["components"].append(
            {
                "type": "library",
                "name": "fixture-invented",
                "version": "9.0.0",
            }
        )
    _write_json(path, document)
    _refresh_sbom_manifest(evidence, "rust-workspace")

    report = _assert_failed(_run_evaluate(repo, evidence), evidence)

    error_text = json.dumps(report["errors"]).lower()
    assert any(token in error_text for token in ["cargo.lock", "lock union", "supplement"])


def test_rust_lock_union_member_hash_binds_the_exact_supplement_bytes(tmp_path):
    repo, evidence = _build_bundle(tmp_path)
    sbom_dir = evidence / "sboms"
    manifest = json.loads((sbom_dir / "rust-workspace.sbom-manifest.json").read_text())
    supplement = next(
        member for member in manifest["members"] if member["kind"] == "cargo-lock-union"
    )
    path = sbom_dir / supplement["path"]
    path.write_bytes(path.read_bytes() + b"\n")

    report = _assert_failed(_run_evaluate(repo, evidence), evidence)

    assert "sha256" in json.dumps(report["errors"]).lower()


def test_rehashed_rust_lock_union_rejects_semantically_equal_noncanonical_bytes(tmp_path):
    repo, evidence = _build_bundle(tmp_path)
    sbom_dir = evidence / "sboms"
    manifest = json.loads((sbom_dir / "rust-workspace.sbom-manifest.json").read_text())
    supplement = next(
        member for member in manifest["members"] if member["kind"] == "cargo-lock-union"
    )
    path = sbom_dir / supplement["path"]
    document = json.loads(path.read_text())
    compact = json.dumps(document, separators=(",", ":"), sort_keys=False).encode()
    assert compact != _json_bytes(document)
    path.write_bytes(compact)
    _refresh_sbom_manifest(evidence, "rust-workspace")

    report = _assert_failed(_run_evaluate(repo, evidence), evidence)

    assert "canonical" in json.dumps(report["errors"]).lower()


def test_rehashed_rust_tool_member_extra_component_still_fails_exact_union_parity(tmp_path):
    repo, evidence = _build_bundle(tmp_path)
    sbom_dir = evidence / "sboms"
    manifest = json.loads((sbom_dir / "rust-workspace.sbom-manifest.json").read_text())
    tool = next(member for member in manifest["members"] if member["kind"] == "cargo-cyclonedx")
    path = sbom_dir / tool["path"]
    document = json.loads(path.read_text())
    document["components"].append(
        {
            "type": "library",
            "bom-ref": "cargo:invented@9.0.0",
            "name": "fixture-invented-tool-component",
            "version": "9.0.0",
            "hashes": [],
        }
    )
    _write_json(path, document)
    _refresh_sbom_manifest(evidence, "rust-workspace")

    report = _assert_failed(_run_evaluate(repo, evidence), evidence)

    assert "fixture-invented-tool-component" in json.dumps(report["errors"])


@pytest.mark.parametrize(
    "mutation",
    [
        "missing-metadata",
        "metadata-not-object",
        "missing-component",
        "component-not-object",
        "missing-type",
        "empty-name",
        "version-not-string",
    ],
)
def test_rust_cargo_cyclonedx_member_requires_exact_nonempty_target_subject_shape(
    tmp_path, mutation
):
    repo, evidence = _build_bundle(tmp_path)
    sbom_dir = evidence / "sboms"
    manifest = json.loads((sbom_dir / "rust-workspace.sbom-manifest.json").read_text())
    tool = next(member for member in manifest["members"] if member["kind"] == "cargo-cyclonedx")
    path = sbom_dir / tool["path"]
    document = json.loads(path.read_text())
    if mutation == "missing-metadata":
        document.pop("metadata")
    elif mutation == "metadata-not-object":
        document["metadata"] = []
    elif mutation == "missing-component":
        document["metadata"].pop("component")
    elif mutation == "component-not-object":
        document["metadata"]["component"] = []
    else:
        subject = document["metadata"]["component"]
        if mutation == "missing-type":
            subject.pop("type")
        elif mutation == "empty-name":
            subject["name"] = ""
        else:
            subject["version"] = 1
    _write_json(path, document)
    _refresh_sbom_manifest(evidence, "rust-workspace")

    report = _assert_failed(_run_evaluate(repo, evidence), evidence)

    error_text = json.dumps(report["errors"]).lower()
    assert "metadata" in error_text or "subject" in error_text


def test_rust_target_subject_cannot_hide_rehashed_off_lock_nested_components(tmp_path):
    repo, evidence = _build_bundle(tmp_path)
    sbom_dir = evidence / "sboms"
    manifest = json.loads((sbom_dir / "rust-workspace.sbom-manifest.json").read_text())
    tool = next(member for member in manifest["members"] if member["kind"] == "cargo-cyclonedx")
    path = sbom_dir / tool["path"]
    document = json.loads(path.read_text())
    document["metadata"]["component"]["components"] = [
        {
            "type": "library",
            "name": "fixture-hidden-off-lock-component",
            "version": "9.0.0",
        }
    ]
    _write_json(path, document)
    _refresh_sbom_manifest(evidence, "rust-workspace")

    report = _assert_failed(_run_evaluate(repo, evidence), evidence)

    error_text = json.dumps(report["errors"]).lower()
    assert "nested" in error_text or "components" in error_text


def test_rust_lock_union_cannot_omit_target_only_cargo_lock_component(tmp_path):
    repo, evidence = _build_bundle(tmp_path)
    sbom_dir = evidence / "sboms"
    manifest = json.loads((sbom_dir / "rust-workspace.sbom-manifest.json").read_text())
    supplement = next(
        member for member in manifest["members"] if member["kind"] == "cargo-lock-union"
    )
    member = sbom_dir / supplement["path"]
    sbom = json.loads(member.read_text())
    sbom["components"] = [
        component for component in sbom["components"] if component["name"] != "fixture-target-only"
    ]
    assert all(component["name"] != "fixture-target-only" for component in sbom["components"])
    _write_json(member, sbom)
    _refresh_sbom_manifest(evidence, "rust-workspace")
    report = _assert_failed(_run_evaluate(repo, evidence), evidence)
    assert "canonical recomputed document" in json.dumps(report["errors"])


@pytest.mark.parametrize(
    "mutation",
    [
        "extra",
        "missing-name",
        "empty-name",
        "blank-name",
        "wrong-type-name",
        "missing-version",
        "empty-version",
        "blank-version",
        "wrong-type-version",
    ],
)
def test_rust_member_nested_extra_or_malformed_component_fails_after_rehash(tmp_path, mutation):
    repo, evidence = _build_bundle(tmp_path)
    member = evidence / "sboms" / "rust-workspace" / "fixture.cdx.json"
    sbom = json.loads(member.read_text())
    sbom["components"][0]["components"] = [_adversarial_sbom_component(mutation)]
    _write_json(member, sbom)
    _refresh_sbom_manifest(evidence, "rust-workspace")

    report = _assert_failed(_run_evaluate(repo, evidence), evidence)

    assert report["errors"], "a rehashed nested Rust component must be a policy error"


@pytest.mark.parametrize("project_id", ["python-tools", "node-package", "rust-workspace"])
def test_rehashed_sbom_rejects_invented_hash_beyond_exact_lock_checksum_set(tmp_path, project_id):
    repo, evidence = _build_bundle(tmp_path)
    if project_id == "rust-workspace":
        path = evidence / "sboms" / "rust-workspace" / "fixture.cdx.json"
        target_name = "fixture-root"
    else:
        path = evidence / "sboms" / f"{project_id}.cdx.json"
        target_name = (
            "python-tools-dependency" if project_id == "python-tools" else "node-package-root"
        )
    sbom = json.loads(path.read_text())
    component = next(item for item in sbom["components"] if item["name"] == target_name)
    if project_id == "python-tools":
        assert component["hashes"], "fixture must retain the correct expected SHA-256"
    else:
        assert component["hashes"] == [], "fixture must cover an empty expected checksum set"
    component["hashes"].append({"alg": "SHA-512", "content": "f" * 128})
    _write_json(path, sbom)
    _refresh_sbom_manifest(evidence, project_id)

    report = _assert_failed(_run_evaluate(repo, evidence), evidence)

    assert any(
        token in json.dumps(report["errors"]).lower() for token in ["hash", "checksum", "sha-512"]
    )


@pytest.mark.parametrize("mutation", ["normalization", "sbom-epoch"])
def test_scan_manifest_requires_exact_reproducibility_declarations(tmp_path, mutation):
    repo, evidence = _build_bundle(tmp_path)
    path = evidence / "raw" / "scan-manifest.json"
    manifest = json.loads(path.read_text())
    if mutation == "normalization":
        manifest["normalization"]["uv_removed_fields"] = []
    else:
        manifest["sboms"][0]["source_date_epoch"] += 1
    _write_json(path, manifest)

    _assert_failed(_run_evaluate(repo, evidence), evidence)


@pytest.mark.parametrize(
    ("project_id", "volatile_field"),
    [
        ("python-tools", "serialNumber"),
        ("node-package", "metadata.timestamp"),
        ("rust-workspace", "serialNumber"),
    ],
)
def test_rehashed_stored_sbom_rejects_volatile_nonreproducible_fields(
    tmp_path, project_id, volatile_field
):
    repo, evidence = _build_bundle(tmp_path)
    if project_id == "rust-workspace":
        path = evidence / "sboms" / "rust-workspace" / "fixture.cdx.json"
    else:
        path = evidence / "sboms" / f"{project_id}.cdx.json"
    sbom = json.loads(path.read_text())
    if volatile_field == "serialNumber":
        sbom["serialNumber"] = "urn:uuid:00000000-0000-0000-0000-000000000000"
    else:
        sbom.setdefault("metadata", {})["timestamp"] = "2026-08-10T00:00:00Z"
    _write_json(path, sbom)
    _refresh_sbom_manifest(evidence, project_id)

    _assert_failed(_run_evaluate(repo, evidence), evidence)


def test_candidate_lock_and_raw_hash_drift_each_fail_closed(tmp_path):
    for mutation in ["candidate", "lock", "raw"]:
        case = tmp_path / mutation
        repo, evidence = _build_bundle(case)
        manifest_path = evidence / "raw" / "scan-manifest.json"
        manifest = json.loads(manifest_path.read_text())
        if mutation == "candidate":
            manifest["candidate_sha"] = "b" * 40
        elif mutation == "lock":
            manifest["inputs"][0]["sha256"] = "0" * 64
        else:
            manifest["raw_files"][0]["sha256"] = "0" * 64
        _write_json(manifest_path, manifest)
        _assert_failed(_run_evaluate(repo, evidence), evidence)


@pytest.mark.parametrize("mutation", ["candidate", "lock", "member-hash"])
def test_rust_sbom_manifest_candidate_lock_and_member_hash_drift_fail(tmp_path, mutation):
    repo, evidence = _build_bundle(tmp_path)
    path = evidence / "sboms" / "rust-workspace.sbom-manifest.json"
    manifest = json.loads(path.read_text())
    if mutation == "candidate":
        manifest["candidate_sha"] = "b" * 40
    elif mutation == "lock":
        manifest["lockfile_sha256"] = "0" * 64
    else:
        manifest["members"][0]["sha256"] = "0" * 64
    _write_json(path, manifest)
    scan_path = evidence / "raw" / "scan-manifest.json"
    scan_manifest = json.loads(scan_path.read_text())
    declaration = next(
        item for item in scan_manifest["sboms"] if item["project_id"] == "rust-workspace"
    )
    declaration["sha256"] = _sha256(path)
    _write_json(scan_path, scan_manifest)
    _assert_failed(_run_evaluate(repo, evidence), evidence)


def test_extra_unreferenced_raw_file_and_unsafe_manifest_path_fail_closed(tmp_path):
    for mutation in ["extra", "escape"]:
        case = tmp_path / mutation
        repo, evidence = _build_bundle(case)
        if mutation == "extra":
            _write_json(evidence / "raw" / "unreferenced.json", {})
        else:
            manifest_path = evidence / "raw" / "scan-manifest.json"
            manifest = json.loads(manifest_path.read_text())
            manifest["sboms"][0]["path"] = "../escape.cdx.json"
            _write_json(manifest_path, manifest)
        _assert_failed(_run_evaluate(repo, evidence), evidence)


@pytest.mark.parametrize(
    "mutation",
    ["missing", "extra", "bool", "negative", "total", "category"],
)
def test_npm_dependency_metadata_must_exactly_describe_the_package_lock(tmp_path, mutation):
    repo, evidence = _build_bundle(tmp_path)
    filename = "node-package.npm-audit.json"
    report = json.loads((evidence / "raw" / filename).read_text())
    dependencies = report["metadata"]["dependencies"]
    if mutation == "missing":
        report["metadata"].pop("dependencies")
    elif mutation == "extra":
        dependencies["workspace"] = 0
    elif mutation == "bool":
        dependencies["prod"] = True
    elif mutation == "negative":
        dependencies["dev"] = -1
    elif mutation == "total":
        dependencies["total"] += 1
    else:
        dependencies["prod"] -= 1
        dependencies["dev"] += 1
    _replace_raw(evidence, filename, report)
    _assert_failed(_run_evaluate(repo, evidence), evidence)


@pytest.mark.parametrize("section", ["top", "input", "raw", "sbom", "license"])
def test_scan_manifest_is_closed_at_every_evidence_declaration_level(tmp_path, section):
    repo, evidence = _build_bundle(tmp_path)
    path = evidence / "raw" / "scan-manifest.json"
    manifest = json.loads(path.read_text())
    target = {
        "top": manifest,
        "input": manifest["inputs"][0],
        "raw": manifest["raw_files"][0],
        "sbom": manifest["sboms"][0],
        "license": manifest["license_notice"],
    }[section]
    target["unreviewed_field"] = True
    _write_json(path, manifest)
    _assert_failed(_run_evaluate(repo, evidence), evidence)


def test_swapped_rehashed_sbom_paths_cannot_cross_project_boundaries(tmp_path):
    repo, evidence = _build_bundle(tmp_path)
    node_package = evidence / "sboms" / "node-package.cdx.json"
    node_addon = evidence / "sboms" / "node-addon.cdx.json"
    package_bytes, addon_bytes = node_package.read_bytes(), node_addon.read_bytes()
    node_package.write_bytes(addon_bytes)
    node_addon.write_bytes(package_bytes)
    manifest_path = evidence / "raw" / "scan-manifest.json"
    manifest = json.loads(manifest_path.read_text())
    declarations = {item["project_id"]: item for item in manifest["sboms"]}
    declarations["node-package"]["path"] = "node-addon.cdx.json"
    declarations["node-addon"]["path"] = "node-package.cdx.json"
    _write_json(manifest_path, manifest)
    _assert_failed(_run_evaluate(repo, evidence), evidence)


@pytest.mark.parametrize(
    "mutation",
    ["bare", "list", "missing", "extra", "version", "export", "audit", "empty"],
)
def test_pip_audit_evidence_requires_the_exact_pinned_batch_envelope(tmp_path, mutation):
    repo, evidence = _build_bundle(tmp_path)
    filename = "python-tools.pip-audit.json"
    envelope = json.loads((evidence / "raw" / filename).read_text())
    if mutation == "bare":
        value = envelope["reports"][0]
    elif mutation == "list":
        value = envelope["reports"]
    else:
        value = envelope
        if mutation == "missing":
            value.pop("export_command")
        elif mutation == "extra":
            value["unreviewed"] = True
        elif mutation == "version":
            value["version"] = 2
        elif mutation == "export":
            value["export_command"] = ["uv", "export"]
        elif mutation == "audit":
            value["audit_command"] = ["pip-audit", "--format", "json"]
        else:
            value["reports"] = []
    _replace_raw(evidence, filename, value)
    _assert_failed(_run_evaluate(repo, evidence), evidence)


@pytest.mark.parametrize(
    "mutation",
    [
        "missing-database",
        "missing-lockfile",
        "missing-settings",
        "dependency-count",
        "ignore",
        "target",
        "informational",
        "commit",
        "warning-type",
    ],
)
def test_cargo_audit_evidence_is_bound_to_full_lock_scope_and_database(tmp_path, mutation):
    repo, evidence = _build_bundle(tmp_path)
    filename = "rust-workspace.cargo-audit.json"
    report = json.loads((evidence / "raw" / filename).read_text())
    if mutation.startswith("missing-"):
        report.pop(mutation.removeprefix("missing-"))
    elif mutation == "dependency-count":
        report["lockfile"]["dependency-count"] += 1
    elif mutation == "ignore":
        report["settings"]["ignore"] = ["RUSTSEC-2099-0001"]
    elif mutation == "target":
        report["settings"]["target_os"] = ["linux"]
    elif mutation == "informational":
        report["settings"]["informational_warnings"] = ["unmaintained"]
    elif mutation == "commit":
        report["database"]["last-commit"] = "2" * 40
    else:
        report["warnings"] = {"unmaintained": {}}
    _replace_raw(evidence, filename, report)
    _assert_failed(_run_evaluate(repo, evidence), evidence)


@pytest.mark.parametrize("field", ["manifest", "lockfile"])
def test_evaluate_rejects_symlinked_declared_project_inputs(tmp_path, field):
    repo, evidence = _build_bundle(tmp_path)
    relative = PROJECTS["node-package"][1 if field == "manifest" else 2]
    path = repo / relative
    external = tmp_path / f"external-{path.name}"
    external.write_bytes(path.read_bytes())
    path.unlink()
    path.symlink_to(external)
    _assert_failed(_run_evaluate(repo, evidence), evidence)


def test_evaluate_rejects_a_symlinked_repository_policy_root(tmp_path):
    repo, evidence = _build_bundle(tmp_path)
    policy = repo / "dependency-policy.json"
    external = tmp_path / "external-policy.json"
    external.write_bytes(policy.read_bytes())
    policy.unlink()
    policy.symlink_to(external)
    result = _run_evaluate(repo, evidence)
    assert result.returncode != 0
    assert "symlink" in (result.stdout + result.stderr).lower()


@pytest.mark.parametrize("filename", ["config", "config.toml"])
@pytest.mark.parametrize("alias", ["audit", "deny", "cyclonedx", "about"])
def test_repository_cargo_configs_cannot_alias_dependency_tools(tmp_path, filename, alias):
    repo, evidence = _build_bundle(tmp_path)
    config = repo / ".cargo" / filename
    config.parent.mkdir(exist_ok=True)
    config.write_text(f'[alias]\n{alias} = "metadata --format-version 1"\n', encoding="utf-8")
    _assert_failed(_run_evaluate(repo, evidence), evidence)


@pytest.mark.parametrize(
    "variable",
    ["CARGO_ALIAS_AUDIT", "CARGO_ALIAS_DENY", "CARGO_ALIAS_CYCLONEDX", "CARGO_ALIAS_ABOUT"],
)
def test_ambient_cargo_aliases_are_rejected_even_with_direct_binaries(
    tmp_path, monkeypatch, variable
):
    module = _load_evaluator()
    repo, _ = _build_bundle(tmp_path)
    monkeypatch.setenv(variable, "metadata --format-version 1")
    with pytest.raises(module.PolicyError, match=r"ambient Cargo scanner aliases"):
        module.validate_repository_scanner_configs(repo)


def test_uv_and_uvx_invocations_ignore_repository_and_ambient_redirects(tmp_path, monkeypatch):
    module = _load_evaluator()
    cwd = tmp_path / "project"
    temporary_dir = tmp_path / "requirements"
    cwd.mkdir()
    temporary_dir.mkdir()
    (cwd / "uv.toml").write_text('default-index = "https://attacker.invalid/simple"\n')
    (cwd / ".env").write_text("UV_DEFAULT_INDEX=https://attacker.invalid/simple\n")
    monkeypatch.setenv("UV_CONFIG_FILE", str(cwd / "uv.toml"))
    monkeypatch.setenv("UV_DEFAULT_INDEX", "https://attacker.invalid/simple")
    monkeypatch.setenv("UV_FIND_LINKS", str(tmp_path / "attacker"))
    calls = []

    def fake_run(command, *, cwd, env=None, clear_env_prefixes=()):
        argv = list(command)
        calls.append((argv, tuple(clear_env_prefixes)))
        if argv == module.UV_EXPORT_ARGV:
            return subprocess.CompletedProcess(argv, 0, b"", b"")
        return subprocess.CompletedProcess(
            argv,
            0,
            _json_bytes({"dependencies": [], "fixes": []}),
            b"",
        )

    monkeypatch.setattr(module, "_run_command", fake_run)
    module._run_pip_audit(
        project={"project_id": "python-tools"},
        cwd=cwd,
        lock_data=_uv_lock("python-tools", "1"),
        temporary_dir=temporary_dir,
    )

    assert calls[0] == (module.UV_EXPORT_ARGV, ("UV_",))
    assert all(clear == ("UV_", "PIP_AUDIT_") for _, clear in calls[1:])
    assert all("--no-config" in command and "--no-cache" in command for command, _ in calls)
    assert "--isolated" in module.PIP_AUDIT_ARGV
    assert "--no-env-file" in module.PIP_AUDIT_ARGV
    assert "https://pypi.org/simple" in module.PIP_AUDIT_ARGV


@pytest.mark.parametrize(
    "mutation",
    [
        "dependency",
        "version",
        "peer-meta",
        "workspaces",
        "engines",
        "platform",
        "overrides",
        "lock-version",
    ],
)
def test_npm_manifest_and_lock_root_graph_must_remain_synchronized(tmp_path, mutation):
    repo, evidence = _build_bundle(tmp_path)
    manifest_path = repo / PROJECTS["node-package"][1]
    lock_path = repo / PROJECTS["node-package"][2]
    manifest = json.loads(manifest_path.read_text())
    if mutation == "dependency":
        manifest.setdefault("dependencies", {})["new-runtime-dependency"] = "1.0.0"
        _write_json(manifest_path, manifest)
    elif mutation == "version":
        manifest["version"] = "9.9.9"
        _write_json(manifest_path, manifest)
    elif mutation == "peer-meta":
        manifest["peerDependenciesMeta"] = {"fixture-peer": {"optional": True}}
        _write_json(manifest_path, manifest)
    elif mutation == "workspaces":
        manifest["workspaces"] = ["packages/*"]
        _write_json(manifest_path, manifest)
    elif mutation == "engines":
        manifest["engines"] = {"node": ">=99"}
        _write_json(manifest_path, manifest)
    elif mutation == "platform":
        manifest["os"] = ["linux"]
        _write_json(manifest_path, manifest)
    elif mutation == "overrides":
        manifest["overrides"] = {"fixture": "9.9.9"}
        _write_json(manifest_path, manifest)
    else:
        lock = json.loads(lock_path.read_text())
        lock["lockfileVersion"] = 2
        _write_json(lock_path, lock)
    _assert_failed(_run_evaluate(repo, evidence), evidence)


@pytest.mark.parametrize(
    ("relative", "source_project"),
    [
        ("extra/Cargo.lock", "rust-workspace"),
        ("extra/package-lock.json", "node-package"),
        ("extra/uv.lock", "python-tools"),
        ("npm-shrinkwrap.json", "node-package"),
    ],
)
def test_repository_lock_census_rejects_every_undeclared_or_precedence_lock(
    tmp_path, relative, source_project
):
    repo, evidence = _build_bundle(tmp_path)
    source = repo / PROJECTS[source_project][2]
    extra = repo / relative
    extra.parent.mkdir(parents=True, exist_ok=True)
    extra.write_bytes(source.read_bytes())
    _assert_failed(_run_evaluate(repo, evidence), evidence)


@pytest.mark.parametrize(
    ("relative", "contents"),
    [
        ("new-app/package.json", '{"name":"new-app","version":"1.0.0"}\n'),
        (
            "new-tool/pyproject.toml",
            '[project]\nname = "new-tool"\nversion = "1.0.0"\n',
        ),
    ],
)
def test_repository_surface_census_rejects_an_unlocked_dependency_manifest(
    tmp_path, relative, contents
):
    repo, evidence = _build_bundle(tmp_path)
    extra = repo / relative
    extra.parent.mkdir(parents=True, exist_ok=True)
    extra.write_text(contents, encoding="utf-8")
    _assert_failed(_run_evaluate(repo, evidence), evidence)


@pytest.mark.parametrize(
    ("relative", "replacement", "field"),
    [
        ("about.toml", 'accepted = ["MIT"]\n', "about_config_sha256"),
        ("licenses/third-party-notices.hbs", "# hollow notices\n", "template_sha256"),
        ("scripts/license-notices.sh", "#!/usr/bin/env bash\nexit 0\n", "script_sha256"),
    ],
)
def test_rehashed_license_contract_mutation_cannot_forge_passing_evidence(
    tmp_path, relative, replacement, field
):
    repo, evidence = _build_bundle(tmp_path)
    changed = repo / relative
    changed.write_text(replacement, encoding="utf-8")
    digest = _sha256(changed)
    notice_path = evidence / "raw" / "license-notices.json"
    notice = json.loads(notice_path.read_text())
    notice[field] = digest
    _replace_raw(evidence, notice_path.name, notice)
    manifest_path = evidence / "raw" / "scan-manifest.json"
    manifest = json.loads(manifest_path.read_text())
    manifest["license_notice"][field] = digest
    _write_json(manifest_path, manifest)

    _assert_failed(_run_evaluate(repo, evidence), evidence)


@pytest.mark.parametrize(
    "mutation",
    ["legacy-root", "nested", "source", "path-escape", "missing-target"],
)
def test_repository_cargo_config_cannot_redirect_or_expand_the_locked_graph(tmp_path, mutation):
    repo, evidence = _build_bundle(tmp_path)
    config_path = repo / ".cargo/config.toml"
    if mutation == "legacy-root":
        (repo / ".cargo/config").write_text("[alias]\naudit = 'metadata'\n", encoding="utf-8")
    elif mutation == "nested":
        nested = repo / "packages/ferric/.cargo/config.toml"
        nested.parent.mkdir(parents=True)
        nested.write_text("[source.crates-io]\nreplace-with = 'attacker'\n", encoding="utf-8")
    elif mutation == "source":
        config_path.write_text(
            config_path.read_text() + "\n[source.crates-io]\nreplace-with = 'attacker'\n",
            encoding="utf-8",
        )
    elif mutation == "path-escape":
        config_path.write_text(
            config_path.read_text().replace(
                'path = "crates/ferric-rules-core"',
                'path = "../outside/ferric-rules-core"',
            ),
            encoding="utf-8",
        )
    else:
        (repo / "crates/ferric-rules-core/Cargo.toml").unlink()
    _assert_failed(_run_evaluate(repo, evidence), evidence)


def test_scan_manifest_binds_the_exact_repository_cargo_config_hash(tmp_path):
    repo, evidence = _build_bundle(tmp_path)
    manifest_path = evidence / "raw/scan-manifest.json"
    manifest = json.loads(manifest_path.read_text())
    manifest["cargo_config_sha256"] = "0" * 64
    _write_json(manifest_path, manifest)
    _assert_failed(_run_evaluate(repo, evidence), evidence)


def test_cargo_workspace_manifest_aggregate_detects_dependency_scope_drift(tmp_path):
    repo, evidence = _build_bundle(tmp_path)
    manifest = repo / "crates/ferric-rules-runtime/Cargo.toml"
    original = "bincode = { workspace = true, optional = true }"
    assert original in manifest.read_text()
    manifest.write_text(
        manifest.read_text().replace(original, "bincode = { workspace = true }"),
        encoding="utf-8",
    )
    _assert_failed(_run_evaluate(repo, evidence), evidence)


def test_cargo_workspace_manifest_aggregate_requires_review_for_new_members(tmp_path):
    repo, evidence = _build_bundle(tmp_path)
    root_manifest = repo / "Cargo.toml"
    root_manifest.write_text(
        root_manifest.read_text().replace(
            '    "tools/bindings-conformance-adapter",',
            '    "tools/bindings-conformance-adapter",\n    "scratch",',
        ),
        encoding="utf-8",
    )
    member = repo / "scratch/Cargo.toml"
    member.parent.mkdir()
    member.write_text(
        '[package]\nname = "review-required"\nversion = "0.1.0"\n',
        encoding="utf-8",
    )
    _assert_failed(_run_evaluate(repo, evidence), evidence)


def test_scan_manifest_binds_reviewed_cargo_workspace_manifest_aggregate(tmp_path):
    repo, evidence = _build_bundle(tmp_path)
    manifest_path = evidence / "raw/scan-manifest.json"
    manifest = json.loads(manifest_path.read_text())
    manifest["cargo_workspace_manifests_sha256"] = "0" * 64
    _write_json(manifest_path, manifest)
    _assert_failed(_run_evaluate(repo, evidence), evidence)


@pytest.mark.parametrize("mutation", ["standalone", "symlink", "missing-member"])
def test_cargo_manifest_census_rejects_any_manifest_outside_the_root_workspace(tmp_path, mutation):
    repo, evidence = _build_bundle(tmp_path)
    if mutation == "standalone":
        manifest = repo / "scratch/Cargo.toml"
        manifest.parent.mkdir()
        manifest.write_text('[package]\nname = "unscanned"\nversion = "1.0.0"\n', encoding="utf-8")
    elif mutation == "symlink":
        outside = tmp_path / "outside-Cargo.toml"
        outside.write_text('[package]\nname = "unscanned"\nversion = "1.0.0"\n', encoding="utf-8")
        manifest = repo / "scratch/Cargo.toml"
        manifest.parent.mkdir()
        manifest.symlink_to(outside)
    else:
        (repo / "crates/ferric-rules-runtime/Cargo.toml").unlink()
    _assert_failed(_run_evaluate(repo, evidence), evidence)


@pytest.mark.parametrize("record", ["scan", "license"])
def test_unretained_stderr_digest_is_rejected_as_an_unknown_evidence_field(tmp_path, record):
    repo, evidence = _build_bundle(tmp_path)
    if record == "scan":
        manifest_path = evidence / "raw/scan-manifest.json"
        manifest = json.loads(manifest_path.read_text())
        manifest["scans"][0]["stderr_sha256"] = "0" * 64
        _write_json(manifest_path, manifest)
    else:
        notice_path = evidence / "raw/license-notices.json"
        notice = json.loads(notice_path.read_text())
        notice["stderr_sha256"] = "0" * 64
        _replace_raw(evidence, notice_path.name, notice)
    _assert_failed(_run_evaluate(repo, evidence), evidence)
