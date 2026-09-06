"""The Python 3.9 test dependency must not use a shared predictable temp base."""

import os
import stat

import pytest


def test_temp_files_live_under_private_parent(tmp_path, tmp_path_factory, pytestconfig):
    if any(
        arg.startswith("--basetemp")
        for arg in [
            *pytestconfig.invocation_params.args,
            *os.environ.get("PYTEST_ADDOPTS", "").split(),
        ]
    ):
        pytest.skip("Explicit --basetemp is the caller's responsibility")
    parent = tmp_path_factory.getbasetemp().parent
    assert tmp_path.is_relative_to(parent)
    assert parent.name.startswith("ferric-pytest-")
    if os.name == "posix":
        assert stat.S_IMODE(parent.stat().st_mode) == 0o700
        assert parent.stat().st_uid == os.getuid()
