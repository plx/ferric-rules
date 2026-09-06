"""Live observer regressions; run after building the pinned reference image."""

from __future__ import annotations

import hashlib
import os
import secrets
import subprocess
from pathlib import Path
from tempfile import TemporaryDirectory

import pytest

from ferric_tools.compat.projection import project_clips_observation
from ferric_tools.compat.run import load_reference_provenance, run_clips_observer

pytestmark = pytest.mark.skipif(
    os.environ.get("FERRIC_CLIPS_REFERENCE_TESTS") != "1",
    reason="set FERRIC_CLIPS_REFERENCE_TESTS=1 after building the pinned CLIPS image",
)


def test_probe_owns_private_and_imported_facts_without_lookup_errors():
    repo = Path(__file__).resolve().parents[3]
    script = repo / "scripts" / "clips-reference.sh"
    provenance = load_reference_provenance(str(script), root=str(repo))
    assert provenance["engine_version"] == "6.30"
    scratch_parent = repo / ".context"
    scratch_parent.mkdir(exist_ok=True)
    # The wrapper deliberately mounts only its Git root into Docker.
    with TemporaryDirectory(prefix="clips-observer-", dir=scratch_parent) as directory:
        root = Path(directory)
        subprocess.run(["git", "init", "-q", str(root)], check=True)
        source = root / "modules.clp"
        source.write_text(
            """
(defmodule LEFT (export deftemplate shared))
(deftemplate LEFT::shared (slot value))
(deftemplate LEFT::private (slot value))
(deffacts LEFT::seed (shared (value imported)) (private (value left)))
(defmodule RIGHT)
(deftemplate RIGHT::private (slot value))
(deffacts RIGHT::seed (private (value right)))
(defmodule VIEW (import LEFT deftemplate shared))
(defrule MAIN::ready => (printout t "ready" crlf))
""",
            encoding="utf-8",
        )
        digest = hashlib.sha256(source.read_bytes()).hexdigest()
        result = run_clips_observer(
            str(source),
            str(root),
            str(script),
            30,
            fixture_id="observer.module-ownership",
            nonce=secrets.token_hex(16),
            source_sha256=digest,
            composed_sha256=digest,
            globals_to_capture=(),
        )
        assert result["exit_code"] == 0, result
        assert "observation_error" not in result, result
        raw = result["observation"]
        assert raw["protocol_issues"] == []
        assert raw["run"]["rules_fired"] == 1
        projected = project_clips_observation(raw, harness_identity=None)
        assert projected["channels"] == {"stdout": "ready\n", "stderr": ""}
        assert projected["diagnostic"] == {
            "phase": "none",
            "category": "none",
            "continued": True,
        }
        # Imported shared facts appear once; same-named private templates retain
        # their actual owners and distinct values, regardless of enumeration order.
        assert {
            (fact["module"], fact["template"], fact["slots"][0]["value"]["value"])
            for fact in projected["facts"]
        } == {
            ("LEFT", "shared", "imported"),
            ("LEFT", "private", "left"),
            ("RIGHT", "private", "right"),
        }
        assert len(projected["facts"]) == 3
