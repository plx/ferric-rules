"""Verify the granular corpus goldens against an actual CLIPS 6.30 process.

This command never derives a CLIPS expectation from Ferric and never silently
falls back to Ferric-only validation. The Rust integration test consumes the
same manifest and .out files without requiring Docker on ordinary CI runs.
"""

from __future__ import annotations

import argparse
import concurrent.futures
import json
import re
import subprocess
import tempfile
import uuid
from pathlib import Path

from ferric_tools._paths import repo_root


class ReferenceFailure(RuntimeError):
    """The reference did not complete a valid, bounded program."""


RUN_LIMIT = 1_000
STATISTICS = re.compile(
    r"(?m)^(?P<count>\d+) rules fired(?:[ \t]+Run time is [^\n]+ seconds\.)?\n"
    r"(?:[^\n]+ rules per second\.\n)?"
    r"\d+ mean number of facts \(\d+ maximum\)\.\n"
    r"(?:\d+ mean number of instances \(\d+ maximum\)\.\n)?"
    r"\d+ mean number of activations \(\d+ maximum\)\.\n\Z"
)


def extract_output(stdout: str, stderr: str, begin: str, end: str) -> str:
    """Require exactly one complete frame and reject reference diagnostics."""
    if stderr.strip():
        raise ReferenceFailure(f"CLIPS diagnostics on stderr:\n{stderr}")
    if stdout.count(begin + "\n") != 1 or stdout.count(end + "\n") != 1:
        raise ReferenceFailure(f"missing/duplicate CLIPS output markers:\n{stdout}")
    prefix, remainder = stdout.split(begin + "\n", 1)
    output, suffix = remainder.split(end + "\n", 1)
    # A fresh CLIPS environment already contains MAIN; declaring its imports
    # legitimately emits this one specific warning even when load succeeds.
    preamble = prefix.replace("[CSTRCPSR1] WARNING: Redefining defmodule: MAIN\n", "")
    if re.search(r"\[[A-Z]+\d+\]", preamble + suffix):
        raise ReferenceFailure(f"CLIPS load/protocol diagnostic:\n{prefix}{suffix}")
    # The Debian CLIPS executable also writes runtime errors to stdout. Reserve
    # its diagnostic-code-plus-message syntax; a literal [USER123] stays valid.
    if re.search(r"\[[A-Z]+\d+\][ \t]", output):
        raise ReferenceFailure(f"CLIPS runtime diagnostic:\n{output}")
    return output


def extract_runs(output: str, begin: str, end: str, resets: int) -> str:
    """Remove framed statistics, rejecting any run that exhausted its bound.

    Corpus programs must finish their output with a newline and must not change
    the statistics watch setting. This separates program output from CLIPS's
    trailing statistics without changing or normalizing the golden text.
    """
    captured = []
    remaining = output
    for index in range(resets):
        run_begin = f"{begin}_RUN_{index}\n"
        run_end = f"{end}_RUN_{index}\n"
        if not remaining.startswith(run_begin) or remaining.count(run_end) != 1:
            raise ReferenceFailure("missing/duplicate CLIPS run/statistics frame")
        run_output, remaining = remaining[len(run_begin) :].split(run_end, 1)
        statistics = STATISTICS.search(run_output)
        if statistics is None:
            raise ReferenceFailure("missing CLIPS statistics or output did not end with a newline")
        count = int(statistics["count"])
        if count >= RUN_LIMIT:
            raise ReferenceFailure(f"reference reached the {RUN_LIMIT}-firing limit")
        captured.append(run_output[: statistics.start()])
    if remaining:
        raise ReferenceFailure(f"unexpected output after CLIPS run frames: {remaining!r}")
    return "".join(captured)


def batch_source(path: str, begin: str, end: str, resets: int) -> str:
    """Use load's boolean result so partial loads cannot produce an oracle."""
    # Paths are relative, corpus-controlled POSIX names, never CLIPS expressions.
    if not re.fullmatch(r"[a-zA-Z0-9_./-]+", path) or ".." in Path(path).parts:
        raise ReferenceFailure(f"invalid fixture path: {path!r}")
    if not 1 <= resets <= 3:
        raise ReferenceFailure("resets must be between 1 and 3")
    runs = "\n".join(
        f'(reset) (printout t "{begin}_RUN_{index}" crlf)\n'
        f"(watch statistics) (run {RUN_LIMIT}) (unwatch statistics)\n"
        f'(printout t "{end}_RUN_{index}" crlf)'
        for index in range(resets)
    )
    return (
        f'(if (load "{path}") then\n'
        f' (printout t "{begin}" crlf)\n{runs}\n'
        f' (printout t "{end}" crlf))\n(exit)\n'
    )


def run_reference(root: Path, case: dict, image: str, timeout: float) -> str:
    """One isolated container, read-only source mount, deadline and cleanup."""
    token = uuid.uuid4().hex
    begin, end = f"CORPUS_BEGIN_{token}", f"CORPUS_END_{token}"
    name = f"ferric-corpus-{token}"
    scratch = root / "target" / "compat-corpus"
    scratch.mkdir(parents=True, exist_ok=True)
    resets = case.get("resets", 1)
    input_path = (root / "tests/clips_compat/corpus" / case["path"]).with_suffix(".in")
    if input_path.exists() and resets != 1:
        raise ReferenceFailure(
            "input fixtures require exactly one reset; input replay is undefined"
        )
    source = batch_source(f"tests/clips_compat/corpus/{case['path']}", begin, end, resets)
    with tempfile.NamedTemporaryFile(mode="w", suffix=".clp", dir=scratch) as batch:
        batch.write(source)
        batch.flush()
        command = [
            "docker",
            "run",
            "--rm",
            "-i",
            "--name",
            name,
            "-v",
            f"{root}:/workspace:ro",
            "-w",
            "/workspace",
            image,
            "-f2",
            str(Path(batch.name).relative_to(root)),
        ]
        try:
            input_text = input_path.read_text() if input_path.exists() else ""
            process = subprocess.run(
                command, input=input_text, capture_output=True, text=True, timeout=timeout
            )
        except subprocess.TimeoutExpired as error:
            # Killing the Docker client alone can leave the container running.
            subprocess.run(
                ["docker", "rm", "-f", name], capture_output=True, check=False, timeout=10
            )
            raise ReferenceFailure(f"reference timed out after {timeout}s") from error
    if process.returncode:
        raise ReferenceFailure(f"CLIPS exit {process.returncode}: {process.stderr}")
    output = extract_output(process.stdout, process.stderr, begin, end)
    return extract_runs(output, begin, end, resets)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--filter", default="", help="Substring of the corpus-relative .clp path")
    parser.add_argument("--level", choices=["basic", "boundary", "interaction"])
    parser.add_argument("--image", default="ferric-rules/clips-reference:latest")
    parser.add_argument("--timeout", type=float, default=15)
    parser.add_argument("--workers", type=int, default=4)
    parser.add_argument("--report", type=Path, help="Write reference provenance/results as JSON")
    args = parser.parse_args()
    if args.timeout <= 0 or args.workers < 1:
        parser.error("timeout and workers must be positive")
    root = repo_root()
    corpus = root / "tests" / "clips_compat" / "corpus"
    manifest = json.loads((corpus / "manifest.json").read_text())
    cases = [
        case
        for case in manifest["cases"]
        if args.filter in case["path"] and (args.level is None or case["level"] == args.level)
    ]
    if not cases:
        parser.error("filter matched no cases")
    # Resolve a mutable tag once; all programs in this run use the same image.
    image = subprocess.run(
        ["docker", "image", "inspect", args.image, "--format", "{{.Id}}"],
        capture_output=True,
        text=True,
        check=True,
        timeout=15,
    ).stdout.strip()
    version = subprocess.run(
        ["docker", "run", "--rm", "-i", image],
        input="(exit)\n",
        capture_output=True,
        text=True,
        check=True,
        timeout=15,
    ).stdout.strip()
    if "CLIPS (6.30 " not in version:
        raise ReferenceFailure(f"expected CLIPS 6.30, got {version!r}")
    results = {}

    def verify(case: dict) -> tuple[str, dict]:
        try:
            output = run_reference(root, case, image, args.timeout)
            expected = (corpus / case["path"]).with_suffix(".out").read_text()
            return case["path"], {"matches": output == expected, "output": output}
        except (ReferenceFailure, OSError, subprocess.SubprocessError) as error:
            return case["path"], {"matches": False, "error": str(error)}

    with concurrent.futures.ThreadPoolExecutor(max_workers=args.workers) as pool:
        for path, result in pool.map(verify, cases):
            results[path] = result
            if not result["matches"]:
                print(f"FAIL {path}: {result}")
    report = {"version": version, "image_id": image, "results": results}
    if args.report:
        args.report.write_text(json.dumps(report, indent=2) + "\n")
    failures = sum(not result["matches"] for result in results.values())
    print(f"CLIPS 6.30: {len(results) - failures}/{len(results)} reference outputs verified")
    raise SystemExit(bool(failures))


if __name__ == "__main__":
    main()
