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


# CLIPS writes these scanner notices for recoverable input problems and keeps
# the scanned value. The Rust runner compares them with Ferric's routers.
SCANNER_NOTICES = (
    "[SCANNER1] WARNING: Over or underflow of long long integer.\n",
    "\n[SCANNER1] Encountered End-Of-File while scanning a string\n",
)


def decode(data: bytes) -> str:
    """Decode CLIPS bytes, keeping any invalid UTF-8 byte-for-byte."""
    return data.decode("utf-8", errors="surrogateescape")


DIAGNOSTIC = re.compile(r"(?m)^\[[A-Z]+\d+\][ \t]")
FACT_NOTICE = re.compile(
    r"\[PRNTUTIL1\] Unable to find fact f-[0-9]+\.\n"
    r"|\[ARGACCES5\] Function (?:fact-existp|fact-relation|fact-slot-names|fact-slot-value) "
    r"expected argument #1 to be of type fact-address or fact-index\n"
    r"|\[ARGACCES5\] Function fact-index expected argument #1 to be of type fact-address\n"
    r"|\[ARGACCES5\] Function retract expected argument #[1-9][0-9]* "
    r"to be of type fact-address, fact-index, or the symbol \*\n"
)
CONTROL_NOTICE = re.compile(
    r"\[CONSTRCT1\] Some constructs are still in use\. Clear cannot continue\.\n"
    r"|\[PRNTUTIL1\] Unable to find defmodule [A-Za-z0-9_:-]+\.\n"
)
RANDOM_NOTICE = "[MISCFUN3] Function random expected argument #1 to be less than argument #2\n"
RANDOM_ARITY_NOTICE = "[MISCFUN2] Function random expected either 0 or 2 arguments\n"
# A build that would redefine a deftemplate in use: the CSTRCPSR4 message and
# the parser's echo of the construct up to its module-qualified name.
BUILD_NOTICE = re.compile(
    r"\n\[CSTRCPSR4\] Cannot redefine deftemplate ([A-Za-z0-9_-]+) while it is in use\.\n"
    r"\nERROR:\n\(deftemplate [A-Za-z0-9_-]+::\1\n"
)


def extract_output(
    stdout: str,
    stderr: str,
    begin: str,
    end: str,
    error: str | None = None,
    recoverable_fact_notices: bool = False,
    recoverable_control_notices: bool = False,
    recoverable_random_notices: bool = False,
    recoverable_build_notices: bool = False,
) -> str:
    """Require exactly one complete frame and check reference diagnostics.

    Only a case that declares a CLIPS run-time error may print diagnostics in
    its frame, and it must print at least one.
    """
    if stderr.strip():
        raise ReferenceFailure(f"CLIPS diagnostics on stderr:\n{stderr}")
    if stdout.count(begin + "\n") != 1 or stdout.count(end + "\n") != 1:
        raise ReferenceFailure(f"missing/duplicate CLIPS output markers:\n{stdout}")
    prefix, remainder = stdout.split(begin + "\n", 1)
    output, suffix = remainder.split(end + "\n", 1)
    # A fresh CLIPS environment already contains MAIN; declaring its imports
    # legitimately emits this one specific warning even when load succeeds.
    preamble = prefix.replace("[CSTRCPSR1] WARNING: Redefining defmodule: MAIN\n", "")
    # Source integers outside the signed 64-bit range are clamped by CLIPS.
    # Ferric's source lexer clamps silently; only allow this exact load notice.
    # Notices within the execution frame remain part of the returned oracle.
    preamble = preamble.replace(SCANNER_NOTICES[0], "")
    # Redefinition fixtures deliberately replace construct definitions
    # before reset. Only their complete load-warning lines are expected.
    preamble = re.sub(
        r"(?m)^\[CSTRCPSR1\] WARNING: Redefining (?:deffunction|deftemplate): [^\s]+\n"
        r"|^\[CSTRCPSR1\] WARNING: Redefining defrule: [^\s]+(?: (?:[+=][aj])+)?\n",
        "",
        preamble,
    )
    if re.search(r"\[[A-Z]+\d+\]", preamble + suffix):
        raise ReferenceFailure(f"CLIPS load/protocol diagnostic:\n{prefix}{suffix}")
    # The Debian CLIPS executable also writes runtime errors to stdout. Reserve
    # its diagnostic-code-plus-message syntax; a literal [USER123] stays valid.
    checked = output
    for notice in SCANNER_NOTICES:
        checked = checked.replace(notice, "")
    if recoverable_fact_notices:
        checked, count = FACT_NOTICE.subn("", checked)
        if count == 0:
            raise ReferenceFailure("expected a recoverable CLIPS fact notice")
    if recoverable_control_notices:
        checked, count = CONTROL_NOTICE.subn("", checked)
        if count == 0:
            raise ReferenceFailure("expected a recoverable CLIPS control notice")
    if recoverable_random_notices:
        if not any(notice in checked for notice in (RANDOM_NOTICE, RANDOM_ARITY_NOTICE)):
            raise ReferenceFailure("expected a recoverable CLIPS random notice")
        checked = checked.replace(RANDOM_NOTICE, "")
        checked = checked.replace(RANDOM_ARITY_NOTICE, "")
    if recoverable_build_notices:
        checked, count = BUILD_NOTICE.subn("", checked)
        if count == 0:
            raise ReferenceFailure("expected a recoverable CLIPS build notice")
    if error == "run" and not DIAGNOSTIC.search(checked):
        raise ReferenceFailure(f"expected a CLIPS runtime diagnostic:\n{output}")
    # Unanchored, unlike DIAGNOSTIC: a diagnostic printed after other text on
    # the same line still disqualifies a case without a declared error.
    if error != "run" and re.search(r"\[[A-Z]+\d+\][ \t]", checked):
        raise ReferenceFailure(f"CLIPS runtime diagnostic:\n{output}")
    return output


def extract_load_error(stdout: str, stderr: str, begin: str, end: str) -> str:
    """Return the diagnostic CLIPS prints when it rejects the program."""
    if stderr.strip():
        raise ReferenceFailure(f"CLIPS diagnostics on stderr:\n{stderr}")
    if stdout.count(begin + "\n") != 1 or stdout.count(end + "\n") != 1:
        raise ReferenceFailure(f"missing/duplicate CLIPS output markers:\n{stdout}")
    output = stdout.split(begin + "\n", 1)[1].split(end + "\n", 1)[0]
    if f"{begin}_ACCEPTED" in output:
        raise ReferenceFailure(f"CLIPS loaded a program that expects a load error:\n{output}")
    if not DIAGNOSTIC.search(output):
        raise ReferenceFailure(f"CLIPS rejected the program without a diagnostic:\n{output}")
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


def batch_source(path: str, begin: str, end: str, resets: int, error: str | None = None) -> str:
    """Use load's boolean result so partial loads cannot produce an oracle."""
    # Paths are relative, corpus-controlled POSIX names, never CLIPS expressions.
    if not re.fullmatch(r"[a-zA-Z0-9_./-]+", path) or ".." in Path(path).parts:
        raise ReferenceFailure(f"invalid fixture path: {path!r}")
    if not 1 <= resets <= 3:
        raise ReferenceFailure("resets must be between 1 and 3")
    if error == "load":
        # load* prints no progress characters, so the frame holds only the
        # diagnostic; a successful load marks the frame as a failure.
        return (
            f'(printout t "{begin}" crlf)\n'
            f'(if (load* "{path}") then (printout t "{begin}_ACCEPTED" crlf))\n'
            f'(printout t "{end}" crlf)\n(exit)\n'
        )
    runs = "\n".join(
        f'(reset) (printout t "{begin}_RUN_{index}" crlf)\n'
        f"(watch statistics) (run {RUN_LIMIT}) (unwatch statistics)\n"
        f'(printout t "{end}_RUN_{index}" crlf)'
        for index in range(resets)
    )
    if error == "run":
        # A run-time error abandons the rest of an enclosing call, so each
        # step is a top-level command on its own line (CLIPS drops the rest
        # of a command line). A failed load still omits BEGIN.
        steps = "".join(
            f'(reset)\n(printout t "{begin}_RUN_{index}" crlf)\n(watch statistics)\n'
            f'(run {RUN_LIMIT})\n(unwatch statistics)\n(printout t "{end}_RUN_{index}" crlf)\n'
            for index in range(resets)
        )
        return (
            f'(if (load "{path}") then (printout t "{begin}" crlf))\n'
            f'{steps}(printout t "{end}" crlf)\n(exit)\n'
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
    error = case.get("error")
    if error not in (None, "load", "run"):
        raise ReferenceFailure(f"unknown error phase: {error!r}")
    if error == "load" and (resets != 1 or input_path.exists()):
        raise ReferenceFailure("a load error case has no runs")
    recoverable_fact_notices = case.get("recoverable_fact_notices", False)
    if not isinstance(recoverable_fact_notices, bool) or (
        recoverable_fact_notices and error is not None
    ):
        raise ReferenceFailure("recoverable_fact_notices requires a successful run")
    recoverable_control_notices = case.get("recoverable_control_notices", False)
    if not isinstance(recoverable_control_notices, bool) or (
        recoverable_control_notices and error is not None
    ):
        raise ReferenceFailure("recoverable_control_notices requires a successful run")
    recoverable_random_notices = case.get("recoverable_random_notices", False)
    if not isinstance(recoverable_random_notices, bool) or (
        recoverable_random_notices and error is not None
    ):
        raise ReferenceFailure("recoverable_random_notices requires a successful run")
    recoverable_build_notices = case.get("recoverable_build_notices", False)
    if not isinstance(recoverable_build_notices, bool) or (
        recoverable_build_notices and error is not None
    ):
        raise ReferenceFailure("recoverable_build_notices requires a successful run")
    source = batch_source(f"tests/clips_compat/corpus/{case['path']}", begin, end, resets, error)
    strategy = case.get("strategy")
    if strategy not in (None, "breadth"):
        raise ReferenceFailure(f"unknown strategy: {strategy!r}")
    if strategy:
        source = f"(set-strategy {strategy})\n{source}"
    with tempfile.NamedTemporaryFile(mode="w", suffix=".clp", dir=scratch) as batch:
        batch.write(source)
        batch.flush()
        # NamedTemporaryFile defaults to 0600. Once every container capability
        # is dropped, root cannot read a native Linux bind mount owned by the
        # host runner. Expose only this generated control file, read-only.
        Path(batch.name).chmod(0o444)
        command = [
            "docker",
            "run",
            "--rm",
            "-i",
            "--network",
            "none",
            "--read-only",
            "--cap-drop",
            "ALL",
            "--security-opt",
            "no-new-privileges",
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
            input_bytes = input_path.read_bytes() if input_path.exists() else b""
            process = subprocess.run(
                command, input=input_bytes, capture_output=True, timeout=timeout
            )
        except subprocess.TimeoutExpired as error:
            # Killing the Docker client alone can leave the container running.
            subprocess.run(
                ["docker", "rm", "-f", name], capture_output=True, check=False, timeout=10
            )
            raise ReferenceFailure(f"reference timed out after {timeout}s") from error
    stdout, stderr = decode(process.stdout), decode(process.stderr)
    if process.returncode:
        raise ReferenceFailure(f"CLIPS exit {process.returncode}: {stderr}")
    if error == "load":
        return extract_load_error(stdout, stderr, begin, end)
    output = extract_output(
        stdout,
        stderr,
        begin,
        end,
        error,
        recoverable_fact_notices,
        recoverable_control_notices,
        recoverable_random_notices,
        recoverable_build_notices,
    )
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
            expected = decode((corpus / case["path"]).with_suffix(".out").read_bytes())
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
