"""Tests for ferric_tools.compat.diff.

Covers compute_diff() and format_markdown().
"""

from __future__ import annotations

import csv

from ferric_tools.compat.diff import compute_diff, format_markdown, write_tsv
from ferric_tools.compat.report import compute_oracle_coverage

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _manifest(files: dict) -> dict:
    """Build a minimal manifest dict with the given files mapping."""
    return {"version": 1, "files": files}


def _oracle(
    status: str = "valid",
    *,
    version: int = 1,
    declaration: bool = True,
    reached: bool = True,
    completed: bool = True,
    effect: bool = True,
    normalizations: list[str] | None = None,
) -> dict:
    return {
        "status": status,
        "version": version,
        "declaration": declaration,
        "reached": reached,
        "completed": completed,
        "effect": effect,
        "normalizations": normalizations or [],
        "violations": [],
    }


def _missing_oracle() -> dict:
    return _oracle(
        status="missing",
        declaration=False,
        reached=False,
        completed=False,
        effect=False,
    )


def _file_entry(classification: str, reason: str = "", *, oracle: dict | None = None) -> dict:
    entry = {"classification": classification, "reason": reason}
    if oracle is not None:
        entry["oracle_evidence"] = oracle
    return entry


# ---------------------------------------------------------------------------
# compute_diff — classification changes
# ---------------------------------------------------------------------------


def test_compute_diff_improvement_detected():
    # When a file moves from "divergent" (rank 1) to "equivalent" (rank 0)
    # it must appear in real_improvements, not regressions.
    base = _manifest({"foo.clp": _file_entry("divergent")})
    head = _manifest({"foo.clp": _file_entry("equivalent", oracle=_oracle())})

    _base_counts, _head_counts, regressions, real_improvements, _reason_changes = compute_diff(
        base, head
    )

    assert len(real_improvements) == 1
    assert real_improvements[0][0] == "foo.clp"
    assert len(regressions) == 0


def test_compute_diff_regression_detected():
    # When a file moves from "equivalent" to "divergent" it is a regression.
    base = _manifest({"bar.clp": _file_entry("equivalent")})
    head = _manifest({"bar.clp": _file_entry("divergent")})

    _base_counts, _head_counts, regressions, real_improvements, _reason_changes = compute_diff(
        base, head
    )

    assert len(regressions) == 1
    assert regressions[0][0] == "bar.clp"
    assert len(real_improvements) == 0


def test_compute_diff_no_changes_when_manifests_identical():
    # Identical manifests produce no regressions, no improvements, no reason
    # changes, and identical counts.
    entry = _file_entry("pending", "testable")
    base = _manifest({"a.clp": entry, "b.clp": entry})
    head = _manifest({"a.clp": entry, "b.clp": entry})

    base_counts, head_counts, regressions, real_improvements, reason_changes = compute_diff(
        base, head
    )

    assert regressions == []
    assert real_improvements == []
    assert reason_changes == []
    assert base_counts == head_counts


def test_compute_diff_reason_change_within_same_classification():
    # When the classification stays the same but the reason text changes, the
    # entry must land in reason_changes (not real_improvements or regressions).
    base = _manifest({"c.clp": _file_entry("divergent", "old-reason")})
    head = _manifest({"c.clp": _file_entry("divergent", "new-reason")})

    _bc, _hc, regressions, real_improvements, reason_changes = compute_diff(base, head)

    assert len(reason_changes) == 1
    assert reason_changes[0][0] == "c.clp"
    assert len(regressions) == 0
    assert len(real_improvements) == 0


def test_compute_diff_counts_reflect_head_manifest():
    # head_counts should count classifications from the head manifest, not base.
    base = _manifest({"x.clp": _file_entry("pending")})
    head = _manifest({"x.clp": _file_entry("equivalent")})

    _bc, head_counts, _r, _i, _rc = compute_diff(base, head)

    assert head_counts["equivalent"] == 1
    assert head_counts["pending"] == 0


def test_compute_diff_keeps_ordinary_additions_and_removals_neutral():
    base = _manifest({"old.clp": _file_entry("pending")})
    head = _manifest({"new.clp": _file_entry("pending")})

    _bc, _hc, regressions, real_improvements, _rc = compute_diff(base, head)

    assert regressions == []
    assert real_improvements == []


def test_compute_diff_flags_newly_added_unverified_equivalent_with_absent_base_tuple():
    base = _manifest({})
    head = _manifest({"new.clp": _file_entry("equivalent", "oracle-v1-match")})

    _bc, _hc, regressions, improvements, reason_changes = compute_diff(base, head)

    assert improvements == []
    assert reason_changes == []
    assert regressions == [
        (
            "new.clp",
            "absent",
            "not present",
            "equivalent",
            "oracle-v1-match; oracle regression: unverified equivalent claim",
        )
    ]


def test_compute_diff_rejects_unchanged_legacy_equivalent_in_v3_head():
    entry = _file_entry("equivalent", "empty-match")
    base = {"version": 2, "files": {"legacy.clp": entry}}
    head = {"version": 3, "files": {"legacy.clp": entry}}

    _bc, _hc, regressions, improvements, reason_changes = compute_diff(base, head)

    assert improvements == []
    assert reason_changes == []
    assert regressions == [
        (
            "legacy.clp",
            "equivalent",
            "empty-match",
            "equivalent",
            "empty-match; oracle regression: unverified equivalent claim",
        )
    ]


def test_compute_diff_keeps_newly_added_verified_equivalent_neutral():
    base = _manifest({})
    head = _manifest({"new.clp": _file_entry("equivalent", oracle=_oracle())})

    _bc, _hc, regressions, improvements, reason_changes = compute_diff(base, head)

    assert regressions == []
    assert improvements == []
    assert reason_changes == []


def test_compute_diff_flags_removed_valid_oracle_fixture_with_absent_head_tuple():
    base = _manifest({"removed.clp": _file_entry("divergent", "oracle-mismatch", oracle=_oracle())})
    head = _manifest({})

    _bc, _hc, regressions, improvements, reason_changes = compute_diff(base, head)

    assert improvements == []
    assert reason_changes == []
    assert regressions == [
        (
            "removed.clp",
            "divergent",
            "oracle-mismatch",
            "absent",
            "not present; oracle regression: valid oracle-backed fixture removed",
        )
    ]


def test_compute_diff_oracle_completion_loss_is_regression_with_same_classification():
    base = _manifest({"covered.clp": _file_entry("equivalent", oracle=_oracle())})
    head = _manifest(
        {
            "covered.clp": _file_entry(
                "equivalent",
                oracle=_oracle(completed=False),
            )
        }
    )

    _bc, _hc, regressions, improvements, reason_changes = compute_diff(base, head)

    assert improvements == []
    assert reason_changes == []
    assert len(regressions) == 1
    assert regressions[0][0] == "covered.clp"
    assert "completed true\u2192false" in regressions[0][4]


def test_compute_diff_valid_equivalent_becoming_missing_is_regression():
    base = _manifest({"covered.clp": _file_entry("equivalent", oracle=_oracle())})
    head = _manifest(
        {
            "covered.clp": _file_entry(
                "equivalent",
                oracle=_missing_oracle(),
            )
        }
    )

    _bc, _hc, regressions, improvements, reason_changes = compute_diff(base, head)

    assert improvements == []
    assert reason_changes == []
    assert len(regressions) == 1
    assert "status valid\u2192invalid" in regressions[0][4]
    assert "unverified equivalent claim" in regressions[0][4]


def test_compute_diff_refuses_new_equivalent_claim_without_valid_evidence():
    base = _manifest({"claim.clp": _file_entry("divergent")})
    head = _manifest({"claim.clp": _file_entry("equivalent")})

    _bc, _hc, regressions, improvements, _reason_changes = compute_diff(base, head)

    assert improvements == []
    assert len(regressions) == 1
    assert "unverified equivalent claim" in regressions[0][4]


def test_compute_diff_refuses_new_equivalent_claim_with_false_validity_flags():
    base = _manifest({"claim.clp": _file_entry("divergent")})
    head = _manifest(
        {
            "claim.clp": _file_entry(
                "equivalent",
                oracle=_oracle(
                    declaration=False,
                    reached=False,
                    completed=False,
                    effect=False,
                ),
            )
        }
    )

    _bc, _hc, regressions, improvements, _reason_changes = compute_diff(base, head)

    assert improvements == []
    assert len(regressions) == 1
    assert "unverified equivalent claim" in regressions[0][4]


def test_compute_diff_refuses_new_equivalent_claim_with_unsupported_evidence_version():
    base = _manifest({"claim.clp": _file_entry("divergent")})
    head = _manifest(
        {
            "claim.clp": _file_entry(
                "equivalent",
                oracle=_oracle(version=2),
            )
        }
    )

    _bc, _hc, regressions, improvements, _reason_changes = compute_diff(base, head)

    assert improvements == []
    assert len(regressions) == 1
    assert "unverified equivalent claim" in regressions[0][4]


def test_compute_diff_schema_migration_reason_change_is_neutral():
    base = {
        "version": 2,
        "files": {
            "migrated.clp": _file_entry(
                "divergent",
                "legacy-output-mismatch",
                oracle=_oracle(version=1),
            )
        },
    }
    head = {
        "version": 3,
        "files": {
            "migrated.clp": _file_entry(
                "divergent",
                "oracle-state-mismatch",
                oracle=_oracle(version=1),
            )
        },
    }

    _bc, _hc, regressions, improvements, reason_changes = compute_diff(base, head)

    assert regressions == []
    assert improvements == []
    assert len(reason_changes) == 1
    assert reason_changes[0][0] == "migrated.clp"


def test_compute_diff_legacy_equivalent_oracle_migration_is_neutral():
    base = {
        "version": 2,
        "files": {
            "legacy.clp": _file_entry(
                "equivalent",
                "exact-match",
            )
        },
    }
    head = {
        "version": 3,
        "files": {
            "legacy.clp": _file_entry(
                "pending",
                "oracle-missing",
                oracle=_missing_oracle(),
            )
        },
    }

    _bc, _hc, regressions, improvements, reason_changes = compute_diff(base, head)

    assert regressions == []
    assert improvements == []
    assert reason_changes == []


def test_compute_diff_legacy_migration_without_explicit_missing_evidence_is_regression():
    base = {
        "version": 2,
        "files": {
            "legacy.clp": _file_entry(
                "equivalent",
                "exact-match",
            )
        },
    }
    head = {
        "version": 3,
        "files": {
            "legacy.clp": _file_entry(
                "pending",
                "oracle-missing",
            )
        },
    }

    _bc, _hc, regressions, improvements, reason_changes = compute_diff(base, head)

    assert improvements == []
    assert reason_changes == []
    assert len(regressions) == 1
    assert regressions[0][0] == "legacy.clp"


def test_compute_diff_legacy_migration_requires_all_missing_coverage_flags_false():
    noncanonical_missing = _missing_oracle()
    noncanonical_missing["effect"] = True
    base = {
        "version": 2,
        "files": {
            "legacy.clp": _file_entry(
                "equivalent",
                "exact-match",
            )
        },
    }
    head = {
        "version": 3,
        "files": {
            "legacy.clp": _file_entry(
                "pending",
                "oracle-missing",
                oracle=noncanonical_missing,
            )
        },
    }

    _bc, _hc, regressions, improvements, reason_changes = compute_diff(base, head)

    assert improvements == []
    assert reason_changes == []
    assert len(regressions) == 1
    assert regressions[0][0] == "legacy.clp"


def test_compute_diff_legacy_divergent_oracle_migration_is_neutral():
    base = {
        "version": 2,
        "files": {
            "legacy.clp": _file_entry(
                "divergent",
                "output-mismatch",
            )
        },
    }
    head = {
        "version": 3,
        "files": {
            "legacy.clp": _file_entry(
                "pending",
                "oracle-missing",
                oracle=_missing_oracle(),
            )
        },
    }

    _bc, _hc, regressions, improvements, reason_changes = compute_diff(base, head)

    assert regressions == []
    assert improvements == []
    assert reason_changes == []


def test_compute_diff_legacy_runtime_incompatible_oracle_migration_is_neutral():
    base = {
        "version": 2,
        "files": {
            "legacy.clp": _file_entry(
                "incompatible",
                "both-error",
            )
        },
    }
    head = {
        "version": 3,
        "files": {
            "legacy.clp": _file_entry(
                "pending",
                "oracle-missing",
                oracle=_missing_oracle(),
            )
        },
    }

    _bc, _hc, regressions, improvements, reason_changes = compute_diff(base, head)

    assert regressions == []
    assert improvements == []
    assert reason_changes == []


def test_compute_diff_legacy_pending_reason_migration_is_neutral():
    base = {
        "version": 2,
        "files": {
            "library.clp": _file_entry(
                "pending",
                "library-only",
            )
        },
    }
    head = {
        "version": 3,
        "files": {
            "library.clp": _file_entry(
                "pending",
                "oracle-missing",
            )
        },
    }

    _bc, _hc, regressions, improvements, reason_changes = compute_diff(base, head)

    assert regressions == []
    assert improvements == []
    assert reason_changes == [
        (
            "library.clp",
            "pending",
            "library-only",
            "pending",
            "oracle-missing",
        )
    ]


def test_compute_diff_static_incompatible_oracle_reset_is_a_regression():
    base = {
        "version": 2,
        "files": {
            "static.clp": _file_entry(
                "incompatible",
                "unsupported-form",
            )
        },
    }
    head = {
        "version": 3,
        "files": {
            "static.clp": _file_entry(
                "pending",
                "oracle-missing",
                oracle=_missing_oracle(),
            )
        },
    }

    _bc, _hc, regressions, improvements, reason_changes = compute_diff(base, head)

    assert improvements == []
    assert reason_changes == []
    assert len(regressions) == 1
    assert regressions[0][0] == "static.clp"


def test_compute_diff_v3_oracle_coverage_loss_remains_a_regression():
    base = {
        "version": 3,
        "files": {
            "covered.clp": _file_entry(
                "equivalent",
                "oracle-equivalent",
                oracle=_oracle(),
            )
        },
    }
    head = {
        "version": 3,
        "files": {
            "covered.clp": _file_entry(
                "pending",
                "oracle-missing",
                oracle=_missing_oracle(),
            )
        },
    }

    _bc, _hc, regressions, improvements, reason_changes = compute_diff(base, head)

    assert improvements == []
    assert reason_changes == []
    assert len(regressions) == 1
    assert regressions[0][0] == "covered.clp"
    assert "oracle regression" in regressions[0][4]


def test_write_tsv_labels_legacy_oracle_demotion_as_schema_migration(tmp_path):
    base = {
        "version": 2,
        "files": {
            "legacy.clp": {
                **_file_entry("equivalent", "exact-match"),
                "source": "fixtures",
            }
        },
    }
    head = {
        "version": 3,
        "files": {
            "legacy.clp": {
                **_file_entry("pending", "oracle-missing", oracle=_missing_oracle()),
                "source": "fixtures",
            }
        },
    }
    output = tmp_path / "diff.tsv"

    write_tsv(base, head, str(output))

    with output.open(newline="", encoding="utf-8") as stream:
        rows = list(csv.DictReader(stream, delimiter="\t"))
    assert rows[0]["change"] == "schema-migration"
    assert rows[0]["oracle_regression"] == ""


def test_write_tsv_marks_oracle_coverage_loss_as_regression(tmp_path):
    base = _manifest(
        {
            "covered.clp": {
                **_file_entry("equivalent", oracle=_oracle(normalizations=["fact-ids"])),
                "source": "fixtures",
            }
        }
    )
    head = _manifest(
        {
            "covered.clp": {
                **_file_entry(
                    "equivalent",
                    oracle=_oracle(completed=False, normalizations=["fact-ids"]),
                ),
                "source": "fixtures",
            }
        }
    )
    output = tmp_path / "diff.tsv"

    write_tsv(base, head, str(output))

    with output.open(newline="", encoding="utf-8") as stream:
        rows = list(csv.DictReader(stream, delimiter="\t"))
    assert rows[0]["change"] == "regression"
    assert rows[0]["base_oracle_status"] == "valid"
    assert rows[0]["head_oracle_status"] == "invalid"
    assert rows[0]["head_oracle_normalizations"] == "fact-ids"
    assert "completed true\u2192false" in rows[0]["oracle_regression"]


# ---------------------------------------------------------------------------
# format_markdown
# ---------------------------------------------------------------------------


def test_format_markdown_returns_list_of_strings():
    base_counts = {"equivalent": 1, "divergent": 0, "incompatible": 0, "pending": 0}
    head_counts = {"equivalent": 1, "divergent": 0, "incompatible": 0, "pending": 0}

    lines = format_markdown(base_counts, head_counts, [], [], [])

    assert isinstance(lines, list)
    assert all(isinstance(line, str) for line in lines)


def test_format_markdown_contains_report_heading():
    # The very first content line must be the standard heading.
    base_counts = {"equivalent": 0, "divergent": 0, "incompatible": 0, "pending": 1}
    head_counts = {"equivalent": 0, "divergent": 0, "incompatible": 0, "pending": 1}

    lines = format_markdown(base_counts, head_counts, [], [], [])

    assert "## CLIPS Compatibility Report" in lines


def test_format_markdown_lists_regression_file():
    # When there is a regression, the offending file name should appear in the
    # output so readers can identify what broke.
    base_counts = {"equivalent": 1, "divergent": 0, "incompatible": 0, "pending": 0}
    head_counts = {"equivalent": 0, "divergent": 1, "incompatible": 0, "pending": 0}
    regressions = [("my-test.clp", "equivalent", "", "divergent", "")]

    lines = format_markdown(base_counts, head_counts, regressions, [], [])

    full_output = "\n".join(lines)
    assert "my-test.clp" in full_output


def test_format_markdown_no_regressions_says_none():
    # When there are no regressions, the report must include the word "None"
    # under the Regressions heading.
    base_counts = {"equivalent": 1, "divergent": 0, "incompatible": 0, "pending": 0}
    head_counts = {"equivalent": 1, "divergent": 0, "incompatible": 0, "pending": 0}

    lines = format_markdown(base_counts, head_counts, [], [], [])

    full_output = "\n".join(lines)
    assert "None" in full_output


def test_format_markdown_exposes_oracle_coverage_and_normalizations():
    base = _manifest({"a.clp": _file_entry("pending")})
    head = _manifest(
        {
            "a.clp": _file_entry(
                "equivalent",
                oracle=_oracle(normalizations=["fact-ids"]),
            )
        }
    )
    base_counts = {"equivalent": 0, "divergent": 0, "incompatible": 0, "pending": 1}
    head_counts = {"equivalent": 1, "divergent": 0, "incompatible": 0, "pending": 0}

    lines = format_markdown(
        base_counts,
        head_counts,
        [],
        [],
        [],
        base_oracle=compute_oracle_coverage(base),
        head_oracle=compute_oracle_coverage(head),
    )

    output = "\n".join(lines)
    assert "### Oracle evidence coverage" in output
    assert "| selected | 0 | 1 | +1 |" in output
    assert "Versions \u2014 base: (none); head: 1: 1" in output
    assert "Normalizations \u2014 base: (none); head: fact-ids: 1" in output
