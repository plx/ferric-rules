"""Build and execute both corpus runners, then publish documentation evidence.

There is deliberately no cached mode: a successful invocation requires the whole
Ferric characterization suite and every CLIPS reference program to finish. The
manifest's historical observations are assertions, never displayed evidence.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
import tempfile
import time
from datetime import UTC, datetime
from pathlib import Path, PurePosixPath

from ferric_tools._paths import repo_root

CORPUS = Path("tests/clips_compat/corpus")
DEFAULT_OUTPUT = Path("site/src/generated/compatibility.json")
DEFAULT_IMAGE = "ferric-rules/clips-reference:latest"
DISPOSITION_KINDS = {"tracked-defect", "deliberate-boundary", "irrelevant"}
GROUPS = {
    "facts": ("Facts and templates", "Fact shapes, slot defaults, duplication, and modification."),
    "patterns": (
        "Patterns and joins",
        "Bindings, constraints, joins, and conditional elements, from simple to interacting cases.",
    ),
    "agenda": ("Agenda and firing", "Activation ordering, salience, refraction, and cancellation."),
    "procedural": (
        "Procedural code",
        "Functions, branching, iteration, local bindings, and return behavior.",
    ),
    "stdlib": (
        "Standard library",
        "Arithmetic, predicates, strings, symbols, multifields, and callable built-ins.",
    ),
    "generics": ("Generic functions", "Method selection, type restrictions, and dispatch."),
    "modules": ("Modules and focus", "Module visibility, imports, exports, and focus behavior."),
    "queries": ("Fact queries", "Query predicates, result values, and query-driven actions."),
    "io": ("Input and output", "Router output, formatting, and replayed input fixtures."),
    "lifecycle": (
        "Lifecycle and globals",
        "Reset behavior, global values, and repeated execution.",
    ),
}


class ReportFailure(RuntimeError):
    """Current, complete, classified execution evidence could not be obtained."""


def read_json(path: Path) -> dict:
    """Read an object, rejecting duplicate keys that could conceal missing cases."""

    def unique_object(pairs: list[tuple[str, object]]) -> dict:
        result = {}
        for key, value in pairs:
            if key in result:
                raise ReportFailure(f"duplicate JSON key {key!r} in {path}")
            result[key] = value
        return result

    try:
        result = json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=unique_object)
    except (OSError, ValueError) as error:
        raise ReportFailure(f"cannot read {path}: {error}") from error
    if not isinstance(result, dict):
        raise ReportFailure(f"expected a JSON object in {path}")
    return result


def exact_keys(actual: set, expected: set, label: str) -> None:
    if actual != expected:
        raise ReportFailure(
            f"{label}: missing {sorted(expected - actual)}, extra {sorted(actual - expected)}"
        )


def source_digest(root: Path) -> str:
    """Hash workspace code, build configuration, fixtures, runners, and policy.

    Include uncommitted edits and added files, but exclude build products and the
    generated report itself. Checking again after execution detects edits made
    while the engines were running.
    """
    inputs = {
        Path("Cargo.toml"),
        Path("Cargo.lock"),
        Path("rust-toolchain.toml"),
        Path("tools/ferric-tools/pyproject.toml"),
        Path("tools/ferric-tools/uv.lock"),
    }
    for directory in ("crates", "examples", "tools"):
        for parent, children, files in os.walk(root / directory):
            children[:] = sorted(
                name for name in children if name not in {"target", ".venv", "node_modules"}
            )
            for filename in files:
                if filename.endswith(".rs") or filename == "Cargo.toml":
                    inputs.add((Path(parent) / filename).relative_to(root))
    for directory in (
        CORPUS,
        Path(".cargo"),
        Path("docker/clips-reference"),
        Path("tools/ferric-tools/src/ferric_tools"),
    ):
        inputs.update(
            path.relative_to(root)
            for path in (root / directory).rglob("*")
            if path.is_file() and "__pycache__" not in path.parts and path.suffix != ".pyc"
        )
    digest = hashlib.sha256()
    for relative in sorted(inputs):
        path = root / relative
        if path.is_file():
            content = path.read_bytes()
            digest.update(relative.as_posix().encode("utf-8") + b"\0")
            digest.update(str(len(content)).encode("ascii") + b"\0" + content)
    return f"sha256:{digest.hexdigest()}"


def validate_manifest(root: Path, manifest: dict) -> dict[str, dict]:
    if manifest.get("schema_version") != 1 or not isinstance(manifest.get("cases"), list):
        raise ReportFailure("unsupported corpus manifest")
    cases = {}
    for case in manifest["cases"]:
        if not isinstance(case, dict) or not isinstance(case.get("path"), str):
            raise ReportFailure("corpus case needs a path")
        path = case["path"]
        parts = PurePosixPath(path)
        if (
            not re.fullmatch(r"[a-zA-Z0-9_./-]+\.clp", path)
            or parts.is_absolute()
            or ".." in parts.parts
            or len(parts.parts) != 2
            or parts.parts[0] not in GROUPS
        ):
            raise ReportFailure(f"unsupported corpus path/group: {path!r}")
        if path in cases:
            raise ReportFailure(f"duplicate corpus case: {path}")
        if case.get("level") not in {"basic", "boundary", "interaction"}:
            raise ReportFailure(f"invalid level: {path}")
        covers = case.get("covers")
        if (
            not isinstance(covers, list)
            or not covers
            or any(not isinstance(tag, str) or not tag for tag in covers)
        ):
            raise ReportFailure(f"missing coverage tags: {path}")
        resets = case.get("resets", 1)
        if type(resets) is not int or not 1 <= resets <= 3:
            raise ReportFailure(f"invalid reset count: {path}")
        if (root / CORPUS / path).with_suffix(".in").is_file() and resets != 1:
            raise ReportFailure(f"input replay across resets is undefined: {path}")
        cases[path] = case
    if not cases:
        raise ReportFailure("empty corpus")
    fixtures = {
        path.relative_to(root / CORPUS).as_posix() for path in (root / CORPUS).rglob("*.clp")
    }
    exact_keys(set(cases), fixtures, "corpus registration")
    return cases


def validate_dispositions(policy: dict, cases: dict[str, dict]) -> None:
    if policy.get("schema_version") != 1:
        raise ReportFailure("unsupported disposition schema")
    exact_keys(set(policy), {"schema_version", "issues", "overrides"}, "disposition policy")
    if not isinstance(policy["issues"], dict) or not isinstance(policy["overrides"], dict):
        raise ReportFailure("disposition issues and overrides must be objects")
    gaps = {path: case["gap"] for path, case in cases.items() if case.get("gap")}
    active_issues = {issue for gap in gaps.values() for issue in gap["issues"]}
    unknown_issues = set(policy["issues"]) - active_issues
    unknown_paths = set(policy["overrides"]) - set(gaps)
    if unknown_issues or unknown_paths:
        raise ReportFailure(
            f"stale dispositions: issues {sorted(unknown_issues)}, cases {sorted(unknown_paths)}"
        )
    for is_override, entries in ((False, policy["issues"]), (True, policy["overrides"])):
        for key, entry in entries.items():
            if not isinstance(entry, dict):
                raise ReportFailure(f"invalid disposition: {key}")
            fields = {"kind", "label", "explanation"} | ({"issues"} if is_override else set())
            exact_keys(set(entry), fields, f"disposition {key}")
            if entry.get("kind") not in DISPOSITION_KINDS or any(
                not isinstance(entry.get(field), str) or not entry[field].strip()
                for field in ("label", "explanation")
            ):
                raise ReportFailure(f"unknown or incomplete disposition: {key}")
            if is_override and entry["issues"] != gaps[key]["issues"]:
                raise ReportFailure(f"override issues disagree with corpus: {key}")
    for path, gap in gaps.items():
        if path not in policy["overrides"]:
            missing = set(gap["issues"]) - set(policy["issues"])
            if missing:
                raise ReportFailure(f"unclassified mismatch {path}: {sorted(missing)}")


def disposition_for(path: str, case: dict, policy: dict, matches: bool) -> dict:
    gap = case.get("gap")
    if matches:
        if gap:
            raise ReportFailure(f"{path}: unexpected match; update characterization and policy")
        return {
            "kind": "match",
            "label": "Matches CLIPS",
            "explanation": "Exact output match after completion, with no Ferric diagnostics.",
            "issues": [],
        }
    if not gap:
        raise ReportFailure(f"unclassified mismatch: {path}")
    if override := policy["overrides"].get(path):
        return dict(override)
    entries = [policy["issues"][issue] for issue in gap["issues"]]
    if (
        len({entry["kind"] for entry in entries}) != 1
        or len({entry["label"] for entry in entries}) != 1
    ):
        raise ReportFailure(f"mixed dispositions require a per-case override: {path}")
    return {
        "kind": entries[0]["kind"],
        "label": entries[0]["label"],
        "explanation": "\n\n".join(dict.fromkeys(entry["explanation"] for entry in entries)),
        "issues": gap["issues"],
    }


def case_title(path: str, source: str) -> str:
    for line in source.splitlines():
        comment = line.strip()
        if not comment:
            continue
        if not comment.startswith(";"):
            break
        comment = comment.lstrip(";").strip()
        if comment and not re.match(r"(?:Level|Covers|Protocol):", comment, re.IGNORECASE):
            return comment.removesuffix(".")
    return re.sub(r"^\d+_", "", PurePosixPath(path).stem).replace("_", " ").capitalize()


def build_report(
    root: Path,
    cases: dict[str, dict],
    policy: dict,
    ferric: dict,
    reference: dict,
    provenance: dict,
) -> dict:
    """Validate fresh observations and join them to prose without normalizing output."""
    validate_dispositions(policy, cases)
    exact_keys(set(ferric), set(cases), "Ferric report cases")
    exact_keys(set(reference), {"version", "image_id", "results"}, "CLIPS report")
    if not isinstance(reference["results"], dict):
        raise ReportFailure("CLIPS results must be an object")
    exact_keys(set(reference["results"]), set(cases), "CLIPS report cases")
    if not isinstance(reference["version"], str) or "CLIPS (6.30 " not in reference["version"]:
        raise ReportFailure("CLIPS report does not identify the required 6.30 reference")
    if not isinstance(reference["image_id"], str) or not re.fullmatch(
        r"sha256:[0-9a-f]{64}", reference["image_id"]
    ):
        raise ReportFailure("CLIPS report lacks an immutable reference image identity")
    groups = {
        key: {"id": key, "title": title, "description": description, "cases": []}
        for key, (title, description) in GROUPS.items()
    }
    matching = 0
    levels = {"basic": 0, "boundary": 1, "interaction": 2}
    for path, case in sorted(cases.items(), key=lambda item: (levels[item[1]["level"]], item[0])):
        observation = ferric[path]
        if not isinstance(observation, dict):
            raise ReportFailure(f"invalid Ferric observation: {path}")
        exact_keys(set(observation), {"phase", "output", "diagnostics"}, f"Ferric {path}")
        if (
            observation["phase"] not in {"complete", "load", "reset", "run"}
            or not isinstance(observation["output"], str)
            or not isinstance(observation["diagnostics"], list)
            or any(not isinstance(item, str) for item in observation["diagnostics"])
        ):
            raise ReportFailure(f"invalid Ferric observation: {path}")
        clips = reference["results"][path]
        if not isinstance(clips, dict):
            raise ReportFailure(f"invalid CLIPS result: {path}")
        exact_keys(set(clips), {"matches", "output"}, f"CLIPS {path}")
        if clips["matches"] is not True or not isinstance(clips["output"], str):
            raise ReportFailure(f"CLIPS reference failed: {path}")
        fixture = root / CORPUS / path
        golden = fixture.with_suffix(".out").read_text(encoding="utf-8")
        if not golden or not golden.endswith("\n") or clips["output"] != golden:
            raise ReportFailure(f"CLIPS output does not match its nonempty golden: {path}")
        matches = (
            observation["phase"] == "complete"
            and not observation["diagnostics"]
            and observation["output"] == clips["output"]
        )
        disposition = disposition_for(path, case, policy, matches)
        source = fixture.read_text(encoding="utf-8")
        input_path = fixture.with_suffix(".in")
        groups[PurePosixPath(path).parts[0]]["cases"].append(
            {
                "id": path.removesuffix(".clp").replace("/", "_"),
                "path": path,
                "title": case_title(path, source),
                "level": case["level"],
                "covers": case["covers"],
                "source": source,
                "input": input_path.read_text(encoding="utf-8") if input_path.is_file() else None,
                "resets": case.get("resets", 1),
                "clips_output": clips["output"],
                "ferric_output": observation["output"],
                "ferric_phase": observation["phase"],
                "diagnostics": observation["diagnostics"],
                "matches": matches,
                "disposition": disposition,
            }
        )
        matching += matches
    return {
        "schema_version": 1,
        "provenance": {
            **provenance,
            "reference_version": reference["version"],
            "reference_image": reference["image_id"],
        },
        "summary": {"total": len(cases), "matching": matching, "different": len(cases) - matching},
        "groups": [group for group in groups.values() if group["cases"]],
    }


def execute_report(command: list[str], path: Path, root: Path, env: dict, timeout: float) -> dict:
    """Only accept a new report from a successful, bounded subprocess."""
    if path.exists():
        raise ReportFailure(f"refusing preexisting execution report: {path}")
    started = time.time_ns()
    subprocess.run(command, cwd=root, env=env, check=True, timeout=timeout)
    if not path.is_file():
        raise ReportFailure(f"runner succeeded without producing its report: {path}")
    # Unique previously absent paths are the primary freshness guarantee; this
    # additionally rejects a runner that copied an older report into that path.
    if path.stat().st_mtime_ns < started:
        raise ReportFailure(f"stale execution report: {path}")
    return read_json(path)


def generate(
    root: Path,
    output: Path,
    *,
    image: str = DEFAULT_IMAGE,
    ferric_timeout: float = 600,
    reference_timeout: float = 600,
    case_timeout: float = 15,
    workers: int = 4,
) -> dict:
    """Publish atomically on success; leave no old data available after failure."""
    output.unlink(missing_ok=True)
    before = source_digest(root)
    manifest = read_json(root / CORPUS / "manifest.json")
    cases = validate_manifest(root, manifest)
    policy = read_json(root / CORPUS / "dispositions.json")
    validate_dispositions(policy, cases)
    revision = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=root,
        capture_output=True,
        text=True,
        check=True,
        timeout=15,
    ).stdout.strip()
    if not re.fullmatch(r"[0-9a-f]{40,64}", revision):
        raise ReportFailure("cannot identify source revision")
    workspace_dirty = bool(
        subprocess.run(
            ["git", "status", "--porcelain", "--untracked-files=all"],
            cwd=root,
            capture_output=True,
            text=True,
            check=True,
            timeout=15,
        ).stdout.strip()
    )
    env = os.environ.copy()
    env.pop("FERRIC_CORPUS_FILTER", None)
    env.pop("FERRIC_CORPUS_LEVEL", None)
    with tempfile.TemporaryDirectory(prefix="ferric-compat-site-") as scratch:
        ferric_path, clips_path = Path(scratch) / "ferric.json", Path(scratch) / "clips.json"
        env["FERRIC_CORPUS_REPORT"] = str(ferric_path)
        ferric = execute_report(
            [
                "cargo",
                "test",
                "--locked",
                "-p",
                "ferric-rules",
                "--test",
                "compat_corpus",
                "--",
                "--nocapture",
            ],
            ferric_path,
            root,
            env,
            ferric_timeout,
        )
        reference = execute_report(
            [
                sys.executable,
                "-m",
                "ferric_tools.compat.corpus",
                "--image",
                image,
                "--report",
                str(clips_path),
                "--timeout",
                str(case_timeout),
                "--workers",
                str(workers),
            ],
            clips_path,
            root,
            env,
            reference_timeout,
        )
    report = build_report(
        root,
        cases,
        policy,
        ferric,
        reference,
        {
            "revision": revision,
            "workspace_dirty": workspace_dirty,
            "source_digest": before,
            "generated_at": datetime.now(UTC).isoformat(timespec="seconds").replace("+00:00", "Z"),
        },
    )
    if source_digest(root) != before:
        raise ReportFailure("source inputs changed during execution; rerun with stable inputs")
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w", encoding="utf-8", dir=output.parent, suffix=".tmp", delete=False
        ) as file:
            temporary = Path(file.name)
            json.dump(report, file, indent=2, ensure_ascii=False)
            file.write("\n")
        temporary.replace(output)
    finally:
        if temporary:
            temporary.unlink(missing_ok=True)
    return report


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, help=f"Default: {DEFAULT_OUTPUT}")
    parser.add_argument("--image", default=DEFAULT_IMAGE)
    parser.add_argument("--ferric-timeout", type=float, default=600)
    parser.add_argument("--reference-timeout", type=float, default=600)
    parser.add_argument("--case-timeout", type=float, default=15)
    parser.add_argument("--workers", type=int, default=4)
    args = parser.parse_args()
    if min(args.ferric_timeout, args.reference_timeout, args.case_timeout) <= 0 or args.workers < 1:
        parser.error("timeouts and workers must be positive")
    root = repo_root()
    output = args.output if args.output is not None else root / DEFAULT_OUTPUT
    try:
        report = generate(
            root,
            output,
            image=args.image,
            ferric_timeout=args.ferric_timeout,
            reference_timeout=args.reference_timeout,
            case_timeout=args.case_timeout,
            workers=args.workers,
        )
    except (ReportFailure, OSError, subprocess.SubprocessError) as error:
        parser.exit(1, f"Compatibility documentation generation failed: {error}\n")
    summary = report["summary"]
    print(
        f"Compatibility documentation: {summary['total']} probes, "
        f"{summary['matching']} matches, {summary['different']} explained differences -> {output}"
    )


if __name__ == "__main__":
    main()
