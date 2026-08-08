"""Keep the Python release-artifact workflow fail-closed and matrix-complete."""

import json
import pathlib
import re


REPO_ROOT = pathlib.Path(__file__).resolve().parents[3]
WORKFLOW = REPO_ROOT / ".github" / "workflows" / "python-package-artifacts.yml"
CI_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "ci.yml"
TARGETS = REPO_ROOT / "crates" / "ferric-rules-python" / "wheel-targets.json"

MATURIN_ACTION = "PyO3/maturin-action@e83996d129638aa358a18fbd1dfb82f0b0fb5d3b"
SETUP_UV_ACTION = "astral-sh/setup-uv@c771a70e6277c0a99b617c7a806ffedaca235ff9"


def _job(workflow: str, name: str) -> str:
    match = re.search(
        rf"(?ms)^  {re.escape(name)}:\n(?P<job>.*?)(?=^  [a-z][a-z0-9-]+:\n|\Z)",
        workflow,
    )
    assert match is not None, f"workflow must define the {name} job"
    return match.group("job")


def _producer_matrix(job: str) -> dict[str, dict[str, str]]:
    matrix = re.search(
        r"(?ms)^      matrix:\n        include:\n(?P<rows>.*?)(?=^    runs-on:)",
        job,
    )
    assert matrix is not None, "build-wheels must use an explicit include matrix"
    rows: dict[str, dict[str, str]] = {}
    for match in re.finditer(
        r"(?ms)^          - target: (?P<target>[^\n]+)\n"
        r"(?P<body>.*?)(?=^          - target:|\Z)",
        matrix.group("rows"),
    ):
        values = {
            key: value.strip('"')
            for key, value in re.findall(
                r"(?m)^            ([a-z_]+): ([^\n]+)$", match.group("body")
            )
        }
        rows[match.group("target")] = values
    return rows


def test_build_matrix_exactly_matches_the_checked_in_contract():
    workflow = WORKFLOW.read_text(encoding="utf-8")
    contract = json.loads(TARGETS.read_text(encoding="utf-8"))
    rows = _producer_matrix(_job(workflow, "build-wheels"))
    wheels = {wheel["id"]: wheel for wheel in contract["wheels"]}
    pinned_containers = {
        "musllinux1_2-x86_64": "ghcr.io/rust-cross/rust-musl-cross:x86_64-musl@sha256:ce75e9174325d4fbb3de85c309e2d7ca29f7500169bc4b5d2c611ff7e86d549a",
        "musllinux1_2-aarch64": "ghcr.io/rust-cross/rust-musl-cross:aarch64-musl@sha256:ecae5dd62d1c938c14f8071d36c16fa699860aace03bfb5284fb1216474d2643",
    }

    assert len(rows) == 7
    assert rows.keys() == wheels.keys()
    for target, wheel in wheels.items():
        row = rows[target]
        assert row["runner"] == wheel["runner"]
        assert row["rust_target"] == wheel["rust_target"]
        assert row["family"] == wheel["compatibility"]["family"]
        assert row["compatibility"] == wheel["compatibility"]["maturin"]
        assert row.get("container_image", "") == pinned_containers.get(target, "")
        expected_cli = (
            ""
            if row["family"] in {"manylinux", "musllinux"}
            else "--compatibility pypi"
        )
        assert row["compatibility_args"] == expected_cli
        expected_deployment = wheel["compatibility"].get("deployment_target", "")
        assert row["deployment_target"] == expected_deployment


def test_path_filters_cover_every_artifact_contract_input():
    workflow = WORKFLOW.read_text(encoding="utf-8")

    # These appear once under push and once under pull_request. The crate glob
    # covers pyproject, wheel-targets, package assets, and both contract tests.
    for path_filter in [
        '      - ".cargo/**"',
        '      - ".gitattributes"',
        '      - ".github/workflows/python-package-artifacts.yml"',
        '      - "Cargo.lock"',
        '      - "Cargo.toml"',
        '      - "LICENSE-APACHE"',
        '      - "LICENSE-MIT"',
        '      - "crates/ferric-*/**"',
        '      - "scripts/python_package_lib.py"',
        '      - "scripts/musl_static_libgcc_linker.py"',
        '      - "scripts/test-python-sdist-artifact.py"',
        '      - "scripts/test-python-wheel-artifact.py"',
        '      - "scripts/validate-python-package.py"',
        '      - "scripts/verify-python-package-artifacts.py"',
    ]:
        assert workflow.count(path_filter) == 2, f"path filter drifted: {path_filter}"


def test_smoke_matrix_is_the_full_seven_by_five_cross_product():
    workflow = WORKFLOW.read_text(encoding="utf-8")
    contract = json.loads(TARGETS.read_text(encoding="utf-8"))
    job = _job(workflow, "smoke-wheels")

    target_block = re.search(
        r"(?ms)^        target:\n(?P<targets>.*?)(?=^        python_version:)", job
    )
    assert target_block is not None
    targets = re.findall(r"(?m)^          - ([^\n]+)$", target_block.group("targets"))
    expected_targets = [wheel["id"] for wheel in contract["wheels"]]
    assert targets == expected_targets

    minor_block = re.search(r"python_version: \[([^]]+)]", job)
    assert minor_block is not None
    minors = re.findall(r'"(\d+\.\d+)"', minor_block.group(1))
    assert minors == contract["python"]["supported_minors"]
    assert len(targets) * len(minors) == 35

    # Each include row only enriches one original target dimension. GitHub's
    # matrix include semantics therefore apply its runner/family to all five
    # original target x python_version combinations rather than adding jobs.
    include = re.search(r"(?ms)^        include:\n(?P<rows>.*?)(?=^    runs-on:)", job)
    assert include is not None
    included_targets = re.findall(
        r"(?m)^          - target: ([^\n]+)$", include.group("rows")
    )
    assert included_targets == expected_targets
    assert "python_version:" not in include.group("rows")


def test_builds_are_pinned_repaired_audited_and_uploaded_exactly_once():
    workflow = WORKFLOW.read_text(encoding="utf-8")
    job = _job(workflow, "build-wheels")

    assert workflow.count(f"uses: {MATURIN_ACTION}") == 3
    assert f"uses: {SETUP_UV_ACTION}" in workflow
    for pinned_version in [
        'ABI3AUDIT_VERSION: "0.0.26"',
        'AUDITWHEEL_VERSION: "6.7.0"',
        'DELOCATE_VERSION: "0.13.0"',
        'DELVEWHEEL_VERSION: "1.13.0"',
        'MATURIN_VERSION: "1.12.6"',
        'PYTHON_AUDIT_IMAGE: "python:3.12-alpine@sha256:6d43704baacd1bfbe7c295d7f13079d5d8104ed33568873133f8fc69980419df"',
        'RUST_TOOLCHAIN: "1.93.0"',
        'UV_VERSION: "0.12.3"',
    ]:
        assert pinned_version in workflow

    assert "--release" in job
    assert "--locked" in job
    assert "${{ matrix.compatibility_args }}" in job
    assert job.count('compatibility_args: ""') == 4
    assert job.count('compatibility_args: "--compatibility pypi"') == 3
    assert "manylinux: ${{ matrix.container_policy }}" in job
    assert "container: ${{ matrix.container_image }}" in job
    assert "Install the abi3 baseline interpreter for Windows linking" in job
    assert "uses: actions/setup-python@v5" in job
    assert 'python-version: "3.9"' in job
    assert job.count('PYO3_NO_PYTHON: "1"') == 1
    assert "Configure the pinned static musl unwind linker" in job
    assert "CARGO_FERRIC_MUSL_LINKER" in job
    assert "CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER" in job
    assert "CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER" in job
    assert "scripts/musl_static_libgcc_linker.py" in job
    assert "--auditwheel ${{ matrix.auditwheel_mode }}" in job
    assert job.count("auditwheel_mode: repair") == 4
    assert "auditwheel show" in job
    assert "uv pip install" in job
    assert '--target "$auditwheel_root"' in job
    assert '"$PYTHON_AUDIT_IMAGE"' in job
    assert "python -m auditwheel show" in job
    assert "auditwheel did not report exact policy" in job
    assert "musllinux_1_2_${{ matrix.required_arch }}" in job
    assert "--network none" in job
    assert "--read-only" in job
    assert "delocate-wheel" in job
    assert "delvewheel repair" in job
    assert job.count("abi3audit") >= 4
    assert job.count("abi3audit --strict") == 3
    assert job.count("scripts/validate-python-package.py") == 3
    assert job.count("trap 'status=$?") == 3
    assert "steps.linux-audit.outcome != 'skipped'" in job
    assert "steps.macos-audit.outcome != 'skipped'" in job
    assert "steps.windows-audit.outcome != 'skipped'" in job
    assert 'deployment_target: "10.12"' in job
    assert 'deployment_target: "11.0"' in job
    assert "*-cp39-abi3-*.whl" in job
    assert "name: python-wheel-${{ matrix.target }}" in job
    assert "name: python-audit-${{ matrix.target }}" in job
    assert "if-no-files-found: error" in job


def test_each_smoke_uses_only_the_downloaded_repaired_wheel():
    workflow = WORKFLOW.read_text(encoding="utf-8")
    job = _job(workflow, "smoke-wheels")

    assert "needs: build-wheels" in job
    assert "name: python-wheel-${{ matrix.target }}" in job
    assert "maturin build" not in job
    assert "test-python-wheel-artifact.py" in job
    assert '--wheel "$WHEEL"' in job
    assert '--target "$TARGET_ID"' in job
    assert '--receipt "$RECEIPT"' in job
    assert "--network none" in job
    assert '"python:$PYTHON_VERSION-alpine"' in job
    assert "ubuntu-24.04-arm" in job
    assert "--read-only" in job
    assert "Rust toolchain leaked into clean wheel smoke PATH" in job
    assert "UV_OFFLINE=1" in job
    assert "UV_PYTHON_DOWNLOADS=never" in job
    assert "if-no-files-found: error" in job


def test_python_314_rejection_and_clean_sdist_build_are_required():
    workflow = WORKFLOW.read_text(encoding="utf-8")
    rejection = _job(workflow, "reject-python-314")
    sdist = _job(workflow, "build-and-smoke-sdist")

    assert "needs: build-wheels" in rejection
    assert 'python-version: "3.14"' in rejection
    assert "uv python install 3.14" in rejection
    assert rejection.index("uv python install 3.14") < rejection.index("UV_OFFLINE=1")
    assert "--expect-python-rejection" in rejection
    assert "wheel-reject-py314.json" in rejection

    assert "needs: validate-artifact-contract" in sdist
    assert "runs-on: ubuntu-24.04" in sdist
    assert "Install the source-build interpreter" in sdist
    assert 'python-version: "3.12"' in sdist
    assert "command: sdist" in sdist
    assert "args: --out python-sdist-staging" in sdist
    assert "args: --locked" not in sdist
    assert 'cargo "+$RUST_TOOLCHAIN" fetch' in sdist
    assert "--locked --manifest-path Cargo.toml" in sdist
    assert "test-python-sdist-artifact.py" in sdist
    assert '--with "maturin==$MATURIN_VERSION"' in sdist
    assert "python-sdist-final" in sdist
    assert "path: ${{ steps.sdist.outputs.final_path }}" in sdist
    assert "Normalize, locked-build, and smoke on native Linux" in sdist
    assert "docker run" not in sdist
    assert "quay.io/pypa" not in sdist
    assert '--sdist "$RAW_SDIST"' in sdist
    assert '--output "$FINAL_SDIST"' in sdist
    assert "--target manylinux2014-x86_64" in sdist
    assert sdist.count('PYO3_NO_PYTHON: "1"') == 1
    assert "sdist-smoke.json" in sdist
    assert "name: python-sdist" in sdist


def test_aggregate_verifies_every_receipt_and_only_dry_runs_publication():
    workflow = WORKFLOW.read_text(encoding="utf-8")
    job = _job(workflow, "verify-artifact-set")

    for dependency in [
        "smoke-wheels",
        "reject-python-314",
        "build-and-smoke-sdist",
    ]:
        assert f"- {dependency}" in job
    assert "pattern: python-wheel-*" in job
    assert "pattern: python-smoke-*" in job
    assert "pattern: python-audit-*" in job
    assert "verify-python-package-artifacts.py" in job
    assert "--artifacts-dir downloaded-python-artifacts" in job
    assert "--receipts-dir downloaded-python-receipts" in job
    assert "python-package-manifest.json" in job
    assert "expected exactly eight publish files" in job

    assert workflow.count("uv publish") == 2
    assert workflow.count("--dry-run") == 2
    assert workflow.count("--trusted-publishing never") == 2
    assert workflow.count("--no-attestations") == 2
    assert workflow.count('"${publish_files[@]}"') == 2
    assert "--publish-url https://test.pypi.org/legacy/" in workflow
    assert "name: python-package-release-bundle" in job
    assert "downloaded-python-reports" in job

    assert "continue-on-error" not in workflow
    assert "id-token: write" not in workflow
    assert "permissions:\n  contents: read" in workflow


def test_fast_ci_retains_source_tests_without_rebuilding_release_wheels():
    workflow = CI_WORKFLOW.read_text(encoding="utf-8")
    job = _job(workflow, "python-bindings")

    assert "maturin develop" in job
    assert "pytest tests/" in job
    assert "maturin build" not in job
    assert "pip install" not in job
