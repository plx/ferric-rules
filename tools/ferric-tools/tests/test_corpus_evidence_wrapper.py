"""Exercise wrapper finalization with real summaries and isolated command stubs."""

import os
import shlex
import subprocess
import sys

import pytest

from ferric_tools._paths import repo_root
from ferric_tools.compat.corpus_summary import load_summary, unavailable_run, write_json


@pytest.fixture
def wrapper(tmp_path):
    # Avoid uv cache/network work, while invoking the actual summary CLI.
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    uv = bin_dir / "uv"
    uv.write_text(
        '#!/bin/sh\nwhile [ "$1" != python ]; do shift; done\nshift\n'
        f'exec {shlex.quote(sys.executable)} "$@"\n'
    )
    uv.chmod(0o755)
    root = repo_root()
    revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
    output = tmp_path / "evidence"
    environment = {**os.environ, "PATH": str(bin_dir) + os.pathsep + os.environ["PATH"]}

    def call(action, run_id="wrapper-test"):
        return subprocess.run(
            [
                "bash",
                str(root / "scripts/corpus-evidence.sh"),
                action,
                str(output),
                revision,
                run_id,
            ],
            env=environment,
            capture_output=True,
            text=True,
            timeout=30,
        )

    assert call("capture").returncode == 0
    return output, call


def test_capture_removes_only_previous_owned_outputs(wrapper):
    output, call = wrapper
    (output / "ferric.json").write_text("old")
    (output / "reference-exit-code").write_text("0")
    (output / "ferric.log").write_text("old log")
    (output / "other.txt").write_text("retain")
    assert call("capture").returncode == 0
    assert not (output / "ferric.json").exists()
    assert not (output / "reference-exit-code").exists()
    assert not (output / "ferric.log").exists()
    assert (output / "other.txt").read_text() == "retain"


def test_interrupted_start_cannot_restore_previous_evidence(wrapper):
    output, call = wrapper
    summary = load_summary(output / "summary.json")
    summary["runs"]["ferric"] = unavailable_run("previous run", 1)
    write_json(output / "summary.json", summary)
    (output / "ferric.json").write_text("old")
    (output / "ferric-exit-code").write_text("0")
    (output / "starting").touch()
    assert call("finalize").returncode == 0
    assert load_summary(output / "summary.json")["runs"] == {"ferric": None, "reference": None}
    assert not (output / "ferric.json").exists()
    assert not (output / "starting").exists()


@pytest.mark.parametrize("code", [None, "", "truncated", "0", "101"])
def test_missing_and_partial_exit_records_do_not_invent_success(wrapper, code):
    output, call = wrapper
    (output / "ferric.json").write_text("{truncated")
    if code is not None:
        (output / "ferric-exit-code").write_text(code)
    result = call("finalize")
    assert result.returncode == 0, result.stderr
    run = load_summary(output / "summary.json")["runs"]["ferric"]
    assert run["status"] == ("failed" if code == "101" else "incomplete")
    assert run["exit_code"] == (int(code) if code in ("0", "101") else None)


def test_finalize_rejects_a_different_run_id(wrapper):
    _, call = wrapper
    assert call("finalize", "wrong-id").returncode != 0
