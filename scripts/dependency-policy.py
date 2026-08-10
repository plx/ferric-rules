#!/usr/bin/env python3
"""Fail-closed dependency policy evaluator and evidence orchestrator.

The evaluator is deliberately Python-stdlib-only.  Scanner output remains raw
evidence; this module normalizes it into one stable, reviewable gate result.

Evidence layout (all generated, never committed)::

    dependency-policy-evidence/
      dependency-policy-report.json
      dependency-policy-report.md
      dependency-policy.json
      deny.toml
      raw/
        scan-manifest.json
        tool-versions.json
        license-notices.json
        rust-workspace.cargo-audit.json
        cargo-deny.json
        node-package.npm-audit.json
        node-addon.npm-audit.json
        documentation.npm-audit.json
        site.npm-audit.json
        python-package.pip-audit.json
        python-tools.pip-audit.json
      sboms/
        rust-workspace.sbom-manifest.json
        rust-workspace/*.cdx.json
        <non-rust-project>.cdx.json

The authoritative directory names are ``dependency-policy-evidence/raw`` and
``dependency-policy-evidence/sboms``.
"""

from __future__ import annotations

import argparse
import base64
import datetime as dt
import hashlib
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib
from collections.abc import Iterable, Mapping, Sequence
from typing import Any

POLICY_SCHEMA = "ferric.dependency-policy"
REPORT_SCHEMA = "ferric.dependency-policy-report"
SCAN_MANIFEST_SCHEMA = "ferric.dependency-scan-manifest"
RUST_SBOM_MANIFEST_SCHEMA = "ferric.rust-sbom-manifest"
SCHEMA_VERSION = 1
RUST_SBOM_GENERATOR_KIND = "cargo-cyclonedx"
RUST_SBOM_LOCK_UNION_KIND = "cargo-lock-union"
RUST_SBOM_LOCK_UNION_PATH = "rust-workspace/cargo-lock-union.cdx.json"

EXPECTED_TOOL_PINS = {
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
PINNED_PYPI_REGISTRY = "https://pypi.org/simple"

EXPECTED_PROJECTS = {
    "rust-workspace": ("cargo", "Cargo.toml", "Cargo.lock"),
    "node-package": (
        "npm",
        "packages/ferric/package.json",
        "packages/ferric/package-lock.json",
    ),
    "node-addon": (
        "npm",
        "crates/ferric-rules-napi/package.json",
        "crates/ferric-rules-napi/package-lock.json",
    ),
    "documentation": (
        "npm",
        "documentation/package.json",
        "documentation/package-lock.json",
    ),
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

OWNERS = {
    "rust-runtime",
    "release-engineering",
    "python-bindings",
    "typescript-bindings",
    "documentation",
    "tooling",
}
KINDS = {
    "vulnerability",
    "unmaintained",
    "unsound",
    "notice",
    "yanked",
    "license",
    "source",
    "ban",
}
SEVERITIES = {"critical", "high", "moderate", "low", "info", "unknown"}
SCOPES = {"runtime", "build", "development", "optional"}
REACHABILITY = {"reachable", "not_reachable", "unknown", "not_applicable"}
TRACKING_ISSUE_RE = re.compile(r"https://github\.com/plx/ferric-rules/issues/(151|215|219|220)\Z")
SHA256_RE = re.compile(r"[0-9a-f]{64}\Z")
SHA_RE = re.compile(r"[0-9a-f]{40}\Z")
GHSA_RE = re.compile(r"GHSA-[0-9a-z]{4}-[0-9a-z]{4}-[0-9a-z]{4}", re.IGNORECASE)
CANONICAL_RUSTSEC_RE = re.compile(r"RUSTSEC-[0-9]{4}-[0-9]{4}\Z")
CANONICAL_GHSA_RE = re.compile(r"GHSA-[0-9A-Z]{4}-[0-9A-Z]{4}-[0-9A-Z]{4}\Z")
CANONICAL_PYSEC_RE = re.compile(r"PYSEC-[0-9]{4}-[0-9]+\Z")
CANONICAL_CVE_RE = re.compile(r"CVE-[0-9]{4}-[0-9]{4,}\Z")
CANONICAL_NPM_RE = re.compile(r"NPM-[0-9]+\Z")
SEMVER_TOKEN_RE = re.compile(
    r"(?<![0-9A-Za-z])v?"
    r"(?P<version>[0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.-]+)?)"
    r"(?![0-9A-Za-z.-])"
)
NPM_EXACT_VERSION_RE = re.compile(
    r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)"
    r"(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?\Z"
)
PEP517_V1_NUMERIC_RELEASE_RE = re.compile(r"(?:0|[1-9][0-9]*)(?:\.(?:0|[1-9][0-9]*))*\Z")
PEP517_V1_SPECIFIER_RE = re.compile(
    r"(?P<operator>==|!=|<=|>=|<|>)(?P<version>"
    + PEP517_V1_NUMERIC_RELEASE_RE.pattern.removesuffix(r"\Z")
    + r")\Z"
)
CDX_HASH_ALGORITHMS = {
    "SHA-1": ("sha1", 20),
    "SHA-256": ("sha256", 32),
    "SHA-384": ("sha384", 48),
    "SHA-512": ("sha512", 64),
}
CARGO_AUDIT_DATABASE_URL = "https://github.com/RustSec/advisory-db"
CARGO_ALIAS_NAMES = ("audit", "deny", "cyclonedx", "about")
CARGO_ALIAS_ENVIRONMENT_NAMES = tuple(f"CARGO_ALIAS_{name.upper()}" for name in CARGO_ALIAS_NAMES)
CARGO_ALIAS_ENVIRONMENT = {name: "<cleared>" for name in CARGO_ALIAS_ENVIRONMENT_NAMES}
CARGO_AUDIT_ENVIRONMENT = {
    "CARGO_HOME": "<isolated-empty-directory>",
    "CARGO_AUDIT_*": "<cleared>",
    "RUSTSEC_*": "<cleared>",
    **CARGO_ALIAS_ENVIRONMENT,
}
CARGO_DENY_ENVIRONMENT = dict(CARGO_ALIAS_ENVIRONMENT)
NPM_AUDIT_ENVIRONMENT = {
    "NPM_CONFIG_*": "<cleared>",
    "NPM_CONFIG_USERCONFIG": "<isolated-empty-file>",
    "NPM_CONFIG_GLOBALCONFIG": "<isolated-empty-file>",
    "NPM_CONFIG_CACHE": "<isolated-directory>",
}
PIP_AUDIT_ENVIRONMENT = {
    "PIP_AUDIT_*": "<cleared>",
    "UV_*": "<cleared>",
}
UV_ENVIRONMENT = {"UV_*": "<cleared>"}
DENY_AUXILIARY_CONFIG_PATHS = (
    "deny.exceptions.toml",
    ".deny.exceptions.toml",
    ".cargo/deny.exceptions.toml",
)
EXPECTED_NORMALIZATION = {
    "schema": "canonical-json-v1",
    "uv_preview_feature": "sbom-export",
    "uv_removed_fields": ["serialNumber", "metadata.timestamp"],
    "uv_checksum_enrichment": "all-registry-artifact-sha256-from-uv-lock",
}
LOCK_DISCOVERY_ROOT_EXCLUDED = {
    ".git",
    ".context",
    "target",
    "dependency-policy-evidence",
}
LOCK_DISCOVERY_CACHE_PARTS = {
    ".venv",
    "venv",
    "node_modules",
    "__pycache__",
    ".pytest_cache",
    ".ruff_cache",
    ".mypy_cache",
    ".cache",
}
ABOUT_ACCEPTED_LICENSES = [
    "MIT",
    "Apache-2.0",
    "Apache-2.0 WITH LLVM-exception",
    "BSD-2-Clause",
    "BSL-1.0",
    "ISC",
    "Unicode-3.0",
    "Unlicense",
    "Zlib",
]
LICENSE_TEMPLATE_SHA256 = "7ff67f57f54a7e411539fbf2087d271bb1967b0e3964d001c3c41edcd2ea6161"
LICENSE_SCRIPT_SHA256 = "472a47d13f05d020c7c9eedd66fbb3fb74b95748833d963b8072fd5a94ebd14e"
CARGO_CONFIG_SHA256 = "4317bf5980c303c718623b18fd21aa0653544292e08bed159cadeda3f2f153ce"
CARGO_WORKSPACE_MANIFESTS_SHA256 = (
    "4c270476c840bec13f14fa531496268ce2e26aef1a450881c82231d903023753"
)
CARGO_EXCEPTION_GRAPH_CONTEXT_SCHEMA = "ferric.cargo-exception-graph-context"
CARGO_PATCH_PATHS = {
    "ferric-rules-core": "crates/ferric-rules-core",
    "ferric-rules-ffi-macros": "crates/ferric-rules-ffi-macros",
    "ferric-rules-parser": "crates/ferric-rules-parser",
    "ferric-rules-pinned": "crates/ferric-rules-pinned",
    "ferric-rules-runtime": "crates/ferric-rules-runtime",
}

REQUIRED_EXCEPTION_FIELDS = {
    "exception_id",
    "ecosystem",
    "project_id",
    "lockfile",
    "kind",
    "finding_id",
    "package",
    "scanner_severity",
    "dependency_scopes",
    "dev_only",
    "affected_surfaces",
    "reachability",
    "evidence",
    "owner",
    "tracking_issue",
    "rationale",
    "remediation",
    "issued_on",
    "expires_on",
}

RAW_FILENAMES = {
    "scan-manifest.json",
    "tool-versions.json",
    "license-notices.json",
    "rust-workspace.cargo-audit.json",
    "cargo-deny.json",
    "node-package.npm-audit.json",
    "node-addon.npm-audit.json",
    "documentation.npm-audit.json",
    "site.npm-audit.json",
    "python-package.pip-audit.json",
    "python-tools.pip-audit.json",
}

SCAN_REPORTS = {
    ("rust-workspace", "cargo-audit"): "rust-workspace.cargo-audit.json",
    ("rust-workspace", "cargo-deny"): "cargo-deny.json",
    ("node-package", "npm-audit"): "node-package.npm-audit.json",
    ("node-addon", "npm-audit"): "node-addon.npm-audit.json",
    ("documentation", "npm-audit"): "documentation.npm-audit.json",
    ("site", "npm-audit"): "site.npm-audit.json",
    ("python-package", "pip-audit"): "python-package.pip-audit.json",
    ("python-tools", "pip-audit"): "python-tools.pip-audit.json",
}

SCAN_MANIFEST_FIELDS = {
    "schema",
    "version",
    "candidate_sha",
    "evaluated_on",
    "target_scope",
    "source_date_epoch",
    "normalization",
    "policy_sha256",
    "deny_config_sha256",
    "cargo_config_sha256",
    "cargo_workspace_manifests_sha256",
    "tool_versions",
    "inputs",
    "scans",
    "sboms",
    "license_notice",
    "raw_files",
}
INPUT_MANIFEST_FIELDS = {
    "project_id",
    "ecosystem",
    "manifest",
    "manifest_sha256",
    "lockfile",
    "sha256",
}
RAW_FILE_MANIFEST_FIELDS = {"path", "sha256"}
SBOM_MANIFEST_FIELDS = {
    "project_id",
    "path",
    "command",
    "environment",
    "working_directory",
    "exit_code",
    "sha256",
    "lockfile_sha256",
    "normalization",
    "source_date_epoch",
}
LICENSE_NOTICE_MANIFEST_FIELDS = {
    "path",
    "command",
    "working_directory",
    "exit_code",
    "status",
    "sha256",
    "about_config_sha256",
    "template_sha256",
    "script_sha256",
    "cargo_lock_sha256",
}
EXPECTED_SBOM_PATHS = {
    project_id: (
        "rust-workspace.sbom-manifest.json"
        if project_id == "rust-workspace"
        else f"{project_id}.cdx.json"
    )
    for project_id in EXPECTED_PROJECTS
}

# Invoke Cargo plugin binaries directly so Cargo aliases from ancestor/global
# configuration cannot replace the pinned scanners.
CARGO_AUDIT_ARGV = ["cargo-audit", "audit", "--file", "Cargo.lock", "--format", "json"]
CARGO_DENY_ARGV = [
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
NPM_AUDIT_ARGV = [
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
PIP_AUDIT_TOOL_ARGV = [
    "uvx",
    "--isolated",
    "--no-env-file",
    "--no-config",
    "--no-cache",
    "--default-index",
    PINNED_PYPI_REGISTRY,
    "--from",
    "pip-audit==2.10.1",
    "pip-audit",
]
PIP_AUDIT_ARGV = [
    *PIP_AUDIT_TOOL_ARGV,
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
CARGO_SBOM_ARGV = [
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
NPM_SBOM_ARGV = [
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
UV_EXPORT_ARGV = [
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
UV_SBOM_ARGV = [
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
LICENSE_NOTICE_ARGV = ["./scripts/license-notices.sh", "check"]


class PolicyError(ValueError):
    """A fail-closed policy, evidence, scanner, or inventory error."""


def _reject_duplicate_pairs(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise PolicyError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def _json_loads(value: str | bytes, *, label: str = "JSON") -> Any:
    try:
        if isinstance(value, bytes):
            value = value.decode("utf-8")
        return json.loads(value, object_pairs_hook=_reject_duplicate_pairs)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise PolicyError(f"invalid {label}: {error}") from error


def _json_value(value: Any, *, allow_jsonl: bool = False, label: str = "scanner report") -> Any:
    if isinstance(value, (Mapping, list)):
        return value
    if isinstance(value, pathlib.Path):
        value = value.read_bytes()
    if isinstance(value, (str, bytes)):
        try:
            return _json_loads(value, label=label)
        except PolicyError:
            if not allow_jsonl:
                raise
            text = value.decode("utf-8") if isinstance(value, bytes) else value
            objects = []
            for line_number, line in enumerate(text.splitlines(), 1):
                if line.strip():
                    objects.append(_json_loads(line, label=f"{label} JSONL line {line_number}"))
            if not objects:
                raise PolicyError(f"empty {label}") from None
            return objects
    raise PolicyError(f"{label} must be parsed JSON, JSON text, bytes, or a path")


def _require(condition: bool, message: str) -> None:
    if not condition:
        raise PolicyError(message)


def _parse_date(value: Any, field: str) -> dt.date:
    _require(
        isinstance(value, str) and re.fullmatch(r"\d{4}-\d{2}-\d{2}", value) is not None,
        f"{field} must be UTC YYYY-MM-DD",
    )
    try:
        parsed = dt.date.fromisoformat(value)
    except ValueError as error:
        raise PolicyError(f"{field} is not a calendar date: {value}") from error
    _require(parsed.isoformat() == value, f"{field} must be canonical UTC YYYY-MM-DD")
    return parsed


def _today(value: str | dt.date) -> dt.date:
    return value if isinstance(value, dt.date) else _parse_date(value, "today")


def _current_utc_date() -> str:
    return dt.datetime.now(dt.UTC).date().isoformat()


def _sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _sha256_file(path: pathlib.Path) -> str:
    return _sha256_bytes(path.read_bytes())


def _canonical_name(name: str, ecosystem: str) -> str:
    if ecosystem == "pypi":
        return re.sub(r"[-_.]+", "-", name).lower()
    return name


def load_policy(path: str | pathlib.Path) -> dict[str, Any]:
    policy_path = pathlib.Path(path)
    result = _json_loads(policy_path.read_bytes(), label=str(policy_path))
    _require(isinstance(result, dict), "dependency policy must be a JSON object")
    return result


def _policy_repository_path(
    path: str | pathlib.Path,
) -> tuple[pathlib.Path, pathlib.Path]:
    """Return the lexical canonical policy path and repository root."""

    policy_path = pathlib.Path(os.path.abspath(path))
    repo_root = policy_path.parent
    _require(
        policy_path.name == "dependency-policy.json",
        "policy path must be the lexical repository-root dependency-policy.json",
    )
    _require(
        repo_root.is_dir() and not repo_root.is_symlink(),
        "policy repository root must be a regular non-symlink directory",
    )
    _require(
        policy_path.is_file() and not policy_path.is_symlink(),
        "dependency-policy.json must be a regular non-symlink file",
    )
    return policy_path, repo_root


def _validate_project_files(repo_root: pathlib.Path, policy: Mapping[str, Any]) -> str:
    """Require safe project inputs and return the authenticated Cargo graph digest."""

    _validate_lock_surface_discovery(repo_root)
    cargo_graph_sha256: str | None = None
    for project in policy["projects"]:
        project_id = str(project["project_id"])
        for field in ("manifest", "lockfile"):
            _lexical_regular_repo_file(
                repo_root,
                project[field],
                label=f"{project_id} {field}",
            )
        if project["ecosystem"] == "cargo":
            _require(cargo_graph_sha256 is None, "policy contains multiple Cargo projects")
            cargo_graph_sha256 = _repository_cargo_graph_sha256(repo_root, project)
        elif project["ecosystem"] == "npm":
            _validate_npm_manifest_lock_sync(
                repo_root / project["manifest"],
                repo_root / project["lockfile"],
                project_id=project_id,
            )
            if "build" in project.get("default_dependency_scopes", []):
                _npm_build_reachable_nodes(
                    repo_root / project["manifest"],
                    repo_root / project["lockfile"],
                )
        elif project["ecosystem"] == "pypi":
            _uv_dependency_context(
                repo_root / project["lockfile"],
                repo_root / project["manifest"],
                include_build_scope="build" in project.get("default_dependency_scopes", []),
            )
    _require(cargo_graph_sha256 is not None, "policy Cargo project is missing")
    for index, exception in enumerate(policy["exceptions"]):
        if exception.get("ecosystem") == "cargo":
            _require(
                exception.get("cargo_graph_sha256") == cargo_graph_sha256,
                f"exceptions[{index}].cargo_graph_sha256 does not match "
                "the authenticated Cargo graph",
            )
    _license_contract_hashes(repo_root)
    return cargo_graph_sha256


def _lexical_regular_repo_file(
    repo_root: pathlib.Path,
    relative_value: Any,
    *,
    label: str,
) -> pathlib.Path:
    relative = _safe_relative(relative_value, field=label)
    path = repo_root
    for part in relative.parts:
        path /= part
        _require(not path.is_symlink(), f"{label} path contains a symlink: {relative}")
    _require(path.is_file(), f"{label} is not a regular file: {relative}")
    return path


def _npm_root_string_map(value: Mapping[str, Any], field: str, *, label: str) -> dict[str, str]:
    if field not in value:
        return {}
    mapping = value[field]
    _require(isinstance(mapping, Mapping), f"{label}.{field} must be an object when present")
    _require(
        all(
            isinstance(name, str)
            and bool(name)
            and isinstance(specification, str)
            and bool(specification)
            for name, specification in mapping.items()
        ),
        f"{label}.{field} must map nonempty package names to nonempty specifications",
    )
    return dict(mapping)


def _npm_workspaces(value: Mapping[str, Any], *, label: str) -> Any:
    if "workspaces" not in value:
        return []
    workspaces = value["workspaces"]
    if isinstance(workspaces, list):
        _require(
            all(isinstance(item, str) and bool(item) for item in workspaces)
            and len(workspaces) == len(set(workspaces)),
            f"{label}.workspaces must contain unique nonempty strings",
        )
        return sorted(workspaces)
    _require(
        isinstance(workspaces, Mapping) and set(workspaces) <= {"packages", "nohoist"},
        f"{label}.workspaces has an unsupported shape",
    )
    normalized: dict[str, list[str]] = {}
    for field in ("packages", "nohoist"):
        entries = workspaces.get(field, [])
        _require(
            isinstance(entries, list)
            and all(isinstance(item, str) and bool(item) for item in entries)
            and len(entries) == len(set(entries)),
            f"{label}.workspaces.{field} must contain unique nonempty strings",
        )
        normalized[field] = sorted(entries)
    return normalized


def _npm_root_string_array(value: Mapping[str, Any], field: str, *, label: str) -> list[str]:
    if field not in value:
        return []
    entries = value[field]
    _require(
        isinstance(entries, list)
        and all(isinstance(item, str) and bool(item) for item in entries)
        and len(entries) == len(set(entries)),
        f"{label}.{field} must contain unique nonempty strings",
    )
    return sorted(entries)


def _require_no_unbound_npm_root_fields(value: Mapping[str, Any], *, label: str) -> None:
    for field in (
        "overrides",
        "bundleDependencies",
        "bundledDependencies",
        "acceptDependencies",
    ):
        if field not in value:
            continue
        setting = value[field]
        empty = (
            setting is None
            or setting is False
            or (isinstance(setting, Mapping) and not setting)
            or (isinstance(setting, list) and not setting)
        )
        _require(
            empty,
            f"{label}.{field} is nonempty but has no locked policy binding",
        )


def _npm_peer_metadata(value: Mapping[str, Any], *, label: str) -> dict[str, dict[str, bool]]:
    if "peerDependenciesMeta" not in value:
        return {}
    metadata = value["peerDependenciesMeta"]
    _require(
        isinstance(metadata, Mapping),
        f"{label}.peerDependenciesMeta must be an object when present",
    )
    result: dict[str, dict[str, bool]] = {}
    for name, settings in metadata.items():
        _require(
            isinstance(name, str)
            and bool(name)
            and isinstance(settings, Mapping)
            and set(settings) == {"optional"}
            and isinstance(settings["optional"], bool),
            f"{label}.peerDependenciesMeta has an unsupported entry: {name}",
        )
        result[name] = {"optional": settings["optional"]}
    return result


def _validate_npm_manifest_lock_sync(
    manifest_path: pathlib.Path,
    lockfile_path: pathlib.Path,
    *,
    project_id: str,
) -> None:
    manifest = _json_loads(manifest_path.read_bytes(), label=f"{project_id} package.json")
    lock = _json_loads(lockfile_path.read_bytes(), label=f"{project_id} package-lock.json")
    _require(
        isinstance(manifest, Mapping) and isinstance(lock, Mapping),
        f"{project_id} npm manifest/lock must be objects",
    )
    _require(
        set(lock) == {"name", "version", "lockfileVersion", "requires", "packages"}
        and lock.get("lockfileVersion") == 3
        and not isinstance(lock.get("lockfileVersion"), bool)
        and lock.get("requires") is True,
        f"{project_id} package-lock must be the complete npm lockfile v3 schema",
    )
    packages = lock.get("packages")
    _require(
        isinstance(packages, Mapping) and isinstance(packages.get(""), Mapping),
        f"{project_id} package-lock root package is missing",
    )
    lock_root = packages[""]
    _npm_manifest_scripts(manifest)
    _require_no_unbound_npm_root_fields(manifest, label=f"{project_id} package.json")
    _require_no_unbound_npm_root_fields(lock_root, label=f"{project_id} package-lock root")
    for field in ("name", "version"):
        _require(
            isinstance(manifest.get(field), str)
            and bool(manifest[field])
            and isinstance(lock_root.get(field), str)
            and bool(lock_root[field])
            and manifest[field] == lock_root[field],
            f"{project_id} package.json/{field} differs from package-lock root",
        )
        _require(
            isinstance(lock.get(field), str)
            and bool(lock[field])
            and lock[field] == manifest[field],
            f"{project_id} package-lock top-level {field} differs from package.json",
        )
    for field in (
        "dependencies",
        "devDependencies",
        "optionalDependencies",
        "peerDependencies",
    ):
        _require(
            _npm_root_string_map(manifest, field, label=f"{project_id} package.json")
            == _npm_root_string_map(lock_root, field, label=f"{project_id} package-lock root"),
            f"{project_id} package.json {field} differs from package-lock root",
        )
    _require(
        _npm_peer_metadata(manifest, label=f"{project_id} package.json")
        == _npm_peer_metadata(lock_root, label=f"{project_id} package-lock root"),
        f"{project_id} package.json peerDependenciesMeta differs from package-lock root",
    )
    _require(
        _npm_workspaces(manifest, label=f"{project_id} package.json")
        == _npm_workspaces(lock_root, label=f"{project_id} package-lock root"),
        f"{project_id} package.json workspaces differ from package-lock root",
    )
    _require(
        _npm_root_string_map(manifest, "engines", label=f"{project_id} package.json")
        == _npm_root_string_map(lock_root, "engines", label=f"{project_id} package-lock root"),
        f"{project_id} package.json engines differs from package-lock root",
    )
    for field in ("os", "cpu", "libc"):
        _require(
            _npm_root_string_array(manifest, field, label=f"{project_id} package.json")
            == _npm_root_string_array(
                lock_root,
                field,
                label=f"{project_id} package-lock root",
            ),
            f"{project_id} package.json {field} differs from package-lock root",
        )


def _validate_lock_surface_discovery(repo_root: pathlib.Path) -> None:
    workspace_manifests = _workspace_cargo_manifest_paths(repo_root)
    expected = {
        "Cargo.lock": {"Cargo.lock"},
        "Cargo.toml": workspace_manifests,
        "package-lock.json": {
            lockfile
            for ecosystem, _manifest, lockfile in EXPECTED_PROJECTS.values()
            if ecosystem == "npm"
        },
        "uv.lock": {
            lockfile
            for ecosystem, _manifest, lockfile in EXPECTED_PROJECTS.values()
            if ecosystem == "pypi"
        },
        "package.json": {
            manifest
            for ecosystem, manifest, _lockfile in EXPECTED_PROJECTS.values()
            if ecosystem == "npm"
        },
        "pyproject.toml": {
            manifest
            for ecosystem, manifest, _lockfile in EXPECTED_PROJECTS.values()
            if ecosystem == "pypi"
        },
        "npm-shrinkwrap.json": set(),
    }
    discovered = {name: set() for name in expected}
    for current, directories, filenames in os.walk(repo_root, followlinks=False):
        directory = pathlib.Path(current)
        relative_directory = directory.relative_to(repo_root)

        def excluded(relative: pathlib.PurePath) -> bool:
            return bool(relative.parts) and (
                relative.parts[0] in LOCK_DISCOVERY_ROOT_EXCLUDED
                or any(part in LOCK_DISCOVERY_CACHE_PARTS for part in relative.parts)
            )

        directories[:] = [
            name
            for name in directories
            if not excluded(relative_directory / name) and not (directory / name).is_symlink()
        ]
        if excluded(relative_directory):
            continue
        for filename in filenames:
            if filename not in expected:
                continue
            path = directory / filename
            relative = path.relative_to(repo_root)
            _require(
                path.is_file() and not path.is_symlink(),
                f"dependency surface is not a regular non-symlink file: {relative}",
            )
            discovered[filename].add(relative.as_posix())
    for name, locked_paths in expected.items():
        _require(
            discovered[name] == locked_paths,
            f"discovered {name} surfaces differ from policy; "
            f"missing={sorted(locked_paths - discovered[name])} "
            f"extra={sorted(discovered[name] - locked_paths)}",
        )


def _workspace_cargo_manifest_paths(repo_root: pathlib.Path) -> set[str]:
    """Expand the root workspace members into the covered Cargo manifests."""

    root_manifest = _lexical_regular_repo_file(
        repo_root,
        "Cargo.toml",
        label="Cargo workspace manifest",
    )
    try:
        cargo = tomllib.loads(root_manifest.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, tomllib.TOMLDecodeError) as error:
        raise PolicyError(f"invalid Cargo workspace manifest: {error}") from error
    workspace = cargo.get("workspace")
    _require(isinstance(workspace, Mapping), "Cargo.toml workspace table is missing")
    members = workspace.get("members")
    _require(
        isinstance(members, list)
        and members
        and all(isinstance(member, str) and bool(member) for member in members)
        and len(members) == len(set(members)),
        "Cargo workspace members must be unique nonempty strings",
    )
    manifests = {"Cargo.toml"}
    for member_pattern in members:
        safe_pattern = _safe_relative(member_pattern, field="Cargo workspace member")
        matched_directories = []
        for match in sorted(repo_root.glob(safe_pattern.as_posix())):
            _require(
                not match.is_symlink(),
                f"Cargo workspace member glob matched a symlink: {match.relative_to(repo_root)}",
            )
            if match.is_dir():
                matched_directories.append(match)
        _require(
            matched_directories,
            f"Cargo workspace member pattern matched no directories: {member_pattern}",
        )
        for match in matched_directories:
            relative_directory = match.relative_to(repo_root)
            current = repo_root
            for part in relative_directory.parts:
                current /= part
                _require(
                    not current.is_symlink(),
                    f"Cargo workspace member path contains a symlink: {relative_directory}",
                )
            _require(
                match.is_dir(),
                f"Cargo workspace member is not a directory: {relative_directory}",
            )
            relative_manifest = relative_directory / "Cargo.toml"
            _lexical_regular_repo_file(
                repo_root,
                relative_manifest.as_posix(),
                label="Cargo workspace member manifest",
            )
            manifest_value = relative_manifest.as_posix()
            _require(
                manifest_value not in manifests,
                f"Cargo workspace member patterns overlap: {manifest_value}",
            )
            manifests.add(manifest_value)
    return manifests


def _cargo_workspace_manifest_contract(repo_root: pathlib.Path) -> str:
    """Bind every safely expanded workspace Cargo manifest to reviewed bytes."""

    pairs = [
        [path, _sha256_file(_lexical_regular_repo_file(repo_root, path, label=path))]
        for path in sorted(_workspace_cargo_manifest_paths(repo_root))
    ]
    digest = _sha256_bytes(_canonical_json_bytes(pairs))
    _require(
        digest == CARGO_WORKSPACE_MANIFESTS_SHA256,
        "Cargo workspace manifest aggregate differs from the reviewed contract",
    )
    return digest


def _license_contract_hashes(repo_root: pathlib.Path) -> dict[str, str]:
    """Validate and hash the fixed cargo-about notice-generation contract."""

    about_path = _lexical_regular_repo_file(repo_root, "about.toml", label="about.toml")
    template_path = _lexical_regular_repo_file(
        repo_root,
        "licenses/third-party-notices.hbs",
        label="license notice template",
    )
    script_path = _lexical_regular_repo_file(
        repo_root,
        "scripts/license-notices.sh",
        label="license notice script",
    )
    cargo_manifest_path = _lexical_regular_repo_file(
        repo_root,
        "Cargo.toml",
        label="Cargo workspace manifest",
    )
    cargo_lock_path = _lexical_regular_repo_file(
        repo_root,
        "Cargo.lock",
        label="Cargo workspace lockfile",
    )
    try:
        about = tomllib.loads(about_path.read_text(encoding="utf-8"))
        cargo_manifest = tomllib.loads(cargo_manifest_path.read_text(encoding="utf-8"))
        cargo_lock = tomllib.loads(cargo_lock_path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, tomllib.TOMLDecodeError) as error:
        raise PolicyError(f"invalid license-notice TOML input: {error}") from error
    _require(
        set(about) == {"accepted", "cbindgen"}
        and about.get("accepted") == ABOUT_ACCEPTED_LICENSES
        and about.get("cbindgen") == {"accepted": ["MPL-2.0"]},
        "about.toml differs from the locked license-notice schema",
    )
    workspace = cargo_manifest.get("workspace")
    _require(isinstance(workspace, Mapping), "Cargo.toml workspace table is missing")
    dependencies = workspace.get("dependencies")
    _require(
        isinstance(dependencies, Mapping) and dependencies.get("cbindgen") == "=0.28.0",
        "Cargo.toml must bind the cbindgen license carve-out to exactly 0.28.0",
    )
    packages = cargo_lock.get("package")
    _require(isinstance(packages, list), "Cargo.lock package inventory is missing")
    cbindgen_packages = [
        package
        for package in packages
        if isinstance(package, Mapping) and package.get("name") == "cbindgen"
    ]
    _require(
        len(cbindgen_packages) == 1 and cbindgen_packages[0].get("version") == "0.28.0",
        "Cargo.lock must contain exactly cbindgen 0.28.0 for the license carve-out",
    )
    template_sha256 = _sha256_file(template_path)
    script_sha256 = _sha256_file(script_path)
    _require(
        template_sha256 == LICENSE_TEMPLATE_SHA256,
        "license notice template bytes differ from the locked template",
    )
    _require(
        script_sha256 == LICENSE_SCRIPT_SHA256,
        "license notice script bytes differ from the locked generator",
    )
    return {
        "about_config_sha256": _sha256_file(about_path),
        "template_sha256": template_sha256,
        "script_sha256": script_sha256,
        "cargo_lock_sha256": _sha256_file(cargo_lock_path),
    }


def _exception_identity(
    item: Mapping[str, Any],
) -> tuple[str, str, str, str, str, str, str]:
    package = item.get("package")
    _require(isinstance(package, Mapping), "exception package must be an object")
    fields = ("ecosystem", "project_id", "lockfile", "kind", "finding_id")
    _require(
        all(isinstance(item.get(field), str) and bool(item[field]) for field in fields),
        "finding/exception identity fields must be nonempty strings",
    )
    _require(
        isinstance(package.get("name"), str)
        and bool(package["name"])
        and isinstance(package.get("version"), str)
        and bool(package["version"]),
        "finding/exception package identity must use nonempty strings",
    )
    return (
        item["ecosystem"],
        item["project_id"],
        item["lockfile"],
        item["kind"],
        item["finding_id"],
        package["name"],
        package["version"],
    )


def _validate_policy(
    policy: Mapping[str, Any], today: str | dt.date, *, require_active: bool
) -> None:
    day = _today(today)
    _require(
        set(policy) == {"schema", "version", "tool_pins", "owners", "projects", "exceptions"},
        "policy top-level fields differ from the locked schema",
    )
    _require(policy.get("schema") == POLICY_SCHEMA, f"schema must be {POLICY_SCHEMA}")
    _require(
        isinstance(policy.get("version"), int)
        and not isinstance(policy.get("version"), bool)
        and policy.get("version") == SCHEMA_VERSION,
        "policy version must be 1",
    )
    _require(
        policy.get("tool_pins") == EXPECTED_TOOL_PINS,
        "tool_pins must exactly match the locked versions",
    )
    owners = policy.get("owners")
    _require(
        isinstance(owners, list) and set(owners) == OWNERS and len(owners) == len(OWNERS),
        "owners must exactly match the locked enum",
    )

    projects = policy.get("projects")
    _require(isinstance(projects, list), "projects must be a list")
    by_id: dict[str, Mapping[str, Any]] = {}
    for project in projects:
        _require(isinstance(project, Mapping), "each project must be an object")
        project_id = project.get("project_id")
        _require(
            isinstance(project_id, str) and project_id not in by_id,
            f"duplicate or invalid project_id: {project_id}",
        )
        by_id[project_id] = project
    _require(
        set(by_id) == set(EXPECTED_PROJECTS),
        "projects must be exactly the seven locked surfaces",
    )
    for project_id, (ecosystem, manifest, lockfile) in EXPECTED_PROJECTS.items():
        project = by_id[project_id]
        expected_project_fields = {
            "project_id",
            "ecosystem",
            "manifest",
            "lockfile",
            "dependency_groups",
            "targets",
            "owner",
            "default_dependency_scopes",
            "affected_surfaces",
        }
        if ecosystem == "cargo":
            expected_project_fields.add("features")
        _require(
            set(project) == expected_project_fields,
            f"projects[{project_id}] fields differ from the locked schema",
        )
        _require(
            project.get("ecosystem") == ecosystem,
            f"projects[{project_id}].ecosystem is invalid",
        )
        _require(
            project.get("manifest") == manifest,
            f"projects[{project_id}].manifest is invalid",
        )
        _require(
            project.get("lockfile") == lockfile,
            f"projects[{project_id}].lockfile is invalid",
        )
        _require(
            project.get("dependency_groups") == "all",
            f"projects[{project_id}].dependency_groups must be all",
        )
        _require(
            project.get("targets") == "all",
            f"projects[{project_id}].targets must be all",
        )
        if ecosystem == "cargo":
            _require(
                project.get("features") == "all",
                f"projects[{project_id}].features must be all",
            )
        _require(project.get("owner") in OWNERS, f"projects[{project_id}].owner is invalid")
        scopes = project.get("default_dependency_scopes")
        _require(
            isinstance(scopes, list) and scopes and set(scopes) <= SCOPES,
            f"projects[{project_id}].default_dependency_scopes is invalid",
        )
        surfaces = project.get("affected_surfaces")
        _require(
            isinstance(surfaces, list)
            and surfaces
            and all(isinstance(x, str) and x for x in surfaces),
            f"projects[{project_id}].affected_surfaces is invalid",
        )

    exceptions = policy.get("exceptions")
    _require(isinstance(exceptions, list), "exceptions must be a list")
    ids: set[str] = set()
    identities: set[tuple[str, ...]] = set()
    for index, exception in enumerate(exceptions):
        prefix = f"exceptions[{index}]"
        _require(isinstance(exception, Mapping), f"{prefix} must be an object")
        expected_exception_fields = set(REQUIRED_EXCEPTION_FIELDS)
        if exception.get("ecosystem") == "cargo":
            expected_exception_fields.add("cargo_graph_sha256")
        missing = expected_exception_fields - set(exception)
        extra = set(exception) - expected_exception_fields
        _require(
            not missing,
            f"{prefix} missing required field(s): {', '.join(sorted(missing))}",
        )
        _require(not extra, f"{prefix} has unknown field(s): {', '.join(sorted(extra))}")
        exception_id = exception.get("exception_id")
        _require(
            isinstance(exception_id, str) and exception_id and exception_id not in ids,
            f"{prefix}.exception_id is duplicate or invalid",
        )
        ids.add(exception_id)
        project_id = exception.get("project_id")
        _require(project_id in by_id, f"{prefix}.project_id is invalid")
        project = by_id[str(project_id)]
        _require(
            exception.get("ecosystem") == project["ecosystem"],
            f"{prefix}.ecosystem does not match its project",
        )
        if exception.get("ecosystem") == "cargo":
            _require(
                isinstance(exception.get("cargo_graph_sha256"), str)
                and SHA256_RE.fullmatch(exception["cargo_graph_sha256"]) is not None,
                f"{prefix}.cargo_graph_sha256 must be exact lowercase SHA-256",
            )
        _require(
            exception.get("lockfile") == project["lockfile"],
            f"{prefix}.lockfile does not match its project",
        )
        _require(exception.get("kind") in KINDS, f"{prefix}.kind is invalid")
        _require(
            exception.get("scanner_severity") in SEVERITIES,
            f"{prefix}.scanner_severity is invalid",
        )
        _require(exception.get("owner") in OWNERS, f"{prefix}.owner is invalid")
        _require(
            exception.get("reachability") in REACHABILITY,
            f"{prefix}.reachability is invalid",
        )
        if exception.get("kind") == "vulnerability":
            _require(
                exception.get("reachability") != "not_applicable",
                f"{prefix}.reachability cannot be not_applicable for a vulnerability",
            )
        scopes = exception.get("dependency_scopes")
        _require(
            isinstance(scopes, list) and scopes and set(scopes) <= SCOPES,
            f"{prefix}.dependency_scopes is invalid",
        )
        _require(
            isinstance(exception.get("dev_only"), bool),
            f"{prefix}.dev_only must be boolean",
        )
        surfaces = exception.get("affected_surfaces")
        _require(
            isinstance(surfaces, list)
            and surfaces
            and all(isinstance(x, str) and x for x in surfaces),
            f"{prefix}.affected_surfaces is invalid",
        )
        evidence = exception.get("evidence")
        _require(
            isinstance(evidence, list)
            and evidence
            and all(
                isinstance(item, Mapping)
                and set(item) == {"kind", "reference"}
                and isinstance(item["kind"], str)
                and bool(item["kind"])
                and isinstance(item["reference"], str)
                and bool(item["reference"])
                for item in evidence
            ),
            f"{prefix}.evidence must contain nonempty kind/reference objects",
        )
        _require(
            isinstance(exception.get("tracking_issue"), str)
            and TRACKING_ISSUE_RE.fullmatch(exception["tracking_issue"]) is not None,
            f"{prefix}.tracking_issue is invalid",
        )
        for field in ("finding_id", "rationale", "remediation"):
            _require(
                isinstance(exception.get(field), str) and bool(exception[field]),
                f"{prefix}.{field} must be nonempty",
            )
        finding_id = str(exception["finding_id"])
        ecosystem = str(exception["ecosystem"])
        kind = str(exception["kind"])
        if ecosystem == "cargo" and kind in {
            "vulnerability",
            "unmaintained",
            "unsound",
            "notice",
        }:
            _require(
                CANONICAL_RUSTSEC_RE.fullmatch(finding_id) is not None,
                f"{prefix}.finding_id must be canonical uppercase RUSTSEC",
            )
        elif ecosystem == "npm" and kind == "vulnerability":
            _require(
                CANONICAL_GHSA_RE.fullmatch(finding_id) is not None
                or CANONICAL_NPM_RE.fullmatch(finding_id) is not None,
                f"{prefix}.finding_id must be canonical uppercase GHSA or npm source",
            )
        elif ecosystem == "pypi" and kind == "vulnerability":
            _require(
                any(
                    pattern.fullmatch(finding_id) is not None
                    for pattern in (
                        CANONICAL_GHSA_RE,
                        CANONICAL_PYSEC_RE,
                        CANONICAL_CVE_RE,
                    )
                ),
                f"{prefix}.finding_id must be canonical uppercase GHSA/PYSEC/CVE",
            )
        package = exception.get("package")
        _require(
            isinstance(package, Mapping)
            and set(package) == {"name", "version"}
            and isinstance(package["name"], str)
            and bool(package["name"])
            and isinstance(package["version"], str)
            and bool(package["version"]),
            f"{prefix}.package must contain exact nonempty name/version",
        )
        issued = _parse_date(exception.get("issued_on"), f"{prefix}.issued_on")
        expires = _parse_date(exception.get("expires_on"), f"{prefix}.expires_on")
        _require(issued <= day, f"{prefix}.issued_on is in the future")
        _require(expires > issued, f"{prefix}.expires_on must be after issued_on")
        if require_active:
            _require(day < expires, f"{prefix}.expires_on is not active on {day.isoformat()}")
        identity = _exception_identity(exception)
        _require(
            identity not in identities,
            f"duplicate or ambiguous exception identity: {identity}",
        )
        identities.add(identity)


def validate_policy(policy: Mapping[str, Any], today: str | dt.date) -> None:
    """Validate the complete policy and require every exception to be active."""

    _validate_policy(policy, today, require_active=True)


def _project(
    project: Mapping[str, Any] | None, default_id: str = "rust-workspace"
) -> dict[str, Any]:
    if project is not None:
        return dict(project)
    ecosystem, manifest, lockfile = EXPECTED_PROJECTS[default_id]
    return {
        "project_id": default_id,
        "ecosystem": ecosystem,
        "manifest": manifest,
        "lockfile": lockfile,
        "owner": "release-engineering",
        "default_dependency_scopes": ["runtime", "build", "development", "optional"],
        "affected_surfaces": ["rust-workspace"],
    }


def _severity_from_cvss(cvss: Any) -> str:
    if isinstance(cvss, Mapping):
        cvss = cvss.get("score")
    try:
        score = float(cvss)
    except (TypeError, ValueError):
        return "unknown"
    if score >= 9:
        return "critical"
    if score >= 7:
        return "high"
    if score >= 4:
        return "moderate"
    if score > 0:
        return "low"
    return "info"


def _finding(
    project: Mapping[str, Any],
    *,
    kind: str,
    finding_id: str,
    name: str,
    version: str,
    severity: str = "unknown",
    **extra: Any,
) -> dict[str, Any]:
    result = {
        "ecosystem": project["ecosystem"],
        "project_id": project["project_id"],
        "lockfile": project["lockfile"],
        "kind": kind,
        "finding_id": finding_id,
        "package": {"name": name, "version": version},
        "scanner_severity": severity if severity in SEVERITIES else "unknown",
    }
    result.update(extra)
    return result


def _cargo_lock_package_count(lockfile_data: bytes | str | pathlib.Path) -> int:
    if isinstance(lockfile_data, pathlib.Path):
        lockfile_data = lockfile_data.read_bytes()
    if isinstance(lockfile_data, bytes):
        try:
            lockfile_data = lockfile_data.decode("utf-8")
        except UnicodeDecodeError as error:
            raise PolicyError(f"invalid Cargo.lock encoding: {error}") from error
    try:
        lock = tomllib.loads(lockfile_data)
    except tomllib.TOMLDecodeError as error:
        raise PolicyError(f"invalid Cargo.lock: {error}") from error
    packages = lock.get("package")
    _require(
        isinstance(packages, list) and all(isinstance(item, Mapping) for item in packages),
        "Cargo.lock package inventory is missing or invalid",
    )
    return len(packages)


def _validate_cargo_audit_evidence(
    report: Any,
    *,
    cargo_lock: bytes | str | pathlib.Path,
    advisory_commit: str,
) -> Mapping[str, Any]:
    """Authenticate pinned cargo-audit result scope and database provenance."""

    data = _json_value(report, label="cargo-audit evidence")
    _require(isinstance(data, Mapping), "cargo-audit evidence must be an object")
    _require(
        set(data) == {"database", "lockfile", "settings", "vulnerabilities", "warnings"},
        "cargo-audit evidence fields differ from the pinned 0.22.2 schema",
    )
    database = data.get("database")
    _require(
        isinstance(database, Mapping)
        and set(database) == {"advisory-count", "last-commit", "last-updated"},
        "cargo-audit database metadata is invalid",
    )
    _require(
        isinstance(database["advisory-count"], int)
        and not isinstance(database["advisory-count"], bool)
        and database["advisory-count"] > 0,
        "cargo-audit advisory count is invalid",
    )
    _require(
        isinstance(database["last-commit"], str)
        and SHA_RE.fullmatch(database["last-commit"]) is not None
        and database["last-commit"] == advisory_commit,
        "cargo-audit database commit does not match isolated RustSec provenance",
    )
    _require(
        isinstance(database["last-updated"], str) and bool(database["last-updated"]),
        "cargo-audit database last-updated is invalid",
    )
    lockfile = data.get("lockfile")
    _require(
        isinstance(lockfile, Mapping) and set(lockfile) == {"dependency-count"},
        "cargo-audit lockfile metadata is invalid",
    )
    dependency_count = lockfile["dependency-count"]
    _require(
        isinstance(dependency_count, int)
        and not isinstance(dependency_count, bool)
        and dependency_count == _cargo_lock_package_count(cargo_lock),
        "cargo-audit dependency count does not match the exact Cargo.lock",
    )
    settings = data.get("settings")
    _require(
        isinstance(settings, Mapping)
        and set(settings)
        == {"target_arch", "target_os", "severity", "ignore", "informational_warnings"},
        "cargo-audit settings fields differ from the pinned schema",
    )
    _require(
        settings["target_arch"] == [] and settings["target_os"] == [],
        "cargo-audit settings narrow target coverage",
    )
    _require(settings["severity"] is None, "cargo-audit severity threshold must be unset")
    _require(settings["ignore"] == [], "cargo-audit ignore list must be empty")
    informational = settings["informational_warnings"]
    _require(
        isinstance(informational, list)
        and len(informational) == 3
        and all(isinstance(item, str) for item in informational)
        and set(informational) == {"unmaintained", "unsound", "notice"},
        "cargo-audit informational warning scope is incomplete",
    )
    vulnerabilities = data.get("vulnerabilities")
    _require(
        isinstance(vulnerabilities, Mapping) and set(vulnerabilities) == {"found", "count", "list"},
        "cargo-audit vulnerability fields differ from the pinned schema",
    )
    warnings = data.get("warnings")
    supported_warnings = {"unmaintained", "unsound", "notice", "yanked"}
    _require(
        isinstance(warnings, Mapping) and set(warnings) <= supported_warnings,
        "cargo-audit warning categories differ from the pinned schema",
    )
    _require(
        all(isinstance(items, list) for items in warnings.values()),
        "cargo-audit warning categories must contain complete lists",
    )
    return data


def normalize_cargo_audit(
    report: Any, project: Mapping[str, Any] | None = None
) -> list[dict[str, Any]]:
    """Normalize cargo-audit JSON, including every informational warning."""

    data = _json_value(report, label="cargo-audit report")
    _require(isinstance(data, Mapping), "cargo-audit report must be an object")
    _require(
        "error" not in data,
        f"cargo-audit returned an error payload: {data.get('error')}",
    )
    _require(
        "vulnerabilities" in data and "warnings" in data,
        "cargo-audit report is missing required result fields",
    )
    resolved = _project(project)
    results: list[dict[str, Any]] = []
    vulnerabilities = data.get("vulnerabilities", {})
    _require(
        isinstance(vulnerabilities, Mapping),
        "cargo-audit vulnerabilities must be an object",
    )
    _require(
        isinstance(vulnerabilities.get("found"), bool)
        and isinstance(vulnerabilities.get("count"), int)
        and "list" in vulnerabilities,
        "cargo-audit vulnerabilities is missing found/count/list",
    )
    vuln_list = vulnerabilities["list"]
    _require(isinstance(vuln_list, list), "cargo-audit vulnerabilities.list must be a list")
    _require(
        vulnerabilities["count"] == len(vuln_list) and vulnerabilities["found"] == bool(vuln_list),
        "cargo-audit vulnerability count/found fields are inconsistent",
    )
    for item in vuln_list:
        _require(isinstance(item, Mapping), "cargo-audit vulnerability must be an object")
        advisory = item.get("advisory", {})
        package = item.get("package", {})
        _require(
            isinstance(advisory, Mapping) and isinstance(package, Mapping),
            "cargo-audit vulnerability lacks advisory/package",
        )
        advisory_id = advisory.get("id")
        name, version = package.get("name"), package.get("version")
        _require(
            isinstance(advisory_id, str)
            and CANONICAL_RUSTSEC_RE.fullmatch(advisory_id) is not None,
            "cargo advisory id must be canonical RUSTSEC",
        )
        _require(
            isinstance(name, str) and isinstance(version, str),
            "cargo-audit package name/version are required",
        )
        results.append(
            _finding(
                resolved,
                kind="vulnerability",
                finding_id=advisory_id,
                name=name,
                version=version,
                severity=_severity_from_cvss(advisory.get("cvss")),
                title=advisory.get("title", ""),
            )
        )

    warnings = data.get("warnings", {})
    _require(isinstance(warnings, Mapping), "cargo-audit warnings must be an object")
    for warning_kind, items in warnings.items():
        _require(
            isinstance(items, list),
            f"cargo-audit warning {warning_kind} must be a list",
        )
        for item in items:
            _require(isinstance(item, Mapping), "cargo-audit warning must be an object")
            package = item.get("package", {})
            advisory = item.get("advisory", {})
            _require(
                isinstance(package, Mapping) and isinstance(advisory, Mapping),
                "cargo-audit warning lacks package/advisory",
            )
            name, version = package.get("name"), package.get("version")
            _require(
                isinstance(name, str) and isinstance(version, str),
                "cargo-audit warning package name/version are required",
            )
            kind = str(item.get("kind") or warning_kind)
            _require(
                kind in {"unmaintained", "unsound", "notice", "yanked"},
                f"unsupported cargo informational warning: {kind}",
            )
            advisory_id = advisory.get("id")
            if kind == "yanked" and not advisory_id:
                advisory_id = f"YANKED:{name}@{version}"
            _require(
                isinstance(advisory_id, str) and advisory_id,
                "cargo warning finding id is required",
            )
            if kind != "yanked":
                _require(
                    CANONICAL_RUSTSEC_RE.fullmatch(advisory_id) is not None,
                    "cargo warning advisory id must be canonical RUSTSEC",
                )
            results.append(
                _finding(
                    resolved,
                    kind=kind,
                    finding_id=advisory_id,
                    name=name,
                    version=version,
                    title=advisory.get("title", ""),
                )
            )
    return sorted(results, key=_finding_sort_key)


def _diagnostic_package(diagnostic: Mapping[str, Any]) -> tuple[str, str]:
    fields = diagnostic.get("fields")
    graphs = fields.get("graphs") if isinstance(fields, Mapping) else None
    _require(
        isinstance(graphs, list) and bool(graphs),
        "cargo-deny package diagnostic must contain a nonempty fields.graphs list",
    )
    identities: set[tuple[str, str]] = set()
    for graph in graphs:
        krate = graph.get("Krate") if isinstance(graph, Mapping) else None
        _require(
            isinstance(krate, Mapping)
            and isinstance(krate.get("name"), str)
            and bool(krate["name"])
            and isinstance(krate.get("version"), str)
            and bool(krate["version"]),
            "cargo-deny graph root must identify an exact crate name/version",
        )
        identities.add((krate["name"], krate["version"]))
    _require(
        len(identities) == 1,
        "cargo-deny package diagnostic graph roots identify multiple crate versions",
    )
    return next(iter(identities))


_CARGO_DENY_ADVISORY_CODES = {
    "advisory-ignored",
    "advisory-not-detected",
    "index-cache-load-failure",
    "index-failure",
    "notice",
    "unknown-advisory",
    "unmaintained",
    "unsound",
    "vulnerability",
    "yanked",
    "yanked-ignored",
    "yanked-not-detected",
}
_CARGO_DENY_BAN_CODES = {
    "allowed",
    "allowed-by-wrapper",
    "banned",
    "build-script-not-allowed",
    "checksum-match",
    "checksum-mismatch",
    "default-feature-enabled",
    "denied-by-extension",
    "detected-executable",
    "detected-executable-script",
    "duplicate",
    "exact-features-mismatch",
    "feature-banned",
    "feature-not-explicitly-allowed",
    "features-enabled",
    "non-root-path",
    "non-utf8-path",
    "not-allowed",
    "path-bypassed",
    "path-bypassed-by-glob",
    "replaced-in-std",
    "skipped",
    "skipped-by-root",
    "unable-to-check-path",
    "unknown-feature",
    "unmatched-bypass",
    "unmatched-glob",
    "unmatched-path-bypass",
    "unmatched-replacement-ignore",
    "unmatched-skip",
    "unmatched-skip-root",
    "unmatched-wrapper",
    "unnecessary-skip",
    "unresolved-workspace-dependency",
    "unused-workspace-dependency",
    "unused-wrapper",
    "wildcard",
    "workspace-duplicate",
}
_CARGO_DENY_LICENSE_CODES = {
    "accepted",
    "empty-license-field",
    "gather-failure",
    "license-exception-not-encountered",
    "license-not-encountered",
    "missing-clarification-file",
    "no-license-field",
    "parse-error",
    "rejected",
    "skipped-private-workspace-crate",
    "unlicensed",
}
_CARGO_DENY_SOURCE_CODES = {
    "allowed-by-organization",
    "allowed-source",
    "git-source-underspecified",
    "source-not-allowed",
    "unmatched-organization",
    "unmatched-source",
}
_CARGO_DENY_NONBLOCKING_CODES = {
    "accepted",
    "allowed",
    "allowed-by-organization",
    "allowed-by-wrapper",
    "allowed-source",
    "checksum-match",
    "duplicate",
    "features-enabled",
    "license-not-encountered",
    "replaced-in-std",
    "skipped",
    "skipped-by-root",
    "skipped-private-workspace-crate",
}
_CARGO_DENY_EXIT_BITS = {
    "advisories": 1,
    "bans": 2,
    "licenses": 4,
    "sources": 8,
}


def _cargo_deny_check(code: str) -> str | None:
    if code in _CARGO_DENY_ADVISORY_CODES:
        return "advisories"
    if code in _CARGO_DENY_LICENSE_CODES:
        return "licenses"
    if code in _CARGO_DENY_SOURCE_CODES:
        return "sources"
    if code in _CARGO_DENY_BAN_CODES:
        return "bans"
    return None


def _cargo_deny_diagnostics(
    report: Any, *, require_summary: bool
) -> tuple[list[Mapping[str, Any]], Mapping[str, Any] | None]:
    data = _json_value(report, allow_jsonl=True, label="cargo-deny report")
    entries = data if isinstance(data, list) else [data]
    diagnostics: list[Mapping[str, Any]] = []
    summary: Mapping[str, Any] | None = None
    for index, entry in enumerate(entries):
        _require(isinstance(entry, Mapping), "cargo-deny stream entry must be an object")
        _require(
            "error" not in entry,
            f"cargo-deny returned an error payload: {entry.get('error')}",
        )
        entry_type = entry.get("type")
        if entry_type == "diagnostic":
            _require(summary is None, "cargo-deny diagnostic appears after its terminal summary")
            fields = entry.get("fields")
            _require(
                isinstance(fields, Mapping),
                "cargo-deny diagnostic fields must be an object",
            )
            if require_summary:
                _require(
                    set(entry) == {"type", "fields"}
                    and "kind" not in fields
                    and "package" not in fields
                    and all(
                        isinstance(fields.get(name), str) and bool(fields[name])
                        for name in ("code", "message", "severity")
                    ),
                    "cargo-deny diagnostic differs from the pinned JSON schema",
                )
            diagnostics.append(entry)
            continue
        if entry_type == "summary":
            _require(summary is None, "cargo-deny emitted duplicate summary records")
            _require(index == len(entries) - 1, "cargo-deny summary must be the terminal record")
            _require(
                set(entry) == {"type", "fields"} and isinstance(entry.get("fields"), Mapping),
                "cargo-deny summary fields differ from the pinned schema",
            )
            summary = entry["fields"]
            expected_checks = {"advisories", "bans", "licenses", "sources"}
            expected_counters = {"errors", "warnings", "notes", "helps"}
            _require(
                set(summary) == expected_checks,
                "cargo-deny summary checks differ from the requested complete check set",
            )
            for check, counters in summary.items():
                _require(
                    isinstance(counters, Mapping) and set(counters) == expected_counters,
                    f"cargo-deny summary counters are invalid for {check}",
                )
                _require(
                    all(
                        isinstance(value, int) and not isinstance(value, bool) and value >= 0
                        for value in counters.values()
                    ),
                    f"cargo-deny summary counts are invalid for {check}",
                )
            continue
        if entry_type == "log":
            fields = entry.get("fields")
            level = fields.get("level") if isinstance(fields, Mapping) else None
            message = fields.get("message") if isinstance(fields, Mapping) else None
            raise PolicyError(f"cargo-deny emitted unexpected {level!r} log record: {message!r}")
        raise PolicyError(f"cargo-deny emitted unknown stream entry type: {entry_type!r}")

    _require(not require_summary or summary is not None, "cargo-deny terminal summary is missing")
    if summary is not None:
        severity_counters = {
            "error": "errors",
            "warning": "warnings",
            "warn": "warnings",
            "note": "notes",
            "help": "helps",
        }
        observed = dict.fromkeys(("errors", "warnings", "notes", "helps"), 0)
        observed_enforcement = {
            check: {"errors": 0, "warnings": 0}
            for check in ("advisories", "bans", "licenses", "sources")
        }
        for diagnostic in diagnostics:
            fields = diagnostic.get("fields")
            fields = fields if isinstance(fields, Mapping) else {}
            severity = str(fields.get("severity") or diagnostic.get("level") or "").lower()
            _require(
                severity in severity_counters,
                f"cargo-deny diagnostic severity is invalid: {severity!r}",
            )
            counter = severity_counters[severity]
            observed[counter] += 1
            if counter in {"errors", "warnings"}:
                code = str(diagnostic.get("code") or fields.get("code") or "")
                check = _cargo_deny_check(code)
                _require(
                    check is not None,
                    f"cargo-deny diagnostic code cannot be assigned to a check: {code!r}",
                )
                observed_enforcement[check][counter] += 1
        summarized = {
            counter: sum(int(check[counter]) for check in summary.values()) for counter in observed
        }
        _require(
            all(
                int(summary[check][counter]) == observed_enforcement[check][counter]
                for check in observed_enforcement
                for counter in ("errors", "warnings")
            )
            and summarized["notes"] >= observed["notes"]
            and summarized["helps"] >= observed["helps"],
            "cargo-deny terminal summary counts do not match its emitted diagnostics",
        )
    return diagnostics, summary


def _cargo_deny_expected_exit_code(report: Any) -> int:
    """Derive cargo-deny 0.20.2's check-bitset exit code from its summary."""

    _, summary = _cargo_deny_diagnostics(report, require_summary=True)
    assert summary is not None  # Required by the call above.
    return sum(
        bit for check, bit in _CARGO_DENY_EXIT_BITS.items() if int(summary[check]["errors"]) > 0
    )


def normalize_cargo_deny(
    report: Any,
    project: Mapping[str, Any] | None = None,
    *,
    require_summary: bool = False,
) -> list[dict[str, Any]]:
    """Normalize cargo-deny 0.20.2 JSON or its production JSONL stream."""

    diagnostics, _ = _cargo_deny_diagnostics(report, require_summary=require_summary)
    resolved = _project(project)
    results: list[dict[str, Any]] = []
    for diagnostic in diagnostics:
        _require(isinstance(diagnostic, Mapping), "cargo-deny diagnostic must be an object")
        _require(
            "error" not in diagnostic,
            f"cargo-deny returned an error payload: {diagnostic.get('error')}",
        )
        fields = diagnostic.get("fields") if isinstance(diagnostic.get("fields"), Mapping) else {}
        code = str(diagnostic.get("code") or fields.get("code") or "diagnostic")
        if code in _CARGO_DENY_NONBLOCKING_CODES:
            continue
        if code == "yanked":
            # cargo-audit is canonical for yanked packages as well as every
            # RUSTSEC advisory; avoid a duplicate exact exception identity.
            continue
        if code in _CARGO_DENY_LICENSE_CODES:
            kind = "license"
        elif code in _CARGO_DENY_SOURCE_CODES:
            kind = "source"
        elif code in _CARGO_DENY_BAN_CODES:
            kind = "ban"
        elif code in {"vulnerability", "unmaintained", "unsound", "notice"}:
            # cargo-audit is the canonical RUSTSEC source and prevents duplicate
            # exception matching for cargo-deny's advisory diagnostics.
            continue
        else:
            severity = str(fields.get("severity") or diagnostic.get("level") or "unknown")
            raise PolicyError(
                f"unsupported cargo-deny diagnostic code {code!r} at severity {severity!r}"
            )
        name, version = _diagnostic_package(diagnostic)
        advisory: Mapping[str, Any] = (
            fields.get("advisory", {}) if isinstance(fields.get("advisory"), Mapping) else {}
        )
        if kind in {"vulnerability", "unmaintained", "unsound", "notice"}:
            finding_id = str(advisory.get("id", ""))
        elif kind == "yanked":
            finding_id = f"YANKED:{name}@{version}"
        else:
            finding_id = f"CARGO-DENY:{code}:{name}@{version}"
        results.append(
            _finding(
                resolved,
                kind=kind,
                finding_id=finding_id,
                name=name,
                version=version,
                diagnostic_code=code,
                message=diagnostic.get("message") or fields.get("message") or "",
            )
        )
    return sorted(results, key=_finding_sort_key)


def cargo_deny_reported_finding_count(
    report: Any,
    project: Mapping[str, Any] | None = None,
    *,
    require_summary: bool = False,
) -> int:
    """Count cargo-deny enforcement diagnostics before canonical de-duplication.

    RUSTSEC and yanked diagnostics are normalized only from cargo-audit, but
    still constitute real findings for cargo-deny's exit/classification
    coherence.
    """

    diagnostics, _ = _cargo_deny_diagnostics(report, require_summary=require_summary)
    count = len(normalize_cargo_deny(diagnostics, project))
    canonical_codes = {"vulnerability", "unmaintained", "unsound", "notice", "yanked"}
    for diagnostic in diagnostics:
        _require(isinstance(diagnostic, Mapping), "cargo-deny diagnostic must be an object")
        fields = diagnostic.get("fields") if isinstance(diagnostic.get("fields"), Mapping) else {}
        code = str(diagnostic.get("code") or fields.get("code") or "diagnostic")
        if code in canonical_codes:
            count += 1
    return count


def cargo_deny_canonical_advisory_findings(
    report: Any,
    project: Mapping[str, Any] | None = None,
    *,
    require_summary: bool = False,
) -> list[dict[str, Any]]:
    """Extract identities cargo-audit must also report from cargo-deny diagnostics."""

    diagnostics, _ = _cargo_deny_diagnostics(report, require_summary=require_summary)
    resolved = _project(project)
    results: list[dict[str, Any]] = []
    for diagnostic in diagnostics:
        _require(isinstance(diagnostic, Mapping), "cargo-deny diagnostic must be an object")
        fields = diagnostic.get("fields") if isinstance(diagnostic.get("fields"), Mapping) else {}
        code = str(diagnostic.get("code") or fields.get("code") or "diagnostic")
        kind = code
        if kind not in {"vulnerability", "unmaintained", "unsound", "notice", "yanked"}:
            continue
        name, version = _diagnostic_package(diagnostic)
        if kind == "yanked":
            finding_id = f"YANKED:{name}@{version}"
        else:
            advisory = fields.get("advisory")
            _require(
                isinstance(advisory, Mapping)
                and isinstance(advisory.get("id"), str)
                and CANONICAL_RUSTSEC_RE.fullmatch(advisory["id"]) is not None,
                "cargo-deny advisory diagnostic lacks a canonical RUSTSEC id",
            )
            finding_id = str(advisory["id"])
        results.append(
            _finding(
                resolved,
                kind=kind,
                finding_id=finding_id,
                name=name,
                version=version,
            )
        )
    return sorted(_dedupe_findings(results), key=_finding_sort_key)


def _npm_name_from_path(node: str) -> str:
    marker = "node_modules/"
    return node.rsplit(marker, 1)[-1] if marker in node else pathlib.PurePosixPath(node).name


def _npm_manifest_scripts(manifest: Mapping[str, Any]) -> dict[str, str]:
    scripts = manifest.get("scripts", {})
    _require(isinstance(scripts, Mapping), "npm package.json scripts must be an object")
    _require(
        all(
            isinstance(name, str)
            and bool(name)
            and isinstance(command, str)
            and bool(command.strip())
            for name, command in scripts.items()
        ),
        "npm package.json scripts must map nonempty names to nonempty commands",
    )
    return dict(scripts)


def _npm_resolve_locked_edge(
    packages: Mapping[str, Any],
    owner_path: str,
    dependency_name: str,
    *,
    label: str,
    allow_missing: bool = False,
) -> str | None:
    """Resolve one dependency using npm's nearest-node_modules lookup order."""

    _require(
        isinstance(dependency_name, str) and bool(dependency_name),
        f"{label} dependency name must be nonempty",
    )
    base = owner_path
    candidates: list[str] = []
    while True:
        candidate = (
            f"{base}/node_modules/{dependency_name}" if base else f"node_modules/{dependency_name}"
        )
        candidates.append(candidate)
        if not base:
            break
        nested_marker = "/node_modules/"
        if nested_marker in base:
            base = base.rsplit(nested_marker, 1)[0]
        elif base.startswith("node_modules/"):
            base = ""
        else:
            raise PolicyError(f"npm lock package path has an unsupported shape: {owner_path}")
    matches = [path for path in candidates if path in packages]
    if allow_missing and not matches:
        return None
    _require(
        bool(matches),
        f"{label} does not resolve to an exact package-lock node: {dependency_name}",
    )
    target_path = matches[0]
    target = packages[target_path]
    _require(
        isinstance(target, Mapping),
        f"npm lock package {target_path!r} must be an object",
    )
    locked_name = target.get("name") or _npm_name_from_path(target_path)
    _require(
        locked_name == dependency_name,
        f"{label} resolved package name differs from its lock edge: {dependency_name}",
    )
    return target_path


def _npm_build_reachable_nodes(
    manifest_data: Any,
    package_lock: Any,
) -> set[str]:
    """Return the exact devDependency closure used by a configured build lifecycle.

    The contract deliberately does not infer package use from shell text or package
    names.  A declared prebuild/build/postbuild lifecycle conservatively makes every
    authenticated direct devDependency, and all of its locked transitive edges, part
    of the build graph.
    """

    manifest = _json_value(manifest_data, label="npm package manifest")
    lock = _json_value(package_lock, label="npm package lock")
    _require(
        isinstance(manifest, Mapping) and isinstance(lock, Mapping),
        "npm build context requires package.json and package-lock objects",
    )
    packages = lock.get("packages")
    _require(
        isinstance(packages, Mapping) and isinstance(packages.get(""), Mapping),
        "npm package lock root package is missing",
    )
    lock_root = packages[""]
    manifest_dev = _npm_root_string_map(manifest, "devDependencies", label="npm package.json")
    lock_dev = _npm_root_string_map(
        lock_root,
        "devDependencies",
        label="npm package-lock root",
    )
    _require(
        manifest_dev == lock_dev,
        "npm package.json devDependencies differ from package-lock root",
    )
    scripts = _npm_manifest_scripts(manifest)
    if not any(name in scripts for name in ("prebuild", "build", "postbuild")):
        return set()

    pending = [
        _npm_resolve_locked_edge(
            packages,
            "",
            dependency_name,
            label=f"npm root devDependencies.{dependency_name}",
        )
        for dependency_name in sorted(manifest_dev)
    ]
    reachable: set[str] = set()
    while pending:
        path = pending.pop()
        if path in reachable:
            continue
        reachable.add(path)
        package = packages[path]
        _require(isinstance(package, Mapping), f"npm lock package {path!r} must be an object")
        for field in ("dependencies", "optionalDependencies", "peerDependencies"):
            edges = _npm_root_string_map(package, field, label=f"npm lock package {path!r}")
            peer_metadata = (
                _npm_peer_metadata(package, label=f"npm lock package {path!r}")
                if field == "peerDependencies"
                else {}
            )
            for dependency_name in sorted(edges, reverse=True):
                target = _npm_resolve_locked_edge(
                    packages,
                    path,
                    dependency_name,
                    label=f"npm lock package {path!r} {field}.{dependency_name}",
                    allow_missing=bool(peer_metadata.get(dependency_name, {}).get("optional")),
                )
                if target is not None:
                    pending.append(target)
    return reachable


def _npm_versions(
    package_lock: Mapping[str, Any],
    nodes: Sequence[Any],
    fallback_name: str,
    *,
    include_build_scope: bool,
    build_nodes: set[str] | None = None,
) -> list[tuple[str, str, list[str], bool]]:
    packages = package_lock.get("packages", {})
    _require(isinstance(packages, Mapping), "npm package lock packages must be an object")
    _require(bool(nodes), f"npm vulnerability {fallback_name} must identify lockfile nodes")
    lock_components = {
        item["path"]: item for item in _npm_components(_canonical_json_bytes(package_lock))
    }
    results: dict[tuple[str, str], tuple[set[str], bool]] = {}
    seen_nodes: set[str] = set()
    for node in nodes:
        _require(
            isinstance(node, str) and bool(node),
            f"npm vulnerability {fallback_name} has an invalid lockfile node",
        )
        _require(
            node not in seen_nodes,
            f"npm vulnerability {fallback_name} repeats lockfile node: {node}",
        )
        seen_nodes.add(node)
        entry = packages.get(node)
        component = lock_components.get(node)
        _require(
            isinstance(entry, Mapping) and component is not None,
            f"npm vulnerability {fallback_name} node is absent from the exact lock: {node}",
        )
        if "devOptional" in entry:
            _require(
                isinstance(entry["devOptional"], bool),
                f"npm lock package {node!r} devOptional flag must be boolean",
            )
        name = str(component["name"])
        version = str(component["version"])
        scopes: list[str] = []
        dev = bool(entry.get("dev"))
        dev_optional = bool(entry.get("devOptional"))
        if dev or dev_optional:
            scopes.append("development")
        if not dev or dev_optional:
            scopes.append("runtime")
            if include_build_scope:
                scopes.append("build")
        if build_nodes is not None and node in build_nodes and "build" not in scopes:
            scopes.append("build")
        if entry.get("optional") or dev_optional:
            scopes.append("optional")
        path_dev_only = (
            dev and not dev_optional and not (build_nodes is not None and node in build_nodes)
        )
        identity = (name, version)
        merged_scopes, all_paths_dev_only = results.setdefault(identity, (set(), True))
        merged_scopes.update(scopes)
        results[identity] = (merged_scopes, all_paths_dev_only and path_dev_only)
    scope_order = ("runtime", "build", "development", "optional")
    return [
        (
            name,
            version,
            [scope for scope in scope_order if scope in scopes],
            all_paths_dev_only,
        )
        for (name, version), (scopes, all_paths_dev_only) in sorted(results.items())
    ]


def _npm_advisory_id(advisory: Mapping[str, Any]) -> tuple[str, str]:
    url = advisory.get("url")
    _require(isinstance(url, str), "npm advisory url must be a string")
    ghsa = GHSA_RE.search(url)
    source = advisory.get("source")
    _require(
        isinstance(source, int) and not isinstance(source, bool) and source > 0,
        "npm advisory source must be a positive integer",
    )
    return (ghsa.group(0).upper() if ghsa else f"NPM-{source}", url)


def _resolve_npm_advisories(
    vulnerabilities: Mapping[str, Any], package_name: str, stack: tuple[str, ...] = ()
) -> list[tuple[str, Mapping[str, Any]]]:
    _require(
        package_name not in stack,
        f"npm vulnerability via cycle: {(*stack, package_name)}",
    )
    vulnerability = vulnerabilities.get(package_name)
    _require(
        isinstance(vulnerability, Mapping),
        f"npm vulnerability via reference is unknown: {package_name}",
    )
    via = vulnerability.get("via")
    _require(
        isinstance(via, list) and via,
        f"npm vulnerability {package_name}.via must be nonempty",
    )
    results: list[tuple[str, Mapping[str, Any]]] = []
    for item in via:
        if isinstance(item, Mapping):
            _npm_advisory_id(item)
            results.append((package_name, item))
        else:
            _require(
                isinstance(item, str) and bool(item),
                f"npm vulnerability {package_name}.via entry is invalid",
            )
            results.extend(_resolve_npm_advisories(vulnerabilities, item, (*stack, package_name)))
    _require(results, f"npm vulnerability {package_name}.via resolves to no advisories")
    return results


def _npm_audit_dependency_counts(package_lock: Any) -> dict[str, int]:
    """Reconstruct npm audit v2 dependency metadata from the exact lock."""

    lock = _json_value(package_lock, label="npm package lock")
    _require(isinstance(lock, Mapping), "npm package lock must be an object")
    packages = lock.get("packages")
    _require(isinstance(packages, Mapping), "npm package lock packages must be an object")
    _require("" in packages, "npm package lock root package is missing")
    counts = {
        "prod": 0,
        "dev": 0,
        "optional": 0,
        "peer": 0,
        "peerOptional": 0,
        "total": len(packages) - 1,
    }
    dependency_kinds = ("dev", "optional", "peer", "peerOptional")
    for path, package in packages.items():
        _require(
            isinstance(path, str) and isinstance(package, Mapping),
            "npm package lock contains an invalid package entry",
        )
        if "devOptional" in package:
            _require(
                isinstance(package["devOptional"], bool),
                f"npm lock package {path!r} devOptional flag must be boolean",
            )
        classified = False
        for kind in dependency_kinds:
            if kind in package:
                _require(
                    isinstance(package[kind], bool),
                    f"npm lock package {path!r} {kind} flag must be boolean",
                )
            if package.get(kind) is True:
                counts[kind] += 1
                classified = True
        if not classified:
            counts["prod"] += 1
    return counts


def normalize_npm_audit(
    report: Any,
    project: Mapping[str, Any],
    package_lock: Any | None = None,
    manifest_data: Any | None = None,
) -> list[dict[str, Any]]:
    """Normalize npm audit v2 JSON, preserving every advisory and lock version."""

    data = _json_value(report, label="npm audit report")
    _require(isinstance(data, Mapping), "npm audit report must be an object")
    _require("error" not in data, f"npm audit returned an error payload: {data.get('error')}")
    _require(
        set(data) == {"auditReportVersion", "vulnerabilities", "metadata"},
        "npm audit top-level fields differ from v2",
    )
    _require(data.get("auditReportVersion") == 2, "npm audit report version must be 2")
    _require("vulnerabilities" in data, "npm audit report is missing required result fields")
    vulnerabilities = data.get("vulnerabilities", {})
    _require(
        isinstance(vulnerabilities, Mapping),
        "npm audit vulnerabilities must be an object",
    )
    metadata = data.get("metadata")
    _require(isinstance(metadata, Mapping), "npm audit metadata must be an object")
    _require(
        set(metadata) == {"vulnerabilities", "dependencies"},
        "npm audit metadata fields differ from v2",
    )
    metadata_counts = metadata.get("vulnerabilities")
    _require(
        isinstance(metadata_counts, Mapping),
        "npm audit metadata.vulnerabilities must be an object",
    )
    severity_names = ("info", "low", "moderate", "high", "critical")
    _require(
        set(metadata_counts) == {*severity_names, "total"},
        "npm audit metadata.vulnerabilities fields differ from v2",
    )
    for severity in (*severity_names, "total"):
        count = metadata_counts[severity]
        _require(
            isinstance(count, int) and not isinstance(count, bool) and count >= 0,
            f"npm audit metadata.vulnerabilities.{severity} must be a nonnegative integer",
        )
    _require(
        metadata_counts["total"] == sum(metadata_counts[severity] for severity in severity_names),
        "npm audit metadata vulnerability total does not equal its severity buckets",
    )
    _require(
        metadata_counts["total"] == len(vulnerabilities),
        "npm audit metadata vulnerability total does not equal vulnerability objects",
    )
    observed_counts = dict.fromkeys(severity_names, 0)
    for package_name, vulnerability in vulnerabilities.items():
        _require(
            isinstance(package_name, str)
            and bool(package_name)
            and isinstance(vulnerability, Mapping),
            f"npm vulnerability {package_name} must be an object",
        )
        severity = vulnerability.get("severity")
        _require(
            severity in observed_counts,
            f"npm vulnerability {package_name}.severity is invalid or missing",
        )
        observed_counts[str(severity)] += 1
    _require(
        all(metadata_counts[severity] == observed_counts[severity] for severity in severity_names),
        "npm audit metadata severity buckets do not match vulnerability objects",
    )
    _require(package_lock is not None, "npm audit report requires its exact package lock")
    lock = _json_value(package_lock, label="npm package lock")
    dependencies = metadata.get("dependencies")
    expected_dependencies = _npm_audit_dependency_counts(lock)
    _require(
        isinstance(dependencies, Mapping) and set(dependencies) == set(expected_dependencies),
        "npm audit metadata.dependencies fields differ from v2",
    )
    for kind, count in dependencies.items():
        _require(
            isinstance(count, int) and not isinstance(count, bool) and count >= 0,
            f"npm audit metadata.dependencies.{kind} must be a nonnegative integer",
        )
    _require(
        dependencies == expected_dependencies,
        "npm audit metadata.dependencies do not match the exact package lock",
    )
    include_build_scope = "build" in project.get("default_dependency_scopes", [])
    _require(
        not include_build_scope or manifest_data is not None,
        "npm build-scoped audit normalization requires its authenticated package.json",
    )
    build_nodes = _npm_build_reachable_nodes(manifest_data, lock) if include_build_scope else set()
    versions_by_owner: dict[str, list[tuple[str, str, list[str], bool]]] = {}
    for fallback_name, vulnerability in vulnerabilities.items():
        _require(
            isinstance(vulnerability, Mapping),
            f"npm vulnerability {fallback_name} must be an object",
        )
        _require(
            "nodes" in vulnerability,
            f"npm vulnerability {fallback_name}.nodes is missing",
        )
        nodes = vulnerability["nodes"]
        _require(
            isinstance(nodes, list),
            f"npm vulnerability {fallback_name}.nodes must be a list",
        )
        _require(
            isinstance(lock, Mapping),
            f"npm vulnerability {fallback_name} cannot be normalized without its package lock",
        )
        versions_by_owner[str(fallback_name)] = _npm_versions(
            lock,
            nodes,
            str(fallback_name),
            include_build_scope=include_build_scope,
            build_nodes=build_nodes,
        )

    results: list[dict[str, Any]] = []
    for fallback_name in vulnerabilities:
        advisories = _resolve_npm_advisories(vulnerabilities, str(fallback_name))
        for owner_name, advisory in advisories:
            owner = vulnerabilities.get(owner_name)
            _require(
                isinstance(owner, Mapping),
                f"npm vulnerability via owner is unknown: {owner_name}",
            )
            versions = versions_by_owner[owner_name]
            finding_id, url = _npm_advisory_id(advisory)
            severity = str(advisory.get("severity") or owner.get("severity") or "unknown").lower()
            if severity not in SEVERITIES:
                severity = "unknown"
            for name, version, scopes, dev in versions:
                results.append(
                    _finding(
                        project,
                        kind="vulnerability",
                        finding_id=finding_id,
                        name=name,
                        version=version,
                        severity=severity,
                        dependency_scopes=scopes,
                        dev_only=dev,
                        advisory_url=url,
                        title=advisory.get("title", ""),
                    )
                )
    return sorted(_dedupe_findings(results), key=_finding_sort_key)


def _pip_report_objects(report: Any) -> list[Mapping[str, Any]]:
    data = _json_value(report, label="pip-audit report")
    if isinstance(data, Mapping):
        _require(
            "error" not in data,
            f"pip-audit returned an error payload: {data.get('error')}",
        )
    if isinstance(data, Mapping) and isinstance(data.get("reports"), list):
        values = data["reports"]
    elif isinstance(data, list):
        values = data
    else:
        values = [data]
    _require(
        values and all(isinstance(value, Mapping) for value in values),
        "pip-audit report envelope must contain objects",
    )
    _require(
        all("error" not in value for value in values),
        "pip-audit batch contains an error payload",
    )
    return list(values)


def _validate_pip_audit_evidence(report: Any) -> Mapping[str, Any]:
    """Authenticate the exact production batch envelope before normalization."""

    data = _json_value(report, label="pip-audit evidence")
    _require(isinstance(data, Mapping), "pip-audit evidence must be an object")
    _require(
        set(data) == {"schema", "version", "export_command", "audit_command", "reports"},
        "pip-audit evidence fields differ from the locked batch schema",
    )
    _require(
        data.get("schema") == "ferric.pip-audit-batches"
        and isinstance(data.get("version"), int)
        and not isinstance(data.get("version"), bool)
        and data.get("version") == 1,
        "pip-audit evidence schema/version is invalid",
    )
    _require(
        data.get("export_command") == UV_EXPORT_ARGV,
        "pip-audit evidence uv export argv differs from the locked command",
    )
    _require(
        data.get("audit_command") == PIP_AUDIT_ARGV,
        "pip-audit evidence audit argv differs from the locked command",
    )
    _require(
        isinstance(data.get("reports"), list) and bool(data["reports"]),
        "pip-audit evidence reports must be a nonempty list",
    )
    return data


def _pip_aliases(vulnerability: Mapping[str, Any]) -> list[str]:
    aliases = vulnerability.get("aliases", [])
    _require(isinstance(aliases, list), "pip-audit vulnerability aliases must be a list")
    _require(
        all(isinstance(value, str) and bool(value) for value in aliases),
        "pip-audit vulnerability aliases must contain nonempty strings",
    )
    return sorted({value.upper() for value in aliases})


def _pip_finding_id(vulnerability: Mapping[str, Any]) -> str:
    candidates = _pip_aliases(vulnerability)
    primary = vulnerability.get("id")
    if isinstance(primary, str):
        candidates.append(primary.upper())
    for pattern in (CANONICAL_GHSA_RE, CANONICAL_PYSEC_RE, CANONICAL_CVE_RE):
        for value in candidates:
            canonical = value.upper()
            if pattern.fullmatch(canonical) is not None:
                return canonical
    raise PolicyError("pip-audit vulnerability has no canonical GHSA/PYSEC/CVE id")


def normalize_pip_audit(
    report: Any,
    project: Mapping[str, Any],
    lockfile_data: bytes | str | pathlib.Path,
    manifest_data: Any | None = None,
) -> list[dict[str, Any]]:
    """Normalize one or more pip-audit JSON batches."""

    context = _uv_dependency_context(
        lockfile_data,
        manifest_data,
        include_build_scope="build" in project.get("default_dependency_scopes", []),
    )
    results: list[dict[str, Any]] = []
    for batch in _pip_report_objects(report):
        dependencies = batch.get("dependencies")
        _require(isinstance(dependencies, list), "pip-audit dependencies must be a list")
        _require(isinstance(batch.get("fixes"), list), "pip-audit fixes must be a list")
        for dependency in dependencies:
            _require(
                isinstance(dependency, Mapping),
                "pip-audit dependency must be an object",
            )
            name, version = dependency.get("name"), dependency.get("version")
            _require(
                isinstance(name, str) and bool(name) and isinstance(version, str) and bool(version),
                "pip-audit dependency requires name/version",
            )
            identity = (_canonical_name(name, "pypi"), version)
            _require(
                identity in context,
                f"pip-audit dependency is absent or unreachable in uv.lock: {identity}",
            )
            _require("vulns" in dependency, "pip-audit dependency vulns is missing")
            vulns = dependency["vulns"]
            _require(isinstance(vulns, list), "pip-audit dependency vulns must be a list")
            for vulnerability in vulns:
                _require(
                    isinstance(vulnerability, Mapping),
                    "pip-audit vulnerability must be an object",
                )
                aliases = _pip_aliases(vulnerability)
                results.append(
                    _finding(
                        project,
                        kind="vulnerability",
                        finding_id=_pip_finding_id(vulnerability),
                        name=identity[0],
                        version=version,
                        severity="unknown",
                        dependency_scopes=context[identity]["dependency_scopes"],
                        dev_only=context[identity]["dev_only"],
                        aliases=aliases,
                        description=vulnerability.get("description", ""),
                    )
                )
    return sorted(_dedupe_findings(results), key=_finding_sort_key)


def _uv_lock_packages(
    lockfile_data: bytes | str | pathlib.Path,
) -> list[Mapping[str, Any]]:
    if isinstance(lockfile_data, pathlib.Path):
        raw = lockfile_data.read_bytes()
    else:
        raw = lockfile_data.encode("utf-8") if isinstance(lockfile_data, str) else lockfile_data
    try:
        lock = tomllib.loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, tomllib.TOMLDecodeError) as error:
        raise PolicyError(f"invalid uv lockfile: {error}") from error
    packages = lock.get("package")
    _require(isinstance(packages, list), "uv lockfile package array is missing")
    _require(
        all(isinstance(package, Mapping) for package in packages),
        "uv lock package must be an object",
    )
    return packages


def _uv_packages(
    lockfile_data: bytes | str | pathlib.Path,
) -> dict[tuple[str, str], set[str]]:
    packages = _uv_lock_packages(lockfile_data)
    result: dict[tuple[str, str], set[str]] = {}
    for package in packages:
        source = package.get("source")
        if not isinstance(source, Mapping) or "registry" not in source:
            continue
        name, version = package.get("name"), package.get("version")
        _require(
            isinstance(name, str) and isinstance(version, str),
            "uv registry package requires name/version",
        )
        identity = (_canonical_name(name, "pypi"), version)
        hashes: set[str] = set()
        sdist = package.get("sdist")
        if sdist is not None:
            _require(isinstance(sdist, Mapping), f"uv sdist is invalid for {name} {version}")
            digest = sdist.get("hash")
            _require(
                isinstance(digest, str)
                and re.fullmatch(r"sha256:[0-9A-Fa-f]{64}", digest) is not None,
                f"uv sdist hash is invalid for {name} {version}",
            )
            hashes.add(digest.lower())
        wheels = package.get("wheels", [])
        _require(isinstance(wheels, list), f"uv wheels are invalid for {name} {version}")
        for wheel in wheels:
            _require(isinstance(wheel, Mapping), f"uv wheel is invalid for {name} {version}")
            digest = wheel.get("hash")
            _require(
                isinstance(digest, str)
                and re.fullmatch(r"sha256:[0-9A-Fa-f]{64}", digest) is not None,
                f"uv wheel hash is invalid for {name} {version}",
            )
            hashes.add(digest.lower())
        _require(bool(hashes), f"uv registry package {name} {version} has no artifact hashes")
        result.setdefault(identity, set()).update(hashes)
    _require(bool(result), "uv lockfile contains no registry packages")
    return result


def _toml_value(value: Any, *, label: str) -> Mapping[str, Any]:
    if isinstance(value, Mapping):
        return value
    if isinstance(value, pathlib.Path):
        raw = value.read_bytes()
    elif isinstance(value, bytes):
        raw = value
    elif isinstance(value, str):
        raw = value.encode("utf-8")
    else:
        raise PolicyError(f"{label} must be parsed TOML, TOML text, bytes, or a path")
    try:
        parsed = tomllib.loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, tomllib.TOMLDecodeError) as error:
        raise PolicyError(f"invalid {label}: {error}") from error
    _require(isinstance(parsed, Mapping), f"{label} must be a TOML table")
    return parsed


def _pep517_v1_release(value: Any, *, label: str) -> tuple[int, ...]:
    """Parse the policy's explicit numeric-release v1 subset.

    This is deliberately not a partial PEP 440 parser. Constructs outside plain
    dot-separated, nonnegative numeric release segments fail closed.
    """

    _require(
        isinstance(value, str) and PEP517_V1_NUMERIC_RELEASE_RE.fullmatch(value) is not None,
        f"{label} is outside the PEP 517 v1 numeric-release subset: {value}",
    )
    release = tuple(int(part) for part in value.split("."))
    while len(release) > 1 and release[-1] == 0:
        release = release[:-1]
    return release


def _pep517_v1_specifier(
    value: Any, *, require_nonempty: bool
) -> list[tuple[str, tuple[int, ...]]]:
    """Parse comma-conjoined comparisons in the explicit build-binding v1 subset."""

    _require(isinstance(value, str), "PEP 517 v1 build specifier must be a string")
    if not value:
        _require(
            not require_nonempty,
            "build-scoped PEP 517 v1 requirements must have a nonempty numeric specifier",
        )
        return []
    result: list[tuple[str, tuple[int, ...]]] = []
    for clause in value.split(","):
        match = PEP517_V1_SPECIFIER_RE.fullmatch(clause)
        _require(
            match is not None,
            "PEP 517 v1 build specifiers support only comma-conjoined "
            f"==, !=, <, <=, >, or >= numeric-release comparisons: {value}",
        )
        result.append(
            (
                match.group("operator"),
                _pep517_v1_release(
                    match.group("version"),
                    label="PEP 517 v1 build specifier version",
                ),
            )
        )
    return result


def _pep517_v1_version_satisfies(version: Any, specifier: str) -> bool:
    locked = _pep517_v1_release(version, label="uv.lock build package version")
    comparisons = _pep517_v1_specifier(specifier, require_nonempty=True)
    operators = {
        "==": lambda comparison: comparison == 0,
        "!=": lambda comparison: comparison != 0,
        "<": lambda comparison: comparison < 0,
        "<=": lambda comparison: comparison <= 0,
        ">": lambda comparison: comparison > 0,
        ">=": lambda comparison: comparison >= 0,
    }
    for operator, required in comparisons:
        width = max(len(locked), len(required))
        left = locked + (0,) * (width - len(locked))
        right = required + (0,) * (width - len(required))
        comparison = (left > right) - (left < right)
        if not operators[operator](comparison):
            return False
    return True


def _pep517_v1_build_requirement(value: Any) -> tuple[str, str, str | None]:
    """Parse only the explicit requirement syntax supported by binding v1."""

    _require(
        isinstance(value, str) and bool(value.strip()),
        "pyproject build-system.requires entries must be nonempty strings",
    )
    requirement, separator, marker = value.partition(";")
    if separator:
        _require(
            bool(marker.strip()) and ";" not in marker,
            f"PEP 508 build requirement has an empty marker: {value}",
        )
        marker_value: str | None = marker.strip()
    else:
        marker_value = None
    match = re.fullmatch(
        r"\s*(?P<name>[A-Za-z0-9][A-Za-z0-9._-]*)\s*(?P<spec>.*?)\s*",
        requirement,
    )
    _require(match is not None, f"unsupported PEP 508 build requirement: {value}")
    name = _canonical_name(match.group("name"), "pypi")
    raw_spec = match.group("spec").strip()
    _require(
        "[" not in raw_spec and "]" not in raw_spec and "@" not in raw_spec,
        f"build requirements with extras or direct URLs are unsupported: {value}",
    )
    _pep517_v1_specifier(raw_spec, require_nonempty=False)
    return name, raw_spec, marker_value


def _pyproject_build_requirements(
    manifest_data: Any,
) -> list[tuple[str, str, str | None]]:
    manifest = _toml_value(manifest_data, label="pyproject.toml")
    build_system = manifest.get("build-system")
    _require(isinstance(build_system, Mapping), "pyproject.toml build-system must be a table")
    _require(
        set(build_system) <= {"requires", "build-backend", "backend-path"},
        "pyproject.toml build-system has unsupported fields",
    )
    requires = build_system.get("requires")
    _require(
        isinstance(requires, list) and bool(requires),
        "pyproject.toml build-system.requires must be a nonempty list",
    )
    _require(
        len(requires) == len(set(requires))
        if all(isinstance(item, str) for item in requires)
        else False,
        "pyproject.toml build-system.requires must contain unique strings",
    )
    backend = build_system.get("build-backend")
    _require(
        backend is None or (isinstance(backend, str) and bool(backend.strip())),
        "pyproject.toml build-system.build-backend must be nonempty when present",
    )
    backend_path = build_system.get("backend-path")
    if backend_path is not None:
        _require(
            isinstance(backend_path, list)
            and bool(backend_path)
            and all(
                isinstance(item, str)
                and bool(item)
                and not pathlib.PurePosixPath(item).is_absolute()
                and ".." not in pathlib.PurePosixPath(item).parts
                for item in backend_path
            ),
            "pyproject.toml build-system.backend-path must contain safe relative paths",
        )
    return [_pep517_v1_build_requirement(item) for item in requires]


def _uv_dependency_context(
    lockfile_data: bytes | str | pathlib.Path,
    manifest_data: Any | None = None,
    *,
    include_build_scope: bool = False,
) -> dict[tuple[str, str], dict[str, Any]]:
    """Derive authenticated scope context for every registry identity in uv.lock."""

    _require(
        not include_build_scope or manifest_data is not None,
        "Python build-scoped dependency context requires its authenticated pyproject.toml",
    )
    packages = _uv_lock_packages(lockfile_data)
    roots = [
        index
        for index, package in enumerate(packages)
        if isinstance(package.get("source"), Mapping) and package["source"].get("editable") == "."
    ]
    _require(
        len(roots) == 1,
        "uv lockfile must contain exactly one editable root package at '.'",
    )

    by_name: dict[str, list[int]] = {}
    node_signatures: list[tuple[str, str, str]] = []
    registry_identities: dict[int, tuple[str, str]] = {}
    for index, package in enumerate(packages):
        name = package.get("name")
        _require(
            isinstance(name, str) and bool(name),
            "uv lock package requires a nonempty name",
        )
        canonical_name = _canonical_name(name, "pypi")
        source = package.get("source")
        _require(isinstance(source, Mapping), f"uv package {name} source is invalid")
        version = package.get("version")
        if "registry" in source:
            _require(
                isinstance(version, str) and bool(version),
                f"uv registry package {name} requires a version",
            )
            signature = (
                json.dumps(dict(source), sort_keys=True, separators=(",", ":")),
                canonical_name,
                version,
            )
            registry_identities[index] = (canonical_name, version)
        else:
            _require(
                version is None or (isinstance(version, str) and bool(version)),
                f"uv local package {name} version is invalid",
            )
            signature = (
                json.dumps(dict(source), sort_keys=True, separators=(",", ":")),
                canonical_name,
                version or "",
            )
        node_signatures.append(signature)
        by_name.setdefault(canonical_name, []).append(index)

    def resolve_edge(edge: Any, *, label: str) -> list[int]:
        _require(isinstance(edge, Mapping), f"{label} must be an object")
        name = edge.get("name")
        _require(
            isinstance(name, str) and bool(name),
            f"{label} requires a nonempty package name",
        )
        candidates = list(by_name.get(_canonical_name(name, "pypi"), []))
        version = edge.get("version")
        if "version" in edge:
            _require(
                isinstance(version, str) and bool(version),
                f"{label} version must be a nonempty string",
            )
            candidates = [
                index for index in candidates if packages[index].get("version") == version
            ]
        edge_source = edge.get("source")
        if "source" in edge:
            _require(isinstance(edge_source, Mapping), f"{label} source must be an object")
            candidates = [
                index for index in candidates if packages[index].get("source") == edge_source
            ]
        _require(bool(candidates), f"{label} does not resolve to a uv.lock package")
        signatures = {node_signatures[index] for index in candidates}
        _require(
            len(signatures) == 1,
            f"{label} is ambiguous across uv.lock package versions or sources",
        )
        return candidates

    scopes_by_node: dict[int, set[str]] = {}
    pending: list[tuple[int, str]] = []

    def add_edges(edges: Any, scope: str, *, label: str) -> None:
        _require(isinstance(edges, list), f"{label} must be a list")
        for position, edge in enumerate(edges):
            for target in resolve_edge(edge, label=f"{label}[{position}]"):
                pending.append((target, scope))

    root = packages[roots[0]]
    add_edges(root.get("dependencies", []), "runtime", label="uv root dependencies")
    dev_dependencies = root.get("dev-dependencies", {})
    _require(
        isinstance(dev_dependencies, Mapping),
        "uv root dev-dependencies must be a table",
    )
    for group, edges in sorted(dev_dependencies.items()):
        _require(
            isinstance(group, str) and bool(group),
            "uv root development group name must be a nonempty string",
        )
        add_edges(edges, "development", label=f"uv root dev-dependencies.{group}")
    optional_dependencies = root.get("optional-dependencies", {})
    _require(
        isinstance(optional_dependencies, Mapping),
        "uv root optional-dependencies must be a table",
    )
    for group, edges in sorted(optional_dependencies.items()):
        _require(
            isinstance(group, str) and bool(group),
            "uv root optional group name must be a nonempty string",
        )
        add_edges(edges, "optional", label=f"uv root optional-dependencies.{group}")

    if manifest_data is not None:
        build_requirements = _pyproject_build_requirements(manifest_data)
        if include_build_scope:
            metadata = root.get("metadata")
            _require(
                isinstance(metadata, Mapping),
                "uv root metadata must authenticate Python build requirements",
            )
            requires_dev = metadata.get("requires-dev")
            _require(
                isinstance(requires_dev, Mapping),
                "uv root metadata.requires-dev must authenticate Python build requirements",
            )
            metadata_requirements: list[tuple[str, int, tuple[str, str, str | None]]] = []
            for group, entries in sorted(requires_dev.items()):
                _require(
                    isinstance(group, str) and bool(group) and isinstance(entries, list),
                    "uv root metadata.requires-dev groups must contain lists",
                )
                for position, entry in enumerate(entries):
                    _require(
                        isinstance(entry, Mapping),
                        f"uv root metadata.requires-dev.{group}[{position}] must be an object",
                    )
                    _require(
                        set(entry) <= {"name", "specifier", "marker"},
                        f"uv root metadata.requires-dev.{group}[{position}] has unsupported fields",
                    )
                    name = entry.get("name")
                    specifier = entry.get("specifier", "")
                    marker = entry.get("marker")
                    _require(
                        isinstance(name, str)
                        and bool(name)
                        and isinstance(specifier, str)
                        and (marker is None or (isinstance(marker, str) and bool(marker))),
                        f"uv root metadata.requires-dev.{group}[{position}] is invalid",
                    )
                    metadata_requirements.append(
                        (
                            group,
                            position,
                            (_canonical_name(name, "pypi"), specifier, marker),
                        )
                    )

            build_root_indices: list[int] = []
            for requirement in build_requirements:
                name, specifier, marker = requirement
                _require(
                    marker is None,
                    f"build-scoped PEP 517 v1 requirements must be marker-free: {requirement}",
                )
                _pep517_v1_specifier(specifier, require_nonempty=True)
                same_name_metadata = [
                    locked_requirement
                    for _group, _position, locked_requirement in metadata_requirements
                    if locked_requirement[0] == name
                ]
                _require(
                    len(same_name_metadata) == 1,
                    "build-scoped PEP 517 v1 package names must have exactly one "
                    f"uv.lock metadata.requires-dev row: {name}",
                )
                matches = [
                    (group, position)
                    for group, position, locked_requirement in metadata_requirements
                    if locked_requirement == requirement
                ]
                _require(
                    len(matches) == 1,
                    "pyproject build requirement must have one exact uv.lock "
                    f"metadata.requires-dev binding: {requirement}",
                )
                group, _position = matches[0]
                group_edges = dev_dependencies.get(group)
                _require(
                    isinstance(group_edges, list),
                    f"uv root dev-dependencies.{group} must bind its build requirement",
                )
                selected_edges = [
                    (position, edge)
                    for position, edge in enumerate(group_edges)
                    if isinstance(edge, Mapping)
                    and isinstance(edge.get("name"), str)
                    and _canonical_name(edge["name"], "pypi") == name
                ]
                _require(
                    bool(selected_edges),
                    "pyproject build requirement has no corresponding exact uv.lock "
                    f"dev-dependency edge: {requirement}",
                )
                for position, edge in selected_edges:
                    label = f"uv root dev-dependencies.{group}[{position}] build binding"
                    _require(
                        isinstance(edge, Mapping),
                        f"{label} must be an object",
                    )
                    _require(
                        "marker" not in edge,
                        f"{label} must be marker-free under the PEP 517 v1 contract",
                    )
                    _require(
                        set(edge) <= {"name", "version", "source"},
                        f"{label} has unsupported fields under the PEP 517 v1 contract",
                    )
                    if "source" in edge:
                        _require(
                            edge["source"] == {"registry": PINNED_PYPI_REGISTRY},
                            f"{label} source must be the pinned official PyPI registry",
                        )
                    resolved = resolve_edge(edge, label=label)
                    _require(
                        len(resolved) == 1,
                        f"{label} must resolve to exactly one uv.lock package record",
                    )
                    index = resolved[0]
                    package = packages[index]
                    _require(
                        index in registry_identities
                        and package.get("source") == {"registry": PINNED_PYPI_REGISTRY},
                        f"{label} must resolve to the pinned official PyPI registry",
                    )
                    locked_version = package.get("version")
                    _require(
                        _pep517_v1_version_satisfies(locked_version, specifier),
                        f"{label} resolved version {locked_version!r} does not satisfy "
                        f"the authenticated build specifier {specifier!r}",
                    )
                    build_root_indices.append(index)

            _require(
                all(index in registry_identities for index in build_root_indices),
                "pyproject build requirements must resolve to registry packages in uv.lock",
            )
            pending.extend((index, "build") for index in build_root_indices)

    visited: set[tuple[int, str]] = set()
    while pending:
        index, scope = pending.pop()
        if (index, scope) in visited:
            continue
        visited.add((index, scope))
        scopes_by_node.setdefault(index, set()).add(scope)
        package = packages[index]
        add_edges(
            package.get("dependencies", []),
            scope,
            label=f"uv package {package['name']} dependencies",
        )

    scopes_by_identity: dict[tuple[str, str], set[str]] = {
        identity: set() for identity in _uv_packages(lockfile_data)
    }
    for index, identity in registry_identities.items():
        scopes_by_identity[identity].update(scopes_by_node.get(index, set()))
    unreachable = sorted(identity for identity, scopes in scopes_by_identity.items() if not scopes)
    _require(
        not unreachable,
        f"uv.lock contains registry packages unreachable from the editable root: {unreachable}",
    )
    scope_order = ("runtime", "build", "development", "optional")
    return {
        identity: {
            "dependency_scopes": [scope for scope in scope_order if scope in scopes],
            "dev_only": scopes == {"development"},
        }
        for identity, scopes in scopes_by_identity.items()
    }


def verify_pip_audit_coverage(
    lockfile_data: bytes | str,
    reports: Sequence[dict[str, Any]] | dict[str, Any],
    manifest_data: Any | None = None,
    *,
    include_build_scope: bool = False,
) -> None:
    """Require pip-audit coverage for every registry name/version in a uv lock.

    Host marker evaluation is never accepted as proof of full lock coverage.
    """

    expected = set(
        _uv_dependency_context(
            lockfile_data,
            manifest_data,
            include_build_scope=include_build_scope,
        )
    )
    values: Any = reports
    batches = _pip_report_objects(values)
    audited: set[tuple[str, str]] = set()
    for batch in batches:
        dependencies = batch.get("dependencies")
        _require(isinstance(dependencies, list), "pip-audit dependencies must be a list")
        for dependency in dependencies:
            _require(
                isinstance(dependency, Mapping),
                "pip-audit dependency must be an object",
            )
            name, version = dependency.get("name"), dependency.get("version")
            _require(
                isinstance(name, str) and isinstance(version, str),
                "pip-audit dependency requires name/version",
            )
            audited.add((_canonical_name(name, "pypi"), version))
    missing = expected - audited
    extra = audited - expected
    _require(
        not missing,
        f"pip-audit did not cover all uv.lock variants; missing: {sorted(missing)}",
    )
    _require(not extra, f"pip-audit reported packages absent from uv.lock: {sorted(extra)}")


def _finding_sort_key(item: Mapping[str, Any]) -> tuple[str, ...]:
    package = item.get("package", {})
    return (
        str(item.get("ecosystem", "")),
        str(item.get("project_id", "")),
        str(item.get("kind", "")),
        str(item.get("finding_id", "")),
        str(package.get("name", "")) if isinstance(package, Mapping) else "",
        str(package.get("version", "")) if isinstance(package, Mapping) else "",
    )


def _dedupe_findings(findings: Iterable[dict[str, Any]]) -> list[dict[str, Any]]:
    result: dict[tuple[str, ...], dict[str, Any]] = {}
    for finding in findings:
        key = _exception_identity(finding)
        if key in result and result[key] != finding:
            raise PolicyError(f"ambiguous normalized finding identity: {key}")
        result[key] = finding
    return list(result.values())


def evaluate_findings(
    policy: Mapping[str, Any],
    findings: Sequence[Mapping[str, Any]],
    today: str | dt.date,
) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    """Apply exact reviewed exceptions to normalized findings."""

    day = _today(today)
    _validate_policy(policy, day, require_active=False)
    exceptions = policy.get("exceptions", [])
    by_identity = {_exception_identity(item): item for item in exceptions}
    used: set[str] = set()
    evaluated: list[dict[str, Any]] = []
    for raw_finding in findings:
        finding = dict(raw_finding)
        identity = _exception_identity(finding)
        exception = by_identity.get(identity)
        cargo_graph_matches = exception is not None and (
            finding.get("ecosystem") != "cargo"
            or (
                isinstance(finding.get("cargo_graph_sha256"), str)
                and finding.get("cargo_graph_sha256") == exception.get("cargo_graph_sha256")
            )
        )
        observed_context_required = finding.get("ecosystem") in {"npm", "pypi"}
        observed_scopes = finding.get("dependency_scopes")
        scopes_are_valid = (
            isinstance(observed_scopes, list)
            and bool(observed_scopes)
            and all(isinstance(scope, str) for scope in observed_scopes)
            and len(observed_scopes) == len(set(observed_scopes))
            and set(observed_scopes) <= SCOPES
        )
        scopes_match = exception is not None and (
            (not observed_context_required and "dependency_scopes" not in finding)
            or (
                scopes_are_valid
                and set(observed_scopes) == set(exception.get("dependency_scopes", []))
            )
        )
        dev_only_match = exception is not None and (
            (not observed_context_required and "dev_only" not in finding)
            or (
                isinstance(finding.get("dev_only"), bool)
                and finding.get("dev_only") == exception.get("dev_only")
            )
        )
        if (
            exception is not None
            and finding.get("scanner_severity") == exception.get("scanner_severity")
            and cargo_graph_matches
            and scopes_match
            and dev_only_match
            and day < _parse_date(exception["expires_on"], "expires_on")
        ):
            if not observed_context_required:
                finding.setdefault("dependency_scopes", exception["dependency_scopes"])
                finding.setdefault("dev_only", exception["dev_only"])
            finding.update(
                {
                    "affected_surfaces": exception["affected_surfaces"],
                    "reachability": exception["reachability"],
                    "owner": exception["owner"],
                    "tracking_issue": exception["tracking_issue"],
                    "status": "excepted",
                    "exception_id": exception["exception_id"],
                }
            )
            used.add(str(exception["exception_id"]))
        else:
            project = next(
                (
                    item
                    for item in policy["projects"]
                    if item["project_id"] == finding.get("project_id")
                ),
                {},
            )
            finding.setdefault(
                "dependency_scopes",
                project.get("default_dependency_scopes", ["runtime"]),
            )
            finding.setdefault("dev_only", False)
            finding.setdefault("affected_surfaces", project.get("affected_surfaces", ["unknown"]))
            finding.setdefault(
                "reachability",
                "unknown" if finding.get("kind") == "vulnerability" else "not_applicable",
            )
            finding.setdefault("owner", project.get("owner", "release-engineering"))
            finding["status"] = "blocked"
            finding["exception_id"] = None
        evaluated.append(finding)

    exception_results: list[dict[str, Any]] = []
    for exception in exceptions:
        exception_id = str(exception["exception_id"])
        expires = _parse_date(exception["expires_on"], "expires_on")
        if day >= expires:
            status = "expired"
        elif exception_id in used:
            status = "active"
        else:
            status = "unused"
        exception_results.append(
            {
                "exception_id": exception_id,
                "status": status,
                "matched_finding_count": 1 if exception_id in used else 0,
            }
        )
    return sorted(evaluated, key=_finding_sort_key), sorted(
        exception_results, key=lambda item: item["exception_id"]
    )


def _component_hashes(component: Mapping[str, Any]) -> set[str]:
    result: set[str] = set()
    hashes = component.get("hashes", [])
    _require(isinstance(hashes, list), "CycloneDX component hashes must be a list")
    for item in hashes:
        _require(
            isinstance(item, Mapping) and set(item) == {"alg", "content"},
            "CycloneDX component hash must contain exact alg/content fields",
        )
        algorithm, content = item.get("alg"), item.get("content")
        _require(
            algorithm in CDX_HASH_ALGORITHMS and isinstance(content, str),
            "CycloneDX component hash uses an unsupported algorithm or content type",
        )
        prefix, byte_length = CDX_HASH_ALGORITHMS[str(algorithm)]
        expected_length = byte_length * 2
        _require(
            len(content) == expected_length and re.fullmatch(r"[0-9A-Fa-f]+", content) is not None,
            f"CycloneDX {algorithm} content is not an exact hexadecimal digest",
        )
        normalized = f"{prefix}:{content.lower()}"
        _require(normalized not in result, "CycloneDX component contains a duplicate hash")
        result.add(normalized)
    return result


def _cdx_component_name(component: Mapping[str, Any], ecosystem: str) -> str:
    name = component.get("name")
    _require(
        isinstance(name, str) and bool(name) and name == name.strip(),
        "CycloneDX component name must be a nonempty trimmed string",
    )
    if ecosystem != "npm":
        return name
    group = component.get("group")
    _require(
        group is None or isinstance(group, str),
        "npm CycloneDX component group must be a string when present",
    )
    if group:
        _require(
            group == group.strip()
            and group.startswith("@")
            and len(group) > 1
            and "/" not in group,
            f"npm CycloneDX component group is not an exact scope: {group!r}",
        )
        _require(
            not name.startswith("@") and "/" not in name,
            f"npm CycloneDX grouped component name is already scoped: {name!r}",
        )
        return f"{group}/{name}"
    if name.startswith("@"):
        parts = name.split("/")
        _require(
            len(parts) == 2 and len(parts[0]) > 1 and bool(parts[1]),
            f"npm CycloneDX component has a malformed scoped name: {name!r}",
        )
    else:
        _require(
            "/" not in name,
            f"npm CycloneDX ungrouped component has a malformed name: {name!r}",
        )
    return name


def _component_evidence_hashes(component: Mapping[str, Any], *, ecosystem: str) -> set[str]:
    direct = _component_hashes(component)
    if ecosystem != "npm":
        return direct
    references = component.get("externalReferences", [])
    _require(
        isinstance(references, list),
        "npm CycloneDX component externalReferences must be a list",
    )
    distribution: set[str] = set()
    for reference in references:
        _require(
            isinstance(reference, Mapping),
            "npm CycloneDX external reference must be an object",
        )
        reference_type = reference.get("type")
        _require(
            isinstance(reference_type, str) and bool(reference_type),
            "npm CycloneDX external reference type must be a nonempty string",
        )
        _require(
            isinstance(reference.get("url"), str) and bool(reference.get("url")),
            "npm CycloneDX external reference url must be a nonempty string",
        )
        reference_hashes = _component_hashes(reference)
        if reference_type == "distribution":
            distribution.update(reference_hashes)
    _require(
        not direct or not distribution or direct == distribution,
        "npm CycloneDX top-level and distribution hashes conflict",
    )
    return direct or distribution


def _npm_cdx_name_fields(name: str) -> dict[str, str]:
    if name.startswith("@"):
        group, package_name = name.split("/", 1)
        return {"group": group, "name": package_name}
    return {"name": name}


def _cdx_components(data: Mapping[str, Any], *, include_metadata: bool) -> list[Mapping[str, Any]]:
    result: list[Mapping[str, Any]] = []
    if include_metadata:
        metadata = data.get("metadata")
        if isinstance(metadata, Mapping) and isinstance(metadata.get("component"), Mapping):
            result.append(metadata["component"])

    def visit(values: Any) -> None:
        _require(isinstance(values, list), "CycloneDX components must be a list")
        for component in values:
            _require(isinstance(component, Mapping), "CycloneDX component must be an object")
            result.append(component)
            nested = component.get("components")
            if nested is not None:
                visit(nested)

    visit(data.get("components", []))
    return result


def _validate_reproducible_cdx(data: Mapping[str, Any], *, project_id: str) -> None:
    _require(
        "serialNumber" not in data,
        f"{project_id} stored SBOM contains volatile serialNumber",
    )
    metadata = data.get("metadata")
    if metadata is not None:
        _require(
            isinstance(metadata, Mapping),
            f"{project_id} SBOM metadata must be an object",
        )
        _require(
            "timestamp" not in metadata,
            f"{project_id} stored SBOM contains volatile metadata.timestamp",
        )


def verify_sbom(
    sbom: Any,
    expected_components: Sequence[Mapping[str, Any]] | Mapping[tuple[str, str], Any],
    *,
    project_id: str = "unknown",
    ecosystem: str | None = None,
) -> dict[str, Any]:
    """Verify a CycloneDX 1.5 inventory against exact lock identities/checksums."""

    data = _json_value(sbom, label=f"{project_id} SBOM")
    _require(isinstance(data, Mapping), f"{project_id} SBOM must be an object")
    _require(
        data.get("bomFormat") == "CycloneDX",
        f"{project_id} SBOM bomFormat must be CycloneDX",
    )
    _require(
        str(data.get("specVersion")) == "1.5",
        f"{project_id} SBOM specVersion must be 1.5",
    )
    _validate_reproducible_cdx(data, project_id=project_id)
    components = _cdx_components(data, include_metadata=ecosystem != "pypi")

    actual: dict[tuple[str, str], list[tuple[str, set[str]]]] = {}
    bom_refs: set[str] = set()
    for component in components:
        name = _cdx_component_name(component, ecosystem or "")
        version = component.get("version")
        _require(
            isinstance(version, str) and bool(version),
            f"{project_id} SBOM component lacks a nonempty name/version identity",
        )
        key = (_canonical_name(name, ecosystem or ""), version)
        bom_ref = component.get("bom-ref")
        if ecosystem == "npm":
            _require(
                isinstance(bom_ref, str) and bom_ref,
                f"{project_id} npm SBOM component lacks bom-ref: {key}",
            )
            _require(
                bom_ref not in bom_refs,
                f"{project_id} npm SBOM has duplicate bom-ref: {bom_ref}",
            )
            bom_refs.add(bom_ref)
        else:
            _require(
                key not in actual,
                f"{project_id} SBOM has duplicate component identity {key}",
            )
        actual.setdefault(key, []).append(
            (
                str(bom_ref or ""),
                _component_evidence_hashes(component, ecosystem=ecosystem or ""),
            )
        )

    expected: dict[tuple[str, str], list[tuple[str, set[str]]]] = {}
    expected_paths: set[str] = set()
    if isinstance(expected_components, Mapping):
        iterator = (
            {"name": key[0], "version": key[1], "hashes": value}
            for key, value in expected_components.items()
        )
    else:
        iterator = iter(expected_components)
    for component in iterator:
        name, version = component.get("name"), component.get("version")
        _require(
            isinstance(name, str) and isinstance(version, str),
            "expected SBOM component requires name/version",
        )
        key = (_canonical_name(name, ecosystem or ""), version)
        raw_hashes = component.get("hashes", set())
        hashes = {str(value).lower().replace("sha-256:", "sha256:") for value in raw_hashes}
        path = str(component.get("path", ""))
        if ecosystem == "npm":
            _require(
                path and path not in expected_paths,
                f"npm lockfile has duplicate or missing install path: {path}",
            )
            expected_paths.add(path)
        else:
            _require(key not in expected, f"lockfile has ambiguous component identity {key}")
        expected.setdefault(key, []).append((path, hashes))
    missing, extra = set(expected) - set(actual), set(actual) - set(expected)
    _require(not missing, f"{project_id} SBOM is missing lock components: {sorted(missing)}")
    _require(
        not extra,
        f"{project_id} SBOM contains components absent from lockfile: {sorted(extra)}",
    )
    for key, expected_occurrences in expected.items():
        actual_occurrences = actual[key]
        _require(
            len(expected_occurrences) == len(actual_occurrences),
            f"{project_id} SBOM multiplicity mismatch for {key}: "
            f"expected {len(expected_occurrences)}, got {len(actual_occurrences)}",
        )
        remaining = list(actual_occurrences)
        for path, hashes in expected_occurrences:
            match_index = next(
                (
                    index
                    for index, (_, actual_hashes) in enumerate(remaining)
                    if hashes
                    == {value.lower().replace("sha-256:", "sha256:") for value in actual_hashes}
                ),
                None,
            )
            _require(
                match_index is not None,
                f"{project_id} SBOM checksum mismatch for {key} at {path}",
            )
            remaining.pop(match_index)
    return {
        "project_id": project_id,
        "spec_version": "1.5",
        "component_count": sum(len(items) for items in expected.values()),
        "verified": True,
    }


def normalize_sbom(
    sbom: Any,
    expected_components: Sequence[Mapping[str, Any]],
    *,
    ecosystem: str,
) -> dict[str, Any]:
    """Remove volatile optional fields and enrich exact lockfile checksums.

    uv 0.11.16 emits a random ``serialNumber`` and wall-clock
    ``metadata.timestamp`` and omits artifact hashes.  The same normalization is
    safe for every generated CycloneDX document and makes the canonical JSON
    bytes reproducible.
    """

    data = _json_value(sbom, label="SBOM to normalize")
    _require(isinstance(data, Mapping), "SBOM to normalize must be an object")
    result = json.loads(json.dumps(data))
    result.pop("serialNumber", None)
    metadata = result.get("metadata")
    if isinstance(metadata, dict):
        metadata.pop("timestamp", None)

    expected: dict[tuple[str, str], set[str]] = {}
    for component in expected_components:
        name, version = component.get("name"), component.get("version")
        _require(
            isinstance(name, str) and isinstance(version, str),
            "expected component requires name/version",
        )
        key = (_canonical_name(name, ecosystem), version)
        if key in expected and ecosystem != "npm":
            raise PolicyError(f"ambiguous expected SBOM identity: {key}")
        expected.setdefault(key, set()).update(
            {
                str(value).lower().replace("sha-256:", "sha256:")
                for value in component.get("hashes", [])
            }
        )

    all_components: list[dict[str, Any]] = []
    if isinstance(metadata, dict) and isinstance(metadata.get("component"), dict):
        all_components.append(metadata["component"])
    components = result.get("components")
    _require(isinstance(components, list), "SBOM components must be a list")
    _require(
        all(isinstance(item, dict) for item in components),
        "SBOM components must be objects",
    )
    all_components.extend(components)
    for component in all_components:
        version = component.get("version")
        if not isinstance(version, str):
            continue
        name = _cdx_component_name(component, ecosystem)
        hashes = expected.get((_canonical_name(name, ecosystem), version))
        if hashes is None:
            continue
        supplied = {
            value.lower().replace("sha-256:", "sha256:")
            for value in _component_evidence_hashes(component, ecosystem=ecosystem)
        }
        if supplied:
            _require(
                supplied == hashes,
                f"SBOM generator supplied conflicting checksums for {(name, version)}",
            )
        elif hashes and ecosystem == "pypi":
            component["hashes"] = [
                {"alg": "SHA-256", "content": value.removeprefix("sha256:")}
                for value in sorted(hashes)
            ]

    if ecosystem == "npm":
        expected_occurrences: dict[tuple[str, str], list[Mapping[str, Any]]] = {}
        actual_occurrences: dict[tuple[str, str], int] = {}
        for component in expected_components:
            name, version = component.get("name"), component.get("version")
            if isinstance(name, str) and isinstance(version, str):
                expected_occurrences.setdefault((name, version), []).append(component)
        for component in all_components:
            version = component.get("version")
            if isinstance(version, str):
                key = (_cdx_component_name(component, ecosystem), version)
                actual_occurrences[key] = actual_occurrences.get(key, 0) + 1
        for key, occurrences in sorted(expected_occurrences.items()):
            deficit = len(occurrences) - actual_occurrences.get(key, 0)
            if deficit <= 0:
                continue
            inferred = [item for item in occurrences if item.get("inferred_version") is True]
            for item in inferred[:deficit]:
                path = str(item["path"])
                components.append(
                    {
                        "type": "library",
                        "bom-ref": "urn:ferric:npm-lock:"
                        + hashlib.sha256(path.encode("utf-8")).hexdigest(),
                        **_npm_cdx_name_fields(str(item["name"])),
                        "version": str(item["version"]),
                        "properties": [
                            {"name": "ferric:npm-lock-path", "value": path},
                            {
                                "name": "ferric:npm-version-source",
                                "value": "root.optionalDependencies",
                            },
                        ],
                    }
                )

    def component_key(component: Mapping[str, Any]) -> tuple[str, str, str]:
        return (
            _cdx_component_name(component, ecosystem),
            str(component.get("version", "")),
            str(component.get("bom-ref", "")),
        )

    components.sort(key=component_key)
    dependencies = result.get("dependencies")
    if isinstance(dependencies, list):
        for dependency in dependencies:
            if isinstance(dependency, dict) and isinstance(dependency.get("dependsOn"), list):
                dependency["dependsOn"].sort()
        dependencies.sort(
            key=lambda item: str(item.get("ref", "")) if isinstance(item, Mapping) else ""
        )
    return result


def _canonical_json_bytes(value: Any) -> bytes:
    return (json.dumps(value, indent=2, sort_keys=True, ensure_ascii=False) + "\n").encode("utf-8")


def _cargo_graph_sha256(
    *,
    project_id: str,
    lockfile: str,
    lockfile_sha256: str,
    workspace_manifests_sha256: str,
    cargo_config_sha256: str,
    deny_config_sha256: str,
    dependency_groups: str,
    targets: str,
    features: str,
) -> str:
    """Hash the complete reviewed Cargo exception graph context without I/O."""

    hashes = {
        "lockfile_sha256": lockfile_sha256,
        "workspace_manifests_sha256": workspace_manifests_sha256,
        "cargo_config_sha256": cargo_config_sha256,
        "deny_config_sha256": deny_config_sha256,
    }
    for field, digest in hashes.items():
        _require(
            isinstance(digest, str) and SHA256_RE.fullmatch(digest) is not None,
            f"Cargo graph {field} must be exact lowercase SHA-256",
        )
    _require(project_id == "rust-workspace", "Cargo graph project_id must be rust-workspace")
    _require(lockfile == "Cargo.lock", "Cargo graph lockfile must be Cargo.lock")
    for field, value in {
        "dependency_groups": dependency_groups,
        "targets": targets,
        "features": features,
    }.items():
        _require(value == "all", f"Cargo graph {field} must be all")
    payload = {
        "schema": CARGO_EXCEPTION_GRAPH_CONTEXT_SCHEMA,
        "version": 1,
        "project_id": project_id,
        "lockfile": lockfile,
        **hashes,
        "dependency_groups": dependency_groups,
        "targets": targets,
        "features": features,
    }
    return _sha256_bytes(_canonical_json_bytes(payload))


def _repository_cargo_graph_sha256(
    repo_root: pathlib.Path,
    project: Mapping[str, Any],
) -> str:
    """Derive the Cargo graph digest from validated, lexical repository inputs."""

    _require(project.get("ecosystem") == "cargo", "Cargo graph project must use Cargo")
    lockfile = _lexical_regular_repo_file(
        repo_root,
        project.get("lockfile"),
        label="Cargo graph lockfile",
    )
    deny_config = _lexical_regular_repo_file(
        repo_root,
        "deny.toml",
        label="Cargo graph deny config",
    )
    validate_deny_config(deny_config)
    return _cargo_graph_sha256(
        project_id=str(project.get("project_id")),
        lockfile=str(project.get("lockfile")),
        lockfile_sha256=_sha256_file(lockfile),
        workspace_manifests_sha256=_cargo_workspace_manifest_contract(repo_root),
        cargo_config_sha256=_validate_cargo_repository_config(repo_root),
        deny_config_sha256=_sha256_file(deny_config),
        dependency_groups=str(project.get("dependency_groups")),
        targets=str(project.get("targets")),
        features=str(project.get("features")),
    )


def _cargo_components(lockfile_data: bytes) -> list[dict[str, Any]]:
    try:
        lock = tomllib.loads(lockfile_data.decode("utf-8"))
    except (UnicodeDecodeError, tomllib.TOMLDecodeError) as error:
        raise PolicyError(f"invalid Cargo.lock: {error}") from error
    packages = lock.get("package")
    _require(isinstance(packages, list), "Cargo.lock package array is missing")
    result = []
    for package in packages:
        _require(isinstance(package, Mapping), "Cargo.lock package must be an object")
        name, version = package.get("name"), package.get("version")
        _require(
            isinstance(name, str) and isinstance(version, str),
            "Cargo.lock package requires name/version",
        )
        hashes = []
        checksum = package.get("checksum")
        if isinstance(checksum, str):
            _require(
                SHA256_RE.fullmatch(checksum.lower()) is not None,
                f"invalid Cargo.lock checksum for {name} {version}",
            )
            hashes.append(f"sha256:{checksum.lower()}")
        result.append({"name": name, "version": version, "hashes": hashes})
    return result


def _cargo_lock_union_sbom(
    expected_components: Sequence[Mapping[str, Any]],
) -> dict[str, Any]:
    """Build the deterministic full-lock supplement for cargo-cyclonedx.

    cargo-cyclonedx 0.5.9 deliberately filters development dependencies even
    when invoked for every workspace member and Cargo target.  Its generated
    documents remain retained as primary tool evidence; this minimal member
    makes the complete Cargo.lock identity/checksum union explicit instead of
    silently weakening the all-groups policy contract.
    """

    components: list[dict[str, Any]] = []
    identities: set[tuple[str, str]] = set()
    for expected in sorted(
        expected_components,
        key=lambda item: (str(item.get("name", "")), str(item.get("version", ""))),
    ):
        name, version = expected.get("name"), expected.get("version")
        _require(
            isinstance(name, str) and bool(name) and isinstance(version, str) and bool(version),
            "Cargo.lock union component requires name/version",
        )
        identity = (name, version)
        _require(identity not in identities, f"ambiguous Cargo.lock union identity: {identity}")
        identities.add(identity)
        hashes: list[dict[str, str]] = []
        for value in sorted(str(item).lower() for item in expected.get("hashes", [])):
            _require(
                value.startswith("sha256:")
                and SHA256_RE.fullmatch(value.removeprefix("sha256:")) is not None,
                f"Cargo.lock union checksum is invalid for {identity}",
            )
            hashes.append(
                {
                    "alg": "SHA-256",
                    "content": value.removeprefix("sha256:"),
                }
            )
        component: dict[str, Any] = {
            "type": "library",
            "name": name,
            "version": version,
        }
        if hashes:
            component["hashes"] = hashes
        components.append(component)
    return {
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "version": 1,
        "components": components,
    }


def _integrity_hashes(value: Any) -> list[str]:
    if value is None:
        return []
    _require(isinstance(value, str), "npm integrity value must be a string when present")
    _require(bool(value.strip()), "npm integrity value must not be empty")
    result: set[str] = set()
    sri_algorithms = {
        prefix: (algorithm, byte_length)
        for algorithm, (prefix, byte_length) in CDX_HASH_ALGORITHMS.items()
    }
    for token in value.split():
        _require("-" in token, f"invalid npm integrity token: {token}")
        algorithm, encoded = token.split("-", 1)
        normalized_algorithm = algorithm.lower()
        _require(
            normalized_algorithm in sri_algorithms,
            f"unsupported npm integrity algorithm: {algorithm}",
        )
        try:
            digest_bytes = base64.b64decode(encoded, validate=True)
        except (ValueError, base64.binascii.Error):
            raise PolicyError(f"invalid npm integrity value: {token}") from None
        _, expected_length = sri_algorithms[normalized_algorithm]
        _require(
            len(digest_bytes) == expected_length,
            f"npm integrity digest has the wrong length for {algorithm}: {token}",
        )
        normalized = f"{normalized_algorithm}:{digest_bytes.hex()}"
        _require(normalized not in result, f"duplicate npm integrity token: {token}")
        result.add(normalized)
    _require(bool(result), "npm integrity value contains no supported digests")
    return sorted(result)


def _npm_components(lockfile_data: bytes) -> list[dict[str, Any]]:
    lock = _json_loads(lockfile_data, label="npm package lock")
    _require(isinstance(lock, Mapping), "npm package lock must be an object")
    packages = lock.get("packages")
    _require(isinstance(packages, Mapping), "npm package lock packages object is missing")
    root_package = packages.get("")
    _require(isinstance(root_package, Mapping), "npm package lock root package is missing")
    root_optional = root_package.get("optionalDependencies", {})
    _require(
        isinstance(root_optional, Mapping),
        "npm root optionalDependencies must be an object",
    )
    result: list[dict[str, Any]] = []
    for path, package in packages.items():
        _require(
            isinstance(path, str) and isinstance(package, Mapping),
            "npm lock package entry is invalid",
        )
        installed_name = _npm_name_from_path(path) if path else package.get("name")
        name = package.get("name") or installed_name
        version = package.get("version")
        inferred_version = False
        if not isinstance(version, str):
            _require(
                path
                and isinstance(installed_name, str)
                and path == f"node_modules/{installed_name}"
                and package.get("optional") is True,
                f"npm lock package {path!r} lacks a version that can be safely inferred",
            )
            specification = root_optional.get(installed_name)
            _require(
                isinstance(specification, str)
                and NPM_EXACT_VERSION_RE.fullmatch(specification) is not None,
                f"npm optional package {installed_name} lacks an exact root version",
            )
            if isinstance(package.get("name"), str):
                _require(
                    package["name"] == installed_name,
                    f"npm optional package name conflicts with install path: {path}",
                )
            version = specification
            inferred_version = True
        _require(
            isinstance(name, str) and bool(name) and bool(version),
            f"npm lock package {path!r} has an invalid name/version",
        )
        if (
            path
            and isinstance(installed_name, str)
            and path == f"node_modules/{installed_name}"
            and installed_name in root_optional
        ):
            specification = root_optional[installed_name]
            _require(
                isinstance(specification, str)
                and NPM_EXACT_VERSION_RE.fullmatch(specification) is not None,
                f"npm optional package {installed_name} has a non-exact root version",
            )
            _require(
                package.get("optional") is True and version == specification,
                f"npm optional package {installed_name} conflicts with its root declaration",
            )
        if "integrity" in package:
            _require(
                package["integrity"] is not None,
                f"npm lock package {path!r} has a null integrity value",
            )
        result.append(
            {
                "name": name,
                "version": version,
                "path": path or ".",
                "hashes": _integrity_hashes(package.get("integrity")),
                "inferred_version": inferred_version,
            }
        )
    _require(bool(result), "npm package lock contains no versioned packages")
    return sorted(result, key=lambda item: (item["name"], item["version"], item["path"]))


def _uv_components(lockfile_data: bytes) -> list[dict[str, Any]]:
    return [
        {"name": name, "version": version, "hashes": sorted(hashes)}
        for (name, version), hashes in sorted(_uv_packages(lockfile_data).items())
    ]


def _lock_components(ecosystem: str, lockfile_data: bytes) -> list[dict[str, Any]]:
    if ecosystem == "cargo":
        return _cargo_components(lockfile_data)
    if ecosystem == "npm":
        return _npm_components(lockfile_data)
    if ecosystem == "pypi":
        return _uv_components(lockfile_data)
    raise PolicyError(f"unsupported ecosystem: {ecosystem}")


def _safe_relative(path: Any, *, field: str) -> pathlib.PurePosixPath:
    _require(isinstance(path, str) and path, f"{field} must be a nonempty relative path")
    value = pathlib.PurePosixPath(path)
    _require(
        not value.is_absolute() and ".." not in value.parts,
        f"{field} must not escape its evidence directory",
    )
    return value


def _project_working_directory(project: Mapping[str, Any]) -> str:
    parent = pathlib.PurePosixPath(str(project["manifest"])).parent
    return "." if str(parent) == "." else str(parent)


def _expected_scan_command(project_id: str, scanner: str) -> list[str]:
    if scanner == "cargo-audit":
        return CARGO_AUDIT_ARGV
    if scanner == "cargo-deny":
        return CARGO_DENY_ARGV
    if scanner == "npm-audit":
        return NPM_AUDIT_ARGV
    if scanner == "pip-audit":
        return PIP_AUDIT_ARGV
    raise PolicyError(f"unknown scanner command: {project_id}/{scanner}")


def _expected_scan_environment(scanner: str) -> dict[str, str]:
    if scanner == "cargo-audit":
        return dict(CARGO_AUDIT_ENVIRONMENT)
    if scanner == "cargo-deny":
        return dict(CARGO_DENY_ENVIRONMENT)
    if scanner == "npm-audit":
        return dict(NPM_AUDIT_ENVIRONMENT)
    if scanner == "pip-audit":
        return dict(PIP_AUDIT_ENVIRONMENT)
    return {}


def _expected_sbom_environment(project: Mapping[str, Any]) -> dict[str, str]:
    if project["ecosystem"] == "cargo":
        return dict(CARGO_ALIAS_ENVIRONMENT)
    if project["ecosystem"] == "npm":
        return dict(NPM_AUDIT_ENVIRONMENT)
    if project["ecosystem"] == "pypi":
        return dict(UV_ENVIRONMENT)
    raise PolicyError(f"unknown SBOM ecosystem: {project['ecosystem']}")


def _expected_sbom_command(project: Mapping[str, Any]) -> list[str]:
    if project["ecosystem"] == "cargo":
        return CARGO_SBOM_ARGV
    if project["ecosystem"] == "npm":
        return NPM_SBOM_ARGV
    if project["ecosystem"] == "pypi":
        return UV_SBOM_ARGV
    raise PolicyError(f"unknown SBOM ecosystem: {project['ecosystem']}")


def _read_json_file(path: pathlib.Path) -> Any:
    _require(
        path.is_file() and not path.is_symlink(),
        f"missing or unsafe evidence file: {path}",
    )
    return _json_loads(path.read_bytes(), label=str(path))


def _validate_scan_manifest(
    manifest: Mapping[str, Any],
    *,
    policy: Mapping[str, Any],
    reports_dir: pathlib.Path,
    candidate_sha: str,
    today: str,
    repo_root: pathlib.Path,
) -> tuple[dict[str, Mapping[str, Any]], dict[str, Any]]:
    _require(
        set(manifest) == SCAN_MANIFEST_FIELDS,
        "scan manifest top-level fields differ from the locked schema",
    )
    _require(
        manifest.get("schema") == SCAN_MANIFEST_SCHEMA,
        f"scan manifest schema must be {SCAN_MANIFEST_SCHEMA}",
    )
    _require(
        isinstance(manifest.get("version"), int)
        and not isinstance(manifest.get("version"), bool)
        and manifest.get("version") == 1,
        "scan manifest version must be 1",
    )
    _require(
        manifest.get("candidate_sha") == candidate_sha,
        "scan manifest candidate_sha does not match the evaluated candidate",
    )
    _require(
        manifest.get("evaluated_on") == today,
        "scan manifest evaluated_on does not match --today",
    )
    _require(manifest.get("target_scope") == "all", "scan manifest target_scope must be all")
    _require(
        isinstance(manifest.get("source_date_epoch"), int)
        and not isinstance(manifest["source_date_epoch"], bool)
        and manifest["source_date_epoch"] >= 0,
        "scan manifest source_date_epoch must be a nonnegative integer",
    )
    _require(
        manifest.get("normalization") == EXPECTED_NORMALIZATION,
        "scan manifest normalization declaration differs from the locked contract",
    )
    _require(
        manifest.get("cargo_workspace_manifests_sha256")
        == _cargo_workspace_manifest_contract(repo_root),
        "scan manifest Cargo workspace manifest aggregate mismatch",
    )

    entries = list(reports_dir.iterdir())
    _require(
        all(entry.is_file() and not entry.is_symlink() for entry in entries),
        "raw evidence directory contains a directory, symlink, or non-regular file",
    )
    actual_names = {entry.name for entry in entries}
    _require(
        actual_names == RAW_FILENAMES,
        "raw evidence files differ from the authoritative set; "
        f"missing={sorted(RAW_FILENAMES - actual_names)} "
        f"extra={sorted(actual_names - RAW_FILENAMES)}",
    )

    raw_files = manifest.get("raw_files")
    _require(isinstance(raw_files, list), "scan manifest raw_files must be a list")
    raw_map: dict[str, str] = {}
    for item in raw_files:
        _require(isinstance(item, Mapping), "scan manifest raw_files entry must be an object")
        _require(
            set(item) == RAW_FILE_MANIFEST_FIELDS,
            "scan manifest raw file fields differ from the locked schema",
        )
        path = str(_safe_relative(item.get("path"), field="raw_files.path"))
        digest = item.get("sha256")
        _require(
            path not in raw_map and path != "scan-manifest.json",
            f"duplicate or self-referential raw file: {path}",
        )
        _require(
            isinstance(digest, str) and SHA256_RE.fullmatch(digest) is not None,
            f"invalid raw sha256 for {path}",
        )
        raw_map[path] = digest
    _require(
        set(raw_map) == RAW_FILENAMES - {"scan-manifest.json"},
        "scan manifest must enumerate every non-self raw file exactly once",
    )
    for path, digest in raw_map.items():
        _require(
            _sha256_file(reports_dir / path) == digest,
            f"raw evidence sha256 mismatch: {path}",
        )

    tool_versions = manifest.get("tool_versions")
    _require(
        tool_versions == EXPECTED_TOOL_PINS,
        "scan manifest tool_versions differ from locked pins",
    )
    _require(
        _read_json_file(reports_dir / "tool-versions.json") == EXPECTED_TOOL_PINS,
        "tool-versions.json differs from locked pins",
    )

    projects = {item["project_id"]: item for item in policy["projects"]}
    inputs = manifest.get("inputs")
    _require(isinstance(inputs, list), "scan manifest inputs must be a list")
    input_map: dict[str, Mapping[str, Any]] = {}
    for item in inputs:
        _require(isinstance(item, Mapping), "scan manifest input must be an object")
        _require(
            set(item) == INPUT_MANIFEST_FIELDS,
            "scan manifest input fields differ from the locked schema",
        )
        project_id = item.get("project_id")
        _require(
            project_id in projects and project_id not in input_map,
            f"invalid or duplicate input project_id: {project_id}",
        )
        project = projects[str(project_id)]
        _require(
            item.get("ecosystem") == project["ecosystem"],
            f"input ecosystem mismatch: {project_id}",
        )
        _require(
            item.get("manifest") == project["manifest"],
            f"input manifest mismatch: {project_id}",
        )
        manifest_digest = item.get("manifest_sha256")
        _require(
            isinstance(manifest_digest, str) and SHA256_RE.fullmatch(manifest_digest) is not None,
            f"invalid input manifest_sha256: {project_id}",
        )
        _require(
            _sha256_file(repo_root / project["manifest"]) == manifest_digest,
            f"input manifest sha256 mismatch: {project_id}",
        )
        _require(
            item.get("lockfile") == project["lockfile"],
            f"input lockfile mismatch: {project_id}",
        )
        digest = item.get("sha256")
        _require(
            isinstance(digest, str) and SHA256_RE.fullmatch(digest) is not None,
            f"invalid input sha256: {project_id}",
        )
        lock_path = repo_root / project["lockfile"]
        _require(
            _sha256_file(lock_path) == digest,
            f"input lockfile sha256 mismatch: {project_id}",
        )
        input_map[str(project_id)] = item
    _require(
        set(input_map) == set(projects),
        "scan manifest inputs must cover exactly seven projects",
    )

    scans = manifest.get("scans")
    _require(isinstance(scans, list), "scan manifest scans must be a list")
    scan_map: dict[tuple[str, str], Mapping[str, Any]] = {}
    for item in scans:
        _require(isinstance(item, Mapping), "scan manifest scan must be an object")
        key = (str(item.get("project_id", "")), str(item.get("scanner", "")))
        _require(
            key in SCAN_REPORTS and key not in scan_map,
            f"invalid or duplicate scan entry: {key}",
        )
        expected_fields = {
            "project_id",
            "scanner",
            "report",
            "command",
            "environment",
            "working_directory",
            "exit_code",
            "exit_classification",
            "raw_sha256",
            "lockfile_sha256",
            "target_scope",
        }
        if key[1] == "cargo-audit":
            expected_fields.add("advisory_database")
        _require(
            set(item) == expected_fields,
            f"scan manifest fields differ from the locked schema: {key}",
        )
        project = projects[key[0]]
        expected_report = SCAN_REPORTS[key]
        _require(item.get("report") == expected_report, f"scan report path mismatch: {key}")
        command = item.get("command")
        _require(
            command == _expected_scan_command(*key),
            f"scan command differs from locked argv: {key}",
        )
        _require(
            item.get("environment") == _expected_scan_environment(key[1]),
            f"scan environment differs from the locked isolation: {key}",
        )
        _require(
            item.get("working_directory") == _project_working_directory(project),
            f"scan working_directory mismatch: {key}",
        )
        _require(
            isinstance(item.get("exit_code"), int) and not isinstance(item.get("exit_code"), bool),
            f"scan exit_code is invalid: {key}",
        )
        _require(
            item.get("exit_classification") in {"clean", "findings", "operational-error"},
            f"scan exit_classification is invalid: {key}",
        )
        if item.get("exit_classification") == "clean":
            _require(item.get("exit_code") == 0, f"clean scan must exit zero: {key}")
        elif item.get("exit_classification") == "findings":
            supported_finding_codes = set(range(1, 16)) if key[1] == "cargo-deny" else {0, 1}
            _require(
                item.get("exit_code") in supported_finding_codes,
                f"findings scan must use a supported scanner exit code: {key}",
            )
        _require(
            item.get("raw_sha256") == raw_map[expected_report],
            f"scan raw_sha256 mismatch: {key}",
        )
        _require(
            item.get("lockfile_sha256") == input_map[key[0]]["sha256"],
            f"scan lockfile_sha256 mismatch: {key}",
        )
        _require(item.get("target_scope") == "all", f"scan target_scope must be all: {key}")
        if key[1] == "cargo-audit":
            database = item.get("advisory_database")
            _require(
                isinstance(database, Mapping) and set(database) == {"url", "commit", "fresh_fetch"},
                "cargo-audit advisory database provenance is invalid",
            )
            _require(
                database.get("url") == CARGO_AUDIT_DATABASE_URL,
                "cargo-audit advisory database URL is not canonical RustSec",
            )
            _require(
                isinstance(database.get("commit"), str)
                and SHA_RE.fullmatch(database["commit"]) is not None,
                "cargo-audit advisory database commit is invalid",
            )
            _require(
                database.get("fresh_fetch") is True,
                "cargo-audit advisory database was not fetched into an isolated empty home",
            )
        scan_map[key] = item
    _require(
        set(scan_map) == set(SCAN_REPORTS),
        "scan manifest scans must cover exactly the eight locked scans",
    )

    sboms = manifest.get("sboms")
    _require(isinstance(sboms, list), "scan manifest sboms must be a list")
    sbom_projects = set()
    sbom_paths: set[str] = set()
    for item in sboms:
        _require(isinstance(item, Mapping), "scan manifest sbom entry must be an object")
        _require(
            set(item) == SBOM_MANIFEST_FIELDS,
            "scan manifest SBOM fields differ from the locked schema",
        )
        project_id = item.get("project_id")
        _require(
            project_id in projects and project_id not in sbom_projects,
            f"invalid or duplicate SBOM project: {project_id}",
        )
        relative = str(_safe_relative(item.get("path"), field="sboms.path"))
        _require(
            relative == EXPECTED_SBOM_PATHS[str(project_id)] and relative not in sbom_paths,
            f"SBOM path is not the canonical unique path: {project_id}",
        )
        command = item.get("command")
        _require(
            command == _expected_sbom_command(projects[str(project_id)]),
            f"SBOM command differs from locked argv: {project_id}",
        )
        _require(
            item.get("environment") == _expected_sbom_environment(projects[str(project_id)]),
            f"SBOM environment differs from locked isolation: {project_id}",
        )
        _require(
            item.get("working_directory") == _project_working_directory(projects[str(project_id)]),
            f"SBOM working_directory mismatch: {project_id}",
        )
        _require(
            isinstance(item.get("exit_code"), int)
            and not isinstance(item.get("exit_code"), bool)
            and item.get("exit_code") == 0,
            f"SBOM generation failed: {project_id}",
        )
        _require(
            isinstance(item.get("sha256"), str) and SHA256_RE.fullmatch(item["sha256"]) is not None,
            f"SBOM sha256 is invalid: {project_id}",
        )
        _require(
            item.get("lockfile_sha256") == input_map[str(project_id)]["sha256"],
            f"SBOM lock hash mismatch: {project_id}",
        )
        _require(
            item.get("normalization") == "canonical-json-v1",
            f"SBOM normalization is not declared: {project_id}",
        )
        _require(
            item.get("source_date_epoch") == manifest["source_date_epoch"],
            f"SBOM source_date_epoch mismatch: {project_id}",
        )
        sbom_projects.add(str(project_id))
        sbom_paths.add(relative)
    _require(
        sbom_projects == set(projects),
        "scan manifest must cover exactly seven logical SBOMs",
    )

    license_notice = manifest.get("license_notice")
    _require(
        isinstance(license_notice, Mapping),
        "scan manifest license_notice must be an object",
    )
    _require(
        set(license_notice) == LICENSE_NOTICE_MANIFEST_FIELDS,
        "scan manifest license notice fields differ from the locked schema",
    )
    _require(
        license_notice.get("path") == "license-notices.json",
        "license notice path is invalid",
    )
    _require(
        license_notice.get("command") == LICENSE_NOTICE_ARGV,
        "license notice command is invalid",
    )
    _require(
        license_notice.get("working_directory") == ".",
        "license notice working_directory is invalid",
    )
    _require(
        license_notice.get("sha256") == raw_map["license-notices.json"],
        "license notice hash mismatch",
    )
    _require(
        isinstance(license_notice.get("exit_code"), int)
        and not isinstance(license_notice.get("exit_code"), bool)
        and license_notice.get("exit_code") == 0
        and license_notice.get("status") == "pass",
        "license notice manifest disposition failed",
    )
    license_contract_hashes = _license_contract_hashes(repo_root)
    for field, digest in license_contract_hashes.items():
        _require(
            license_notice.get(field) == digest,
            f"license notice manifest {field} mismatch",
        )
    return scan_map, {
        "inputs": input_map,
        "sboms": sboms,
        "license_notice": license_notice,
    }


def _verify_rust_sbom(
    manifest_path: pathlib.Path,
    expected: list[dict[str, Any]],
    *,
    candidate_sha: str,
    lockfile_sha256: str,
    sbom_dir: pathlib.Path,
) -> dict[str, Any]:
    manifest = _read_json_file(manifest_path)
    _require(isinstance(manifest, Mapping), "Rust SBOM manifest must be an object")
    _require(
        set(manifest)
        == {
            "schema",
            "version",
            "project_id",
            "candidate_sha",
            "lockfile_sha256",
            "members",
        },
        "Rust SBOM manifest fields differ from the locked schema",
    )
    _require(
        manifest.get("schema") == RUST_SBOM_MANIFEST_SCHEMA
        and isinstance(manifest.get("version"), int)
        and not isinstance(manifest.get("version"), bool)
        and manifest.get("version") == 1,
        "Rust SBOM manifest schema/version is invalid",
    )
    _require(
        manifest.get("project_id") == "rust-workspace",
        "Rust SBOM manifest project_id is invalid",
    )
    _require(
        manifest.get("candidate_sha") == candidate_sha,
        "Rust SBOM manifest candidate_sha mismatch",
    )
    _require(
        manifest.get("lockfile_sha256") == lockfile_sha256,
        "Rust SBOM manifest lockfile hash mismatch",
    )
    members = manifest.get("members")
    _require(
        isinstance(members, list) and members,
        "Rust SBOM manifest members must be nonempty",
    )
    logical: dict[tuple[str, str], dict[str, Any]] = {}
    paths: set[str] = set()
    generator_member_count = 0
    lock_union_member_count = 0
    expected_lock_union = _cargo_lock_union_sbom(expected)
    expected_lock_union_bytes = _canonical_json_bytes(expected_lock_union)
    for member in members:
        _require(isinstance(member, Mapping), "Rust SBOM member must be an object")
        _require(
            set(member) == {"kind", "path", "sha256"},
            "Rust SBOM member fields differ from the locked schema",
        )
        kind = member.get("kind")
        _require(
            kind in {RUST_SBOM_GENERATOR_KIND, RUST_SBOM_LOCK_UNION_KIND},
            "Rust SBOM member kind is invalid",
        )
        relative = _safe_relative(member.get("path"), field="Rust SBOM member path")
        _require(
            relative.parts and relative.parts[0] == "rust-workspace",
            "Rust SBOM members must be under sboms/rust-workspace/",
        )
        path_text = str(relative)
        if kind == RUST_SBOM_LOCK_UNION_KIND:
            _require(
                path_text == RUST_SBOM_LOCK_UNION_PATH,
                "Cargo.lock union member path is invalid",
            )
            lock_union_member_count += 1
        else:
            _require(
                path_text != RUST_SBOM_LOCK_UNION_PATH,
                "cargo-cyclonedx member uses the reserved Cargo.lock union path",
            )
            generator_member_count += 1
        _require(path_text not in paths, f"duplicate Rust SBOM member path: {path_text}")
        paths.add(path_text)
        path = sbom_dir / pathlib.Path(*relative.parts)
        digest = member.get("sha256")
        _require(
            isinstance(digest, str) and _sha256_file(path) == digest,
            f"Rust SBOM member sha256 mismatch: {path_text}",
        )
        data = _read_json_file(path)
        _require(
            isinstance(data, Mapping)
            and data.get("bomFormat") == "CycloneDX"
            and str(data.get("specVersion")) == "1.5",
            f"invalid Rust CycloneDX member: {path_text}",
        )
        _validate_reproducible_cdx(data, project_id=f"rust-workspace/{path_text}")
        if kind == RUST_SBOM_LOCK_UNION_KIND:
            _require(
                path.read_bytes() == expected_lock_union_bytes,
                "Cargo.lock union member is not the canonical recomputed document",
            )
            _require(
                data == expected_lock_union,
                "Cargo.lock union member differs from the deterministic locked inventory",
            )
            verify_sbom(
                data,
                expected,
                project_id="rust-workspace Cargo.lock union",
                ecosystem="cargo",
            )
        else:
            metadata = data.get("metadata")
            _require(
                isinstance(metadata, Mapping) and isinstance(metadata.get("component"), Mapping),
                f"cargo-cyclonedx member lacks a metadata subject: {path_text}",
            )
            subject = metadata["component"]
            _require(
                all(
                    isinstance(subject.get(field), str) and bool(subject[field])
                    for field in ("type", "name", "version")
                ),
                f"cargo-cyclonedx metadata subject is invalid: {path_text}",
            )
            _require(
                "components" not in subject,
                f"cargo-cyclonedx metadata subject contains hidden components: {path_text}",
            )
        # With --describe all-cargo-targets cargo-cyclonedx names the document
        # subject after the Rust target (for example ferric_rules_ffi), not the
        # Cargo package.  It is provenance metadata, not a lock component, and
        # the pinned generator emits no dependency subtree beneath it.
        values = _cdx_components(data, include_metadata=False)
        local: set[tuple[str, str]] = set()
        for component in values:
            name, version = component.get("name"), component.get("version")
            _require(
                isinstance(name, str) and bool(name) and isinstance(version, str) and bool(version),
                f"Rust SBOM component lacks a nonempty name/version: {path_text}",
            )
            key = (name, version)
            _require(
                key not in local,
                f"duplicate identity inside Rust SBOM member {path_text}: {key}",
            )
            local.add(key)
            current = {
                "name": name,
                "version": version,
                "hashes": _component_hashes(component),
            }
            previous = logical.get(key)
            if previous is not None:
                previous["hashes"] = set(previous["hashes"]) | set(current["hashes"])
            else:
                logical[key] = current
    _require(
        generator_member_count > 0,
        "Rust SBOM manifest has no cargo-cyclonedx members",
    )
    _require(
        lock_union_member_count == 1,
        "Rust SBOM manifest must contain exactly one Cargo.lock union member",
    )
    summary = verify_sbom(
        {
            "bomFormat": "CycloneDX",
            "specVersion": "1.5",
            "components": [
                {
                    "name": component["name"],
                    "version": component["version"],
                    "hashes": [
                        {
                            "alg": "SHA-256" if value.startswith("sha256:") else "SHA-512",
                            "content": value.split(":", 1)[1],
                        }
                        for value in sorted(component["hashes"])
                    ],
                }
                for component in logical.values()
            ],
        },
        expected,
        project_id="rust-workspace",
        ecosystem="cargo",
    )
    summary.update(
        {
            "path": "rust-workspace.sbom-manifest.json",
            "sha256": _sha256_file(manifest_path),
            "lockfile_sha256": lockfile_sha256,
        }
    )
    return summary


def _validate_sbom_tree(sbom_dir: pathlib.Path) -> None:
    _require(
        sbom_dir.is_dir() and not sbom_dir.is_symlink(),
        "SBOM evidence root is not a safe directory",
    )
    rust_manifest_path = sbom_dir / "rust-workspace.sbom-manifest.json"
    rust_manifest = _read_json_file(rust_manifest_path)
    _require(isinstance(rust_manifest, Mapping), "Rust SBOM manifest must be an object")
    members = rust_manifest.get("members")
    _require(isinstance(members, list), "Rust SBOM manifest members must be a list")
    declared = {
        "rust-workspace.sbom-manifest.json",
        "node-package.cdx.json",
        "node-addon.cdx.json",
        "documentation.cdx.json",
        "site.cdx.json",
        "python-package.cdx.json",
        "python-tools.cdx.json",
    }
    for member in members:
        _require(isinstance(member, Mapping), "Rust SBOM member must be an object")
        declared.add(str(_safe_relative(member.get("path"), field="Rust SBOM member path")))
    actual: set[str] = set()
    for entry in sbom_dir.rglob("*"):
        _require(not entry.is_symlink(), f"SBOM evidence contains a symlink: {entry}")
        if entry.is_dir():
            _require(
                entry == sbom_dir / "rust-workspace",
                f"SBOM evidence contains an unexpected directory: {entry}",
            )
        else:
            _require(entry.is_file(), f"SBOM evidence contains a non-regular entry: {entry}")
            actual.add(entry.relative_to(sbom_dir).as_posix())
    _require(
        actual == declared,
        "SBOM evidence files differ from manifest; "
        f"missing={sorted(declared - actual)} extra={sorted(actual - declared)}",
    )


def _validate_license_evidence(
    value: Any,
    *,
    candidate_sha: str,
    today: str,
    repo_root: pathlib.Path,
) -> dict[str, Any]:
    _require(isinstance(value, Mapping), "license-notices.json must be an object")
    expected_fields = {
        "schema",
        "version",
        "candidate_sha",
        "evaluated_on",
        "command",
        "working_directory",
        "exit_code",
        "status",
        "path",
        "sha256",
        "cargo_about",
        "about_config_sha256",
        "template_sha256",
        "script_sha256",
        "cargo_lock_sha256",
    }
    _require(
        set(value) == expected_fields,
        "license-notices.json fields differ from the locked schema",
    )
    _require(
        value.get("schema") == "ferric.license-notices-evidence"
        and isinstance(value.get("version"), int)
        and not isinstance(value.get("version"), bool)
        and value.get("version") == 1,
        "license notice schema/version is invalid",
    )
    _require(
        value.get("candidate_sha") == candidate_sha,
        "license notice candidate_sha mismatch",
    )
    _require(value.get("evaluated_on") == today, "license notice evaluated_on mismatch")
    _require(
        value.get("command") == LICENSE_NOTICE_ARGV,
        "license notice command mismatch",
    )
    _require(
        value.get("working_directory") == ".",
        "license notice working_directory mismatch",
    )
    _require(
        isinstance(value.get("exit_code"), int)
        and not isinstance(value.get("exit_code"), bool)
        and value.get("exit_code") == 0
        and value.get("status") == "pass",
        "license notice check failed",
    )
    _require(
        value.get("path") == "THIRD_PARTY_NOTICES.md",
        "license notice candidate path mismatch",
    )
    _require(
        value.get("cargo_about") == EXPECTED_TOOL_PINS["cargo_about"],
        "license notice cargo-about version mismatch",
    )
    _require(
        isinstance(value.get("sha256"), str) and SHA256_RE.fullmatch(value["sha256"]) is not None,
        "license notice sha256 is invalid",
    )
    license_contract_hashes = _license_contract_hashes(repo_root)
    for field, digest in license_contract_hashes.items():
        _require(value.get(field) == digest, f"license notice {field} mismatch")
    notices_path = repo_root / "THIRD_PARTY_NOTICES.md"
    _require(
        _sha256_file(notices_path) == value["sha256"],
        "license notice candidate hash mismatch",
    )
    return dict(value)


def _stage_report_file(path: pathlib.Path, content: bytes) -> pathlib.Path:
    temporary_path: pathlib.Path | None = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="wb",
            dir=path.parent,
            prefix=f".{path.name}.",
            suffix=".tmp",
            delete=False,
        ) as stream:
            temporary_path = pathlib.Path(stream.name)
            stream.write(content)
            stream.flush()
            os.fsync(stream.fileno())
        return temporary_path
    except BaseException:
        if temporary_path is not None:
            temporary_path.unlink(missing_ok=True)
        raise


def _write_report(output_dir: pathlib.Path, report: Mapping[str, Any]) -> None:
    output_dir.mkdir(parents=True, exist_ok=True)
    lines = [
        "# Dependency Policy",
        "",
        f"- Candidate: `{report['candidate_sha']}`",
        f"- Evaluated: `{report['evaluated_on']}`",
        f"- Verdict: **{str(report['verdict']).upper()}**",
        f"- Findings: {len(report['findings'])}",
        f"- Errors: {len(report['errors'])}",
        "",
        "| Ecosystem | Project | Kind | Finding | Package | Status | Exception |",
        "| --- | --- | --- | --- | --- | --- | --- |",
    ]
    for finding in report["findings"]:
        package = finding["package"]
        lines.append(
            (
                "| {ecosystem} | {project_id} | {kind} | {finding_id} | "
                "{name} {version} | {status} | {exception} |"
            ).format(
                **finding,
                name=package["name"],
                version=package["version"],
                exception=finding.get("exception_id", ""),
            )
        )
    if report["errors"]:
        lines.extend(["", "## Errors", ""])
        lines.extend(f"- `{error['code']}`: {error['message']}" for error in report["errors"])
    json_path = output_dir / "dependency-policy-report.json"
    markdown_path = output_dir / "dependency-policy-report.md"
    json_temporary = _stage_report_file(json_path, _canonical_json_bytes(report))
    markdown_temporary: pathlib.Path | None = None
    try:
        markdown_temporary = _stage_report_file(
            markdown_path,
            ("\n".join(lines) + "\n").encode("utf-8"),
        )
        os.replace(markdown_temporary, markdown_path)
        markdown_temporary = None
        # JSON is the authoritative verdict. Replace it last so the initialized
        # failure remains authoritative until the complete report is staged.
        os.replace(json_temporary, json_path)
        json_temporary = None
    finally:
        if markdown_temporary is not None:
            markdown_temporary.unlink(missing_ok=True)
        if json_temporary is not None:
            json_temporary.unlink(missing_ok=True)


def evaluate_evidence(
    *,
    policy_path: pathlib.Path,
    reports_dir: pathlib.Path,
    sbom_dir: pathlib.Path,
    candidate_sha: str,
    today: str,
    output_dir: pathlib.Path,
) -> dict[str, Any]:
    _require(
        SHA_RE.fullmatch(candidate_sha) is not None,
        "candidate_sha must be exact lowercase 40-hex",
    )
    day = _parse_date(today, "today").isoformat()
    policy_path, repo_root = _policy_repository_path(policy_path)
    policy = load_policy(policy_path)
    errors: list[dict[str, Any]] = []
    try:
        _validate_policy(policy, day, require_active=False)
    except PolicyError as error:
        errors.append({"code": "policy-invalid", "message": str(error), "project_id": None})

    report: dict[str, Any] = {
        "schema": REPORT_SCHEMA,
        "version": 1,
        "candidate_sha": candidate_sha,
        "evaluated_on": day,
        "tool_versions": {},
        "inputs": [],
        "findings": [],
        "exceptions": [],
        "sboms": [],
        "license_notice": {"status": "fail"},
        "errors": errors,
        "verdict": "fail",
    }
    output_dir.mkdir(parents=True, exist_ok=True)
    policy_copy = output_dir / "dependency-policy.json"
    deny_copy = output_dir / "deny.toml"
    if pathlib.Path(os.path.abspath(policy_copy)) != policy_path:
        shutil.copyfile(policy_path, policy_copy)
    source_deny = repo_root / "deny.toml"
    if source_deny.resolve() != deny_copy.resolve() and source_deny.is_file():
        shutil.copyfile(source_deny, deny_copy)
    if errors:
        _write_report(output_dir, report)
        return report
    try:
        deny_path = repo_root / "deny.toml"
        validate_deny_config(deny_path)
        cargo_graph_sha256 = _validate_project_files(repo_root, policy)
        manifest_data = _read_json_file(reports_dir / "scan-manifest.json")
        _require(isinstance(manifest_data, Mapping), "scan-manifest.json must be an object")
        scans, manifest_parts = _validate_scan_manifest(
            manifest_data,
            policy=policy,
            reports_dir=reports_dir,
            candidate_sha=candidate_sha,
            today=day,
            repo_root=repo_root,
        )
        report["tool_versions"] = dict(manifest_data["tool_versions"])
        _require(
            manifest_data.get("policy_sha256") == _sha256_file(policy_path),
            "scan manifest policy sha256 mismatch",
        )
        _require(
            manifest_data.get("deny_config_sha256") == _sha256_file(deny_path),
            "scan manifest deny.toml sha256 mismatch",
        )
        _require(
            manifest_data.get("cargo_config_sha256")
            == _validate_cargo_repository_config(repo_root),
            "scan manifest Cargo config sha256 mismatch",
        )
        _require(
            manifest_data.get("cargo_workspace_manifests_sha256")
            == _cargo_workspace_manifest_contract(repo_root),
            "scan manifest Cargo workspace manifest aggregate mismatch",
        )
        input_map = manifest_parts["inputs"]
        report["inputs"] = [
            {
                "project_id": project_id,
                "ecosystem": policy_project["ecosystem"],
                "manifest": policy_project["manifest"],
                "manifest_sha256": item["manifest_sha256"],
                "lockfile": item["lockfile"],
                "sha256": item["sha256"],
            }
            for project_id, item in sorted(input_map.items())
            for policy_project in [
                next(
                    project for project in policy["projects"] if project["project_id"] == project_id
                )
            ]
        ]
        projects = {item["project_id"]: item for item in policy["projects"]}
        findings: list[dict[str, Any]] = []
        cargo_audit_canonical: list[dict[str, Any]] = []
        cargo_deny_canonical: list[dict[str, Any]] = []
        for key, filename in SCAN_REPORTS.items():
            scan = scans[key]
            if scan["exit_classification"] == "operational-error":
                raise PolicyError(f"scanner operational failure: {key}")
            raw = (reports_dir / filename).read_bytes()
            project = projects[key[0]]
            if key[1] == "cargo-audit":
                _validate_cargo_audit_evidence(
                    raw,
                    cargo_lock=repo_root / project["lockfile"],
                    advisory_commit=scan["advisory_database"]["commit"],
                )
                normalized = normalize_cargo_audit(raw, project)
                cargo_audit_canonical = normalized
                reported_finding_count = len(normalized)
            elif key[1] == "cargo-deny":
                normalized = normalize_cargo_deny(raw, project, require_summary=True)
                cargo_deny_canonical = cargo_deny_canonical_advisory_findings(
                    raw, project, require_summary=True
                )
                reported_finding_count = cargo_deny_reported_finding_count(
                    raw, project, require_summary=True
                )
                observed_classification = _classify_cargo_deny_scan(
                    scan["exit_code"], raw, reported_finding_count
                )
                _require(
                    observed_classification == scan["exit_classification"],
                    "cargo-deny exit code/classification does not match its terminal summary",
                )
            elif key[1] == "npm-audit":
                normalized = normalize_npm_audit(
                    raw,
                    project,
                    repo_root / project["lockfile"],
                    repo_root / project["manifest"],
                )
                reported_finding_count = len(normalized)
            elif key[1] == "pip-audit":
                parsed = _validate_pip_audit_evidence(raw)
                verify_pip_audit_coverage(
                    (repo_root / project["lockfile"]).read_bytes(),
                    parsed,
                    repo_root / project["manifest"],
                    include_build_scope="build" in project.get("default_dependency_scopes", []),
                )
                normalized = normalize_pip_audit(
                    parsed,
                    project,
                    repo_root / project["lockfile"],
                    repo_root / project["manifest"],
                )
                reported_finding_count = len(normalized)
            else:  # pragma: no cover - SCAN_REPORTS is closed above.
                raise PolicyError(f"unsupported scanner: {key}")
            if project["ecosystem"] == "cargo":
                for finding in normalized:
                    finding["cargo_graph_sha256"] = cargo_graph_sha256
            if scan["exit_classification"] == "clean":
                _require(
                    reported_finding_count == 0,
                    f"clean scan contains findings: {key}",
                )
            elif scan["exit_classification"] == "findings":
                _require(
                    reported_finding_count > 0,
                    f"findings scan contains no findings: {key}",
                )
            findings.extend(normalized)
        cargo_audit_identities = {_exception_identity(item) for item in cargo_audit_canonical}
        missing_canonical = [
            item
            for item in cargo_deny_canonical
            if _exception_identity(item) not in cargo_audit_identities
        ]
        _require(
            not missing_canonical,
            "cargo-deny advisory/yanked finding is absent from canonical cargo-audit report: "
            + ", ".join(
                f"{item['finding_id']} {item['package']['name']} {item['package']['version']}"
                for item in missing_canonical
            ),
        )
        findings = _dedupe_findings(findings)
        evaluated, exception_results = evaluate_findings(policy, findings, day)
        report["findings"] = evaluated
        report["exceptions"] = exception_results

        _validate_sbom_tree(sbom_dir)
        sbom_records = {item["project_id"]: item for item in manifest_parts["sboms"]}
        verified_sboms = []
        for project_id, project in projects.items():
            lock_data = (repo_root / project["lockfile"]).read_bytes()
            expected = _lock_components(project["ecosystem"], lock_data)
            lock_sha = input_map[project_id]["sha256"]
            declared = sbom_records[project_id]
            relative = _safe_relative(declared["path"], field="sbom path")
            path = sbom_dir / pathlib.Path(*relative.parts)
            if project_id == "rust-workspace":
                summary = _verify_rust_sbom(
                    path,
                    expected,
                    candidate_sha=candidate_sha,
                    lockfile_sha256=lock_sha,
                    sbom_dir=sbom_dir,
                )
            else:
                summary = verify_sbom(
                    path,
                    expected,
                    project_id=project_id,
                    ecosystem=project["ecosystem"],
                )
                summary.update(
                    {
                        "path": str(relative),
                        "sha256": _sha256_file(path),
                        "lockfile_sha256": lock_sha,
                    }
                )
            _require(
                summary["sha256"] == declared["sha256"],
                f"SBOM manifest hash mismatch: {project_id}",
            )
            summary["candidate_sha"] = candidate_sha
            verified_sboms.append(summary)
        report["sboms"] = sorted(verified_sboms, key=lambda item: item["project_id"])

        license_data = _validate_license_evidence(
            _read_json_file(reports_dir / "license-notices.json"),
            candidate_sha=candidate_sha,
            today=day,
            repo_root=repo_root,
        )
        _require(
            manifest_parts["license_notice"].get("status") == "pass",
            "license notice manifest status failed",
        )
        report["license_notice"] = license_data
    except (OSError, PolicyError) as error:
        report["errors"].append(
            {"code": "evidence-invalid", "message": str(error), "project_id": None}
        )

    blocked = any(item.get("status") == "blocked" for item in report["findings"])
    bad_exceptions = any(item.get("status") != "active" for item in report["exceptions"])
    all_sboms = len(report["sboms"]) == len(EXPECTED_PROJECTS) and all(
        item.get("verified") for item in report["sboms"]
    )
    if (
        not report["errors"]
        and not blocked
        and not bad_exceptions
        and all_sboms
        and report["license_notice"].get("status") == "pass"
    ):
        report["verdict"] = "pass"
    _write_report(output_dir, report)
    return report


def validate_deny_config(path: str | pathlib.Path) -> None:
    """Validate the official cargo-deny configuration without hidden ignores."""

    config_path = pathlib.Path(os.path.abspath(path))
    _require(
        config_path.name == "deny.toml" and config_path.is_file() and not config_path.is_symlink(),
        "deny.toml must be the lexical repository-root regular file, not a symlink",
    )
    repo_root = config_path.parent
    try:
        config = tomllib.loads(config_path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, tomllib.TOMLDecodeError) as error:
        raise PolicyError(f"invalid deny.toml: {error}") from error
    graph = config.get("graph", {})
    advisories = config.get("advisories", {})
    licenses = config.get("licenses", {})
    bans = config.get("bans", {})
    sources = config.get("sources", {})
    _require(
        all(
            isinstance(section, Mapping) for section in (graph, advisories, licenses, bans, sources)
        ),
        "deny.toml graph/check sections must be tables",
    )
    _require(
        set(config) == {"graph", "advisories", "licenses", "bans", "sources"},
        "deny.toml top-level fields differ from the locked schema",
    )
    _require(
        set(graph) == {"targets", "all-features", "exclude", "exclude-dev", "exclude-unpublished"},
        "deny.toml graph fields differ from the locked schema",
    )
    _require(
        set(advisories)
        == {
            "unmaintained",
            "unsound",
            "yanked",
            "unused-ignored-advisory",
            "disable-yank-checking",
            "ignore",
        },
        "deny.toml advisories fields differ from the locked schema",
    )
    _require(
        set(licenses)
        == {
            "confidence-threshold",
            "include-dev",
            "include-build",
            "unused-license-exception",
            "private",
            "clarify",
            "allow",
            "exceptions",
        },
        "deny.toml licenses fields differ from the locked schema",
    )
    _require(
        set(bans) == {"multiple-versions", "wildcards", "allow", "deny", "skip", "skip-tree"},
        "deny.toml bans fields differ from the locked schema",
    )
    _require(
        set(sources) == {"unknown-registry", "unknown-git", "allow-registry", "allow-git"},
        "deny.toml sources fields differ from the locked schema",
    )
    _require(
        graph.get("targets") == [],
        "deny.toml graph.targets must be empty (all targets)",
    )
    _require(graph.get("all-features") is True, "deny.toml graph.all-features must be true")
    _require(graph.get("exclude", []) == [], "deny.toml graph.exclude must be empty")
    _require(graph.get("exclude-dev") is False, "deny.toml graph.exclude-dev must be false")
    _require(
        graph.get("exclude-unpublished") is False,
        "deny.toml graph.exclude-unpublished must be false",
    )
    _require(
        advisories.get("unmaintained") == "all",
        "deny.toml must report all unmaintained advisories",
    )
    _require(
        advisories.get("unsound") == "all",
        "deny.toml must report all unsound advisories",
    )
    _require(advisories.get("yanked") == "deny", "deny.toml must deny yanked packages")
    _require(
        advisories.get("disable-yank-checking") is False,
        "deny.toml must not disable yanked-package checking",
    )
    _require(
        advisories.get("unused-ignored-advisory") == "deny",
        "deny.toml must deny unused advisory ignores",
    )
    _require(advisories.get("ignore") == [], "deny.toml advisory ignore list must be empty")
    _require(
        licenses.get("confidence-threshold") == 0.8,
        "deny.toml license confidence threshold changed",
    )
    _require(
        licenses.get("include-dev") is True,
        "deny.toml licenses.include-dev must be true",
    )
    _require(
        licenses.get("include-build") is True,
        "deny.toml licenses.include-build must be true",
    )
    _require(
        licenses.get("unused-license-exception") == "deny",
        "deny.toml must deny unused license exceptions",
    )
    _require(
        licenses.get("private") == {"ignore": False, "ignore-sources": [], "registries": []},
        "deny.toml licenses.private must not suppress license checks",
    )
    _require(licenses.get("clarify") == [], "deny.toml license clarifications must be empty")
    _require(
        licenses.get("allow")
        == [
            "MIT",
            "Apache-2.0",
            "Apache-2.0 WITH LLVM-exception",
            "BSD-2-Clause",
            "BSL-1.0",
            "ISC",
            "Unicode-3.0",
            "Unlicense",
            "Zlib",
        ],
        "deny.toml license allow list changed",
    )
    _require(
        licenses.get("exceptions") == [{"allow": ["MPL-2.0"], "crate": "cbindgen@0.28.0"}],
        "deny.toml license exceptions changed",
    )
    _require(bans.get("wildcards") == "deny", "deny.toml must deny wildcard dependencies")
    for field in ("allow", "deny", "skip", "skip-tree"):
        _require(bans.get(field) == [], f"deny.toml bans.{field} must be empty")
    _require(
        sources.get("unknown-registry") == "deny",
        "deny.toml must deny unknown registries",
    )
    _require(sources.get("unknown-git") == "deny", "deny.toml must deny unknown git sources")
    _require(
        sources.get("allow-registry") == ["https://github.com/rust-lang/crates.io-index"],
        "deny.toml registry allow list changed",
    )
    _require(sources.get("allow-git") == [], "deny.toml git allow list must be empty")
    for relative in DENY_AUXILIARY_CONFIG_PATHS:
        _require(
            not (repo_root / relative).exists() and not (repo_root / relative).is_symlink(),
            f"auxiliary cargo-deny exception file is forbidden: {relative}",
        )
    validate_repository_scanner_configs(repo_root)


def validate_repository_scanner_configs(repo_root: pathlib.Path) -> None:
    """Reject repository scanner configuration that can suppress or redirect evidence."""

    _validate_cargo_repository_config(repo_root)
    ambient_aliases = sorted(name for name in CARGO_ALIAS_ENVIRONMENT_NAMES if name in os.environ)
    _require(
        not ambient_aliases,
        f"ambient Cargo scanner aliases are forbidden: {ambient_aliases}",
    )

    audit_config = repo_root / ".cargo" / "audit.toml"
    _require(
        not audit_config.exists() and not audit_config.is_symlink(),
        "repository .cargo/audit.toml is forbidden; cargo-audit uses isolated defaults",
    )
    npm_configs = [
        path
        for path in repo_root.rglob(".npmrc")
        if ".git" not in path.parts and "node_modules" not in path.parts
    ]
    _require(
        not npm_configs,
        "repository/project .npmrc files are forbidden: "
        + ", ".join(str(path.relative_to(repo_root)) for path in sorted(npm_configs)),
    )


def _validate_cargo_repository_config(repo_root: pathlib.Path) -> str:
    """Require the one exact repository Cargo patch configuration and targets."""

    discovered: set[str] = set()
    for current, directories, filenames in os.walk(repo_root, followlinks=False):
        directory = pathlib.Path(current)
        relative_directory = directory.relative_to(repo_root)

        def excluded(relative: pathlib.PurePath) -> bool:
            return bool(relative.parts) and (
                relative.parts[0] in LOCK_DISCOVERY_ROOT_EXCLUDED
                or any(part in LOCK_DISCOVERY_CACHE_PARTS for part in relative.parts)
            )

        retained_directories = []
        for name in directories:
            relative = relative_directory / name
            path = directory / name
            if excluded(relative):
                continue
            if name == ".cargo":
                _require(
                    path.is_dir() and not path.is_symlink(),
                    f"repository Cargo config directory is unsafe: {relative}",
                )
            if not path.is_symlink():
                retained_directories.append(name)
        directories[:] = retained_directories
        if excluded(relative_directory) or directory.name != ".cargo":
            continue
        for filename in filenames:
            if filename not in {"config", "config.toml"}:
                continue
            path = directory / filename
            relative = path.relative_to(repo_root)
            _require(
                path.is_file() and not path.is_symlink(),
                f"repository Cargo config is unsafe: {relative}",
            )
            discovered.add(relative.as_posix())
    _require(
        discovered == {".cargo/config.toml"},
        "repository Cargo configs differ from the sole locked root .cargo/config.toml; "
        f"found={sorted(discovered)}",
    )
    config_path = _lexical_regular_repo_file(
        repo_root,
        ".cargo/config.toml",
        label="repository Cargo config",
    )
    try:
        config = tomllib.loads(config_path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, tomllib.TOMLDecodeError) as error:
        raise PolicyError(f"invalid repository Cargo config: {error}") from error
    expected_patch = {name: {"path": path} for name, path in CARGO_PATCH_PATHS.items()}
    _require(
        config == {"patch": {"crates-io": expected_patch}},
        "repository Cargo config differs from the exact locked patch schema",
    )
    for crate, relative_value in CARGO_PATCH_PATHS.items():
        relative = _safe_relative(relative_value, field=f"Cargo patch {crate}")
        target = repo_root
        for part in relative.parts:
            target /= part
            _require(
                not target.is_symlink(),
                f"Cargo patch {crate} path contains a symlink: {relative}",
            )
        _require(
            target.is_dir(),
            f"Cargo patch {crate} is not an in-repo directory: {relative}",
        )
        _lexical_regular_repo_file(
            repo_root,
            (relative / "Cargo.toml").as_posix(),
            label=f"Cargo patch {crate} manifest",
        )
    digest = _sha256_file(config_path)
    _require(digest == CARGO_CONFIG_SHA256, "repository Cargo config bytes changed")
    return digest


def _run_command(
    command: Sequence[str],
    *,
    cwd: pathlib.Path,
    env: Mapping[str, str] | None = None,
    clear_env_prefixes: Sequence[str] = (),
) -> subprocess.CompletedProcess[bytes]:
    normalized_prefixes = tuple(prefix.upper() for prefix in clear_env_prefixes)
    process_env = {
        key: value
        for key, value in os.environ.items()
        if not key.upper().startswith(normalized_prefixes)
    }
    if env:
        process_env.update(env)
    try:
        return subprocess.run(
            list(command),
            cwd=cwd,
            env=process_env,
            stdin=subprocess.DEVNULL,
            capture_output=True,
            check=False,
        )
    except OSError as error:
        return subprocess.CompletedProcess(list(command), 127, b"", str(error).encode("utf-8"))


def verify_tool_version_output(tool: str, output: str) -> None:
    """Require the first reported semantic version to equal the stable pin."""

    _require(tool in EXPECTED_TOOL_PINS, f"unknown pinned tool: {tool}")
    pin = EXPECTED_TOOL_PINS[tool]
    reported_versions = [match.group("version") for match in SEMVER_TOKEN_RE.finditer(output)]
    _require(
        bool(reported_versions) and reported_versions[0] == pin,
        f"{tool} version is not pinned to exact stable {pin}: {output.strip()}",
    )


def _assert_tool_versions(repo_root: pathlib.Path) -> dict[str, str]:
    commands = {
        "rustc": ["rustc", "--version"],
        "cargo_audit": ["cargo-audit", "--version"],
        "cargo_deny": ["cargo-deny", "--version"],
        "cargo_cyclonedx": ["cargo-cyclonedx", "cyclonedx", "--version"],
        "cargo_about": ["cargo-about", "--version"],
        "uv": ["uv", "--no-config", "--no-cache", "--version"],
        "pip_audit": [*PIP_AUDIT_TOOL_ARGV, "--version"],
        "node": ["node", "--version"],
        "npm": ["npm", "--version"],
        "cyclonedx_npm": ["cyclonedx-npm", "--version"],
    }
    for tool, command in commands.items():
        if tool.startswith("cargo_"):
            cleared = CARGO_ALIAS_ENVIRONMENT_NAMES
        elif tool in {"uv", "pip_audit"}:
            cleared = ("UV_", "PIP_AUDIT_")
        else:
            cleared = ()
        result = _run_command(command, cwd=repo_root, clear_env_prefixes=cleared)
        output = (result.stdout + result.stderr).decode("utf-8", errors="replace")
        _require(result.returncode == 0, f"{tool} version command failed: {output.strip()}")
        verify_tool_version_output(tool, output)
    return dict(EXPECTED_TOOL_PINS)


def _write_json(path: pathlib.Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(_canonical_json_bytes(value))


def _classify_scan(returncode: int, parsed: Any, finding_count: int) -> str:
    error_payload = (isinstance(parsed, Mapping) and "error" in parsed) or (
        isinstance(parsed, list)
        and any(isinstance(item, Mapping) and "error" in item for item in parsed)
    )
    if error_payload:
        return "operational-error"
    if returncode == 0:
        return "clean" if finding_count == 0 else "findings"
    if returncode == 1 and finding_count > 0:
        return "findings"
    return "operational-error"


def _classify_cargo_deny_scan(returncode: int, parsed: Any, finding_count: int) -> str:
    error_payload = (isinstance(parsed, Mapping) and "error" in parsed) or (
        isinstance(parsed, list)
        and any(isinstance(item, Mapping) and "error" in item for item in parsed)
    )
    if error_payload:
        return "operational-error"
    expected_returncode = _cargo_deny_expected_exit_code(parsed)
    if returncode != expected_returncode:
        return "operational-error"
    if returncode == 0:
        return "clean" if finding_count == 0 else "operational-error"
    return "findings" if finding_count > 0 else "operational-error"


def _scan_record(
    *,
    project: Mapping[str, Any],
    scanner: str,
    report: str,
    result: subprocess.CompletedProcess[bytes],
    parsed: Any,
    finding_count: int,
    raw_dir: pathlib.Path,
    lockfile_sha256: str,
    advisory_database: Mapping[str, Any] | None = None,
) -> dict[str, Any]:
    _write_json(raw_dir / report, parsed)
    record = {
        "project_id": project["project_id"],
        "scanner": scanner,
        "report": report,
        "command": _expected_scan_command(str(project["project_id"]), scanner),
        "environment": _expected_scan_environment(scanner),
        "working_directory": _project_working_directory(project),
        "exit_code": result.returncode,
        "exit_classification": (
            _classify_cargo_deny_scan(result.returncode, parsed, finding_count)
            if scanner == "cargo-deny"
            else _classify_scan(result.returncode, parsed, finding_count)
        ),
        "raw_sha256": _sha256_file(raw_dir / report),
        "lockfile_sha256": lockfile_sha256,
        "target_scope": "all",
    }
    if scanner == "cargo-audit":
        record["advisory_database"] = dict(advisory_database or {})
    return record


def _parse_command_json(result: subprocess.CompletedProcess[bytes], label: str) -> Any:
    try:
        return _json_loads(result.stdout, label=label)
    except PolicyError as error:
        return {"error": str(error), "exit_code": result.returncode}


def _parse_cargo_deny_output(result: subprocess.CompletedProcess[bytes]) -> Any:
    stdout = result.stdout.decode("utf-8", errors="replace")
    if stdout.strip():
        return [
            {
                "error": "cargo-deny 0.20.2 emitted unexpected stdout",
                "exit_code": result.returncode,
                "stdout_sha256": _sha256_bytes(result.stdout),
            }
        ]
    text = result.stderr.decode("utf-8", errors="replace")
    if not text.strip() and result.returncode == 0:
        return []
    try:
        value = _json_value(text, allow_jsonl=True, label="cargo-deny JSONL")
        return value if isinstance(value, list) else [value]
    except PolicyError as error:
        return [{"error": str(error), "exit_code": result.returncode}]


def _capture_cargo_deny_output(
    result: subprocess.CompletedProcess[bytes],
    *,
    raw_dir: pathlib.Path,
    project: Mapping[str, Any],
) -> tuple[Any, int]:
    parsed = _parse_cargo_deny_output(result)
    _write_json(raw_dir / SCAN_REPORTS[("rust-workspace", "cargo-deny")], parsed)
    finding_count = (
        cargo_deny_reported_finding_count(parsed, project, require_summary=True)
        if not any(isinstance(item, Mapping) and "error" in item for item in parsed)
        else 0
    )
    return parsed, finding_count


def _cargo_audit_database_provenance(
    cargo_home: pathlib.Path, *, repo_root: pathlib.Path
) -> dict[str, Any]:
    database = cargo_home / "advisory-db"
    url_result = _run_command(
        ["git", "-C", str(database), "remote", "get-url", "origin"],
        cwd=repo_root,
    )
    commit_result = _run_command(
        ["git", "-C", str(database), "rev-parse", "HEAD"],
        cwd=repo_root,
    )
    raw_url = url_result.stdout.decode("utf-8", errors="replace").strip()
    normalized_url = raw_url.removesuffix(".git").rstrip("/")
    commit = commit_result.stdout.decode("utf-8", errors="replace").strip()
    return {
        "url": normalized_url if url_result.returncode == 0 else None,
        "commit": commit if commit_result.returncode == 0 else None,
        "fresh_fetch": True,
    }


def _marker_free_requirement_batches(lock_data: bytes) -> list[list[str]]:
    packages = _uv_packages(lock_data)
    by_name: dict[str, list[tuple[str, set[str]]]] = {}
    for (name, version), hashes in packages.items():
        _require(bool(hashes), f"uv registry package {name} {version} has no lockfile hashes")
        by_name.setdefault(name, []).append((version, hashes))
    for values in by_name.values():
        values.sort(key=lambda item: item[0])
    result: list[list[str]] = []
    for index in range(max(len(values) for values in by_name.values())):
        rows = []
        for name, values in sorted(by_name.items()):
            if index >= len(values):
                continue
            version, hashes = values[index]
            suffix = "".join(f" --hash={value}" for value in sorted(hashes))
            rows.append(f"{name}=={version}{suffix}")
        result.append(rows)
    return result


def _run_pip_audit(
    *,
    project: Mapping[str, Any],
    cwd: pathlib.Path,
    lock_data: bytes,
    temporary_dir: pathlib.Path,
) -> tuple[subprocess.CompletedProcess[bytes], dict[str, Any]]:
    export = _run_command(UV_EXPORT_ARGV, cwd=cwd, clear_env_prefixes=("UV_",))
    if export.returncode != 0:
        return export, {"error": "uv export failed", "reports": []}
    reports = []
    returncode = 0
    stderr = bytearray(export.stderr)
    for index, rows in enumerate(_marker_free_requirement_batches(lock_data)):
        requirement_path = temporary_dir / f"{project['project_id']}-{index}.requirements.txt"
        requirement_path.write_text("\n".join(rows) + "\n", encoding="utf-8")
        command = [*PIP_AUDIT_ARGV[:-1], str(requirement_path)]
        result = _run_command(
            command,
            cwd=cwd,
            clear_env_prefixes=("UV_", "PIP_AUDIT_"),
        )
        stderr.extend(result.stderr)
        if result.returncode not in {0, 1}:
            returncode = result.returncode
        elif result.returncode == 1 and returncode == 0:
            returncode = 1
        parsed = _parse_command_json(result, f"{project['project_id']} pip-audit batch {index}")
        reports.append(parsed)
    envelope = {
        "schema": "ferric.pip-audit-batches",
        "version": 1,
        "export_command": UV_EXPORT_ARGV,
        "audit_command": PIP_AUDIT_ARGV,
        "reports": reports,
    }
    stdout = _canonical_json_bytes(envelope)
    return subprocess.CompletedProcess(PIP_AUDIT_ARGV, returncode, stdout, bytes(stderr)), envelope


def _generate_sboms(
    *,
    repo_root: pathlib.Path,
    sbom_dir: pathlib.Path,
    projects: Mapping[str, Mapping[str, Any]],
    inputs: Mapping[str, Mapping[str, Any]],
    candidate_sha: str,
    source_date_epoch: int,
) -> list[dict[str, Any]]:
    records: list[dict[str, Any]] = []
    rust_project = projects["rust-workspace"]
    existing = {
        path: _sha256_file(path)
        for path in repo_root.rglob("*.cdx.json")
        if sbom_dir not in path.parents and ".git" not in path.parts
    }
    manifest_directories = {path.parent for path in repo_root.rglob("Cargo.toml")}
    at_risk = [path for path in existing if path.parent in manifest_directories]
    _require(
        not at_risk,
        f"refusing to let cargo-cyclonedx overwrite pre-existing outputs: {at_risk}",
    )
    rust_result = _run_command(
        CARGO_SBOM_ARGV,
        cwd=repo_root,
        env={"SOURCE_DATE_EPOCH": str(source_date_epoch)},
        clear_env_prefixes=CARGO_ALIAS_ENVIRONMENT_NAMES,
    )
    after = {
        path
        for path in repo_root.rglob("*.cdx.json")
        if sbom_dir not in path.parents and ".git" not in path.parts
    }
    _require(
        all(path.is_file() and _sha256_file(path) == digest for path, digest in existing.items()),
        "cargo-cyclonedx modified a pre-existing CycloneDX file",
    )
    generated = sorted(after - set(existing))
    rust_member_dir = sbom_dir / "rust-workspace"
    rust_member_dir.mkdir(parents=True, exist_ok=True)
    rust_expected = _cargo_components((repo_root / rust_project["lockfile"]).read_bytes())
    members = []
    try:
        for index, source in enumerate(generated):
            destination = rust_member_dir / f"{index:03d}-{source.stem}.cdx.json"
            data = normalize_sbom(source, rust_expected, ecosystem="cargo")
            _write_json(destination, data)
            members.append(
                {
                    "kind": RUST_SBOM_GENERATOR_KIND,
                    "path": f"rust-workspace/{destination.name}",
                    "sha256": _sha256_file(destination),
                }
            )
    finally:
        for source in generated:
            source.unlink(missing_ok=True)
    _require(
        rust_result.returncode == 0 and members,
        "cargo-cyclonedx failed to generate workspace SBOM members",
    )
    lock_union_path = sbom_dir / RUST_SBOM_LOCK_UNION_PATH
    _write_json(lock_union_path, _cargo_lock_union_sbom(rust_expected))
    members.append(
        {
            "kind": RUST_SBOM_LOCK_UNION_KIND,
            "path": RUST_SBOM_LOCK_UNION_PATH,
            "sha256": _sha256_file(lock_union_path),
        }
    )
    rust_manifest_path = sbom_dir / "rust-workspace.sbom-manifest.json"
    _write_json(
        rust_manifest_path,
        {
            "schema": RUST_SBOM_MANIFEST_SCHEMA,
            "version": 1,
            "project_id": "rust-workspace",
            "candidate_sha": candidate_sha,
            "lockfile_sha256": inputs["rust-workspace"]["sha256"],
            "members": members,
        },
    )
    records.append(
        {
            "project_id": "rust-workspace",
            "path": "rust-workspace.sbom-manifest.json",
            "command": CARGO_SBOM_ARGV,
            "environment": _expected_sbom_environment(rust_project),
            "working_directory": ".",
            "exit_code": rust_result.returncode,
            "sha256": _sha256_file(rust_manifest_path),
            "lockfile_sha256": inputs["rust-workspace"]["sha256"],
            "normalization": "canonical-json-v1",
            "source_date_epoch": source_date_epoch,
        }
    )

    for project_id, project in projects.items():
        if project_id == "rust-workspace":
            continue
        cwd = repo_root / pathlib.PurePosixPath(_project_working_directory(project))
        output = sbom_dir / f"{project_id}.cdx.json"
        expected = _lock_components(
            project["ecosystem"], (repo_root / project["lockfile"]).read_bytes()
        )
        if project["ecosystem"] == "npm":
            command = [
                *NPM_SBOM_ARGV[:-2],
                str(output),
                str(repo_root / project["manifest"]),
            ]
            with tempfile.TemporaryDirectory(
                prefix=f"dependency-policy-{project_id}-npm-sbom-"
            ) as npm_temp:
                npm_temp_path = pathlib.Path(npm_temp)
                npm_user_config = npm_temp_path / "npm-user-config"
                npm_global_config = npm_temp_path / "npm-global-config"
                npm_user_config.touch()
                npm_global_config.touch()
                npm_environment = {
                    "NPM_CONFIG_USERCONFIG": str(npm_user_config),
                    "NPM_CONFIG_GLOBALCONFIG": str(npm_global_config),
                    "NPM_CONFIG_CACHE": str(npm_temp_path / "npm-cache"),
                }
                result = _run_command(
                    command,
                    cwd=cwd,
                    env=npm_environment,
                    clear_env_prefixes=("NPM_CONFIG_",),
                )
            _require(
                result.returncode == 0 and output.is_file(),
                f"cyclonedx-npm failed for {project_id}",
            )
            raw = output.read_bytes()
        else:
            result = _run_command(UV_SBOM_ARGV, cwd=cwd, clear_env_prefixes=("UV_",))
            _require(result.returncode == 0, f"uv CycloneDX export failed for {project_id}")
            raw = result.stdout
        normalized = normalize_sbom(raw, expected, ecosystem=project["ecosystem"])
        _write_json(output, normalized)
        verify_sbom(output, expected, project_id=project_id, ecosystem=project["ecosystem"])
        records.append(
            {
                "project_id": project_id,
                "path": output.name,
                "command": _expected_sbom_command(project),
                "environment": _expected_sbom_environment(project),
                "working_directory": _project_working_directory(project),
                "exit_code": result.returncode,
                "sha256": _sha256_file(output),
                "lockfile_sha256": inputs[project_id]["sha256"],
                "normalization": "canonical-json-v1",
                "source_date_epoch": source_date_epoch,
            }
        )
    return records


def _prepare_run_output(output_dir: pathlib.Path) -> None:
    preserved_reports = {
        "dependency-policy-report.json",
        "dependency-policy-report.md",
    }
    if output_dir.exists():
        _require(
            output_dir.is_dir() and not output_dir.is_symlink(),
            "output directory is not a safe directory",
        )
    else:
        output_dir.mkdir(parents=True)
    for child in output_dir.iterdir():
        if child.name in preserved_reports:
            _require(
                child.is_file() and not child.is_symlink(),
                f"initialized failure report is not a regular file: {child.name}",
            )
            continue
        _require(not child.is_symlink(), f"output entry is an unsafe symlink: {child.name}")
        if child.is_dir():
            shutil.rmtree(child)
        elif child.is_file():
            child.unlink()
        else:
            raise PolicyError(f"output entry is not a regular file or directory: {child.name}")


def run_policy(
    *,
    policy_path: pathlib.Path,
    candidate_sha: str,
    today: str,
    output_dir: pathlib.Path,
) -> dict[str, Any]:
    _require(
        SHA_RE.fullmatch(candidate_sha) is not None,
        "candidate_sha must be exact lowercase 40-hex",
    )
    day = _parse_date(today, "today").isoformat()
    policy_path, repo_root = _policy_repository_path(policy_path)
    policy = load_policy(policy_path)
    validate_policy(policy, day)
    _validate_project_files(repo_root, policy)
    deny_path = repo_root / "deny.toml"
    validate_deny_config(deny_path)
    expected_output = pathlib.Path(os.path.abspath(repo_root / "dependency-policy-evidence"))
    _require(
        pathlib.Path(os.path.abspath(output_dir)) == expected_output,
        "run --output-dir must be exactly the repository dependency-policy-evidence directory",
    )
    output_dir = expected_output
    _require(
        day == _current_utc_date(),
        "run --today must equal the current UTC date; rerun with a fresh date",
    )
    head = _run_command(["git", "rev-parse", "HEAD"], cwd=repo_root)
    _require(
        head.returncode == 0 and head.stdout.decode().strip() == candidate_sha,
        "checked-out HEAD does not equal --candidate-sha",
    )
    timestamp = _run_command(["git", "show", "-s", "--format=%ct", candidate_sha], cwd=repo_root)
    _require(
        timestamp.returncode == 0 and timestamp.stdout.decode().strip().isdigit(),
        "cannot determine candidate commit timestamp",
    )
    source_date_epoch = int(timestamp.stdout.decode().strip())

    _prepare_run_output(output_dir)
    _require(
        _write_incomplete_run_evidence(
            policy_path=policy_path,
            candidate_sha=candidate_sha,
            today=day,
            output_dir=output_dir,
            code="gate-not-completed",
            message="Dependency Policy aggregate execution has not completed",
        ),
        "cannot initialize fail-closed dependency policy evidence",
    )
    raw_dir = output_dir / "raw"
    sbom_dir = output_dir / "sboms"
    shutil.copyfile(policy_path, output_dir / "dependency-policy.json")
    shutil.copyfile(deny_path, output_dir / "deny.toml")

    tool_versions = _assert_tool_versions(repo_root)
    _write_json(raw_dir / "tool-versions.json", tool_versions)
    projects = {item["project_id"]: item for item in policy["projects"]}
    inputs = {
        project_id: {
            "project_id": project_id,
            "ecosystem": project["ecosystem"],
            "manifest": project["manifest"],
            "manifest_sha256": _sha256_file(repo_root / project["manifest"]),
            "lockfile": project["lockfile"],
            "sha256": _sha256_file(repo_root / project["lockfile"]),
        }
        for project_id, project in projects.items()
    }
    scans = []
    cargo_project = projects["rust-workspace"]
    with tempfile.TemporaryDirectory(prefix="dependency-policy-cargo-audit-") as cargo_temp:
        cargo_home = pathlib.Path(cargo_temp) / "cargo-home"
        cargo_home.mkdir()
        cargo_audit = _run_command(
            CARGO_AUDIT_ARGV,
            cwd=repo_root,
            env={"CARGO_HOME": str(cargo_home)},
            clear_env_prefixes=(
                "CARGO_HOME",
                "CARGO_AUDIT_",
                "RUSTSEC_",
                *CARGO_ALIAS_ENVIRONMENT_NAMES,
            ),
        )
        cargo_audit_json = _parse_command_json(cargo_audit, "cargo-audit report")
        cargo_audit_provenance = _cargo_audit_database_provenance(
            cargo_home,
            repo_root=repo_root,
        )
        if "error" not in cargo_audit_json:
            _validate_cargo_audit_evidence(
                cargo_audit_json,
                cargo_lock=repo_root / cargo_project["lockfile"],
                advisory_commit=str(cargo_audit_provenance.get("commit", "")),
            )
        cargo_count = (
            len(normalize_cargo_audit(cargo_audit_json, cargo_project))
            if "error" not in cargo_audit_json
            else 0
        )
    scans.append(
        _scan_record(
            project=cargo_project,
            scanner="cargo-audit",
            report=SCAN_REPORTS[("rust-workspace", "cargo-audit")],
            result=cargo_audit,
            parsed=cargo_audit_json,
            finding_count=cargo_count,
            raw_dir=raw_dir,
            lockfile_sha256=inputs["rust-workspace"]["sha256"],
            advisory_database=cargo_audit_provenance,
        )
    )
    cargo_deny = _run_command(
        CARGO_DENY_ARGV,
        cwd=repo_root,
        clear_env_prefixes=CARGO_ALIAS_ENVIRONMENT_NAMES,
    )
    cargo_deny_json, deny_count = _capture_cargo_deny_output(
        cargo_deny,
        raw_dir=raw_dir,
        project=cargo_project,
    )
    scans.append(
        _scan_record(
            project=cargo_project,
            scanner="cargo-deny",
            report=SCAN_REPORTS[("rust-workspace", "cargo-deny")],
            result=cargo_deny,
            parsed=cargo_deny_json,
            finding_count=deny_count,
            raw_dir=raw_dir,
            lockfile_sha256=inputs["rust-workspace"]["sha256"],
        )
    )

    with tempfile.TemporaryDirectory(prefix="dependency-policy-") as temp_name:
        temporary_dir = pathlib.Path(temp_name)
        npm_user_config = temporary_dir / "npm-user-config"
        npm_global_config = temporary_dir / "npm-global-config"
        npm_user_config.touch()
        npm_global_config.touch()
        npm_environment = {
            "NPM_CONFIG_USERCONFIG": str(npm_user_config),
            "NPM_CONFIG_GLOBALCONFIG": str(npm_global_config),
            "NPM_CONFIG_CACHE": str(temporary_dir / "npm-cache"),
        }
        for project_id in ("node-package", "node-addon", "documentation", "site"):
            project = projects[project_id]
            cwd = repo_root / pathlib.PurePosixPath(_project_working_directory(project))
            result = _run_command(
                NPM_AUDIT_ARGV,
                cwd=cwd,
                env=npm_environment,
                clear_env_prefixes=("NPM_CONFIG_",),
            )
            parsed = _parse_command_json(result, f"{project_id} npm audit")
            count = 0
            if isinstance(parsed, Mapping) and "error" not in parsed:
                count = len(
                    normalize_npm_audit(
                        parsed,
                        project,
                        repo_root / project["lockfile"],
                        repo_root / project["manifest"],
                    )
                )
            scans.append(
                _scan_record(
                    project=project,
                    scanner="npm-audit",
                    report=SCAN_REPORTS[(project_id, "npm-audit")],
                    result=result,
                    parsed=parsed,
                    finding_count=count,
                    raw_dir=raw_dir,
                    lockfile_sha256=inputs[project_id]["sha256"],
                )
            )
        for project_id in ("python-package", "python-tools"):
            project = projects[project_id]
            cwd = repo_root / pathlib.PurePosixPath(_project_working_directory(project))
            lock_data = (repo_root / project["lockfile"]).read_bytes()
            result, parsed = _run_pip_audit(
                project=project,
                cwd=cwd,
                lock_data=lock_data,
                temporary_dir=temporary_dir,
            )
            count = 0
            if "error" not in parsed:
                count = len(
                    normalize_pip_audit(
                        parsed,
                        project,
                        lock_data,
                        repo_root / project["manifest"],
                    )
                )
                verify_pip_audit_coverage(
                    lock_data,
                    parsed,
                    repo_root / project["manifest"],
                    include_build_scope="build" in project.get("default_dependency_scopes", []),
                )
            scans.append(
                _scan_record(
                    project=project,
                    scanner="pip-audit",
                    report=SCAN_REPORTS[(project_id, "pip-audit")],
                    result=result,
                    parsed=parsed,
                    finding_count=count,
                    raw_dir=raw_dir,
                    lockfile_sha256=inputs[project_id]["sha256"],
                )
            )

    license_contract_hashes = _license_contract_hashes(repo_root)
    license_result = _run_command(
        LICENSE_NOTICE_ARGV,
        cwd=repo_root,
        clear_env_prefixes=CARGO_ALIAS_ENVIRONMENT_NAMES,
    )
    notices_path = repo_root / "THIRD_PARTY_NOTICES.md"
    license_evidence = {
        "schema": "ferric.license-notices-evidence",
        "version": 1,
        "candidate_sha": candidate_sha,
        "evaluated_on": day,
        "command": LICENSE_NOTICE_ARGV,
        "working_directory": ".",
        "exit_code": license_result.returncode,
        "status": "pass" if license_result.returncode == 0 else "fail",
        "path": "THIRD_PARTY_NOTICES.md",
        "sha256": _sha256_file(notices_path) if notices_path.is_file() else None,
        "cargo_about": EXPECTED_TOOL_PINS["cargo_about"],
        **license_contract_hashes,
    }
    _write_json(raw_dir / "license-notices.json", license_evidence)

    sbom_records = _generate_sboms(
        repo_root=repo_root,
        sbom_dir=sbom_dir,
        projects=projects,
        inputs=inputs,
        candidate_sha=candidate_sha,
        source_date_epoch=source_date_epoch,
    )
    _require(
        day == _current_utc_date(),
        "UTC date changed during dependency-policy run; rerun the complete gate",
    )
    (raw_dir / "operational-error.json").unlink()
    raw_files = []
    for filename in sorted(RAW_FILENAMES - {"scan-manifest.json"}):
        path = raw_dir / filename
        raw_files.append({"path": filename, "sha256": _sha256_file(path)})
    manifest = {
        "schema": SCAN_MANIFEST_SCHEMA,
        "version": 1,
        "candidate_sha": candidate_sha,
        "evaluated_on": day,
        "target_scope": "all",
        "source_date_epoch": source_date_epoch,
        "tool_versions": tool_versions,
        "policy_sha256": _sha256_file(policy_path),
        "deny_config_sha256": _sha256_file(deny_path),
        "cargo_config_sha256": _validate_cargo_repository_config(repo_root),
        "cargo_workspace_manifests_sha256": _cargo_workspace_manifest_contract(repo_root),
        "inputs": [inputs[project_id] for project_id in sorted(inputs)],
        "scans": sorted(scans, key=lambda item: (item["project_id"], item["scanner"])),
        "sboms": sorted(sbom_records, key=lambda item: item["project_id"]),
        "license_notice": {
            "path": "license-notices.json",
            "command": LICENSE_NOTICE_ARGV,
            "working_directory": ".",
            "exit_code": license_result.returncode,
            "status": license_evidence["status"],
            "sha256": _sha256_file(raw_dir / "license-notices.json"),
            **license_contract_hashes,
        },
        "raw_files": raw_files,
        "normalization": EXPECTED_NORMALIZATION,
    }
    _write_json(raw_dir / "scan-manifest.json", manifest)
    return evaluate_evidence(
        policy_path=policy_path,
        reports_dir=raw_dir,
        sbom_dir=sbom_dir,
        candidate_sha=candidate_sha,
        today=day,
        output_dir=output_dir,
    )


def _write_incomplete_run_evidence(
    *,
    policy_path: pathlib.Path,
    candidate_sha: str,
    today: str,
    output_dir: pathlib.Path,
    code: str,
    message: str,
) -> bool:
    """Write an honest partial artifact if setup or orchestration cannot finish."""

    lexical_policy = pathlib.Path(os.path.abspath(policy_path))
    if (
        lexical_policy.name != "dependency-policy.json"
        or lexical_policy.is_symlink()
        or not lexical_policy.is_file()
        or lexical_policy.parent.is_symlink()
        or not lexical_policy.parent.is_dir()
    ):
        return False
    policy_path = lexical_policy
    repo_root = policy_path.parent
    expected_output = pathlib.Path(os.path.abspath(repo_root / "dependency-policy-evidence"))
    lexical_output = pathlib.Path(os.path.abspath(output_dir))
    if lexical_output != expected_output:
        return False
    if output_dir.is_symlink() or (output_dir.exists() and not output_dir.is_dir()):
        return False
    output_dir.mkdir(parents=True, exist_ok=True)
    raw_dir = output_dir / "raw"
    sbom_dir = output_dir / "sboms"
    for directory in (raw_dir, sbom_dir):
        if directory.is_symlink() or (directory.exists() and not directory.is_dir()):
            return False
        directory.mkdir(exist_ok=True)
    destinations = [
        output_dir / "dependency-policy.json",
        output_dir / "deny.toml",
        raw_dir / "operational-error.json",
    ]
    if any(path.is_symlink() or (path.exists() and not path.is_file()) for path in destinations):
        return False
    if policy_path.is_file():
        shutil.copyfile(policy_path, output_dir / "dependency-policy.json")
    deny_path = repo_root / "deny.toml"
    if deny_path.is_file():
        shutil.copyfile(deny_path, output_dir / "deny.toml")
    error = {"code": code, "message": message, "project_id": None}
    _write_json(
        raw_dir / "operational-error.json",
        {
            "schema": "ferric.dependency-operational-error",
            "version": 1,
            "candidate_sha": candidate_sha,
            "evaluated_on": today,
            "error": error,
        },
    )
    _write_report(
        output_dir,
        {
            "schema": REPORT_SCHEMA,
            "version": 1,
            "candidate_sha": candidate_sha,
            "evaluated_on": today,
            "tool_versions": {},
            "inputs": [],
            "findings": [],
            "exceptions": [],
            "sboms": [],
            "license_notice": {"status": "fail"},
            "errors": [error],
            "verdict": "fail",
        },
    )
    return True


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Evaluate the ferric-rules dependency policy")
    subparsers = parser.add_subparsers(dest="command", required=True)
    validate = subparsers.add_parser("validate", help="validate policy and deny.toml")
    validate.add_argument("--policy", required=True, type=pathlib.Path)
    validate.add_argument("--today", required=True)
    evaluate = subparsers.add_parser("evaluate", help="evaluate existing raw reports and SBOMs")
    evaluate.add_argument("--policy", required=True, type=pathlib.Path)
    evaluate.add_argument("--reports-dir", required=True, type=pathlib.Path)
    evaluate.add_argument("--sbom-dir", required=True, type=pathlib.Path)
    evaluate.add_argument("--candidate-sha", required=True)
    evaluate.add_argument("--today", required=True)
    evaluate.add_argument("--output-dir", required=True, type=pathlib.Path)
    run = subparsers.add_parser("run", help="run pinned scanners, generate SBOMs, and evaluate")
    run.add_argument("--policy", required=True, type=pathlib.Path)
    run.add_argument("--candidate-sha", required=True)
    run.add_argument("--today", required=True)
    run.add_argument("--output-dir", required=True, type=pathlib.Path)
    initialize = subparsers.add_parser(
        "initialize",
        help="initialize a fail-closed artifact before workflow tool setup",
    )
    initialize.add_argument("--policy", required=True, type=pathlib.Path)
    initialize.add_argument("--candidate-sha", required=True)
    initialize.add_argument("--today", required=True)
    initialize.add_argument("--output-dir", required=True, type=pathlib.Path)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    parser = _build_parser()
    arguments = parser.parse_args(argv)
    try:
        if arguments.command == "initialize":
            _require(
                SHA_RE.fullmatch(arguments.candidate_sha.lower()) is not None,
                "candidate_sha must be exact lowercase 40-hex",
            )
            initialized_on = _parse_date(arguments.today, "today").isoformat()
            _require(
                _write_incomplete_run_evidence(
                    policy_path=arguments.policy,
                    candidate_sha=arguments.candidate_sha.lower(),
                    today=initialized_on,
                    output_dir=arguments.output_dir,
                    code="gate-not-completed",
                    message="Dependency Policy tool setup or aggregate execution has not completed",
                ),
                "initialize --output-dir must be exactly the repository "
                "dependency-policy-evidence directory",
            )
            print("dependency policy failure artifact initialized")
            return 0
        if arguments.command == "validate":
            policy_path, repo_root = _policy_repository_path(arguments.policy)
            policy = load_policy(policy_path)
            validate_policy(policy, arguments.today)
            _validate_project_files(repo_root, policy)
            validate_deny_config(repo_root / "deny.toml")
            print("dependency policy is valid")
            return 0
        if arguments.command == "evaluate":
            report = evaluate_evidence(
                policy_path=arguments.policy,
                reports_dir=arguments.reports_dir,
                sbom_dir=arguments.sbom_dir,
                candidate_sha=arguments.candidate_sha.lower(),
                today=arguments.today,
                output_dir=arguments.output_dir,
            )
        else:
            report = run_policy(
                policy_path=arguments.policy,
                candidate_sha=arguments.candidate_sha.lower(),
                today=arguments.today,
                output_dir=arguments.output_dir,
            )
        print(f"Dependency Policy: {str(report['verdict']).upper()}")
        return 0 if report["verdict"] == "pass" else 1
    except (OSError, PolicyError) as error:
        if arguments.command == "run":
            _write_incomplete_run_evidence(
                policy_path=arguments.policy,
                candidate_sha=arguments.candidate_sha.lower(),
                today=arguments.today,
                output_dir=arguments.output_dir,
                code="operational-failure",
                message=str(error),
            )
        print(f"dependency policy error: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
