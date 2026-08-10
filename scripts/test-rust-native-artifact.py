#!/usr/bin/env python3
"""Build, install, and smoke one declared native Rust/CLI target."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
from collections.abc import Mapping, Sequence
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parents[1]

CARGO_TEST_COMMAND = (
    "cargo",
    "test",
    "--release",
    "-p",
    "ferric-rules",
    "-p",
    "ferric-rules-cli",
    "--all-features",
    "--locked",
)
CARGO_BUILD_COMMAND = (
    "cargo",
    "build",
    "--release",
    "-p",
    "ferric-rules-cli",
    "--all-features",
    "--locked",
)
CARGO_PACKAGE_COMMANDS = (
    ("cargo", "package", "-p", "ferric-rules", "--all-features", "--locked"),
    (
        "cargo",
        "package",
        "-p",
        "ferric-rules-cli",
        "--all-features",
        "--locked",
    ),
)

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

ARTIFACT_CONTRACT_KEYS = {
    "packages",
    "binary",
    "install_all_features",
    "execution",
    "distribution",
}
TARGET_KEYS = {
    "id",
    "runner",
    "rust_target",
    "family",
    "architecture",
    "libc",
    "native_environment",
    "conformance",
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


def _validate_declaration(value: dict[str, Any]) -> None:
    _require_exact_keys(value, {"schema_version", "artifact_contract", "targets"}, "declaration")
    if value["schema_version"] != 1:
        raise ValueError("release declaration schema_version must be 1")
    contract = value["artifact_contract"]
    if not isinstance(contract, dict):
        raise ValueError("artifact_contract must be an object")
    _require_exact_keys(contract, ARTIFACT_CONTRACT_KEYS, "artifact_contract")
    expected_contract = {
        "packages": ["ferric-rules", "ferric-rules-cli"],
        "binary": "ferric",
        "install_all_features": True,
        "execution": "native",
        "distribution": "ci-evidence-only",
    }
    if contract != expected_contract:
        raise ValueError("artifact_contract differs from the native Rust CI contract")
    targets = value["targets"]
    if not isinstance(targets, list) or len(targets) != 7:
        raise ValueError("release declaration must contain exactly 7 targets")
    identifiers: set[str] = set()
    for index, target in enumerate(targets):
        if not isinstance(target, dict):
            raise ValueError(f"targets[{index}] must be an object")
        _require_exact_keys(target, TARGET_KEYS, f"targets[{index}]")
        target_id = target["id"]
        if not isinstance(target_id, str) or not target_id:
            raise ValueError(f"targets[{index}].id must be non-empty")
        if target_id in identifiers:
            raise ValueError(f"duplicate target id {target_id}")
        identifiers.add(target_id)
        if target["conformance"] != "native":
            raise ValueError(f"{target_id} is not native conformance")
        if target["family"] not in {"linux", "macos", "windows"}:
            raise ValueError(f"unsupported family for {target_id}")
        if target["libc"] not in {"glibc", "musl", "none", "msvc"}:
            raise ValueError(f"unsupported libc for {target_id}")


def _is_within(path: Path, root: Path) -> bool:
    try:
        path.resolve().relative_to(root.resolve())
        return True
    except ValueError:
        return False


def _normalize_argument(argument: str, roots: Mapping[str, Path]) -> str:
    normalized = argument.replace("\\", "/")
    candidates = sorted(
        ((token, str(path.resolve()).replace("\\", "/")) for token, path in roots.items()),
        key=lambda item: len(item[1]),
        reverse=True,
    )
    for token, prefix in candidates:
        if normalized == prefix:
            return token
        if normalized.startswith(prefix + "/"):
            return token + normalized[len(prefix) :]
    return normalized


def _command_record(
    *,
    name: str,
    argv: Sequence[str],
    expected_exit: int,
    actual_exit: int,
    roots: Mapping[str, Path],
) -> dict[str, Any]:
    return {
        "name": name,
        "argv": [_normalize_argument(str(argument), roots) for argument in argv],
        "expected_exit": expected_exit,
        "actual_exit": actual_exit,
    }


def _run(
    argv: Sequence[str],
    *,
    cwd: Path = REPO_ROOT,
    env: Mapping[str, str] | None = None,
    input_text: str | None = None,
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(argument) for argument in argv],
        cwd=cwd,
        env=None if env is None else dict(env),
        input=input_text,
        text=True,
        encoding="utf-8",
        errors="strict",
        capture_output=True,
        check=False,
    )


def _run_expected(
    *,
    name: str,
    argv: Sequence[str],
    expected_exit: int,
    commands: list[dict[str, Any]],
    roots: Mapping[str, Path],
    cwd: Path = REPO_ROOT,
    env: Mapping[str, str] | None = None,
    input_text: str | None = None,
) -> subprocess.CompletedProcess[str]:
    result = _run(argv, cwd=cwd, env=env, input_text=input_text)
    commands.append(
        _command_record(
            name=name,
            argv=argv,
            expected_exit=expected_exit,
            actual_exit=result.returncode,
            roots=roots,
        )
    )
    if result.returncode != expected_exit:
        raise RuntimeError(
            f"{name} exited {result.returncode}, expected {expected_exit}\n"
            f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}"
        )
    return result


def _parse_verbose_version(output: str, tool: str) -> dict[str, str]:
    fields: dict[str, str] = {}
    for line in output.splitlines():
        key, separator, value = line.partition(":")
        if separator and key in {"release", "host", "commit-hash"}:
            fields[key] = value.strip()
    missing = {"release", "host", "commit-hash"} - set(fields)
    if missing:
        raise RuntimeError(f"{tool} verbose version omitted {sorted(missing)}")
    return {
        "release": fields["release"],
        "host": fields["host"],
        "commit_hash": fields["commit-hash"],
    }


def _normalize_architecture(machine: str) -> str:
    value = machine.lower()
    if value in {"amd64", "x64", "x86_64"}:
        return "x86_64"
    if value in {"arm64", "aarch64"}:
        return "aarch64"
    return value


def _observed_environment(target: Mapping[str, Any]) -> dict[str, Any]:
    system = platform.system().lower()
    family = {"darwin": "macos", "windows": "windows", "linux": "linux"}.get(system, system)
    architecture = _normalize_architecture(platform.machine())
    libc = str(target["libc"])
    libc_version: str | None = None
    if family == "linux":
        observed_libc, observed_version = platform.libc_ver()
        observed_libc = observed_libc.lower()
        if libc == "glibc" and observed_libc not in {"glibc", "libc"}:
            version_result = _run(["ldd", "--version"])
            combined = (version_result.stdout + version_result.stderr).lower()
            if "glibc" not in combined and "gnu libc" not in combined:
                raise RuntimeError("declared glibc target is not executing on glibc")
        if libc == "musl":
            version_result = _run(["ldd", "--version"])
            combined = version_result.stdout + version_result.stderr
            if "musl" not in combined.lower():
                raise RuntimeError("declared musl target is not executing in a musl runtime")
            match = re.search(r"Version\s+(1\.2\.\d+)", combined, flags=re.IGNORECASE)
            if match is None:
                raise RuntimeError("musl runtime version is not an observed 1.2.x release")
            observed_version = match.group(1)
        libc_version = observed_version or observed_libc or libc
    if family != target["family"]:
        raise RuntimeError(f"runner family {family} does not match {target['family']}")
    if architecture != target["architecture"]:
        raise RuntimeError(
            f"runner architecture {architecture} does not match {target['architecture']}"
        )
    return {
        "family": family,
        "architecture": architecture,
        "libc": libc,
        "libc_version": libc_version,
        "platform": platform.platform(),
    }


def _assert_json_diagnostics(stderr: str) -> None:
    lines = [line for line in stderr.splitlines() if line.strip()]
    if not lines:
        raise RuntimeError("invalid-source smoke emitted no JSON diagnostics")
    for line in lines:
        try:
            diagnostic = json.loads(line)
        except json.JSONDecodeError as error:
            raise RuntimeError(f"invalid JSON diagnostic: {line}") from error
        if not isinstance(diagnostic, dict):
            raise RuntimeError("JSON diagnostic must be an object")
        for key in ("command", "level", "kind", "message"):
            if not isinstance(diagnostic.get(key), str) or not diagnostic[key]:
                raise RuntimeError(f"JSON diagnostic omitted {key}")


def run_cli_smokes(*, binary: Path, scratch_root: Path) -> list[dict[str, Any]]:
    """Run the observable CLI contract from a Unicode, spaced, outside-worktree path."""

    binary = binary.resolve()
    scratch_root = scratch_root.resolve()
    smoke_root = scratch_root / "outside-worktree Unicode space Ω"
    smoke_root.mkdir(parents=True, exist_ok=False)
    roots = {"$INSTALL": binary.parent.parent, "$SMOKE": scratch_root}
    commands: list[dict[str, Any]] = []

    source = smoke_root / "规则 unicode CRLF.clp"
    source_text = (
        '(deffacts start (message "Unicode café 世界"))\n'
        "(defrule emit (message ?text) => (printout t ?text crlf))\n"
    )
    source.write_bytes(source_text.replace("\n", "\r\n").encode("utf-8"))
    if b"\r\n" not in source.read_bytes():
        raise RuntimeError("CRLF smoke fixture was not written with CRLF bytes")

    invalid = smoke_root / "invalid source.clp"
    invalid.write_bytes(b"(defrule broken\r\n")
    snapshot = smoke_root / "snapshot state.json"

    version = _run_expected(
        name="cli-version-command",
        argv=[str(binary), "version"],
        expected_exit=0,
        commands=commands,
        roots=roots,
        cwd=smoke_root,
    )
    expected_version = f"ferric {_workspace_version()}"
    if version.stdout.strip() != expected_version:
        raise RuntimeError("version command did not identify the exact package version")

    version_flag = _run_expected(
        name="cli-version-flag",
        argv=[str(binary), "--version"],
        expected_exit=0,
        commands=commands,
        roots=roots,
        cwd=smoke_root,
    )
    if version_flag.stdout.strip() != expected_version:
        raise RuntimeError("--version did not identify the exact package version")
    if version.stdout.strip() != version_flag.stdout.strip():
        raise RuntimeError("version command and --version disagree")

    help_result = _run_expected(
        name="cli-help",
        argv=[str(binary), "--help"],
        expected_exit=0,
        commands=commands,
        roots=roots,
        cwd=smoke_root,
    )
    if "snapshot" not in help_result.stdout.lower() or "--trace" not in help_result.stdout:
        raise RuntimeError("all-features CLI help omitted snapshot or tracing")

    _run_expected(
        name="cli-check-crlf-unicode",
        argv=[str(binary), "check", str(source)],
        expected_exit=0,
        commands=commands,
        roots=roots,
        cwd=smoke_root,
    )
    run_result = _run_expected(
        name="cli-run-crlf-unicode",
        argv=[str(binary), "run", str(source)],
        expected_exit=0,
        commands=commands,
        roots=roots,
        cwd=smoke_root,
    )
    if run_result.stdout != "Unicode café 世界\n":
        raise RuntimeError("Unicode run smoke did not preserve the exact output line")

    _run_expected(
        name="cli-snapshot",
        argv=[
            str(binary),
            "snapshot",
            "--json",
            str(source),
            "--output",
            str(snapshot),
            "--format",
            "json",
        ],
        expected_exit=0,
        commands=commands,
        roots=roots,
        cwd=smoke_root,
    )
    if not snapshot.is_file() or snapshot.stat().st_size == 0:
        raise RuntimeError("snapshot smoke produced no snapshot")

    _run_expected(
        name="cli-snapshot-repl-eof",
        argv=[
            str(binary),
            "repl",
            "--snapshot",
            str(snapshot),
            "--snapshot-format",
            "json",
        ],
        expected_exit=0,
        commands=commands,
        roots=roots,
        cwd=smoke_root,
        input_text="",
    )

    invalid_result = _run_expected(
        name="cli-invalid-source",
        argv=[str(binary), "check", "--json", str(invalid)],
        expected_exit=1,
        commands=commands,
        roots=roots,
        cwd=smoke_root,
    )
    _assert_json_diagnostics(invalid_result.stderr)

    usage = _run_expected(
        name="cli-usage-error",
        argv=[str(binary)],
        expected_exit=2,
        commands=commands,
        roots=roots,
        cwd=smoke_root,
    )
    if not usage.stderr.strip():
        raise RuntimeError("usage-error smoke emitted no diagnostic")

    return commands


def validate_dynamic_dependency_report(*, family: str, libc: str, report: str) -> None:
    """Reject vacuous, missing, or wrong-libc native dependency reports."""

    normalized = report.strip().lower()
    if not normalized:
        raise ValueError("dynamic dependency report is empty")
    if "not found" in normalized or "could not be found" in normalized:
        raise RuntimeError("dynamic dependency report contains a missing library")
    normalized_repo = str(REPO_ROOT.resolve()).replace("\\", "/").casefold()
    normalized_report = report.replace("\\", "/").casefold()
    if normalized_repo in normalized_report:
        raise RuntimeError("dynamic dependency report references the repository worktree")
    if family == "linux" and libc == "musl":
        probe_names = ("ldd", "readelf -l", "file")
        markers = list(re.finditer(r"^\[probe (ldd|readelf -l|file)\]\n", report, re.MULTILINE))
        if tuple(match.group(1) for match in markers) != probe_names:
            raise RuntimeError("musl dependency report has incomplete probe sections")
        probes: dict[str, tuple[int, str]] = {}
        for index, (probe, marker) in enumerate(zip(probe_names, markers, strict=True)):
            end = markers[index + 1].start() if index + 1 < len(markers) else len(report)
            section = report[marker.end() : end]
            status = re.match(r"(?:argv=[^\r\n]*\r?\n)?exit=(-?\d+)(?:\r?\n|$)", section)
            if status is None:
                raise RuntimeError(f"musl dependency report omitted {probe} status")
            probes[probe] = (int(status.group(1)), section[status.end() :])
        if probes["readelf -l"][0] != 0 or probes["file"][0] != 0:
            raise RuntimeError("musl dependency inspection probe failed")
        if "ld-linux" in normalized or "glibc" in normalized:
            raise RuntimeError("musl artifact references a glibc interpreter")
        if re.search(r"(?<!musl[-_.])libc\.so\.6", normalized):
            raise RuntimeError("musl artifact references glibc libc.so.6")
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
                raise RuntimeError("musl readelf evidence has an ambiguous interpreter")
            interpreter = interpreters[0].strip()
            if (
                not interpreter.startswith("/")
                or any(character.isspace() for character in interpreter)
                or not interpreter.rsplit("/", 1)[-1].startswith("ld-musl")
            ):
                raise RuntimeError("musl readelf evidence has a non-musl interpreter path")
        elif re.search(r"\bstatic(?:ally|-pie)?\b", file_report) is None:
            raise RuntimeError(
                "musl file evidence does not corroborate a no-interpreter static binary"
            )
    elif family == "linux" and libc == "glibc":
        if "libc.so.6" not in normalized:
            raise RuntimeError("glibc dependency report omitted libc.so.6")
    elif family == "macos":
        if ".dylib" not in normalized:
            raise RuntimeError("Mach-O dependency report omitted dylibs")
    elif family == "windows":
        if ".dll" not in normalized or "dependencies" not in normalized:
            raise RuntimeError("PE dependency report omitted DLL dependencies")
    else:
        raise ValueError(f"unsupported dependency report family/libc: {family}/{libc}")


def _find_dumpbin() -> Path:
    direct = shutil.which("dumpbin")
    if direct:
        return Path(direct)
    program_files_x86 = os.environ.get("PROGRAMFILES(X86)")
    if not program_files_x86:
        raise RuntimeError("ProgramFiles(x86) is unavailable; cannot locate dumpbin")
    vswhere = Path(program_files_x86) / "Microsoft Visual Studio" / "Installer" / "vswhere.exe"
    if not vswhere.is_file():
        raise RuntimeError(f"cannot locate vswhere at {vswhere}")
    result = _run(
        [
            str(vswhere),
            "-latest",
            "-products",
            "*",
            "-requires",
            "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
            "-property",
            "installationPath",
        ]
    )
    if result.returncode != 0 or not result.stdout.strip():
        raise RuntimeError(f"vswhere could not locate Visual Studio: {result.stderr}")
    tools = Path(result.stdout.strip()) / "VC" / "Tools" / "MSVC"
    matches = sorted(tools.glob("*/bin/Hostx64/x64/dumpbin.exe"), reverse=True)
    if not matches:
        raise RuntimeError(f"cannot locate dumpbin beneath {tools}")
    return matches[0]


def _inspect_dynamic_dependencies(
    *, binary: Path, target: Mapping[str, Any]
) -> tuple[str, str, list[str], str]:
    family = str(target["family"])
    libc = str(target["libc"])
    if family == "linux" and libc == "musl":
        ldd_argv = ["ldd", str(binary)]
        ldd_result = _run(ldd_argv, cwd=binary.parent)
        readelf_argv = ["readelf", "-l", str(binary)]
        readelf_result = _run(readelf_argv, cwd=binary.parent)
        file_result = _run(["file", str(binary)], cwd=binary.parent)
        if readelf_result.returncode != 0:
            raise RuntimeError(f"readelf failed for musl binary: {readelf_result.stderr}")
        if file_result.returncode != 0:
            raise RuntimeError(f"file failed for musl binary: {file_result.stderr}")
        report = (
            "[probe ldd]\n"
            f"argv=ldd {binary}\n"
            f"exit={ldd_result.returncode}\n"
            f"stdout:\n{ldd_result.stdout}\n"
            f"stderr:\n{ldd_result.stderr}\n"
            "[probe readelf -l]\n"
            f"argv=readelf -l {binary}\n"
            f"exit={readelf_result.returncode}\n"
            f"stdout:\n{readelf_result.stdout}\n"
            f"stderr:\n{readelf_result.stderr}\n"
            "[probe file]\n"
            f"argv=file {binary}\n"
            f"exit={file_result.returncode}\n"
            f"stdout:\n{file_result.stdout}\n"
            f"stderr:\n{file_result.stderr}"
        )
        kind = (
            "musl"
            if "requesting program interpreter:" in readelf_result.stdout.lower()
            else "static"
        )
        display_argv = readelf_argv
        tool = "readelf"
    elif family == "linux":
        display_argv = ["ldd", str(binary)]
        result = _run(display_argv, cwd=binary.parent)
        if result.returncode != 0:
            raise RuntimeError(f"ldd failed: {result.stdout}{result.stderr}")
        report = result.stdout + result.stderr
        tool = "ldd"
        kind = "glibc"
    elif family == "macos":
        display_argv = ["otool", "-L", str(binary)]
        result = _run(display_argv, cwd=binary.parent)
        if result.returncode != 0:
            raise RuntimeError(f"otool failed: {result.stdout}{result.stderr}")
        report = result.stdout + result.stderr
        tool = "otool"
        kind = "mach-o"
    elif family == "windows":
        dumpbin = _find_dumpbin()
        actual_argv = [str(dumpbin), "/dependents", str(binary)]
        result = _run(actual_argv, cwd=binary.parent)
        if result.returncode != 0:
            raise RuntimeError(f"dumpbin failed: {result.stdout}{result.stderr}")
        display_argv = ["dumpbin", "/dependents", str(binary)]
        report = result.stdout + result.stderr
        tool = "dumpbin"
        kind = "pe"
    else:
        raise ValueError(f"unsupported target family {family}")
    validate_dynamic_dependency_report(family=family, libc=libc, report=report)
    return tool, kind, display_argv, report


def _verify_package_vcs(archive: Path, candidate_sha: str) -> None:
    try:
        with tarfile.open(archive, mode="r:gz") as package:
            members = [
                member
                for member in package.getmembers()
                if member.name.endswith("/.cargo_vcs_info.json")
            ]
            if len(members) != 1:
                raise RuntimeError(f"{archive.name} has {len(members)} Cargo VCS records")
            extracted = package.extractfile(members[0])
            if extracted is None:
                raise RuntimeError(f"cannot read Cargo VCS record in {archive.name}")
            vcs = json.loads(extracted.read().decode("utf-8"))
    except (tarfile.TarError, json.JSONDecodeError, UnicodeDecodeError) as error:
        raise RuntimeError(f"cannot inspect {archive}: {error}") from error
    git = vcs.get("git", {}) if isinstance(vcs, dict) else {}
    if not isinstance(git, dict):
        raise RuntimeError(f"{archive.name} Cargo VCS git record is invalid")
    if git.get("sha1") != candidate_sha:
        raise RuntimeError(f"{archive.name} is not tied to candidate {candidate_sha}")
    if "dirty" in git and git["dirty"] is not False:
        raise RuntimeError(f"{archive.name} was packaged from a dirty worktree")


def _package_record(*, archive: Path, name: str, version: str) -> dict[str, Any]:
    return {
        "name": name,
        "version": version,
        "filename": f"packages/{archive.name}",
        "sha256": _sha256(archive),
        "size": archive.stat().st_size,
    }


def _workspace_version() -> str:
    import tomllib

    with (REPO_ROOT / "Cargo.toml").open("rb") as handle:
        manifest = tomllib.load(handle)
    version = manifest.get("workspace", {}).get("package", {}).get("version")
    if not isinstance(version, str) or not version:
        raise RuntimeError("workspace package version is missing")
    return version


def _write_json(path: Path, value: Any) -> None:
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def build_native_evidence(
    *,
    declaration_path: Path,
    target_id: str,
    candidate_sha: str,
    candidate_tree: str,
    output_dir: Path,
) -> dict[str, Any]:
    declaration_path = declaration_path.resolve()
    output_dir = output_dir.resolve()
    declaration = _load_json(declaration_path)
    _validate_declaration(declaration)
    target = next((item for item in declaration["targets"] if item["id"] == target_id), None)
    if target is None:
        raise ValueError(f"target {target_id} is not declared")
    if not re.fullmatch(r"[0-9a-f]{40}", candidate_sha):
        raise ValueError("candidate SHA must be a lowercase 40-character Git SHA")
    if not re.fullmatch(r"[0-9a-f]{40}", candidate_tree):
        raise ValueError("candidate tree must be a lowercase 40-character Git object ID")
    if _is_within(output_dir, REPO_ROOT):
        raise ValueError("native evidence output must be outside the repository worktree")
    output_dir.mkdir(parents=True, exist_ok=False)
    packages_dir = output_dir / "packages"
    binary_dir = output_dir / "bin"
    packages_dir.mkdir()
    binary_dir.mkdir()

    actual_sha = _run(["git", "rev-parse", "HEAD"]).stdout.strip()
    actual_tree = _run(["git", "rev-parse", "HEAD^{tree}"]).stdout.strip()
    if actual_sha != candidate_sha or actual_tree != candidate_tree:
        raise RuntimeError(
            f"checkout is {actual_sha}/{actual_tree}, expected {candidate_sha}/{candidate_tree}"
        )

    work_parent = output_dir.parent
    with tempfile.TemporaryDirectory(
        prefix=f"rust-native-{target_id}-", dir=work_parent
    ) as temporary:
        scratch_root = Path(temporary)
        cargo_target = scratch_root / "cargo-target"
        install_root = scratch_root / "install"
        roots = {
            "$REPO": REPO_ROOT,
            "$INSTALL": install_root,
            "$SMOKE": scratch_root,
            "$EVIDENCE": output_dir,
        }
        cargo_env = os.environ.copy()
        cargo_env["CARGO_TARGET_DIR"] = str(cargo_target)
        commands: list[dict[str, Any]] = []

        rustc_verbose = _run_expected(
            name="rustc-verbose",
            argv=["rustc", "-vV"],
            expected_exit=0,
            commands=commands,
            roots=roots,
            env=cargo_env,
        )
        cargo_verbose = _run_expected(
            name="cargo-verbose",
            argv=["cargo", "-vV"],
            expected_exit=0,
            commands=commands,
            roots=roots,
            env=cargo_env,
        )
        toolchain = {
            "rustc": _parse_verbose_version(rustc_verbose.stdout, "rustc"),
            "cargo": _parse_verbose_version(cargo_verbose.stdout, "cargo"),
        }
        for tool, identity in toolchain.items():
            if identity["host"] != target["rust_target"]:
                raise RuntimeError(
                    f"{tool} host {identity['host']} does not match {target['rust_target']}"
                )
        environment = _observed_environment(target)

        target_args = [
            "--target",
            target["rust_target"],
            "--target-dir",
            str(cargo_target),
        ]
        _run_expected(
            name="release-test",
            argv=[*CARGO_TEST_COMMAND, *target_args],
            expected_exit=0,
            commands=commands,
            roots=roots,
            env=cargo_env,
        )
        _run_expected(
            name="release-build",
            argv=[*CARGO_BUILD_COMMAND, *target_args],
            expected_exit=0,
            commands=commands,
            roots=roots,
            env=cargo_env,
        )

        package_names = ("ferric-rules", "ferric-rules-cli")
        package_command_names = ("package-facade", "package-cli")
        for record_name, package_command in zip(
            package_command_names, CARGO_PACKAGE_COMMANDS, strict=True
        ):
            _run_expected(
                name=record_name,
                argv=[*package_command, *target_args],
                expected_exit=0,
                commands=commands,
                roots=roots,
                env=cargo_env,
            )

        install_argv = [
            "cargo",
            "install",
            "--path",
            str(REPO_ROOT / "crates" / "ferric-rules-cli"),
            "--root",
            str(install_root),
            "--all-features",
            "--locked",
            *target_args,
        ]
        _run_expected(
            name="install-cli",
            argv=install_argv,
            expected_exit=0,
            commands=commands,
            roots=roots,
            env=cargo_env,
        )

        executable_name = "ferric.exe" if target["family"] == "windows" else "ferric"
        installed_binary = install_root / "bin" / executable_name
        if (
            not installed_binary.is_file()
            or installed_binary.is_symlink()
            or installed_binary.stat().st_size == 0
        ):
            raise RuntimeError(f"cargo install did not produce {installed_binary}")

        commands.extend(run_cli_smokes(binary=installed_binary, scratch_root=scratch_root))

        dynamic_tool, dynamic_kind, dynamic_argv, report = _inspect_dynamic_dependencies(
            binary=installed_binary, target=target
        )
        commands.append(
            _command_record(
                name="dynamic-dependencies",
                argv=dynamic_argv,
                expected_exit=0,
                actual_exit=0,
                roots=roots,
            )
        )
        if tuple(command["name"] for command in commands) != MANDATORY_COMMAND_NAMES:
            raise RuntimeError("native command evidence is incomplete or out of order")

        output_binary = binary_dir / executable_name
        shutil.copy2(installed_binary, output_binary)
        report_path = output_dir / "dynamic-dependencies.txt"
        report_path.write_text(report.rstrip() + "\n", encoding="utf-8")

        version = _workspace_version()
        package_records: list[dict[str, Any]] = []
        for package_name in package_names:
            archive = cargo_target / "package" / f"{package_name}-{version}.crate"
            if not archive.is_file():
                raise RuntimeError(f"cargo package did not produce {archive}")
            _verify_package_vcs(archive, candidate_sha)
            destination = packages_dir / archive.name
            shutil.copy2(archive, destination)
            package_records.append(
                _package_record(archive=destination, name=package_name, version=version)
            )

        receipt = {
            "schema_version": 1,
            "candidate": {"sha": candidate_sha, "tree": candidate_tree},
            "artifact_contract": declaration["artifact_contract"],
            "target": target,
            "declaration_sha256": _sha256(declaration_path),
            "toolchain": toolchain,
            "environment": environment,
            "commands": commands,
            "packages": package_records,
            "installed_binary": {
                "filename": f"bin/{output_binary.name}",
                "sha256": _sha256(output_binary),
                "size": output_binary.stat().st_size,
            },
            "smokes": [{"name": name, "status": "passed"} for name in SMOKE_NAMES],
            "dynamic_dependencies": {
                "tool": dynamic_tool,
                "kind": dynamic_kind,
                "filename": report_path.name,
                "sha256": _sha256(report_path),
            },
        }
        _write_json(output_dir / "receipt.json", receipt)
        return receipt


def _parse_args(argv: Sequence[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--declaration", type=Path, required=True)
    parser.add_argument("--target-id", required=True)
    parser.add_argument("--candidate-sha", required=True)
    parser.add_argument("--candidate-tree", required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    args = _parse_args(sys.argv[1:] if argv is None else argv)
    receipt = build_native_evidence(
        declaration_path=args.declaration,
        target_id=args.target_id,
        candidate_sha=args.candidate_sha,
        candidate_tree=args.candidate_tree,
        output_dir=args.output_dir,
    )
    print(
        f"native Rust evidence passed for {receipt['target']['id']} "
        f"at {receipt['candidate']['sha']}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
