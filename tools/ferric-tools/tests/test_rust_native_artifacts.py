"""Adversarial tests for native Rust dependency and evidence verification."""

import copy
import hashlib
import importlib.util
import io
import json
import pathlib
import shutil
import sys
import tarfile

import pytest

REPO_ROOT = pathlib.Path(__file__).resolve().parents[3]
HARNESS = REPO_ROOT / "scripts" / "test-rust-native-artifact.py"
VERIFIER = REPO_ROOT / "scripts" / "verify-rust-native-artifacts.py"

CANDIDATE_SHA = "1" * 40
CANDIDATE_TREE = "2" * 40

TARGETS = [
    {
        "id": "linux-x86_64-gnu",
        "runner": "ubuntu-24.04",
        "rust_target": "x86_64-unknown-linux-gnu",
        "family": "linux",
        "architecture": "x86_64",
        "libc": "glibc",
        "native_environment": "github-hosted runner",
        "conformance": "native",
    },
    {
        "id": "linux-aarch64-gnu",
        "runner": "ubuntu-24.04-arm",
        "rust_target": "aarch64-unknown-linux-gnu",
        "family": "linux",
        "architecture": "aarch64",
        "libc": "glibc",
        "native_environment": "github-hosted runner",
        "conformance": "native",
    },
    {
        "id": "linux-x86_64-musl",
        "runner": "ubuntu-24.04",
        "rust_target": "x86_64-unknown-linux-musl",
        "family": "linux",
        "architecture": "x86_64",
        "libc": "musl",
        "native_environment": "matching-architecture Alpine container",
        "conformance": "native",
    },
    {
        "id": "linux-aarch64-musl",
        "runner": "ubuntu-24.04-arm",
        "rust_target": "aarch64-unknown-linux-musl",
        "family": "linux",
        "architecture": "aarch64",
        "libc": "musl",
        "native_environment": "matching-architecture Alpine container",
        "conformance": "native",
    },
    {
        "id": "macos-x86_64",
        "runner": "macos-15-intel",
        "rust_target": "x86_64-apple-darwin",
        "family": "macos",
        "architecture": "x86_64",
        "libc": "none",
        "native_environment": "github-hosted runner",
        "conformance": "native",
    },
    {
        "id": "macos-aarch64",
        "runner": "macos-15",
        "rust_target": "aarch64-apple-darwin",
        "family": "macos",
        "architecture": "aarch64",
        "libc": "none",
        "native_environment": "github-hosted runner",
        "conformance": "native",
    },
    {
        "id": "windows-x86_64-msvc",
        "runner": "windows-2025",
        "rust_target": "x86_64-pc-windows-msvc",
        "family": "windows",
        "architecture": "x86_64",
        "libc": "msvc",
        "native_environment": "github-hosted runner",
        "conformance": "native",
    },
]

ARTIFACT_CONTRACT = {
    "packages": ["ferric-rules", "ferric-rules-cli"],
    "binary": "ferric",
    "install_all_features": True,
    "execution": "native",
    "distribution": "ci-evidence-only",
}

SMOKE_NAMES = [
    "outside-worktree-install",
    "version",
    "unicode-path",
    "crlf-source",
    "snapshot-restore",
    "repl-eof",
    "exit-codes",
    "dynamic-dependencies",
]

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

MUSL_STATIC_REPORT = """[probe ldd]
exit=1
statically linked
[probe readelf -l]
exit=0
Elf file type is EXEC
Program Headers:
  LOAD
[probe file]
exit=0
ELF 64-bit executable, statically linked
"""

MUSL_DYNAMIC_REPORT = """[probe ldd]
exit=0
/lib/ld-musl-x86_64.so.1 (0x1)
[probe readelf -l]
exit=0
Program Headers:
  INTERP
      [Requesting program interpreter: /lib/ld-musl-x86_64.so.1]
[probe file]
exit=0
ELF 64-bit executable, dynamically linked
"""

MUSL_GLIBC_REPORT = """[probe ldd]
exit=0
/lib64/ld-linux-x86-64.so.2
libc.so.6 => /lib/libc.so.6 (0x1)
[probe readelf -l]
exit=0
Program Headers:
  INTERP
      [Requesting program interpreter: /lib64/ld-linux-x86-64.so.2]
[probe file]
exit=0
ELF 64-bit executable, dynamically linked
"""

MUSL_AMBIGUOUS_REPORT = """[probe ldd]
exit=0
[probe readelf -l]
exit=0
Program Headers:
  INTERP
[probe file]
exit=0
ELF 64-bit executable, dynamically linked
"""

MUSL_CONTRADICTORY_REPORT = """[probe ldd]
exit=1
statically linked
[probe readelf -l]
exit=0
Program Headers:
  INTERP
[probe file]
exit=0
ELF 64-bit executable, statically linked
"""

MUSL_FALSE_STATIC_REPORT = """[probe ldd]
exit=1
statically linked
[probe readelf -l]
exit=0
Program Headers:
  LOAD
[probe file]
exit=0
ELF 64-bit executable, dynamically linked
"""

MUSL_NONPATH_INTERPRETER_REPORT = """[probe ldd]
exit=0
ld-musl
[probe readelf -l]
exit=0
Program Headers:
  INTERP
      [Requesting program interpreter: ld-musl]
[probe file]
exit=0
ELF 64-bit executable, dynamically linked
"""

FAKE_FERRIC = r"""#!/usr/bin/env python3
import json
import pathlib
import re
import sys

binary = pathlib.Path(__file__)
args = sys.argv[1:]
stdin = sys.stdin.buffer.read() if args and args[0] == "repl" else b""
source = next((pathlib.Path(arg) for arg in args if arg.endswith(".clp")), None)
record = {"argv": args, "cwd": str(pathlib.Path.cwd()), "stdin_size": len(stdin)}
if source is not None and source.exists():
    content = source.read_bytes()
    record.update(
        source=str(source),
        source_has_crlf=b"\r\n" in content,
        source_has_unicode=any(ord(char) > 127 for char in str(source)),
        source_has_space=" " in str(source),
    )
with binary.with_name("fake-ferric-calls.jsonl").open("a", encoding="utf-8") as log:
    log.write(json.dumps(record) + "\n")

if not args:
    print("Usage: ferric <COMMAND>", file=sys.stderr)
    raise SystemExit(2)
if args in (["version"], ["--version"]):
    print("ferric 0.1.0")
    raise SystemExit(0)
if args == ["--help"]:
    print("Usage: ferric [--trace PATH] <COMMAND>\nCommands: snapshot")
    raise SystemExit(0)
if args[0] in {"check", "run", "snapshot"}:
    if source is None or not source.exists():
        raise SystemExit(1)
    if "invalid" in source.name:
        print(
            json.dumps(
                {
                    "command": args[0],
                    "level": "error",
                    "kind": "load_error",
                    "message": "invalid fixture",
                }
            ),
            file=sys.stderr,
        )
        raise SystemExit(1)
    path_checks = ["source_has_crlf", "source_has_unicode", "source_has_space"]
    if not all(record[key] for key in path_checks):
        raise SystemExit(90)
    if args[0] == "run":
        text = source.read_text(encoding="utf-8")
        match = re.search(r'message\s+"([^"]+)"', text)
        print(match.group(1) if match else "Ferric Unicode ✓")
    if args[0] == "snapshot":
        option = "--output" if "--output" in args else "-o"
        output = pathlib.Path(args[args.index(option) + 1])
        output.write_bytes(b"fake snapshot")
    raise SystemExit(0)
if args[0] == "repl":
    raise SystemExit(0)
raise SystemExit(2)
"""


def _load_script(path: pathlib.Path, name: str):
    assert path.is_file(), f"missing required native Rust script: {path}"
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


def _sha256(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _write_json(path: pathlib.Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def _write_crate(
    path: pathlib.Path,
    package: str,
    *,
    candidate_sha: str = CANDIDATE_SHA,
    dirty: object = False,
    include_dirty: bool = True,
) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    root = f"{package}-0.1.0"
    git: dict[str, object] = {"sha1": candidate_sha}
    if include_dirty:
        git["dirty"] = dirty
    vcs = json.dumps(
        {"git": git, "path_in_vcs": ""},
        sort_keys=True,
    ).encode()
    manifest = f'[package]\nname = "{package}"\nversion = "0.1.0"\n'.encode()
    with tarfile.open(path, mode="w:gz") as archive:
        for name, content in [
            (f"{root}/.cargo_vcs_info.json", vcs),
            (f"{root}/Cargo.toml", manifest),
        ]:
            member = tarfile.TarInfo(name)
            member.size = len(content)
            member.mtime = 0
            member.mode = 0o644
            archive.addfile(member, io.BytesIO(content))


def _declaration(tmp_path: pathlib.Path) -> pathlib.Path:
    path = tmp_path / "release-targets.json"
    _write_json(
        path,
        {
            "schema_version": 1,
            "artifact_contract": ARTIFACT_CONTRACT,
            "targets": TARGETS,
        },
    )
    return path


def _dependency_kind(target: dict[str, str]) -> tuple[str, str, str]:
    if target["family"] == "macos":
        return "otool", "mach-o", "ferric:\n\t/usr/lib/libSystem.B.dylib"
    if target["family"] == "windows":
        return "dumpbin", "pe", "Image has the following dependencies:\nKERNEL32.dll"
    if target["libc"] == "musl":
        return "readelf", "static", MUSL_STATIC_REPORT.rstrip()
    return "ldd", "glibc", "libc.so.6 => /lib/libc.so.6 (0x1)"


def _command_entries(
    binary: str, dependency_tool: str, rust_target: str
) -> list[dict[str, object]]:
    installed_binary = f"$INSTALL/bin/{binary}"
    smoke_root = "$SMOKE/outside-worktree Unicode space Ω"
    source = f"{smoke_root}/规则 unicode CRLF.clp"
    invalid = f"{smoke_root}/invalid source.clp"
    snapshot = f"{smoke_root}/snapshot state.json"
    target_args = ["--target", rust_target, "--target-dir", "$SMOKE/cargo-target"]
    dependency_argv = {
        "ldd": ["ldd", installed_binary],
        "readelf": ["readelf", "-l", installed_binary],
        "otool": ["otool", "-L", installed_binary],
        "dumpbin": ["dumpbin", "/dependents", installed_binary],
    }[dependency_tool]
    commands = {
        "rustc-verbose": ["rustc", "-vV"],
        "cargo-verbose": ["cargo", "-vV"],
        "release-test": [
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
        "release-build": [
            "cargo",
            "build",
            "--release",
            "-p",
            "ferric-rules-cli",
            "--all-features",
            "--locked",
            *target_args,
        ],
        "package-facade": [
            "cargo",
            "package",
            "-p",
            "ferric-rules",
            "--all-features",
            "--locked",
            *target_args,
        ],
        "package-cli": [
            "cargo",
            "package",
            "-p",
            "ferric-rules-cli",
            "--all-features",
            "--locked",
            *target_args,
        ],
        "install-cli": [
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
        "cli-version-command": [installed_binary, "version"],
        "cli-version-flag": [installed_binary, "--version"],
        "cli-help": [installed_binary, "--help"],
        "cli-check-crlf-unicode": [installed_binary, "check", source],
        "cli-run-crlf-unicode": [installed_binary, "run", source],
        "cli-snapshot": [
            installed_binary,
            "snapshot",
            "--json",
            source,
            "--output",
            snapshot,
            "--format",
            "json",
        ],
        "cli-snapshot-repl-eof": [
            installed_binary,
            "repl",
            "--snapshot",
            snapshot,
            "--snapshot-format",
            "json",
        ],
        "cli-invalid-source": [installed_binary, "check", "--json", invalid],
        "cli-usage-error": [installed_binary],
        "dynamic-dependencies": dependency_argv,
    }
    expected_exits = {"cli-invalid-source": 1, "cli-usage-error": 2}
    return [
        {
            "name": name,
            "argv": commands[name],
            "expected_exit": expected_exits.get(name, 0),
            "actual_exit": expected_exits.get(name, 0),
        }
        for name in MANDATORY_COMMAND_NAMES
    ]


def _artifact_set(tmp_path: pathlib.Path) -> tuple[pathlib.Path, pathlib.Path]:
    declaration = _declaration(tmp_path)
    declaration_sha256 = _sha256(declaration)
    artifacts = tmp_path / "artifacts"

    for target in TARGETS:
        root = artifacts / f"rust-native-{target['id']}-{CANDIDATE_SHA}"
        binary_name = "ferric.exe" if target["family"] == "windows" else "ferric"
        binary = root / "bin" / binary_name
        binary.parent.mkdir(parents=True, exist_ok=True)
        binary.write_bytes(f"binary:{target['id']}".encode())

        packages = []
        for package in ARTIFACT_CONTRACT["packages"]:
            archive = root / "packages" / f"{package}-0.1.0.crate"
            _write_crate(archive, package)
            packages.append(
                {
                    "name": package,
                    "version": "0.1.0",
                    "filename": archive.relative_to(root).as_posix(),
                    "sha256": _sha256(archive),
                    "size": archive.stat().st_size,
                }
            )

        tool, kind, report_text = _dependency_kind(target)
        report = root / "dynamic-dependencies.txt"
        report.write_text(report_text + "\n", encoding="utf-8")
        receipt = {
            "schema_version": 1,
            "candidate": {"sha": CANDIDATE_SHA, "tree": CANDIDATE_TREE},
            "artifact_contract": ARTIFACT_CONTRACT,
            "target": target,
            "declaration_sha256": declaration_sha256,
            "toolchain": {
                "rustc": {
                    "release": "1.93.0",
                    "host": target["rust_target"],
                    "commit_hash": "3" * 40,
                },
                "cargo": {
                    "release": "1.93.0",
                    "host": target["rust_target"],
                    "commit_hash": "4" * 40,
                },
            },
            "environment": {
                "family": target["family"],
                "architecture": target["architecture"],
                "libc": target["libc"],
                "libc_version": (
                    "2.39"
                    if target["libc"] == "glibc"
                    else "1.2.5"
                    if target["libc"] == "musl"
                    else None
                ),
                "platform": target["native_environment"],
            },
            "commands": _command_entries(binary.name, tool, target["rust_target"]),
            "packages": packages,
            "installed_binary": {
                "filename": binary.relative_to(root).as_posix(),
                "sha256": _sha256(binary),
                "size": binary.stat().st_size,
            },
            "smokes": [{"name": name, "status": "passed"} for name in SMOKE_NAMES],
            "dynamic_dependencies": {
                "tool": tool,
                "kind": kind,
                "filename": report.name,
                "sha256": _sha256(report),
            },
        }
        _write_json(root / "receipt.json", receipt)

    return declaration, artifacts


@pytest.mark.parametrize(
    "report",
    [
        MUSL_STATIC_REPORT,
        MUSL_DYNAMIC_REPORT,
    ],
)
def test_musl_dependency_audit_accepts_only_static_or_musl_runtime(report: str):
    harness = _load_script(HARNESS, "rust_native_harness_accept")
    harness.validate_dynamic_dependency_report(family="linux", libc="musl", report=report)


@pytest.mark.parametrize(
    "report",
    [
        MUSL_GLIBC_REPORT,
        MUSL_STATIC_REPORT.replace("statically linked", "not found", 1),
        MUSL_CONTRADICTORY_REPORT,
        MUSL_FALSE_STATIC_REPORT,
        MUSL_NONPATH_INTERPRETER_REPORT,
        "statically linked\n",
        "",
    ],
)
def test_musl_dependency_audit_rejects_glibc_missing_or_vacuous_reports(report: str):
    harness = _load_script(HARNESS, "rust_native_harness_reject")
    with pytest.raises((RuntimeError, ValueError)):
        harness.validate_dynamic_dependency_report(family="linux", libc="musl", report=report)


def test_dependency_audit_rejects_missing_library_on_every_dynamic_platform():
    harness = _load_script(HARNESS, "rust_native_harness_missing")
    for family, libc in [("linux", "glibc"), ("macos", "none"), ("windows", "msvc")]:
        with pytest.raises((RuntimeError, ValueError)):
            harness.validate_dynamic_dependency_report(
                family=family,
                libc=libc,
                report="required-library => not found\n",
            )


def test_harness_decodes_subprocess_evidence_as_utf8(monkeypatch: pytest.MonkeyPatch):
    harness = _load_script(HARNESS, "rust_native_harness_utf8_subprocess")
    captured: dict[str, object] = {}

    def fake_run(*_args, **kwargs):
        captured.update(kwargs)
        return object()

    monkeypatch.setattr(harness.subprocess, "run", fake_run)

    harness._run(["unicode-output-probe"])

    assert captured["encoding"] == "utf-8"


def test_cli_smoke_harness_really_exercises_unicode_crlf_snapshot_and_exit_codes(
    tmp_path: pathlib.Path,
):
    if sys.platform == "win32":
        pytest.skip("the fake executable seam is exercised by the portable Python-tools job")
    harness = _load_script(HARNESS, "rust_native_harness_cli_smoke")
    binary = tmp_path / "install" / "bin" / "ferric"
    binary.parent.mkdir(parents=True)
    binary.write_text(FAKE_FERRIC, encoding="utf-8")
    binary.chmod(0o755)
    scratch = tmp_path / "outside repository scratch"

    commands_evidence = harness.run_cli_smokes(binary=binary, scratch_root=scratch)

    assert commands_evidence == _command_entries(binary.name, "ldd", "unused")[7:16]
    assert all(command["expected_exit"] == command["actual_exit"] for command in commands_evidence)
    records = [
        json.loads(line)
        for line in binary.with_name("fake-ferric-calls.jsonl").read_text().splitlines()
    ]
    commands = [record["argv"] for record in records]
    assert ["version"] in commands
    assert ["--version"] in commands
    assert ["--help"] in commands
    assert [] in commands
    assert any(command and command[0] == "snapshot" for command in commands)
    assert any(command and command[0] == "repl" and "--snapshot" in command for command in commands)
    source_records = [record for record in records if "source" in record]
    assert source_records
    assert all(record["source_has_crlf"] for record in source_records)
    assert all(record["source_has_unicode"] for record in source_records)
    assert all(record["source_has_space"] for record in source_records)


@pytest.mark.parametrize(
    ("replacement", "error"),
    [
        ('print("ferric 9.9.9")', "version"),
        (
            'print((match.group(1) if match else "Ferric Unicode ✓") + " extra")',
            "Unicode",
        ),
    ],
)
def test_cli_smoke_harness_rejects_inexact_version_or_unicode_output(
    tmp_path: pathlib.Path, replacement: str, error: str
):
    if sys.platform == "win32":
        pytest.skip("the fake executable seam is exercised by the portable Python-tools job")
    harness = _load_script(HARNESS, f"rust_native_harness_cli_inexact_{error.lower()}")
    binary = tmp_path / "install" / "bin" / "ferric"
    binary.parent.mkdir(parents=True)
    if error == "version":
        fake = FAKE_FERRIC.replace('print("ferric 0.1.0")', replacement)
    else:
        fake = FAKE_FERRIC.replace(
            'print(match.group(1) if match else "Ferric Unicode ✓")', replacement
        )
    binary.write_text(fake, encoding="utf-8")
    binary.chmod(0o755)

    with pytest.raises(RuntimeError, match=error):
        harness.run_cli_smokes(
            binary=binary,
            scratch_root=tmp_path / "outside repository scratch",
        )


def test_verifier_accepts_and_retains_one_exact_seven_target_bundle(tmp_path: pathlib.Path):
    verifier = _load_script(VERIFIER, "rust_native_verifier_valid")
    declaration, artifacts = _artifact_set(tmp_path)
    output = tmp_path / "verified"

    manifest = verifier.verify_artifact_set(
        declaration_path=declaration,
        artifacts_dir=artifacts,
        candidate_sha=CANDIDATE_SHA,
        candidate_tree=CANDIDATE_TREE,
        output_dir=output,
    )

    assert manifest["candidate"] == {"sha": CANDIDATE_SHA, "tree": CANDIDATE_TREE}
    assert len(manifest["targets"]) == 7
    assert (output / "manifest.json").is_file()
    assert len(list(output.rglob("receipt.json"))) == 7
    assert len(list(output.rglob("*.crate"))) == 14
    assert len(list(output.rglob("dynamic-dependencies.txt"))) == 7
    assert len(list(output.rglob("bin/ferric"))) == 6
    assert len(list(output.rglob("bin/ferric.exe"))) == 1


def _receipt_for(artifacts: pathlib.Path, target_id: str) -> pathlib.Path:
    matches = list(artifacts.rglob(f"rust-native-{target_id}-*/receipt.json"))
    assert len(matches) == 1
    return matches[0]


@pytest.mark.parametrize(
    "mutation",
    [
        "candidate-sha",
        "candidate-tree",
        "declaration-digest",
        "target-declaration",
        "package-digest",
        "binary-digest",
        "empty-binary-rehashed",
        "dependency-digest",
        "dependency-tool",
        "dependency-kind",
        "dependency-filename",
        "failed-smoke",
        "missing-smoke",
        "extra-receipt-field",
        "unexpected-package",
        "path-traversal",
        "missing-toolchain",
        "missing-environment",
        "missing-commands",
        "extra-candidate-field",
        "extra-toolchain-field",
        "extra-tool-field",
        "extra-environment-field",
        "extra-package-field",
        "extra-binary-field",
        "extra-smoke-field",
        "extra-dynamic-field",
        "rustc-release",
        "cargo-release",
        "toolchain-host",
        "cargo-host",
        "environment-family",
        "environment-architecture",
        "environment-libc",
        "missing-command",
        "command-order",
        "command-exit",
        "command-expected",
        "command-schema",
        "command-argv",
        "command-root-leak",
    ],
)
def test_verifier_rejects_tampered_receipts(tmp_path: pathlib.Path, mutation: str):
    verifier = _load_script(VERIFIER, f"rust_native_verifier_{mutation.replace('-', '_')}")
    declaration, artifacts = _artifact_set(tmp_path)
    receipt_path = _receipt_for(artifacts, "linux-x86_64-gnu")
    receipt = json.loads(receipt_path.read_text(encoding="utf-8"))

    if mutation == "candidate-sha":
        receipt["candidate"]["sha"] = "3" * 40
    elif mutation == "candidate-tree":
        receipt["candidate"]["tree"] = "3" * 40
    elif mutation == "declaration-digest":
        receipt["declaration_sha256"] = "3" * 64
    elif mutation == "target-declaration":
        receipt["target"]["runner"] = "ubuntu-latest"
    elif mutation == "package-digest":
        receipt["packages"][0]["sha256"] = "3" * 64
    elif mutation == "binary-digest":
        receipt["installed_binary"]["sha256"] = "3" * 64
    elif mutation == "empty-binary-rehashed":
        binary = receipt_path.parent / receipt["installed_binary"]["filename"]
        binary.write_bytes(b"")
        receipt["installed_binary"]["sha256"] = _sha256(binary)
        receipt["installed_binary"]["size"] = 0
    elif mutation == "dependency-digest":
        receipt["dynamic_dependencies"]["sha256"] = "3" * 64
    elif mutation == "dependency-tool":
        receipt["dynamic_dependencies"]["tool"] = "cat"
    elif mutation == "dependency-kind":
        receipt["dynamic_dependencies"]["kind"] = "static"
    elif mutation == "dependency-filename":
        receipt["dynamic_dependencies"]["filename"] = "../dynamic-dependencies.txt"
    elif mutation == "failed-smoke":
        receipt["smokes"][0]["status"] = "failed"
    elif mutation == "missing-smoke":
        receipt["smokes"].pop()
    elif mutation == "extra-receipt-field":
        receipt["unverified"] = True
    elif mutation == "unexpected-package":
        receipt["packages"][0]["name"] = "other-package"
    elif mutation == "path-traversal":
        receipt["installed_binary"]["filename"] = "../ferric"
    elif mutation == "missing-toolchain":
        del receipt["toolchain"]
    elif mutation == "missing-environment":
        del receipt["environment"]
    elif mutation == "missing-commands":
        del receipt["commands"]
    elif mutation == "extra-candidate-field":
        receipt["candidate"]["ref"] = "refs/heads/main"
    elif mutation == "extra-toolchain-field":
        receipt["toolchain"]["channel"] = "stable"
    elif mutation == "extra-tool-field":
        receipt["toolchain"]["rustc"]["verbose"] = True
    elif mutation == "extra-environment-field":
        receipt["environment"]["runner"] = "ubuntu-latest"
    elif mutation == "extra-package-field":
        receipt["packages"][0]["unchecked"] = True
    elif mutation == "extra-binary-field":
        receipt["installed_binary"]["unchecked"] = True
    elif mutation == "extra-smoke-field":
        receipt["smokes"][0]["detail"] = "unchecked"
    elif mutation == "extra-dynamic-field":
        receipt["dynamic_dependencies"]["unchecked"] = True
    elif mutation == "rustc-release":
        receipt["toolchain"]["rustc"]["release"] = "1.92.0"
    elif mutation == "cargo-release":
        receipt["toolchain"]["cargo"]["release"] = "1.92.0"
    elif mutation == "toolchain-host":
        receipt["toolchain"]["rustc"]["host"] = "x86_64-pc-windows-msvc"
    elif mutation == "cargo-host":
        receipt["toolchain"]["cargo"]["host"] = "x86_64-pc-windows-msvc"
    elif mutation == "environment-family":
        receipt["environment"]["family"] = "windows"
    elif mutation == "environment-architecture":
        receipt["environment"]["architecture"] = "aarch64"
    elif mutation == "environment-libc":
        receipt["environment"]["libc"] = "musl"
    elif mutation == "missing-command":
        receipt["commands"].pop()
    elif mutation == "command-order":
        receipt["commands"][0], receipt["commands"][1] = (
            receipt["commands"][1],
            receipt["commands"][0],
        )
    elif mutation == "command-exit":
        receipt["commands"][0]["actual_exit"] = 1
    elif mutation == "command-expected":
        receipt["commands"][0]["expected_exit"] = 99
        receipt["commands"][0]["actual_exit"] = 99
    elif mutation == "command-schema":
        receipt["commands"][0]["ignored"] = True
    elif mutation == "command-argv":
        receipt["commands"][8]["argv"][-1] = "--help"
    elif mutation == "command-root-leak":
        receipt["commands"][7]["argv"][0] = "/tmp/install/bin/ferric"
    else:
        raise AssertionError(f"unknown mutation {mutation}")
    _write_json(receipt_path, receipt)

    with pytest.raises((RuntimeError, ValueError)):
        verifier.verify_artifact_set(
            declaration_path=declaration,
            artifacts_dir=artifacts,
            candidate_sha=CANDIDATE_SHA,
            candidate_tree=CANDIDATE_TREE,
            output_dir=tmp_path / "verified",
        )


@pytest.mark.parametrize(
    ("target_id", "report"),
    [
        ("linux-x86_64-gnu", ""),
        ("linux-x86_64-gnu", "libmissing.so => not found\n"),
        (
            "linux-x86_64-musl",
            MUSL_GLIBC_REPORT,
        ),
        (
            "linux-x86_64-musl",
            MUSL_AMBIGUOUS_REPORT,
        ),
        (
            "linux-x86_64-musl",
            MUSL_CONTRADICTORY_REPORT,
        ),
        (
            "linux-x86_64-musl",
            MUSL_FALSE_STATIC_REPORT,
        ),
        (
            "linux-x86_64-musl",
            MUSL_NONPATH_INTERPRETER_REPORT,
        ),
        (
            "linux-x86_64-gnu",
            f"{REPO_ROOT}/target/release/ferric\nlibc.so.6 => /lib/libc.so.6 (0x1)\n",
        ),
    ],
)
def test_verifier_revalidates_rehashed_dependency_report_semantics(
    tmp_path: pathlib.Path, target_id: str, report: str
):
    verifier = _load_script(
        VERIFIER,
        f"rust_native_verifier_dependency_semantics_{target_id.replace('-', '_')}",
    )
    declaration, artifacts = _artifact_set(tmp_path)
    receipt_path = _receipt_for(artifacts, target_id)
    receipt = json.loads(receipt_path.read_text(encoding="utf-8"))
    report_path = receipt_path.parent / receipt["dynamic_dependencies"]["filename"]
    report_path.write_text(report, encoding="utf-8")
    receipt["dynamic_dependencies"]["sha256"] = _sha256(report_path)
    _write_json(receipt_path, receipt)

    with pytest.raises((RuntimeError, ValueError)):
        verifier.verify_artifact_set(
            declaration_path=declaration,
            artifacts_dir=artifacts,
            candidate_sha=CANDIDATE_SHA,
            candidate_tree=CANDIDATE_TREE,
            output_dir=tmp_path / "verified",
        )


@pytest.mark.parametrize(
    "mutation",
    ["candidate-sha", "dirty-true", "dirty-null", "dirty-string", "dirty-number"],
)
def test_verifier_revalidates_rehashed_package_vcs_provenance(
    tmp_path: pathlib.Path, mutation: str
):
    verifier = _load_script(VERIFIER, f"rust_native_verifier_package_vcs_{mutation}")
    declaration, artifacts = _artifact_set(tmp_path)
    receipt_path = _receipt_for(artifacts, "linux-x86_64-gnu")
    receipt = json.loads(receipt_path.read_text(encoding="utf-8"))
    package = receipt["packages"][0]
    archive = receipt_path.parent / package["filename"]
    dirty = {
        "candidate-sha": False,
        "dirty-true": True,
        "dirty-null": None,
        "dirty-string": "false",
        "dirty-number": 0,
    }[mutation]
    _write_crate(
        archive,
        package["name"],
        candidate_sha="f" * 40 if mutation == "candidate-sha" else CANDIDATE_SHA,
        dirty=dirty,
    )
    package["sha256"] = _sha256(archive)
    package["size"] = archive.stat().st_size
    _write_json(receipt_path, receipt)

    with pytest.raises((RuntimeError, ValueError)):
        verifier.verify_artifact_set(
            declaration_path=declaration,
            artifacts_dir=artifacts,
            candidate_sha=CANDIDATE_SHA,
            candidate_tree=CANDIDATE_TREE,
            output_dir=tmp_path / "verified",
        )


@pytest.mark.parametrize(
    ("mutation", "version"),
    [
        ("missing", None),
        ("null", None),
        ("empty", ""),
        ("generic", "musl"),
        ("old", "1.1.24"),
        ("new", "1.3.0"),
        ("unrelated", "9.9.9"),
    ],
)
def test_verifier_requires_observed_musl_1_2_version(
    tmp_path: pathlib.Path, mutation: str, version: object
):
    verifier = _load_script(VERIFIER, f"rust_native_verifier_musl_version_{mutation}")
    declaration, artifacts = _artifact_set(tmp_path)
    receipt_path = _receipt_for(artifacts, "linux-x86_64-musl")
    receipt = json.loads(receipt_path.read_text(encoding="utf-8"))
    if mutation == "missing":
        del receipt["environment"]["libc_version"]
    else:
        receipt["environment"]["libc_version"] = version
    _write_json(receipt_path, receipt)

    with pytest.raises((RuntimeError, ValueError)):
        verifier.verify_artifact_set(
            declaration_path=declaration,
            artifacts_dir=artifacts,
            candidate_sha=CANDIDATE_SHA,
            candidate_tree=CANDIDATE_TREE,
            output_dir=tmp_path / "verified",
        )


def test_verifier_accepts_cargo_clean_vcs_record_with_omitted_dirty(
    tmp_path: pathlib.Path,
):
    verifier = _load_script(VERIFIER, "rust_native_verifier_cargo_omitted_dirty")
    declaration, artifacts = _artifact_set(tmp_path)
    receipt_path = _receipt_for(artifacts, "linux-x86_64-gnu")
    receipt = json.loads(receipt_path.read_text(encoding="utf-8"))
    package = receipt["packages"][0]
    archive = receipt_path.parent / package["filename"]
    _write_crate(archive, package["name"], include_dirty=False)
    package["sha256"] = _sha256(archive)
    package["size"] = archive.stat().st_size
    _write_json(receipt_path, receipt)

    verifier.verify_artifact_set(
        declaration_path=declaration,
        artifacts_dir=artifacts,
        candidate_sha=CANDIDATE_SHA,
        candidate_tree=CANDIDATE_TREE,
        output_dir=tmp_path / "verified",
    )


@pytest.mark.parametrize("unexpected", ["file", "directory", "symlink"])
def test_verifier_rejects_unexpected_or_linked_artifact_entries(
    tmp_path: pathlib.Path, unexpected: str
):
    verifier = _load_script(VERIFIER, f"rust_native_verifier_layout_{unexpected}")
    declaration, artifacts = _artifact_set(tmp_path)
    receipt = _receipt_for(artifacts, "linux-x86_64-gnu")
    root = receipt.parent

    if unexpected == "file":
        (root / "unverified.txt").write_text("not in receipt", encoding="utf-8")
    elif unexpected == "directory":
        (root / "unverified").mkdir()
    elif unexpected == "symlink":
        link = root / "binary-link"
        try:
            link.symlink_to(root / "bin" / "ferric")
        except OSError as error:
            pytest.skip(f"symlink unavailable: {error}")
    else:
        raise AssertionError(f"unknown unexpected entry {unexpected}")

    with pytest.raises((RuntimeError, ValueError)):
        verifier.verify_artifact_set(
            declaration_path=declaration,
            artifacts_dir=artifacts,
            candidate_sha=CANDIDATE_SHA,
            candidate_tree=CANDIDATE_TREE,
            output_dir=tmp_path / "verified",
        )


@pytest.mark.parametrize("mutation", ["missing", "duplicate", "extra"])
def test_verifier_rejects_inexact_target_coverage(tmp_path: pathlib.Path, mutation: str):
    verifier = _load_script(VERIFIER, f"rust_native_verifier_coverage_{mutation}")
    declaration, artifacts = _artifact_set(tmp_path)
    source = _receipt_for(artifacts, "linux-x86_64-gnu").parent

    if mutation == "missing":
        shutil.rmtree(_receipt_for(artifacts, "macos-aarch64").parent)
    elif mutation == "duplicate":
        shutil.copytree(
            source,
            artifacts / f"rust-native-linux-x86_64-gnu-{CANDIDATE_SHA}-duplicate",
        )
    elif mutation == "extra":
        extra = artifacts / f"rust-native-unsupported-target-{CANDIDATE_SHA}"
        shutil.copytree(source, extra)
        receipt = json.loads((source / "receipt.json").read_text(encoding="utf-8"))
        receipt["target"] = copy.deepcopy(receipt["target"])
        receipt["target"]["id"] = "unsupported-target"
        _write_json(extra / "receipt.json", receipt)
    else:
        raise AssertionError(f"unknown mutation {mutation}")

    with pytest.raises((RuntimeError, ValueError)):
        verifier.verify_artifact_set(
            declaration_path=declaration,
            artifacts_dir=artifacts,
            candidate_sha=CANDIDATE_SHA,
            candidate_tree=CANDIDATE_TREE,
            output_dir=tmp_path / "verified",
        )
