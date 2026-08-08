"""Adversarial tests for the Python package artifact verification tooling."""

from __future__ import annotations

import base64
import copy
import csv
import hashlib
import io
import json
import os
import shutil
import subprocess
import sys
import tarfile
import zipfile
from pathlib import Path

import pytest


REPO_ROOT = Path(__file__).resolve().parents[3]
SCRIPTS_ROOT = REPO_ROOT / "scripts"
sys.path.insert(0, str(SCRIPTS_ROOT))

import musl_static_libgcc_linker as musl_linker  # noqa: E402
import python_package_lib as package_lib  # noqa: E402


@pytest.fixture(scope="module")
def contract():
    return package_lib.load_contract()


@pytest.fixture(scope="module")
def version(contract):
    return package_lib.package_version(contract)


def _metadata(
    version, *, requires_python=">=3.9,<3.14", dependency=None, metadata_version=None
):
    classifiers = [
        "Programming Language :: Python :: 3 :: Only",
        "Programming Language :: Python :: Implementation :: CPython",
        *(
            f"Programming Language :: Python :: {minor}"
            for minor in package_lib.EXPECTED_SUPPORTED_MINORS
        ),
    ]
    headers = [
        "Metadata-Version: 2.4",
        "Name: ferric",
        f"Version: {metadata_version or version}",
        f"Summary: {package_lib.EXPECTED_SUMMARY}",
        "License-Expression: MIT OR Apache-2.0",
        "License-File: LICENSE-MIT",
        "License-File: LICENSE-APACHE",
        f"Requires-Python: {requires_python}",
        "Description-Content-Type: text/markdown",
        *(f"Classifier: {classifier}" for classifier in classifiers),
        *(
            f"Project-URL: {project_url}"
            for project_url in sorted(package_lib.EXPECTED_PROJECT_URLS)
        ),
    ]
    if dependency is not None:
        headers.append(f"Requires-Dist: {dependency}")
    readme = (package_lib.PACKAGE_ROOT / "README.md").read_text(encoding="utf-8")
    return ("\n".join(headers) + "\n\n" + readme).encode()


def _native_binary(target, *, architecture=None, truncated=False, wrong_format=False):
    family = target["compatibility"]["family"]
    architecture = architecture or target["runtime"]["architecture"]
    if wrong_format:
        return b"not-a-native-library".ljust(128, b"\0")
    if family in {"manylinux", "musllinux"}:
        data = bytearray(64)
        data[:6] = b"\x7fELF\x02\x01"
        data[18:20] = {"x86_64": 62, "aarch64": 183}[architecture].to_bytes(2, "little")
    elif family == "macos":
        data = bytearray(32)
        data[:4] = b"\xcf\xfa\xed\xfe"
        data[4:8] = {"x86_64": 0x01000007, "aarch64": 0x0100000C}[
            architecture
        ].to_bytes(4, "little")
    else:
        data = bytearray(256)
        data[:2] = b"MZ"
        data[0x3C:0x40] = (0x80).to_bytes(4, "little")
        data[0x80:0x84] = b"PE\0\0"
        data[0x84:0x86] = {"x86_64": 0x8664, "aarch64": 0xAA64}[architecture].to_bytes(
            2, "little"
        )
        data[0x98:0x9A] = (0x20B).to_bytes(2, "little")
    return bytes(data[:8] if truncated else data)


def _sbom(version):
    return json.dumps(
        {
            "bomFormat": "CycloneDX",
            "specVersion": "1.5",
            "version": 1,
            "metadata": {
                "component": {
                    "type": "library",
                    "name": "ferric-rules-python",
                    "version": version,
                }
            },
            "components": [],
            "dependencies": [],
        },
        sort_keys=True,
    ).encode()


def _record_bytes(members, *, bad_hash=False, omit_path=None):
    rows = []
    for name, data in members.items():
        if name == omit_path:
            continue
        digest = (
            base64.urlsafe_b64encode(hashlib.sha256(data).digest())
            .rstrip(b"=")
            .decode()
        )
        if bad_hash and not rows:
            digest = "A" * 43
        rows.append([name, f"sha256={digest}", str(len(data))])
    output = io.StringIO(newline="")
    writer = csv.writer(output, lineterminator="\n")
    writer.writerows(rows)
    return output.getvalue().encode()


def _write_wheel(
    directory,
    contract,
    version,
    target,
    *,
    platform_tags=None,
    wheel_tags=None,
    metadata_version=None,
    metadata_override=None,
    requires_python=">=3.9,<3.14",
    dependency=None,
    extra_members=None,
    omit_members=(),
    bad_record=False,
    omit_record_path=None,
    native_architecture=None,
    native_truncated=False,
    native_wrong_format=False,
    license_drift=False,
    sbom_override=None,
):
    platform_tags = platform_tags or target["platform_tags"]
    wheel_tags = wheel_tags or platform_tags
    filename = package_lib.wheel_filename_for_platform_tags(
        contract, platform_tags, version
    )
    path = directory / filename
    dist_info = f"ferric-{version}.dist-info"
    native_name = (
        "ferric/ferric.pyd"
        if target["runtime"]["os"] == "windows"
        else "ferric/ferric.abi3.so"
    )
    members = {
        "ferric/__init__.py": b"from .ferric import *\n",
        native_name: _native_binary(
            target,
            architecture=native_architecture,
            truncated=native_truncated,
            wrong_format=native_wrong_format,
        ),
        f"{dist_info}/METADATA": (
            metadata_override
            if metadata_override is not None
            else _metadata(
                version,
                requires_python=requires_python,
                dependency=dependency,
                metadata_version=metadata_version,
            )
        ),
        f"{dist_info}/WHEEL": (
            "Wheel-Version: 1.0\n"
            "Generator: synthetic-test\n"
            "Root-Is-Purelib: false\n"
            + "".join(f"Tag: cp39-abi3-{tag}\n" for tag in wheel_tags)
        ).encode(),
        f"{dist_info}/licenses/LICENSE-MIT": (
            package_lib.PACKAGE_ROOT / "LICENSE-MIT"
        ).read_bytes(),
        f"{dist_info}/licenses/LICENSE-APACHE": (
            b"drift"
            if license_drift
            else (package_lib.PACKAGE_ROOT / "LICENSE-APACHE").read_bytes()
        ),
        f"{dist_info}/sboms/ferric-rules-python.cyclonedx.json": (
            _sbom(version) if sbom_override is None else sbom_override
        ),
    }
    members.update(extra_members or {})
    for name in omit_members:
        members.pop(name, None)
    record_path = f"{dist_info}/RECORD"
    record = _record_bytes(members, bad_hash=bad_record, omit_path=omit_record_path)
    record += f"{record_path},,\n".encode()
    members[record_path] = record
    path.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(path, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for name, data in members.items():
            info = zipfile.ZipInfo(name, date_time=(2020, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o100644 << 16
            archive.writestr(info, data)
    return path


def _sdist_files(version, *, locked=True):
    readme = (package_lib.PACKAGE_ROOT / "README.md").read_bytes()
    mit = (package_lib.PACKAGE_ROOT / "LICENSE-MIT").read_bytes()
    apache = (package_lib.PACKAGE_ROOT / "LICENSE-APACHE").read_bytes()
    files = {
        "PKG-INFO": _metadata(version),
        "Cargo.lock": b"# synthetic lock\nversion = 4\n",
        "Cargo.toml": b"[workspace]\nmembers = []\n",
        "pyproject.toml": f"[tool.maturin]\nlocked = {'true' if locked else 'false'}\n".encode(),
        "README.md": readme,
        "LICENSE-MIT": mit,
        "LICENSE-APACHE": apache,
        "crates/ferric-rules-python/Cargo.toml": b"[package]\nname='ferric-rules-python'\n",
        "crates/ferric-rules-python/README.md": readme,
        "crates/ferric-rules-python/LICENSE-MIT": mit,
        "crates/ferric-rules-python/LICENSE-APACHE": apache,
        "crates/ferric-rules-python/build.rs": b"fn main() {}\n",
        "crates/ferric-rules-python/src/lib.rs": b"pub fn synthetic() {}\n",
        "crates/ferric-rules-core/Cargo.toml": b"[package]\nname='ferric-rules-core'\n",
        "crates/ferric-rules-core/README.md": b"core\n",
        "crates/ferric-rules-core/src/lib.rs": b"pub fn synthetic() {}\n",
        "crates/ferric-rules-parser/Cargo.toml": b"[package]\nname='ferric-rules-parser'\n",
        "crates/ferric-rules-parser/README.md": b"parser\n",
        "crates/ferric-rules-parser/src/lib.rs": b"pub fn synthetic() {}\n",
        "crates/ferric-rules-runtime/Cargo.toml": b"[package]\nname='ferric-rules-runtime'\n",
        "crates/ferric-rules-runtime/README.md": b"runtime\n",
        "crates/ferric-rules-runtime/src/lib.rs": b"pub fn synthetic() {}\n",
    }
    return files


def _write_sdist(
    directory, contract, version, *, extra_files=None, omit=(), locked=True
):
    path = directory / package_lib.expected_sdist_filename(contract, version)
    root = path.name[: -len(".tar.gz")]
    files = _sdist_files(version, locked=locked)
    files.update(extra_files or {})
    for name in omit:
        files.pop(name, None)
    path.parent.mkdir(parents=True, exist_ok=True)
    with tarfile.open(path, "w:gz") as archive:
        for relative, data in files.items():
            info = tarfile.TarInfo(f"{root}/{relative}")
            info.size = len(data)
            info.mode = 0o644
            archive.addfile(info, io.BytesIO(data))
    return path


def _runtime_receipt(target):
    runtime = target["runtime"]
    return {
        "os": runtime["os"],
        "architecture": runtime["architecture"],
        "libc": runtime.get("libc"),
        "libc_version": runtime.get("libc_minimum"),
        "os_version": runtime.get("minimum_os_version"),
    }


def _smoke_payload(target, minor):
    prefix = "Lib" if target["runtime"]["os"] == "windows" else f"lib/python{minor}"
    native = "ferric.pyd" if target["runtime"]["os"] == "windows" else "ferric.abi3.so"
    return {
        "checks": list(package_lib.EXPECTED_SMOKE_CHECKS),
        "import_path_relative_to_venv": f"{prefix}/site-packages/ferric/__init__.py",
        "native_path_relative_to_venv": f"{prefix}/site-packages/ferric/{native}",
    }


def _wheel_receipt(inspection, target, minor):
    return {
        "schema_version": 1,
        "kind": "wheel-smoke",
        "status": "passed",
        "distribution": "ferric",
        "version": inspection.version,
        "target_id": target["id"],
        "python": {
            "implementation": "CPython",
            "version": f"{minor}.1",
            "minor": minor,
            "gil_enabled": True,
        },
        "runtime": _runtime_receipt(target),
        "wheel": {"filename": inspection.filename, "sha256": inspection.sha256},
        "smoke": _smoke_payload(target, minor),
    }


def _write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, sort_keys=True), encoding="utf-8")


def _complete_artifact_set(root, contract, version):
    artifacts = root / "artifacts"
    receipts = root / "receipts"
    inspections = {}
    targets = {target["id"]: target for target in contract["wheels"]}
    for target in contract["wheels"]:
        wheel = _write_wheel(artifacts, contract, version, target)
        inspections[target["id"]] = package_lib.validate_wheel(wheel, contract, version)
    sdist_path = _write_sdist(artifacts, contract, version)
    sdist = package_lib.validate_sdist(sdist_path, contract, version)
    for target_id, inspection in inspections.items():
        target = targets[target_id]
        for minor in package_lib.EXPECTED_SUPPORTED_MINORS:
            _write_json(
                receipts / f"wheel-{target_id}-py{minor.replace('.', '')}.json",
                _wheel_receipt(inspection, target, minor),
            )
    rejection_target = targets["manylinux2014-x86_64"]
    rejection_inspection = inspections[rejection_target["id"]]
    rejection = _wheel_receipt(rejection_inspection, rejection_target, "3.14")
    rejection["kind"] = "python-rejection"
    rejection.pop("smoke")
    rejection["rejection"] = {
        "reason": "Requires-Python",
        "requires_python": ">=3.9,<3.14",
        "installer_exit_code": 1,
        "module_importable": False,
    }
    _write_json(receipts / "python-rejection.json", rejection)
    source_smoke = _wheel_receipt(rejection_inspection, rejection_target, "3.12")
    source_smoke["kind"] = "source-built-wheel"
    source_smoke["wheel"] = {
        "filename": package_lib.wheel_filename_for_platform_tags(
            contract,
            package_lib.source_built_wheel_platform_tags(rejection_target),
            version,
        ),
        "sha256": "a" * 64,
    }
    sdist_receipt = {
        "schema_version": 1,
        "kind": "sdist-smoke",
        "status": "passed",
        "distribution": "ferric",
        "version": version,
        "target_id": rejection_target["id"],
        "sdist": {
            "filename": sdist.filename,
            "sha256": sdist.sha256,
            "safe_archive_paths": True,
            "cargo_lock_locked_offline": True,
            "deterministic_repack": True,
            "pep517_built_from_exact_archive": True,
        },
        "wheel_smoke": source_smoke,
    }
    _write_json(receipts / "sdist-smoke.json", sdist_receipt)
    return artifacts, receipts


def test_repository_metadata_and_abi_contract_is_valid():
    assert package_lib.validate_repository_contract() == "0.1.0"


def test_distribution_license_copies_have_stable_checkout_bytes():
    attributes = (REPO_ROOT / ".gitattributes").read_text(encoding="utf-8")
    for filename in package_lib.EXPECTED_LICENSE_FILES:
        assert f"{filename} text eol=lf" in attributes
        assert f"crates/ferric-rules-python/{filename} text eol=lf" in attributes


def test_musl_linker_replaces_only_the_dynamic_gcc_runtime_and_libc_name(
    tmp_path,
):
    original = ["-shared", "input.o", "-lgcc_s", "-lc", "-o", "ferric.so"]
    assert musl_linker.rewrite_linker_arguments(
        original,
        alias_dir=tmp_path,
        canonical_libc="libc.musl-x86_64.so.1",
    ) == [
        "-shared",
        "input.o",
        "-Wl,-Bstatic",
        "-lgcc_eh",
        "-Wl,-Bdynamic",
        f"-L{tmp_path}",
        "-l:libc.musl-x86_64.so.1",
        "-o",
        "ferric.so",
    ]


@pytest.mark.parametrize(
    "dynamic_arguments",
    [[], ["-lgcc_s", "-lgcc_s", "-lc"], ["-lgcc_s", "-lc", "-lc"]],
)
def test_musl_linker_rejects_toolchain_argument_drift(dynamic_arguments, tmp_path):
    with pytest.raises(ValueError, match="exactly one dynamic -lgcc_s and one -lc"):
        musl_linker.rewrite_linker_arguments(
            dynamic_arguments,
            alias_dir=tmp_path,
            canonical_libc="libc.musl-x86_64.so.1",
        )


def test_clean_venv_uses_platform_safe_python_layout(tmp_path):
    venv_root = package_lib._new_clean_venv(tmp_path)  # noqa: SLF001
    python = package_lib.find_python_executable(venv_root)
    result = subprocess.run(
        [str(python), "-I", "-c", "import pip,sys; print(sys.prefix)"],
        cwd=tmp_path,
        check=True,
        capture_output=True,
        text=True,
    )
    assert Path(result.stdout.strip()).resolve() == venv_root.resolve()
    if os.name != "nt":
        assert python.is_symlink()


@pytest.mark.parametrize("target_id", list(package_lib.EXPECTED_TARGETS))
def test_each_exact_wheel_shape_is_accepted(tmp_path, contract, version, target_id):
    target = package_lib.contract_target(contract, target_id)
    wheel = _write_wheel(tmp_path, contract, version, target)
    inspection = package_lib.validate_wheel(wheel, contract, version)
    assert inspection.target_id == target_id


@pytest.mark.parametrize(
    ("mutation", "message"),
    [
        ({"metadata_version": "9.9.9"}, "Version differs"),
        ({"requires_python": ">=3.9"}, "Requires-Python"),
        ({"dependency": "requests"}, "runtime dependencies"),
        ({"bad_record": True}, "RECORD hash mismatch"),
        ({"omit_record_path": "ferric/__init__.py"}, "RECORD paths"),
        ({"extra_members": {"../escape": b"secret"}}, "traverses upward"),
        ({"extra_members": {"ferric.libs/libsecret.so": b"ELF"}}, "exactly one native"),
        ({"native_architecture": "aarch64"}, "e_machine"),
        ({"native_truncated": True}, "truncated ELF"),
        ({"native_wrong_format": True}, "not an ELF"),
        ({"license_drift": True}, "checked-in license"),
        ({"sbom_override": b"not-json"}, "CycloneDX JSON"),
    ],
)
def test_wheel_adversarial_mutations_are_rejected(
    tmp_path, contract, version, mutation, message
):
    target = package_lib.contract_target(contract, "manylinux2014-x86_64")
    wheel = _write_wheel(tmp_path, contract, version, target, **mutation)
    with pytest.raises(package_lib.PackageValidationError, match=message):
        package_lib.validate_wheel(wheel, contract, version)


@pytest.mark.parametrize(
    ("target_id", "mutation", "message"),
    [
        ("macos-arm64", {"native_architecture": "x86_64"}, "CPU type"),
        ("macos-arm64", {"native_truncated": True}, "truncated Mach-O"),
        ("macos-arm64", {"native_wrong_format": True}, "not a little-endian"),
        ("windows-x86_64", {"native_architecture": "aarch64"}, "PE machine"),
        ("windows-x86_64", {"native_truncated": True}, "truncated PE"),
        ("windows-x86_64", {"native_wrong_format": True}, "not a PE"),
    ],
)
def test_native_header_format_and_architecture_must_match_target(
    tmp_path, contract, version, target_id, mutation, message
):
    target = package_lib.contract_target(contract, target_id)
    wheel = _write_wheel(tmp_path, contract, version, target, **mutation)
    with pytest.raises(package_lib.PackageValidationError, match=message):
        package_lib.validate_wheel(wheel, contract, version)


def test_bad_filename_platform_tag_is_rejected(tmp_path, contract, version):
    target = package_lib.contract_target(contract, "manylinux2014-x86_64")
    wheel = _write_wheel(tmp_path, contract, version, target)
    bad = wheel.with_name(wheel.name.replace("manylinux_2_17_x86_64", "linux_x86_64"))
    wheel.rename(bad)
    with pytest.raises(
        package_lib.PackageValidationError, match="not one exact contracted artifact"
    ):
        package_lib.validate_wheel(bad, contract, version)


def test_wheel_tag_metadata_must_match_filename(tmp_path, contract, version):
    target = package_lib.contract_target(contract, "manylinux2014-x86_64")
    wheel = _write_wheel(
        tmp_path, contract, version, target, wheel_tags=["linux_x86_64"]
    )
    with pytest.raises(package_lib.PackageValidationError, match="WHEEL tags"):
        package_lib.validate_wheel(wheel, contract, version)


def test_wheel_requires_both_license_payloads(tmp_path, contract, version):
    target = package_lib.contract_target(contract, "manylinux2014-x86_64")
    dist_info = f"ferric-{version}.dist-info"
    wheel = _write_wheel(
        tmp_path,
        contract,
        version,
        target,
        omit_members=(f"{dist_info}/licenses/LICENSE-MIT",),
    )
    with pytest.raises(
        package_lib.PackageValidationError, match="missing required members"
    ):
        package_lib.validate_wheel(wheel, contract, version)


@pytest.mark.parametrize(
    ("old", "new", "message"),
    [
        (package_lib.EXPECTED_SUMMARY.encode(), b"Wrong summary", "Summary drifted"),
        (b"License-File: LICENSE-MIT\n", b"", "License-File headers"),
        (
            b"License-Expression: MIT OR Apache-2.0",
            b"License-Expression: MIT",
            "license expression",
        ),
        (
            b"Project-URL: Homepage, https://github.com/plx/ferric-rules\n",
            b"",
            "project URLs",
        ),
        (b"# Ferric for Python", b"# Wrong README", "README body"),
    ],
)
def test_publish_metadata_deletion_or_drift_is_rejected(
    tmp_path, contract, version, old, new, message
):
    target = package_lib.contract_target(contract, "manylinux2014-x86_64")
    metadata = _metadata(version)
    assert old in metadata
    wheel = _write_wheel(
        tmp_path,
        contract,
        version,
        target,
        metadata_override=metadata.replace(old, new, 1),
    )
    with pytest.raises(package_lib.PackageValidationError, match=message):
        package_lib.validate_wheel(wheel, contract, version)


def test_sbom_rejects_an_unexpected_native_component(tmp_path, contract, version):
    target = package_lib.contract_target(contract, "manylinux2014-x86_64")
    sbom = json.loads(_sbom(version))
    sbom["components"] = [{"type": "file", "name": "vendored-secret.so"}]
    wheel = _write_wheel(
        tmp_path,
        contract,
        version,
        target,
        sbom_override=json.dumps(sbom).encode(),
    )
    with pytest.raises(
        package_lib.PackageValidationError, match="bundled native components"
    ):
        package_lib.validate_wheel(wheel, contract, version)


@pytest.mark.parametrize(
    ("options", "message"),
    [
        ({"extra_files": {"../escape": b"secret"}}, "traverses upward"),
        (
            {"extra_files": {"credentials.env": b"TOKEN=secret"}},
            "unexpected publish payload",
        ),
        (
            {"extra_files": {"crates/unlisted/src/lib.rs": b"secret"}},
            "unexpected publish payload",
        ),
        ({"omit": ("LICENSE-MIT",)}, "missing members"),
        ({"extra_files": {"README.md": b"drift"}}, "copies of README.md differ"),
        ({"locked": False}, "locked = true"),
    ],
)
def test_sdist_adversarial_mutations_are_rejected(
    tmp_path, contract, version, options, message
):
    sdist = _write_sdist(tmp_path, contract, version, **options)
    with pytest.raises(package_lib.PackageValidationError, match=message):
        package_lib.validate_sdist(sdist, contract, version)


def test_complete_artifact_and_receipt_matrix_produces_deterministic_manifest(
    tmp_path, contract, version
):
    artifacts, receipts = _complete_artifact_set(tmp_path, contract, version)
    first = package_lib.verify_artifact_set(artifacts, receipts, contract, version)
    second = package_lib.verify_artifact_set(artifacts, receipts, contract, version)
    assert first == second
    assert len(first["artifacts"]) == 8
    assert first["receipt_coverage"] == {
        "wheel_smokes": 35,
        "python_314_rejections": 1,
        "sdist_smokes": 1,
    }


def test_artifact_set_rejects_missing_and_duplicate_wheels(tmp_path, contract, version):
    missing_root = tmp_path / "missing"
    artifacts, receipts = _complete_artifact_set(missing_root, contract, version)
    next(artifacts.glob("*.whl")).unlink()
    with pytest.raises(
        package_lib.PackageValidationError, match="exactly seven wheels"
    ):
        package_lib.verify_artifact_set(artifacts, receipts, contract, version)

    duplicate_root = tmp_path / "duplicate"
    artifacts, receipts = _complete_artifact_set(duplicate_root, contract, version)
    wheel = next(artifacts.glob("*.whl"))
    duplicate = artifacts / "nested" / wheel.name
    duplicate.parent.mkdir()
    shutil.copyfile(wheel, duplicate)
    with pytest.raises(
        package_lib.PackageValidationError, match="exactly seven wheels"
    ):
        package_lib.verify_artifact_set(artifacts, receipts, contract, version)


def test_receipt_matrix_rejects_missing_duplicate_and_bad_identity(
    tmp_path, contract, version
):
    missing_root = tmp_path / "missing"
    artifacts, receipts = _complete_artifact_set(missing_root, contract, version)
    next(receipts.glob("wheel-*.json")).unlink()
    with pytest.raises(
        package_lib.PackageValidationError, match="expected 37 receipts"
    ):
        package_lib.verify_artifact_set(artifacts, receipts, contract, version)

    duplicate_root = tmp_path / "duplicate"
    artifacts, receipts = _complete_artifact_set(duplicate_root, contract, version)
    receipt = next(receipts.glob("wheel-*.json"))
    shutil.copyfile(receipt, receipts / f"duplicate-{receipt.name}")
    with pytest.raises(
        package_lib.PackageValidationError, match="expected 37 receipts"
    ):
        package_lib.verify_artifact_set(artifacts, receipts, contract, version)

    identity_root = tmp_path / "identity"
    artifacts, receipts = _complete_artifact_set(identity_root, contract, version)
    receipt = next(receipts.glob("wheel-*.json"))
    value = json.loads(receipt.read_text())
    value["wheel"]["sha256"] = "0" * 64
    _write_json(receipt, value)
    with pytest.raises(package_lib.PackageValidationError, match="final wheel"):
        package_lib.verify_artifact_set(artifacts, receipts, contract, version)


def test_receipt_rejects_boolean_schema_version(tmp_path, contract, version):
    artifacts, receipts = _complete_artifact_set(tmp_path, contract, version)
    receipt = next(receipts.glob("wheel-*.json"))
    value = json.loads(receipt.read_text())
    value["schema_version"] = True
    _write_json(receipt, value)
    with pytest.raises(package_lib.PackageValidationError, match="integer 1"):
        package_lib.verify_artifact_set(artifacts, receipts, contract, version)


def test_receipt_rejects_an_unsupported_python_minor(tmp_path, contract, version):
    artifacts, receipts = _complete_artifact_set(tmp_path, contract, version)
    receipt = next(receipts.glob("wheel-*.json"))
    value = json.loads(receipt.read_text())
    value["python"]["minor"] = "3.8"
    value["python"]["version"] = "3.8.20"
    _write_json(receipt, value)
    with pytest.raises(package_lib.PackageValidationError, match="supported-minor"):
        package_lib.verify_artifact_set(artifacts, receipts, contract, version)


def test_contract_rejects_boolean_schema_version(contract):
    invalid = copy.deepcopy(contract)
    invalid["schema_version"] = True
    with pytest.raises(package_lib.PackageValidationError, match="integer 1"):
        package_lib.validate_contract(invalid)

    invalid = copy.deepcopy(contract)
    invalid["sdist"]["artifact_count"] = True
    with pytest.raises(package_lib.PackageValidationError, match="exactly one sdist"):
        package_lib.validate_contract(invalid)


def _maturin_executable():
    executable = Path(sys.executable).with_name(
        "maturin.exe" if os.name == "nt" else "maturin"
    )
    if executable.is_file():
        return str(executable)
    return shutil.which("maturin")


@pytest.mark.skipif(_maturin_executable() is None, reason="Maturin is unavailable")
def test_real_sdist_stale_lock_is_normalized_and_repacked_deterministically(
    tmp_path, contract, version
):
    raw_dir = tmp_path / "raw"
    raw_dir.mkdir()
    subprocess.run(
        [
            _maturin_executable(),
            "sdist",
            "--manifest-path",
            str(package_lib.PYTHON_CARGO_PATH),
            "--out",
            str(raw_dir),
        ],
        cwd=REPO_ROOT,
        check=True,
        capture_output=True,
        text=True,
    )
    raw = raw_dir / package_lib.expected_sdist_filename(contract, version)
    source = package_lib.safe_extract_sdist(
        raw, tmp_path / "raw-extracted", contract, version
    )
    stale = subprocess.run(
        [
            "cargo",
            "metadata",
            "--format-version",
            "1",
            "--offline",
            "--locked",
            "--manifest-path",
            str(source / "Cargo.toml"),
        ],
        cwd=source,
        check=False,
        capture_output=True,
        text=True,
    )
    assert stale.returncode != 0
    assert "lock file" in stale.stderr
    assert "--locked" in stale.stderr

    first = package_lib.normalize_sdist(
        raw,
        tmp_path / "first" / raw.name,
        contract,
        version,
    )
    second = package_lib.normalize_sdist(
        raw,
        tmp_path / "second" / raw.name,
        contract,
        version,
    )
    assert first.sha256 == second.sha256
    assert first.size == second.size
