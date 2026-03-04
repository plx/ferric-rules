"""Tests for ferric_tools._clips_parser.

Covers strip_comments(), extract_top_level_forms(), first_keyword(),
detect_features(), and classify_keyword().
"""

from __future__ import annotations

from ferric_tools._clips_parser import (
    classify_keyword,
    detect_features,
    extract_top_level_forms,
    first_keyword,
    strip_comments,
)

# ---------------------------------------------------------------------------
# strip_comments
# ---------------------------------------------------------------------------


def test_strip_comments_removes_inline_comment():
    # Everything from the first unquoted semicolon to end of line is a comment.
    result = strip_comments("foo ; bar")
    assert result == "foo "


def test_strip_comments_full_line_comment_becomes_empty():
    # A line whose first non-whitespace character is ";" is a comment line.
    # strip_comments replaces it with an empty string (preserving line count).
    result = strip_comments(";comment\ncode")
    lines = result.split("\n")
    assert lines[0] == ""
    assert lines[1] == "code"


def test_strip_comments_does_not_strip_semicolon_inside_string():
    # A semicolon inside a double-quoted string literal is part of the string,
    # not a comment delimiter.
    result = strip_comments('(assert (msg "hello; world"))')
    assert "hello; world" in result


def test_strip_comments_multiline_preserves_line_count():
    # The number of output lines must equal the number of input lines so that
    # downstream tools can correlate output to source line numbers.
    source = "line1\n; full comment\nline3"
    result = strip_comments(source)
    assert result.count("\n") == source.count("\n")


def test_strip_comments_empty_input():
    assert strip_comments("") == ""


# ---------------------------------------------------------------------------
# extract_top_level_forms
# ---------------------------------------------------------------------------


def test_extract_top_level_forms_single_form():
    # A simple top-level form is returned as a one-element list.
    text = "(defrule foo)"
    forms = extract_top_level_forms(text)
    assert len(forms) == 1
    form_text, start_offset = forms[0]
    assert form_text == "(defrule foo)"
    assert start_offset == 0


def test_extract_top_level_forms_nested_parens_handled():
    # Nested parentheses must not be mistaken for the end of the top-level form.
    text = "(defrule foo (bar (baz)))"
    forms = extract_top_level_forms(text)
    assert len(forms) == 1
    assert forms[0][0] == "(defrule foo (bar (baz)))"


def test_extract_top_level_forms_multiple_forms():
    # Each top-level form is returned as a separate entry, in order.
    text = "(defrule foo) (deffacts init)"
    forms = extract_top_level_forms(text)
    assert len(forms) == 2
    assert forms[0][0] == "(defrule foo)"
    assert forms[1][0] == "(deffacts init)"


def test_extract_top_level_forms_string_with_parens_not_split():
    # Parentheses inside string literals must not affect the depth counter.
    text = '(assert (value "some (nested) text"))'
    forms = extract_top_level_forms(text)
    assert len(forms) == 1
    assert "nested" in forms[0][0]


def test_extract_top_level_forms_start_offset_is_accurate():
    # The second element of each tuple is the character offset of the "(" in
    # the original text.
    text = "   (foo)   (bar)"
    forms = extract_top_level_forms(text)
    assert len(forms) == 2
    assert text[forms[0][1]] == "("
    assert text[forms[1][1]] == "("


def test_extract_top_level_forms_empty_input():
    assert extract_top_level_forms("") == []


# ---------------------------------------------------------------------------
# first_keyword
# ---------------------------------------------------------------------------


def test_first_keyword_basic():
    assert first_keyword("(defrule foo)") == "defrule"


def test_first_keyword_case_insensitive():
    # first_keyword() always returns the keyword lowercased.
    assert first_keyword("(DEFRULE foo)") == "defrule"


def test_first_keyword_with_leading_whitespace_after_paren():
    # Whitespace between "(" and the keyword is allowed.
    assert first_keyword("( defrule foo)") == "defrule"


def test_first_keyword_single_token_form():
    assert first_keyword("(reset)") == "reset"


def test_first_keyword_mixed_case():
    assert first_keyword("(DefTemplate person)") == "deftemplate"


# ---------------------------------------------------------------------------
# detect_features
# ---------------------------------------------------------------------------


def test_detect_features_defrule_detected():
    content = "(defrule my-rule (fact) => (printout t hello crlf))"
    features, _unsupported = detect_features(content)
    assert "defrule" in features


def test_detect_features_defclass_in_features_and_unsupported():
    # defclass is a COOL construct: it appears in features (it was detected)
    # and also in unsupported (it is not supported by ferric).
    content = "(defclass Person (is-a USER))"
    features, unsupported = detect_features(content)
    assert "defclass" in features
    assert "defclass" in unsupported


def test_detect_features_open_is_unsupported():
    # (open ...) is an unsupported file-I/O command.
    content = '(defrule r => (open "file.txt" data "r"))'
    _features, unsupported = detect_features(content)
    assert "open" in unsupported


def test_detect_features_clean_file_has_no_unsupported():
    # A file that only uses supported constructs should have an empty
    # unsupported list.
    content = "(deftemplate person (slot name))\n(defrule greet => (printout t hi crlf))"
    _features, unsupported = detect_features(content)
    assert unsupported == []


def test_detect_features_printout_added_to_features():
    content = "(defrule r => (printout t hello crlf))"
    features, _ = detect_features(content)
    assert "printout" in features


def test_detect_features_returns_two_lists():
    features, unsupported = detect_features("(defrule r => nil)")
    assert isinstance(features, list)
    assert isinstance(unsupported, list)


# ---------------------------------------------------------------------------
# classify_keyword
# ---------------------------------------------------------------------------


def test_classify_keyword_construct():
    assert classify_keyword("defrule") == "construct"


def test_classify_keyword_control():
    assert classify_keyword("reset") == "control"


def test_classify_keyword_watch():
    assert classify_keyword("watch") == "watch"


def test_classify_keyword_cool():
    assert classify_keyword("defclass") == "cool"


def test_classify_keyword_noise():
    assert classify_keyword("clear") == "noise"


def test_classify_keyword_repl_assert():
    assert classify_keyword("assert") == "repl"


def test_classify_keyword_unknown_falls_back_to_repl():
    # Any keyword not in the known sets defaults to the "repl" category.
    assert classify_keyword("some-unknown-command") == "repl"


def test_classify_keyword_deftemplate_is_construct():
    assert classify_keyword("deftemplate") == "construct"


def test_classify_keyword_unwatch_is_watch():
    assert classify_keyword("unwatch") == "watch"
