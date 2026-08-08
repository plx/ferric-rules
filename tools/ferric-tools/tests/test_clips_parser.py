"""Tests for ferric_tools._clips_parser.

Covers strip_comments(), extract_top_level_forms(), first_keyword(),
scan_features(), detect_features(), and classify_keyword().
"""

from __future__ import annotations

import pytest

from ferric_tools._clips_parser import (
    classify_keyword,
    detect_features,
    extract_top_level_forms,
    first_keyword,
    scan_features,
    strip_comments,
)


def _span_text(source: str, span) -> str:
    return source.encode("utf-8")[span.start_byte : span.end_byte].decode("utf-8")


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


def test_strip_comments_does_not_strip_after_escaped_quote_inside_string():
    source = '(printout t "escaped quote: \\"; still string" crlf) ; trailing comment'
    result = strip_comments(source)
    assert result == '(printout t "escaped quote: \\"; still string" crlf) '


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
# scan_features
# ---------------------------------------------------------------------------


def test_scan_features_detects_real_form_heads_at_every_depth_case_insensitively():
    source = """(DeFrUlE sample
  (fact value)
  =>
  (PrInToUt t "hello" crlf)
  (OpEn "out.txt" output "w"))
(READ)
(LoAd* "more.clp")"""

    result = scan_features(source)

    assert [detection.feature for detection in result.detections] == [
        "defrule",
        "printout",
        "open",
        "read",
        "load*",
    ]
    assert [detection.category for detection in result.detections] == [
        "supported-construct",
        "output",
        "file-io",
        "interactive-io",
        "loading-command",
    ]
    assert [detection.reason for detection in result.detections] == [
        "supported-form",
        "supported-output",
        "unsupported-io",
        "interactive",
        "unsupported-command",
    ]
    assert result.feature_names == ("defrule", "printout")
    assert result.unsupported_feature_names == ("open", "read", "load*")
    assert result.issues == ()


def test_scan_features_ignores_strings_comments_and_symbol_substrings():
    source = r"""(defrule lexical
  (message "plain (open x); escaped quote: \"(read)\"")
  ; (batch "ignored.clp") and (close ignored)
  (open-file yes)
  (reader no)
  (preload no)
  (defrule-suffix no)
  =>
  (OpEn "real;name" logical "r"))"""

    result = scan_features(source)

    assert [detection.feature for detection in result.detections] == ["defrule", "open"]
    assert result.feature_names == ("defrule",)
    assert result.unsupported_feature_names == ("open",)
    assert result.issues == ()


def test_scan_features_allows_comments_before_a_form_head():
    source = "(\n  ; the head follows on the next line\n  ClOsE logical)"

    result = scan_features(source)

    assert [detection.feature for detection in result.detections] == ["close"]
    assert result.detections[0].head_span.start_line == 3
    assert result.detections[0].head_span.start_column == 3


def test_scan_features_uses_pinned_clips_symbol_terminators_after_form_heads():
    source = (
        '(OpEn&foo) (CLOSE|bar) (read~value) (LOAD<path) (batch"file") '
        "(load*(nested)) (readline; comment\n)"
    )

    result = scan_features(source)

    assert [detection.feature for detection in result.detections] == [
        "open",
        "close",
        "read",
        "load",
        "batch",
        "load*",
        "readline",
    ]
    assert [_span_text(source, detection.head_span) for detection in result.detections] == [
        "OpEn",
        "CLOSE",
        "read",
        "LOAD",
        "batch",
        "load*",
        "readline",
    ]
    assert result.issues == ()


def test_scan_features_reports_nonprinting_ascii_terminators_and_retains_heads():
    source = "(OpEn\x01argument) (read\x0bvalue) (close\x7fvalue)"

    result = scan_features(source)

    assert [detection.feature for detection in result.detections] == ["open", "read", "close"]
    assert [_span_text(source, detection.head_span) for detection in result.detections] == [
        "OpEn",
        "read",
        "close",
    ]
    assert [issue.kind for issue in result.issues] == [
        "invalid-control-character",
        "invalid-control-character",
        "invalid-control-character",
    ]
    assert [_span_text(source, issue.span) for issue in result.issues] == [
        "\x01",
        "\x0b",
        "\x7f",
    ]
    assert result.to_dict()["status"] == "invalid"


@pytest.mark.parametrize("hard_stop", ["\x00", "\x03"])
def test_scan_features_leading_nul_or_etx_stops_before_later_input(hard_stop):
    source = f"{hard_stop}(OpEn)\x1f(ReAd))"

    result = scan_features(source)

    assert result.detections == ()
    assert [issue.kind for issue in result.issues] == ["invalid-control-character"]
    assert [_span_text(source, issue.span) for issue in result.issues] == [hard_stop]
    assert result.issues[0].span.to_dict() == {
        "start_byte": 0,
        "end_byte": 1,
        "start_line": 1,
        "start_column": 1,
        "end_line": 1,
        "end_column": 2,
    }


@pytest.mark.parametrize("hard_stop", ["\x00", "\x03"])
def test_scan_features_internal_nul_or_etx_retains_prefix_and_ends_open_forms_at_stop(
    hard_stop,
):
    source = f"☃\r\n(DeFrUlE r => (OpEn{hard_stop}) (ReAd))\x1f(CLOSE)"
    stop = source.index(hard_stop)
    stop_byte = len(source[:stop].encode("utf-8"))
    outer_form_start = source.index("(")

    result = scan_features(source)

    assert [detection.feature for detection in result.detections] == ["defrule", "open"]
    assert [_span_text(source, detection.form_span) for detection in result.detections] == [
        source[outer_form_start:stop],
        "(OpEn",
    ]
    assert [issue.kind for issue in result.issues] == [
        "unclosed-form",
        "unclosed-form",
        "invalid-control-character",
    ]
    assert [_span_text(source, issue.span) for issue in result.issues] == [
        source[outer_form_start:stop],
        "(OpEn",
        hard_stop,
    ]
    assert all(detection.form_span.end_byte == stop_byte for detection in result.detections)
    assert result.issues[-1].span.start_byte == stop_byte
    assert result.issues[-1].span.end_byte == stop_byte + 1
    assert result.issues[-1].span.start_line == 2


def test_scan_features_invalid_control_as_first_form_token_prevents_false_head():
    source = "(\x01open) (ReAd)"

    result = scan_features(source)

    assert [detection.feature for detection in result.detections] == ["read"]
    assert [issue.kind for issue in result.issues] == ["invalid-control-character"]


def test_scan_features_ignores_control_characters_inside_strings_and_comments():
    source = '(defrule r => (printout t "\x00\x01\x03\x7f" crlf)) ; \x00\x03\x1f\n(ReAd)'

    result = scan_features(source)

    assert [detection.feature for detection in result.detections] == [
        "defrule",
        "printout",
        "read",
    ]
    assert result.issues == ()


def test_scan_features_does_not_treat_standalone_or_nonterminating_symbols_as_heads():
    source = (
        "(&open) (|read) (~load) (<close) (open>suffix) (read?field) "
        "(open\u200bsuffix) (close\u00a0suffix) (load\u0085suffix)"
    )

    result = scan_features(source)

    assert result.detections == ()
    assert result.issues == ()


def test_scan_features_returns_every_repeated_detection_in_source_order():
    result = scan_features("(OPEN a b c) (open d e f) (CLOSE a)")

    assert [detection.feature for detection in result.detections] == [
        "open",
        "open",
        "close",
    ]
    assert result.unsupported_feature_names == ("open", "close")


def test_scan_features_spans_use_utf8_bytes_and_one_based_character_locations():
    source = '☃ (OpEn "é" data "r")'

    result = scan_features(source)
    detection = result.detections[0]

    assert detection.feature == "open"
    assert detection.head_span.to_dict() == {
        "start_byte": 5,
        "end_byte": 9,
        "start_line": 1,
        "start_column": 4,
        "end_line": 1,
        "end_column": 8,
    }
    assert detection.form_span.start_byte == 4
    assert detection.form_span.end_byte == len(source.encode("utf-8"))
    assert detection.form_span.start_line == 1
    assert detection.form_span.start_column == 3
    assert detection.form_span.end_line == 1
    assert detection.form_span.end_column == len(source) + 1
    assert _span_text(source, detection.head_span) == "OpEn"
    assert _span_text(source, detection.form_span) == '(OpEn "é" data "r")'


def test_scan_features_large_source_without_evidence_remains_empty():
    source = "x" * 300_000 + '\r\n"(open ignored)" ; (read ignored)'

    result = scan_features(source)

    assert result.detections == ()
    assert result.issues == ()
    assert result.to_dict() == {
        "version": 1,
        "status": "valid",
        "detections": [],
        "issues": [],
    }


def test_scan_features_large_source_maps_few_boundaries_exactly():
    prefix = "x" * 250_000 + "\r\né "
    form = '(OpEn "é" data "r")'
    source = prefix + form + "\n" + "y" * 250_000

    result = scan_features(source)
    detection = result.detections[0]

    assert len(result.detections) == 1
    assert result.issues == ()
    assert detection.head_span.start_byte == len((prefix + "(").encode("utf-8"))
    assert detection.head_span.end_byte == detection.head_span.start_byte + len("OpEn")
    assert detection.head_span.start_line == 2
    assert detection.head_span.start_column == 4
    assert detection.head_span.end_line == 2
    assert detection.head_span.end_column == 8
    assert detection.form_span.start_byte == len(prefix.encode("utf-8"))
    assert detection.form_span.end_byte == len((prefix + form).encode("utf-8"))
    assert detection.form_span.start_line == 2
    assert detection.form_span.start_column == 3
    assert _span_text(source, detection.form_span) == form


def test_scan_features_spans_cover_the_nested_form_not_its_parent():
    source = '(defrule r => (open "x" data "r"))'

    result = scan_features(source)

    defrule, open_command = result.detections
    assert _span_text(source, defrule.form_span) == source
    assert _span_text(source, open_command.form_span) == '(open "x" data "r")'
    assert _span_text(source, open_command.head_span) == "open"


def test_scan_features_reports_unterminated_string_and_retains_prior_detection():
    source = '(OpEn "unterminated ; (read)'

    result = scan_features(source)

    assert [detection.feature for detection in result.detections] == ["open"]
    assert [issue.kind for issue in result.issues] == [
        "unclosed-form",
        "unterminated-string",
    ]
    assert _span_text(source, result.detections[0].form_span) == source
    assert _span_text(source, result.issues[0].span) == source
    assert _span_text(source, result.issues[1].span) == '"unterminated ; (read)'
    assert result.to_dict()["status"] == "invalid"


def test_scan_features_reports_each_unclosed_form_with_partial_detections():
    source = "(defrule r => (OpEn"

    result = scan_features(source)

    assert [detection.feature for detection in result.detections] == ["defrule", "open"]
    assert [issue.kind for issue in result.issues] == ["unclosed-form", "unclosed-form"]
    assert [_span_text(source, issue.span) for issue in result.issues] == [
        source,
        "(OpEn",
    ]
    assert [_span_text(source, detection.form_span) for detection in result.detections] == [
        source,
        "(OpEn",
    ]


def test_scan_features_reports_unmatched_close_and_keeps_later_detections():
    source = '(defrule r => (open "x" data "r")))\n(ReAd)'

    result = scan_features(source)

    assert [detection.feature for detection in result.detections] == [
        "defrule",
        "open",
        "read",
    ]
    assert [issue.kind for issue in result.issues] == ["unmatched-close"]
    assert _span_text(source, result.issues[0].span) == ")"
    assert result.issues[0].span.start_line == 1
    assert result.detections[-1].head_span.start_line == 2


def test_scan_features_serializer_has_a_stable_complete_schema():
    source = '(OpEn "x" data "r"))'

    result = scan_features(source)

    assert result.to_dict() == {
        "version": 1,
        "status": "invalid",
        "detections": [
            {
                "feature": "open",
                "category": "file-io",
                "reason": "unsupported-io",
                "head_span": {
                    "start_byte": 1,
                    "end_byte": 5,
                    "start_line": 1,
                    "start_column": 2,
                    "end_line": 1,
                    "end_column": 6,
                },
                "form_span": {
                    "start_byte": 0,
                    "end_byte": 19,
                    "start_line": 1,
                    "start_column": 1,
                    "end_line": 1,
                    "end_column": 20,
                },
            }
        ],
        "issues": [
            {
                "kind": "unmatched-close",
                "reason": "unmatched-close",
                "span": {
                    "start_byte": 19,
                    "end_byte": 20,
                    "start_line": 1,
                    "start_column": 20,
                    "end_line": 1,
                    "end_column": 21,
                },
            }
        ],
    }


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


def test_detect_features_uses_lexical_scanner_for_raw_source():
    content = r"""(defrule r
  (message "(open file logical r); \"(read)\"")
  ; (load "ignored.clp")
  (open-file value)
  => nil)"""

    features, unsupported = detect_features(content)

    assert features == ["defrule"]
    assert unsupported == []


def test_detect_features_preserves_historical_keyword_order_and_deduplication():
    content = "(printout t x) (DEFCLASS X) (defrule r) (open x y z) (deftemplate t)"

    features, unsupported = detect_features(content)

    assert features == ["defrule", "deftemplate", "defclass", "printout"]
    assert unsupported == ["defclass", "open"]


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
