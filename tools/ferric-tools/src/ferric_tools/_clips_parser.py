"""CLIPS source file parsing utilities.

Provides comment stripping, top-level form extraction, feature detection,
and keyword classification shared by compat-scan, bat-analyze, and harness-gen.
"""

from __future__ import annotations

import re

# ---------------------------------------------------------------------------
# Feature detection patterns
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


def _build_keyword_pattern(keywords: list[str]) -> re.Pattern[str]:
    """Build a regex matching (keyword ... at a word boundary."""
    escaped = [re.escape(k) for k in keywords]
    return re.compile(r"\(\s*(?:" + "|".join(escaped) + r")(?:\s|[)\"])", re.IGNORECASE)


PAT_COOL = _build_keyword_pattern(COOL_CONSTRUCTS)
PAT_UNSUPPORTED_CONTROL = (
    _build_keyword_pattern(UNSUPPORTED_CONTROL) if UNSUPPORTED_CONTROL else None
)
PAT_UNSUPPORTED_IO = _build_keyword_pattern(UNSUPPORTED_IO)
PAT_INTERACTIVE = _build_keyword_pattern(INTERACTIVE_IO)
PAT_LOADING = _build_keyword_pattern(LOADING_COMMANDS)
PAT_DEFRULE = re.compile(r"\(\s*defrule\s", re.IGNORECASE)
PAT_DEFFACTS = re.compile(r"\(\s*deffacts\s", re.IGNORECASE)
PAT_PRINTOUT = re.compile(r"\(\s*printout\s", re.IGNORECASE)

_ALL_CONSTRUCTS = SUPPORTED_CONSTRUCTS + COOL_CONSTRUCTS
PAT_ALL_CONSTRUCTS = {
    kw: re.compile(r"\(\s*" + re.escape(kw) + r"(?:\s|[)\"])", re.IGNORECASE)
    for kw in _ALL_CONSTRUCTS
}


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
        clean: list[str] = []
        for ch in line:
            if ch == '"':
                in_string = not in_string
            if ch == ";" and not in_string:
                break
            clean.append(ch)
        result.append("".join(clean))
    return "\n".join(result)


# ---------------------------------------------------------------------------
# Feature detection
# ---------------------------------------------------------------------------


def detect_features(content: str) -> tuple[list[str], list[str]]:
    """Detect CLIPS language features in (comment-stripped) content.

    Returns (features, unsupported).
    """
    features: list[str] = []
    unsupported: list[str] = []

    for kw, pat in PAT_ALL_CONSTRUCTS.items():
        if pat.search(content):
            features.append(kw)

    if PAT_PRINTOUT.search(content):
        features.append("printout")

    # COOL
    for kw in COOL_CONSTRUCTS:
        if kw in features:
            unsupported.append(kw)

    # Unsupported control flow
    if PAT_UNSUPPORTED_CONTROL and PAT_UNSUPPORTED_CONTROL.search(content):
        for kw in UNSUPPORTED_CONTROL:
            pat = re.compile(r"\(\s*" + re.escape(kw) + r"(?:\s|[)\"])", re.IGNORECASE)
            if pat.search(content):
                unsupported.append(kw)

    # File I/O
    if PAT_UNSUPPORTED_IO.search(content):
        for kw in UNSUPPORTED_IO:
            pat = re.compile(r"\(\s*" + re.escape(kw) + r"(?:\s|[)\"])", re.IGNORECASE)
            if pat.search(content):
                unsupported.append(kw)

    # Interactive I/O
    if PAT_INTERACTIVE.search(content):
        for kw in INTERACTIVE_IO:
            pat = re.compile(r"\(\s*" + re.escape(kw) + r"(?:\s|[)\"])", re.IGNORECASE)
            if pat.search(content):
                unsupported.append(kw)

    # Loading commands
    if PAT_LOADING.search(content):
        for kw in LOADING_COMMANDS:
            pat = re.compile(r"\(\s*" + re.escape(kw) + r"(?:\s|[)\"])", re.IGNORECASE)
            if pat.search(content):
                unsupported.append(kw)

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
