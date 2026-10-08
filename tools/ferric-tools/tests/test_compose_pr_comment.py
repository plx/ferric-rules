"""Exercise the PR comment composer with a stub `gh` that records the posted body."""

import os
import subprocess

import pytest

from ferric_tools._paths import repo_root

MARKER = "<!-- pr-assessment-report -->"
HEADLINE = "### Granular corpus changes"
GITHUB_COMMENT_LIMIT = 65_536

GH_STUB = """#!/bin/sh
if [ "$1" = pr ] && [ "$2" = comment ]; then
    while [ $# -gt 0 ]; do
        [ "$1" = --body-file ] && cp "$2" "$GH_STUB_BODY"
        shift
    done
    echo created > "$GH_STUB_BODY.action"
    exit 0
fi
if [ "$1" = api ]; then
    for arg in "$@"; do
        case "$arg" in
            body=@*) cp "${arg#body=@}" "$GH_STUB_BODY"; echo updated > "$GH_STUB_BODY.action" ;;
        esac
    done
    case "$*" in
        *--paginate*) [ -n "$GH_STUB_EXISTING" ] && echo "$GH_STUB_EXISTING" ;;
    esac
    exit 0
fi
exit 1
"""


@pytest.fixture
def compose(tmp_path):
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    gh = bin_dir / "gh"
    gh.write_text(GH_STUB)
    gh.chmod(0o755)
    body = tmp_path / "body.md"

    def call(report_text, existing=""):
        report = tmp_path / "compat-diff-report.md"
        report.write_text(report_text, encoding="utf-8")
        environment = {
            **os.environ,
            "PATH": str(bin_dir) + os.pathsep + os.environ["PATH"],
            "GITHUB_REPOSITORY": "owner/repo",
            "GITHUB_SERVER_URL": "https://github.com",
            "GITHUB_RUN_ID": "12345",
            "GH_STUB_BODY": str(body),
            "GH_STUB_EXISTING": existing,
        }
        result = subprocess.run(
            [
                "bash",
                str(repo_root() / "scripts/compose-pr-comment.sh"),
                "--pr",
                "7",
                "--compat-report",
                str(report),
                "--compat-status",
                "success",
            ],
            env=environment,
            capture_output=True,
            text=True,
            timeout=30,
        )
        assert result.returncode == 0, result.stderr
        action = body.with_name(body.name + ".action").read_text().strip()
        return body.read_text(encoding="utf-8"), action

    return call


def _report(rows):
    lines = ["## CLIPS Compatibility Report", "", HEADLINE, "", "Corpus: 1266 cases", ""]
    lines += ["<details><summary>Coverage, removals and inventory changes</summary>", ""]
    lines += ["| File | Classification | Before | After |", "|---|---|---|---|"]
    lines += [
        f"| `clips-official/examples/nested/case-{index:05}.clp` | unassessed | "
        "oracle-missing — scanner: testable | not present — removed |"
        for index in range(rows)
    ]
    return "\n".join([*lines, "", "</details>", ""])


@pytest.mark.parametrize("existing", ["", "99"])
def test_oversized_report_is_posted_from_a_file_and_truncated(compose, existing):
    report = _report(3500)
    assert len(report.encode()) > 340_000
    body, action = compose(report, existing)

    assert action == ("updated" if existing else "created")
    assert len(body) < GITHUB_COMMENT_LIMIT
    assert len(body.encode()) < GITHUB_COMMENT_LIMIT
    assert body.startswith(MARKER)
    assert HEADLINE in body
    assert "Report truncated" in body
    assert "https://github.com/owner/repo/actions/runs/12345" in body
    assert "`compat-diff-report` artifact" in body
    assert body.count("<details>") == body.count("</details>") == 1
    assert body.rstrip().endswith("compat: pass*")


def test_small_report_is_posted_whole(compose):
    report = _report(10)
    body, _ = compose(report)
    assert body.startswith(MARKER)
    assert report in body
    assert "Report truncated" not in body
