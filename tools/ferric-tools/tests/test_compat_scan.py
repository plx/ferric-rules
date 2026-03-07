"""Tests for ferric_tools.compat.scan.

Covers classify_file(), which pre-classifies a CLIPS file based on
detected features and the path's file extension.

classify_file(path, features, unsupported) returns
  (classification: str, reason: str, runability: str).
"""

from __future__ import annotations

from pathlib import Path

from ferric_tools.compat.scan import classify_file

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _clp(name: str = "example.clp") -> Path:
    """Return a synthetic .clp Path."""
    return Path(name)


# ---------------------------------------------------------------------------
# COOL constructs → incompatible
# ---------------------------------------------------------------------------


def test_classify_file_cool_construct_is_incompatible():
    # A file containing COOL constructs (e.g. defclass) is classified
    # "incompatible" because ferric does not support COOL.
    path = _clp("cool_example.clp")
    features = ["defclass", "defrule"]
    unsupported = ["defclass"]

    classification, reason, _runability = classify_file(path, features, unsupported)

    assert classification == "incompatible"
    assert reason == "unsupported-form"


# ---------------------------------------------------------------------------
# Interactive I/O → incompatible
# ---------------------------------------------------------------------------


def test_classify_file_interactive_io_is_incompatible():
    # Files that use (read) or (readline) require an interactive terminal;
    # they are classified "incompatible" with runability "interactive".
    path = _clp("interactive.clp")
    features = ["defrule"]
    unsupported = ["read"]

    classification, reason, runability = classify_file(path, features, unsupported)

    assert classification == "incompatible"
    assert reason == "interactive"
    assert runability == "interactive"


# ---------------------------------------------------------------------------
# Supported constructs only → pending / testable
# ---------------------------------------------------------------------------


def test_classify_file_supported_constructs_with_defrule_is_pending_testable():
    # A file that uses only supported constructs AND has at least one defrule
    # is classified "pending" with reason "testable".
    path = _clp("simple_rule.clp")
    features = ["defrule", "deftemplate"]
    unsupported = []

    classification, reason, runability = classify_file(path, features, unsupported)

    assert classification == "pending"
    assert reason == "testable"
    assert runability == "standalone"


def test_classify_file_no_defrule_is_library_only():
    # A file without any defrule is a library/setup file, not directly
    # testable as a standalone scenario.
    path = _clp("library.clp")
    features = ["deftemplate", "deffacts"]
    unsupported = []

    classification, reason, _runability = classify_file(path, features, unsupported)

    assert classification == "pending"
    assert reason == "library-only"


# ---------------------------------------------------------------------------
# File I/O → incompatible
# ---------------------------------------------------------------------------


def test_classify_file_open_io_is_incompatible():
    # Files using (open ...) for file I/O are incompatible.
    path = _clp("file_io.clp")
    features = ["defrule"]
    unsupported = ["open"]

    classification, reason, _runability = classify_file(path, features, unsupported)

    assert classification == "incompatible"
    assert reason == "unsupported-io"


# ---------------------------------------------------------------------------
# .bat extension → always incompatible
# ---------------------------------------------------------------------------


def test_classify_file_bat_extension_is_incompatible():
    # .bat files are CLIPS test-suite batch files; they are always classified
    # "incompatible" regardless of their feature set.
    path = Path("testfile.bat")
    features = []
    unsupported = []

    classification, reason, _runability = classify_file(path, features, unsupported)

    assert classification == "incompatible"
    assert reason == "test-suite-batch"
