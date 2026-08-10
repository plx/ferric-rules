#!/usr/bin/env python3
"""Verify and retain the exact seven-target native Rust evidence bundle."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import sys
import tarfile
from collections.abc import Mapping, Sequence
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parents[1]

MANDATORY_COMMAND_NAMES = (
    "rustc-verbose",
    "cargo-verbose",
    "release-test",
    "release-build",
    "package-facade",
    "package-cli",
    "install-cli",
    "cli-version-command",
    "cli-version-flag",
    "cli-help",
    "cli-check-crlf-unicode",
    "cli-run-crlf-unicode",
    "cli-snapshot",
    "cli-snapshot-repl-eof",
    "cli-invalid-source",
    "cli-usage-error",
    "dynamic-dependencies",
)

SMOKE_NAMES = (
    "outside-worktree-install",
    "version",
    "unicode-path",
    "crlf-source",
    "snapshot-restore",
    "repl-eof",
    "exit-codes",
    "dynamic-dependencies",
)

RECEIPT_KEYS = {
    "schema_version",
    "candidate",
    "artifact_contract",
    "target",
    "declaration_sha256",
    "toolchain",
    "environment",
    "commands",
    "packages",
    "installed_binary",
    "smokes",
    "dynamic_dependencies",
}


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError(f"cannot read JSON from {path}: {error}") from error
    if not isinstance(value, dict):
        raise ValueError(f"{path} must contain a JSON object")
    return value


def _require_exact_keys(value: Mapping[str, Any], expected: set[str], context: str) -> None:
    actual = set(value)
    if actual != expected:
        raise ValueError(
            f"{context} keys differ: missing={sorted(expected - actual)}, "
            f"extra={sorted(actual - expected)}"
        )


def _require_hex(value: Any, length: int, context: str) -> str:
    if not isinstance(value, str) or re.fullmatch(rf"[0-9a-f]{{{length}}}", value) is None:
        raise ValueError(f"{context} must be {length} lowercase hexadecimal characters")
    return value


def _require_nonempty_string(value: Any, context: str) -> str:
    if not isinstance(value, str) or not value:
        raise ValueError(f"{context} must be a non-empty string")
    return value


def _expected_commands(target: Mapping[str, Any]) -> list[list[str]]:
    rust_target = str(target["rust_target"])
    target_args = ["--target", rust_target, "--target-dir", "$SMOKE/cargo-target"]
    binary_name = "ferric.exe" if target["family"] == "windows" else "ferric"
    binary = f"$INSTALL/bin/{binary_name}"
    smoke_root = "$SMOKE/outside-worktree Unicode space Ω"
    source = f"{smoke_root}/规则 unicode CRLF.clp"
    invalid = f"{smoke_root}/invalid source.clp"
    snapshot = f"{smoke_root}/snapshot state.json"
    if target["libc"] == "musl":
        dependency = ["readelf", "-l", binary]
    elif target["family"] == "macos":
        dependency = ["otool", "-L", binary]
    elif target["family"] == "windows":
        dependency = ["dumpbin", "/dependents", binary]
    else:
        dependency = ["ldd", binary]
    return [
        ["rustc", "-vV"],
        ["cargo", "-vV"],
        [
            "cargo",
            "test",
            "--release",
            "-p",
            "ferric-rules",
            "-p",
            "ferric-rules-cli",
            "--all-features",
            "--locked",
            *target_args,
        ],
        [
            "cargo",
            "build",
            "--release",
            "-p",
            "ferric-rules-cli",
            "--all-features",
            "--locked",
            *target_args,
        ],
        [
            "cargo",
            "package",
            "-p",
            "ferric-rules",
            "--all-features",
            "--locked",
            *target_args,
        ],
        [
            "cargo",
            "package",
            "-p",
            "ferric-rules-cli",
            "--all-features",
            "--locked",
            *target_args,
        ],
        [
            "cargo",
            "install",
            "--path",
            "$REPO/crates/ferric-rules-cli",
            "--root",
            "$INSTALL",
            "--all-features",
            "--locked",
            *target_args,
        ],
        [binary, "version"],
        [binary, "--version"],
        [binary, "--help"],
        [binary, "check", source],
        [binary, "run", source],
        [
            binary,
            "snapshot",
            "--json",
            source,
            "--output",
            snapshot,
            "--format",
            "json",
        ],
        [binary, "repl", "--snapshot", snapshot, "--snapshot-format", "json"],
        [binary, "check", "--json", invalid],
        [binary],
        dependency,
    ]


def _validate_toolchain(toolchain: Any, target: Mapping[str, Any]) -> None:
    if not isinstance(toolchain, dict):
        raise ValueError("toolchain must be an object")
    _require_exact_keys(toolchain, {"rustc", "cargo"}, "toolchain")
    for tool in ("rustc", "cargo"):
        identity = toolchain[tool]
        if not isinstance(identity, dict):
            raise ValueError(f"toolchain.{tool} must be an object")
        _require_exact_keys(identity, {"release", "host", "commit_hash"}, f"toolchain.{tool}")
        if identity["release"] != "1.93.0":
            raise ValueError(f"toolchain.{tool}.release must be 1.93.0")
        if identity["host"] != target["rust_target"]:
            raise ValueError(f"toolchain.{tool}.host does not match the target")
        _require_hex(identity["commit_hash"], 40, f"toolchain.{tool}.commit_hash")


def _validate_environment(environment: Any, target: Mapping[str, Any]) -> None:
    if not isinstance(environment, dict):
        raise ValueError("environment must be an object")
    _require_exact_keys(
        environment,
        {"family", "architecture", "libc", "libc_version", "platform"},
        "environment",
    )
    for key in ("family", "architecture", "libc"):
        if environment[key] != target[key]:
            raise ValueError(f"environment.{key} does not match target.{key}")
    if target["family"] == "linux":
        libc_version = _require_nonempty_string(
            environment["libc_version"], "environment.libc_version"
        )
        if target["libc"] == "musl" and re.fullmatch(r"1\.2\.\d+", libc_version) is None:
            raise ValueError("environment.libc_version must be an observed musl 1.2.x")
    elif environment["libc_version"] is not None:
        raise ValueError("environment.libc_version must be null outside Linux")
    _require_nonempty_string(environment["platform"], "environment.platform")


def _validate_commands(commands: Any, target: Mapping[str, Any]) -> None:
    if not isinstance(commands, list) or len(commands) != len(MANDATORY_COMMAND_NAMES):
        raise ValueError("commands must contain the exact mandatory command sequence")
    expected_argv = _expected_commands(target)
    for index, (command, name, argv) in enumerate(
        zip(commands, MANDATORY_COMMAND_NAMES, expected_argv, strict=True)
    ):
        if not isinstance(command, dict):
            raise ValueError(f"commands[{index}] must be an object")
        _require_exact_keys(
            command,
            {"name", "argv", "expected_exit", "actual_exit"},
            f"commands[{index}]",
        )
        if command["name"] != name:
            raise ValueError(f"commands[{index}] must be {name}")
        if command["argv"] != argv:
            raise ValueError(f"commands[{index}].argv differs from the normalized contract")
        expected_exit = 1 if name == "cli-invalid-source" else 2 if name == "cli-usage-error" else 0
        if command["expected_exit"] != expected_exit:
            raise ValueError(f"commands[{index}].expected_exit differs")
        if command["actual_exit"] != expected_exit:
            raise ValueError(f"commands[{index}] did not pass")


def _validate_dynamic_dependency_report(*, family: str, libc: str, report: str) -> None:
    normalized = report.strip().lower()
    if not normalized:
        raise ValueError("dynamic dependency report is empty")
    if "not found" in normalized or "could not be found" in normalized:
        raise ValueError("dynamic dependency report contains a missing library")
    normalized_repo = str(REPO_ROOT.resolve()).replace("\\", "/").casefold()
    normalized_report = report.replace("\\", "/").casefold()
    if normalized_repo in normalized_report:
        raise ValueError("dynamic dependency report references the repository worktree")
    if family == "linux" and libc == "musl":
        probe_names = ("ldd", "readelf -l", "file")
        markers = list(re.finditer(r"^\[probe (ldd|readelf -l|file)\]\n", report, re.MULTILINE))
        if tuple(match.group(1) for match in markers) != probe_names:
            raise ValueError("musl dependency report has incomplete probe sections")
        probes: dict[str, tuple[int, str]] = {}
        for index, (probe, marker) in enumerate(zip(probe_names, markers, strict=True)):
            end = markers[index + 1].start() if index + 1 < len(markers) else len(report)
            section = report[marker.end() : end]
            status = re.match(r"(?:argv=[^\r\n]*\r?\n)?exit=(-?\d+)(?:\r?\n|$)", section)
            if status is None:
                raise ValueError(f"musl dependency report omitted {probe} status")
            probes[probe] = (int(status.group(1)), section[status.end() :])
        if probes["readelf -l"][0] != 0 or probes["file"][0] != 0:
            raise ValueError("musl dependency inspection probe failed")
        if "ld-linux" in normalized or "glibc" in normalized:
            raise ValueError("musl evidence references a glibc interpreter")
        if re.search(r"(?<!musl[-_.])libc\.so\.6", normalized):
            raise ValueError("musl evidence references glibc libc.so.6")
        readelf_report = probes["readelf -l"][1].lower()
        file_report = probes["file"][1].lower()
        interpreters = re.findall(
            r"requesting program interpreter:\s*([^\]\r\n]+)",
            readelf_report,
        )
        has_interpreter_segment = (
            re.search(r"^\s*interp(?:\s|$)", readelf_report, re.MULTILINE) is not None
            or "program interpreter" in readelf_report
        )
        if has_interpreter_segment:
            if len(interpreters) != 1:
                raise ValueError("musl readelf evidence has an ambiguous interpreter")
            interpreter = interpreters[0].strip()
            if (
                not interpreter.startswith("/")
                or any(character.isspace() for character in interpreter)
                or not interpreter.rsplit("/", 1)[-1].startswith("ld-musl")
            ):
                raise ValueError("musl readelf evidence has a non-musl interpreter path")
        elif re.search(r"\bstatic(?:ally|-pie)?\b", file_report) is None:
            raise ValueError(
                "musl file evidence does not corroborate a no-interpreter static binary"
            )
    elif family == "linux" and libc == "glibc":
        if "libc.so.6" not in normalized:
            raise ValueError("glibc evidence omitted libc.so.6")
    elif family == "macos":
        if ".dylib" not in normalized:
            raise ValueError("Mach-O evidence omitted dylibs")
    elif family == "windows":
        if ".dll" not in normalized or "dependencies" not in normalized:
            raise ValueError("PE evidence omitted DLL dependencies")
    else:
        raise ValueError(f"unsupported dependency report family/libc {family}/{libc}")


def _safe_evidence_tree(root: Path, target_id: str, candidate_sha: str) -> dict[str, Path]:
    if root.name != f"rust-native-{target_id}-{candidate_sha}":
        raise ValueError(f"unexpected artifact directory {root.name}")
    if root.is_symlink() or not root.is_dir():
        raise ValueError(f"artifact root is not a real directory: {root}")
    for current, directories, files in os.walk(root, followlinks=False):
        current_path = Path(current)
        for name in [*directories, *files]:
            path = current_path / name
            if path.is_symlink():
                raise ValueError(f"artifact evidence contains symlink {path}")
    expected_directories = {root / "packages", root / "bin"}
    actual_directories = {path for path in root.rglob("*") if path.is_dir()}
    if actual_directories != expected_directories:
        raise ValueError(f"unexpected directories in {root}")
    expected_files = {
        root / "receipt.json",
        root / "dynamic-dependencies.txt",
        root / "bin" / ("ferric.exe" if target_id.startswith("windows-") else "ferric"),
    }
    package_files = set((root / "packages").glob("*.crate"))
    expected_files.update(package_files)
    actual_files = {path for path in root.rglob("*") if path.is_file()}
    if actual_files != expected_files:
        raise ValueError(f"unexpected files in {root}")
    if len(package_files) != 2:
        raise ValueError(f"{root} must contain exactly two source packages")
    return {
        "receipt": root / "receipt.json",
        "report": root / "dynamic-dependencies.txt",
        "binary": root / "bin" / ("ferric.exe" if target_id.startswith("windows-") else "ferric"),
    }


def _validate_file_record(record: Any, *, root: Path, context: str) -> Path:
    if not isinstance(record, dict):
        raise ValueError(f"{context} must be an object")
    required = {"filename", "sha256", "size"}
    if context.startswith("packages["):
        required.update({"name", "version"})
    _require_exact_keys(record, required, context)
    filename = _require_nonempty_string(record["filename"], f"{context}.filename")
    relative = Path(filename)
    if relative.is_absolute() or ".." in relative.parts:
        raise ValueError(f"{context}.filename is not a safe relative path")
    path = root / relative
    if path.is_symlink() or not path.is_file():
        raise ValueError(f"{context} file is missing: {path}")
    if record["sha256"] != _sha256(path):
        raise ValueError(f"{context} sha256 does not match {path}")
    if record["size"] != path.stat().st_size:
        raise ValueError(f"{context} size does not match {path}")
    return path


def _validate_package_vcs(path: Path, candidate_sha: str) -> None:
    try:
        with tarfile.open(path, mode="r:gz") as package:
            records = [
                member
                for member in package.getmembers()
                if member.name.endswith("/.cargo_vcs_info.json")
            ]
            if len(records) != 1:
                raise ValueError(f"{path.name} must contain exactly one Cargo VCS record")
            extracted = package.extractfile(records[0])
            if extracted is None:
                raise ValueError(f"cannot read Cargo VCS record in {path.name}")
            vcs = json.loads(extracted.read().decode("utf-8"))
    except (tarfile.TarError, json.JSONDecodeError, UnicodeDecodeError) as error:
        raise ValueError(f"cannot inspect Cargo VCS record in {path}: {error}") from error
    git = vcs.get("git", {}) if isinstance(vcs, dict) else {}
    if not isinstance(git, dict):
        raise ValueError(f"{path.name} Cargo VCS git record is invalid")
    if git.get("sha1") != candidate_sha:
        raise ValueError(f"{path.name} Cargo VCS SHA does not match the candidate")
    if "dirty" in git and git["dirty"] is not False:
        raise ValueError(f"{path.name} Cargo VCS record is not clean")


def _validate_receipt(
    *,
    receipt_path: Path,
    declaration: Mapping[str, Any],
    declaration_sha256: str,
    declared_target: Mapping[str, Any],
    candidate_sha: str,
    candidate_tree: str,
) -> dict[str, Any]:
    receipt = _load_json(receipt_path)
    _require_exact_keys(receipt, RECEIPT_KEYS, str(receipt_path))
    if receipt["schema_version"] != 1:
        raise ValueError("receipt schema_version must be 1")
    if receipt["candidate"] != {"sha": candidate_sha, "tree": candidate_tree}:
        raise ValueError("receipt candidate SHA/tree drifted")
    if receipt["declaration_sha256"] != declaration_sha256:
        raise ValueError("receipt declaration_sha256 drifted")
    if receipt["artifact_contract"] != declaration["artifact_contract"]:
        raise ValueError("receipt artifact_contract drifted")
    if receipt["target"] != declared_target:
        raise ValueError("receipt target declaration drifted")
    _validate_toolchain(receipt["toolchain"], declared_target)
    _validate_environment(receipt["environment"], declared_target)
    _validate_commands(receipt["commands"], declared_target)

    root = receipt_path.parent
    files = _safe_evidence_tree(root, declared_target["id"], candidate_sha)
    packages = receipt["packages"]
    if not isinstance(packages, list) or len(packages) != 2:
        raise ValueError("receipt packages must contain exactly two records")
    expected_packages = declaration["artifact_contract"]["packages"]
    if [
        package.get("name") for package in packages if isinstance(package, dict)
    ] != expected_packages:
        raise ValueError("receipt package order or names drifted")
    versions: set[str] = set()
    for index, package in enumerate(packages):
        path = _validate_file_record(package, root=root, context=f"packages[{index}]")
        expected_prefix = f"packages/{package['name']}-{package['version']}"
        if package["filename"] != expected_prefix + ".crate" or path.suffix != ".crate":
            raise ValueError(f"packages[{index}] filename drifted")
        _validate_package_vcs(path, candidate_sha)
        versions.add(package["version"])
    if len(versions) != 1:
        raise ValueError("source package versions differ")

    binary = receipt["installed_binary"]
    binary_path = _validate_file_record(binary, root=root, context="installed_binary")
    if binary_path != files["binary"]:
        raise ValueError("installed_binary path drifted")
    if binary_path.stat().st_size == 0:
        raise ValueError("installed binary is empty")

    smokes = receipt["smokes"]
    if not isinstance(smokes, list) or len(smokes) != len(SMOKE_NAMES):
        raise ValueError("receipt smokes are incomplete")
    for index, (smoke, expected_name) in enumerate(zip(smokes, SMOKE_NAMES, strict=True)):
        if not isinstance(smoke, dict):
            raise ValueError(f"smokes[{index}] must be an object")
        _require_exact_keys(smoke, {"name", "status"}, f"smokes[{index}]")
        if smoke != {"name": expected_name, "status": "passed"}:
            raise ValueError(f"smokes[{index}] did not pass the exact contract")

    dynamic = receipt["dynamic_dependencies"]
    if not isinstance(dynamic, dict):
        raise ValueError("dynamic_dependencies must be an object")
    _require_exact_keys(dynamic, {"tool", "kind", "filename", "sha256"}, "dynamic_dependencies")
    if dynamic["filename"] != "dynamic-dependencies.txt":
        raise ValueError("dynamic dependency report filename drifted")
    if dynamic["sha256"] != _sha256(files["report"]):
        raise ValueError("dynamic dependency report sha256 drifted")
    _validate_dynamic_dependency_report(
        family=declared_target["family"],
        libc=declared_target["libc"],
        report=files["report"].read_text(encoding="utf-8"),
    )
    expected_tool = (
        "readelf"
        if declared_target["libc"] == "musl"
        else "otool"
        if declared_target["family"] == "macos"
        else "dumpbin"
        if declared_target["family"] == "windows"
        else "ldd"
    )
    if dynamic["tool"] != expected_tool:
        raise ValueError("dynamic dependency tool drifted")
    expected_kinds = (
        {"static", "musl"}
        if declared_target["libc"] == "musl"
        else {
            "mach-o"
            if declared_target["family"] == "macos"
            else "pe"
            if declared_target["family"] == "windows"
            else "glibc"
        }
    )
    if dynamic["kind"] not in expected_kinds:
        raise ValueError("dynamic dependency kind drifted")
    return receipt


def _copy_verified_tree(source: Path, destination: Path) -> None:
    shutil.copytree(source, destination, symlinks=False)


def verify_artifact_set(
    *,
    declaration_path: Path,
    artifacts_dir: Path,
    candidate_sha: str,
    candidate_tree: str,
    output_dir: Path,
) -> dict[str, Any]:
    """Verify exactly 7 native receipts and copy their immutable evidence bundle."""

    declaration_path = declaration_path.resolve()
    artifacts_dir = artifacts_dir.resolve()
    output_dir = output_dir.resolve()
    _require_hex(candidate_sha, 40, "candidate_sha")
    _require_hex(candidate_tree, 40, "candidate_tree")
    declaration = _load_json(declaration_path)
    _require_exact_keys(
        declaration, {"schema_version", "artifact_contract", "targets"}, "declaration"
    )
    if declaration["schema_version"] != 1:
        raise ValueError("declaration schema_version must be 1")
    targets = declaration["targets"]
    if not isinstance(targets, list) or len(targets) != 7:
        raise ValueError("release declaration must contain exactly 7 targets")
    by_id: dict[str, dict[str, Any]] = {}
    for target in targets:
        if not isinstance(target, dict) or not isinstance(target.get("id"), str):
            raise ValueError("release declaration contains an invalid target")
        if target["id"] in by_id:
            raise ValueError(f"duplicate declared target {target['id']}")
        by_id[target["id"]] = target

    receipt_paths = sorted(artifacts_dir.rglob("receipt.json"))
    if len(receipt_paths) != 7:
        raise ValueError(
            f"artifact set must contain exactly 7 receipts, found {len(receipt_paths)}"
        )
    declaration_digest = _sha256(declaration_path)
    receipts: dict[str, dict[str, Any]] = {}
    roots: dict[str, Path] = {}
    for receipt_path in receipt_paths:
        preliminary = _load_json(receipt_path)
        target_id = (
            preliminary.get("target", {}).get("id")
            if isinstance(preliminary.get("target"), dict)
            else None
        )
        if not isinstance(target_id, str) or target_id not in by_id:
            raise ValueError(f"receipt declares unsupported target {target_id!r}")
        if target_id in receipts:
            raise ValueError(f"duplicate receipt for target {target_id}")
        receipts[target_id] = _validate_receipt(
            receipt_path=receipt_path,
            declaration=declaration,
            declaration_sha256=declaration_digest,
            declared_target=by_id[target_id],
            candidate_sha=candidate_sha,
            candidate_tree=candidate_tree,
        )
        roots[target_id] = receipt_path.parent
    if set(receipts) != set(by_id):
        raise ValueError("artifact receipt target coverage is not exact")

    top_level = {path for path in artifacts_dir.iterdir()}
    if top_level != set(roots.values()):
        raise ValueError("artifact download contains unexpected top-level entries")
    if output_dir.exists():
        raise ValueError(f"verified output already exists: {output_dir}")
    evidence_dir = output_dir / "evidence"
    evidence_dir.mkdir(parents=True)
    shutil.copy2(declaration_path, output_dir / "release-targets.json")

    manifest_targets: list[dict[str, Any]] = []
    for target in targets:
        target_id = target["id"]
        destination = evidence_dir / roots[target_id].name
        _copy_verified_tree(roots[target_id], destination)
        receipt = receipts[target_id]
        manifest_targets.append(
            {
                "id": target_id,
                "evidence": f"evidence/{destination.name}",
                "packages": receipt["packages"],
                "installed_binary": receipt["installed_binary"],
                "dynamic_dependencies": receipt["dynamic_dependencies"],
            }
        )
    manifest = {
        "schema_version": 1,
        "candidate": {"sha": candidate_sha, "tree": candidate_tree},
        "declaration_sha256": declaration_digest,
        "artifact_contract": declaration["artifact_contract"],
        "targets": manifest_targets,
    }
    (output_dir / "manifest.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    return manifest


def _parse_args(argv: Sequence[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--declaration", type=Path, required=True)
    parser.add_argument("--artifacts-dir", type=Path, required=True)
    parser.add_argument("--candidate-sha", required=True)
    parser.add_argument("--candidate-tree", required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    args = _parse_args(sys.argv[1:] if argv is None else argv)
    manifest = verify_artifact_set(
        declaration_path=args.declaration,
        artifacts_dir=args.artifacts_dir,
        candidate_sha=args.candidate_sha,
        candidate_tree=args.candidate_tree,
        output_dir=args.output_dir,
    )
    print(
        f"verified native Rust artifact bundle for {len(manifest['targets'])} targets "
        f"at {manifest['candidate']['sha']}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
