"""Capture corpus declarations and attach matching execution evidence.

This module does not execute engines or interpret their output. The producers
own their verdicts; this module checks identity, selection and completion.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import uuid
from pathlib import Path, PurePosixPath

from ferric_tools._paths import repo_root

SCHEMA = "ferric.compat-corpus-summary"
MANIFEST = "tests/clips_compat/corpus/manifest.json"
VERDICTS = {
    "conformance": (True, True),
    "known_gap": (False, True),
    "mismatch": (False, False),
    "unexpected_fix": (True, False),
    "gap_changed": (False, False),
}
# Every manifest flag that changes how a case's output is compared belongs to
# its execution contract, so flipping one is a changed scenario, not a fix.
NOTICE_FLAGS = tuple(
    f"recoverable_{name}_notices"
    for name in ("fact", "control", "random", "build", "introspection")
)
# Mirrors the Rust harness's `deny_unknown_fields`: a new manifest field must be
# classified here before capture accepts it.
CASE_FIELDS = frozenset(
    {"path", "level", "covers", "error", "gap", "resets", "strategy", *NOTICE_FLAGS}
)


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def canonical_digest(value: object) -> str:
    return digest(
        json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()
    )


def write_json(path: Path, value: object) -> None:
    """Replace a report atomically; an interrupted write never looks complete."""
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(path.name + ".tmp")
    temporary.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")
    temporary.replace(path)


def checkout_identity(root: Path, revision: str | None = None) -> dict:
    def git(*args: str) -> str:
        return subprocess.run(
            ["git", "-C", str(root), *args],
            check=True,
            capture_output=True,
            text=True,
            timeout=30,
        ).stdout.strip()

    actual = git("rev-parse", "HEAD")
    if revision is not None and revision != actual:
        raise ValueError(f"requested revision {revision} differs from checkout {actual}")
    return {
        "revision": actual,
        "requested_revision": revision,
        "dirty": bool(git("status", "--porcelain")),
    }


def _case_path(path: object) -> str:
    if (
        not isinstance(path, str)
        or not path
        or "\\" in path
        or PurePosixPath(path).is_absolute()
        or any(part in {"", ".", ".."} for part in path.split("/"))
        or not path.endswith(".clp")
    ):
        raise ValueError(f"unsafe corpus case path: {path!r}")
    return path


def corpus_identity(root: Path) -> dict:
    raw = (root / MANIFEST).read_bytes()
    manifest = json.loads(raw)
    if (
        type(manifest.get("schema_version")) is not int
        or manifest["schema_version"] != 1
        or not isinstance(manifest.get("cases"), list)
    ):
        raise ValueError("invalid corpus manifest")
    cases = {}
    files = {}
    for case in manifest["cases"]:
        path = _case_path(case.get("path"))
        unknown = sorted(case.keys() - CASE_FIELDS)
        if unknown:
            raise ValueError(f"unknown corpus case fields for {path}: {', '.join(unknown)}")
        if path in cases:
            raise ValueError(f"duplicate corpus case: {path}")
        error = case.get("error")
        if case.get("level") not in {"basic", "boundary", "interaction"}:
            raise ValueError(f"invalid corpus level for {path}")
        if error not in (None, "load", "run"):
            raise ValueError(f"invalid error phase for {path}")
        source = root / Path(MANIFEST).parent / path
        for extension in (".clp", ".out", ".in"):
            file = source.with_suffix(extension)
            files[str(PurePosixPath(path).with_suffix(extension))] = (
                digest(file.read_bytes()) if extension != ".in" or file.exists() else None
            )
        contract = {
            "source_sha256": digest(source.read_bytes()),
            "input_sha256": digest(source.with_suffix(".in").read_bytes())
            if source.with_suffix(".in").exists()
            else None,
            "error": error,
            "resets": case.get("resets", 1),
            "strategy": case.get("strategy", "depth"),
        }
        for key in NOTICE_FLAGS:
            contract[key] = case.get(key, False)
            if type(contract[key]) is not bool:
                raise ValueError(f"invalid {key} for {path}")
        if type(contract["resets"]) is not int or contract["resets"] < 1:
            raise ValueError(f"invalid resets for {path}")
        if contract["strategy"] not in {"depth", "breadth", "lex", "mea"}:
            raise ValueError(f"invalid strategy for {path}")
        gap = case.get("gap")
        if gap is not None and (not isinstance(gap, dict) or not gap.get("issues")):
            raise ValueError(f"invalid gap for {path}")
        cases[path] = {
            "expectation": "gap" if gap is not None else "conformance",
            "level": case["level"],
            "error": error,
            "scenario_sha256": canonical_digest(contract),
            "golden_sha256": digest(source.with_suffix(".out").read_bytes()),
            "gap_issues": gap["issues"] if gap is not None else [],
        }
    if not cases:
        raise ValueError("empty corpus manifest")
    return {
        "manifest_path": MANIFEST,
        "manifest_sha256": digest(raw),
        "content_sha256": canonical_digest(cases),
        "files_sha256": canonical_digest(files),
        "declared": _counts(cases),
        "cases": cases,
    }


def _counts(cases: dict) -> dict:
    conformance = [case for case in cases.values() if case["expectation"] == "conformance"]
    return {
        "total": len(cases),
        "conformance": len(conformance),
        "gaps": len(cases) - len(conformance),
        "expected_errors": {
            phase: sum(c["error"] == phase for c in conformance) for phase in ("load", "run")
        },
    }


def capture(root: Path, revision: str | None = None, run_id: str | None = None) -> dict:
    return {
        "schema": SCHEMA,
        "version": 1,
        "run_id": run_id or uuid.uuid4().hex,
        "checkout": checkout_identity(root, revision),
        "corpus": corpus_identity(root),
        "runs": {"ferric": None, "reference": None},
    }


def corpus_counts(summary: dict) -> dict:
    return _counts(summary["corpus"]["cases"])


def _validate_identity(summary: dict) -> None:
    if (
        summary.get("schema") != SCHEMA
        or type(summary.get("version")) is not int
        or summary["version"] != 1
    ):
        raise ValueError("unsupported corpus summary schema")
    if not isinstance(summary.get("run_id"), str) or not summary["run_id"]:
        raise ValueError("missing corpus run ID")
    checkout = summary["checkout"]
    if (
        not re.fullmatch(r"[0-9a-f]{40}", checkout["revision"])
        or type(checkout["dirty"]) is not bool
    ):
        raise ValueError("invalid checkout identity")
    if checkout.get("requested_revision") not in (None, checkout["revision"]):
        raise ValueError("checkout does not match requested revision")
    corpus = summary["corpus"]
    cases = corpus["cases"]
    if not isinstance(cases, dict) or not cases:
        raise ValueError("missing corpus cases")
    for path, case in cases.items():
        _case_path(path)
        if case["expectation"] not in {"gap", "conformance"} or case["error"] not in (
            None,
            "load",
            "run",
        ):
            raise ValueError(f"invalid case declaration: {path}")
        for key in ("scenario_sha256", "golden_sha256"):
            if not re.fullmatch(r"[0-9a-f]{64}", case[key]):
                raise ValueError(f"invalid case digest: {path}")
    if corpus["manifest_path"] != MANIFEST or not re.fullmatch(
        r"[0-9a-f]{64}", corpus["manifest_sha256"]
    ):
        raise ValueError("invalid manifest identity")
    if not re.fullmatch(r"[0-9a-f]{64}", corpus["files_sha256"]):
        raise ValueError("invalid corpus file identity")
    if corpus["content_sha256"] != canonical_digest(cases) or corpus["declared"] != _counts(cases):
        raise ValueError("corpus counts or content identity differ from cases")


def _normalized_run(summary: dict, kind: str, evidence: dict, exit_code: int | None) -> dict:
    """Validate a producer record and recompute all aggregate claims."""
    if evidence.get("run_id") != summary["run_id"]:
        raise ValueError("stale or missing run ID")
    if evidence.get("revision") != summary["checkout"]["revision"]:
        raise ValueError("evidence revision differs from capture")
    if evidence.get("manifest_sha256") != summary["corpus"]["manifest_sha256"]:
        raise ValueError("evidence manifest differs from capture")
    if evidence.get("files_sha256") != summary["corpus"]["files_sha256"]:
        raise ValueError("evidence source/input/golden files differ from capture")
    if exit_code is not None and type(exit_code) is not int:
        raise ValueError("invalid process exit code")
    selection = evidence["selection"]
    paths = selection["paths"]
    cases = summary["corpus"]["cases"]
    if (
        not isinstance(paths, list)
        or not paths
        or len(set(paths)) != len(paths)
        or not set(paths) <= cases.keys()
    ):
        raise ValueError("invalid selected paths")
    if not isinstance(selection.get("filter"), str) or selection.get("level") not in (
        None,
        "basic",
        "boundary",
        "interaction",
    ):
        raise ValueError("invalid selection filters")
    expected_paths = [
        path
        for path, case in cases.items()
        if selection["filter"] in path
        and (selection["level"] is None or case["level"] == selection["level"])
    ]
    if paths != expected_paths:
        raise ValueError("selected paths differ from declared filters or order")
    results = evidence["results"]
    if not isinstance(results, dict) or not results.keys() <= set(paths):
        raise ValueError("unexpected result paths")
    failed = []
    accepted = 0
    for path, result in results.items():
        if kind == "ferric":
            verdict = result.get("verdict")
            if (
                verdict not in VERDICTS
                or (result.get("conforms"), result.get("accepted")) != VERDICTS[verdict]
            ):
                raise ValueError(f"invalid Ferric verdict: {path}")
            if type(result.get("conforms")) is not bool or type(result.get("accepted")) is not bool:
                raise ValueError(f"invalid Ferric verdict flags: {path}")
            expected = "conformance" if cases[path]["expectation"] == "conformance" else "known_gap"
            good = verdict == expected
            if result["accepted"] != good:
                raise ValueError(f"verdict differs from declaration: {path}")
        else:
            if type(result.get("matches")) is not bool:
                raise ValueError(f"invalid reference verdict: {path}")
            good = result["matches"] and "error" not in result
            if result["matches"] and not isinstance(result.get("output"), str):
                raise ValueError(f"missing reference output: {path}")
        accepted += good
        if not good:
            failed.append(path)
    complete = evidence.get("complete") is True and results.keys() == set(paths)
    passed = (
        complete
        and not failed
        and exit_code == 0
        and evidence.get("status") == "passed"
        and not evidence.get("error")
    )
    provenance = evidence.get("provenance", {})
    if (
        kind == "reference"
        and passed
        and (
            not re.fullmatch(r"sha256:[0-9a-f]{64}", provenance.get("image_id", ""))
            or "CLIPS (6.30 " not in provenance.get("version", "")
        )
    ):
        raise ValueError("missing pinned CLIPS provenance")
    if kind == "ferric" and evidence.get("scope") != "characterization":
        raise ValueError("unexpected Ferric evidence scope")
    return {
        "run_id": evidence["run_id"],
        "revision": evidence["revision"],
        "manifest_sha256": evidence["manifest_sha256"],
        "files_sha256": evidence["files_sha256"],
        "scope": "characterization" if kind == "ferric" else "reference",
        "status": "passed"
        if passed
        else "failed"
        if exit_code not in (None, 0) or failed or evidence.get("error")
        else "incomplete",
        "complete": complete,
        "exit_code": exit_code,
        "selection": {**selection, "full": set(paths) == cases.keys()},
        "reported": len(results),
        "accepted": accepted,
        "failures": failed,
        "error": evidence.get("error"),
        "provenance": provenance,
        "results": results,
    }


def unavailable_run(message: str, exit_code: int | None = None) -> dict:
    return {
        "status": "failed" if exit_code not in (None, 0) else "incomplete",
        "complete": False,
        "exit_code": exit_code,
        "selection": {"filter": "", "level": None, "paths": [], "full": False},
        "reported": 0,
        "accepted": 0,
        "failures": [],
        "results": {},
        "error": {"stage": "report", "message": message},
        "provenance": {},
    }


def attach_run(
    summary: dict, kind: str, report: Path, exit_code: int | None, status: Path | None = None
) -> None:
    """Retain evidence failures in the summary instead of losing the artifact."""
    try:
        raw_bytes = report.read_bytes()
        raw = json.loads(raw_bytes)
        if kind == "reference":
            if (
                raw["evidence"].get("schema") != "ferric.compat-corpus-reference"
                or raw["evidence"].get("version") != 1
            ):
                raise ValueError("unsupported reference evidence schema")
            evidence = {**raw["evidence"], "results": raw["results"]}
        else:
            if status is None:
                raise ValueError("Ferric status sidecar is unavailable")
            evidence = json.loads(status.read_text())
            if (
                evidence.get("schema") != "ferric.compat-corpus-status"
                or evidence.get("version") != 1
            ):
                raise ValueError("unsupported Ferric status schema")
            if raw.keys() != evidence["results"].keys():
                raise ValueError("raw observations differ from status results")
            if evidence.get("observations_sha256") != digest(raw_bytes):
                raise ValueError("raw observations differ from status digest")
        summary["runs"][kind] = _normalized_run(summary, kind, evidence, exit_code)
    except (OSError, ValueError, KeyError, TypeError, AttributeError) as error:
        summary["runs"][kind] = unavailable_run(str(error), exit_code)


def load_summary(path: str | Path) -> dict:
    try:
        summary = json.loads(Path(path).read_text())
        _validate_identity(summary)
        if set(summary["runs"]) != {"ferric", "reference"}:
            raise ValueError("invalid corpus run kinds")
        for kind in ("ferric", "reference"):
            run = summary["runs"][kind]
            if run is None:
                continue
            if not isinstance(run, dict) or run.get("status") not in {
                "passed",
                "failed",
                "incomplete",
            }:
                raise ValueError("invalid corpus run")
            _validate_run_shape(run, summary["corpus"]["cases"])
            # Revalidate success claims, including summaries supplied by CI artifacts.
            if (run["status"] == "passed" or "run_id" in run) and _normalized_run(
                summary, kind, run, run["exit_code"]
            ) != run:
                raise ValueError("inconsistent corpus run aggregates")
        return summary
    except (OSError, ValueError, KeyError, TypeError, AttributeError) as error:
        raise ValueError(f"invalid corpus summary {path}: {error}") from error


def _validate_run_shape(run: dict, cases: dict) -> None:
    """Failed evidence must be safe to render too; unavailable records are valid."""
    selection = run["selection"]
    paths = selection["paths"]
    if (
        not isinstance(paths, list)
        or not all(isinstance(path, str) for path in paths)
        or len(set(paths)) != len(paths)
        or not set(paths) <= cases.keys()
        or type(selection["full"]) is not bool
        or selection["full"] != (set(paths) == cases.keys())
        or not isinstance(selection["filter"], str)
        or selection["level"] not in (None, "basic", "boundary", "interaction")
    ):
        raise ValueError("invalid corpus run selection")
    if type(run["complete"]) is not bool or (
        run["exit_code"] is not None and type(run["exit_code"]) is not int
    ):
        raise ValueError("invalid corpus completion/exit status")
    if not isinstance(run["results"], dict) or not isinstance(run["provenance"], dict):
        raise ValueError("invalid corpus run results/provenance")
    if any(type(run[key]) is not int or run[key] < 0 for key in ("accepted", "reported")):
        raise ValueError("invalid corpus run counts")
    if run["reported"] != len(run["results"]) or run["accepted"] > run["reported"]:
        raise ValueError("invalid corpus run count totals")
    if not isinstance(run["failures"], list) or not all(
        isinstance(path, str) for path in run["failures"]
    ):
        raise ValueError("invalid corpus failure paths")
    if not set(run["failures"]) <= run["results"].keys():
        raise ValueError("unknown corpus failure paths")
    error = run["error"]
    if error is not None and (
        not isinstance(error, dict)
        or not all(isinstance(error.get(key), str) for key in ("stage", "message"))
    ):
        raise ValueError("invalid corpus run error")


def corpus_verification(summary: dict) -> dict:
    runs = summary["runs"]
    labels = {kind: run["status"] if run else "unavailable" for kind, run in runs.items()}
    full = all(
        run and run["status"] == "passed" and run["selection"]["full"] for run in runs.values()
    )
    if full:
        status, reason = "verified", "Complete Ferric and pinned CLIPS reference runs passed."
    elif "failed" in labels.values():
        status, reason = "failed", "A corpus execution or evidence check failed."
    elif any(run for run in runs.values()):
        status, reason = "partial", "Full matching Ferric and reference evidence is not available."
    else:
        status, reason = "unavailable", "Declared counts only; no execution evidence."
    if summary["checkout"]["dirty"]:
        reason += " Checkout has local modifications; this is working-tree evidence."
    return {"status": status, **labels, "full": bool(full), "reason": reason}


def corpus_delta(base: dict, head: dict) -> dict:
    before, after = base["corpus"]["cases"], head["corpus"]["cases"]
    bcounts, hcounts = corpus_counts(base), corpus_counts(head)
    verified = all(corpus_verification(s)["status"] == "verified" for s in (base, head))
    result = {
        "base": bcounts,
        "head": hcounts,
        "delta": {key: hcounts[key] - bcounts[key] for key in ("total", "conformance", "gaps")},
        "added": sorted(after.keys() - before.keys()),
        "removed": sorted(before.keys() - after.keys()),
        "fixed_gaps": [],
        "regressions": [],
        "changed_scenarios": [],
        "changed_goldens": [],
    }
    for path in sorted(before.keys() & after.keys()):
        bcase, hcase = before[path], after[path]
        changed = False
        for field, output in (
            ("scenario_sha256", "changed_scenarios"),
            ("golden_sha256", "changed_goldens"),
        ):
            if bcase[field] != hcase[field]:
                result[output].append(path)
                changed = True
        if not changed and bcase["expectation"] != hcase["expectation"]:
            key = "fixed_gaps" if hcase["expectation"] == "conformance" else "regressions"
            result[key].append({"path": path, "verified": verified})
    return result


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--output", type=Path)
    mode.add_argument("--update", type=Path)
    parser.add_argument("--revision")
    parser.add_argument("--run-id")
    parser.add_argument("--root", type=Path)
    parser.add_argument("--require-verified", action="store_true")
    parser.add_argument(
        "--reset-runs", action="store_true", help="Clear runs before a fresh verification"
    )
    for kind in ("ferric", "reference"):
        parser.add_argument(f"--{kind}-report", type=Path)
        parser.add_argument(f"--{kind}-exit-code", type=int)
    parser.add_argument("--ferric-status", type=Path)
    args = parser.parse_args()
    if args.output and (args.require_verified or args.reset_runs):
        parser.error("--require-verified and --reset-runs require --update")
    root = args.root or repo_root()
    if args.output:
        try:
            summary = capture(root, args.revision, args.run_id)
        except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
            write_json(
                args.output,
                {
                    "schema": SCHEMA,
                    "version": 1,
                    "run_id": args.run_id,
                    "corpus": None,
                    "error": {"stage": "capture", "message": str(error)},
                },
            )
            raise SystemExit(str(error)) from error
        write_json(args.output, summary)
        return
    summary = load_summary(args.update)
    try:
        if args.revision is not None and args.revision != summary["checkout"]["revision"]:
            raise ValueError("requested revision differs from captured revision")
        if args.run_id is not None and args.run_id != summary["run_id"]:
            raise ValueError("requested run ID differs from captured run ID")
        current = capture(root, summary["checkout"]["revision"], summary["run_id"])
        if current["corpus"] != summary["corpus"]:
            raise ValueError("corpus changed after capture")
        if args.reset_runs:
            summary["runs"] = {"ferric": None, "reference": None}
        for kind in ("ferric", "reference"):
            report = getattr(args, f"{kind}_report")
            code = getattr(args, f"{kind}_exit_code")
            if report is not None:
                attach_run(summary, kind, report, code, args.ferric_status)
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        for kind in ("ferric", "reference"):
            if getattr(args, f"{kind}_report") is not None:
                summary["runs"][kind] = unavailable_run(
                    str(error), getattr(args, f"{kind}_exit_code")
                )
        write_json(args.update, summary)
        raise SystemExit(str(error)) from error
    write_json(args.update, summary)
    if args.require_verified and corpus_verification(summary)["status"] != "verified":
        raise SystemExit("full Ferric and pinned reference verification is required")


if __name__ == "__main__":
    main()
