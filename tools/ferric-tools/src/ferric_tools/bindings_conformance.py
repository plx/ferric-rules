"""Run and compare the shared Rust/C/Go/Node/Python semantic corpus."""

from __future__ import annotations

import json
import os
import subprocess
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Annotated, Any

import typer
from rich.console import Console

from ferric_tools._paths import repo_root

REQUIRED_BINDINGS = ("rust", "c", "go", "node", "python")
REQUIRED_SEMANTICS = (
    "value.void",
    "value.integer",
    "value.float",
    "value.symbol",
    "value.string",
    "value.multifield",
    "value.external_address",
    "configuration.default",
    "configuration.custom",
    "configuration.isolation",
    "error.parse",
    "error.compile",
    "error.runtime",
    "fact.lifecycle",
    "execution.run",
    "execution.halt",
    "execution.diagnostic",
    "execution.step",
    "snapshot",
    "lifecycle.close",
    "robustness.nul",
    "identifier.high",
    "count.high",
)

app = typer.Typer(help="Compare every supported binding against one semantic corpus.")
console = Console(stderr=True)


class CorpusError(ValueError):
    """The corpus or an adapter report violates the conformance protocol."""


@dataclass(frozen=True)
class Deviation:
    """An exact, temporary or intentional binding-specific result."""

    expected: Any
    rationale: str
    since: str
    tracking_issue: str


@dataclass(frozen=True)
class Case:
    """One language-neutral semantic case."""

    id: str
    semantic: str
    canonical: Any
    deviations: dict[str, Deviation]
    required_bindings: frozenset[str]


@dataclass(frozen=True)
class Corpus:
    """Validated corpus metadata and cases."""

    schema_version: int
    suite_version: str
    bindings: tuple[str, ...]
    cases: tuple[Case, ...]


def _require_string(value: object, label: str) -> str:
    if not isinstance(value, str) or not value:
        raise CorpusError(f"{label} must be a non-empty string")
    return value


def load_corpus(path: Path) -> Corpus:
    """Load and strictly validate a cross-binding corpus."""

    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise CorpusError(f"cannot load corpus {path}: {error}") from error

    if not isinstance(raw, dict):
        raise CorpusError("corpus root must be an object")
    if raw.get("schema_version") != 1:
        raise CorpusError("corpus schema_version must be 1")

    suite_version = _require_string(raw.get("suite_version"), "suite_version")
    bindings_raw = raw.get("bindings")
    if not isinstance(bindings_raw, list) or not all(
        isinstance(binding, str) for binding in bindings_raw
    ):
        raise CorpusError("bindings must be a string array")
    bindings = tuple(bindings_raw)
    if len(bindings) != len(set(bindings)):
        raise CorpusError("bindings must be unique")

    cases_raw = raw.get("cases")
    if not isinstance(cases_raw, list) or not cases_raw:
        raise CorpusError("cases must be a non-empty array")

    cases: list[Case] = []
    for index, item in enumerate(cases_raw):
        label = f"cases[{index}]"
        if not isinstance(item, dict):
            raise CorpusError(f"{label} must be an object")
        case_id = _require_string(item.get("id"), f"{label}.id")
        semantic = _require_string(item.get("semantic"), f"{label}.semantic")
        if "canonical" not in item:
            raise CorpusError(f"{label}.canonical is required")

        required_raw = item.get("required_bindings", list(bindings))
        if not isinstance(required_raw, list) or not all(
            isinstance(binding, str) for binding in required_raw
        ):
            raise CorpusError(f"{label}.required_bindings must be a string array")
        required = frozenset(required_raw)
        unknown_required = required - set(bindings)
        if unknown_required:
            raise CorpusError(
                f"{label}.required_bindings contains unknown bindings: {sorted(unknown_required)}"
            )

        deviations_raw = item.get("deviations", {})
        if not isinstance(deviations_raw, dict):
            raise CorpusError(f"{label}.deviations must be an object")
        deviations: dict[str, Deviation] = {}
        for binding, deviation_raw in deviations_raw.items():
            if binding not in bindings:
                raise CorpusError(f"{label}.deviations has unknown binding {binding!r}")
            if not isinstance(deviation_raw, dict) or "expected" not in deviation_raw:
                raise CorpusError(f"{label}.deviations.{binding} must include expected")
            deviations[binding] = Deviation(
                expected=deviation_raw["expected"],
                rationale=_require_string(
                    deviation_raw.get("rationale"),
                    f"{label}.deviations.{binding}.rationale",
                ),
                since=_require_string(
                    deviation_raw.get("since"),
                    f"{label}.deviations.{binding}.since",
                ),
                tracking_issue=_require_string(
                    deviation_raw.get("tracking_issue"),
                    f"{label}.deviations.{binding}.tracking_issue",
                ),
            )

        cases.append(
            Case(
                id=case_id,
                semantic=semantic,
                canonical=item["canonical"],
                deviations=deviations,
                required_bindings=required,
            )
        )

    case_ids = [case.id for case in cases]
    if len(case_ids) != len(set(case_ids)):
        raise CorpusError("case IDs must be unique")

    return Corpus(
        schema_version=1,
        suite_version=suite_version,
        bindings=bindings,
        cases=tuple(cases),
    )


@dataclass(frozen=True)
class AdapterCommand:
    """How to invoke one adapter from the repository root."""

    argv: tuple[str, ...]
    cwd: Path


def _default_adapter_commands(root: Path) -> dict[str, AdapterCommand]:
    return {
        "rust": AdapterCommand(
            (
                "cargo",
                "run",
                "--quiet",
                "--release",
                "-p",
                "ferric-bindings-conformance-adapter",
                "--",
            ),
            root,
        ),
        "c": AdapterCommand(
            (str(root / "target" / "bindings-conformance" / "c-adapter"),),
            root,
        ),
        "go": AdapterCommand(
            ("go", "run", "./cmd/bindings-conformance"),
            root / "bindings" / "go",
        ),
        "node": AdapterCommand(
            ("node", "--import", "tsx", "test/bindings-conformance/adapter.ts"),
            root / "packages" / "ferric",
        ),
        "python": AdapterCommand(
            (
                str(root / "crates" / "ferric-rules-python" / ".venv" / "bin" / "python"),
                "tests/bindings_conformance_adapter.py",
            ),
            root / "crates" / "ferric-rules-python",
        ),
    }


def _parse_adapter_output(binding: str, stdout: str, expected_ids: set[str]) -> dict[str, Any]:
    observations: dict[str, Any] = {}
    for line_number, line in enumerate(stdout.splitlines(), start=1):
        if not line.strip():
            continue
        try:
            record = json.loads(line)
        except json.JSONDecodeError as error:
            raise CorpusError(
                f"{binding} adapter emitted invalid JSON on line {line_number}: {error}"
            ) from error
        if not isinstance(record, dict) or set(record) != {"case", "result"}:
            raise CorpusError(
                f"{binding} adapter line {line_number} must contain only case and result"
            )
        case_id = record["case"]
        if not isinstance(case_id, str) or case_id not in expected_ids:
            raise CorpusError(f"{binding} adapter emitted unknown case {case_id!r}")
        if case_id in observations:
            raise CorpusError(f"{binding} adapter emitted duplicate case {case_id!r}")
        observations[case_id] = record["result"]

    missing = expected_ids - set(observations)
    if missing:
        raise CorpusError(f"{binding} adapter omitted cases: {sorted(missing)}")
    return observations


def run_adapter(
    binding: str,
    command: AdapterCommand,
    case_ids_path: Path,
    root: Path,
) -> dict[str, Any]:
    """Run one adapter and return its normalized observations."""

    env = os.environ.copy()
    env["FERRIC_BINDINGS_CONFORMANCE_ROOT"] = str(root)
    completed = subprocess.run(
        [*command.argv, str(case_ids_path)],
        cwd=command.cwd,
        env=env,
        check=False,
        capture_output=True,
        text=True,
    )
    if completed.returncode != 0:
        details = completed.stderr.strip() or completed.stdout.strip() or "(no output)"
        raise CorpusError(f"{binding} adapter exited {completed.returncode}:\n{details}")

    expected_ids = set(case_ids_path.read_text(encoding="utf-8").splitlines())
    return _parse_adapter_output(binding, completed.stdout, expected_ids)


def compare_observations(
    corpus: Corpus, observations: dict[str, dict[str, Any]]
) -> tuple[list[str], list[str]]:
    """Return exact failures and accepted-deviation descriptions."""

    failures: list[str] = []
    accepted: list[str] = []
    for case in corpus.cases:
        for binding in sorted(case.required_bindings):
            actual = observations[binding][case.id]
            deviation = case.deviations.get(binding)
            if deviation is None:
                if actual != case.canonical:
                    failures.append(
                        f"{case.id}/{binding}: expected canonical "
                        f"{json.dumps(case.canonical, sort_keys=True)}, got "
                        f"{json.dumps(actual, sort_keys=True)}"
                    )
                continue

            if actual == case.canonical:
                failures.append(
                    f"{case.id}/{binding}: now matches canonical; remove the stale deviation"
                )
            elif actual != deviation.expected:
                failures.append(
                    f"{case.id}/{binding}: expected versioned deviation "
                    f"{json.dumps(deviation.expected, sort_keys=True)}, got "
                    f"{json.dumps(actual, sort_keys=True)}"
                )
            else:
                accepted.append(
                    f"{case.id}/{binding} ({deviation.since}, {deviation.tracking_issue})"
                )
    return failures, accepted


@app.command()
def main(
    corpus_path: Annotated[
        Path | None,
        typer.Option("--corpus", help="Path to the language-neutral corpus."),
    ] = None,
) -> None:
    """Run all adapters and reject unknown or stale semantic drift."""

    root = repo_root()
    path = corpus_path or root / "tests" / "bindings-conformance" / "corpus.json"
    try:
        corpus = load_corpus(path)
        if set(corpus.bindings) != set(REQUIRED_BINDINGS):
            raise CorpusError(
                f"corpus bindings must be exactly {list(REQUIRED_BINDINGS)}, "
                f"got {list(corpus.bindings)}"
            )
        missing_semantics = set(REQUIRED_SEMANTICS) - {case.semantic for case in corpus.cases}
        if missing_semantics:
            raise CorpusError(f"corpus omits required semantics: {sorted(missing_semantics)}")

        commands = _default_adapter_commands(root)
        observations: dict[str, dict[str, Any]] = {}
        with tempfile.TemporaryDirectory(prefix="ferric-bindings-conformance-") as temp_dir:
            ids_path = Path(temp_dir) / "case-ids.txt"
            ids_path.write_text(
                "".join(f"{case.id}\n" for case in corpus.cases),
                encoding="utf-8",
            )
            for binding in corpus.bindings:
                console.print(f"[cyan]running {binding} adapter[/]")
                observations[binding] = run_adapter(binding, commands[binding], ids_path, root)

        failures, accepted = compare_observations(corpus, observations)
        if failures:
            for failure in failures:
                console.print(f"[red]FAIL[/] {failure}")
            raise typer.Exit(1)

        console.print(
            f"[green]bindings conformance passed[/]: {len(corpus.cases)} cases, "
            f"{len(corpus.bindings)} adapters, {len(accepted)} versioned deviations"
        )
        for deviation in accepted:
            console.print(f"[yellow]DEVIATION[/] {deviation}")
    except CorpusError as error:
        console.print(f"[red]error:[/] {error}")
        raise typer.Exit(1) from error


if __name__ == "__main__":
    app()
