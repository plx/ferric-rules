"""Keep public CLIPS claims aligned with the blocking differential policy."""

from __future__ import annotations

import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
POLICY = ROOT / "tests" / "examples" / "compat-semantic-policy.json"
PUBLIC_COMPATIBILITY_DOCS = (
    ROOT / "docs" / "compatibility.md",
    ROOT / "site" / "src" / "content" / "docs" / "docs" / "compatibility.md",
)


def test_policy_known_divergences_are_disclosed_in_public_compatibility_docs():
    policy = json.loads(POLICY.read_text(encoding="utf-8"))
    divergences = {
        case["id"]: (case["issue"], case["family"])
        for case in policy["cases"]
        if case["expected"]["classification"] == "divergent"
    }

    assert divergences
    for path in PUBLIC_COMPATIBILITY_DOCS:
        content = path.read_text(encoding="utf-8")
        _, separator, tail = content.partition("## Known Differential Gaps")
        assert separator
        section, _, _ = tail.partition("\n## ")
        table_rows = tuple(line for line in section.splitlines() if line.startswith("|"))

        missing = {
            case_id: {"issue": issue_url, "family": family}
            for case_id, (issue_url, family) in divergences.items()
            if not any(
                f"`{case_id}`" in row and issue_url in row and family in row for row in table_rows
            )
        }
        assert not missing, f"{path.relative_to(ROOT)} omits policy cases: {missing}"
