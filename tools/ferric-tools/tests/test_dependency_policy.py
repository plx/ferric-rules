"""Offline contract and adversarial tests for the dependency policy gate."""

from __future__ import annotations

import base64
import copy
import importlib.util
import json
import pathlib
import re
import subprocess
import sys
import tomllib
from datetime import date

import pytest

REPO_ROOT = pathlib.Path(__file__).resolve().parents[3]
POLICY = REPO_ROOT / "dependency-policy.json"
DENY_CONFIG = REPO_ROOT / "deny.toml"
EVALUATOR = REPO_ROOT / "scripts" / "dependency-policy.py"
WORKFLOW = REPO_ROOT / ".github" / "workflows" / "dependency-policy.yml"
DOCUMENTATION = REPO_ROOT / "docs" / "dependency-security-policy.md"
GITIGNORE = REPO_ROOT / ".gitignore"
FIXTURES = pathlib.Path(__file__).with_name("fixtures") / "dependency-policy"

TODAY = "2026-08-09"
EXPIRY = "2026-09-08"
CANDIDATE_SHA = "a" * 40
CARGO_GRAPH_SHA256 = "f26957ce219d372d9d7535971d2e5080336fac1697a89f219563cc63c59b386f"
CARGO_LOCK_SHA256 = "b05b227710a509cc4834f2c7a67d3dde0ce3b5448361c418b8028f150e6bf9a7"
CARGO_CONFIG_SHA256 = "4317bf5980c303c718623b18fd21aa0653544292e08bed159cadeda3f2f153ce"
CARGO_DENY_SHA256 = "266a829c940118ec2cde3f69df9a9d7ce91d80fc2824067e25db4931f69e77c3"
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

RAW_REPORTS = {
    "rust-workspace.cargo-audit.json",
    "cargo-deny.json",
    "node-package.npm-audit.json",
    "node-addon.npm-audit.json",
    "documentation.npm-audit.json",
    "site.npm-audit.json",
    "python-package.pip-audit.json",
    "python-tools.pip-audit.json",
    "scan-manifest.json",
    "tool-versions.json",
    "license-notices.json",
}

TOOL_PINS = {
    "rustc": "1.93.0",
    "cargo-audit": "0.22.2",
    "cargo-deny": "0.20.2",
    "cargo-cyclonedx": "0.5.9",
    "cargo-about": "0.9.0",
    "uv": "0.11.16",
    "pip-audit": "2.10.1",
    "node": "22.18.0",
    "npm": "11.12.1",
    "cyclonedx-npm": "4.2.1",
}

EXPECTED_INITIAL_EXCEPTION_IDS = {
    "cargo-pyo3-rustsec-2025-0020",
    "cargo-pyo3-rustsec-2026-0177",
    "cargo-atomic-polyfill-rustsec-2023-0089",
    "cargo-bincode-rustsec-2025-0141",
    "cargo-paste-rustsec-2024-0436",
    "npm-node-esbuild-ghsa-g7r4-m6w7-qqqr",
    "npm-docs-fast-uri-ghsa-v2hh-gcrm-f6hx",
    "npm-docs-fast-uri-ghsa-7p8r-x3mc-p8w7",
    "npm-docs-fast-uri-ghsa-4c8g-83qw-93j6",
    "npm-site-fast-uri-ghsa-7p8r-x3mc-p8w7",
    "npm-site-js-yaml-ghsa-5p4m-2wfm-xmqj",
    "npm-site-nanoid-ghsa-2v37-7h3g-55p8",
    "npm-site-postcss-ghsa-fxqj-rqcc-2cmp",
    "pypi-python-package-pygments-ghsa-5239-wwwm-4pmq",
    "pypi-python-package-pytest-8-4-2-ghsa-6w46-j5rx-g56g",
    "pypi-python-package-pytest-ghsa-6w46-j5rx-g56g",
    "pypi-python-tools-click-ghsa-47fr-3ffg-hgmw",
    "pypi-python-tools-pygments-ghsa-5239-wwwm-4pmq",
    "pypi-python-tools-pytest-ghsa-6w46-j5rx-g56g",
}

NODE_PLATFORM_OPTIONALS = {
    "@ferric-rules/napi-darwin-arm64",
    "@ferric-rules/napi-darwin-x64",
    "@ferric-rules/napi-linux-arm64-gnu",
    "@ferric-rules/napi-linux-arm64-musl",
    "@ferric-rules/napi-linux-x64-gnu",
    "@ferric-rules/napi-linux-x64-musl",
    "@ferric-rules/napi-win32-x64-msvc",
}


def _policy() -> dict[str, object]:
    assert POLICY.is_file(), f"missing dependency policy: {POLICY}"
    return json.loads(POLICY.read_text(encoding="utf-8"))


def _populate_project_files(repo: pathlib.Path) -> None:
    for project in _policy()["projects"]:
        for field in ("manifest", "lockfile"):
            relative = pathlib.Path(str(project[field]))
            destination = repo / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes((REPO_ROOT / relative).read_bytes())
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
    workspace = tomllib.loads((REPO_ROOT / "Cargo.toml").read_text(encoding="utf-8"))["workspace"]
    for member_pattern in workspace["members"]:
        for member in REPO_ROOT.glob(member_pattern):
            if not member.is_dir():
                continue
            relative = member.relative_to(REPO_ROOT) / "Cargo.toml"
            destination = repo / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes((REPO_ROOT / relative).read_bytes())


def _exceptions(policy: dict[str, object]) -> list[dict[str, object]]:
    exceptions = policy.get("exceptions")
    assert isinstance(exceptions, list) and exceptions, "policy must declare reviewed exceptions"
    assert all(isinstance(item, dict) for item in exceptions)
    return exceptions


def _load_evaluator():
    assert EVALUATOR.is_file(), f"missing dependency policy evaluator: {EVALUATOR}"
    spec = importlib.util.spec_from_file_location("dependency_policy_under_test", EVALUATOR)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def _cargo_graph_sha256(module: object, **overrides: object) -> str:
    values: dict[str, object] = {
        "project_id": "rust-workspace",
        "lockfile": "Cargo.lock",
        "lockfile_sha256": CARGO_LOCK_SHA256,
        "workspace_manifests_sha256": CARGO_WORKSPACE_MANIFESTS_SHA256,
        "cargo_config_sha256": CARGO_CONFIG_SHA256,
        "deny_config_sha256": CARGO_DENY_SHA256,
        "dependency_groups": "all",
        "targets": "all",
        "features": "all",
    }
    values.update(overrides)
    return module._cargo_graph_sha256(**values)


def _run_validate(
    tmp_path: pathlib.Path,
    policy: dict[str, object],
    *,
    today: str = TODAY,
) -> subprocess.CompletedProcess[str]:
    assert EVALUATOR.is_file(), f"missing dependency policy evaluator: {EVALUATOR}"
    path = tmp_path / "dependency-policy.json"
    path.write_text(json.dumps(policy, sort_keys=True), encoding="utf-8")
    (tmp_path / "deny.toml").write_bytes(DENY_CONFIG.read_bytes())
    _populate_project_files(tmp_path)
    return subprocess.run(
        [
            sys.executable,
            str(EVALUATOR),
            "validate",
            "--policy",
            str(path),
            "--today",
            today,
        ],
        cwd=REPO_ROOT,
        text=True,
        capture_output=True,
        check=False,
    )


def _first_exception(policy: dict[str, object]) -> dict[str, object]:
    return _exceptions(policy)[0]


def _project(policy: dict[str, object], project_id: str) -> dict[str, object]:
    return next(project for project in policy["projects"] if project["project_id"] == project_id)


def _project_lock(project: dict[str, object]) -> bytes:
    return (REPO_ROOT / str(project["lockfile"])).read_bytes()


def _project_manifest(project: dict[str, object]) -> bytes:
    return (REPO_ROOT / str(project["manifest"])).read_bytes()


def _exception_for(finding: dict[str, object]) -> dict[str, object]:
    return {
        "exception_id": "fixture-exception",
        "ecosystem": finding["ecosystem"],
        "project_id": finding["project_id"],
        "lockfile": finding["lockfile"],
        "kind": finding["kind"],
        "finding_id": finding["finding_id"],
        "package": copy.deepcopy(finding["package"]),
        "scanner_severity": finding["scanner_severity"],
        "dependency_scopes": copy.deepcopy(finding.get("dependency_scopes", ["runtime"])),
        "dev_only": finding.get("dev_only", False),
        "affected_surfaces": copy.deepcopy(finding.get("affected_surfaces", ["fixture"])),
        "reachability": finding.get("reachability", "unknown"),
        "evidence": [{"kind": "fixture", "reference": "deterministic offline evidence"}],
        "owner": "release-engineering",
        "tracking_issue": "https://github.com/plx/ferric-rules/issues/215",
        "rationale": "Exercise an exact reviewed exception.",
        "remediation": "Remove the fixture finding.",
        "issued_on": TODAY,
        "expires_on": EXPIRY,
    }


def _npm_single_node_report(
    module: object,
    package_lock: dict[str, object],
    node: str,
    *,
    finding_id: str = "GHSA-ABCD-1234-EFGH",
) -> dict[str, object]:
    package = package_lock["packages"][node]
    name = package.get("name") or node.rsplit("node_modules/", 1)[-1]
    report = {
        "auditReportVersion": 2,
        "vulnerabilities": {
            name: {
                "name": name,
                "severity": "low",
                "isDirect": True,
                "via": [
                    {
                        "source": 990099,
                        "name": name,
                        "dependency": name,
                        "title": "authenticated build-scope fixture",
                        "url": f"https://github.com/advisories/{finding_id}",
                        "severity": "low",
                        "range": "*",
                    }
                ],
                "effects": [],
                "range": "*",
                "nodes": [node],
                "fixAvailable": False,
            }
        },
        "metadata": {
            "vulnerabilities": {
                "info": 0,
                "low": 1,
                "moderate": 0,
                "high": 0,
                "critical": 0,
                "total": 1,
            },
            "dependencies": module._npm_audit_dependency_counts(package_lock),
        },
    }
    return report


def _job(workflow: str, job_id: str) -> str:
    match = re.search(
        rf"(?ms)^  {re.escape(job_id)}:\n(?P<body>.*?)(?=^  [a-z][a-z0-9-]*:\n|\Z)",
        workflow,
    )
    assert match is not None, f"workflow must define the `{job_id}` job"
    return match.group("body")


def test_committed_policy_has_the_exact_seven_project_surfaces():
    policy = _policy()
    assert policy["schema"] == "ferric.dependency-policy"
    assert policy["version"] == 1
    assert set(policy["owners"]) == OWNERS
    assert policy["tool_pins"] == {
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

    projects = policy.get("projects")
    assert isinstance(projects, list)
    by_id = {project["project_id"]: project for project in projects}
    assert len(projects) == len(by_id) == 7
    assert by_id.keys() == PROJECTS.keys()

    for project_id, (ecosystem, manifest, lockfile) in PROJECTS.items():
        project = by_id[project_id]
        assert project["ecosystem"] == ecosystem
        assert project["manifest"] == manifest
        assert project["lockfile"] == lockfile
        assert project["dependency_groups"] == "all"
        assert project["targets"] == "all"
        if ecosystem == "cargo":
            assert project["features"] == "all"


def test_npm_audit_dependency_counts_match_pinned_arborist_semantics():
    module = _load_evaluator()
    expected = {
        "packages/ferric/package-lock.json": {
            "prod": 2,
            "dev": 34,
            "optional": 34,
            "peer": 0,
            "peerOptional": 0,
            "total": 42,
        },
        "crates/ferric-rules-napi/package-lock.json": {
            "prod": 1,
            "dev": 1,
            "optional": 0,
            "peer": 0,
            "peerOptional": 0,
            "total": 1,
        },
        "documentation/package-lock.json": {
            "prod": 1,
            "dev": 9,
            "optional": 1,
            "peer": 0,
            "peerOptional": 0,
            "total": 9,
        },
        "site/package-lock.json": {
            "prod": 346,
            "dev": 191,
            "optional": 124,
            "peer": 1,
            "peerOptional": 0,
            "total": 652,
        },
    }
    for relative, counts in expected.items():
        assert module._npm_audit_dependency_counts(REPO_ROOT / relative) == counts

    overlap = {
        "packages": {
            "": {},
            "node_modules/dev-optional": {"dev": True, "optional": True},
            "node_modules/peer-optional": {"peer": True, "peerOptional": True},
            "node_modules/dev-optional-only": {"devOptional": True},
            "node_modules/prod": {},
        }
    }
    assert module._npm_audit_dependency_counts(overlap) == {
        "prod": 3,
        "dev": 1,
        "optional": 1,
        "peer": 1,
        "peerOptional": 1,
        "total": 4,
    }


def test_repository_cargo_config_is_the_exact_bound_patch_contract():
    module = _load_evaluator()
    assert module._validate_cargo_repository_config(REPO_ROOT) == (
        "4317bf5980c303c718623b18fd21aa0653544292e08bed159cadeda3f2f153ce"
    )
    assert module.CARGO_PATCH_PATHS == {
        "ferric-rules-core": "crates/ferric-rules-core",
        "ferric-rules-ffi-macros": "crates/ferric-rules-ffi-macros",
        "ferric-rules-parser": "crates/ferric-rules-parser",
        "ferric-rules-pinned": "crates/ferric-rules-pinned",
        "ferric-rules-runtime": "crates/ferric-rules-runtime",
    }


def test_cargo_manifest_census_is_exactly_the_root_workspace_expansion():
    module = _load_evaluator()
    manifests = module._workspace_cargo_manifest_paths(REPO_ROOT)
    assert "Cargo.toml" in manifests
    assert len(manifests) == 28
    assert manifests == {
        "Cargo.toml",
        *{
            (member.relative_to(REPO_ROOT) / "Cargo.toml").as_posix()
            for pattern in tomllib.loads((REPO_ROOT / "Cargo.toml").read_text())["workspace"][
                "members"
            ]
            for member in REPO_ROOT.glob(pattern)
            if member.is_dir()
        },
    }


def test_reviewed_exceptions_are_exact_bounded_and_evidence_bearing():
    policy = _policy()
    exceptions = _exceptions(policy)
    identities: set[tuple[object, ...]] = set()
    exception_ids: set[object] = set()

    for exception in exceptions:
        assert exception.keys() >= REQUIRED_EXCEPTION_FIELDS
        assert exception["exception_id"] not in exception_ids
        exception_ids.add(exception["exception_id"])
        assert exception["project_id"] in PROJECTS
        ecosystem, _, lockfile = PROJECTS[str(exception["project_id"])]
        assert exception["ecosystem"] == ecosystem
        assert exception["lockfile"] == lockfile
        assert exception["kind"] in KINDS
        assert exception["scanner_severity"] in SEVERITIES
        assert exception["owner"] in OWNERS
        assert exception["reachability"] in REACHABILITY
        assert set(exception["dependency_scopes"]) <= SCOPES
        assert exception["dependency_scopes"]
        assert isinstance(exception["dev_only"], bool)
        assert exception["affected_surfaces"]
        assert exception["evidence"]
        assert all(
            isinstance(evidence, dict)
            and set(evidence) == {"kind", "reference"}
            and evidence["kind"]
            and evidence["reference"]
            for evidence in exception["evidence"]
        )
        assert re.fullmatch(
            r"https://github\.com/plx/ferric-rules/issues/(151|215|219|220)",
            str(exception["tracking_issue"]),
        )
        assert exception["rationale"]
        assert exception["remediation"]
        assert exception["issued_on"] == TODAY
        assert exception["expires_on"] == EXPIRY
        assert set(exception["package"]) == {"name", "version"}
        assert exception["package"]["name"]
        assert exception["package"]["version"]
        identity = (
            exception["ecosystem"],
            exception["project_id"],
            exception["lockfile"],
            exception["kind"],
            exception["finding_id"],
            exception["package"]["name"],
            exception["package"]["version"],
        )
        assert identity not in identities, f"ambiguous exception identity: {identity}"
        identities.add(identity)
        assert "approved" not in exception
        assert "approved_by" not in exception
        assert "reviewer" not in exception

    assert exception_ids == EXPECTED_INITIAL_EXCEPTION_IDS


def test_current_five_cargo_exceptions_bind_the_exact_canonical_graph_digest():
    module = _load_evaluator()
    policy = _policy()
    cargo_exceptions = [
        exception for exception in _exceptions(policy) if exception["ecosystem"] == "cargo"
    ]
    foreign_exceptions = [
        exception for exception in _exceptions(policy) if exception["ecosystem"] != "cargo"
    ]

    assert _cargo_graph_sha256(module) == CARGO_GRAPH_SHA256
    assert len(cargo_exceptions) == 5
    assert {exception["cargo_graph_sha256"] for exception in cargo_exceptions} == {
        CARGO_GRAPH_SHA256
    }
    assert all(
        set(exception) == REQUIRED_EXCEPTION_FIELDS | {"cargo_graph_sha256"}
        for exception in cargo_exceptions
    )
    assert all(
        set(exception) == REQUIRED_EXCEPTION_FIELDS and "cargo_graph_sha256" not in exception
        for exception in foreign_exceptions
    )


@pytest.mark.parametrize(
    ("field", "drifted"),
    [
        ("lockfile_sha256", "0" * 64),
        ("workspace_manifests_sha256", "1" * 64),
        ("cargo_config_sha256", "2" * 64),
        ("deny_config_sha256", "3" * 64),
    ],
)
def test_cargo_graph_digest_changes_for_every_authenticated_hash_constituent(field, drifted):
    module = _load_evaluator()

    current = _cargo_graph_sha256(module)
    changed = _cargo_graph_sha256(module, **{field: drifted})

    assert current == CARGO_GRAPH_SHA256
    assert re.fullmatch(r"[0-9a-f]{64}", changed)
    assert changed != current


@pytest.mark.parametrize(
    ("field", "drifted"),
    [
        ("project_id", "another-workspace"),
        ("lockfile", "nested/Cargo.lock"),
        ("dependency_groups", "runtime"),
        ("targets", "host"),
        ("features", "default"),
    ],
)
def test_cargo_graph_digest_rejects_drift_in_closed_identity_or_scope(field, drifted):
    module = _load_evaluator()

    assert _cargo_graph_sha256(module) == CARGO_GRAPH_SHA256
    with pytest.raises(module.PolicyError, match=field):
        _cargo_graph_sha256(module, **{field: drifted})


def test_validate_rejects_missing_cargo_graph_digest(tmp_path):
    policy = copy.deepcopy(_policy())
    cargo_exception = next(
        exception for exception in _exceptions(policy) if exception["ecosystem"] == "cargo"
    )
    assert cargo_exception.pop("cargo_graph_sha256") == CARGO_GRAPH_SHA256

    result = _run_validate(tmp_path, policy)

    assert result.returncode != 0
    assert "cargo_graph_sha256" in result.stdout + result.stderr


@pytest.mark.parametrize(
    "invalid",
    [None, True, "F" * 64, "f" * 63, "g" * 64],
)
def test_validate_rejects_malformed_or_noncanonical_cargo_graph_digest(tmp_path, invalid):
    policy = copy.deepcopy(_policy())
    cargo_exception = next(
        exception for exception in _exceptions(policy) if exception["ecosystem"] == "cargo"
    )
    assert cargo_exception["cargo_graph_sha256"] == CARGO_GRAPH_SHA256
    cargo_exception["cargo_graph_sha256"] = invalid

    result = _run_validate(tmp_path, policy)

    assert result.returncode != 0
    assert "cargo_graph_sha256" in result.stdout + result.stderr


def test_validate_rejects_well_formed_but_stale_cargo_graph_digest(tmp_path):
    policy = copy.deepcopy(_policy())
    cargo_exception = next(
        exception for exception in _exceptions(policy) if exception["ecosystem"] == "cargo"
    )
    assert cargo_exception["cargo_graph_sha256"] == CARGO_GRAPH_SHA256
    cargo_exception["cargo_graph_sha256"] = "0" * 64

    result = _run_validate(tmp_path, policy)

    assert result.returncode != 0
    assert "cargo_graph_sha256" in result.stdout + result.stderr


@pytest.mark.parametrize("ecosystem", ["npm", "pypi"])
def test_validate_forbids_cargo_graph_digest_on_foreign_exceptions(tmp_path, ecosystem):
    policy = copy.deepcopy(_policy())
    assert any(
        exception.get("cargo_graph_sha256") == CARGO_GRAPH_SHA256
        for exception in _exceptions(policy)
        if exception["ecosystem"] == "cargo"
    )
    foreign = next(
        exception for exception in _exceptions(policy) if exception["ecosystem"] == ecosystem
    )
    foreign["cargo_graph_sha256"] = CARGO_GRAPH_SHA256

    result = _run_validate(tmp_path, policy)

    assert result.returncode != 0
    assert "cargo_graph_sha256" in result.stdout + result.stderr


def test_cargo_graph_digest_does_not_make_duplicate_exception_identity_valid(tmp_path):
    policy = copy.deepcopy(_policy())
    cargo_exception = next(
        exception for exception in _exceptions(policy) if exception["ecosystem"] == "cargo"
    )
    assert cargo_exception["cargo_graph_sha256"] == CARGO_GRAPH_SHA256
    duplicate = copy.deepcopy(cargo_exception)
    duplicate["exception_id"] += "-duplicate"
    policy["exceptions"].append(duplicate)

    result = _run_validate(tmp_path, policy)

    assert result.returncode != 0
    assert "ambiguous" in (result.stdout + result.stderr).lower()


def test_cargo_deny_covers_every_feature_target_and_group_without_hidden_ignores():
    assert DENY_CONFIG.is_file(), f"missing cargo-deny policy: {DENY_CONFIG}"
    config = tomllib.loads(DENY_CONFIG.read_text(encoding="utf-8"))
    graph = config["graph"]
    assert graph["all-features"] is True
    assert graph["targets"] == []
    assert graph.get("exclude", []) == []
    assert graph.get("exclude-dev") is False
    assert graph.get("exclude-unpublished") is False
    assert config.get("advisories", {}).get("ignore", []) == []
    assert config.get("advisories", {}).get("disable-yank-checking") is False
    assert config.get("licenses", {}).get("exceptions", []) == [
        {"allow": ["MPL-2.0"], "crate": "cbindgen@0.28.0"}
    ]
    assert config.get("licenses", {}).get("unused-license-exception") == "deny"
    assert config.get("licenses", {}).get("include-dev") is True
    assert config.get("licenses", {}).get("include-build") is True
    assert config.get("licenses", {}).get("private") == {
        "ignore": False,
        "ignore-sources": [],
        "registries": [],
    }
    assert config.get("licenses", {}).get("clarify") == []
    assert config.get("bans", {}).get("skip", []) == []
    assert config.get("bans", {}).get("skip-tree", []) == []
    assert config.get("licenses", {}).get("allow")
    assert "sources" in config


@pytest.mark.parametrize(
    "replacement",
    [
        pytest.param("", id="missing"),
        pytest.param("include-dev = false", id="disabled"),
        pytest.param('include-dev = "true"', id="wrong-type-string"),
        pytest.param("include-dev = 1", id="wrong-type-integer"),
    ],
)
def test_validate_rejects_deny_config_without_exact_boolean_dev_license_coverage(
    tmp_path, replacement
):
    policy_path = tmp_path / "dependency-policy.json"
    policy_path.write_text(json.dumps(_policy(), sort_keys=True), encoding="utf-8")
    deny = DENY_CONFIG.read_text(encoding="utf-8")
    assert "include-dev = true" in deny
    (tmp_path / "deny.toml").write_text(
        deny.replace("include-dev = true", replacement),
        encoding="utf-8",
    )
    _populate_project_files(tmp_path)

    result = subprocess.run(
        [
            sys.executable,
            str(EVALUATOR),
            "validate",
            "--policy",
            str(policy_path),
            "--today",
            TODAY,
        ],
        cwd=REPO_ROOT,
        text=True,
        capture_output=True,
        check=False,
    )

    assert result.returncode != 0
    assert any(
        token in result.stdout + result.stderr for token in ["include-dev", "licenses fields"]
    )


@pytest.mark.parametrize(
    ("needle", "replacement"),
    [
        ("exclude-dev = false", "exclude-dev = true"),
        ("exclude-unpublished = false", "exclude-unpublished = true"),
        ("disable-yank-checking = false", "disable-yank-checking = true"),
        ("include-build = true", "include-build = false"),
        ('unused-license-exception = "deny"', 'unused-license-exception = "allow"'),
        ('crate = "cbindgen@0.28.0"', 'crate = "cbindgen"'),
        (
            "private = { ignore = false, ignore-sources = [], registries = [] }",
            "private = { ignore = true, ignore-sources = [], registries = [] }",
        ),
        (
            "clarify = []",
            'clarify = [{ name = "fixture", expression = "MIT", license-files = [] }]',
        ),
        ("exclude-dev = false", "exclude-dev = false\nfeatures = []"),
        ("[sources]", "unknown-narrowing-key = true\n\n[sources]"),
    ],
)
def test_validate_rejects_cargo_deny_scope_narrowing_or_unknown_keys(tmp_path, needle, replacement):
    policy_path = tmp_path / "dependency-policy.json"
    policy_path.write_text(json.dumps(_policy(), sort_keys=True), encoding="utf-8")
    deny = DENY_CONFIG.read_text(encoding="utf-8")
    assert needle in deny
    (tmp_path / "deny.toml").write_text(
        deny.replace(needle, replacement, 1),
        encoding="utf-8",
    )
    _populate_project_files(tmp_path)

    result = subprocess.run(
        [
            sys.executable,
            str(EVALUATOR),
            "validate",
            "--policy",
            str(policy_path),
            "--today",
            TODAY,
        ],
        cwd=REPO_ROOT,
        text=True,
        capture_output=True,
        check=False,
    )

    assert result.returncode != 0
    assert "deny.toml" in result.stdout + result.stderr


def test_validate_rejects_every_auxiliary_cargo_deny_exception_file(tmp_path):
    module = _load_evaluator()
    for relative in module.DENY_AUXILIARY_CONFIG_PATHS:
        case = tmp_path / relative.replace("/", "-")
        case.mkdir()
        policy_path = case / "dependency-policy.json"
        policy_path.write_text(json.dumps(_policy(), sort_keys=True), encoding="utf-8")
        (case / "deny.toml").write_bytes(DENY_CONFIG.read_bytes())
        _populate_project_files(case)
        auxiliary = case / relative
        auxiliary.parent.mkdir(parents=True, exist_ok=True)
        auxiliary.write_text('[advisories]\nignore = ["RUSTSEC-2099-0001"]\n', encoding="utf-8")

        result = subprocess.run(
            [
                sys.executable,
                str(EVALUATOR),
                "validate",
                "--policy",
                str(policy_path),
                "--today",
                TODAY,
            ],
            cwd=REPO_ROOT,
            text=True,
            capture_output=True,
            check=False,
        )

        assert result.returncode != 0, relative
        assert relative in result.stdout + result.stderr


def test_validate_rejects_repository_deny_toml_symlink_before_following_target_config(tmp_path):
    repo = tmp_path / "repo"
    external = tmp_path / "external"
    repo.mkdir()
    external.mkdir()
    policy_path = repo / "dependency-policy.json"
    policy_path.write_text(json.dumps(_policy(), sort_keys=True), encoding="utf-8")
    _populate_project_files(repo)
    target = external / "deny.toml"
    target.write_bytes(DENY_CONFIG.read_bytes())
    (repo / "deny.toml").symlink_to(target)
    (repo / "deny.exceptions.toml").write_text(
        '[licenses]\nexceptions = [{ crate = "hidden", allow = ["GPL-3.0"] }]\n',
        encoding="utf-8",
    )

    result = subprocess.run(
        [
            sys.executable,
            str(EVALUATOR),
            "validate",
            "--policy",
            str(policy_path),
            "--today",
            TODAY,
        ],
        cwd=REPO_ROOT,
        text=True,
        capture_output=True,
        check=False,
    )

    assert result.returncode != 0
    assert "deny.toml" in result.stdout + result.stderr
    assert "symlink" in result.stdout + result.stderr


@pytest.mark.parametrize(
    ("relative", "replacement"),
    [
        ("about.toml", 'accepted = ["MIT"]\n'),
        ("licenses/third-party-notices.hbs", "# hollow notices\n"),
        ("scripts/license-notices.sh", "#!/usr/bin/env bash\nexit 0\n"),
    ],
)
def test_validate_rejects_hollow_or_changed_license_notice_contract(
    tmp_path, relative, replacement
):
    policy_path = tmp_path / "dependency-policy.json"
    policy_path.write_text(json.dumps(_policy(), sort_keys=True), encoding="utf-8")
    (tmp_path / "deny.toml").write_bytes(DENY_CONFIG.read_bytes())
    _populate_project_files(tmp_path)
    (tmp_path / relative).write_text(replacement, encoding="utf-8")

    result = subprocess.run(
        [
            sys.executable,
            str(EVALUATOR),
            "validate",
            "--policy",
            str(policy_path),
            "--today",
            TODAY,
        ],
        cwd=REPO_ROOT,
        text=True,
        capture_output=True,
        check=False,
    )

    assert result.returncode != 0
    assert "license" in (result.stdout + result.stderr).lower() or "about.toml" in (
        result.stdout + result.stderr
    )


@pytest.mark.parametrize(
    "relative",
    [
        ".cargo/audit.toml",
        ".npmrc",
        "packages/ferric/.npmrc",
        "crates/ferric-rules-napi/.npmrc",
        "documentation/.npmrc",
        "site/.npmrc",
    ],
)
def test_validate_rejects_repository_or_project_scanner_config_bypass_files(tmp_path, relative):
    policy_path = tmp_path / "dependency-policy.json"
    policy_path.write_text(json.dumps(_policy(), sort_keys=True), encoding="utf-8")
    (tmp_path / "deny.toml").write_bytes(DENY_CONFIG.read_bytes())
    _populate_project_files(tmp_path)
    bypass = tmp_path / relative
    bypass.parent.mkdir(parents=True, exist_ok=True)
    bypass.write_text(
        'ignore = ["fixture"]\nregistry=https://attacker.invalid/\n', encoding="utf-8"
    )

    result = subprocess.run(
        [
            sys.executable,
            str(EVALUATOR),
            "validate",
            "--policy",
            str(policy_path),
            "--today",
            TODAY,
        ],
        cwd=REPO_ROOT,
        text=True,
        capture_output=True,
        check=False,
    )

    assert result.returncode != 0
    assert relative.rsplit("/", 1)[-1] in result.stdout + result.stderr


def test_evaluator_exposes_the_locked_public_offline_contract():
    module = _load_evaluator()
    for name in [
        "PolicyError",
        "load_policy",
        "validate_policy",
        "normalize_cargo_audit",
        "normalize_cargo_deny",
        "normalize_npm_audit",
        "normalize_pip_audit",
        "normalize_sbom",
        "evaluate_findings",
        "verify_tool_version_output",
        "verify_sbom",
    ]:
        assert hasattr(module, name), f"evaluator must expose `{name}`"


def test_validate_accepts_the_committed_policy_and_has_no_implicit_clock(tmp_path):
    result = _run_validate(tmp_path, _policy())
    assert result.returncode == 0, result.stdout + result.stderr

    missing_today = subprocess.run(
        [sys.executable, str(EVALUATOR), "validate", "--policy", str(POLICY)],
        cwd=REPO_ROOT,
        text=True,
        capture_output=True,
        check=False,
    )
    assert missing_today.returncode != 0
    assert "today" in (missing_today.stdout + missing_today.stderr).lower()


@pytest.mark.parametrize("casing", ["lower", "mixed"])
def test_validate_rejects_noncanonical_ghsa_exception_finding_id(tmp_path, casing):
    policy = copy.deepcopy(_policy())
    exception = next(
        item for item in _exceptions(policy) if str(item["finding_id"]).startswith("GHSA-")
    )
    canonical = str(exception["finding_id"])
    exception["finding_id"] = (
        canonical.lower() if casing == "lower" else f"GhSa-{canonical[5:].lower()}"
    )

    result = _run_validate(tmp_path, policy)

    assert result.returncode != 0
    assert "finding_id" in result.stdout + result.stderr


def test_every_committed_ghsa_exception_matches_canonical_normalizer_identity():
    module = _load_evaluator()
    policy = _policy()
    ghsa_exceptions = [
        item for item in _exceptions(policy) if str(item["finding_id"]).startswith("GHSA-")
    ]
    assert len(ghsa_exceptions) == 14

    for exception in ghsa_exceptions:
        project = _project(policy, str(exception["project_id"]))
        package = exception["package"]
        finding_id = str(exception["finding_id"])
        if exception["ecosystem"] == "npm":
            package_lock = json.loads((REPO_ROOT / str(exception["lockfile"])).read_text())
            nodes = [
                path
                for path, entry in package_lock["packages"].items()
                if path.endswith(f"node_modules/{package['name']}")
                and entry.get("version") == package["version"]
            ]
            assert nodes, exception["exception_id"]
            severity = str(exception["scanner_severity"])
            counts = dict.fromkeys(["info", "low", "moderate", "high", "critical"], 0)
            counts[severity] = 1
            counts["total"] = 1
            raw = {
                "auditReportVersion": 2,
                "vulnerabilities": {
                    package["name"]: {
                        "name": package["name"],
                        "severity": severity,
                        "isDirect": False,
                        "via": [
                            {
                                "source": 1,
                                "name": package["name"],
                                "dependency": package["name"],
                                "title": "canonical GHSA fixture",
                                "url": (f"https://github.com/advisories/{finding_id.lower()}"),
                                "severity": severity,
                                "range": "*",
                            }
                        ],
                        "effects": [],
                        "range": "*",
                        "nodes": nodes,
                        "fixAvailable": False,
                    }
                },
                "metadata": {
                    "vulnerabilities": counts,
                    "dependencies": module._npm_audit_dependency_counts(package_lock),
                },
            }
            normalized = module.normalize_npm_audit(
                raw,
                project,
                package_lock,
                _project_manifest(project),
            )
        else:
            raw = {
                "dependencies": [
                    {
                        "name": package["name"],
                        "version": package["version"],
                        "vulns": [
                            {
                                "id": "PYSEC-2099-0001",
                                "aliases": [finding_id.lower()],
                                "fix_versions": [],
                                "description": "canonical GHSA fixture",
                            }
                        ],
                    }
                ],
                "fixes": [],
            }
            normalized = module.normalize_pip_audit(
                raw,
                project,
                _project_lock(project),
                _project_manifest(project),
            )

        identity_fields = [
            "ecosystem",
            "project_id",
            "lockfile",
            "kind",
            "finding_id",
            "package",
            "scanner_severity",
        ]
        if exception["ecosystem"] in {"npm", "pypi"}:
            identity_fields.extend(["dependency_scopes", "dev_only"])
        identity = {key: normalized[0][key] for key in identity_fields}
        assert identity == {key: exception[key] for key in identity_fields}, exception[
            "exception_id"
        ]


@pytest.mark.parametrize("field", sorted(REQUIRED_EXCEPTION_FIELDS))
def test_validate_rejects_every_missing_exception_field(tmp_path, field):
    policy = copy.deepcopy(_policy())
    _first_exception(policy).pop(field)
    result = _run_validate(tmp_path, policy)
    assert result.returncode != 0, field
    assert field in result.stdout + result.stderr


@pytest.mark.parametrize("container", ["package", "evidence"])
@pytest.mark.parametrize("field", ["name", "version"])
@pytest.mark.parametrize("invalid", [1, True, None])
def test_validate_requires_nested_package_and_evidence_values_to_be_strings(
    tmp_path, container, field, invalid
):
    policy = copy.deepcopy(_policy())
    exception = _first_exception(policy)
    if container == "package":
        exception["package"][field] = invalid
        expected_field = "package"
    else:
        evidence_field = {"name": "kind", "version": "reference"}[field]
        exception["evidence"][0][evidence_field] = invalid
        expected_field = "evidence"

    result = _run_validate(tmp_path, policy)

    assert result.returncode != 0
    assert expected_field in result.stdout + result.stderr


@pytest.mark.parametrize(
    ("field", "invalid"),
    [
        ("owner", "nobody"),
        ("kind", "ignored"),
        ("scanner_severity", "negligible"),
        ("reachability", "probably"),
        ("dependency_scopes", ["test"]),
        ("tracking_issue", "https://github.com/another/repo/issues/151"),
    ],
)
def test_validate_rejects_unknown_enums_and_foreign_tracking_issues(tmp_path, field, invalid):
    policy = copy.deepcopy(_policy())
    _first_exception(policy)[field] = invalid
    result = _run_validate(tmp_path, policy)
    assert result.returncode != 0
    assert field in result.stdout + result.stderr


@pytest.mark.parametrize("extra_field", ["approved", "scanner_severty"])
def test_validate_rejects_unknown_exception_fields_including_approval_flags(tmp_path, extra_field):
    policy = copy.deepcopy(_policy())
    _first_exception(policy)[extra_field] = True
    result = _run_validate(tmp_path, policy)
    assert result.returncode != 0
    assert extra_field in result.stdout + result.stderr


@pytest.mark.parametrize(
    ("issued_on", "expires_on", "today"),
    [
        ("2026-08-10", EXPIRY, TODAY),
        (TODAY, TODAY, TODAY),
        (TODAY, "2026-08-08", TODAY),
        (TODAY, "2026-08-09T23:59:59Z", TODAY),
        ("08/09/2026", EXPIRY, TODAY),
        (TODAY, "2026-02-30", TODAY),
        (TODAY, EXPIRY, EXPIRY),
    ],
)
def test_validate_rejects_future_issued_expired_and_malformed_dates(
    tmp_path, issued_on, expires_on, today
):
    policy = copy.deepcopy(_policy())
    exception = _first_exception(policy)
    exception["issued_on"] = issued_on
    exception["expires_on"] = expires_on
    result = _run_validate(tmp_path, policy, today=today)
    assert result.returncode != 0


def test_expiry_is_exclusive_but_policy_has_no_hidden_maximum_lifetime(tmp_path):
    policy = copy.deepcopy(_policy())
    _first_exception(policy)["expires_on"] = "2036-08-09"
    result = _run_validate(tmp_path, policy)
    assert result.returncode == 0, result.stdout + result.stderr


def test_duplicate_or_ambiguous_exception_identity_is_invalid(tmp_path):
    policy = copy.deepcopy(_policy())
    duplicate = copy.deepcopy(_first_exception(policy))
    duplicate["exception_id"] = f"{duplicate['exception_id']}-duplicate"
    policy["exceptions"].append(duplicate)
    result = _run_validate(tmp_path, policy)
    assert result.returncode != 0
    assert (
        "ambiguous" in (result.stdout + result.stderr).lower()
        or "duplicate" in (result.stdout + result.stderr).lower()
    )


def test_duplicate_exception_id_is_invalid_even_when_finding_identity_differs(tmp_path):
    policy = copy.deepcopy(_policy())
    duplicate = copy.deepcopy(_first_exception(policy))
    duplicate["finding_id"] = "RUSTSEC-2099-9999"
    policy["exceptions"].append(duplicate)
    result = _run_validate(tmp_path, policy)
    assert result.returncode != 0
    assert "duplicate" in (result.stdout + result.stderr).lower()


def test_adversarial_raw_fixtures_are_complete_and_network_independent():
    cargo = json.loads((FIXTURES / "cargo-audit-findings.json").read_text(encoding="utf-8"))
    npm = json.loads((FIXTURES / "npm-audit-findings.json").read_text(encoding="utf-8"))
    pip = json.loads((FIXTURES / "pip-audit-findings.json").read_text(encoding="utf-8"))
    deny = json.loads((FIXTURES / "cargo-deny-findings.json").read_text(encoding="utf-8"))

    assert cargo["vulnerabilities"]["list"][0]["advisory"]["cvss"] == "10.0"
    assert set(cargo["warnings"]) == {"unmaintained", "unsound", "notice", "yanked"}
    assert npm["vulnerabilities"]["detect-libc"]["severity"] == "critical"
    assert npm["vulnerabilities"]["esbuild"]["severity"] == "low"
    assert pip["dependencies"][0]["vulns"][0]["aliases"][1].startswith("GHSA-")
    assert {diagnostic["fields"]["code"] for diagnostic in deny} == {
        "banned",
        "source-not-allowed",
        "unlicensed",
    }


def test_normalizers_preserve_critical_runtime_and_low_development_findings():
    module = _load_evaluator()
    policy = _policy()
    report = json.loads((FIXTURES / "npm-audit-findings.json").read_text(encoding="utf-8"))
    package_lock = json.loads(
        (REPO_ROOT / "packages/ferric/package-lock.json").read_text(encoding="utf-8")
    )
    findings = module.normalize_npm_audit(
        report,
        _project(policy, "node-package"),
        package_lock,
    )
    by_id = {finding["finding_id"]: finding for finding in findings}

    critical = by_id["GHSA-1111-2222-3333"]
    assert critical["scanner_severity"] == "critical"
    assert critical["dev_only"] is False
    assert "runtime" in critical["dependency_scopes"]

    low = by_id["GHSA-4444-5555-6666"]
    assert low["scanner_severity"] == "low"
    assert low["dev_only"] is True
    assert "development" in low["dependency_scopes"]


def test_npm_canonical_identifier_falls_back_to_source_when_no_ghsa_url_exists():
    module = _load_evaluator()
    policy = _policy()
    report = json.loads((FIXTURES / "npm-audit-findings.json").read_text(encoding="utf-8"))
    report["vulnerabilities"]["detect-libc"]["via"][0]["url"] = (
        "https://registry.npmjs.org/-/npm/v1/security/advisories/990001"
    )
    package_lock = json.loads(
        (REPO_ROOT / "packages/ferric/package-lock.json").read_text(encoding="utf-8")
    )
    findings = module.normalize_npm_audit(
        report,
        _project(policy, "node-package"),
        package_lock,
    )
    detect_libc = next(item for item in findings if item["package"]["name"] == "detect-libc")
    assert detect_libc["finding_id"] == "NPM-990001"


def test_cargo_normalizer_covers_informational_and_yanked_finding_kinds():
    module = _load_evaluator()
    report = json.loads((FIXTURES / "cargo-audit-findings.json").read_text(encoding="utf-8"))
    findings = module.normalize_cargo_audit(report)
    assert {finding["kind"] for finding in findings} == {
        "vulnerability",
        "unmaintained",
        "unsound",
        "notice",
        "yanked",
    }
    critical = next(finding for finding in findings if finding["kind"] == "vulnerability")
    assert critical["finding_id"] == "RUSTSEC-2099-0001"
    assert critical["scanner_severity"] == "critical"
    assert module.normalize_cargo_audit(report) == findings
    assert all(finding["finding_id"] for finding in findings)


def test_cargo_deny_normalizes_unknown_license_source_and_ban_from_array_or_jsonl():
    module = _load_evaluator()
    diagnostics = json.loads((FIXTURES / "cargo-deny-findings.json").read_text(encoding="utf-8"))
    from_array = module.normalize_cargo_deny(diagnostics)
    jsonl = "\n".join(json.dumps(diagnostic) for diagnostic in diagnostics)
    from_jsonl = module.normalize_cargo_deny(jsonl)
    assert from_jsonl == from_array
    assert {finding["kind"] for finding in from_array} == {"license", "source", "ban"}
    license_finding = next(finding for finding in from_array if finding["kind"] == "license")
    assert license_finding["package"] == {"name": "mystery-crate", "version": "1.2.3"}
    assert license_finding["scanner_severity"] == "unknown"


def test_cargo_deny_exit_one_with_advisory_only_diagnostics_is_findings_not_tool_failure():
    module = _load_evaluator()
    diagnostics = json.loads(
        (FIXTURES / "cargo-deny-advisory-only.json").read_text(encoding="utf-8")
    )
    assert module.normalize_cargo_deny(diagnostics) == []
    reported = module.cargo_deny_reported_finding_count(diagnostics)
    assert reported == 1
    assert module._classify_cargo_deny_scan(1, diagnostics, finding_count=reported) == "findings"


def test_cargo_deny_0202_parses_rc1_jsonl_from_stderr_and_classifies_findings():
    module = _load_evaluator()
    diagnostics = json.loads(
        (FIXTURES / "cargo-deny-advisory-only.json").read_text(encoding="utf-8")
    )
    jsonl = "\n".join(json.dumps(item) for item in diagnostics).encode()
    result = subprocess.CompletedProcess(
        module.CARGO_DENY_ARGV,
        1,
        stdout=b"",
        stderr=jsonl,
    )

    parsed = module._parse_cargo_deny_output(result)
    reported = module.cargo_deny_reported_finding_count(parsed)

    assert parsed == diagnostics
    assert reported == 1
    assert module._classify_cargo_deny_scan(result.returncode, parsed, reported) == "findings"


def _cargo_deny_error_diagnostic(check):
    definitions = {
        "advisories": "vulnerability",
        "bans": "banned",
        "licenses": "unlicensed",
        "sources": "source-not-allowed",
    }
    code = definitions[check]
    name = f"fixture-{check}"
    return {
        "type": "diagnostic",
        "fields": {
            "code": code,
            "graphs": [
                {
                    "Krate": {"name": name, "version": "1.2.3"},
                }
            ],
            "message": f"fixture {check} finding",
            "severity": "error",
        },
    }


def _cargo_deny_stream_with_errors(*checks):
    entries = [_cargo_deny_error_diagnostic(check) for check in checks]
    entries.append(
        {
            "type": "summary",
            "fields": {
                check: {
                    "errors": int(check in checks),
                    "helps": 0,
                    "notes": 0,
                    "warnings": 0,
                }
                for check in ("advisories", "bans", "licenses", "sources")
            },
        }
    )
    return entries


@pytest.mark.parametrize(
    ("checks", "returncode"),
    [
        pytest.param(("bans",), 2, id="bans"),
        pytest.param(("licenses",), 4, id="licenses"),
        pytest.param(("sources",), 8, id="sources"),
        pytest.param(("advisories", "bans", "licenses", "sources"), 15, id="all"),
    ],
)
def test_cargo_deny_classifies_every_supported_check_bitset_as_findings(checks, returncode):
    module = _load_evaluator()
    entries = _cargo_deny_stream_with_errors(*checks)
    reported = module.cargo_deny_reported_finding_count(entries, require_summary=True)

    assert reported == len(checks)
    assert module._cargo_deny_expected_exit_code(entries) == returncode
    assert module._classify_cargo_deny_scan(returncode, entries, reported) == "findings"
    assert module._classify_cargo_deny_scan(returncode ^ 1, entries, reported) == (
        "operational-error"
    )


def test_cargo_deny_classifies_a_valid_empty_summary_as_clean():
    module = _load_evaluator()
    entries = _cargo_deny_stream_with_errors()

    assert module._cargo_deny_expected_exit_code(entries) == 0
    assert module._classify_cargo_deny_scan(0, entries, 0) == "clean"
    assert module._classify_cargo_deny_scan(1, entries, 0) == "operational-error"


@pytest.mark.parametrize(
    ("code", "check"),
    [
        ("index-failure", "advisories"),
        ("checksum-match", "bans"),
        ("unmatched-skip", "bans"),
        ("license-exception-not-encountered", "licenses"),
        ("skipped-private-workspace-crate", "licenses"),
        ("allowed-by-organization", "sources"),
        ("unmatched-source", "sources"),
    ],
)
def test_cargo_deny_0202_codes_are_assigned_to_their_authoritative_check(code, check):
    module = _load_evaluator()
    assert module._cargo_deny_check(code) == check


def test_cargo_deny_package_identity_comes_only_from_unambiguous_graph_roots():
    module = _load_evaluator()
    entries = _cargo_deny_stream_with_errors("licenses")
    entries[0]["fields"]["attacker_controlled"] = {
        "name": "wrong-crate",
        "version": "9.9.9",
    }

    finding = module.normalize_cargo_deny(entries, require_summary=True)[0]
    assert finding["package"] == {"name": "fixture-licenses", "version": "1.2.3"}

    ambiguous = copy.deepcopy(entries)
    ambiguous[0]["fields"]["graphs"].append({"Krate": {"name": "other-crate", "version": "4.5.6"}})
    with pytest.raises(module.PolicyError, match=r"multiple crate versions"):
        module.normalize_cargo_deny(ambiguous, require_summary=True)


@pytest.mark.parametrize("invented_field", ["kind", "package"])
def test_cargo_deny_rejects_legacy_identity_overrides_in_production_evidence(invented_field):
    module = _load_evaluator()
    entries = _cargo_deny_stream_with_errors("bans")
    entries[0]["fields"][invented_field] = (
        "yanked" if invented_field == "kind" else {"name": "wrong", "version": "9.9.9"}
    )

    with pytest.raises(module.PolicyError, match=r"pinned JSON schema"):
        module.normalize_cargo_deny(entries, require_summary=True)


def test_cargo_deny_0202_code_sets_cover_the_complete_pinned_enums():
    module = _load_evaluator()
    expected = {
        "advisories": {
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
        },
        "bans": {
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
        },
        "licenses": {
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
        },
        "sources": {
            "allowed-by-organization",
            "allowed-source",
            "git-source-underspecified",
            "source-not-allowed",
            "unmatched-organization",
            "unmatched-source",
        },
    }

    assert expected["advisories"] == module._CARGO_DENY_ADVISORY_CODES
    assert expected["bans"] == module._CARGO_DENY_BAN_CODES
    assert expected["licenses"] == module._CARGO_DENY_LICENSE_CODES
    assert expected["sources"] == module._CARGO_DENY_SOURCE_CODES
    all_codes = [code for codes in expected.values() for code in codes]
    assert len(all_codes) == len(set(all_codes))


def test_cargo_deny_requires_and_validates_the_terminal_summary_for_evidence():
    module = _load_evaluator()
    entries = json.loads((FIXTURES / "cargo-deny-advisory-only.json").read_text(encoding="utf-8"))

    assert module.normalize_cargo_deny(entries, require_summary=True) == []
    assert module.cargo_deny_reported_finding_count(entries, require_summary=True) == 1

    with pytest.raises(module.PolicyError, match=r"summary.*missing"):
        module.normalize_cargo_deny(entries[:-1], require_summary=True)

    inconsistent = copy.deepcopy(entries)
    inconsistent[-1]["fields"]["advisories"]["errors"] = 0
    with pytest.raises(module.PolicyError, match=r"summary.*diagnostics"):
        module.normalize_cargo_deny(inconsistent, require_summary=True)

    shifted = copy.deepcopy(entries)
    shifted[-1]["fields"]["advisories"]["errors"] = 0
    shifted[-1]["fields"]["licenses"]["errors"] = 1
    with pytest.raises(module.PolicyError, match=r"summary.*diagnostics"):
        module.normalize_cargo_deny(shifted, require_summary=True)

    legacy_shape = copy.deepcopy(entries)
    legacy_shape[0]["code"] = legacy_shape[0]["fields"].pop("code")
    with pytest.raises(module.PolicyError, match=r"diagnostic.*pinned JSON schema"):
        module.normalize_cargo_deny(legacy_shape, require_summary=True)


def test_cargo_deny_rejects_log_records_in_the_locked_evidence_stream():
    module = _load_evaluator()
    entries = json.loads((FIXTURES / "cargo-deny-advisory-only.json").read_text(encoding="utf-8"))
    entries.insert(
        -1,
        {
            "type": "log",
            "fields": {
                "timestamp": "2026-08-10T00:00:00Z",
                "level": "WARN",
                "message": "unexpected ambient cargo-deny warning",
            },
        },
    )

    with pytest.raises(module.PolicyError, match=r"unexpected.*log"):
        module.normalize_cargo_deny(entries, require_summary=True)


def test_cargo_deny_rejects_unbucketed_bug_diagnostics_as_operational_failures():
    module = _load_evaluator()
    entries = _cargo_deny_stream_with_errors()
    entries.insert(
        0,
        {
            "type": "diagnostic",
            "fields": {
                "code": "unresolved-workspace-dependency",
                "message": "internal workspace resolution failure",
                "severity": "bug",
            },
        },
    )

    with pytest.raises(module.PolicyError, match=r"severity.*bug"):
        module.normalize_cargo_deny(entries, require_summary=True)


def test_cargo_deny_capture_retains_raw_before_summary_validation(tmp_path):
    module = _load_evaluator()
    entries = json.loads((FIXTURES / "cargo-deny-advisory-only.json").read_text(encoding="utf-8"))
    entries[-1]["fields"]["advisories"]["errors"] = 0
    result = subprocess.CompletedProcess(
        module.CARGO_DENY_ARGV,
        1,
        stdout=b"",
        stderr="\n".join(json.dumps(item) for item in entries).encode(),
    )
    raw_dir = tmp_path / "raw"

    with pytest.raises(module.PolicyError, match=r"summary.*diagnostics"):
        module._capture_cargo_deny_output(
            result,
            raw_dir=raw_dir,
            project=_project(_policy(), "rust-workspace"),
        )

    retained = json.loads((raw_dir / "cargo-deny.json").read_text())
    assert retained == entries


@pytest.mark.parametrize(
    ("returncode", "stdout", "stderr"),
    [
        pytest.param(1, b"VALID_JSONL", b"", id="stdout-only"),
        pytest.param(1, b"VALID_JSONL", b"VALID_JSONL", id="both-streams"),
        pytest.param(1, b"", b"plain diagnostic text", id="malformed-stderr"),
        pytest.param(1, b"", b"", id="findings-exit-without-diagnostics"),
    ],
)
def test_cargo_deny_unexpected_or_contradictory_streams_fail_closed(returncode, stdout, stderr):
    module = _load_evaluator()
    diagnostics = json.loads(
        (FIXTURES / "cargo-deny-advisory-only.json").read_text(encoding="utf-8")
    )
    jsonl = "\n".join(json.dumps(item) for item in diagnostics).encode()
    result = subprocess.CompletedProcess(
        module.CARGO_DENY_ARGV,
        returncode,
        stdout=jsonl if stdout == b"VALID_JSONL" else stdout,
        stderr=jsonl if stderr == b"VALID_JSONL" else stderr,
    )

    parsed = module._parse_cargo_deny_output(result)

    assert (
        module._classify_cargo_deny_scan(returncode, parsed, finding_count=0) == "operational-error"
    )
    assert "error" in json.dumps(parsed).lower()


@pytest.mark.parametrize(
    "clean_report",
    [
        [],
        {
            "vulnerabilities": {"found": False, "count": 0, "list": []},
            "warnings": {},
        },
        {
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
                }
            },
        },
        {"dependencies": [], "fixes": []},
    ],
)
def test_scanner_exit_one_with_valid_clean_json_is_operational_not_findings(clean_report):
    module = _load_evaluator()
    assert module._classify_scan(1, clean_report, finding_count=0) == "operational-error"


def test_cargo_audit_and_deny_yanked_diagnostics_coalesce_to_one_exact_exception():
    module = _load_evaluator()
    policy = copy.deepcopy(_policy())
    cargo = json.loads((FIXTURES / "cargo-audit-findings.json").read_text(encoding="utf-8"))
    deny = json.loads((FIXTURES / "cargo-deny-yanked.json").read_text(encoding="utf-8"))
    cargo_findings = module.normalize_cargo_audit(cargo)
    deny_findings = module.normalize_cargo_deny(deny)
    for finding in [*cargo_findings, *deny_findings]:
        finding["cargo_graph_sha256"] = _cargo_graph_sha256(module)
    yanked = next(finding for finding in cargo_findings if finding["kind"] == "yanked")
    exception = _exception_for(yanked)
    exception["cargo_graph_sha256"] = _cargo_graph_sha256(module)
    exception["reachability"] = "not_applicable"
    policy["exceptions"] = [exception]

    evaluated, exceptions = module.evaluate_findings(
        policy,
        [*cargo_findings, *deny_findings],
        TODAY,
    )

    evaluated_yanked = [finding for finding in evaluated if finding["kind"] == "yanked"]
    assert deny_findings == []
    assert len(evaluated_yanked) == 1
    assert evaluated_yanked[0]["status"] == "excepted"
    assert exceptions == [
        {"exception_id": "fixture-exception", "status": "active", "matched_finding_count": 1}
    ]


@pytest.mark.parametrize("level", ["error", "warning"])
def test_cargo_deny_unknown_diagnostic_code_fails_closed_at_every_level(level):
    module = _load_evaluator()
    diagnostics = json.loads(
        (FIXTURES / "cargo-deny-unknown-error.json").read_text(encoding="utf-8")
    )
    diagnostics[0]["level"] = level
    with pytest.raises(module.PolicyError, match=r"future-policy-error|unknown|unmapped"):
        module.normalize_cargo_deny(diagnostics)


@pytest.mark.parametrize(
    "code",
    [
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
    ],
)
def test_cargo_deny_ignores_only_explicitly_known_nonblocking_codes(code):
    module = _load_evaluator()
    diagnostic = {
        "type": "diagnostic",
        "fields": {
            "code": code,
            "message": "known cargo-deny informational diagnostic",
            "severity": "warning",
        },
    }
    assert module.normalize_cargo_deny([diagnostic]) == []


def test_pip_canonical_identifier_precedence_is_ghsa_then_pysec_then_cve():
    module = _load_evaluator()
    policy = _policy()
    report = json.loads((FIXTURES / "pip-audit-findings.json").read_text(encoding="utf-8"))
    project = _project(policy, "python-tools")
    findings = module.normalize_pip_audit(report, project, _project_lock(project))
    by_package = {finding["package"]["name"]: finding for finding in findings}
    assert by_package["click"]["finding_id"] == "GHSA-7777-8888-9999"
    assert by_package["pytest"]["finding_id"] == "PYSEC-2099-0002"
    assert by_package["pygments"]["finding_id"] == "CVE-2099-0003"


def test_uv_lock_graph_authenticates_current_python_finding_scopes():
    module = _load_evaluator()
    python_package = module._uv_dependency_context(REPO_ROOT / "crates/ferric-rules-python/uv.lock")
    python_tools = module._uv_dependency_context(REPO_ROOT / "tools/ferric-tools/uv.lock")

    assert python_package[("pygments", "2.19.2")] == {
        "dependency_scopes": ["development"],
        "dev_only": True,
    }
    assert python_package[("pytest", "9.0.2")] == {
        "dependency_scopes": ["development"],
        "dev_only": True,
    }
    assert python_package[("pytest", "8.4.2")] == {
        "dependency_scopes": ["development"],
        "dev_only": True,
    }
    assert python_tools[("click", "8.3.1")] == {
        "dependency_scopes": ["runtime"],
        "dev_only": False,
    }
    assert python_tools[("pygments", "2.19.2")] == {
        "dependency_scopes": ["runtime", "development"],
        "dev_only": False,
    }
    assert python_tools[("pytest", "9.0.2")] == {
        "dependency_scopes": ["development"],
        "dev_only": True,
    }


def test_python_package_pytest_marker_fork_has_two_exact_reviewed_exceptions():
    policy = _policy()
    lock = tomllib.loads(
        (REPO_ROOT / "crates/ferric-rules-python/uv.lock").read_text(encoding="utf-8")
    )
    pytest_variants = {
        package["version"]: package["resolution-markers"]
        for package in lock["package"]
        if package["name"] == "pytest"
    }
    assert pytest_variants == {
        "8.4.2": ["python_full_version < '3.10'"],
        "9.0.2": ["python_full_version >= '3.10'"],
    }

    reviewed = [
        exception
        for exception in _exceptions(policy)
        if exception["project_id"] == "python-package"
        and exception["package"]["name"] == "pytest"
        and exception["finding_id"] == "GHSA-6W46-J5RX-G56G"
    ]
    assert {
        (exception["exception_id"], exception["package"]["version"]) for exception in reviewed
    } == {
        ("pypi-python-package-pytest-8-4-2-ghsa-6w46-j5rx-g56g", "8.4.2"),
        ("pypi-python-package-pytest-ghsa-6w46-j5rx-g56g", "9.0.2"),
    }
    shared_context = {
        "ecosystem": "pypi",
        "project_id": "python-package",
        "lockfile": "crates/ferric-rules-python/uv.lock",
        "kind": "vulnerability",
        "finding_id": "GHSA-6W46-J5RX-G56G",
        "scanner_severity": "unknown",
        "dependency_scopes": ["development"],
        "dev_only": True,
        "affected_surfaces": ["python-package-tests"],
        "reachability": "unknown",
        "owner": "python-bindings",
    }
    for exception in reviewed:
        assert {field: exception[field] for field in shared_context} == shared_context

    legacy = next(exception for exception in reviewed if exception["package"]["version"] == "8.4.2")
    evidence_text = " ".join(item["reference"] for item in legacy["evidence"])
    assert "python_full_version<'3.10'" in evidence_text.replace(" ", "")
    remediation = legacy["remediation"].lower()
    assert "8.4.3" in remediation or ("retir" in remediation and "3.9" in remediation)


def test_pypi_duplicate_batch_aliases_are_sorted_deduplicated_before_finding_deduplication():
    module = _load_evaluator()
    project = _project(_policy(), "python-tools")
    vulnerability = {
        "id": "PYSEC-2099-0001",
        "fix_versions": ["8.3.3"],
        "aliases": [
            "GHSA-7777-8888-9999",
            "CVE-2099-0001",
            "GHSA-7777-8888-9999",
        ],
        "description": "same advisory emitted by two marker batches",
    }
    reversed_vulnerability = copy.deepcopy(vulnerability)
    reversed_vulnerability["aliases"] = [
        "CVE-2099-0001",
        "GHSA-7777-8888-9999",
        "CVE-2099-0001",
    ]
    report = {
        "schema": "ferric.pip-audit-batches",
        "version": 1,
        "export_command": [],
        "audit_command": [],
        "reports": [
            {
                "dependencies": [
                    {
                        "name": "click",
                        "version": "8.3.1",
                        "vulns": [vulnerability],
                    }
                ],
                "fixes": [],
            },
            {
                "dependencies": [
                    {
                        "name": "click",
                        "version": "8.3.1",
                        "vulns": [reversed_vulnerability],
                    }
                ],
                "fixes": [],
            },
        ],
    }

    findings = module.normalize_pip_audit(report, project, _project_lock(project))

    assert len(findings) == 1
    assert findings[0]["finding_id"] == "GHSA-7777-8888-9999"
    assert findings[0]["aliases"] == ["CVE-2099-0001", "GHSA-7777-8888-9999"]


def test_pypi_exception_stops_matching_when_uv_graph_moves_pytest_to_runtime():
    module = _load_evaluator()
    policy = copy.deepcopy(_policy())
    project = _project(policy, "python-tools")
    lock = (REPO_ROOT / str(project["lockfile"])).read_text()
    lock = lock.replace('    { name = "pytest" },\n', "", 1)
    lock = lock.replace(
        '    { name = "typer" },\n]',
        '    { name = "typer" },\n    { name = "pytest" },\n]',
        1,
    )
    report = json.loads((FIXTURES / "pip-audit-findings.json").read_text())
    report["dependencies"] = [
        dependency for dependency in report["dependencies"] if dependency["name"] == "pytest"
    ]
    finding = module.normalize_pip_audit(report, project, lock)[0]
    assert finding["dependency_scopes"] == ["runtime"]
    assert finding["dev_only"] is False

    exception = next(
        item
        for item in policy["exceptions"]
        if item["exception_id"] == "pypi-python-tools-pytest-ghsa-6w46-j5rx-g56g"
    )
    policy["exceptions"] = [exception]
    evaluated, exceptions = module.evaluate_findings(policy, [finding], TODAY)
    assert evaluated[0]["status"] == "blocked"
    assert evaluated[0]["exception_id"] is None
    assert exceptions == [
        {
            "exception_id": exception["exception_id"],
            "status": "unused",
            "matched_finding_count": 0,
        }
    ]


@pytest.mark.parametrize("missing_field", ["dependency_scopes", "dev_only"])
def test_pypi_exception_never_supplies_missing_observed_scope_context(missing_field):
    module = _load_evaluator()
    policy = copy.deepcopy(_policy())
    project = _project(policy, "python-tools")
    report = json.loads((FIXTURES / "pip-audit-findings.json").read_text())
    finding = module.normalize_pip_audit(report, project, _project_lock(project))[0]
    policy["exceptions"] = [_exception_for(finding)]
    finding.pop(missing_field)

    evaluated, exceptions = module.evaluate_findings(policy, [finding], TODAY)
    assert evaluated[0]["status"] == "blocked"
    assert evaluated[0]["exception_id"] is None
    assert exceptions == [
        {
            "exception_id": "fixture-exception",
            "status": "unused",
            "matched_finding_count": 0,
        }
    ]


def _uv_scope_fixture(root_edges: str, packages: str) -> str:
    return (
        'version = 1\nrevision = 3\nrequires-python = ">=3.12"\n\n'
        '[[package]]\nname = "fixture-root"\nversion = "0.1.0"\n'
        'source = { editable = "." }\n'
        f"{root_edges}\n\n{packages}"
    )


def _uv_registry_package(name: str, version: str, digit: str) -> str:
    return (
        f'[[package]]\nname = "{name}"\nversion = "{version}"\n'
        'source = { registry = "https://pypi.org/simple" }\n'
        f'sdist = {{ url = "https://example.invalid/{name}.tar.gz", '
        f'hash = "sha256:{digit * 64}", size = 1 }}\n'
    )


def _uv_build_binding_fixture(
    requirement: str,
    metadata_specifier: str,
    *,
    versions: tuple[str, ...] = ("1.0.0",),
    registries: tuple[str, ...] | None = None,
    metadata_marker: str | None = None,
    edge_markers: tuple[str | None, ...] | None = None,
    qualify_edges: bool = True,
) -> tuple[dict[str, object], str]:
    if registries is None:
        registries = ("https://pypi.org/simple",) * len(versions)
    if edge_markers is None:
        edge_markers = (None,) * len(versions)
    assert len(versions) == len(registries) == len(edge_markers)

    edges = []
    packages = []
    for position, (version, registry, marker) in enumerate(
        zip(versions, registries, edge_markers, strict=True),
        start=1,
    ):
        fields = ['name = "build-tool"']
        if qualify_edges:
            fields.extend(
                [
                    f"version = {json.dumps(version)}",
                    f"source = {{ registry = {json.dumps(registry)} }}",
                ]
            )
        if marker is not None:
            fields.append(f"marker = {json.dumps(marker)}")
        edges.append("{ " + ", ".join(fields) + " }")
        packages.append(
            _uv_registry_package("build-tool", version, str(position)).replace(
                'registry = "https://pypi.org/simple"',
                f"registry = {json.dumps(registry)}",
            )
        )

    metadata_fields = [
        'name = "build-tool"',
        f"specifier = {json.dumps(metadata_specifier)}",
    ]
    if metadata_marker is not None:
        metadata_fields.append(f"marker = {json.dumps(metadata_marker)}")
    lock = _uv_scope_fixture(
        "[package.dev-dependencies]\n"
        f"dev = [{', '.join(edges)}]\n\n"
        "[package.metadata]\n\n"
        "[package.metadata.requires-dev]\n"
        f"dev = [{{ {', '.join(metadata_fields)} }}]",
        "\n".join(packages),
    )
    manifest = {
        "build-system": {
            "requires": [requirement],
            "build-backend": "fixture_backend",
        }
    }
    return manifest, lock


@pytest.mark.parametrize("mutation", ["ambiguous", "missing", "unreachable"])
def test_uv_scope_graph_fails_closed_on_unresolved_or_unreachable_packages(mutation):
    module = _load_evaluator()
    reached = _uv_registry_package("reached", "1.0.0", "1")
    if mutation == "ambiguous":
        lock = _uv_scope_fixture(
            'dependencies = [{ name = "duplicate" }]',
            _uv_registry_package("duplicate", "1.0.0", "1")
            + "\n"
            + _uv_registry_package("duplicate", "2.0.0", "2"),
        )
    elif mutation == "missing":
        lock = _uv_scope_fixture('dependencies = [{ name = "missing" }]', reached)
    else:
        lock = _uv_scope_fixture(
            'dependencies = [{ name = "reached" }]',
            reached + "\n" + _uv_registry_package("orphan", "2.0.0", "2"),
        )
    with pytest.raises(module.PolicyError, match=r"ambiguous|resolve|unreachable"):
        module._uv_dependency_context(lock)


def test_uv_root_optional_dependency_tables_are_classified_as_optional():
    module = _load_evaluator()
    lock = _uv_scope_fixture(
        '[package.optional-dependencies]\nextra = [{ name = "optional-package" }]',
        _uv_registry_package("optional-package", "1.0.0", "1"),
    )
    assert module._uv_dependency_context(lock)[("optional-package", "1.0.0")] == {
        "dependency_scopes": ["optional"],
        "dev_only": False,
    }


def test_real_python_build_system_maturin_and_transitive_are_build_reachable():
    module = _load_evaluator()
    project = _project(_policy(), "python-package")

    context = module._uv_dependency_context(
        REPO_ROOT / str(project["lockfile"]),
        REPO_ROOT / str(project["manifest"]),
        include_build_scope=True,
    )

    assert context[("maturin", "1.12.6")] == {
        "dependency_scopes": ["build", "development"],
        "dev_only": False,
    }
    assert context[("tomli", "2.4.0")] == {
        "dependency_scopes": ["build", "development"],
        "dev_only": False,
    }


def test_python_build_requirement_seeds_every_exact_locked_resolution_variant_and_transitive():
    module = _load_evaluator()
    manifest = {
        "build-system": {
            "requires": ["build-tool>=1"],
            "build-backend": "fixture_backend",
        }
    }
    lock = _uv_scope_fixture(
        "[package.dev-dependencies]\n"
        'dev = [{ name = "build-tool", version = "1.0.0", '
        'source = { registry = "https://pypi.org/simple" } }, '
        '{ name = "build-tool", version = "2.0.0", '
        'source = { registry = "https://pypi.org/simple" } }]\n\n'
        "[package.metadata]\n\n"
        "[package.metadata.requires-dev]\n"
        'dev = [{ name = "build-tool", specifier = ">=1" }]',
        _uv_registry_package("build-tool", "1.0.0", "1")
        + 'dependencies = [{ name = "build-child", version = "3.0.0" }]\n'
        + "resolution-markers = [\"python_full_version < '3.11'\"]\n\n"
        + _uv_registry_package("build-tool", "2.0.0", "2")
        + 'dependencies = [{ name = "build-child", version = "3.0.0" }]\n'
        + "resolution-markers = [\"python_full_version >= '3.11'\"]\n\n"
        + _uv_registry_package("build-child", "3.0.0", "3"),
    )

    context = module._uv_dependency_context(lock, manifest, include_build_scope=True)

    for identity in (
        ("build-tool", "1.0.0"),
        ("build-tool", "2.0.0"),
        ("build-child", "3.0.0"),
    ):
        assert context[identity] == {
            "dependency_scopes": ["build", "development"],
            "dev_only": False,
        }


def test_python_build_requirement_rejects_jointly_forged_manifest_and_metadata_version():
    module = _load_evaluator()
    manifest, lock = _uv_build_binding_fixture(
        "build-tool>=999",
        ">=999",
        versions=("1.0.0",),
    )

    with pytest.raises(module.PolicyError, match=r"build|specifier|version|satisf"):
        module._uv_dependency_context(lock, manifest, include_build_scope=True)


def test_python_build_requirement_rejects_joint_version_and_marker_adversary():
    module = _load_evaluator()
    manifest, lock = _uv_build_binding_fixture(
        "build-tool>=999; python_version < '4'",
        ">=999",
        versions=("1.0.0",),
        metadata_marker="python_version < '4'",
        edge_markers=("sys_platform == 'win32'",),
    )

    with pytest.raises(module.PolicyError, match=r"build|marker|specifier|version|satisf"):
        module._uv_dependency_context(lock, manifest, include_build_scope=True)


@pytest.mark.parametrize(
    ("specifier", "versions"),
    [
        (">=1.2.3,<=2.0.0", ("1.2.3", "2.0.0")),
        (">1.2.3,<2.0.0", ("1.2.4", "1.9.9")),
        ("==1.0.0", ("1", "1.0", "1.0.0")),
        ("!=1.2.3", ("1.2.2", "1.2.4")),
    ],
)
def test_python_build_requirement_accepts_supported_numeric_release_boundaries(
    specifier,
    versions,
):
    module = _load_evaluator()
    manifest, lock = _uv_build_binding_fixture(
        f"build-tool{specifier}",
        specifier,
        versions=versions,
    )

    context = module._uv_dependency_context(lock, manifest, include_build_scope=True)

    for version in versions:
        assert context[("build-tool", version)] == {
            "dependency_scopes": ["build", "development"],
            "dev_only": False,
        }


@pytest.mark.parametrize(
    ("specifier", "version"),
    [
        (">1.2.3", "1.2.3"),
        ("<2.0.0", "2.0.0"),
        (">=1.2.3", "1.2.2"),
        ("<=2.0.0", "2.0.1"),
        ("==1.0", "1.0.1"),
        ("!=1.2.3", "1.2.3"),
    ],
)
def test_python_build_requirement_rejects_versions_outside_numeric_release_bounds(
    specifier,
    version,
):
    module = _load_evaluator()
    manifest, lock = _uv_build_binding_fixture(
        f"build-tool{specifier}",
        specifier,
        versions=(version,),
    )

    with pytest.raises(module.PolicyError, match=r"build|specifier|version|satisf"):
        module._uv_dependency_context(lock, manifest, include_build_scope=True)


def test_python_build_requirement_rejects_group_when_any_selected_variant_is_out_of_range():
    module = _load_evaluator()
    manifest, lock = _uv_build_binding_fixture(
        "build-tool>=1,<2",
        ">=1,<2",
        versions=("1.9.9", "2.0.0"),
    )

    with pytest.raises(module.PolicyError, match=r"build|specifier|version|satisf"):
        module._uv_dependency_context(lock, manifest, include_build_scope=True)


@pytest.mark.parametrize(
    "specifier",
    [
        "",
        "~=1.2",
        "===1.2.3",
        "==1.*",
        ">=1!1.0",
        ">=1.0rc1",
        ">=1.0.post1",
        ">=1.0.dev1",
        "==1.0+local",
        ">=01.2",
        ">=1..2",
        ">=1,",
        ">=1,,<2",
    ],
)
def test_python_build_requirement_rejects_unsupported_or_empty_pep440_specifiers(
    specifier,
):
    module = _load_evaluator()
    manifest, lock = _uv_build_binding_fixture(
        f"build-tool{specifier}",
        specifier,
        versions=("1.2.3",),
    )

    with pytest.raises(module.PolicyError, match=r"build|specifier|version|unsupported|empty"):
        module._uv_dependency_context(lock, manifest, include_build_scope=True)


@pytest.mark.parametrize(
    "version",
    [
        "1.0rc1",
        "1.0.post1",
        "1.0.dev1",
        "1.0+local",
        "1!1.0",
        "01.2",
        "1..2",
    ],
)
def test_python_build_requirement_rejects_nonnumeric_constrained_locked_versions(version):
    module = _load_evaluator()
    manifest, lock = _uv_build_binding_fixture(
        "build-tool>=1",
        ">=1",
        versions=(version,),
    )

    with pytest.raises(module.PolicyError, match=r"build|specifier|version|unsupported"):
        module._uv_dependency_context(lock, manifest, include_build_scope=True)


@pytest.mark.parametrize(
    ("requirement", "metadata_marker", "edge_marker"),
    [
        (
            "build-tool>=1; python_version < '4'",
            "python_version < '4'",
            None,
        ),
        (
            "build-tool>=1; python_version < '4'",
            "python_version < '4'",
            "python_version < '4'",
        ),
        ("build-tool>=1", None, "python_version < '4'"),
        ("build-tool>=1", None, "sys_platform == 'win32'"),
    ],
)
def test_python_build_requirement_rejects_build_or_selected_edge_markers(
    requirement,
    metadata_marker,
    edge_marker,
):
    module = _load_evaluator()
    manifest, lock = _uv_build_binding_fixture(
        requirement,
        ">=1",
        metadata_marker=metadata_marker,
        edge_markers=(edge_marker,),
    )

    with pytest.raises(module.PolicyError, match=r"build|marker|unsupported"):
        module._uv_dependency_context(lock, manifest, include_build_scope=True)


@pytest.mark.parametrize(
    "registry",
    [
        "https://mirror.invalid/simple",
        "https://pypi.org/simple/",
        "http://pypi.org/simple",
    ],
)
def test_python_build_requirement_rejects_noncanonical_pypi_registry_sources(registry):
    module = _load_evaluator()
    manifest, lock = _uv_build_binding_fixture(
        "build-tool==1.0.0",
        "==1.0.0",
        registries=(registry,),
    )

    with pytest.raises(module.PolicyError, match=r"build|pypi|registry|source"):
        module._uv_dependency_context(lock, manifest, include_build_scope=True)


def test_python_build_requirement_accepts_unique_implicit_official_registry_binding():
    module = _load_evaluator()
    manifest, lock = _uv_build_binding_fixture(
        "build-tool==1.0.0",
        "==1.0.0",
        qualify_edges=False,
    )

    context = module._uv_dependency_context(lock, manifest, include_build_scope=True)

    assert context[("build-tool", "1.0.0")] == {
        "dependency_scopes": ["build", "development"],
        "dev_only": False,
    }


def test_python_build_requirement_rejects_implicit_edge_ambiguous_across_versions():
    module = _load_evaluator()
    manifest, lock = _uv_build_binding_fixture(
        "build-tool>=1,<2",
        ">=1,<2",
        versions=("1.0.0", "1.1.0"),
        qualify_edges=False,
    )

    with pytest.raises(module.PolicyError, match=r"ambiguous|resolve|build"):
        module._uv_dependency_context(lock, manifest, include_build_scope=True)


@pytest.mark.parametrize(
    "shadow_requirement",
    [
        '{ name = "build-tool", specifier = ">=0" }',
        '{ name = "build-tool", specifier = "==1.0.0", marker = "python_version < \'4\'" }',
    ],
)
def test_python_build_requirement_rejects_same_name_metadata_ambiguity_across_groups(
    shadow_requirement,
):
    module = _load_evaluator()
    manifest = {
        "build-system": {
            "requires": ["build-tool==1.0.0"],
            "build-backend": "fixture_backend",
        }
    }
    lock = _uv_scope_fixture(
        "[package.dev-dependencies]\n"
        'build = [{ name = "build-tool", version = "1.0.0", '
        'source = { registry = "https://pypi.org/simple" } }]\n'
        "shadow = []\n\n"
        "[package.metadata]\n\n"
        "[package.metadata.requires-dev]\n"
        'build = [{ name = "build-tool", specifier = "==1.0.0" }]\n'
        f"shadow = [{shadow_requirement}]",
        _uv_registry_package("build-tool", "1.0.0", "1"),
    )

    with pytest.raises(module.PolicyError, match=r"build|metadata|exactly one|ambiguous"):
        module._uv_dependency_context(lock, manifest, include_build_scope=True)


@pytest.mark.parametrize(
    "requires",
    [
        ["not-locked>=1"],
        ["build-tool>=999"],
        ["build-tool @ https://example.invalid/build-tool.whl"],
    ],
)
def test_python_build_requirements_fail_closed_when_lock_binding_is_missing_or_ambiguous(
    requires,
):
    module = _load_evaluator()
    manifest = {"build-system": {"requires": requires, "build-backend": "fixture_backend"}}
    lock = _uv_scope_fixture(
        '[package.dev-dependencies]\ndev = [{ name = "build-tool", version = "1.0.0" }]\n\n'
        "[package.metadata]\n\n"
        "[package.metadata.requires-dev]\n"
        'dev = [{ name = "build-tool", specifier = ">=1" }]',
        _uv_registry_package("build-tool", "1.0.0", "1"),
    )

    with pytest.raises(module.PolicyError, match=r"build|require|lock|resolve|unsupported"):
        module._uv_dependency_context(lock, manifest, include_build_scope=True)


def test_python_build_requirement_rejects_same_identity_from_ambiguous_registry_sources():
    module = _load_evaluator()
    manifest = {
        "build-system": {
            "requires": ["build-tool==1.0.0"],
            "build-backend": "fixture_backend",
        }
    }
    public = 'source = { registry = "https://pypi.org/simple" }'
    mirror = 'source = { registry = "https://mirror.invalid/simple" }'
    lock = _uv_scope_fixture(
        "[package.dev-dependencies]\n"
        'dev = [{ name = "build-tool", version = "1.0.0", '
        'source = { registry = "https://pypi.org/simple" } }, '
        '{ name = "build-tool", version = "1.0.0", '
        'source = { registry = "https://mirror.invalid/simple" } }]\n\n'
        "[package.metadata]\n\n"
        "[package.metadata.requires-dev]\n"
        'dev = [{ name = "build-tool", specifier = "==1.0.0" }]',
        _uv_registry_package("build-tool", "1.0.0", "1")
        + "\n"
        + _uv_registry_package("build-tool", "1.0.0", "2").replace(public, mirror),
    )

    with pytest.raises(module.PolicyError, match=r"ambiguous|source|registry"):
        module._uv_dependency_context(lock, manifest, include_build_scope=True)


def test_non_build_python_surface_validates_build_system_shape_without_lock_seeding():
    module = _load_evaluator()
    project = _project(_policy(), "python-tools")
    context = module._uv_dependency_context(
        REPO_ROOT / str(project["lockfile"]),
        REPO_ROOT / str(project["manifest"]),
        include_build_scope=False,
    )
    assert context
    assert all("build" not in item["dependency_scopes"] for item in context.values())

    malformed_manifest = tomllib.loads(_project_manifest(project).decode())
    malformed_manifest["build-system"]["requires"] = "hatchling"
    with pytest.raises(module.PolicyError, match=r"build-system|requires"):
        module._uv_dependency_context(
            _project_lock(project),
            malformed_manifest,
            include_build_scope=False,
        )


def test_python_build_scope_requires_an_authenticated_manifest_at_the_context_api():
    module = _load_evaluator()
    project = _project(_policy(), "python-package")

    with pytest.raises(module.PolicyError, match=r"manifest|pyproject|build"):
        module._uv_dependency_context(
            _project_lock(project),
            include_build_scope=True,
        )


def test_python_non_build_context_may_explicitly_omit_manifest_without_build_seeding():
    module = _load_evaluator()
    project = _project(_policy(), "python-tools")

    context = module._uv_dependency_context(
        _project_lock(project),
        include_build_scope=False,
    )

    assert context
    assert all("build" not in item["dependency_scopes"] for item in context.values())


def test_python_build_declared_normalizer_requires_an_authenticated_manifest():
    module = _load_evaluator()
    project = _project(_policy(), "python-package")
    report = json.loads((FIXTURES / "pip-audit-findings.json").read_text())

    with pytest.raises(module.PolicyError, match=r"manifest|pyproject|build"):
        module.normalize_pip_audit(report, project, _project_lock(project))


@pytest.mark.parametrize("normalizer", ["cargo", "deny", "npm", "pip"])
def test_scanner_error_and_malformed_payloads_fail_closed(normalizer):
    module = _load_evaluator()
    policy = _policy()
    with pytest.raises(module.PolicyError):
        if normalizer == "cargo":
            module.normalize_cargo_audit({"error": "database unavailable"})
        elif normalizer == "deny":
            module.normalize_cargo_deny({"error": "cargo-deny crashed"})
        elif normalizer == "npm":
            module.normalize_npm_audit(
                {"error": {"code": "EAUDIT"}},
                _project(policy, "node-package"),
                {},
            )
        else:
            module.normalize_pip_audit(
                {"error": "pip-audit failed"},
                (project := _project(policy, "python-tools")),
                _project_lock(project),
            )


@pytest.mark.parametrize("mutation", ["missing-list", "wrong-list", "count", "found"])
def test_cargo_audit_rejects_truncated_or_internally_inconsistent_results(mutation):
    module = _load_evaluator()
    report = json.loads((FIXTURES / "cargo-audit-findings.json").read_text(encoding="utf-8"))
    vulnerabilities = report["vulnerabilities"]
    if mutation == "missing-list":
        vulnerabilities.pop("list")
    elif mutation == "wrong-list":
        vulnerabilities["list"] = {}
    elif mutation == "count":
        vulnerabilities["count"] = 0
    else:
        vulnerabilities["found"] = False
    with pytest.raises(module.PolicyError):
        module.normalize_cargo_audit(report)


@pytest.mark.parametrize(
    ("field", "replacement"),
    [("via", None), ("via", {}), ("nodes", None), ("nodes", {})],
)
def test_npm_audit_rejects_missing_or_non_list_nested_result_fields(field, replacement):
    module = _load_evaluator()
    policy = _policy()
    report = json.loads((FIXTURES / "npm-audit-findings.json").read_text(encoding="utf-8"))
    vulnerability = report["vulnerabilities"]["detect-libc"]
    if replacement is None:
        vulnerability.pop(field)
    else:
        vulnerability[field] = replacement
    package_lock = json.loads(
        (REPO_ROOT / "packages/ferric/package-lock.json").read_text(encoding="utf-8")
    )
    with pytest.raises(module.PolicyError):
        module.normalize_npm_audit(
            report,
            _project(policy, "node-package"),
            package_lock,
        )


def test_npm_metadata_counts_vulnerability_objects_not_concrete_via_advisories():
    module = _load_evaluator()
    policy = _policy()
    report = json.loads((FIXTURES / "npm-audit-findings.json").read_text(encoding="utf-8"))
    second_advisory = copy.deepcopy(report["vulnerabilities"]["detect-libc"]["via"][0])
    second_advisory.update(
        {
            "source": 990003,
            "url": "https://github.com/advisories/GHSA-aaaa-bbbb-cccc",
            "title": "second advisory on the same vulnerability object",
        }
    )
    report["vulnerabilities"]["detect-libc"]["via"].append(second_advisory)
    package_lock = json.loads(
        (REPO_ROOT / "packages/ferric/package-lock.json").read_text(encoding="utf-8")
    )

    findings = module.normalize_npm_audit(
        report,
        _project(policy, "node-package"),
        package_lock,
    )

    assert report["metadata"]["vulnerabilities"]["total"] == 2
    assert len(report["vulnerabilities"]) == 2
    assert len(findings) == 3


@pytest.mark.parametrize(
    "mutation",
    [
        "missing-metadata",
        "metadata-not-object",
        "missing-counts",
        "counts-not-object",
        "missing-bucket",
        "string-count",
        "float-count",
        "boolean-count",
        "negative-count",
        "total-not-sum",
        "object-count",
        "severity-histogram",
        "missing-object-severity",
        "invalid-object-severity",
    ],
)
def test_npm_audit_rejects_invalid_metadata_counts_and_object_severity_histogram(mutation):
    module = _load_evaluator()
    policy = _policy()
    report = json.loads((FIXTURES / "npm-audit-findings.json").read_text(encoding="utf-8"))
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
        counts.pop("high")
    elif mutation == "string-count":
        counts["low"] = "1"
    elif mutation == "float-count":
        counts["low"] = 1.0
    elif mutation == "boolean-count":
        counts["low"] = True
    elif mutation == "negative-count":
        counts.update({"low": -1, "high": 2})
    elif mutation == "total-not-sum":
        counts["total"] = 3
    elif mutation == "object-count":
        report["vulnerabilities"].pop("esbuild")
    elif mutation == "severity-histogram":
        counts.update({"low": 0, "high": 1})
    elif mutation == "missing-object-severity":
        report["vulnerabilities"]["detect-libc"].pop("severity")
    else:
        report["vulnerabilities"]["detect-libc"]["severity"] = "unknown"
    package_lock = json.loads(
        (REPO_ROOT / "packages/ferric/package-lock.json").read_text(encoding="utf-8")
    )
    with pytest.raises(module.PolicyError):
        module.normalize_npm_audit(
            report,
            _project(policy, "node-package"),
            package_lock,
        )


def test_npm_string_via_reference_uses_the_referenced_vulnerability_nodes():
    module = _load_evaluator()
    policy = _policy()
    report = json.loads((FIXTURES / "npm-audit-findings.json").read_text(encoding="utf-8"))
    report["vulnerabilities"]["detect-libc"]["via"] = ["esbuild"]
    package_lock = json.loads(
        (REPO_ROOT / "packages/ferric/package-lock.json").read_text(encoding="utf-8")
    )

    findings = module.normalize_npm_audit(
        report,
        _project(policy, "node-package"),
        package_lock,
    )

    assert {finding["finding_id"] for finding in findings} == {"GHSA-4444-5555-6666"}
    assert {
        (finding["package"]["name"], finding["package"]["version"]) for finding in findings
    } == {("esbuild", "0.27.7")}


def test_npm_effect_only_parent_nodes_remain_fail_closed():
    module = _load_evaluator()
    policy = _policy()
    report = json.loads((FIXTURES / "npm-audit-findings.json").read_text(encoding="utf-8"))
    report["vulnerabilities"]["detect-libc"]["via"] = ["esbuild"]
    report["vulnerabilities"]["detect-libc"]["nodes"] = ["node_modules/not-in-lock"]
    package_lock = json.loads(
        (REPO_ROOT / "packages/ferric/package-lock.json").read_text(encoding="utf-8")
    )

    with pytest.raises(module.PolicyError, match=r"detect-libc.*absent.*lock"):
        module.normalize_npm_audit(
            report,
            _project(policy, "node-package"),
            package_lock,
        )


def test_npm_duplicate_package_identity_merges_all_installed_path_scopes():
    module = _load_evaluator()
    policy = _policy()
    report = json.loads((FIXTURES / "npm-audit-findings.json").read_text(encoding="utf-8"))
    package_lock = json.loads(
        (REPO_ROOT / "packages/ferric/package-lock.json").read_text(encoding="utf-8")
    )
    duplicate_path = "node_modules/runtime-parent/node_modules/esbuild"
    duplicate = copy.deepcopy(package_lock["packages"]["node_modules/esbuild"])
    duplicate.pop("dev")
    duplicate["optional"] = True
    package_lock["packages"][duplicate_path] = duplicate
    report["vulnerabilities"]["esbuild"]["nodes"].append(duplicate_path)
    report["metadata"]["dependencies"] = module._npm_audit_dependency_counts(package_lock)

    findings = module.normalize_npm_audit(
        report,
        _project(policy, "node-package"),
        package_lock,
    )

    esbuild = next(finding for finding in findings if finding["package"]["name"] == "esbuild")
    assert esbuild["package"]["version"] == "0.27.7"
    assert esbuild["dependency_scopes"] == ["runtime", "development", "optional"]
    assert esbuild["dev_only"] is False
    assert len([finding for finding in findings if finding["package"]["name"] == "esbuild"]) == 1


@pytest.mark.parametrize(
    ("package_name", "version"),
    [("typescript", "6.0.3"), ("yaml", "2.9.0")],
)
def test_site_dev_optional_nodes_are_runtime_build_and_optional_reachable(package_name, version):
    module = _load_evaluator()
    package_lock = json.loads((REPO_ROOT / "site/package-lock.json").read_text(encoding="utf-8"))

    versions = module._npm_versions(
        package_lock,
        [f"node_modules/{package_name}"],
        package_name,
        include_build_scope=True,
    )

    assert versions == [
        (
            package_name,
            version,
            ["runtime", "build", "development", "optional"],
            False,
        )
    ]


@pytest.mark.parametrize("bad_dev_optional", ["true", 1, None])
def test_npm_dev_optional_flag_must_be_boolean(bad_dev_optional):
    module = _load_evaluator()
    package_lock = json.loads(
        (REPO_ROOT / "packages/ferric/package-lock.json").read_text(encoding="utf-8")
    )
    node = "node_modules/esbuild"
    package_lock["packages"][node]["devOptional"] = bad_dev_optional

    with pytest.raises(module.PolicyError, match=r"devOptional.*boolean"):
        module._npm_versions(
            package_lock,
            [node],
            "esbuild",
            include_build_scope=False,
        )


def test_npm_duplicate_dev_and_dev_optional_paths_are_not_dev_only():
    module = _load_evaluator()
    policy = _policy()
    report = json.loads((FIXTURES / "npm-audit-findings.json").read_text(encoding="utf-8"))
    package_lock = json.loads(
        (REPO_ROOT / "packages/ferric/package-lock.json").read_text(encoding="utf-8")
    )
    duplicate_path = "node_modules/dev-parent/node_modules/esbuild"
    duplicate = copy.deepcopy(package_lock["packages"]["node_modules/esbuild"])
    duplicate.pop("dev")
    duplicate["devOptional"] = True
    package_lock["packages"][duplicate_path] = duplicate
    report["vulnerabilities"]["esbuild"]["nodes"].append(duplicate_path)
    report["metadata"]["dependencies"] = module._npm_audit_dependency_counts(package_lock)

    findings = module.normalize_npm_audit(
        report,
        _project(policy, "node-package"),
        package_lock,
    )

    esbuild = next(finding for finding in findings if finding["package"]["name"] == "esbuild")
    assert esbuild["dependency_scopes"] == ["runtime", "development", "optional"]
    assert esbuild["dev_only"] is False


@pytest.mark.parametrize(
    ("project_id", "node", "expected_scopes", "expected_dev_only"),
    [
        ("node-package", "node_modules/typescript", ["development"], True),
        (
            "node-addon",
            "node_modules/@napi-rs/cli",
            ["build", "development"],
            False,
        ),
        ("documentation", "node_modules/ajv", ["development"], True),
        ("site", "node_modules/cspell", ["build", "development"], False),
    ],
)
def test_all_real_npm_surfaces_authenticate_build_scope_from_manifest_and_lock(
    project_id, node, expected_scopes, expected_dev_only
):
    module = _load_evaluator()
    project = _project(_policy(), project_id)
    package_lock = json.loads(_project_lock(project))
    report = _npm_single_node_report(module, package_lock, node)

    finding = module.normalize_npm_audit(
        report,
        project,
        package_lock,
        _project_manifest(project),
    )[0]

    assert finding["package"] == {
        "name": package_lock["packages"][node].get("name") or node.rsplit("node_modules/", 1)[-1],
        "version": package_lock["packages"][node]["version"],
    }
    assert finding["dependency_scopes"] == expected_scopes
    assert finding["dev_only"] is expected_dev_only


def test_npm_build_declared_normalizer_requires_an_authenticated_manifest():
    module = _load_evaluator()
    project = _project(_policy(), "node-addon")
    package_lock = json.loads(_project_lock(project))
    report = _npm_single_node_report(module, package_lock, "node_modules/@napi-rs/cli")

    with pytest.raises(module.PolicyError, match=r"manifest|package.json|build"):
        module.normalize_npm_audit(report, project, package_lock)


def _npm_build_graph_fixture():
    manifest = {
        "name": "build-fixture",
        "version": "1.0.0",
        "scripts": {"prebuild": "prepare", "build": "compile", "postbuild": "verify"},
        "devDependencies": {"build-tool": "1.0.0"},
    }
    lock = {
        "name": "build-fixture",
        "version": "1.0.0",
        "lockfileVersion": 3,
        "requires": True,
        "packages": {
            "": {
                "name": "build-fixture",
                "version": "1.0.0",
                "devDependencies": {"build-tool": "1.0.0"},
            },
            "node_modules/build-tool": {
                "name": "build-tool",
                "version": "1.0.0",
                "dev": True,
                "dependencies": {"build-child": "2.0.0"},
            },
            "node_modules/build-child": {
                "name": "build-child",
                "version": "2.0.0",
                "dev": True,
            },
        },
    }
    return manifest, lock


def test_npm_build_scope_propagates_from_authenticated_direct_tool_to_transitives():
    module = _load_evaluator()
    manifest, lock = _npm_build_graph_fixture()

    assert module._npm_build_reachable_nodes(manifest, lock) == {
        "node_modules/build-tool",
        "node_modules/build-child",
    }


@pytest.mark.parametrize(
    "mutation",
    [
        "stale-manifest-root",
        "missing-transitive",
        "malformed-lifecycle",
        "malformed-transitive-map",
    ],
)
def test_npm_build_graph_fails_closed_on_stale_or_ambiguous_contracts(mutation):
    module = _load_evaluator()
    manifest, lock = _npm_build_graph_fixture()
    if mutation == "stale-manifest-root":
        manifest["devDependencies"] = {"different-tool": "1.0.0"}
    elif mutation == "missing-transitive":
        lock["packages"].pop("node_modules/build-child")
    elif mutation == "malformed-lifecycle":
        manifest["scripts"]["build"] = ["compile"]
    else:
        lock["packages"]["node_modules/build-tool"]["dependencies"] = ["build-child"]

    with pytest.raises(module.PolicyError, match=r"manifest|lock|script|build|depend|resolve"):
        module._npm_build_reachable_nodes(manifest, lock)


@pytest.mark.parametrize("mutation", ["unknown", "cycle", "self-cycle"])
def test_npm_string_via_unknown_reference_or_cycle_fails_closed(mutation):
    module = _load_evaluator()
    policy = _policy()
    report = json.loads((FIXTURES / "npm-audit-findings.json").read_text(encoding="utf-8"))
    if mutation == "unknown":
        report["vulnerabilities"]["detect-libc"]["via"] = ["not-a-vulnerability-key"]
    elif mutation == "cycle":
        report["vulnerabilities"]["detect-libc"]["via"] = ["esbuild"]
        report["vulnerabilities"]["esbuild"]["via"] = ["detect-libc"]
    else:
        report["vulnerabilities"]["detect-libc"]["via"] = ["detect-libc"]
    package_lock = json.loads(
        (REPO_ROOT / "packages/ferric/package-lock.json").read_text(encoding="utf-8")
    )

    with pytest.raises(module.PolicyError, match=r"via|reference|cycle|unknown"):
        module.normalize_npm_audit(
            report,
            _project(policy, "node-package"),
            package_lock,
        )


@pytest.mark.parametrize("replacement", [None, {}])
def test_pip_audit_rejects_missing_or_non_list_dependency_vulns(replacement):
    module = _load_evaluator()
    policy = _policy()
    report = json.loads((FIXTURES / "pip-audit-findings.json").read_text(encoding="utf-8"))
    dependency = report["dependencies"][0]
    if replacement is None:
        dependency.pop("vulns")
    else:
        dependency["vulns"] = replacement
    with pytest.raises(module.PolicyError):
        project = _project(policy, "python-tools")
        module.normalize_pip_audit(report, project, _project_lock(project))


@pytest.mark.parametrize(
    ("tool", "output"),
    [
        ("rustc", "rustc 1.93.0 (fixture 2026-08-01)"),
        ("cargo_audit", "cargo-audit 0.22.2"),
        ("npm", "11.12.1"),
        ("node", "v22.18.0"),
    ],
)
def test_tool_version_parser_accepts_only_exact_stable_pins(tool, output):
    module = _load_evaluator()
    module.verify_tool_version_output(tool, output)


@pytest.mark.parametrize(
    ("tool", "output"),
    [
        ("rustc", "rustc 1.93.0-nightly (fixture 2026-08-01)"),
        ("npm", "11.12.1-rc.1"),
        ("cargo_audit", "cargo-audit 0.22.20"),
        ("cargo_audit", "wrapper 9.9.9; cargo-audit 0.22.2"),
    ],
)
def test_tool_version_parser_rejects_prerelease_wrong_or_nonfirst_pins(tool, output):
    module = _load_evaluator()
    with pytest.raises(module.PolicyError):
        module.verify_tool_version_output(tool, output)


def test_every_finding_blocks_regardless_of_severity_scope_or_reachability():
    module = _load_evaluator()
    policy = copy.deepcopy(_policy())
    policy["exceptions"] = []
    npm_report = json.loads((FIXTURES / "npm-audit-findings.json").read_text(encoding="utf-8"))
    package_lock = json.loads(
        (REPO_ROOT / "packages/ferric/package-lock.json").read_text(encoding="utf-8")
    )
    findings = module.normalize_npm_audit(
        npm_report,
        _project(policy, "node-package"),
        package_lock,
    )
    evaluated, exceptions = module.evaluate_findings(policy, findings, TODAY)
    assert exceptions == []
    assert {finding["status"] for finding in evaluated} == {"blocked"}
    assert {finding["scanner_severity"] for finding in evaluated} == {"critical", "low"}
    assert {finding["dev_only"] for finding in evaluated} == {False, True}


def test_unmaintained_unsound_notice_yanked_and_unknown_license_all_block():
    module = _load_evaluator()
    policy = copy.deepcopy(_policy())
    policy["exceptions"] = []
    cargo = json.loads((FIXTURES / "cargo-audit-findings.json").read_text(encoding="utf-8"))
    deny = json.loads((FIXTURES / "cargo-deny-findings.json").read_text(encoding="utf-8"))
    findings = module.normalize_cargo_audit(cargo) + module.normalize_cargo_deny(deny)
    evaluated, exceptions = module.evaluate_findings(policy, findings, TODAY)
    assert exceptions == []
    assert {finding["kind"] for finding in evaluated} == {
        "vulnerability",
        "unmaintained",
        "unsound",
        "notice",
        "yanked",
        "license",
        "source",
        "ban",
    }
    assert all(finding["status"] == "blocked" for finding in evaluated)
    unknown_license = next(finding for finding in evaluated if finding["kind"] == "license")
    assert unknown_license["scanner_severity"] == "unknown"
    assert unknown_license["exception_id"] is None


def test_one_exact_active_exception_matches_one_finding():
    module = _load_evaluator()
    policy = copy.deepcopy(_policy())
    report = json.loads((FIXTURES / "pip-audit-findings.json").read_text(encoding="utf-8"))
    project = _project(policy, "python-tools")
    finding = module.normalize_pip_audit(report, project, _project_lock(project))[0]
    policy["exceptions"] = [_exception_for(finding)]
    evaluated, exceptions = module.evaluate_findings(policy, [finding], TODAY)
    assert evaluated[0]["status"] == "excepted"
    assert evaluated[0]["exception_id"] == "fixture-exception"
    assert exceptions == [
        {"exception_id": "fixture-exception", "status": "active", "matched_finding_count": 1}
    ]


@pytest.mark.parametrize("observed_digest", [None, "0" * 64])
def test_cargo_graph_digest_is_observed_context_and_never_supplied_by_exception(
    observed_digest,
):
    module = _load_evaluator()
    policy = copy.deepcopy(_policy())
    report = json.loads((FIXTURES / "cargo-audit-findings.json").read_text(encoding="utf-8"))
    finding = module.normalize_cargo_audit(report)[0]
    finding["cargo_graph_sha256"] = CARGO_GRAPH_SHA256
    exception = _exception_for(finding)
    exception["cargo_graph_sha256"] = CARGO_GRAPH_SHA256
    policy["exceptions"] = [exception]

    exact, exact_exceptions = module.evaluate_findings(policy, [finding], TODAY)
    assert exact[0]["status"] == "excepted"
    assert exact[0]["cargo_graph_sha256"] == CARGO_GRAPH_SHA256
    assert exact_exceptions == [
        {"exception_id": "fixture-exception", "status": "active", "matched_finding_count": 1}
    ]

    drifted = copy.deepcopy(finding)
    if observed_digest is None:
        drifted.pop("cargo_graph_sha256")
    else:
        drifted["cargo_graph_sha256"] = observed_digest
    evaluated, exceptions = module.evaluate_findings(policy, [drifted], TODAY)

    assert evaluated[0]["status"] == "blocked"
    assert evaluated[0]["exception_id"] is None
    assert evaluated[0].get("cargo_graph_sha256") == observed_digest
    assert exceptions == [
        {"exception_id": "fixture-exception", "status": "unused", "matched_finding_count": 0}
    ]


@pytest.mark.parametrize("ecosystem", ["npm", "pypi"])
def test_cargo_graph_digest_does_not_change_foreign_exception_matching(ecosystem):
    module = _load_evaluator()
    policy = copy.deepcopy(_policy())
    assert {
        exception.get("cargo_graph_sha256")
        for exception in policy["exceptions"]
        if exception["ecosystem"] == "cargo"
    } == {CARGO_GRAPH_SHA256}
    if ecosystem == "npm":
        project = _project(policy, "node-package")
        report = json.loads((FIXTURES / "npm-audit-findings.json").read_text())
        lock = json.loads(_project_lock(project))
        finding = module.normalize_npm_audit(report, project, lock)[0]
    else:
        project = _project(policy, "python-tools")
        report = json.loads((FIXTURES / "pip-audit-findings.json").read_text())
        finding = module.normalize_pip_audit(report, project, _project_lock(project))[0]
    exception = _exception_for(finding)
    assert "cargo_graph_sha256" not in exception
    policy["exceptions"] = [exception]

    evaluated, exceptions = module.evaluate_findings(policy, [finding], TODAY)

    assert evaluated[0]["status"] == "excepted"
    assert "cargo_graph_sha256" not in evaluated[0]
    assert exceptions == [
        {"exception_id": "fixture-exception", "status": "active", "matched_finding_count": 1}
    ]


@pytest.mark.parametrize("observed_severity", ["critical", "moderate", "unknown"])
def test_exception_severity_drift_blocks_finding_and_leaves_exception_unused(
    observed_severity,
):
    module = _load_evaluator()
    policy = copy.deepcopy(_policy())
    report = json.loads((FIXTURES / "npm-audit-findings.json").read_text(encoding="utf-8"))
    package_lock = json.loads(
        (REPO_ROOT / "packages/ferric/package-lock.json").read_text(encoding="utf-8")
    )
    findings = module.normalize_npm_audit(
        report,
        _project(policy, "node-package"),
        package_lock,
    )
    low = next(finding for finding in findings if finding["scanner_severity"] == "low")
    exception = _exception_for(low)
    policy["exceptions"] = [exception]

    exact, exact_exceptions = module.evaluate_findings(policy, [low], TODAY)
    assert exact[0]["status"] == "excepted"
    assert exact_exceptions[0] == {
        "exception_id": "fixture-exception",
        "status": "active",
        "matched_finding_count": 1,
    }

    drifted = copy.deepcopy(low)
    drifted["scanner_severity"] = observed_severity
    evaluated, exceptions = module.evaluate_findings(policy, [drifted], TODAY)

    assert evaluated[0]["status"] == "blocked"
    assert evaluated[0]["exception_id"] is None
    assert exceptions[0] == {
        "exception_id": "fixture-exception",
        "status": "unused",
        "matched_finding_count": 0,
    }


def test_dev_only_exception_does_not_match_runtime_scope_context_drift():
    module = _load_evaluator()
    policy = copy.deepcopy(_policy())
    report = json.loads((FIXTURES / "npm-audit-findings.json").read_text(encoding="utf-8"))
    package_lock = json.loads(
        (REPO_ROOT / "packages/ferric/package-lock.json").read_text(encoding="utf-8")
    )
    findings = module.normalize_npm_audit(
        report,
        _project(policy, "node-package"),
        package_lock,
    )
    development = next(finding for finding in findings if finding["dev_only"] is True)
    assert development["dependency_scopes"] == ["development"]
    policy["exceptions"] = [_exception_for(development)]

    exact, exact_exceptions = module.evaluate_findings(policy, [development], TODAY)
    assert exact[0]["status"] == "excepted"
    assert exact_exceptions[0]["matched_finding_count"] == 1

    runtime = copy.deepcopy(development)
    runtime["dependency_scopes"] = ["runtime"]
    runtime["dev_only"] = False
    evaluated, exceptions = module.evaluate_findings(policy, [runtime], TODAY)

    assert evaluated[0]["status"] == "blocked"
    assert evaluated[0]["exception_id"] is None
    assert exceptions[0] == {
        "exception_id": "fixture-exception",
        "status": "unused",
        "matched_finding_count": 0,
    }


@pytest.mark.parametrize(
    ("field", "replacement"),
    [
        ("kind", "notice"),
        ("finding_id", "GHSA-0000-0000-0000"),
        ("package.name", "different-package"),
        ("package.version", "999.0.0"),
    ],
)
def test_exception_matching_rejects_every_wrong_identity_dimension(field, replacement):
    module = _load_evaluator()
    policy = copy.deepcopy(_policy())
    report = json.loads((FIXTURES / "pip-audit-findings.json").read_text(encoding="utf-8"))
    project = _project(policy, "python-tools")
    finding = module.normalize_pip_audit(report, project, _project_lock(project))[0]
    exception = _exception_for(finding)
    if field.startswith("package."):
        exception["package"][field.split(".", 1)[1]] = replacement
    else:
        exception[field] = replacement
    policy["exceptions"] = [exception]
    evaluated, exceptions = module.evaluate_findings(policy, [finding], TODAY)
    assert evaluated[0]["status"] == "blocked"
    assert evaluated[0]["exception_id"] is None
    assert exceptions[0]["status"] == "unused"
    assert exceptions[0]["matched_finding_count"] == 0


@pytest.mark.parametrize(
    ("field", "replacement"),
    [
        ("ecosystem", "npm"),
        ("project_id", "site"),
        ("lockfile", "site/package-lock.json"),
    ],
)
def test_cross_project_ecosystem_or_lock_exception_is_policy_invalid(field, replacement):
    module = _load_evaluator()
    policy = copy.deepcopy(_policy())
    report = json.loads((FIXTURES / "pip-audit-findings.json").read_text(encoding="utf-8"))
    project = _project(policy, "python-tools")
    finding = module.normalize_pip_audit(report, project, _project_lock(project))[0]
    exception = _exception_for(finding)
    exception[field] = replacement
    policy["exceptions"] = [exception]
    with pytest.raises(module.PolicyError):
        module.evaluate_findings(policy, [finding], TODAY)


def test_exception_is_expired_on_the_expiry_day_and_unused_when_finding_is_gone():
    module = _load_evaluator()
    policy = copy.deepcopy(_policy())
    report = json.loads((FIXTURES / "pip-audit-findings.json").read_text(encoding="utf-8"))
    project = _project(policy, "python-tools")
    finding = module.normalize_pip_audit(report, project, _project_lock(project))[0]
    policy["exceptions"] = [_exception_for(finding)]

    evaluated, exceptions = module.evaluate_findings(policy, [finding], EXPIRY)
    assert evaluated[0]["status"] == "blocked"
    assert exceptions[0]["status"] == "expired"

    _, exceptions = module.evaluate_findings(policy, [], TODAY)
    assert exceptions[0]["status"] == "unused"


def _rust_sbom_expected_components() -> list[dict[str, object]]:
    return [
        {
            "name": f"fixture-{kind}",
            "version": version,
            "hashes": [f"sha256:{digit * 64}"],
        }
        for kind, version, digit in [
            ("build", "2.0.0", "2"),
            ("development", "3.0.0", "3"),
            ("optional", "4.0.0", "4"),
            ("target-only", "5.0.0", "5"),
        ]
    ]


def test_rust_sbom_requires_build_development_optional_and_target_only_union():
    module = _load_evaluator()
    complete = json.loads(
        (FIXTURES / "rust-sbom-complete-all-scope.cdx.json").read_text(encoding="utf-8")
    )
    missing = json.loads(
        (FIXTURES / "rust-sbom-missing-target.cdx.json").read_text(encoding="utf-8")
    )
    expected = _rust_sbom_expected_components()

    verified = module.verify_sbom(
        complete,
        expected,
        project_id="rust-workspace",
        ecosystem="cargo",
    )
    assert verified == {
        "project_id": "rust-workspace",
        "spec_version": "1.5",
        "component_count": 4,
        "verified": True,
    }
    with pytest.raises(module.PolicyError, match=r"fixture-target-only|missing"):
        module.verify_sbom(
            missing,
            expected,
            project_id="rust-workspace",
            ecosystem="cargo",
        )


@pytest.mark.parametrize("mutation", ["extra", "version", "checksum", "missing-hash", "duplicate"])
def test_sbom_rejects_extra_version_checksum_missing_hash_and_duplicate_drift(mutation):
    module = _load_evaluator()
    sbom = json.loads(
        (FIXTURES / "rust-sbom-complete-all-scope.cdx.json").read_text(encoding="utf-8")
    )
    expected = _rust_sbom_expected_components()
    if mutation == "extra":
        extra = copy.deepcopy(sbom["components"][0])
        extra.update(
            {
                "bom-ref": "pkg:cargo/fixture-extra@9.0.0",
                "name": "fixture-extra",
                "version": "9.0.0",
                "purl": "pkg:cargo/fixture-extra@9.0.0",
            }
        )
        sbom["components"].append(extra)
    elif mutation == "version":
        sbom["components"][0]["version"] = "999.0.0"
    elif mutation == "checksum":
        sbom["components"][0]["hashes"][0]["content"] = "f" * 64
    elif mutation == "missing-hash":
        sbom["components"][0].pop("hashes")
    else:
        sbom["components"].append(copy.deepcopy(sbom["components"][0]))
    with pytest.raises(module.PolicyError):
        module.verify_sbom(
            sbom,
            expected,
            project_id="rust-workspace",
            ecosystem="cargo",
        )


@pytest.mark.parametrize("expects_checksum", [True, False])
@pytest.mark.parametrize(
    "mutation",
    [
        "hashes-not-list",
        "hash-not-object",
        "algorithm-not-string",
        "content-not-string",
        "unknown-algorithm",
        "wrong-length",
        "nonhex",
    ],
)
def test_sbom_rejects_every_malformed_hash_even_when_lock_has_no_checksum(
    expects_checksum, mutation
):
    module = _load_evaluator()
    expected_hashes = [f"sha256:{'a' * 64}"] if expects_checksum else []
    component = {
        "type": "library",
        "bom-ref": "fixture:hash-shape",
        "name": "fixture-hash-shape",
        "version": "1.0.0",
        "hashes": ([{"alg": "SHA-256", "content": "a" * 64}] if expects_checksum else []),
    }
    if mutation == "hashes-not-list":
        component["hashes"] = {}
    elif mutation == "hash-not-object":
        component["hashes"] = [[]]
    else:
        invalid_hash: dict[str, object] = {"alg": "SHA-256", "content": "a" * 64}
        if mutation == "algorithm-not-string":
            invalid_hash["alg"] = 256
        elif mutation == "content-not-string":
            invalid_hash["content"] = 1
        elif mutation == "unknown-algorithm":
            invalid_hash["alg"] = "MD5"
        elif mutation == "wrong-length":
            invalid_hash["content"] = "a" * 63
        else:
            invalid_hash["content"] = "z" * 64
        component["hashes"] = [invalid_hash]
    sbom = {
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "version": 1,
        "components": [component],
    }

    with pytest.raises(module.PolicyError, match=r"hash|checksum|algorithm|digest"):
        module.verify_sbom(
            sbom,
            [
                {
                    "name": "fixture-hash-shape",
                    "version": "1.0.0",
                    "hashes": expected_hashes,
                }
            ],
            project_id="fixture",
            ecosystem="cargo",
        )


def test_uv_sbom_normalization_removes_volatility_and_enriches_locked_checksum():
    module = _load_evaluator()
    first = json.loads((FIXTURES / "uv-sbom-volatile-a.cdx.json").read_text(encoding="utf-8"))
    second = json.loads((FIXTURES / "uv-sbom-volatile-b.cdx.json").read_text(encoding="utf-8"))
    expected = [
        {
            "name": "click",
            "version": "8.3.1",
            "hashes": [f"sha256:{'a' * 64}"],
        }
    ]
    normalized_first = module.normalize_sbom(first, expected, ecosystem="pypi")
    normalized_second = module.normalize_sbom(second, expected, ecosystem="pypi")
    assert normalized_first == normalized_second
    assert "serialNumber" not in normalized_first
    assert "timestamp" not in normalized_first["metadata"]
    assert normalized_first["components"][0]["hashes"] == [{"alg": "SHA-256", "content": "a" * 64}]
    module.verify_sbom(
        normalized_first,
        expected,
        project_id="python-tools",
        ecosystem="pypi",
    )


def test_uv_sbom_enriches_absent_checksum_but_rejects_wrong_checksum_before_enrichment():
    module = _load_evaluator()
    expected = [
        {
            "name": "click",
            "version": "8.3.1",
            "hashes": [f"sha256:{'a' * 64}"],
        }
    ]
    missing = json.loads((FIXTURES / "uv-sbom-volatile-a.cdx.json").read_text(encoding="utf-8"))
    wrong = json.loads((FIXTURES / "uv-sbom-wrong-checksum.cdx.json").read_text(encoding="utf-8"))

    normalized = module.normalize_sbom(missing, expected, ecosystem="pypi")
    module.verify_sbom(
        normalized,
        expected,
        project_id="python-tools",
        ecosystem="pypi",
    )

    with pytest.raises(module.PolicyError, match=r"checksum|hash"):
        module.normalize_sbom(wrong, expected, ecosystem="pypi")
    with pytest.raises(module.PolicyError, match=r"checksum|hash"):
        module.verify_sbom(
            wrong,
            expected,
            project_id="python-tools",
            ecosystem="pypi",
        )


def test_current_node_lock_explicitly_versions_all_seven_platform_optionals():
    module = _load_evaluator()
    lock_path = REPO_ROOT / "packages/ferric/package-lock.json"

    components = module._npm_components(lock_path.read_bytes())
    platform_components = [
        component for component in components if component["name"] in NODE_PLATFORM_OPTIONALS
    ]
    identities = {
        (component["name"], component["version"], component["path"])
        for component in platform_components
    }

    assert identities == {
        (name, "0.1.0", f"node_modules/{name}") for name in NODE_PLATFORM_OPTIONALS
    }
    assert all(component["inferred_version"] is False for component in platform_components)


def test_realistic_scoped_npm_cdx_identity_and_distribution_hash_verify():
    module = _load_evaluator()
    digest = "a5" * 64
    expected = [
        {
            "name": "@fixture/scoped-package",
            "version": "1.2.3",
            "path": "node_modules/@fixture/scoped-package",
            "hashes": [f"sha512:{digest}"],
        }
    ]
    sbom = {
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "version": 1,
        "components": [
            {
                "type": "library",
                "group": "@fixture",
                "name": "scoped-package",
                "version": "1.2.3",
                "bom-ref": "@fixture/root@1.0.0|@fixture/scoped-package@1.2.3",
                "externalReferences": [
                    {
                        "type": "distribution",
                        "url": "https://registry.npmjs.org/@fixture/scoped-package/-/scoped-package-1.2.3.tgz",
                        "hashes": [{"alg": "SHA-512", "content": digest}],
                    }
                ],
            }
        ],
    }

    normalized = module.normalize_sbom(sbom, expected, ecosystem="npm")
    verified = module.verify_sbom(
        normalized,
        expected,
        project_id="fixture",
        ecosystem="npm",
    )

    assert verified == {
        "project_id": "fixture",
        "spec_version": "1.5",
        "component_count": 1,
        "verified": True,
    }


@pytest.mark.parametrize(
    ("group", "name", "expected_name"),
    [
        (7, "package", "package"),
        ("fixture", "package", "package"),
        ("@fixture", "@fixture/package", "@fixture/package"),
        ("@fixture", "nested/package", "nested/package"),
    ],
)
def test_npm_cdx_rejects_malformed_or_double_scoped_group_identity(group, name, expected_name):
    module = _load_evaluator()
    expected = [
        {
            "name": expected_name,
            "version": "1.0.0",
            "path": f"node_modules/{expected_name}",
            "hashes": [],
        }
    ]
    sbom = {
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "version": 1,
        "components": [
            {
                "type": "library",
                "group": group,
                "name": name,
                "version": "1.0.0",
                "bom-ref": "fixture:invalid-group",
            }
        ],
    }

    with pytest.raises(module.PolicyError, match=r"group|scope|name"):
        module.verify_sbom(sbom, expected, project_id="fixture", ecosystem="npm")


def test_npm_cdx_rejects_conflicting_top_level_and_distribution_hashes():
    module = _load_evaluator()
    expected_digest = "a5" * 64
    conflicting_digest = "b6" * 64
    expected = [
        {
            "name": "fixture-package",
            "version": "1.0.0",
            "path": "node_modules/fixture-package",
            "hashes": [f"sha512:{expected_digest}"],
        }
    ]
    sbom = {
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "version": 1,
        "components": [
            {
                "type": "library",
                "name": "fixture-package",
                "version": "1.0.0",
                "bom-ref": "fixture:conflicting-hashes",
                "hashes": [{"alg": "SHA-512", "content": expected_digest}],
                "externalReferences": [
                    {
                        "type": "distribution",
                        "url": "https://registry.npmjs.org/fixture-package/-/fixture-package-1.0.0.tgz",
                        "hashes": [{"alg": "SHA-512", "content": conflicting_digest}],
                    }
                ],
            }
        ],
    }

    with pytest.raises(module.PolicyError, match=r"hash|conflict"):
        module.verify_sbom(sbom, expected, project_id="fixture", ecosystem="npm")


def test_npm_integrity_preserves_every_supported_sri_digest_for_exact_sbom_parity():
    module = _load_evaluator()
    digests = {
        "sha1": bytes(range(20)),
        "sha256": bytes(range(32)),
        "sha384": bytes(range(48)),
        "sha512": bytes(range(64)),
    }
    integrity = " ".join(
        f"{algorithm}-{base64.b64encode(value).decode()}" for algorithm, value in digests.items()
    )

    normalized = module._integrity_hashes(integrity)

    assert normalized == sorted(
        f"{algorithm}:{value.hex()}" for algorithm, value in digests.items()
    )
    expected = [
        {
            "name": "fixture-integrity",
            "version": "1.0.0",
            "path": "node_modules/fixture-integrity",
            "hashes": normalized,
        }
    ]
    component = {
        "type": "library",
        "bom-ref": "fixture:integrity",
        "name": "fixture-integrity",
        "version": "1.0.0",
        "hashes": [
            {
                "alg": {
                    "sha1": "SHA-1",
                    "sha256": "SHA-256",
                    "sha384": "SHA-384",
                    "sha512": "SHA-512",
                }[algorithm],
                "content": value.hex(),
            }
            for algorithm, value in digests.items()
        ],
    }
    sbom = {
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "version": 1,
        "components": [component],
    }
    verified = module.verify_sbom(
        sbom,
        expected,
        project_id="fixture",
        ecosystem="npm",
    )
    assert verified["verified"] is True

    missing = copy.deepcopy(sbom)
    missing["components"][0]["hashes"].pop()
    with pytest.raises(module.PolicyError, match=r"hash|checksum"):
        module.verify_sbom(
            missing,
            expected,
            project_id="fixture",
            ecosystem="npm",
        )


@pytest.mark.parametrize(
    "integrity",
    [
        "garbage-without-a-supported-sri-shape",
        "sha256-not-base64!!",
        f"sha256-{base64.b64encode(bytes(31)).decode()}",
        f"md5-{base64.b64encode(bytes(16)).decode()}",
        f"sha999-{base64.b64encode(bytes(32)).decode()}",
    ],
)
def test_nonempty_invalid_or_unsupported_npm_integrity_never_becomes_no_checksum(integrity):
    module = _load_evaluator()
    with pytest.raises(module.PolicyError, match=r"integrity|algorithm|digest|base64"):
        module._integrity_hashes(integrity)


@pytest.mark.parametrize("invalid", [7, {"value": "sha512-fixture"}, ["sha512-fixture"], True])
def test_present_nonstring_npm_integrity_fails_while_absent_integrity_is_allowed(invalid):
    module = _load_evaluator()
    lock = {
        "name": "fixture-root",
        "version": "1.0.0",
        "lockfileVersion": 3,
        "packages": {
            "": {"name": "fixture-root", "version": "1.0.0"},
            "node_modules/fixture": {"name": "fixture", "version": "1.0.0"},
        },
    }
    absent = module._npm_components(json.dumps(lock).encode())
    fixture = next(component for component in absent if component["name"] == "fixture")
    assert fixture["hashes"] == []

    lock["packages"]["node_modules/fixture"]["integrity"] = invalid
    with pytest.raises(module.PolicyError, match=r"integrity|string"):
        module._npm_components(json.dumps(lock).encode())


@pytest.mark.parametrize("mutation", ["absent", "nonexact-range", "conflicting-version"])
def test_versionless_npm_optional_requires_one_exact_consistent_root_declaration(mutation):
    module = _load_evaluator()
    lock = json.loads((REPO_ROOT / "packages/ferric/package-lock.json").read_text(encoding="utf-8"))
    name = sorted(NODE_PLATFORM_OPTIONALS)[0]
    path = f"node_modules/{name}"
    lock["packages"][path].pop("version")
    if mutation == "absent":
        lock["packages"][""]["optionalDependencies"].pop(name)
    elif mutation == "nonexact-range":
        lock["packages"][""]["optionalDependencies"][name] = "^0.1.0"
    else:
        lock["packages"][path]["version"] = "0.2.0"

    with pytest.raises(module.PolicyError, match=r"optional|version|exact|conflict"):
        module._npm_components(json.dumps(lock).encode())


def test_node_sbom_parity_cannot_skip_inferred_versionless_platform_optionals():
    module = _load_evaluator()
    lock_path = REPO_ROOT / "packages/ferric/package-lock.json"
    lock = json.loads(lock_path.read_text(encoding="utf-8"))
    for name in NODE_PLATFORM_OPTIONALS:
        lock["packages"][f"node_modules/{name}"].pop("version")
    expected = module._npm_components(json.dumps(lock).encode())
    incomplete = []
    for index, component in enumerate(expected):
        if component["name"] in NODE_PLATFORM_OPTIONALS:
            continue
        hashes = []
        for value in component["hashes"]:
            algorithm, content = value.split(":", 1)
            hashes.append(
                {
                    "alg": {"sha256": "SHA-256", "sha512": "SHA-512"}[algorithm],
                    "content": content,
                }
            )
        incomplete.append(
            {
                "type": "library",
                "bom-ref": f"fixture:{index}",
                "name": component["name"],
                "version": component["version"],
                "hashes": hashes,
            }
        )
    sbom = {
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "version": 1,
        "components": incomplete,
    }

    with pytest.raises(module.PolicyError, match=r"napi-|missing"):
        module.verify_sbom(
            sbom,
            expected,
            project_id="node-package",
            ecosystem="npm",
        )

    normalized = module.normalize_sbom(sbom, expected, ecosystem="npm")
    inferred = {
        (module._cdx_component_name(component, "npm"), component["version"])
        for component in normalized["components"]
        if module._cdx_component_name(component, "npm") in NODE_PLATFORM_OPTIONALS
    }
    assert inferred == {(name, "0.1.0") for name in NODE_PLATFORM_OPTIONALS}
    module.verify_sbom(
        normalized,
        expected,
        project_id="node-package",
        ecosystem="npm",
    )


def test_npm_sbom_accepts_lock_occurrence_multiplicity_but_rejects_duplicate_bom_ref():
    module = _load_evaluator()
    component = {
        "type": "library",
        "name": "fixture-duplicate",
        "version": "1.0.0",
        "hashes": [{"alg": "SHA-256", "content": "d" * 64}],
    }
    sbom = {
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "version": 1,
        "components": [
            {**component, "bom-ref": "npm:node_modules/fixture-duplicate"},
            {**component, "bom-ref": "npm:node_modules/parent/node_modules/fixture-duplicate"},
        ],
    }
    expected = [
        {
            "name": "fixture-duplicate",
            "version": "1.0.0",
            "path": "node_modules/fixture-duplicate",
            "hashes": [f"sha256:{'d' * 64}"],
        },
        {
            "name": "fixture-duplicate",
            "version": "1.0.0",
            "path": "node_modules/parent/node_modules/fixture-duplicate",
            "hashes": [f"sha256:{'d' * 64}"],
        },
    ]
    verified = module.verify_sbom(
        sbom,
        expected,
        project_id="site",
        ecosystem="npm",
    )
    assert verified["component_count"] == 2
    assert verified["verified"] is True

    duplicate_ref = copy.deepcopy(sbom)
    duplicate_ref["components"][1]["bom-ref"] = duplicate_ref["components"][0]["bom-ref"]
    excess = copy.deepcopy(sbom)
    excess["components"].append(
        {**component, "bom-ref": "npm:node_modules/another/fixture-duplicate"}
    )
    for invalid in [duplicate_ref, excess]:
        with pytest.raises(module.PolicyError):
            module.verify_sbom(
                invalid,
                expected,
                project_id="site",
                ecosystem="npm",
            )


def test_npm_sbom_does_not_ignore_unexpected_nested_components_under_flatten_contract():
    module = _load_evaluator()
    nested = json.loads((FIXTURES / "npm-sbom-hierarchical.cdx.json").read_text(encoding="utf-8"))
    expected = [
        {
            "name": name,
            "version": "1.0.0",
            "path": path,
            "hashes": [],
        }
        for name, path in [
            ("parent-a", "node_modules/parent-a"),
            ("parent-b", "node_modules/parent-b"),
        ]
    ]
    with pytest.raises(module.PolicyError, match=r"fixture-duplicate|nested|absent"):
        module.verify_sbom(
            nested,
            expected,
            project_id="site",
            ecosystem="npm",
        )


def test_workflow_is_unconditional_exact_candidate_and_stably_named():
    assert WORKFLOW.is_file(), f"missing dependency policy workflow: {WORKFLOW}"
    workflow = WORKFLOW.read_text(encoding="utf-8")
    assert re.search(r"(?m)^on:\n(?:.*\n)*?  push:\s*$", workflow)
    assert re.search(r"(?m)^  pull_request:\s*$", workflow)
    assert re.search(r"(?m)^  workflow_dispatch:\s*$", workflow)
    assert "paths:" not in workflow
    assert "paths-ignore:" not in workflow
    assert "continue-on-error:" not in workflow
    assert re.search(r"(?m)^permissions:\n  contents: read\s*$", workflow)

    job = _job(workflow, "dependency-policy")
    assert "    name: Dependency Policy\n" in job
    assert "    timeout-minutes: 45\n" in job
    assert "actions/checkout@v4" in job
    assert "github.event.pull_request.head.sha" in job
    assert "github.sha" in job
    assert re.search(r"(?m)^\s+ref:.*CANDIDATE_SHA", job)
    assert "date -u +%F" in job
    assert "dependency-policy.py run" in job
    assert "./scripts/license-notices.sh" in EVALUATOR.read_text(encoding="utf-8")
    assert "taiki-e/install-action@just" not in job
    assert 'cd "${RUNNER_TEMP}"' in job
    assert "CARGO_INSTALL_ROOT" in job
    assert "NPM_CONFIG_USERCONFIG" in job
    assert "--prefix" in job
    assert "--registry=https://registry.npmjs.org/" in job
    assert "--today" in job
    assert "--candidate-sha" in job
    assert "if: always()" in job
    assert "name: dependency-policy-evidence-${{ env.CANDIDATE_SHA }}" in job
    assert "retention-days: 30" in job
    assert "if-no-files-found: error" in job


def test_workflow_derives_utc_date_when_the_final_aggregate_executes():
    workflow = WORKFLOW.read_text(encoding="utf-8")
    job = _job(workflow, "dependency-policy")
    match = re.search(
        r"(?ms)^      - name: Run the stable dependency-policy aggregate\n"
        r"(?P<step>.*?)(?=^      - name:|\Z)",
        job,
    )
    assert match is not None
    aggregate = match.group("step")

    assert '--today "$(date -u +%F)"' in aggregate
    assert "POLICY_TODAY" not in workflow
    assert job.index("Install pinned Rust dependency tools") < job.index(
        "Run the stable dependency-policy aggregate"
    )


def test_workflow_and_runner_pin_every_scanner_and_use_locked_full_scope_commands():
    assert WORKFLOW.is_file()
    assert EVALUATOR.is_file()
    module = _load_evaluator()
    contract = WORKFLOW.read_text(encoding="utf-8") + EVALUATOR.read_text(encoding="utf-8")
    for tool, version in TOOL_PINS.items():
        assert version in contract, f"{tool} must be pinned to {version}"

    assert module.CARGO_AUDIT_ARGV == [
        "cargo-audit",
        "audit",
        "--file",
        "Cargo.lock",
        "--format",
        "json",
    ]
    assert module.CARGO_DENY_ARGV == [
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
    assert module.CARGO_SBOM_ARGV[:2] == ["cargo-cyclonedx", "cyclonedx"]
    assert module.NPM_AUDIT_ARGV == [
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
    assert module.PIP_AUDIT_ARGV == [
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
    assert module.UV_EXPORT_ARGV[:4] == ["uv", "--no-config", "--no-cache", "export"]
    assert module.UV_SBOM_ARGV[:4] == [
        "uv",
        "--no-config",
        "--no-cache",
        "--preview-features",
    ]
    assert module.LICENSE_NOTICE_ARGV == ["./scripts/license-notices.sh", "check"]

    for token in [
        "--json",
        "--format",
        "json",
        "--all-features",
        "--package-lock-only",
        "--audit-level=info",
        "--registry=https://registry.npmjs.org/",
        "--include=dev",
        "--include=optional",
        "--include=peer",
        "pip-audit",
        "--strict",
        "--disable-pip",
        "--vulnerability-service",
        "pypi",
        "--target",
        "all",
        "--describe",
        "all-cargo-targets",
        "cyclonedx-npm",
        "--flatten-components",
    ]:
        assert token in contract
    for forbidden in ["--ignore", "--ignore-vuln", "--omit=dev", "--no-dev"]:
        assert forbidden not in contract
    assert re.search(r"--target(?:[\s\"',]+)all", contract)
    assert re.search(r"--describe(?:[\s\"',]+)all-cargo-targets", contract)


def test_documented_raw_evidence_layout_and_report_schema_are_machine_stable():
    assert DOCUMENTATION.is_file(), f"missing dependency security policy: {DOCUMENTATION}"
    assert EVALUATOR.is_file()
    contract = DOCUMENTATION.read_text(encoding="utf-8") + EVALUATOR.read_text(encoding="utf-8")
    for filename in RAW_REPORTS:
        assert filename in contract
    for token in [
        "dependency-policy-evidence/raw",
        "dependency-policy-evidence/sboms",
        "rust-workspace.sbom-manifest.json",
        "ferric.dependency-policy-report",
        '"version": 1',
        "candidate_sha",
        "evaluated_on",
        "tool_versions",
        "findings",
        "exceptions",
        "license_notice",
        "errors",
    ]:
        assert token in contract


def test_generated_dependency_policy_evidence_is_ignored_only_at_repository_root(tmp_path):
    lines = GITIGNORE.read_text(encoding="utf-8").splitlines()
    assert lines.count("/dependency-policy-evidence/") == 1

    repository = tmp_path / "repository"
    repository.mkdir()
    (repository / ".gitignore").write_bytes(GITIGNORE.read_bytes())
    subprocess.run(
        ["git", "init", "--quiet"],
        cwd=repository,
        check=True,
    )
    candidates = [
        "dependency-policy-evidence/raw/report.json",
        "dependency-policy-evidence.json",
        "nested/dependency-policy-evidence/raw/report.json",
    ]
    result = subprocess.run(
        ["git", "check-ignore", "--no-index", "--stdin"],
        cwd=repository,
        input="\n".join(candidates) + "\n",
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode == 0, result.stderr
    assert result.stdout.splitlines() == [candidates[0]]


def test_date_fixture_is_a_real_utc_calendar_date():
    assert date.fromisoformat(TODAY).isoformat() == TODAY
    assert date.fromisoformat(EXPIRY).isoformat() == EXPIRY
