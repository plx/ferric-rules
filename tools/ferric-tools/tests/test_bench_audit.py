"""The paired audit runner must retain actual medians and reject invalid inputs."""

import importlib.util
import json
from pathlib import Path

import pytest

SCRIPT = Path(__file__).resolve().parents[3] / "scripts" / "bench-audit.py"
spec = importlib.util.spec_from_file_location("bench_audit", SCRIPT)
assert spec and spec.loader
bench_audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bench_audit)


def test_audit_uses_median_and_preserves_raw_measurements(tmp_path):
    source = tmp_path / "criterion" / "case" / "paired"
    source.mkdir(parents=True)
    estimates = {"median": {"point_estimate": 7.0}, "mean": {"point_estimate": 99.0}}
    (source / "estimates.json").write_text(json.dumps(estimates))
    (source / "sample.json").write_text('{"sample": [3, 7, 11]}')
    result = bench_audit.read_medians(tmp_path / "criterion", "paired", tmp_path / "retained")
    assert result == {"case": 7.0}
    for filename in ("estimates.json", "sample.json"):
        assert (tmp_path / "retained" / "case" / "paired" / filename).read_bytes() == (
            source / filename
        ).read_bytes()


@pytest.mark.parametrize("value", [0, -1, "bad", float("inf"), float("nan")])
def test_audit_rejects_invalid_medians(tmp_path, value):
    source = tmp_path / "criterion" / "case" / "paired"
    source.mkdir(parents=True)
    (source / "estimates.json").write_text(json.dumps({"median": {"point_estimate": value}}))
    with pytest.raises(ValueError, match="invalid median"):
        bench_audit.read_medians(tmp_path / "criterion", "paired", tmp_path / "retained")


def test_audit_rejects_missing_measurements(tmp_path):
    with pytest.raises(ValueError, match="no Criterion measurements"):
        bench_audit.read_medians(tmp_path, "missing", tmp_path / "retained")


def test_audit_rejects_non_sha_input_before_accessing_git(tmp_path, monkeypatch):
    monkeypatch.setattr(
        "sys.argv",
        [str(SCRIPT), "main; echo invalid", "0" * 40, "audit-host", str(tmp_path / "output")],
    )
    monkeypatch.setattr(
        bench_audit.subprocess,
        "check_output",
        lambda *args, **kwargs: pytest.fail("invalid SHA reached git"),
    )
    with pytest.raises(SystemExit) as error:
        bench_audit.main()
    assert error.value.code == 2
    assert not (tmp_path / "output").exists()
