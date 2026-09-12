"""Guard against vacuous, partial, and noisy CLIPS reference captures."""

import subprocess
from pathlib import Path
from types import SimpleNamespace

import pytest

from ferric_tools.compat import corpus
from ferric_tools.compat.corpus import (
    ReferenceFailure,
    batch_source,
    extract_output,
    extract_runs,
    run_reference,
)


def test_preserves_exact_output_including_significant_whitespace():
    assert (
        extract_output("Defining defrule: go +j\nBEGIN\n a  b\n\nEND\n", "", "BEGIN", "END")
        == " a  b\n\n"
    )


@pytest.mark.parametrize("stdout", ["", "BEGIN\nx\n", "END\n", "BEGIN\nBEGIN\nx\nEND\n"])
def test_requires_complete_unique_frame(stdout):
    with pytest.raises(ReferenceFailure):
        extract_output(stdout, "", "BEGIN", "END")


def test_rejects_clips_diagnostics_even_on_successful_process_exit():
    with pytest.raises(ReferenceFailure, match="stderr"):
        extract_output("BEGIN\nEND\n", "[ARGACCES5] wrong type", "BEGIN", "END")
    with pytest.raises(ReferenceFailure, match="diagnostic"):
        extract_output("[PRNTUTIL2] syntax\nBEGIN\nEND\n", "", "BEGIN", "END")
    with pytest.raises(ReferenceFailure, match="runtime diagnostic"):
        extract_output("BEGIN\n[PRNTUTIL7] divide by zero\nEND\n", "", "BEGIN", "END")


def test_literal_bracket_text_inside_output_is_not_a_diagnostic():
    assert extract_output("BEGIN\n[USER123]\nEND\n", "", "BEGIN", "END") == "[USER123]\n"


def test_batch_checks_load_and_bounds_each_reset_run():
    source = batch_source("tests/clips_compat/corpus/facts/one.clp", "BEGIN", "END", 2)
    assert source.startswith('(if (load "tests/clips_compat/corpus/facts/one.clp") then')
    assert source.count("(reset)") == 2
    assert source.count("(watch statistics) (run 1000) (unwatch statistics)") == 2
    for index in range(2):
        assert f'"BEGIN_RUN_{index}"' in source
        assert f'"END_RUN_{index}"' in source
    assert source.endswith("(exit)\n")


@pytest.mark.parametrize("path", ['bad"path.clp', "../outside.clp", "x/(exit).clp"])
def test_rejects_path_injection(path):
    with pytest.raises(ReferenceFailure):
        batch_source(path, "BEGIN", "END", 1)


def framed_run(output, count=1, index=0, runtime=""):
    return (
        f"BEGIN_RUN_{index}\n{output}{count} rules fired{runtime}\n"
        "1 mean number of facts (2 maximum).\n"
        "1 mean number of instances (1 maximum).\n"
        "1 mean number of activations (2 maximum).\n"
        f"END_RUN_{index}\n"
    )


def test_statistics_preserve_output_and_handle_multiple_reset_runs():
    output = framed_run(" a  b\n\n", count=999) + framed_run("second\n", index=1)
    assert extract_runs(output, "BEGIN", "END", 2) == " a  b\n\nsecond\n"


def test_statistics_accept_timing_suffix_and_zero_firings():
    output = framed_run("", count=0, runtime="        Run time is 0.01 seconds.")
    assert extract_runs(output, "BEGIN", "END", 1) == ""


@pytest.mark.parametrize("count", [1000, 1001])
def test_reference_cannot_bless_a_truncated_output_prefix(count):
    with pytest.raises(ReferenceFailure, match="firing limit"):
        extract_runs(framed_run("expected prefix\n", count=count), "BEGIN", "END", 1)


@pytest.mark.parametrize(
    "output",
    [
        "BEGIN_RUN_0\nexpected\nEND_RUN_0\n",
        framed_run("missing newline"),
        framed_run("expected\n") + "unexpected\n",
        framed_run("expected\n") + framed_run("duplicate\n"),
        framed_run("expected\n", index=1),
    ],
)
def test_rejects_missing_or_malformed_statistics_protocol(output):
    with pytest.raises(ReferenceFailure):
        extract_runs(output, "BEGIN", "END", 1)


def test_rejects_input_replay_without_starting_reference(tmp_path):
    input_path = tmp_path / "tests/clips_compat/corpus/io/read.in"
    input_path.parent.mkdir(parents=True)
    input_path.write_text("hello\n")
    with pytest.raises(ReferenceFailure, match="exactly one reset"):
        run_reference(tmp_path, {"path": "io/read.clp", "resets": 2}, "unused", 1)


def test_reference_container_is_isolated_and_can_read_generated_batch(tmp_path, monkeypatch):
    token = "a" * 32
    monkeypatch.setattr(corpus.uuid, "uuid4", lambda: SimpleNamespace(hex=token))
    commands = []
    batch_modes = []

    def run(command, **kwargs):
        commands.append(command)
        batch_modes.append((tmp_path / Path(command[-1])).stat().st_mode & 0o777)
        begin = f"CORPUS_BEGIN_{token}"
        end = f"CORPUS_END_{token}"
        framed = framed_run("", index=0).replace("BEGIN", begin).replace("END", end)
        stdout = f"{begin}\n{framed}{end}\n"
        return subprocess.CompletedProcess(command, 0, stdout=stdout, stderr="")

    monkeypatch.setattr(corpus.subprocess, "run", run)

    assert run_reference(tmp_path, {"path": "facts/basic.clp"}, "clips-image", 1) == ""
    command = commands[0]
    assert command[:4] == ["docker", "run", "--rm", "-i"]
    assert command[command.index("--network") : command.index("--network") + 2] == [
        "--network",
        "none",
    ]
    assert "--read-only" in command
    assert command[command.index("--cap-drop") : command.index("--cap-drop") + 2] == [
        "--cap-drop",
        "ALL",
    ]
    assert command[command.index("--security-opt") : command.index("--security-opt") + 2] == [
        "--security-opt",
        "no-new-privileges",
    ]
    assert command[command.index("-v") + 1].endswith(":/workspace:ro")
    assert batch_modes == [0o444]
