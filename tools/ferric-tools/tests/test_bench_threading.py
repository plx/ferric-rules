"""Paired measurements must reject different workload sources before running Cargo."""

import pathlib
import subprocess

import pytest

SCRIPT = pathlib.Path(__file__).resolve().parents[3] / "scripts" / "bench-threading.sh"
RUST_BENCHES = (
    "engine_bench",
    "waltz_bench",
    "churn_bench",
    "join_bench",
    "serialization_bench",
)
RUST_SOURCES = tuple(f"crates/ferric-rules/benches/{name}.rs" for name in RUST_BENCHES)
SUPPORT = "crates/ferric-rules/benches/support/nested/generator.rs"
C_SOURCE = "crates/ferric-rules-ffi/benches/capi_bench.rs"


def git(repo, *args):
    return subprocess.run(
        ["git", *args], cwd=repo, check=True, capture_output=True, text=True
    ).stdout.strip()


def commit(repo):
    git(repo, "add", "--all")
    git(
        repo,
        "-c",
        "user.name=Benchmark test",
        "-c",
        "user.email=benchmark@example.invalid",
        "-c",
        "commit.gpgsign=false",
        "commit",
        "-qm",
        "fixture",
    )
    return git(repo, "rev-parse", "HEAD")


@pytest.fixture
def revisions(tmp_path):
    git(tmp_path, "init", "-q")
    for source in (*RUST_SOURCES, SUPPORT, C_SOURCE, "src/engine.rs"):
        path = tmp_path / source
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("// original source\n")
    base = commit(tmp_path)
    (tmp_path / "src/engine.rs").write_text("// implementation changed\n")
    head = commit(tmp_path)
    return tmp_path, base, head


def run_check(repo, base, head, mode="threading", *, check_only=True):
    command = ["bash", str(SCRIPT), base, head, str(repo / "output"), mode]
    if check_only:
        command.append("--check-sources")
    result = subprocess.run(command, cwd=repo, capture_output=True, text=True, timeout=10)
    # Validation, including all failures, happens before output/worktree creation.
    assert not (repo / "output").exists()
    return result


@pytest.mark.parametrize("mode", ["threading", "threading-capi"])
def test_accepts_identical_workloads_with_changed_engine(revisions, mode):
    repo, base, head = revisions
    result = run_check(repo, base, head, mode)
    assert result.returncode == 0, result.stderr
    assert "Identical benchmark sources" in result.stdout


@pytest.mark.parametrize("source", [*RUST_SOURCES, SUPPORT, C_SOURCE])
def test_rejects_changed_benchmark_or_shared_generator_before_measurement(revisions, source):
    repo, base, _ = revisions
    (repo / source).write_text("// different work or correctness oracle\n")
    head = commit(repo)
    mode = "threading-capi" if source == C_SOURCE else "threading"
    result = run_check(repo, base, head, mode, check_only=False)
    assert result.returncode != 0
    assert "Benchmark source mismatch" in result.stderr
    assert source.rsplit("/nested/", 1)[0] in result.stderr


@pytest.mark.parametrize("both_missing", [False, True])
def test_rejects_missing_required_source(revisions, both_missing):
    repo, base, _ = revisions
    (repo / RUST_SOURCES[0]).unlink()
    head = commit(repo)
    result = run_check(repo, head if both_missing else base, head)
    assert result.returncode != 0
    assert "Missing benchmark source" in result.stderr


def test_rejects_deleted_shared_support_tree(revisions):
    repo, base, _ = revisions
    (repo / SUPPORT).unlink()
    result = run_check(repo, base, commit(repo))
    assert result.returncode != 0
    assert "Missing benchmark source (tree)" in result.stderr


@pytest.mark.parametrize("revision", ["HEAD", "0" * 40, "f" * 39, "F" * 40])
def test_rejects_malformed_or_unavailable_revision(revisions, revision):
    repo, _, head = revisions
    result = run_check(repo, revision, head)
    assert result.returncode != 0
    assert "commit" in result.stderr


def test_rejects_git_object_that_is_not_a_commit(revisions):
    repo, base, head = revisions
    result = run_check(repo, git(repo, "rev-parse", f"{base}^{{tree}}"), head)
    assert result.returncode != 0
    assert "Not an available commit" in result.stderr


def test_rejects_unknown_measurement_set(revisions):
    repo, base, head = revisions
    result = run_check(repo, base, head, "unknown")
    assert result.returncode != 0
    assert "Unknown measurement set" in result.stderr


def test_checks_only_the_selected_suite(revisions):
    repo, base, _ = revisions
    (repo / C_SOURCE).write_text("// C changes do not change Rust workloads\n")
    result = run_check(repo, base, commit(repo))
    assert result.returncode == 0, result.stderr
