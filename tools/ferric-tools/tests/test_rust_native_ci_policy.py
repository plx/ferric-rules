"""Keep the declared Rust/CLI release matrix native, complete, and fail-closed."""

import importlib.util
import json
import pathlib
import re
import sys
import tomllib

REPO_ROOT = pathlib.Path(__file__).resolve().parents[3]
DECLARATION = REPO_ROOT / "crates" / "ferric-rules-cli" / "release-targets.json"
WORKFLOW = REPO_ROOT / ".github" / "workflows" / "rust-native-artifacts.yml"
HARNESS = REPO_ROOT / "scripts" / "test-rust-native-artifact.py"
VERIFIER = REPO_ROOT / "scripts" / "verify-rust-native-artifacts.py"
RUST_TOOLCHAIN = REPO_ROOT / "rust-toolchain.toml"
GITATTRIBUTES = REPO_ROOT / ".gitattributes"

ALPINE_IMAGE = (
    "python:3.12-alpine@sha256:6d43704baacd1bfbe7c295d7f13079d5d8104ed33568873133f8fc69980419df"
)

EXPECTED_TARGETS = {
    "linux-x86_64-gnu": {
        "runner": "ubuntu-24.04",
        "rust_target": "x86_64-unknown-linux-gnu",
        "family": "linux",
        "architecture": "x86_64",
        "libc": "glibc",
        "native_environment": "ubuntu-24.04",
    },
    "linux-aarch64-gnu": {
        "runner": "ubuntu-24.04-arm",
        "rust_target": "aarch64-unknown-linux-gnu",
        "family": "linux",
        "architecture": "aarch64",
        "libc": "glibc",
        "native_environment": "ubuntu-24.04-arm",
    },
    "linux-x86_64-musl": {
        "runner": "ubuntu-24.04",
        "rust_target": "x86_64-unknown-linux-musl",
        "family": "linux",
        "architecture": "x86_64",
        "libc": "musl",
        "native_environment": ALPINE_IMAGE,
    },
    "linux-aarch64-musl": {
        "runner": "ubuntu-24.04-arm",
        "rust_target": "aarch64-unknown-linux-musl",
        "family": "linux",
        "architecture": "aarch64",
        "libc": "musl",
        "native_environment": ALPINE_IMAGE,
    },
    "macos-x86_64": {
        "runner": "macos-15-intel",
        "rust_target": "x86_64-apple-darwin",
        "family": "macos",
        "architecture": "x86_64",
        "libc": "none",
        "native_environment": "macos-15-intel",
    },
    "macos-aarch64": {
        "runner": "macos-15",
        "rust_target": "aarch64-apple-darwin",
        "family": "macos",
        "architecture": "aarch64",
        "libc": "none",
        "native_environment": "macos-15",
    },
    "windows-x86_64-msvc": {
        "runner": "windows-2025",
        "rust_target": "x86_64-pc-windows-msvc",
        "family": "windows",
        "architecture": "x86_64",
        "libc": "msvc",
        "native_environment": "windows-2025",
    },
}

EXPECTED_PACKAGES = [
    "ferric-rules",
    "ferric-rules-cli",
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


def _job(workflow: str, name: str) -> str:
    match = re.search(
        rf"(?ms)^  {re.escape(name)}:\n(?P<job>.*?)(?=^  [a-z][a-z0-9-]+:\n|\Z)",
        workflow,
    )
    assert match is not None, f"workflow must define the {name} job"
    return match.group("job")


def _load_script(path: pathlib.Path, name: str):
    assert path.is_file(), f"missing required native Rust script: {path}"
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


def _workflow_text() -> str:
    assert WORKFLOW.is_file(), f"missing required native Rust workflow: {WORKFLOW}"
    return WORKFLOW.read_text(encoding="utf-8")


def _declaration() -> dict[str, object]:
    assert DECLARATION.is_file(), f"missing Rust/CLI release declaration: {DECLARATION}"
    return json.loads(DECLARATION.read_text(encoding="utf-8"))


def _matrix_rows(job: str) -> dict[str, dict[str, str]]:
    matrix = re.search(
        r"(?ms)^      matrix:\n        include:\n(?P<rows>.*?)(?=^    runs-on:)",
        job,
    )
    assert matrix is not None, "native-rust must use an explicit include matrix"
    rows: dict[str, dict[str, str]] = {}
    for match in re.finditer(
        r"(?ms)^          - target: (?P<target>[^\n]+)\n"
        r"(?P<body>.*?)(?=^          - target:|\Z)",
        matrix.group("rows"),
    ):
        target = match.group("target").strip('"')
        assert target not in rows, f"duplicate native matrix target: {target}"
        rows[target] = {
            key: value.strip().strip('"')
            for key, value in re.findall(
                r"(?m)^            ([a-z_]+): ([^\n#]+?)(?:\s+#.*)?$", match.group("body")
            )
        }
    return rows


def _assert_direct_candidate_checkout(job: str) -> None:
    checkout = re.search(
        r"(?ms)^      - uses: actions/checkout@v4\n(?P<with>        with:\n(?:          .*\n)+)",
        job,
    )
    assert checkout is not None, "job must check out an explicit immutable candidate"
    checkout_with = checkout.group("with")
    assert "ref:" in checkout_with
    assert "github.event.pull_request.head.sha" in checkout_with
    assert "github.sha" in checkout_with
    assert "fetch-depth: 0" in checkout_with


def test_release_declaration_is_the_exact_non_vacuous_native_matrix():
    declaration = _declaration()

    assert declaration["schema_version"] == 1
    assert declaration["artifact_contract"]["packages"] == EXPECTED_PACKAGES
    assert declaration["artifact_contract"]["binary"] == "ferric"
    assert declaration["artifact_contract"]["install_all_features"] is True
    assert declaration["artifact_contract"]["execution"] == "native"
    assert declaration["artifact_contract"]["distribution"] == "ci-evidence-only"

    targets = declaration["targets"]
    assert len(targets) == 7
    by_id = {target["id"]: target for target in targets}
    assert len(by_id) == len(targets), "release target IDs must be unique"
    assert by_id.keys() == EXPECTED_TARGETS.keys()
    for target_id, expected in EXPECTED_TARGETS.items():
        actual = by_id[target_id]
        for key, value in expected.items():
            assert actual.get(key) == value, f"{target_id}.{key} drifted"
        assert actual.get("conformance") == "native"


def test_workflow_matrix_exactly_matches_the_release_declaration():
    workflow = _workflow_text()
    declaration = _declaration()
    declared = {target["id"]: target for target in declaration["targets"]}
    job = _job(workflow, "native-rust")
    rows = _matrix_rows(job)

    assert rows.keys() == EXPECTED_TARGETS.keys()
    for target_id, row in rows.items():
        target = declared[target_id]
        for key in ["runner", "rust_target", "family", "libc"]:
            assert row.get(key) == target.get(key), f"matrix {target_id}.{key} drifted"
        expected_image = target["native_environment"] if target["libc"] == "musl" else None
        assert row.get("container_image") == expected_image, (
            f"matrix {target_id}.container_image drifted"
        )

    assert "    runs-on: ${{ matrix.runner }}\n" in job
    assert "      fail-fast: false\n" in job
    assert "    timeout-minutes: 45\n" in job
    assert "continue-on-error" not in workflow
    assert "cross-compile" not in job.lower()
    _assert_direct_candidate_checkout(job)


def test_each_native_job_packages_installs_and_smokes_outside_the_worktree():
    workflow = _workflow_text()
    job = _job(workflow, "native-rust")

    for command in [
        "scripts/test-rust-native-artifact.py",
        "--declaration crates/ferric-rules-cli/release-targets.json",
        '--target-id "$TARGET_ID"',
        '--candidate-sha "$CANDIDATE_SHA"',
        '--candidate-tree "$CANDIDATE_TREE"',
    ]:
        assert command in job, f"native-rust is missing `{command}`"
    assert "          python scripts/test-rust-native-artifact.py" in job
    assert "              python3 scripts/test-rust-native-artifact.py" in job

    assert "runner.temp" in job
    assert "rust-native-evidence/${{ matrix.target }}" in job
    assert "name: rust-native-${{ matrix.target }}-${{ env.CANDIDATE_SHA }}" in job
    assert "receipt.json" in job
    assert "if-no-files-found: error" in job
    assert "retention-days: 30" in job

    # The musl rows must compile, install, execute, and inspect in Alpine on a
    # same-architecture runner. A target-only build on glibc is not evidence.
    assert "if: matrix.libc == 'musl'" in job
    assert "docker run" in job
    assert "${{ runner.temp }}" in job
    assert "${{ matrix.container_image }}" in job
    assert "rustup" in job
    assert re.search(r"apk add --no-cache [^\n]*\bbinutils\b", job)

    # These observable smokes are intentionally locked at the workflow/harness
    # boundary so a matrix cannot pass after silently dropping one category.
    harness_module = _load_script(HARNESS, "rust_native_artifact_harness_policy")
    assert harness_module.CARGO_TEST_COMMAND == (
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
    assert harness_module.CARGO_BUILD_COMMAND == (
        "cargo",
        "build",
        "--release",
        "-p",
        "ferric-rules-cli",
        "--all-features",
        "--locked",
    )
    assert harness_module.CARGO_PACKAGE_COMMANDS == (
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
    assert harness_module.MANDATORY_COMMAND_NAMES == MANDATORY_COMMAND_NAMES

    harness = HARNESS.read_text(encoding="utf-8").lower()
    for required in [
        "install",
        "--path",
        "--root",
        "--all-features",
        "--locked",
        "version",
        "unicode",
        "crlf",
        "snapshot",
        "repl",
        "exit",
        "dynamic",
        "dependency",
        "readelf",
        "[probe readelf -l]",
        "candidate_sha",
        "candidate_tree",
    ]:
        assert required in harness, f"artifact harness lost its {required} contract"


def test_stable_aggregate_verifies_and_retains_the_exact_candidate_bundle():
    workflow = _workflow_text()
    job = _job(workflow, "native-rust-required")

    assert "    name: Rust Native Artifacts\n" in job
    assert "    needs: native-rust\n" in job
    assert "    if: always()\n" in job
    assert ("if: needs.native-rust.result != 'success'" in job and "exit 1" in job) or (
        "NATIVE_RUST_PASSED: ${{ needs.native-rust.result == 'success' }}" in job
        and 'test "$NATIVE_RUST_PASSED" = true' in job
    )
    _assert_direct_candidate_checkout(job)
    for required in [
        "actions/download-artifact@v4",
        "rust-native-*",
        "scripts/verify-rust-native-artifacts.py",
        "--declaration crates/ferric-rules-cli/release-targets.json",
        '--candidate-sha "$CANDIDATE_SHA"',
        '--candidate-tree "$CANDIDATE_TREE"',
        "actions/upload-artifact@v4",
        "rust-native-verified",
        "if-no-files-found: error",
        "runner.temp",
        "retention-days: 30",
    ]:
        assert required in job, f"native-rust-required is missing `{required}`"
    assert "name: rust-native-verified-${{ env.CANDIDATE_SHA }}" in job

    verifier = VERIFIER.read_text(encoding="utf-8").lower()
    for required in [
        "candidate_sha",
        "candidate_tree",
        "declaration_sha256",
        "packages",
        "installed_binary",
        "sha256",
        "receipt.json",
        "exactly 7",
        "toolchain",
        "environment",
        "commands",
        "expected_exit",
        "actual_exit",
    ]:
        assert required in verifier, f"aggregate verifier lost its {required} contract"


def test_workflow_trigger_and_permissions_make_the_stable_gate_unskippable():
    workflow = _workflow_text()
    toolchain = tomllib.loads(RUST_TOOLCHAIN.read_text(encoding="utf-8"))

    assert workflow.startswith("name: Rust Native Artifacts\n")
    assert re.search(r"(?m)^  push:\n    branches: \[main]$", workflow)
    assert re.search(r"(?m)^  pull_request:\n    branches: \[main]$", workflow)
    assert re.search(r"(?m)^  workflow_dispatch:$", workflow)
    assert "paths:" not in workflow
    assert "paths-ignore:" not in workflow
    assert re.search(r"(?m)^permissions:\n  contents: read$", workflow)
    assert toolchain["toolchain"]["channel"] == "1.93.0"
    assert re.search(r'(?m)^  RUST_TOOLCHAIN: "1\.93\.0"$', workflow)


def test_release_declaration_bytes_are_stable_across_native_checkouts():
    attributes = GITATTRIBUTES.read_text(encoding="utf-8").splitlines()

    assert "crates/ferric-rules-cli/release-targets.json text eol=lf" in attributes
