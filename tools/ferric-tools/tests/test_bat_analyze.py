"""Tests for ferric_tools.bat.analyze and ferric_tools.bat.extract.

bat.analyze.split_into_cycles(text) splits a .bat file's text into
(clear)-delimited cycles.  Each cycle is a dict with keys:
  - "label":  str | None — comment text found after (clear), if any
  - "forms":  list[tuple[form_text: str, kw: str]]

bat.extract.split_into_cycles(text) has the same delimiter semantics but
returns list[list[tuple[str, str]]] — no label, just the forms.

bat.extract.extract_cycle(cycle_forms) strips control/watch commands and
returns the remaining forms joined with newlines, or None if nothing is left.
"""

from __future__ import annotations

# ---------------------------------------------------------------------------
# bat.analyze — cycle splitting
# ---------------------------------------------------------------------------
from ferric_tools.bat.analyze import classify_keyword  # re-exported via _clips_parser
from ferric_tools.bat.analyze import split_into_cycles as analyze_split_into_cycles

# ---------------------------------------------------------------------------
# bat.extract — cycle splitting and content extraction
# ---------------------------------------------------------------------------
from ferric_tools.bat.extract import extract_cycle
from ferric_tools.bat.extract import split_into_cycles as extract_split_into_cycles

# ===========================================================================
# bat.analyze.split_into_cycles
# ===========================================================================


def test_analyze_split_single_cycle_no_clear():
    # A batch file with no (clear) is a single implicit cycle.
    text = "(defrule my-rule (fact) => nil)\n(reset)\n(run)"
    cycles = analyze_split_into_cycles(text)

    assert len(cycles) == 1
    keywords = [kw for _ft, kw in cycles[0]["forms"]]
    assert "defrule" in keywords


def test_analyze_split_two_cycles_separated_by_clear():
    # (clear) is the cycle delimiter; content on each side forms one cycle.
    text = "(deffacts init)\n(run)\n(clear)\n(defrule r => nil)\n(run)"
    cycles = analyze_split_into_cycles(text)

    # We expect at least 2 cycles: one before (clear) and one after.
    assert len(cycles) >= 2


def test_analyze_split_clear_label_captured():
    # A semicolon comment on the same line as (clear) becomes the cycle label.
    text = "(run)\n(clear) ; test: basic-scenario\n(defrule r => nil)\n(run)"
    cycles = analyze_split_into_cycles(text)

    # The cycle after the labelled (clear) should carry the label text.
    labelled_cycles = [c for c in cycles if c["label"] is not None]
    assert labelled_cycles, "Expected at least one cycle with a label"
    assert "basic-scenario" in labelled_cycles[0]["label"]


def test_analyze_split_forms_contain_keyword():
    # Every (form_text, kw) pair in a cycle's forms should have kw equal to
    # the first keyword of form_text (lowercased).
    text = "(deftemplate person (slot name))\n(defrule greet => nil)"
    cycles = analyze_split_into_cycles(text)

    for cycle in cycles:
        for form_text, kw in cycle["forms"]:
            assert form_text.lstrip().startswith("(")
            assert kw == kw.lower()


def test_analyze_split_empty_text_returns_empty():
    assert analyze_split_into_cycles("") == []


def test_analyze_split_only_clear_returns_empty_or_single_empty():
    # A file containing only (clear) has no meaningful content.
    text = "(clear)"
    cycles = analyze_split_into_cycles(text)
    # All cycles should have empty form lists (or no cycles at all).
    non_empty = [c for c in cycles if c["forms"]]
    assert non_empty == []


# ===========================================================================
# bat.analyze — keyword classification (re-exported from _clips_parser)
# ===========================================================================


def test_bat_classify_keyword_construct():
    # classify_keyword is imported into bat.analyze from _clips_parser.
    from ferric_tools._clips_parser import classify_keyword as direct_classify

    assert classify_keyword("defrule") == direct_classify("defrule") == "construct"


def test_bat_classify_keyword_control():
    assert classify_keyword("reset") == "control"


def test_bat_classify_keyword_repl():
    assert classify_keyword("assert") == "repl"


# ===========================================================================
# bat.extract.split_into_cycles
# ===========================================================================


def test_extract_split_returns_list_of_lists():
    text = "(defrule r => nil)\n(reset)\n(run)"
    cycles = extract_split_into_cycles(text)

    assert isinstance(cycles, list)
    for cycle in cycles:
        assert isinstance(cycle, list)
        for form_text, kw in cycle:
            assert isinstance(form_text, str)
            assert isinstance(kw, str)


def test_extract_split_clear_is_the_delimiter():
    # (clear) itself must NOT appear as a form in any cycle — it is the
    # delimiter, consumed by the splitter.
    text = "(defrule a => nil)\n(clear)\n(defrule b => nil)"
    cycles = extract_split_into_cycles(text)

    for cycle in cycles:
        for _form_text, kw in cycle:
            assert kw != "clear", "(clear) must not appear as a form inside a cycle"


def test_extract_split_two_clears_give_three_buckets():
    # Two (clear) delimiters can produce up to three content buckets.
    text = "(defrule a => nil)\n(clear)\n(defrule b => nil)\n(clear)\n(defrule c => nil)"
    cycles = extract_split_into_cycles(text)

    # At minimum the non-empty buckets should contain forms for a, b, c.
    non_empty = [c for c in cycles if c]
    assert len(non_empty) == 3


def test_extract_split_empty_input():
    assert extract_split_into_cycles("") == []


# ===========================================================================
# bat.extract.extract_cycle
# ===========================================================================


def test_extract_cycle_strips_watch_and_reset_and_run():
    # watch, unwatch, reset, and run are in STRIP_KEYWORDS and must not appear
    # in the extracted output.
    cycle_forms = [
        ("(deftemplate person (slot name))", "deftemplate"),
        ("(watch facts)", "watch"),
        ("(reset)", "reset"),
        ("(run)", "run"),
        ("(defrule greet => nil)", "defrule"),
    ]
    result = extract_cycle(cycle_forms)

    assert result is not None
    assert "watch" not in result
    assert "reset" not in result
    assert "(run)" not in result
    assert "deftemplate" in result
    assert "defrule" in result


def test_extract_cycle_returns_none_when_all_forms_stripped():
    # If every form in the cycle is a strip-keyword command, extract_cycle
    # returns None (nothing worth writing to a .clp file).
    cycle_forms = [
        ("(watch facts)", "watch"),
        ("(reset)", "reset"),
        ("(run)", "run"),
    ]
    result = extract_cycle(cycle_forms)

    assert result is None


def test_extract_cycle_trailing_newline():
    # The returned string must end with a newline so written .clp files are
    # POSIX-compliant (no missing final newline).
    cycle_forms = [("(defrule r => nil)", "defrule")]
    result = extract_cycle(cycle_forms)

    assert result is not None
    assert result.endswith("\n")


def test_extract_cycle_empty_forms_list():
    # An empty forms list means there is nothing to extract.
    result = extract_cycle([])

    assert result is None
