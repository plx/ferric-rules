"""CLIPS source file parsing utilities.

Provides comment stripping, top-level form extraction, feature detection,
and keyword classification shared by compat-scan, bat-analyze, and harness-gen.
"""

from __future__ import annotations

from collections.abc import Iterable
from dataclasses import dataclass

# ---------------------------------------------------------------------------
# Feature catalog
# ---------------------------------------------------------------------------

SUPPORTED_CONSTRUCTS = [
    "defrule",
    "deftemplate",
    "deffacts",
    "deffunction",
    "defglobal",
    "defmodule",
    "defgeneric",
    "defmethod",
]

COOL_CONSTRUCTS = [
    "defclass",
    "definstances",
    "defmessage-handler",
]

UNSUPPORTED_CONTROL: list[str] = []

UNSUPPORTED_IO = ["open", "close"]

INTERACTIVE_IO = ["read", "readline"]

LOADING_COMMANDS = ["batch", "batch*", "load", "load*"]


_ALL_CONSTRUCTS = SUPPORTED_CONSTRUCTS + COOL_CONSTRUCTS


# ---------------------------------------------------------------------------
# Raw-source lexical feature scanning
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class SourceSpan:
    """A half-open UTF-8 byte span with 1-based source locations."""

    start_byte: int
    end_byte: int
    start_line: int
    start_column: int
    end_line: int
    end_column: int

    def to_dict(self) -> dict[str, int]:
        """Return the stable JSON representation of this span."""
        return {
            "start_byte": self.start_byte,
            "end_byte": self.end_byte,
            "start_line": self.start_line,
            "start_column": self.start_column,
            "end_line": self.end_line,
            "end_column": self.end_column,
        }


@dataclass(frozen=True)
class FeatureDetection:
    """One recognized form head in CLIPS source."""

    feature: str
    category: str
    reason: str
    head_span: SourceSpan
    form_span: SourceSpan

    def to_dict(self) -> dict[str, object]:
        """Return the stable JSON representation of this detection."""
        return {
            "feature": self.feature,
            "category": self.category,
            "reason": self.reason,
            "head_span": self.head_span.to_dict(),
            "form_span": self.form_span.to_dict(),
        }


@dataclass(frozen=True)
class LexicalIssue:
    """A structural source problem found while scanning."""

    kind: str
    reason: str
    span: SourceSpan

    def to_dict(self) -> dict[str, object]:
        """Return the stable JSON representation of this issue."""
        return {
            "kind": self.kind,
            "reason": self.reason,
            "span": self.span.to_dict(),
        }


@dataclass(frozen=True)
class FeatureScan:
    """Structured result of lexically scanning raw CLIPS source."""

    detections: tuple[FeatureDetection, ...]
    issues: tuple[LexicalIssue, ...]

    @property
    def feature_names(self) -> tuple[str, ...]:
        """Return reported feature names, deduplicated in source order."""
        return _deduplicate(
            detection.feature
            for detection in self.detections
            if detection.feature in _REPORTED_FEATURES
        )

    @property
    def unsupported_feature_names(self) -> tuple[str, ...]:
        """Return unsupported feature names, deduplicated in source order."""
        return _deduplicate(
            detection.feature
            for detection in self.detections
            if detection.feature in _UNSUPPORTED_FEATURES
        )

    def to_dict(self) -> dict[str, object]:
        """Return the versioned JSON representation consumed by compat-scan."""
        return {
            "version": 1,
            "status": "invalid" if self.issues else "valid",
            "detections": [detection.to_dict() for detection in self.detections],
            "issues": [issue.to_dict() for issue in self.issues],
        }


@dataclass(frozen=True)
class _FeatureSpec:
    category: str
    reason: str


@dataclass
class _FormFrame:
    start: int
    awaiting_head: bool = True
    detection_index: int | None = None


@dataclass
class _DetectionDraft:
    feature: str
    spec: _FeatureSpec
    head_start: int
    head_end: int
    form_start: int
    form_end: int | None = None


@dataclass(frozen=True)
class _IssueDraft:
    kind: str
    start: int
    end: int


def _deduplicate(values: Iterable[str]) -> tuple[str, ...]:
    result: list[str] = []
    seen: set[str] = set()
    for value in values:
        if value not in seen:
            result.append(value)
            seen.add(value)
    return tuple(result)


_FEATURE_SPECS = {
    **{
        feature: _FeatureSpec(category="supported-construct", reason="supported-form")
        for feature in SUPPORTED_CONSTRUCTS
    },
    **{
        feature: _FeatureSpec(category="cool-construct", reason="unsupported-form")
        for feature in COOL_CONSTRUCTS
    },
    "printout": _FeatureSpec(category="output", reason="supported-output"),
    **{
        feature: _FeatureSpec(category="unsupported-control", reason="unsupported-control")
        for feature in UNSUPPORTED_CONTROL
    },
    **{
        feature: _FeatureSpec(category="file-io", reason="unsupported-io")
        for feature in UNSUPPORTED_IO
    },
    **{
        feature: _FeatureSpec(category="interactive-io", reason="interactive")
        for feature in INTERACTIVE_IO
    },
    **{
        feature: _FeatureSpec(category="loading-command", reason="unsupported-command")
        for feature in LOADING_COMMANDS
    },
}

_REPORTED_FEATURES = frozenset((*_ALL_CONSTRUCTS, "printout"))
_UNSUPPORTED_FEATURES = frozenset(
    (*COOL_CONSTRUCTS, *UNSUPPORTED_CONTROL, *UNSUPPORTED_IO, *INTERACTIVE_IO, *LOADING_COMMANDS)
)

# CLIPS 6.30 ScanSymbol terminators not already handled as whitespace,
# comments, strings, or parentheses. A leading "<" starts a symbol, while
# "&", "|", and "~" are standalone tokens even at the start of a form.
_CLIPS_SYMBOL_TERMINATORS = frozenset("<&|~")
_CLIPS_STANDALONE_TOKENS = frozenset("&|~")
_CLIPS_WHITESPACE = frozenset(" \n\f\r\t")
# GetToken returns STOP when NUL or ETX begins the next token. ScanString and
# the comment-skipping loop consume them as ordinary content instead.
_CLIPS_HARD_STOPS = frozenset("\x00\x03")


def _is_ascii_control(character: str) -> bool:
    """Match non-printing single-byte characters from CLIPS ScanSymbol."""
    codepoint = ord(character)
    return codepoint < 0x20 or codepoint == 0x7F


def _is_invalid_control(character: str) -> bool:
    """Reject controls CLIPS returns as unknown tokens or premature EOF."""
    return _is_ascii_control(character) and character not in _CLIPS_WHITESPACE


class _SourceMap:
    """Map requested character offsets to UTF-8 bytes and source locations."""

    def __init__(self, source: str, boundaries: Iterable[int]) -> None:
        remaining = set(boundaries)
        self._positions: dict[int, tuple[int, int, int]] = {}
        if not remaining:
            return

        byte_offset = 0
        line = 1
        column = 1
        previous_was_carriage_return = False
        for index, character in enumerate(source):
            if index in remaining:
                self._positions[index] = (byte_offset, line, column)
                remaining.remove(index)
                if not remaining:
                    return
            byte_offset += len(character.encode("utf-8"))
            if character == "\r":
                line += 1
                column = 1
                previous_was_carriage_return = True
            elif character == "\n":
                if not previous_was_carriage_return:
                    line += 1
                column = 1
                previous_was_carriage_return = False
            else:
                column += 1
                previous_was_carriage_return = False

        source_length = len(source)
        if source_length in remaining:
            self._positions[source_length] = (byte_offset, line, column)
            remaining.remove(source_length)
        if remaining:
            raise ValueError("source span boundary is outside the source")

    def span(self, start: int, end: int) -> SourceSpan:
        start_byte, start_line, start_column = self._positions[start]
        end_byte, end_line, end_column = self._positions[end]
        return SourceSpan(
            start_byte=start_byte,
            end_byte=end_byte,
            start_line=start_line,
            start_column=start_column,
            end_line=end_line,
            end_column=end_column,
        )


def scan_features(source: str) -> FeatureScan:
    """Scan raw CLIPS source for recognized form heads and lexical issues.

    Strings, comments, and symbol substrings cannot produce detections. All
    completed and partial detections survive structural errors so callers can
    report both the evidence found and why the source is not safely classifiable.
    """
    frames: list[_FormFrame] = []
    detections: list[_DetectionDraft] = []
    issues: list[_IssueDraft] = []
    source_length = len(source)
    scan_end = source_length
    index = 0

    while index < source_length:
        character = source[index]

        if character == ";":
            while index < source_length and source[index] not in "\r\n":
                index += 1
            continue

        if character == '"':
            if frames and frames[-1].awaiting_head:
                frames[-1].awaiting_head = False
            string_start = index
            index += 1
            while index < source_length:
                if source[index] == "\\":
                    index += min(2, source_length - index)
                    continue
                if source[index] == '"':
                    index += 1
                    break
                index += 1
            else:
                issues.append(
                    _IssueDraft(
                        kind="unterminated-string",
                        start=string_start,
                        end=source_length,
                    )
                )
            continue

        if _is_invalid_control(character):
            issues.append(_IssueDraft(kind="invalid-control-character", start=index, end=index + 1))
            if character in _CLIPS_HARD_STOPS:
                scan_end = index
                break
            if frames and frames[-1].awaiting_head:
                frames[-1].awaiting_head = False
            index += 1
            continue

        if character == "(":
            if frames and frames[-1].awaiting_head:
                frames[-1].awaiting_head = False
            frames.append(_FormFrame(start=index))
            index += 1
            continue

        if character == ")":
            if frames:
                frame = frames.pop()
                if frame.detection_index is not None:
                    detections[frame.detection_index].form_end = index + 1
            else:
                issues.append(_IssueDraft(kind="unmatched-close", start=index, end=index + 1))
            index += 1
            continue

        if not frames or not frames[-1].awaiting_head or character in _CLIPS_WHITESPACE:
            index += 1
            continue

        head_start = index
        if character in _CLIPS_STANDALONE_TOKENS:
            frames[-1].awaiting_head = False
            index += 1
            continue
        while (
            index < source_length
            and source[index] not in _CLIPS_WHITESPACE
            and source[index] not in '();"'
        ):
            if index > head_start and (
                source[index] in _CLIPS_SYMBOL_TERMINATORS or _is_ascii_control(source[index])
            ):
                break
            index += 1
        frame = frames[-1]
        frame.awaiting_head = False
        feature = source[head_start:index].casefold()
        if spec := _FEATURE_SPECS.get(feature):
            frame.detection_index = len(detections)
            detections.append(
                _DetectionDraft(
                    feature=feature,
                    spec=spec,
                    head_start=head_start,
                    head_end=index,
                    form_start=frame.start,
                )
            )

    for frame in frames:
        issues.append(_IssueDraft(kind="unclosed-form", start=frame.start, end=scan_end))

    boundaries = {
        boundary
        for detection in detections
        for boundary in (
            detection.head_start,
            detection.head_end,
            detection.form_start,
            detection.form_end if detection.form_end is not None else scan_end,
        )
    }
    boundaries.update(boundary for issue in issues for boundary in (issue.start, issue.end))
    source_map = _SourceMap(source, boundaries)
    finalized_detections = tuple(
        FeatureDetection(
            feature=detection.feature,
            category=detection.spec.category,
            reason=detection.spec.reason,
            head_span=source_map.span(detection.head_start, detection.head_end),
            form_span=source_map.span(
                detection.form_start,
                detection.form_end if detection.form_end is not None else scan_end,
            ),
        )
        for detection in detections
    )
    finalized_issues = tuple(
        LexicalIssue(
            kind=issue.kind,
            reason=issue.kind,
            span=source_map.span(issue.start, issue.end),
        )
        for issue in sorted(issues, key=lambda issue: (issue.start, issue.end, issue.kind))
    )
    return FeatureScan(detections=finalized_detections, issues=finalized_issues)


# ---------------------------------------------------------------------------
# Comment stripping
# ---------------------------------------------------------------------------


def strip_comments(text: str) -> str:
    """Remove CLIPS comments from source text.

    Strips full-line comments (lines where first non-whitespace is ;) and
    inline comments (from ; to end of line), with a simple heuristic to
    avoid stripping inside string literals.
    """
    lines = text.split("\n")
    result = []
    for line in lines:
        stripped = line.lstrip()
        if stripped.startswith(";"):
            result.append("")
            continue
        in_string = False
        escaped = False
        clean: list[str] = []
        for ch in line:
            if in_string:
                if escaped:
                    escaped = False
                elif ch == "\\":
                    escaped = True
                elif ch == '"':
                    in_string = False
            elif ch == '"':
                in_string = True
            elif ch == ";":
                break
            clean.append(ch)
        result.append("".join(clean))
    return "\n".join(result)


# ---------------------------------------------------------------------------
# Feature detection
# ---------------------------------------------------------------------------


def detect_features(content: str) -> tuple[list[str], list[str]]:
    """Detect CLIPS language features in raw or comment-stripped content.

    Returns ``(features, unsupported)`` with the historical ordering and
    deduplication used by existing bat and compatibility-tool consumers.
    """
    result = scan_features(content)
    detected_features = set(result.feature_names)
    detected_unsupported = set(result.unsupported_feature_names)

    features = [
        feature for feature in (*_ALL_CONSTRUCTS, "printout") if feature in detected_features
    ]
    unsupported = [
        feature
        for feature in (
            *COOL_CONSTRUCTS,
            *UNSUPPORTED_CONTROL,
            *UNSUPPORTED_IO,
            *INTERACTIVE_IO,
            *LOADING_COMMANDS,
        )
        if feature in detected_unsupported
    ]
    return features, unsupported


# ---------------------------------------------------------------------------
# Top-level form extraction
# ---------------------------------------------------------------------------


def extract_top_level_forms(text: str) -> list[tuple[str, int]]:
    """Extract top-level parenthesised forms from *text*.

    Returns a list of (form_text, start_offset) tuples.
    Handles string literals, CLIPS comments, and escaped quotes.
    """
    forms: list[tuple[str, int]] = []
    depth = 0
    in_string = False
    i = 0
    n = len(text)
    form_start: int | None = None

    while i < n:
        ch = text[i]

        if in_string:
            if ch == "\\" and i + 1 < n:
                i += 2
                continue
            if ch == '"':
                in_string = False
            i += 1
            continue

        if ch == '"':
            in_string = True
            i += 1
            continue

        if ch == ";":
            while i < n and text[i] != "\n":
                i += 1
            continue

        if ch == "(":
            if depth == 0:
                form_start = i
            depth += 1
        elif ch == ")":
            depth -= 1
            if depth <= 0:
                depth = 0
                if form_start is not None:
                    forms.append((text[form_start : i + 1], form_start))
                    form_start = None

        i += 1

    return forms


def first_keyword(form_text: str) -> str:
    """Return the first symbol after the opening paren of *form_text*."""
    i = 0
    n = len(form_text)
    while i < n and form_text[i] != "(":
        i += 1
    i += 1
    while i < n and form_text[i] in (" ", "\t", "\n", "\r"):
        i += 1
    start = i
    while i < n and form_text[i] not in (" ", "\t", "\n", "\r", ")", "(", '"'):
        i += 1
    return form_text[start:i].lower()


# ---------------------------------------------------------------------------
# Keyword classification (for bat-analyze)
# ---------------------------------------------------------------------------

CONSTRUCT_KEYWORDS = {
    "defrule",
    "deftemplate",
    "deffacts",
    "deffunction",
    "defglobal",
    "defmodule",
    "defgeneric",
    "defmethod",
}

COOL_KEYWORDS = {
    "defclass",
    "definstances",
    "defmessage-handler",
}

CONTROL_KEYWORDS = {"reset", "run"}

WATCH_KEYWORDS = {"watch", "unwatch"}

NOISE_KEYWORDS = {"clear"}

REPL_KEYWORDS = {
    "assert",
    "retract",
    "facts",
    "agenda",
    "matches",
    "refresh",
    "set-strategy",
    "get-strategy",
    "set-break",
    "remove-break",
    "halt",
    "ppdefrule",
    "ppdeffacts",
    "ppdeftemplate",
    "list-defrules",
    "list-deffacts",
    "list-deftemplates",
    "undefrule",
    "assert-string",
    "load-facts",
    "save-facts",
    "bind",
    "set-salience-evaluation",
}


def classify_keyword(kw: str) -> str:
    """Return the classification category for a keyword string."""
    if kw in CONSTRUCT_KEYWORDS:
        return "construct"
    if kw in COOL_KEYWORDS:
        return "cool"
    if kw in CONTROL_KEYWORDS:
        return "control"
    if kw in WATCH_KEYWORDS:
        return "watch"
    if kw in NOISE_KEYWORDS:
        return "noise"
    if kw in REPL_KEYWORDS:
        return "repl"
    return "repl"
