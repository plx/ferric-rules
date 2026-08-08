#!/usr/bin/env python3
"""Shared validation primitives for Ferric's Python release artifacts.

This module intentionally uses only the Python standard library so release
jobs can inspect artifacts before installing anything from them.
"""

from __future__ import annotations

import base64
import csv
import gzip
import hashlib
import io
import json
import os
import platform
import re
import shutil
import stat
import subprocess
import sys
import sysconfig
import tarfile
import tempfile
import zipfile
from dataclasses import dataclass
from email import policy
from email.parser import BytesParser
from pathlib import Path, PurePosixPath
from typing import Any, Iterable, Mapping, Optional, Sequence


REPO_ROOT = Path(__file__).resolve().parents[1]
PACKAGE_ROOT = REPO_ROOT / "crates" / "ferric-rules-python"
CONTRACT_PATH = PACKAGE_ROOT / "wheel-targets.json"
PYPROJECT_PATH = PACKAGE_ROOT / "pyproject.toml"
PYTHON_CARGO_PATH = PACKAGE_ROOT / "Cargo.toml"
ROOT_CARGO_PATH = REPO_ROOT / "Cargo.toml"

EXPECTED_SUPPORTED_MINORS = ("3.9", "3.10", "3.11", "3.12", "3.13")
EXPECTED_REQUIRES_PYTHON = ">=3.9,<3.14"
EXPECTED_PYTHON_TAG = "cp39"
EXPECTED_ABI_TAG = "abi3"
EXPECTED_SUMMARY = "A CLIPS-inspired rules engine for Python, implemented in Rust"
EXPECTED_LICENSE_EXPRESSION = "MIT OR Apache-2.0"
EXPECTED_LICENSE_FILES = ("LICENSE-MIT", "LICENSE-APACHE")
EXPECTED_PROJECT_URLS = {
    "Documentation, https://github.com/plx/ferric-rules/blob/main/docs/users-guide.md",
    "Homepage, https://github.com/plx/ferric-rules",
    "Issues, https://github.com/plx/ferric-rules/issues",
    "Repository, https://github.com/plx/ferric-rules",
}
EXPECTED_SMOKE_CHECKS = (
    "rust-toolchain-absent",
    "wheel-metadata-record-and-native-header",
    "offline-no-deps-only-binary-install",
    "import-path-within-clean-venv",
    "load-reset-run",
    "unicode-roundtrip",
    "typed-parse-error",
    "serialize-restore",
    "close-lifecycle",
)

WHEEL_SMOKE_PROGRAM = r"""\
import importlib
import json
from pathlib import Path
import shutil
import sys

venv_root = Path(sys.argv[1]).resolve()
assert shutil.which("cargo") is None, "cargo is available in wheel consumer"
assert shutil.which("rustc") is None, "rustc is available in wheel consumer"

def relative_to_venv(value):
    path = Path(value).resolve()
    try:
        relative = path.relative_to(venv_root)
    except ValueError as exc:
        raise AssertionError(f"import escaped clean venv: {path}") from exc
    assert "site-packages" in relative.parts, relative
    return relative.as_posix()

import ferric
native = importlib.import_module("ferric.ferric")
import_relative = relative_to_venv(ferric.__file__)
native_relative = relative_to_venv(native.__file__)

unicode_value = "Grüße 東京 🦀"
engine = ferric.Engine()
engine.load(
    '(deffacts package-smoke-startup (package-input ready)) '
    '(defrule package-smoke (package-input ready) '
    f'=> (assert (package-result "{unicode_value}")))'
)
engine.reset()
result = engine.run()
assert result.rules_fired == 1, result.rules_fired
result_facts = engine.find_facts("package-result")
assert len(result_facts) == 1, result_facts
assert str(result_facts[0].fields[0]) == unicode_value

snapshot = engine.serialize()
assert isinstance(snapshot, bytes) and snapshot
restored = ferric.Engine.from_snapshot(snapshot)
restored_facts = restored.find_facts("package-result")
assert len(restored_facts) == 1
assert str(restored_facts[0].fields[0]) == unicode_value

broken = ferric.Engine()
try:
    broken.load("(defrule incomplete")
except ferric.FerricParseError:
    pass
else:
    raise AssertionError("invalid source did not raise FerricParseError")

broken.close()
restored.close()
engine.close()
engine.close()
try:
    engine.fact_count
except ferric.FerricRuntimeError:
    pass
else:
    raise AssertionError("closed engine remained usable")

print(json.dumps({
    "import_path_relative_to_venv": import_relative,
    "native_path_relative_to_venv": native_relative,
}, sort_keys=True))
"""

EXPECTED_TARGETS = {
    "manylinux2014-x86_64": {
        "runner": "ubuntu-24.04",
        "rust_target": "x86_64-unknown-linux-gnu",
        "platform_tags": ["manylinux_2_17_x86_64", "manylinux2014_x86_64"],
        "runtime": {
            "os": "linux",
            "architecture": "x86_64",
            "libc": "glibc",
            "libc_minimum": "2.17",
        },
        "compatibility": {"family": "manylinux", "maturin": "manylinux2014"},
    },
    "manylinux2014-aarch64": {
        "runner": "ubuntu-24.04-arm",
        "rust_target": "aarch64-unknown-linux-gnu",
        "platform_tags": ["manylinux_2_17_aarch64", "manylinux2014_aarch64"],
        "runtime": {
            "os": "linux",
            "architecture": "aarch64",
            "libc": "glibc",
            "libc_minimum": "2.17",
        },
        "compatibility": {"family": "manylinux", "maturin": "manylinux2014"},
    },
    "musllinux1_2-x86_64": {
        "runner": "ubuntu-24.04",
        "rust_target": "x86_64-unknown-linux-musl",
        "platform_tags": ["musllinux_1_2_x86_64"],
        "runtime": {
            "os": "linux",
            "architecture": "x86_64",
            "libc": "musl",
            "libc_minimum": "1.2",
        },
        "compatibility": {"family": "musllinux", "maturin": "musllinux_1_2"},
    },
    "musllinux1_2-aarch64": {
        "runner": "ubuntu-24.04-arm",
        "rust_target": "aarch64-unknown-linux-musl",
        "platform_tags": ["musllinux_1_2_aarch64"],
        "runtime": {
            "os": "linux",
            "architecture": "aarch64",
            "libc": "musl",
            "libc_minimum": "1.2",
        },
        "compatibility": {"family": "musllinux", "maturin": "musllinux_1_2"},
    },
    "macos-x86_64": {
        "runner": "macos-15-intel",
        "rust_target": "x86_64-apple-darwin",
        "platform_tags": ["macosx_10_12_x86_64"],
        "runtime": {
            "os": "macos",
            "architecture": "x86_64",
            "minimum_os_version": "10.12",
        },
        "compatibility": {
            "family": "macos",
            "maturin": "pypi",
            "deployment_target": "10.12",
        },
    },
    "macos-arm64": {
        "runner": "macos-15",
        "rust_target": "aarch64-apple-darwin",
        "platform_tags": ["macosx_11_0_arm64"],
        "runtime": {
            "os": "macos",
            "architecture": "aarch64",
            "minimum_os_version": "11.0",
        },
        "compatibility": {
            "family": "macos",
            "maturin": "pypi",
            "deployment_target": "11.0",
        },
    },
    "windows-x86_64": {
        "runner": "windows-2025",
        "rust_target": "x86_64-pc-windows-msvc",
        "platform_tags": ["win_amd64"],
        "runtime": {"os": "windows", "architecture": "x86_64"},
        "compatibility": {"family": "windows", "maturin": "pypi"},
    },
}


class PackageValidationError(ValueError):
    """Raised when release policy or an artifact is invalid."""


@dataclass(frozen=True)
class WheelInspection:
    """Verified identity and digest of one wheel."""

    path: Path
    filename: str
    sha256: str
    size: int
    target_id: str
    version: str
    platform_tags: tuple[str, ...]


@dataclass(frozen=True)
class SdistInspection:
    """Verified identity and digest of one source distribution."""

    path: Path
    filename: str
    sha256: str
    size: int
    version: str


def _error(message: str) -> PackageValidationError:
    return PackageValidationError(message)


def _require(condition: bool, message: str) -> None:
    if not condition:
        raise _error(message)


def _require_exact_keys(
    value: Mapping[str, Any], expected: set[str], context: str
) -> None:
    actual = set(value)
    missing = sorted(expected - actual)
    unexpected = sorted(actual - expected)
    _require(
        not missing and not unexpected,
        f"{context} keys differ: missing={missing}, unexpected={unexpected}",
    )


def _require_dict(value: Any, context: str) -> dict[str, Any]:
    _require(isinstance(value, dict), f"{context} must be an object")
    return value


def _require_string(value: Any, context: str) -> str:
    _require(
        isinstance(value, str) and bool(value), f"{context} must be a non-empty string"
    )
    return value


def _require_string_list(value: Any, context: str) -> list[str]:
    _require(isinstance(value, list), f"{context} must be an array")
    _require(
        all(isinstance(item, str) and item for item in value),
        f"{context} must contain only non-empty strings",
    )
    return value


def load_json_object(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        raise _error(f"cannot read JSON object {path}: {exc}") from exc
    return _require_dict(value, str(path))


def load_contract(
    path: Path = CONTRACT_PATH, *, validate: bool = True
) -> dict[str, Any]:
    contract = load_json_object(path)
    if validate:
        validate_contract(contract)
    return contract


def validate_contract(contract: Mapping[str, Any]) -> None:
    """Validate the exact v1 release policy, not merely its JSON shape."""

    _require_exact_keys(
        contract,
        {"schema_version", "distribution", "python", "wheels", "sdist"},
        "wheel-targets.json",
    )
    _require(
        type(contract["schema_version"]) is int and contract["schema_version"] == 1,
        "wheel-targets.json schema_version must be integer 1",
    )

    distribution = _require_dict(contract["distribution"], "distribution")
    _require_exact_keys(
        distribution, {"name", "import_name", "version_source"}, "distribution"
    )
    _require(distribution["name"] == "ferric", "distribution.name must be ferric")
    _require(
        distribution["import_name"] == "ferric",
        "distribution.import_name must be ferric",
    )
    version_source = _require_dict(
        distribution["version_source"], "distribution.version_source"
    )
    _require_exact_keys(
        version_source, {"path", "toml_key"}, "distribution.version_source"
    )
    _require(
        version_source["path"] == "../../Cargo.toml",
        "version source must be ../../Cargo.toml",
    )
    _require(
        version_source["toml_key"] == "workspace.package.version",
        "version source must be workspace.package.version",
    )

    python = _require_dict(contract["python"], "python")
    _require_exact_keys(
        python,
        {
            "implementation",
            "gil",
            "requires_python",
            "supported_minors",
            "abi",
            "exclusions",
        },
        "python",
    )
    _require(python["implementation"] == "CPython", "only CPython may be claimed")
    _require(python["gil"] == "enabled", "only GIL-enabled CPython may be claimed")
    _require(
        canonical_requires_python(python["requires_python"])
        == EXPECTED_REQUIRES_PYTHON,
        f"requires_python must be {EXPECTED_REQUIRES_PYTHON}",
    )
    _require(
        tuple(
            _require_string_list(python["supported_minors"], "python.supported_minors")
        )
        == EXPECTED_SUPPORTED_MINORS,
        f"supported_minors must be {list(EXPECTED_SUPPORTED_MINORS)}",
    )

    abi = _require_dict(python["abi"], "python.abi")
    _require_exact_keys(
        abi,
        {"kind", "pyo3_feature", "minimum_python", "python_tag", "abi_tag"},
        "python.abi",
    )
    _require(
        abi
        == {
            "kind": "abi3",
            "pyo3_feature": "abi3-py39",
            "minimum_python": "3.9",
            "python_tag": EXPECTED_PYTHON_TAG,
            "abi_tag": EXPECTED_ABI_TAG,
        },
        "python.abi must declare the cp39 abi3 baseline",
    )

    exclusions = _require_dict(python["exclusions"], "python.exclusions")
    _require_exact_keys(
        exclusions,
        {"python_versions", "implementations", "cpython_builds", "wheel_targets"},
        "python.exclusions",
    )
    _require(
        exclusions["python_versions"] == ["<3.9", ">=3.14"], "Python exclusions drifted"
    )
    _require(
        exclusions["implementations"] == ["PyPy", "GraalPy"],
        "implementation exclusions drifted",
    )
    _require(
        exclusions["cpython_builds"] == ["free-threaded"],
        "CPython build exclusions drifted",
    )
    _require(
        exclusions["wheel_targets"]
        == [
            "macos-universal2",
            "windows-arm64",
            "unlisted-platforms-and-architectures",
        ],
        "wheel target exclusions drifted",
    )

    wheels = contract["wheels"]
    _require(isinstance(wheels, list), "wheels must be an array")
    _require(len(wheels) == 7, "wheel contract must contain exactly seven targets")
    by_id: dict[str, dict[str, Any]] = {}
    for index, raw_target in enumerate(wheels):
        target = _require_dict(raw_target, f"wheels[{index}]")
        _require_exact_keys(
            target,
            {
                "id",
                "runner",
                "rust_target",
                "platform_tags",
                "runtime",
                "compatibility",
            },
            f"wheels[{index}]",
        )
        target_id = _require_string(target["id"], f"wheels[{index}].id")
        _require(target_id not in by_id, f"duplicate wheel target id {target_id}")
        by_id[target_id] = target

    _require(
        set(by_id) == set(EXPECTED_TARGETS),
        "wheel target IDs differ from the seven-target policy",
    )
    for target_id, expected in EXPECTED_TARGETS.items():
        target = dict(by_id[target_id])
        target.pop("id")
        _require(
            target == expected,
            f"wheel target {target_id} differs from the release policy",
        )

    sdist = _require_dict(contract["sdist"], "sdist")
    _require_exact_keys(
        sdist,
        {
            "included",
            "artifact_count",
            "format",
            "build_and_test",
            "include_in_publish_dry_run",
            "cargo_lock",
            "requirements",
        },
        "sdist",
    )
    _require(sdist["included"] is True, "sdist must be included")
    _require(
        type(sdist["artifact_count"]) is int and sdist["artifact_count"] == 1,
        "exactly one sdist is required",
    )
    _require(sdist["format"] == "tar.gz", "sdist must use tar.gz")
    _require(sdist["build_and_test"] is True, "sdist must be built and tested")
    _require(
        sdist["include_in_publish_dry_run"] is True, "sdist must be in publish dry runs"
    )
    cargo_lock = _require_dict(sdist["cargo_lock"], "sdist.cargo_lock")
    _require_exact_keys(
        cargo_lock,
        {"required", "relocated_workspace", "final_artifact"},
        "sdist.cargo_lock",
    )
    _require(cargo_lock["required"] is True, "the final sdist must contain Cargo.lock")
    _require(
        cargo_lock["relocated_workspace"] == "normalize-offline-then-verify-locked",
        "sdist relocated-workspace lock policy drifted",
    )
    _require(
        cargo_lock["final_artifact"] == "deterministically-repacked-and-tested",
        "sdist final-artifact policy drifted",
    )
    requirements = _require_dict(sdist["requirements"], "sdist.requirements")
    _require_exact_keys(
        requirements,
        {"python", "rust", "maturin", "native_toolchain", "dependency_resolution"},
        "sdist.requirements",
    )
    _require(
        canonical_requires_python(requirements["python"]) == EXPECTED_REQUIRES_PYTHON,
        "sdist Python requirement must match wheel metadata",
    )
    _require(requirements["rust"] == ">=1.75", "sdist Rust requirement must be >=1.75")
    _require(
        requirements["maturin"] == ">=1.0,<2.0", "sdist Maturin requirement drifted"
    )
    _require(
        requirements["native_toolchain"] is True,
        "sdist must require a native toolchain",
    )
    _require(
        requirements["dependency_resolution"]
        == "network access or pre-populated Python and Cargo caches",
        "sdist dependency-resolution requirement drifted",
    )


def contract_target(contract: Mapping[str, Any], target_id: str) -> dict[str, Any]:
    for raw_target in contract["wheels"]:
        if raw_target["id"] == target_id:
            return raw_target
    raise _error(f"unknown wheel target {target_id}")


def package_version(
    contract: Mapping[str, Any], package_root: Path = PACKAGE_ROOT
) -> str:
    source = contract["distribution"]["version_source"]
    path = (package_root / source["path"]).resolve()
    _require(
        path == ROOT_CARGO_PATH.resolve(),
        f"version source resolves outside root Cargo.toml: {path}",
    )
    text = path.read_text(encoding="utf-8")
    return toml_string(text, "workspace.package", "version")


def toml_section(text: str, section: str) -> str:
    match = re.search(
        rf"(?ms)^\[{re.escape(section)}\]\s*\n(?P<body>.*?)(?=^\[|\Z)",
        text,
    )
    _require(match is not None, f"TOML section [{section}] is missing")
    return match.group("body")


def toml_string(text: str, section: str, key: str) -> str:
    body = toml_section(text, section)
    match = re.search(rf'(?m)^{re.escape(key)}\s*=\s*"([^"]+)"\s*(?:#.*)?$', body)
    _require(match is not None, f"TOML key [{section}].{key} must be a string")
    return match.group(1)


def toml_bool(text: str, section: str, key: str) -> bool:
    body = toml_section(text, section)
    match = re.search(rf"(?m)^{re.escape(key)}\s*=\s*(true|false)\s*(?:#.*)?$", body)
    _require(match is not None, f"TOML key [{section}].{key} must be a boolean")
    return match.group(1) == "true"


def toml_string_array(text: str, section: str, key: str) -> list[str]:
    body = toml_section(text, section)
    match = re.search(rf"(?ms)^{re.escape(key)}\s*=\s*\[(?P<items>.*?)]", body)
    _require(match is not None, f"TOML key [{section}].{key} must be an array")
    items = re.findall(r'"([^"]+)"', match.group("items"))
    _require(
        items or not match.group("items").strip(),
        f"[{section}].{key} has invalid items",
    )
    return items


def canonical_requires_python(value: Any) -> str:
    _require(isinstance(value, str), "Requires-Python must be a string")
    return "".join(value.split())


def accepts_python_minor(specifier: str, minor: str) -> bool:
    version = tuple(int(part) for part in minor.split("."))
    for clause in specifier.split(","):
        match = re.fullmatch(r"\s*(>=|<)\s*(\d+)\.(\d+)\s*", clause)
        _require(match is not None, f"unsupported Requires-Python clause {clause!r}")
        boundary = (int(match.group(2)), int(match.group(3)))
        if match.group(1) == ">=" and version < boundary:
            return False
        if match.group(1) == "<" and version >= boundary:
            return False
    return True


def validate_repository_contract(contract: Optional[Mapping[str, Any]] = None) -> str:
    """Cross-check repository metadata, ABI features, and strip policy."""

    contract = contract or load_contract()
    validate_contract(contract)
    version = package_version(contract)

    root_cargo = ROOT_CARGO_PATH.read_text(encoding="utf-8")
    python_cargo = PYTHON_CARGO_PATH.read_text(encoding="utf-8")
    pyproject = PYPROJECT_PATH.read_text(encoding="utf-8")

    _require(
        toml_string(root_cargo, "workspace.package", "rust-version") == "1.75",
        "workspace MSRV must match the sdist Rust requirement",
    )
    pyo3_line = re.search(r"(?m)^pyo3\s*=\s*\{(?P<body>[^\n]+)}\s*$", root_cargo)
    _require(pyo3_line is not None, "workspace pyo3 dependency must be an inline table")
    pyo3_features = re.findall(r'"([^"]+)"', pyo3_line.group("body"))
    _require(
        "extension-module" in pyo3_features,
        "workspace PyO3 must enable extension-module",
    )
    _require("abi3-py39" in pyo3_features, "workspace PyO3 must enable abi3-py39")
    abi_features = [
        feature for feature in pyo3_features if feature.startswith("abi3-py")
    ]
    _require(
        abi_features == ["abi3-py39"], f"unexpected PyO3 ABI features: {abi_features}"
    )

    release = toml_section(root_cargo, "profile.release")
    _require(
        re.search(r'(?m)^strip\s*=\s*"symbols"\s*$', release) is not None,
        "release strip policy drifted",
    )
    python_release = toml_section(
        root_cargo, "profile.release.package.ferric-rules-python"
    )
    _require(
        re.search(r'(?m)^strip\s*=\s*"none"\s*$', python_release) is not None,
        "Python release package must disable Cargo symbol stripping",
    )

    dependencies = toml_section(python_cargo, "dependencies")
    _require(
        re.search(r"(?m)^pyo3\s*=\s*\{\s*workspace\s*=\s*true\s*}\s*$", dependencies)
        is not None,
        "Python crate must inherit the workspace PyO3 ABI policy",
    )
    package = toml_section(python_cargo, "package")
    _require(
        re.search(r"(?m)^version\.workspace\s*=\s*true\s*$", package) is not None,
        "Python crate version must come from the workspace",
    )
    _require(
        re.search(
            r'(?m)^description\s*=\s*"Python bindings for the Ferric rules engine"\s*$',
            package,
        )
        is not None,
        "Python Cargo package description drifted",
    )
    _require(
        re.search(r'(?m)^readme\s*=\s*"README\.md"\s*$', package) is not None,
        "Python Cargo package readme must be README.md",
    )

    _require(
        toml_string(pyproject, "project", "name") == "ferric",
        "pyproject name must be ferric",
    )
    _require(
        canonical_requires_python(toml_string(pyproject, "project", "requires-python"))
        == EXPECTED_REQUIRES_PYTHON,
        "pyproject Requires-Python differs from the artifact contract",
    )
    _require(
        toml_string_array(pyproject, "project", "dynamic") == ["version"],
        "pyproject version must be dynamic",
    )
    project_body = toml_section(pyproject, "project")
    _require(
        re.search(r"(?m)^version\s*=", project_body) is None,
        "pyproject must not duplicate a literal version",
    )
    _require(
        re.search(r"(?m)^dependencies\s*=", project_body) is None,
        "ferric wheel must not declare runtime dependencies",
    )
    _require(
        toml_string(pyproject, "project", "description") == EXPECTED_SUMMARY,
        "pyproject description drifted",
    )
    _require(
        re.search(
            r'(?m)^readme\s*=\s*\{\s*file\s*=\s*"README\.md",\s*content-type\s*=\s*"text/markdown"\s*}\s*$',
            project_body,
        )
        is not None,
        "pyproject readme must bind README.md as text/markdown",
    )
    _require(
        toml_string(pyproject, "project", "license") == EXPECTED_LICENSE_EXPRESSION,
        "pyproject license expression drifted",
    )
    _require(
        toml_string_array(pyproject, "project", "license-files")
        == list(EXPECTED_LICENSE_FILES),
        "pyproject license-files drifted",
    )
    _require(
        re.search(
            r'(?m)^authors\s*=\s*\[\{\s*name\s*=\s*"Ferric contributors"\s*}]\s*$',
            project_body,
        )
        is not None,
        "pyproject authors drifted",
    )
    _require(
        toml_string_array(pyproject, "project", "keywords")
        == ["clips", "expert-system", "rete", "rules-engine"],
        "pyproject keywords drifted",
    )
    classifiers = toml_string_array(pyproject, "project", "classifiers")
    classified_minors = [
        match.group(1)
        for classifier in classifiers
        if (
            match := re.fullmatch(
                r"Programming Language :: Python :: (\d+\.\d+)", classifier
            )
        )
    ]
    _require(
        tuple(classified_minors) == EXPECTED_SUPPORTED_MINORS,
        "pyproject Python classifiers differ from supported_minors",
    )
    _require(
        "Programming Language :: Python :: Implementation :: CPython" in classifiers,
        "pyproject must claim CPython only",
    )
    _require(
        "Programming Language :: Python :: 3 :: Only" in classifiers,
        "pyproject must exclude Python 2",
    )
    project_urls = {
        f"{name}, {toml_string(pyproject, 'project.urls', name)}"
        for name in ("Homepage", "Documentation", "Repository", "Issues")
    }
    _require(project_urls == EXPECTED_PROJECT_URLS, "pyproject project URLs drifted")
    _require(
        toml_bool(pyproject, "tool.maturin", "strip") is False,
        "Maturin stripping must be disabled",
    )
    _require(
        toml_bool(pyproject, "tool.maturin", "locked") is True,
        "Maturin Cargo builds must be locked",
    )
    maturin_features = toml_string_array(pyproject, "tool.maturin", "features")
    _require(
        maturin_features == ["pyo3/extension-module", "pyo3/abi3-py39"],
        "Maturin features must explicitly select extension-module and abi3-py39",
    )
    maturin_body = toml_section(pyproject, "tool.maturin")
    _require(
        re.search(
            r'(?m)^include\s*=\s*\[\{\s*path\s*=\s*"README\.md",\s*format\s*=\s*"sdist"\s*}]\s*$',
            maturin_body,
        )
        is not None,
        "Maturin must copy the package README to the relocated sdist root",
    )
    build_requirements = toml_string_array(pyproject, "build-system", "requires")
    _require(
        build_requirements == ["maturin>=1.0,<2.0"],
        "build-system Maturin range must match the sdist contract",
    )
    _require(
        toml_string(pyproject, "build-system", "build-backend") == "maturin",
        "build backend must be maturin",
    )

    required_files = [
        PACKAGE_ROOT / "README.md",
        PACKAGE_ROOT / "LICENSE-MIT",
        PACKAGE_ROOT / "LICENSE-APACHE",
    ]
    missing_files = [
        str(path.relative_to(REPO_ROOT))
        for path in required_files
        if not path.is_file()
    ]
    _require(
        not missing_files, f"Python package metadata files are missing: {missing_files}"
    )
    for filename in EXPECTED_LICENSE_FILES:
        _require(
            (PACKAGE_ROOT / filename).read_bytes()
            == (REPO_ROOT / filename).read_bytes(),
            f"Python package {filename} differs from the repository license",
        )
    return version


def normalized_distribution_component(name: str) -> str:
    return re.sub(r"[-_.]+", "_", name).strip("_")


def expected_wheel_filename(
    contract: Mapping[str, Any], target: Mapping[str, Any], version: str
) -> str:
    return wheel_filename_for_platform_tags(contract, target["platform_tags"], version)


def wheel_filename_for_platform_tags(
    contract: Mapping[str, Any],
    platform_tags: Sequence[str],
    version: str,
) -> str:
    distribution = normalized_distribution_component(contract["distribution"]["name"])
    abi = contract["python"]["abi"]
    platforms = ".".join(platform_tags)
    return (
        f"{distribution}-{version}-{abi['python_tag']}-{abi['abi_tag']}-{platforms}.whl"
    )


def source_built_wheel_platform_tags(target: Mapping[str, Any]) -> list[str]:
    if target["runtime"]["os"] == "linux":
        architecture = target["runtime"]["architecture"]
        return [f"linux_{architecture}"]
    return list(target["platform_tags"])


def expected_sdist_filename(contract: Mapping[str, Any], version: str) -> str:
    distribution = re.sub(r"[-_.]+", "-", contract["distribution"]["name"])
    return f"{distribution}-{version}.tar.gz"


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _safe_archive_name(name: str, context: str) -> PurePosixPath:
    _require(name and "\x00" not in name, f"{context} contains an empty or NUL path")
    _require("\\" not in name, f"{context} path uses a backslash: {name!r}")
    path = PurePosixPath(name)
    _require(not path.is_absolute(), f"{context} path is absolute: {name!r}")
    _require(".." not in path.parts, f"{context} path traverses upward: {name!r}")
    _require("." not in path.parts, f"{context} path is not normalized: {name!r}")
    _require(
        str(path) == name.rstrip("/"), f"{context} path is not normalized: {name!r}"
    )
    _require(
        not (path.parts and ":" in path.parts[0]),
        f"{context} path has a drive prefix: {name!r}",
    )
    return path


def _parse_email_metadata(data: bytes, context: str) -> Any:
    try:
        message = BytesParser(policy=policy.default).parsebytes(data)
    except Exception as exc:
        raise _error(f"cannot parse {context}: {exc}") from exc
    _require(
        not message.defects,
        f"{context} contains email-parser defects: {message.defects}",
    )
    return message


def _one_header(message: Any, name: str, context: str) -> str:
    values = message.get_all(name, [])
    _require(len(values) == 1, f"{context} must contain exactly one {name} header")
    return str(values[0])


def _metadata_contract(
    data: bytes, contract: Mapping[str, Any], version: str, context: str
) -> Any:
    metadata = _parse_email_metadata(data, context)
    name = _one_header(metadata, "Name", context)
    _require(
        normalized_distribution_component(name)
        == normalized_distribution_component(contract["distribution"]["name"]),
        f"{context} Name is {name!r}",
    )
    _require(
        _one_header(metadata, "Version", context) == version,
        f"{context} Version differs",
    )
    requires_python = canonical_requires_python(
        _one_header(metadata, "Requires-Python", context)
    )
    _require(
        requires_python == EXPECTED_REQUIRES_PYTHON,
        f"{context} Requires-Python is {requires_python!r}",
    )
    dependencies = metadata.get_all("Requires-Dist", [])
    _require(
        not dependencies,
        f"{context} unexpectedly declares runtime dependencies: {dependencies}",
    )
    _require(
        _one_header(metadata, "Summary", context) == EXPECTED_SUMMARY,
        f"{context} Summary drifted",
    )
    _require(
        _one_header(metadata, "License-Expression", context)
        == EXPECTED_LICENSE_EXPRESSION,
        f"{context} license expression drifted",
    )
    license_files = metadata.get_all("License-File", [])
    _require(
        license_files == list(EXPECTED_LICENSE_FILES),
        f"{context} License-File headers drifted",
    )
    _require(
        _one_header(metadata, "Description-Content-Type", context) == "text/markdown",
        f"{context} description content type drifted",
    )
    project_urls = set(metadata.get_all("Project-URL", []))
    _require(project_urls == EXPECTED_PROJECT_URLS, f"{context} project URLs drifted")
    classifiers = metadata.get_all("Classifier", [])
    expected_python_classifiers = {
        "Programming Language :: Python :: 3 :: Only",
        "Programming Language :: Python :: Implementation :: CPython",
        *(
            f"Programming Language :: Python :: {minor}"
            for minor in EXPECTED_SUPPORTED_MINORS
        ),
    }
    actual_python_classifiers = {
        classifier
        for classifier in classifiers
        if classifier.startswith("Programming Language :: Python ::")
    }
    _require(
        actual_python_classifiers == expected_python_classifiers,
        f"{context} Python classifiers drifted",
    )
    payload = metadata.get_payload()
    _require(
        isinstance(payload, str) and payload.strip(), f"{context} has no README body"
    )
    expected_readme = (PACKAGE_ROOT / "README.md").read_text(encoding="utf-8").strip()
    _require(
        payload.strip() == expected_readme,
        f"{context} README body differs from package README.md",
    )
    return metadata


def _wheel_target_for_filename(
    filename: str,
    contract: Mapping[str, Any],
    version: str,
) -> dict[str, Any]:
    matches = [
        target
        for target in contract["wheels"]
        if expected_wheel_filename(contract, target, version) == filename
    ]
    _require(
        len(matches) == 1,
        f"wheel filename is not one exact contracted artifact: {filename}",
    )
    return matches[0]


def _validate_native_binary(
    data: bytes, target: Mapping[str, Any], context: str
) -> None:
    family = target["compatibility"]["family"]
    architecture = target["runtime"]["architecture"]
    if family in {"manylinux", "musllinux"}:
        _require(len(data) >= 64, f"{context} has a truncated ELF header")
        _require(data[:4] == b"\x7fELF", f"{context} is not an ELF binary")
        _require(data[4] == 2, f"{context} is not ELF64")
        _require(data[5] == 1, f"{context} is not little-endian ELF")
        machine = int.from_bytes(data[18:20], "little")
        expected_machine = {"x86_64": 62, "aarch64": 183}[architecture]
        _require(
            machine == expected_machine,
            f"{context} ELF e_machine is {machine}, expected {expected_machine}",
        )
        return
    if family == "macos":
        _require(len(data) >= 32, f"{context} has a truncated Mach-O header")
        _require(
            data[:4]
            not in {
                b"\xca\xfe\xba\xbe",
                b"\xbe\xba\xfe\xca",
                b"\xca\xfe\xba\xbf",
                b"\xbf\xba\xfe\xca",
            },
            f"{context} is a fat/universal Mach-O binary",
        )
        _require(
            data[:4] == b"\xcf\xfa\xed\xfe",
            f"{context} is not a little-endian 64-bit Mach-O binary",
        )
        cpu_type = int.from_bytes(data[4:8], "little")
        expected_cpu = {"x86_64": 0x01000007, "aarch64": 0x0100000C}[architecture]
        _require(
            cpu_type == expected_cpu,
            f"{context} Mach-O CPU type is {cpu_type:#x}, expected {expected_cpu:#x}",
        )
        return
    if family == "windows":
        _require(len(data) >= 64, f"{context} has a truncated PE header")
        _require(data[:2] == b"MZ", f"{context} is not a PE binary")
        pe_offset = int.from_bytes(data[0x3C:0x40], "little")
        _require(
            pe_offset >= 0x40 and pe_offset + 26 <= len(data),
            f"{context} has an invalid PE header offset",
        )
        _require(
            data[pe_offset : pe_offset + 4] == b"PE\x00\x00",
            f"{context} lacks a PE signature",
        )
        machine = int.from_bytes(data[pe_offset + 4 : pe_offset + 6], "little")
        _require(
            machine == 0x8664, f"{context} PE machine is {machine:#x}, expected AMD64"
        )
        optional_magic = int.from_bytes(data[pe_offset + 24 : pe_offset + 26], "little")
        _require(optional_magic == 0x20B, f"{context} is not PE32+")
        return
    raise _error(f"{context} has unsupported platform family {family!r}")


def _validate_sbom(data: bytes, version: str, context: str) -> None:
    try:
        sbom = json.loads(data.decode("utf-8", errors="strict"))
    except (UnicodeError, json.JSONDecodeError) as exc:
        raise _error(f"{context} is not valid UTF-8 CycloneDX JSON: {exc}") from exc
    sbom = _require_dict(sbom, context)
    _require(sbom.get("bomFormat") == "CycloneDX", f"{context} is not CycloneDX")
    _require(isinstance(sbom.get("specVersion"), str), f"{context} lacks a specVersion")
    metadata = _require_dict(sbom.get("metadata"), f"{context}.metadata")
    component = _require_dict(
        metadata.get("component"), f"{context}.metadata.component"
    )
    _require(
        component.get("name") == "ferric-rules-python",
        f"{context} component name drifted",
    )
    _require(
        component.get("version") == version, f"{context} component version drifted"
    )
    components = sbom.get("components")
    _require(isinstance(components, list), f"{context}.components must be an array")
    unexpected_native = []
    for raw_component in components:
        child = _require_dict(raw_component, f"{context}.components[]")
        name = str(child.get("name", ""))
        component_type = child.get("type")
        if component_type == "file" or name.endswith((".so", ".pyd", ".dylib", ".dll")):
            unexpected_native.append(name)
    _require(
        not unexpected_native,
        f"{context} declares unexpected bundled native components: {unexpected_native}",
    )


def validate_wheel(
    path: Path,
    contract: Mapping[str, Any],
    version: str,
    *,
    expected_target_id: Optional[str] = None,
    expected_platform_tags: Optional[Sequence[str]] = None,
) -> WheelInspection:
    path = Path(path)
    _require(
        path.is_file() and not path.is_symlink(), f"wheel is not a regular file: {path}"
    )
    if expected_platform_tags is None:
        target = _wheel_target_for_filename(path.name, contract, version)
        target_platform_tags = list(target["platform_tags"])
        if expected_target_id is not None:
            _require(
                target["id"] == expected_target_id,
                f"wheel is for {target['id']}, expected {expected_target_id}",
            )
    else:
        _require(
            expected_target_id is not None,
            "custom wheel tags require an explicit target ID",
        )
        target = contract_target(contract, expected_target_id)
        target_platform_tags = list(expected_platform_tags)
        _require(
            path.name
            == wheel_filename_for_platform_tags(
                contract, target_platform_tags, version
            ),
            f"source-built wheel filename is not exact: {path.name}",
        )

    try:
        archive = zipfile.ZipFile(path)
    except (OSError, zipfile.BadZipFile) as exc:
        raise _error(f"cannot open wheel {path}: {exc}") from exc

    with archive:
        infos = archive.infolist()
        _require(infos, f"wheel is empty: {path.name}")
        names: list[str] = []
        for info in infos:
            _safe_archive_name(info.filename, f"wheel {path.name}")
            _require(
                not info.is_dir(),
                f"wheel contains an unexpected directory entry: {info.filename}",
            )
            _require(
                not (info.flag_bits & 0x1),
                f"wheel contains an encrypted member: {info.filename}",
            )
            mode = (info.external_attr >> 16) & 0xFFFF
            _require(
                not stat.S_ISLNK(mode),
                f"wheel contains a symbolic link: {info.filename}",
            )
            _require(
                info.file_size <= 256 * 1024 * 1024,
                f"wheel member is unreasonably large: {info.filename}",
            )
            names.append(info.filename)
        _require(
            len(names) == len(set(names)),
            f"wheel contains duplicate archive paths: {path.name}",
        )

        distribution = normalized_distribution_component(
            contract["distribution"]["name"]
        )
        dist_info = f"{distribution}-{version}.dist-info"
        metadata_path = f"{dist_info}/METADATA"
        wheel_path = f"{dist_info}/WHEEL"
        record_path = f"{dist_info}/RECORD"
        import_name = contract["distribution"]["import_name"]
        init_path = f"{import_name}/__init__.py"
        sbom_path = f"{dist_info}/sboms/ferric-rules-python.cyclonedx.json"

        platform_family = target["compatibility"]["family"]
        if platform_family == "windows":
            native_candidates = {
                f"{import_name}/{import_name}.pyd",
                f"{import_name}/{import_name}.abi3.pyd",
            }
        else:
            native_candidates = {f"{import_name}/{import_name}.abi3.so"}
        native_members = [
            name for name in names if name.endswith((".so", ".pyd", ".dylib", ".dll"))
        ]
        _require(
            len(native_members) == 1,
            f"wheel must contain exactly one native module, found {native_members}",
        )
        _require(
            native_members[0] in native_candidates,
            f"unexpected native module path {native_members[0]}",
        )

        license_paths = {
            f"{dist_info}/licenses/LICENSE-MIT",
            f"{dist_info}/licenses/LICENSE-APACHE",
        }
        required = {
            init_path,
            native_members[0],
            metadata_path,
            wheel_path,
            record_path,
            sbom_path,
            *license_paths,
        }
        missing = sorted(required - set(names))
        _require(not missing, f"wheel is missing required members: {missing}")
        allowed = set(required)
        unexpected = sorted(set(names) - allowed)
        _require(not unexpected, f"wheel contains unexpected members: {unexpected}")

        _metadata_contract(
            archive.read(metadata_path), contract, version, metadata_path
        )
        for filename in EXPECTED_LICENSE_FILES:
            wheel_license = archive.read(f"{dist_info}/licenses/{filename}")
            _require(
                wheel_license == (PACKAGE_ROOT / filename).read_bytes(),
                f"wheel {filename} differs from the checked-in license",
            )
        _validate_native_binary(
            archive.read(native_members[0]), target, native_members[0]
        )
        _validate_sbom(archive.read(sbom_path), version, sbom_path)
        wheel_metadata = _parse_email_metadata(archive.read(wheel_path), wheel_path)
        _require(
            _one_header(wheel_metadata, "Wheel-Version", wheel_path) == "1.0",
            "unsupported Wheel-Version",
        )
        _require(
            _one_header(wheel_metadata, "Root-Is-Purelib", wheel_path).lower()
            == "false",
            "native wheel claims purelib",
        )
        tags = wheel_metadata.get_all("Tag", [])
        expected_tags = [
            f"{EXPECTED_PYTHON_TAG}-{EXPECTED_ABI_TAG}-{platform_tag}"
            for platform_tag in target_platform_tags
        ]
        _require(
            tags == expected_tags, f"WHEEL tags are {tags}, expected {expected_tags}"
        )

        record_data = archive.read(record_path).decode("utf-8", errors="strict")
        rows = list(csv.reader(io.StringIO(record_data, newline="")))
        _require(
            all(len(row) == 3 for row in rows),
            "RECORD rows must have exactly three columns",
        )
        record_names = [row[0] for row in rows]
        for record_name in record_names:
            _safe_archive_name(record_name, "wheel RECORD")
        _require(
            len(record_names) == len(set(record_names)),
            "RECORD contains duplicate paths",
        )
        _require(
            set(record_names) == set(names),
            "RECORD paths do not exactly match wheel members",
        )
        by_name = {row[0]: row[1:] for row in rows}
        _require(
            by_name[record_path] == ["", ""],
            "RECORD must leave its own hash and size empty",
        )
        for info in infos:
            if info.filename == record_path:
                continue
            digest_text, size_text = by_name[info.filename]
            _require(
                digest_text.startswith("sha256="),
                f"RECORD hash is not sha256 for {info.filename}",
            )
            encoded_digest = digest_text.removeprefix("sha256=")
            expected_digest = (
                base64.urlsafe_b64encode(
                    hashlib.sha256(archive.read(info.filename)).digest()
                )
                .rstrip(b"=")
                .decode("ascii")
            )
            _require(
                encoded_digest == expected_digest,
                f"RECORD hash mismatch for {info.filename}",
            )
            _require(
                size_text == str(info.file_size),
                f"RECORD size mismatch for {info.filename}",
            )

    return WheelInspection(
        path=path,
        filename=path.name,
        sha256=sha256_file(path),
        size=path.stat().st_size,
        target_id=target["id"],
        version=version,
        platform_tags=tuple(target_platform_tags),
    )


def _allowed_sdist_relative_path(relative: PurePosixPath) -> bool:
    top_level_files = {
        "PKG-INFO",
        "Cargo.lock",
        "Cargo.toml",
        "pyproject.toml",
        "README.md",
        "LICENSE-MIT",
        "LICENSE-APACHE",
    }
    if len(relative.parts) == 1:
        return relative.name in top_level_files
    if len(relative.parts) < 3 or relative.parts[0] != "crates":
        return False
    crate = relative.parts[1]
    tail = PurePosixPath(*relative.parts[2:])
    if crate not in {
        "ferric-rules-core",
        "ferric-rules-parser",
        "ferric-rules-runtime",
        "ferric-rules-python",
    }:
        return False
    if any(part.startswith(".") for part in tail.parts):
        return False
    common_files = {"Cargo.toml", "README.md"}
    if str(tail) in common_files:
        return True
    if tail.parts[0] == "src":
        return tail.suffix == ".rs"
    if crate == "ferric-rules-python":
        if str(tail) in {
            "LICENSE-MIT",
            "LICENSE-APACHE",
            "build.rs",
            "uv.lock",
            "wheel-targets.json",
        }:
            return True
        return tail.parts[0] == "tests" and tail.suffix == ".py"
    if crate == "ferric-rules-parser":
        return False
    if tail.parts[0] == "benches":
        return tail.suffix == ".rs"
    if crate == "ferric-rules-core" and tail.parts[0] == "proptest-regressions":
        return tail.suffix == ".txt"
    if crate == "ferric-rules-runtime" and tail.parts[:2] == ("tests", "fixtures"):
        return tail.suffix == ".clp"
    return False


def validate_sdist(
    path: Path, contract: Mapping[str, Any], version: str
) -> SdistInspection:
    path = Path(path)
    _require(
        path.is_file() and not path.is_symlink(), f"sdist is not a regular file: {path}"
    )
    _require(
        path.name == expected_sdist_filename(contract, version),
        f"sdist filename is {path.name}, expected {expected_sdist_filename(contract, version)}",
    )
    try:
        archive = tarfile.open(path, mode="r:gz")
    except (OSError, tarfile.TarError) as exc:
        raise _error(f"cannot open sdist {path}: {exc}") from exc

    root = path.name[: -len(".tar.gz")]
    with archive:
        members = archive.getmembers()
        _require(members, "sdist is empty")
        names: list[str] = []
        regular_files: set[str] = set()
        for member in members:
            archive_path = _safe_archive_name(member.name, f"sdist {path.name}")
            _require(
                member.name == root or member.name.startswith(f"{root}/"),
                f"sdist member escapes the single {root}/ root: {member.name}",
            )
            _require(
                member.isfile(),
                f"sdist contains a directory, link, or special file: {member.name}",
            )
            _require(
                member.size <= 256 * 1024 * 1024,
                f"sdist member is unreasonably large: {member.name}",
            )
            relative = PurePosixPath(*archive_path.parts[1:])
            _require(
                _allowed_sdist_relative_path(relative),
                f"sdist contains an unexpected publish payload: {relative}",
            )
            names.append(member.name.rstrip("/"))
            if member.isfile():
                regular_files.add(member.name)
        _require(
            len(names) == len(set(names)), "sdist contains duplicate archive paths"
        )

        required_relative = {
            "PKG-INFO",
            "Cargo.lock",
            "Cargo.toml",
            "pyproject.toml",
            "README.md",
            "LICENSE-MIT",
            "LICENSE-APACHE",
            "crates/ferric-rules-python/Cargo.toml",
            "crates/ferric-rules-python/README.md",
            "crates/ferric-rules-python/LICENSE-MIT",
            "crates/ferric-rules-python/LICENSE-APACHE",
            "crates/ferric-rules-python/build.rs",
            "crates/ferric-rules-python/src/lib.rs",
            "crates/ferric-rules-core/Cargo.toml",
            "crates/ferric-rules-core/src/lib.rs",
            "crates/ferric-rules-parser/Cargo.toml",
            "crates/ferric-rules-parser/src/lib.rs",
            "crates/ferric-rules-runtime/Cargo.toml",
            "crates/ferric-rules-runtime/src/lib.rs",
        }
        required = {f"{root}/{relative}" for relative in required_relative}
        missing = sorted(required - regular_files)
        _require(not missing, f"sdist is incomplete; missing members: {missing}")
        pkg_info_member = archive.getmember(f"{root}/PKG-INFO")
        pkg_info = archive.extractfile(pkg_info_member)
        _require(pkg_info is not None, "cannot read sdist PKG-INFO")
        _metadata_contract(pkg_info.read(), contract, version, "sdist PKG-INFO")
        embedded_pyproject = archive.extractfile(
            archive.getmember(f"{root}/pyproject.toml")
        )
        _require(embedded_pyproject is not None, "cannot read sdist pyproject.toml")
        try:
            embedded_pyproject_text = embedded_pyproject.read().decode(
                "utf-8", errors="strict"
            )
        except UnicodeError as exc:
            raise _error(f"sdist pyproject.toml is not UTF-8: {exc}") from exc
        _require(
            toml_bool(embedded_pyproject_text, "tool.maturin", "locked") is True,
            "sdist pyproject.toml must retain locked = true",
        )
        for filename in ("README.md", "LICENSE-MIT", "LICENSE-APACHE"):
            root_file = archive.extractfile(archive.getmember(f"{root}/{filename}"))
            nested_file = archive.extractfile(
                archive.getmember(f"{root}/crates/ferric-rules-python/{filename}")
            )
            _require(
                root_file is not None and nested_file is not None,
                f"cannot read sdist {filename}",
            )
            _require(
                root_file.read() == nested_file.read(),
                f"sdist root and package copies of {filename} differ",
            )

    return SdistInspection(
        path=path,
        filename=path.name,
        sha256=sha256_file(path),
        size=path.stat().st_size,
        version=version,
    )


def safe_extract_sdist(
    path: Path, destination: Path, contract: Mapping[str, Any], version: str
) -> Path:
    """Validate and extract an sdist without trusting tarfile.extractall."""

    validate_sdist(path, contract, version)
    destination.mkdir(parents=True, exist_ok=True)
    root = path.name[: -len(".tar.gz")]
    with tarfile.open(path, mode="r:gz") as archive:
        for member in archive.getmembers():
            relative = _safe_archive_name(member.name, f"sdist {path.name}")
            output = destination.joinpath(*relative.parts)
            if member.isdir():
                output.mkdir(parents=True, exist_ok=True)
                continue
            output.parent.mkdir(parents=True, exist_ok=True)
            source = archive.extractfile(member)
            _require(source is not None, f"cannot extract sdist member {member.name}")
            with source, output.open("wb") as handle:
                while True:
                    chunk = source.read(1024 * 1024)
                    if not chunk:
                        break
                    handle.write(chunk)
            mode = member.mode & 0o777
            output.chmod(mode & ~0o022)
    return destination / root


def _source_file_hashes(source_root: Path) -> dict[str, str]:
    hashes: dict[str, str] = {}
    for path in sorted(source_root.rglob("*")):
        _require(not path.is_symlink(), f"source tree contains a symbolic link: {path}")
        if path.is_file():
            hashes[path.relative_to(source_root).as_posix()] = sha256_file(path)
        else:
            _require(path.is_dir(), f"source tree contains a special file: {path}")
    return hashes


def normalize_extracted_sdist_lock(
    source_root: Path, cargo: str, work_root: Path
) -> None:
    """Prune Maturin's stale full-workspace lock and prove the result is locked."""

    before = _source_file_hashes(source_root)
    _require("Cargo.lock" in before, "extracted sdist has no Cargo.lock")
    environment = dict(os.environ)
    environment["CARGO_TARGET_DIR"] = str((work_root / "cargo-target").resolve())
    environment["CARGO_NET_OFFLINE"] = "true"
    base_command = [
        cargo,
        "metadata",
        "--format-version",
        "1",
        "--offline",
        "--manifest-path",
        str((source_root / "Cargo.toml").resolve()),
    ]
    run_checked(
        base_command,
        cwd=source_root,
        environment=environment,
        context="sdist Cargo.lock normalization",
    )
    after = _source_file_hashes(source_root)
    changed = {
        name for name in set(before) | set(after) if before.get(name) != after.get(name)
    }
    _require(
        changed <= {"Cargo.lock"},
        f"Cargo metadata changed files other than Cargo.lock: {sorted(changed)}",
    )
    run_checked(
        [*base_command, "--locked"],
        cwd=source_root,
        environment=environment,
        context="normalized sdist locked Cargo metadata",
    )


def verify_extracted_sdist_lock(source_root: Path, cargo: str, work_root: Path) -> None:
    environment = dict(os.environ)
    environment["CARGO_TARGET_DIR"] = str((work_root / "cargo-verify-target").resolve())
    environment["CARGO_NET_OFFLINE"] = "true"
    run_checked(
        [
            cargo,
            "metadata",
            "--format-version",
            "1",
            "--offline",
            "--locked",
            "--manifest-path",
            str((source_root / "Cargo.toml").resolve()),
        ],
        cwd=source_root,
        environment=environment,
        context="final sdist locked Cargo metadata",
    )


def deterministic_sdist_repack(source_root: Path, output: Path) -> SdistInspection:
    """Pack a normalized source tree with stable ordering and archive metadata."""

    output = Path(output)
    output.parent.mkdir(parents=True, exist_ok=True)
    files: list[tuple[str, Path, int]] = []
    for path in sorted(
        source_root.rglob("*"),
        key=lambda item: item.relative_to(source_root).as_posix(),
    ):
        _require(not path.is_symlink(), f"cannot repack symbolic link {path}")
        if path.is_dir():
            continue
        _require(path.is_file(), f"cannot repack special file {path}")
        relative = path.relative_to(source_root).as_posix()
        _safe_archive_name(relative, "sdist repack")
        mode = 0o755 if path.stat().st_mode & 0o111 else 0o644
        files.append((relative, path, mode))
    _require(files, "cannot repack an empty sdist source tree")

    temporary = output.with_name(f".{output.name}.tmp")
    try:
        with temporary.open("wb") as raw_output:
            with gzip.GzipFile(
                filename="", mode="wb", fileobj=raw_output, compresslevel=9, mtime=0
            ) as compressed:
                with tarfile.open(
                    fileobj=compressed, mode="w", format=tarfile.PAX_FORMAT
                ) as archive:
                    for relative, path, mode in files:
                        data = path.read_bytes()
                        info = tarfile.TarInfo(name=f"{source_root.name}/{relative}")
                        info.size = len(data)
                        info.mode = mode
                        info.uid = 0
                        info.gid = 0
                        info.uname = ""
                        info.gname = ""
                        info.mtime = 0
                        archive.addfile(info, io.BytesIO(data))
        temporary.replace(output)
    finally:
        if temporary.exists():
            temporary.unlink()

    return SdistInspection(
        path=output,
        filename=output.name,
        sha256=sha256_file(output),
        size=output.stat().st_size,
        version=source_root.name.rsplit("-", 1)[-1],
    )


def normalize_sdist(
    raw_sdist: Path,
    output: Path,
    contract: Mapping[str, Any],
    version: str,
    *,
    cargo: str = "cargo",
) -> SdistInspection:
    """Create the deterministic, lock-consistent sdist that may be published."""

    raw_sdist = Path(raw_sdist).resolve()
    output = Path(output).resolve()
    _require(raw_sdist != output, "raw and normalized sdist paths must differ")
    validate_sdist(raw_sdist, contract, version)
    _require(
        output.name == expected_sdist_filename(contract, version),
        f"normalized sdist output must be named {expected_sdist_filename(contract, version)}",
    )
    with tempfile.TemporaryDirectory(
        prefix="ferric-python-sdist-normalize-"
    ) as temporary:
        work_root = Path(temporary)
        source_root = safe_extract_sdist(
            raw_sdist, work_root / "source", contract, version
        )
        normalize_extracted_sdist_lock(source_root, cargo, work_root)
        first = deterministic_sdist_repack(source_root, output)
        reproducibility_probe = work_root / "reproducibility" / output.name
        second = deterministic_sdist_repack(source_root, reproducibility_probe)
        _require(
            first.sha256 == second.sha256 and first.size == second.size,
            "normalized sdist repack is not deterministic",
        )
    final = validate_sdist(output, contract, version)
    with tempfile.TemporaryDirectory(prefix="ferric-python-sdist-locked-") as temporary:
        work_root = Path(temporary)
        source_root = safe_extract_sdist(
            output, work_root / "source", contract, version
        )
        verify_extracted_sdist_lock(source_root, cargo, work_root)
    return final


def normalized_architecture(machine: str) -> str:
    normalized = machine.lower().replace(" ", "_")
    aliases = {
        "amd64": "x86_64",
        "x64": "x86_64",
        "arm64": "aarch64",
    }
    return aliases.get(normalized, normalized)


def runtime_os() -> str:
    if sys.platform.startswith("linux"):
        return "linux"
    if sys.platform == "darwin":
        return "macos"
    if sys.platform in {"win32", "cygwin"}:
        return "windows"
    return sys.platform


def _version_tuple(value: str) -> tuple[int, ...]:
    numbers = re.findall(r"\d+", value)
    _require(bool(numbers), f"cannot parse runtime version {value!r}")
    return tuple(int(number) for number in numbers)


def detect_libc() -> tuple[Optional[str], Optional[str]]:
    if runtime_os() != "linux":
        return None, None
    family, version = platform.libc_ver()
    lowered = family.lower()
    if "musl" in lowered:
        return "musl", version or None
    if lowered in {"glibc", "gnu libc", "libc"} and version:
        return "glibc", version
    try:
        gnu = os.confstr("CS_GNU_LIBC_VERSION")
    except (AttributeError, OSError, ValueError):
        gnu = None
    if gnu:
        match = re.search(r"glibc\s+([0-9.]+)", gnu, re.IGNORECASE)
        if match:
            return "glibc", match.group(1)
    try:
        result = subprocess.run(
            ["ldd", "--version"],
            check=False,
            capture_output=True,
            text=True,
            timeout=10,
        )
    except (OSError, subprocess.SubprocessError):
        result = None
    if result is not None:
        output = f"{result.stdout}\n{result.stderr}"
        musl = re.search(r"musl[^\n]*\n?Version\s+([0-9.]+)", output, re.IGNORECASE)
        if musl:
            return "musl", musl.group(1)
        glibc = re.search(
            r"(?:glibc|GNU C Library)[^\n]*?([0-9]+\.[0-9]+)", output, re.IGNORECASE
        )
        if glibc:
            return "glibc", glibc.group(1)
    return None, None


def runtime_facts() -> dict[str, Any]:
    libc, libc_version = detect_libc()
    return {
        "os": runtime_os(),
        "architecture": normalized_architecture(platform.machine()),
        "libc": libc,
        "libc_version": libc_version,
    }


def verify_runtime_for_target(target: Mapping[str, Any]) -> dict[str, Any]:
    facts = runtime_facts()
    expected = target["runtime"]
    _require(
        facts["os"] == expected["os"],
        f"runtime OS is {facts['os']}, expected {expected['os']}",
    )
    _require(
        facts["architecture"] == expected["architecture"],
        f"runtime architecture is {facts['architecture']}, expected {expected['architecture']}",
    )
    expected_libc = expected.get("libc")
    _require(
        facts["libc"] == expected_libc,
        f"runtime libc is {facts['libc']}, expected {expected_libc}",
    )
    if expected_libc is not None:
        _require(
            facts["libc_version"] is not None,
            "runtime libc version could not be detected",
        )
        _require(
            _version_tuple(facts["libc_version"])
            >= _version_tuple(expected["libc_minimum"]),
            f"runtime libc {facts['libc_version']} is older than {expected['libc_minimum']}",
        )
    if expected["os"] == "macos":
        macos_version = platform.mac_ver()[0]
        _require(macos_version, "macOS version could not be detected")
        _require(
            _version_tuple(macos_version)
            >= _version_tuple(expected["minimum_os_version"]),
            f"macOS {macos_version} is older than {expected['minimum_os_version']}",
        )
        facts["os_version"] = macos_version
    else:
        facts["os_version"] = None
    return facts


def python_facts() -> dict[str, Any]:
    gil_disabled = sysconfig.get_config_var("Py_GIL_DISABLED")
    return {
        "implementation": platform.python_implementation(),
        "version": platform.python_version(),
        "minor": f"{sys.version_info.major}.{sys.version_info.minor}",
        "gil_enabled": not bool(gil_disabled),
    }


def verify_python_runtime(
    contract: Mapping[str, Any], *, rejection: bool = False
) -> dict[str, Any]:
    facts = python_facts()
    _require(facts["implementation"] == "CPython", "wheel smoke requires CPython")
    _require(
        facts["gil_enabled"] is True,
        "free-threaded CPython is outside the wheel contract",
    )
    expected_minors = ("3.14",) if rejection else EXPECTED_SUPPORTED_MINORS
    _require(
        facts["minor"] in expected_minors,
        f"Python {facts['minor']} is invalid for {'rejection' if rejection else 'smoke'} coverage",
    )
    if rejection:
        _require(
            not accepts_python_minor(
                contract["python"]["requires_python"], facts["minor"]
            ),
            "rejection interpreter unexpectedly satisfies Requires-Python",
        )
    else:
        _require(
            accepts_python_minor(contract["python"]["requires_python"], facts["minor"]),
            "smoke interpreter does not satisfy Requires-Python",
        )
    return facts


def clean_subprocess_environment() -> dict[str, str]:
    environment = dict(os.environ)
    for name in (
        "PYTHONPATH",
        "PYTHONHOME",
        "VIRTUAL_ENV",
        "PIP_INDEX_URL",
        "PIP_EXTRA_INDEX_URL",
    ):
        environment.pop(name, None)
    environment.update(
        {
            "PIP_CONFIG_FILE": os.devnull,
            "PIP_DISABLE_PIP_VERSION_CHECK": "1",
            "PIP_NO_INDEX": "1",
            "PIP_REQUIRE_VIRTUALENV": "1",
            "PYTHONNOUSERSITE": "1",
        }
    )
    return environment


def path_without_commands(path_value: Optional[str], commands: Sequence[str]) -> str:
    """Remove PATH entries that expose any named command."""

    kept: list[str] = []
    for entry in (path_value or os.defpath).split(os.pathsep):
        if not entry:
            continue
        if any(shutil.which(command, path=entry) is not None for command in commands):
            continue
        kept.append(entry)
    result = os.pathsep.join(kept)
    for command in commands:
        _require(
            shutil.which(command, path=result) is None,
            f"could not remove {command} from PATH",
        )
    return result


def receipt_wheel_identity(inspection: WheelInspection) -> dict[str, Any]:
    return {"filename": inspection.filename, "sha256": inspection.sha256}


def write_json(path: Path, value: Mapping[str, Any]) -> None:
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.tmp")
    temporary.write_text(
        json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    temporary.replace(path)


def _new_clean_venv(root: Path) -> Path:
    venv_root = root / "venv"
    environment = dict(os.environ)
    for name in ("PYTHONHOME", "PYTHONPATH", "VIRTUAL_ENV"):
        environment.pop(name, None)
    environment.update(
        {
            "PIP_CONFIG_FILE": os.devnull,
            "PIP_DISABLE_PIP_VERSION_CHECK": "1",
            "PIP_NO_INDEX": "1",
            "PYTHONNOUSERSITE": "1",
        }
    )
    attempts: list[subprocess.CompletedProcess[str]] = []
    venv_mode = ["--copies"] if os.name == "nt" else ["--symlinks"]
    for _attempt in range(2):
        if venv_root.exists():
            shutil.rmtree(venv_root)
        result = subprocess.run(
            [sys.executable, "-I", "-m", "venv", "--clear", *venv_mode, str(venv_root)],
            cwd=root,
            env=environment,
            check=False,
            capture_output=True,
            text=True,
        )
        attempts.append(result)
        if result.returncode == 0:
            break
    else:
        diagnostics = "\n\n".join(
            f"attempt {index} exit={result.returncode}\nstdout:\n{result.stdout}\nstderr:\n{result.stderr}"
            for index, result in enumerate(attempts, start=1)
        )
        raise _error(
            f"cannot create clean wheel-smoke venv with bundled ensurepip:\n{diagnostics}"
        )
    python = find_python_executable(venv_root)
    _require(python.is_file(), f"clean venv did not create {python}")
    return venv_root


def execute_wheel_smoke(
    wheel_path: Path,
    target_id: str,
    contract: Mapping[str, Any],
    version: str,
    *,
    expect_python_rejection: bool = False,
    rust_free_path: Optional[str] = None,
    expected_platform_tags: Optional[Sequence[str]] = None,
    receipt_kind: str = "wheel-smoke",
) -> dict[str, Any]:
    """Install one exact wheel in a disposable venv and return its receipt."""

    inspection = validate_wheel(
        Path(wheel_path),
        contract,
        version,
        expected_target_id=target_id,
        expected_platform_tags=expected_platform_tags,
    )
    _require(
        receipt_kind in {"wheel-smoke", "source-built-wheel"},
        f"invalid wheel smoke receipt kind {receipt_kind}",
    )
    _require(
        not expect_python_rejection or receipt_kind == "wheel-smoke",
        "Python rejection receipts must refer to contracted release wheels",
    )
    target = contract_target(contract, target_id)
    python = verify_python_runtime(contract, rejection=expect_python_rejection)
    runtime = verify_runtime_for_target(target)

    with tempfile.TemporaryDirectory(prefix="ferric-python-wheel-") as temporary:
        work_root = Path(temporary).resolve()
        try:
            work_root.relative_to(REPO_ROOT.resolve())
        except ValueError:
            pass
        else:
            raise _error(
                f"wheel smoke temporary directory is inside the repository: {work_root}"
            )
        consumer = work_root / "consumer"
        consumer.mkdir()
        venv_root = _new_clean_venv(work_root)
        venv_python = find_python_executable(venv_root)
        environment = clean_subprocess_environment()
        if rust_free_path is not None:
            environment["PATH"] = rust_free_path
        effective_path = environment.get("PATH", os.defpath)
        _require(
            shutil.which("cargo", path=effective_path) is None,
            "cargo must be absent from the clean wheel consumer PATH",
        )
        _require(
            shutil.which("rustc", path=effective_path) is None,
            "rustc must be absent from the clean wheel consumer PATH",
        )
        install_command = [
            str(venv_python),
            "-I",
            "-m",
            "pip",
            "install",
            "--no-index",
            "--no-deps",
            "--only-binary=:all:",
            "--no-cache-dir",
            str(inspection.path.resolve()),
        ]
        try:
            install = subprocess.run(
                install_command,
                cwd=consumer,
                env=environment,
                check=False,
                capture_output=True,
                text=True,
            )
        except OSError as exc:
            raise _error(f"cannot run clean wheel install: {exc}") from exc

        if expect_python_rejection:
            _require(
                install.returncode != 0, "Python 3.14 unexpectedly installed the wheel"
            )
            installer_output = f"{install.stdout}\n{install.stderr}".lower()
            _require(
                "requires a different python" in installer_output
                and "3.14" in installer_output,
                "wheel install failed for a reason other than Requires-Python",
            )
            probe = subprocess.run(
                [
                    str(venv_python),
                    "-I",
                    "-c",
                    "import importlib.util,sys; sys.exit(importlib.util.find_spec('ferric') is not None)",
                ],
                cwd=consumer,
                env=environment,
                check=False,
                capture_output=True,
                text=True,
            )
            _require(
                probe.returncode == 0,
                "ferric remained importable after rejected installation",
            )
            return {
                "schema_version": 1,
                "kind": "python-rejection",
                "status": "passed",
                "distribution": contract["distribution"]["name"],
                "version": version,
                "target_id": target_id,
                "python": python,
                "runtime": runtime,
                "wheel": receipt_wheel_identity(inspection),
                "rejection": {
                    "reason": "Requires-Python",
                    "requires_python": contract["python"]["requires_python"],
                    "installer_exit_code": install.returncode,
                    "module_importable": False,
                },
            }

        if install.returncode != 0:
            raise _error(
                "offline exact-wheel install failed:\n"
                f"stdout:\n{install.stdout}\nstderr:\n{install.stderr}"
            )
        smoke = run_checked(
            [str(venv_python), "-I", "-c", WHEEL_SMOKE_PROGRAM, str(venv_root)],
            cwd=consumer,
            environment=environment,
            context="installed-wheel lifecycle smoke",
        )
        try:
            smoke_paths = json.loads(smoke.stdout)
        except json.JSONDecodeError as exc:
            raise _error(f"wheel smoke emitted invalid JSON: {smoke.stdout!r}") from exc
        payload = {
            "checks": list(EXPECTED_SMOKE_CHECKS),
            "import_path_relative_to_venv": smoke_paths.get(
                "import_path_relative_to_venv"
            ),
            "native_path_relative_to_venv": smoke_paths.get(
                "native_path_relative_to_venv"
            ),
        }
        validate_smoke_payload(payload, "generated wheel smoke")
        return {
            "schema_version": 1,
            "kind": receipt_kind,
            "status": "passed",
            "distribution": contract["distribution"]["name"],
            "version": version,
            "target_id": target_id,
            "python": python,
            "runtime": runtime,
            "wheel": receipt_wheel_identity(inspection),
            "smoke": payload,
        }


def collect_files(root: Path) -> list[Path]:
    _require(root.is_dir(), f"directory does not exist: {root}")
    files: list[Path] = []
    for path in root.rglob("*"):
        if path.is_symlink():
            raise _error(f"artifact input contains a symbolic link: {path}")
        if path.is_file():
            files.append(path)
    return sorted(files, key=lambda item: item.relative_to(root).as_posix())


def validate_smoke_payload(value: Any, context: str) -> dict[str, Any]:
    smoke = _require_dict(value, context)
    _require_exact_keys(
        smoke,
        {"checks", "import_path_relative_to_venv", "native_path_relative_to_venv"},
        context,
    )
    _require(
        smoke["checks"] == list(EXPECTED_SMOKE_CHECKS),
        f"{context} checks are incomplete",
    )
    import_path = _require_string(
        smoke["import_path_relative_to_venv"], f"{context}.import_path"
    )
    native_path = _require_string(
        smoke["native_path_relative_to_venv"], f"{context}.native_path"
    )
    for relative in (import_path, native_path):
        safe = _safe_archive_name(relative, context)
        _require(
            "site-packages" in safe.parts,
            f"{context} import did not come from site-packages",
        )
    _require(
        import_path.endswith("/ferric/__init__.py"),
        f"{context} imported the wrong Python package",
    )
    _require(
        native_path.endswith(
            ("/ferric/ferric.abi3.so", "/ferric/ferric.pyd", "/ferric/ferric.abi3.pyd")
        ),
        f"{context} imported an unexpected native module",
    )
    return smoke


def _validate_python_receipt(
    value: Any, context: str, expected_minor: Optional[str] = None
) -> dict[str, Any]:
    python = _require_dict(value, f"{context}.python")
    _require_exact_keys(
        python,
        {"implementation", "version", "minor", "gil_enabled"},
        f"{context}.python",
    )
    _require(python["implementation"] == "CPython", f"{context} did not use CPython")
    _require(python["gil_enabled"] is True, f"{context} used free-threaded CPython")
    minor = _require_string(python["minor"], f"{context}.python.minor")
    if expected_minor is not None:
        _require(
            minor == expected_minor,
            f"{context} Python minor is {minor}, expected {expected_minor}",
        )
    version = _require_string(python["version"], f"{context}.python.version")
    _require(
        version == minor or version.startswith(f"{minor}."),
        f"{context} Python version/minor disagree",
    )
    return python


def _validate_runtime_receipt(
    value: Any, target: Mapping[str, Any], context: str
) -> dict[str, Any]:
    runtime = _require_dict(value, f"{context}.runtime")
    _require_exact_keys(
        runtime,
        {"os", "architecture", "libc", "libc_version", "os_version"},
        f"{context}.runtime",
    )
    expected = target["runtime"]
    _require(
        runtime["os"] == expected["os"], f"{context} runtime OS differs from target"
    )
    _require(
        runtime["architecture"] == expected["architecture"],
        f"{context} runtime architecture differs from target",
    )
    _require(
        runtime["libc"] == expected.get("libc"),
        f"{context} runtime libc differs from target",
    )
    if expected.get("libc"):
        version = _require_string(
            runtime["libc_version"], f"{context}.runtime.libc_version"
        )
        _require(
            _version_tuple(version) >= _version_tuple(expected["libc_minimum"]),
            f"{context} runtime libc is below the target minimum",
        )
    else:
        _require(
            runtime["libc_version"] is None, f"{context} has an unexpected libc version"
        )
    if expected["os"] == "macos":
        version = _require_string(
            runtime["os_version"], f"{context}.runtime.os_version"
        )
        _require(
            _version_tuple(version) >= _version_tuple(expected["minimum_os_version"]),
            f"{context} macOS version is below the target minimum",
        )
    else:
        _require(
            runtime["os_version"] is None, f"{context} has an unexpected OS version"
        )
    return runtime


def validate_wheel_smoke_receipt(
    receipt: Mapping[str, Any],
    contract: Mapping[str, Any],
    version: str,
    wheels_by_target: Mapping[str, WheelInspection],
    context: str,
    *,
    expected_minor: Optional[str] = None,
) -> tuple[str, str]:
    _require_exact_keys(
        receipt,
        {
            "schema_version",
            "kind",
            "status",
            "distribution",
            "version",
            "target_id",
            "python",
            "runtime",
            "wheel",
            "smoke",
        },
        context,
    )
    _require(
        type(receipt["schema_version"]) is int and receipt["schema_version"] == 1,
        f"{context} schema_version must be integer 1",
    )
    _require(receipt["kind"] == "wheel-smoke", f"{context} has the wrong kind")
    _require(receipt["status"] == "passed", f"{context} did not pass")
    _require(
        receipt["distribution"] == contract["distribution"]["name"],
        f"{context} distribution differs",
    )
    _require(receipt["version"] == version, f"{context} version differs")
    target_id = _require_string(receipt["target_id"], f"{context}.target_id")
    _require(
        target_id in wheels_by_target, f"{context} names unknown target {target_id}"
    )
    target = contract_target(contract, target_id)
    python = _validate_python_receipt(receipt["python"], context, expected_minor)
    _require(
        python["minor"] in EXPECTED_SUPPORTED_MINORS,
        f"{context} is not a supported-minor smoke receipt",
    )
    _validate_runtime_receipt(receipt["runtime"], target, context)
    wheel = _require_dict(receipt["wheel"], f"{context}.wheel")
    _require_exact_keys(wheel, {"filename", "sha256"}, f"{context}.wheel")
    inspection = wheels_by_target[target_id]
    _require(
        wheel == receipt_wheel_identity(inspection),
        f"{context} does not identify the final wheel",
    )
    validate_smoke_payload(receipt["smoke"], f"{context}.smoke")
    return target_id, python["minor"]


def validate_rejection_receipt(
    receipt: Mapping[str, Any],
    contract: Mapping[str, Any],
    version: str,
    wheels_by_target: Mapping[str, WheelInspection],
    context: str,
) -> None:
    _require_exact_keys(
        receipt,
        {
            "schema_version",
            "kind",
            "status",
            "distribution",
            "version",
            "target_id",
            "python",
            "runtime",
            "wheel",
            "rejection",
        },
        context,
    )
    _require(
        type(receipt["schema_version"]) is int and receipt["schema_version"] == 1,
        f"{context} schema_version must be integer 1",
    )
    _require(receipt["kind"] == "python-rejection", f"{context} has the wrong kind")
    _require(receipt["status"] == "passed", f"{context} did not pass")
    _require(
        receipt["distribution"] == contract["distribution"]["name"],
        f"{context} distribution differs",
    )
    _require(receipt["version"] == version, f"{context} version differs")
    target_id = _require_string(receipt["target_id"], f"{context}.target_id")
    _require(
        target_id in wheels_by_target, f"{context} names unknown target {target_id}"
    )
    _validate_python_receipt(receipt["python"], context, "3.14")
    _validate_runtime_receipt(
        receipt["runtime"], contract_target(contract, target_id), context
    )
    wheel = _require_dict(receipt["wheel"], f"{context}.wheel")
    _require_exact_keys(wheel, {"filename", "sha256"}, f"{context}.wheel")
    _require(
        wheel == receipt_wheel_identity(wheels_by_target[target_id]),
        f"{context} does not identify the final wheel",
    )
    rejection = _require_dict(receipt["rejection"], f"{context}.rejection")
    _require_exact_keys(
        rejection,
        {"reason", "requires_python", "installer_exit_code", "module_importable"},
        f"{context}.rejection",
    )
    _require(
        rejection["reason"] == "Requires-Python", f"{context} rejection reason differs"
    )
    _require(
        canonical_requires_python(rejection["requires_python"])
        == EXPECTED_REQUIRES_PYTHON,
        f"{context} rejected a different Python range",
    )
    _require(
        type(rejection["installer_exit_code"]) is int
        and rejection["installer_exit_code"] > 0,
        f"{context} installer unexpectedly succeeded",
    )
    _require(
        rejection["module_importable"] is False, f"{context} left ferric importable"
    )


def validate_sdist_receipt(
    receipt: Mapping[str, Any],
    contract: Mapping[str, Any],
    version: str,
    sdist: SdistInspection,
    context: str,
) -> None:
    _require_exact_keys(
        receipt,
        {
            "schema_version",
            "kind",
            "status",
            "distribution",
            "version",
            "target_id",
            "sdist",
            "wheel_smoke",
        },
        context,
    )
    _require(
        type(receipt["schema_version"]) is int and receipt["schema_version"] == 1,
        f"{context} schema_version must be integer 1",
    )
    _require(receipt["kind"] == "sdist-smoke", f"{context} has the wrong kind")
    _require(receipt["status"] == "passed", f"{context} did not pass")
    _require(
        receipt["distribution"] == contract["distribution"]["name"],
        f"{context} distribution differs",
    )
    _require(receipt["version"] == version, f"{context} version differs")
    target_id = _require_string(receipt["target_id"], f"{context}.target_id")
    _require(
        target_id == "manylinux2014-x86_64",
        f"{context} final sdist proof must use the pinned Linux x86-64 source-build lane",
    )
    contract_target(contract, target_id)
    sdist_identity = _require_dict(receipt["sdist"], f"{context}.sdist")
    _require_exact_keys(
        sdist_identity,
        {
            "filename",
            "sha256",
            "safe_archive_paths",
            "cargo_lock_locked_offline",
            "deterministic_repack",
            "pep517_built_from_exact_archive",
        },
        f"{context}.sdist",
    )
    _require(
        sdist_identity
        == {
            "filename": sdist.filename,
            "sha256": sdist.sha256,
            "safe_archive_paths": True,
            "cargo_lock_locked_offline": True,
            "deterministic_repack": True,
            "pep517_built_from_exact_archive": True,
        },
        f"{context} does not identify the verified sdist",
    )
    nested = _require_dict(receipt["wheel_smoke"], f"{context}.wheel_smoke")
    _require_exact_keys(
        nested,
        {
            "schema_version",
            "kind",
            "status",
            "distribution",
            "version",
            "target_id",
            "python",
            "runtime",
            "wheel",
            "smoke",
        },
        f"{context}.wheel_smoke",
    )
    _require(
        type(nested.get("schema_version")) is int and nested.get("schema_version") == 1,
        f"{context} nested schema_version differs",
    )
    _require(
        nested.get("kind") == "source-built-wheel",
        f"{context} must distinguish the source-built wheel from release wheels",
    )
    _require(
        nested.get("status") == "passed", f"{context} built-wheel smoke did not pass"
    )
    _require(
        nested.get("target_id") == target_id,
        f"{context} target differs from built-wheel smoke",
    )
    _require(
        nested.get("distribution") == contract["distribution"]["name"],
        f"{context} nested distribution differs",
    )
    _require(nested.get("version") == version, f"{context} nested version differs")
    nested_python = _validate_python_receipt(
        nested.get("python"), f"{context}.wheel_smoke"
    )
    _require(
        nested_python["minor"] in EXPECTED_SUPPORTED_MINORS,
        f"{context} source-built wheel used an unsupported Python minor",
    )
    _validate_runtime_receipt(
        nested.get("runtime"),
        contract_target(contract, target_id),
        f"{context}.wheel_smoke",
    )
    built_wheel = _require_dict(nested.get("wheel"), f"{context}.wheel_smoke.wheel")
    _require_exact_keys(
        built_wheel, {"filename", "sha256"}, f"{context}.wheel_smoke.wheel"
    )
    target = contract_target(contract, target_id)
    _require(
        built_wheel["filename"]
        == wheel_filename_for_platform_tags(
            contract,
            source_built_wheel_platform_tags(target),
            version,
        ),
        f"{context} source-built wheel must be exact cp39-abi3-linux_x86_64",
    )
    _require(
        re.fullmatch(r"[0-9a-f]{64}", str(built_wheel["sha256"])) is not None,
        f"{context} built wheel hash is invalid",
    )
    validate_smoke_payload(nested.get("smoke"), f"{context}.wheel_smoke.smoke")


def verify_artifact_set(
    artifacts_dir: Path,
    receipts_dir: Path,
    contract: Mapping[str, Any],
    version: str,
) -> dict[str, Any]:
    artifact_files = collect_files(Path(artifacts_dir))
    unexpected_artifacts = [
        path.name
        for path in artifact_files
        if not (path.name.endswith(".whl") or path.name.endswith(".tar.gz"))
    ]
    _require(
        not unexpected_artifacts,
        f"unexpected release artifacts: {unexpected_artifacts}",
    )
    wheel_paths = [path for path in artifact_files if path.name.endswith(".whl")]
    sdist_paths = [path for path in artifact_files if path.name.endswith(".tar.gz")]
    _require(
        len(wheel_paths) == 7,
        f"expected exactly seven wheels, found {len(wheel_paths)}",
    )
    _require(
        len(sdist_paths) == 1, f"expected exactly one sdist, found {len(sdist_paths)}"
    )
    artifact_names = [path.name for path in artifact_files]
    _require(
        len(artifact_names) == len(set(artifact_names)),
        "duplicate artifact filenames are forbidden",
    )

    wheel_inspections = [
        validate_wheel(path, contract, version) for path in wheel_paths
    ]
    wheels_by_target = {
        inspection.target_id: inspection for inspection in wheel_inspections
    }
    _require(
        len(wheels_by_target) == 7,
        "each contracted target must contribute exactly one wheel",
    )
    _require(
        set(wheels_by_target) == set(EXPECTED_TARGETS),
        "final wheel target set is incomplete",
    )
    sdist = validate_sdist(sdist_paths[0], contract, version)

    receipt_files = collect_files(Path(receipts_dir))
    unexpected_receipts = [
        path.name for path in receipt_files if path.suffix != ".json"
    ]
    _require(
        not unexpected_receipts, f"unexpected receipt artifacts: {unexpected_receipts}"
    )
    receipts = [(path, load_json_object(path)) for path in receipt_files]
    _require(
        len(receipts) == 37,
        f"expected 37 receipts (35 smoke, one rejection, one sdist), found {len(receipts)}",
    )
    smoke_keys: set[tuple[str, str]] = set()
    rejection_count = 0
    sdist_count = 0
    for path, receipt in receipts:
        context = f"receipt {path.name}"
        kind = receipt.get("kind")
        if kind == "wheel-smoke":
            key = validate_wheel_smoke_receipt(
                receipt, contract, version, wheels_by_target, context
            )
            _require(
                key not in smoke_keys,
                f"duplicate smoke receipt for {key[0]} Python {key[1]}",
            )
            smoke_keys.add(key)
        elif kind == "python-rejection":
            rejection_count += 1
            validate_rejection_receipt(
                receipt, contract, version, wheels_by_target, context
            )
        elif kind == "sdist-smoke":
            sdist_count += 1
            validate_sdist_receipt(receipt, contract, version, sdist, context)
        else:
            raise _error(f"{context} has unknown kind {kind!r}")

    expected_smoke_keys = {
        (target_id, minor)
        for target_id in EXPECTED_TARGETS
        for minor in EXPECTED_SUPPORTED_MINORS
    }
    missing_smokes = sorted(expected_smoke_keys - smoke_keys)
    unexpected_smokes = sorted(smoke_keys - expected_smoke_keys)
    _require(
        not missing_smokes and not unexpected_smokes,
        f"smoke receipt matrix differs: missing={missing_smokes}, unexpected={unexpected_smokes}",
    )
    _require(
        rejection_count == 1,
        f"expected one Python 3.14 rejection receipt, found {rejection_count}",
    )
    _require(sdist_count == 1, f"expected one sdist smoke receipt, found {sdist_count}")

    artifacts = [
        {
            "filename": inspection.filename,
            "kind": "wheel",
            "sha256": inspection.sha256,
            "size": inspection.size,
            "target_id": inspection.target_id,
        }
        for inspection in wheel_inspections
    ]
    artifacts.append(
        {
            "filename": sdist.filename,
            "kind": "sdist",
            "sha256": sdist.sha256,
            "size": sdist.size,
        }
    )
    artifacts.sort(key=lambda item: item["filename"])
    return {
        "schema_version": 1,
        "distribution": contract["distribution"]["name"],
        "version": version,
        "artifacts": artifacts,
        "receipt_coverage": {
            "wheel_smokes": len(smoke_keys),
            "python_314_rejections": rejection_count,
            "sdist_smokes": sdist_count,
        },
    }


def command_text(command: Sequence[str]) -> str:
    return " ".join(str(part) for part in command)


def run_checked(
    command: Sequence[str],
    *,
    cwd: Path,
    environment: Optional[Mapping[str, str]] = None,
    context: str,
) -> subprocess.CompletedProcess[str]:
    try:
        result = subprocess.run(
            list(command),
            cwd=cwd,
            env=None if environment is None else dict(environment),
            check=False,
            capture_output=True,
            text=True,
        )
    except OSError as exc:
        raise _error(f"cannot run {context}: {exc}") from exc
    if result.returncode != 0:
        raise _error(
            f"{context} failed ({command_text(command)}):\nstdout:\n{result.stdout}\nstderr:\n{result.stderr}"
        )
    return result


def find_python_executable(venv_root: Path) -> Path:
    if os.name == "nt":
        return venv_root / "Scripts" / "python.exe"
    return venv_root / "bin" / "python"


def relative_to_root(path: Path, root: Path, context: str) -> str:
    resolved = path.resolve()
    root_resolved = root.resolve()
    try:
        relative = resolved.relative_to(root_resolved)
    except ValueError as exc:
        raise _error(f"{context} is outside the clean venv: {resolved}") from exc
    return relative.as_posix()


def exact_unique_paths(paths: Iterable[Path], context: str) -> list[Path]:
    values = list(paths)
    names = [path.name for path in values]
    _require(len(names) == len(set(names)), f"{context} contains duplicate filenames")
    return values
