"""Guard against vacuous, partial, and noisy CLIPS reference captures."""

import subprocess
from pathlib import Path
from types import SimpleNamespace

import pytest

from ferric_tools.compat import corpus
from ferric_tools.compat.corpus import (
    ReferenceFailure,
    batch_source,
    extract_load_error,
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


def test_scanner_notices_are_output_not_protocol_failures():
    notice = "[SCANNER1] WARNING: Over or underflow of long long integer.\n"
    assert extract_output(f"BEGIN\n{notice}x\nEND\n", "", "BEGIN", "END") == notice + "x\n"
    with pytest.raises(ReferenceFailure, match="runtime diagnostic"):
        extract_output("BEGIN\n[SCANNER1] other\nEND\n", "", "BEGIN", "END")


def test_source_integer_overflow_notice_is_allowed_only_before_output_frame():
    notice = "[SCANNER1] WARNING: Over or underflow of long long integer.\n"
    assert extract_output(f"{notice}{notice}BEGIN\nclamped\nEND\n", "", "BEGIN", "END") == (
        "clamped\n"
    )
    assert extract_output(f"{notice}BEGIN\n{notice}clamped\nEND\n", "", "BEGIN", "END") == (
        notice + "clamped\n"
    )
    with pytest.raises(ReferenceFailure, match="load/protocol diagnostic"):
        extract_output(f"BEGIN\nclamped\nEND\n{notice}", "", "BEGIN", "END")


@pytest.mark.parametrize(
    "notice",
    [
        "[SCANNER1] other\n",
        "[SCANNER1] WARNING: Over or underflow of long long integer. extra\n",
        "\n[SCANNER1] Encountered End-Of-File while scanning a string\n",
    ],
)
def test_other_source_scanner_notices_remain_protocol_failures(notice):
    with pytest.raises(ReferenceFailure, match="load/protocol diagnostic"):
        extract_output(f"{notice}BEGIN\nvalue\nEND\n", "", "BEGIN", "END")


@pytest.mark.parametrize(
    "construct",
    ["deffunction: APP::value", "deftemplate: value", "defrule: value", "defrule: APP::value =j+j"],
)
def test_construct_redefinition_warnings_are_allowed_only_during_load(construct):
    warning = f"[CSTRCPSR1] WARNING: Redefining {construct}\n"
    assert extract_output(f"{warning}BEGIN\nnew\nEND\n", "", "BEGIN", "END") == "new\n"
    with pytest.raises(ReferenceFailure, match="load/protocol diagnostic"):
        extract_output(f"BEGIN\nnew\nEND\n{warning}", "", "BEGIN", "END")
    with pytest.raises(ReferenceFailure, match="runtime diagnostic"):
        extract_output(f"BEGIN\n{warning}new\nEND\n", "", "BEGIN", "END")


@pytest.mark.parametrize(
    "warning",
    [
        "[CSTRCPSR1] WARNING: Redefining deftemplate: value extra\n",
        "[CSTRCPSR1] WARNING: Redefining defrule: value =j+j extra\n",
        "[CSTRCPSR1] WARNING: Redefining defglobal: value\n",
        "[CSTRCPSR1] WARNING: Redefining deffunction: value extra\n",
        "prefix [CSTRCPSR1] WARNING: Redefining deffunction: value\n",
        "[CSTRCPSR1] WARNING: Redefining deffunction: \n",
    ],
)
def test_other_redefinition_warning_shapes_remain_protocol_failures(warning):
    with pytest.raises(ReferenceFailure, match="load/protocol diagnostic"):
        extract_output(f"{warning}BEGIN\nvalue\nEND\n", "", "BEGIN", "END")


def test_literal_bracket_text_inside_output_is_not_a_diagnostic():
    assert extract_output("BEGIN\n[USER123]\nEND\n", "", "BEGIN", "END") == "[USER123]\n"


def test_run_error_cases_require_a_runtime_diagnostic():
    output = "BEGIN\nfirst\n[PRCCODE4] Execution halted.\nEND\n"
    assert (
        extract_output(output, "", "BEGIN", "END", "run") == "first\n[PRCCODE4] Execution halted.\n"
    )
    with pytest.raises(ReferenceFailure, match="expected a CLIPS runtime diagnostic"):
        extract_output("BEGIN\nfirst\nEND\n", "", "BEGIN", "END", "run")
    # Each run step is its own command line: CLIPS drops the rest of a line.
    source = batch_source("tests/clips_compat/corpus/a.clp", "BEGIN", "END", 1, "run")
    assert "(run 1000)\n" in source
    assert all(line.count("(") - line.count(")") == 0 for line in source.splitlines())


def test_load_error_cases_capture_the_rejection_diagnostic():
    source = batch_source("tests/clips_compat/corpus/a.clp", "BEGIN", "END", 1, "load")
    assert '(load* "tests/clips_compat/corpus/a.clp")' in source
    assert "(run" not in source
    diagnostic = "[PRNTUTIL1] Unable to find deftemplate ghost.\n"
    assert extract_load_error(f"BEGIN\n{diagnostic}END\n", "", "BEGIN", "END") == diagnostic
    with pytest.raises(ReferenceFailure, match="loaded a program"):
        extract_load_error("BEGIN\nBEGIN_ACCEPTED\nEND\n", "", "BEGIN", "END")
    with pytest.raises(ReferenceFailure, match="without a diagnostic"):
        extract_load_error("BEGIN\nEND\n", "", "BEGIN", "END")


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
        return subprocess.CompletedProcess(command, 0, stdout=stdout.encode(), stderr=b"")

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


def test_recoverable_fact_notices_require_explicit_success_case_and_stay_in_oracle():
    output = (
        "prefix:[PRNTUTIL1] Unable to find fact f-9.\nFALSE\n"
        "[ARGACCES5] Function fact-slot-value expected argument #1 "
        "to be of type fact-address or fact-index\n"
        "[ARGACCES5] Function retract expected argument #2 "
        "to be of type fact-address, fact-index, or the symbol *\n"
        "[ARGACCES5] Function fact-index expected argument #1 to be of type fact-address\n"
        "-1\ncontinued\n"
    )
    assert (
        extract_output(f"BEGIN\n{output}END\n", "", "BEGIN", "END", recoverable_fact_notices=True)
        == output
    )
    with pytest.raises(ReferenceFailure, match="runtime diagnostic"):
        extract_output(f"BEGIN\n{output}END\n", "", "BEGIN", "END")
    with pytest.raises(ReferenceFailure, match="expected a recoverable"):
        extract_output("BEGIN\nclean\nEND\n", "", "BEGIN", "END", recoverable_fact_notices=True)


@pytest.mark.parametrize(
    "unexpected",
    [
        "[PRNTUTIL1] Unable to find fact f-9. extra\n",
        "[ARGACCES5] Function + expected argument #1 to be of type integer or float\n",
        "[ARGACCES5] Function fact-slot-value expected argument #2 to be of type symbol\n",
        "[ARGACCES5] Function fact-index expected argument #1 to be of type fact-address or "
        "fact-index\n",
        "[ARGACCES5] Function fact-index expected argument #2 to be of type fact-address\n",
        "[PRCCODE4] Execution halted.\n",
    ],
)
def test_recoverable_fact_notice_flag_does_not_hide_fatal_or_changed_diagnostics(unexpected):
    notice = "[PRNTUTIL1] Unable to find fact f-9.\n"
    with pytest.raises(ReferenceFailure, match="runtime diagnostic"):
        extract_output(
            f"BEGIN\n{notice}{unexpected}END\n",
            "",
            "BEGIN",
            "END",
            recoverable_fact_notices=True,
        )


def test_fact_notice_allowance_does_not_extend_outside_program_frame():
    notice = "[PRNTUTIL1] Unable to find fact f-9.\n"
    for stdout in (f"{notice}BEGIN\n{notice}END\n", f"BEGIN\n{notice}END\n{notice}"):
        with pytest.raises(ReferenceFailure, match="load/protocol diagnostic"):
            extract_output(stdout, "", "BEGIN", "END", recoverable_fact_notices=True)


def test_fact_notice_matching_preserves_literal_near_matches():
    output = (
        "[PRNTUTIL1] Unable to find fact f-nine.\n"
        "prefix [PRNTUTIL1] Unable to find fact f-9. extra\n"
    )
    assert corpus.FACT_NOTICE.sub("", output) == output


@pytest.mark.parametrize("value,error", [("true", None), (True, "load"), (True, "run")])
def test_fact_notice_flag_requires_success_and_boolean(tmp_path, value, error):
    with pytest.raises(ReferenceFailure, match="requires a successful run"):
        run_reference(
            tmp_path,
            {"path": "facts/a.clp", "recoverable_fact_notices": value, "error": error},
            "unused",
            1,
        )


def test_control_notices_require_explicit_success_case_and_stay_in_oracle():
    output = (
        "clear:[[CONSTRCT1] Some constructs are still in use. Clear cannot continue.\n]\n"
        "focus:[[PRNTUTIL1] Unable to find defmodule MISSING.\nFALSE]\ncontinued\n"
    )
    assert (
        extract_output(
            f"BEGIN\n{output}END\n", "", "BEGIN", "END", recoverable_control_notices=True
        )
        == output
    )
    with pytest.raises(ReferenceFailure, match="runtime diagnostic"):
        extract_output(f"BEGIN\n{output}END\n", "", "BEGIN", "END")
    with pytest.raises(ReferenceFailure, match="expected a recoverable"):
        extract_output("BEGIN\nclean\nEND\n", "", "BEGIN", "END", recoverable_control_notices=True)


@pytest.mark.parametrize(
    "unexpected",
    [
        "[CONSTRCT1] Some constructs are still in use. Clear cannot continue. extra\n",
        "[PRNTUTIL1] Unable to find defmodule MISSING. extra\n",
        "[PRNTUTIL1] Unable to find deftemplate MISSING.\n",
        "[ARGACCES5] Function focus expected argument #1 to be of type symbol\n",
        "[PRCCODE4] Execution halted.\n",
    ],
)
def test_control_notice_flag_keeps_other_diagnostics_visible(unexpected):
    notice = "[CONSTRCT1] Some constructs are still in use. Clear cannot continue.\n"
    with pytest.raises(ReferenceFailure, match="runtime diagnostic"):
        extract_output(
            f"BEGIN\n{notice}{unexpected}END\n",
            "",
            "BEGIN",
            "END",
            recoverable_control_notices=True,
        )
    assert corpus.CONTROL_NOTICE.sub("", unexpected) == unexpected


def test_control_notice_flag_does_not_allow_load_or_protocol_diagnostics():
    notice = "[CONSTRCT1] Some constructs are still in use. Clear cannot continue.\n"
    for stdout in (f"{notice}BEGIN\n{notice}END\n", f"BEGIN\n{notice}END\n{notice}"):
        with pytest.raises(ReferenceFailure, match="load/protocol diagnostic"):
            extract_output(stdout, "", "BEGIN", "END", recoverable_control_notices=True)


@pytest.mark.parametrize("value,error", [("true", None), (True, "load"), (True, "run")])
def test_control_notice_flag_requires_success_and_boolean(tmp_path, value, error):
    with pytest.raises(ReferenceFailure, match="requires a successful run"):
        run_reference(
            tmp_path,
            {"path": "facts/a.clp", "recoverable_control_notices": value, "error": error},
            "unused",
            1,
        )


@pytest.mark.parametrize("notice", [corpus.RANDOM_NOTICE, corpus.RANDOM_ARITY_NOTICE])
def test_random_notice_requires_opt_in_and_remains_in_captured_output(notice):
    output = f"value:{notice}71876166;continued\n"
    assert (
        extract_output(f"BEGIN\n{output}END\n", "", "BEGIN", "END", recoverable_random_notices=True)
        == output
    )
    with pytest.raises(ReferenceFailure, match="runtime diagnostic"):
        extract_output(f"BEGIN\n{output}END\n", "", "BEGIN", "END")
    with pytest.raises(ReferenceFailure, match="expected a recoverable"):
        extract_output("BEGIN\nclean\nEND\n", "", "BEGIN", "END", recoverable_random_notices=True)


@pytest.mark.parametrize(
    "unexpected",
    [
        corpus.RANDOM_NOTICE.rstrip("\n") + " extra\n",
        corpus.RANDOM_ARITY_NOTICE.rstrip("\n") + " extra\n",
        "[MISCFUN3] different notice\n",
        "[ARGACCES5] Function random expected argument #1 to be of type integer\n",
        "[PRCCODE4] Execution halted.\n",
    ],
)
def test_random_notice_allowance_does_not_mask_fatal_or_changed_messages(unexpected):
    with pytest.raises(ReferenceFailure, match="runtime diagnostic"):
        extract_output(
            f"BEGIN\n{corpus.RANDOM_NOTICE}{unexpected}END\n",
            "",
            "BEGIN",
            "END",
            recoverable_random_notices=True,
        )
    assert unexpected.replace(corpus.RANDOM_NOTICE, "") == unexpected


def test_random_notice_allowance_is_confined_to_execution_frame():
    notice = corpus.RANDOM_NOTICE
    for stdout in (f"{notice}BEGIN\n{notice}END\n", f"BEGIN\n{notice}END\n{notice}"):
        with pytest.raises(ReferenceFailure, match="load/protocol diagnostic"):
            extract_output(stdout, "", "BEGIN", "END", recoverable_random_notices=True)


@pytest.mark.parametrize("value,error", [("true", None), (True, "load"), (True, "run")])
def test_random_notice_flag_requires_success_and_boolean(tmp_path, value, error):
    with pytest.raises(ReferenceFailure, match="requires a successful run"):
        run_reference(
            tmp_path,
            {"path": "stdlib/a.clp", "recoverable_random_notices": value, "error": error},
            "unused",
            1,
        )
