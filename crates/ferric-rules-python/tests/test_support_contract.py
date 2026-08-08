"""Keep published Python support metadata aligned with executable CI coverage."""

import pathlib
import re


REPO_ROOT = pathlib.Path(__file__).resolve().parents[3]
PYPROJECT = REPO_ROOT / "crates" / "ferric-rules-python" / "pyproject.toml"
CI_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "ci.yml"
JUSTFILE = REPO_ROOT / "justfile"
UV_LOCK = REPO_ROOT / "crates" / "ferric-rules-python" / "uv.lock"

SUPPORTED_MINORS = ["3.9", "3.10", "3.11", "3.12", "3.13"]
REQUIRES_PYTHON = ">=3.9,<3.14"
LOCK_REQUIRES_PYTHON = ">=3.9, <3.14"


def _python_bindings_job(workflow: str) -> str:
    match = re.search(
        r"(?ms)^  python-bindings:\n(?P<job>.*?)(?=^  [a-z][a-z0-9-]+:\n|\Z)",
        workflow,
    )
    assert match is not None, "CI must define the python-bindings job"
    return match.group("job")


def _accepts_minor(specifier: str, minor: str) -> bool:
    version = tuple(int(part) for part in minor.split("."))
    for clause in specifier.split(","):
        match = re.fullmatch(r"\s*(>=|<)\s*(\d+)\.(\d+)\s*", clause)
        assert match is not None, f"unsupported requires-python clause: {clause!r}"
        boundary = (int(match.group(2)), int(match.group(3)))
        if match.group(1) == ">=" and version < boundary:
            return False
        if match.group(1) == "<" and version >= boundary:
            return False
    return True


def test_python_metadata_matches_ci_matrix():
    pyproject = PYPROJECT.read_text(encoding="utf-8")
    workflow = CI_WORKFLOW.read_text(encoding="utf-8")
    job = _python_bindings_job(workflow)

    requires_python = re.search(r'^requires-python\s*=\s*"([^"]+)"', pyproject, re.MULTILINE)
    assert requires_python is not None
    assert requires_python.group(1) == REQUIRES_PYTHON
    assert (
        '"Programming Language :: Python :: Implementation :: CPython"'
        in pyproject
    )

    classifiers = re.findall(
        r'"Programming Language :: Python :: (\d+\.\d+)"',
        pyproject,
    )
    assert classifiers == SUPPORTED_MINORS

    matrix = re.search(r"python-version:\s*\[([^]]+)]", job)
    assert matrix is not None, "python-bindings must use an explicit Python matrix"
    ci_minors = re.findall(r'"(\d+\.\d+)"', matrix.group(1))
    assert ci_minors == SUPPORTED_MINORS
    assert "3.14" not in classifiers
    assert "3.14" not in ci_minors

    expected_acceptance = {
        "3.8": False,
        "3.9": True,
        "3.10": True,
        "3.11": True,
        "3.12": True,
        "3.13": True,
        "3.14": False,
    }
    actual_acceptance = {
        minor: _accepts_minor(requires_python.group(1), minor)
        for minor in expected_acceptance
    }
    assert actual_acceptance == expected_acceptance


def test_lockfile_carries_the_same_python_range_without_314_artifacts():
    pyproject = PYPROJECT.read_text(encoding="utf-8")
    lockfile = UV_LOCK.read_text(encoding="utf-8")

    project_range = re.search(r'^requires-python\s*=\s*"([^"]+)"', pyproject, re.MULTILINE)
    lock_range = re.search(r'^requires-python\s*=\s*"([^"]+)"', lockfile, re.MULTILINE)
    assert project_range is not None
    assert lock_range is not None
    assert lock_range.group(1) == LOCK_REQUIRES_PYTHON
    assert "".join(lock_range.group(1).split()) == "".join(project_range.group(1).split())
    assert re.search(r"(?i)(?:cp|cpython[-_])314", lockfile) is None


def test_every_matrix_lane_builds_tests_and_smokes_a_wheel():
    workflow = CI_WORKFLOW.read_text(encoding="utf-8")
    job = _python_bindings_job(workflow)

    required_commands = [
        "maturin develop",
        "pytest tests/",
        "maturin build",
        "$RUNNER_TEMP",
        "${#artifacts[@]}",
        "-m venv",
        "pip install",
        'cd "$smoke_dir"',
        "import ferric",
        "sys.version_info[:2]",
        "ferric.__file__",
        "engine.load(",
        "engine.run()",
        "engine.close()",
    ]
    missing = [command for command in required_commands if command not in job]
    assert not missing, f"python-bindings job is missing coverage: {missing}"


def test_supported_builds_do_not_use_pyo3_forward_compatibility_escape_hatch():
    workflow = CI_WORKFLOW.read_text(encoding="utf-8")
    justfile = JUSTFILE.read_text(encoding="utf-8")

    assert "PYO3_USE_ABI3_FORWARD_COMPATIBILITY" not in workflow
    assert "PYO3_USE_ABI3_FORWARD_COMPATIBILITY" not in justfile
