"""Tests for the production-readiness issue selector."""

from __future__ import annotations

import json
from dataclasses import replace

import pytest
from typer.testing import CliRunner

from ferric_tools.issues import next_production_readiness as selector
from ferric_tools.issues.next_production_readiness import (
    Blocker,
    ClosingPullRequest,
    GitHubClient,
    SelectionStatus,
    WorkflowError,
    WorkIssue,
    WorkKind,
    collect_closing_pull_requests,
    get_selection,
    normalize_work_issues,
    select_next,
    validate_workflow_membership,
)

REPO = "plx/ferric-rules"
UNIVERSE_LABEL = "program:test"
WORK_LABEL = "workflow:test"
LEAF_LABEL = "workflow:test-leaf"
GATE_LABEL = "workflow:test-gate"
UPDATED_AT = "2026-07-25T12:00:00Z"


def _closing_pr(number: int = 900) -> ClosingPullRequest:
    return ClosingPullRequest(
        number=number,
        title=f"Close test issue from PR {number}",
        url=f"https://github.com/{REPO}/pull/{number}",
        is_draft=True,
    )


def _blocker(
    number: int,
    *,
    state: str = "OPEN",
    repository: str = REPO,
) -> Blocker:
    return Blocker(
        node_id=f"I_blocker_{number}",
        number=number,
        title=f"Blocker {number}",
        url=f"https://github.com/{repository}/issues/{number}",
        state=state,
        repository=repository,
    )


def _issue(
    number: int,
    *,
    priority: int = 1,
    kind: WorkKind = WorkKind.LEAF,
    state: str = "OPEN",
    updated_at: str = UPDATED_AT,
    blockers: tuple[Blocker, ...] = (),
    closing_pull_requests: tuple[ClosingPullRequest, ...] = (),
) -> WorkIssue:
    return WorkIssue(
        node_id=f"I_issue_{number}",
        number=number,
        title=f"Issue {number}",
        url=f"https://github.com/{REPO}/issues/{number}",
        state=state,
        updated_at=updated_at,
        priority=priority,
        kind=kind,
        blockers=blockers,
        closing_pull_requests=closing_pull_requests,
    )


def test_selects_lowest_priority_rank_before_issue_number():
    result = select_next([_issue(10, priority=1), _issue(20, priority=0)])

    assert result.status is SelectionStatus.SELECTED
    assert result.issue == _issue(20, priority=0)
    assert result.message.endswith("/issues/20")


def test_uses_issue_number_as_stable_tiebreaker():
    result = select_next([_issue(20, priority=0), _issue(10, priority=0)])

    assert result.issue == _issue(10, priority=0)


def test_can_exclude_deterministic_winner_for_one_invocation():
    result = select_next(
        [_issue(10, priority=0), _issue(20, priority=0)],
        excluded_numbers=frozenset({10}),
    )

    assert result.issue == _issue(20, priority=0)


def test_reports_exclusion_specific_waiting_when_every_ready_issue_is_excluded():
    result = select_next(
        [_issue(10, priority=0), _issue(20, priority=1)],
        excluded_numbers=frozenset({10, 20}),
    )

    assert result.status is SelectionStatus.WAITING
    assert result.issue is None
    assert "excluded" in result.message.lower()
    assert result.ready_count == 0


def test_prefers_leaf_to_gate_at_same_priority():
    result = select_next(
        [
            _issue(10, priority=0, kind=WorkKind.GATE),
            _issue(20, priority=0, kind=WorkKind.LEAF),
        ]
    )

    assert result.issue == _issue(20, priority=0, kind=WorkKind.LEAF)


def test_skips_issue_with_open_closing_pull_request():
    result = select_next(
        [
            _issue(10, priority=0, closing_pull_requests=(_closing_pr(),)),
            _issue(20, priority=1),
        ]
    )

    assert result.issue == _issue(20, priority=1)
    assert result.covered_count == 1


def test_leaf_can_follow_blocker_with_open_closing_pull_request():
    prerequisite = _issue(10, closing_pull_requests=(_closing_pr(),))
    dependent_blocker = _blocker(10)
    dependent = _issue(20, priority=0, blockers=(dependent_blocker,))
    dependent = replace(
        dependent,
        blockers=(replace(dependent_blocker, node_id=prerequisite.node_id),),
    )

    result = select_next([prerequisite, dependent])

    assert result.issue == dependent


def test_prematurely_covered_leaf_does_not_unlock_downstream_leaf():
    prerequisite = _issue(10, priority=1)
    claimed_blocker = _issue(
        20,
        priority=0,
        blockers=(replace(_blocker(10), node_id=prerequisite.node_id),),
        closing_pull_requests=(_closing_pr(),),
    )
    downstream = _issue(
        30,
        priority=0,
        blockers=(replace(_blocker(20), node_id=claimed_blocker.node_id),),
    )

    result = select_next([prerequisite, claimed_blocker, downstream])

    assert result.issue == prerequisite


def test_validly_sequenced_covered_leaf_chain_unlocks_downstream_leaf():
    prerequisite = _issue(10, closing_pull_requests=(_closing_pr(900),))
    covered_dependent = _issue(
        20,
        blockers=(replace(_blocker(10), node_id=prerequisite.node_id),),
        closing_pull_requests=(_closing_pr(901),),
    )
    downstream = _issue(
        30,
        priority=0,
        blockers=(replace(_blocker(20), node_id=covered_dependent.node_id),),
    )

    result = select_next([prerequisite, covered_dependent, downstream])

    assert result.issue == downstream


def test_gate_waits_until_open_blocker_is_actually_closed():
    prerequisite = _issue(10, closing_pull_requests=(_closing_pr(),))
    blocker = _blocker(10)
    gate = _issue(
        20,
        priority=0,
        kind=WorkKind.GATE,
        blockers=(replace(blocker, node_id=prerequisite.node_id),),
    )

    result = select_next([prerequisite, gate])

    assert result.status is SelectionStatus.WAITING
    assert result.issue is None


def test_prematurely_covered_gate_does_not_report_complete():
    prerequisite = _issue(10, closing_pull_requests=(_closing_pr(900),))
    gate = _issue(
        20,
        priority=0,
        kind=WorkKind.GATE,
        blockers=(replace(_blocker(10), node_id=prerequisite.node_id),),
        closing_pull_requests=(_closing_pr(901),),
    )

    result = select_next([prerequisite, gate])

    assert result.status is SelectionStatus.WAITING
    assert result.issue is None


def test_covered_gate_counts_complete_after_blocker_actually_closes():
    gate = _issue(
        20,
        priority=0,
        kind=WorkKind.GATE,
        blockers=(_blocker(10, state="CLOSED"),),
        closing_pull_requests=(_closing_pr(),),
    )

    result = select_next([gate])

    assert result.status is SelectionStatus.COMPLETE


def test_gate_becomes_ready_after_blocker_closes():
    gate = _issue(
        20,
        priority=0,
        kind=WorkKind.GATE,
        blockers=(_blocker(10, state="CLOSED"),),
    )

    result = select_next([gate])

    assert result.status is SelectionStatus.SELECTED
    assert result.issue == gate


def test_reports_complete_when_every_open_issue_has_closing_pr():
    result = select_next(
        [
            _issue(10, closing_pull_requests=(_closing_pr(900),)),
            _issue(20, closing_pull_requests=(_closing_pr(901),)),
        ],
        work_label=WORK_LABEL,
    )

    assert result.status is SelectionStatus.COMPLETE
    assert result.issue is None
    assert WORK_LABEL in result.message


def test_reports_complete_when_no_open_issues_remain():
    result = select_next([_issue(10, state="CLOSED")], work_label=WORK_LABEL)

    assert result.status is SelectionStatus.COMPLETE
    assert result.open_count == 0


def test_reports_waiting_instead_of_false_completion():
    result = select_next([_issue(20, blockers=(_blocker(10),))], work_label=WORK_LABEL)

    assert result.status is SelectionStatus.WAITING
    assert "waiting" in result.message
    assert result.issue is None


def test_json_output_contains_selected_issue_and_counts():
    result = select_next([_issue(20, priority=0)])

    payload = json.loads(result.as_json())
    assert payload["status"] == "selected"
    assert payload["issue"]["kind"] == "leaf"
    assert payload["issue"]["number"] == 20
    assert payload["ready_count"] == 1


def _raw_issue(
    number: int,
    *,
    labels: list[str] | None = None,
    blockers: list[dict] | None = None,
    state: str = "OPEN",
    updated_at: str = UPDATED_AT,
) -> dict:
    label_names = labels or [WORK_LABEL, LEAF_LABEL, "priority:p1"]
    blocker_nodes = blockers or []
    return {
        "id": f"I_issue_{number}",
        "number": number,
        "title": f"Issue {number}",
        "url": f"https://github.com/{REPO}/issues/{number}",
        "state": state,
        "updatedAt": updated_at,
        "labels": {
            "totalCount": len(label_names),
            "nodes": [{"name": label} for label in label_names],
        },
        "blockedBy": {
            "totalCount": len(blocker_nodes),
            "nodes": blocker_nodes,
        },
    }


def test_normalize_requires_exactly_one_kind_label():
    raw = _raw_issue(
        10,
        labels=[WORK_LABEL, LEAF_LABEL, GATE_LABEL, "priority:p1"],
    )

    with pytest.raises(WorkflowError, match="exactly one"):
        normalize_work_issues(
            [raw],
            repository=REPO,
            work_label=WORK_LABEL,
            leaf_label=LEAF_LABEL,
            gate_label=GATE_LABEL,
            closing_pull_requests={},
        )


def test_membership_validation_rejects_universe_only_issue():
    universe = [_raw_issue(10), _raw_issue(20)]
    work_members = [_raw_issue(10)]

    with pytest.raises(WorkflowError) as raised:
        validate_workflow_membership(
            universe,
            work_members,
            universe_label=UNIVERSE_LABEL,
            work_label=WORK_LABEL,
        )

    assert f"missing {WORK_LABEL!r}: #20" in str(raised.value)


def test_membership_validation_rejects_workflow_only_issue():
    universe = [_raw_issue(10)]
    work_members = [_raw_issue(10), _raw_issue(20)]

    with pytest.raises(WorkflowError) as raised:
        validate_workflow_membership(
            universe,
            work_members,
            universe_label=UNIVERSE_LABEL,
            work_label=WORK_LABEL,
        )

    assert f"missing {UNIVERSE_LABEL!r}: #20" in str(raised.value)


def test_normalize_requires_distinct_work_and_kind_labels():
    raw = _raw_issue(10)

    with pytest.raises(WorkflowError, match="distinct"):
        normalize_work_issues(
            [raw],
            repository=REPO,
            work_label=WORK_LABEL,
            leaf_label=WORK_LABEL,
            gate_label=GATE_LABEL,
            closing_pull_requests={},
        )


def test_normalize_requires_exactly_one_known_priority():
    raw = _raw_issue(10, labels=[WORK_LABEL, LEAF_LABEL])

    with pytest.raises(WorkflowError, match="priority"):
        normalize_work_issues(
            [raw],
            repository=REPO,
            work_label=WORK_LABEL,
            leaf_label=LEAF_LABEL,
            gate_label=GATE_LABEL,
            closing_pull_requests={},
        )


def test_normalize_rejects_known_plus_unknown_priority():
    raw = _raw_issue(
        10,
        labels=[WORK_LABEL, LEAF_LABEL, "priority:p1", "priority:p4"],
    )

    with pytest.raises(WorkflowError, match="priority"):
        normalize_work_issues(
            [raw],
            repository=REPO,
            work_label=WORK_LABEL,
            leaf_label=LEAF_LABEL,
            gate_label=GATE_LABEL,
            closing_pull_requests={},
        )


def test_normalize_rejects_truncated_native_blockers():
    raw = _raw_issue(10)
    raw["blockedBy"]["totalCount"] = 1

    with pytest.raises(WorkflowError, match="truncated"):
        normalize_work_issues(
            [raw],
            repository=REPO,
            work_label=WORK_LABEL,
            leaf_label=LEAF_LABEL,
            gate_label=GATE_LABEL,
            closing_pull_requests={},
        )


def test_normalize_rejects_multiple_open_closing_prs_for_one_issue():
    raw = _raw_issue(10)

    with pytest.raises(WorkflowError, match="multiple open closing"):
        normalize_work_issues(
            [raw],
            repository=REPO,
            work_label=WORK_LABEL,
            leaf_label=LEAF_LABEL,
            gate_label=GATE_LABEL,
            closing_pull_requests={
                raw["id"]: (_closing_pr(900), _closing_pr(901)),
            },
        )


def test_normalize_rejects_one_pr_closing_multiple_workflow_issues():
    first = _raw_issue(10)
    second = _raw_issue(20)
    closing_pr = _closing_pr(900)

    with pytest.raises(WorkflowError, match="closes multiple workflow issues"):
        normalize_work_issues(
            [first, second],
            repository=REPO,
            work_label=WORK_LABEL,
            leaf_label=LEAF_LABEL,
            gate_label=GATE_LABEL,
            closing_pull_requests={
                first["id"]: (closing_pr,),
                second["id"]: (closing_pr,),
            },
        )


def test_normalize_ignores_multiple_closing_prs_for_closed_issue():
    closed = _raw_issue(10)
    closed["state"] = "CLOSED"

    normalized = normalize_work_issues(
        [closed],
        repository=REPO,
        work_label=WORK_LABEL,
        leaf_label=LEAF_LABEL,
        gate_label=GATE_LABEL,
        closing_pull_requests={
            closed["id"]: (_closing_pr(900), _closing_pr(901)),
        },
    )

    assert normalized[0].state == "CLOSED"


def test_normalize_allows_one_pr_to_target_open_and_closed_workflow_issues():
    opened = _raw_issue(10)
    closed = _raw_issue(20)
    closed["state"] = "CLOSED"
    closing_pr = _closing_pr(900)

    normalized = normalize_work_issues(
        [opened, closed],
        repository=REPO,
        work_label=WORK_LABEL,
        leaf_label=LEAF_LABEL,
        gate_label=GATE_LABEL,
        closing_pull_requests={
            opened["id"]: (closing_pr,),
            closed["id"]: (closing_pr,),
        },
    )

    normalized_by_number = {issue.number: issue for issue in normalized}
    assert normalized_by_number[10].closing_pull_requests == (closing_pr,)
    assert normalized_by_number[20].state == "CLOSED"


def _raw_pr(
    number: int,
    *,
    base_branch: str = "main",
    base_repository: str = REPO,
    state: str = "OPEN",
    issue_repository: str = REPO,
    reference_count: int = 1,
) -> dict:
    references = [
        {
            "id": "I_issue_10",
            "number": 10,
            "state": "OPEN",
            "url": f"https://github.com/{issue_repository}/issues/10",
            "repository": {"nameWithOwner": issue_repository},
        }
    ]
    return {
        "number": number,
        "title": f"PR {number}",
        "url": f"https://github.com/{REPO}/pull/{number}",
        "state": state,
        "isDraft": True,
        "baseRefName": base_branch,
        "baseRepository": {"nameWithOwner": base_repository},
        "closingIssuesReferences": {
            "totalCount": reference_count,
            "nodes": references,
        },
    }


def test_collects_draft_default_branch_closing_pr():
    indexed = collect_closing_pull_requests(
        [_raw_pr(900)],
        repository=REPO,
        default_branch="main",
    )

    assert indexed["I_issue_10"][0].number == 900
    assert indexed["I_issue_10"][0].is_draft


@pytest.mark.parametrize(
    "pull_request",
    [
        _raw_pr(900, base_branch="release"),
        _raw_pr(900, base_repository="plx/other"),
        _raw_pr(900, state="CLOSED"),
        _raw_pr(900, issue_repository="plx/other"),
    ],
)
def test_ignores_pr_that_will_not_close_local_issue_on_default_branch(pull_request):
    indexed = collect_closing_pull_requests(
        [pull_request],
        repository=REPO,
        default_branch="main",
    )

    assert indexed == {}


def test_rejects_truncated_closing_issue_references():
    with pytest.raises(WorkflowError, match="truncated"):
        collect_closing_pull_requests(
            [_raw_pr(900, reference_count=2)],
            repository=REPO,
            default_branch="main",
        )


def _issues_page(
    nodes: list[dict],
    *,
    total_count: int,
    has_next_page: bool,
    end_cursor: str | None,
    default_branch: str = "main",
) -> str:
    return json.dumps(
        {
            "data": {
                "repository": {
                    "defaultBranchRef": {"name": default_branch},
                    "issues": {
                        "totalCount": total_count,
                        "nodes": nodes,
                        "pageInfo": {
                            "hasNextPage": has_next_page,
                            "endCursor": end_cursor,
                        },
                    },
                }
            }
        }
    )


def test_resolve_repository_canonicalizes_explicit_repository():
    calls: list[list[str]] = []

    def runner(args):
        calls.append(list(args))
        return f"{REPO}\n"

    client = GitHubClient(runner)

    assert client.resolve_repository("PLX/ferric-rules") == REPO
    assert calls == [
        [
            "gh",
            "repo",
            "view",
            "PLX/ferric-rules",
            "--json",
            "nameWithOwner",
            "--jq",
            ".nameWithOwner",
        ]
    ]


def test_fetch_work_universe_paginates_and_forwards_cursor():
    responses = iter(
        [
            _issues_page(
                [{"id": "I_1"}],
                total_count=2,
                has_next_page=True,
                end_cursor="cursor-1",
            ),
            _issues_page(
                [{"id": "I_2"}],
                total_count=2,
                has_next_page=False,
                end_cursor=None,
            ),
        ]
    )
    calls: list[list[str]] = []

    def runner(args):
        calls.append(list(args))
        return next(responses)

    branch, issues = GitHubClient(runner).fetch_work_universe(REPO, "workflow:universe")

    assert branch == "main"
    assert [issue["id"] for issue in issues] == ["I_1", "I_2"]
    assert "universeLabel=workflow:universe" in calls[0]
    assert "endCursor=cursor-1" not in calls[0]
    assert "endCursor=cursor-1" in calls[1]


def test_fetch_work_universe_rejects_changed_total_during_pagination():
    responses = iter(
        [
            _issues_page(
                [{"id": "I_1"}],
                total_count=2,
                has_next_page=True,
                end_cursor="cursor-1",
            ),
            _issues_page(
                [{"id": "I_2"}],
                total_count=3,
                has_next_page=False,
                end_cursor=None,
            ),
        ]
    )

    with pytest.raises(WorkflowError, match="count changed"):
        GitHubClient(lambda _args: next(responses)).fetch_work_universe(REPO, "workflow:universe")


def test_fetch_work_universe_rejects_missing_next_page_cursor():
    response = _issues_page(
        [{"id": "I_1"}],
        total_count=2,
        has_next_page=True,
        end_cursor=None,
    )

    with pytest.raises(WorkflowError, match="no end cursor"):
        GitHubClient(lambda _args: response).fetch_work_universe(REPO, "workflow:universe")


def test_fetch_work_universe_rejects_invalid_graphql_json():
    with pytest.raises(WorkflowError, match="invalid JSON"):
        GitHubClient(lambda _args: "not JSON").fetch_work_universe(REPO, "workflow:universe")


def test_fetch_work_membership_paginates_and_forwards_work_label():
    responses = iter(
        [
            _issues_page(
                [{"id": "I_1", "number": 1, "url": "https://example.test/1"}],
                total_count=2,
                has_next_page=True,
                end_cursor="cursor-1",
            ),
            _issues_page(
                [{"id": "I_2", "number": 2, "url": "https://example.test/2"}],
                total_count=2,
                has_next_page=False,
                end_cursor=None,
            ),
        ]
    )
    calls: list[list[str]] = []

    def runner(args):
        calls.append(list(args))
        return next(responses)

    issues = GitHubClient(runner).fetch_work_membership(REPO, WORK_LABEL)

    assert [issue["id"] for issue in issues] == ["I_1", "I_2"]
    assert f"workLabel={WORK_LABEL}" in calls[0]
    assert "endCursor=cursor-1" not in calls[0]
    assert "endCursor=cursor-1" in calls[1]


def _guard_response(
    issue: WorkIssue,
    *,
    state: str = "OPEN",
    updated_at: str | None = None,
    pull_requests: list[dict] | None = None,
) -> str:
    priority_label = f"priority:p{issue.priority}"
    kind_label = LEAF_LABEL if issue.kind is WorkKind.LEAF else GATE_LABEL
    pull_request_nodes = pull_requests or []
    return json.dumps(
        {
            "data": {
                "repository": {
                    "defaultBranchRef": {"name": "main"},
                    "issue": {
                        "id": issue.node_id,
                        "number": issue.number,
                        "state": state,
                        "updatedAt": updated_at or issue.updated_at,
                        "repository": {"nameWithOwner": REPO},
                        "labels": {
                            "totalCount": 4,
                            "nodes": [
                                {"name": UNIVERSE_LABEL},
                                {"name": WORK_LABEL},
                                {"name": kind_label},
                                {"name": priority_label},
                            ],
                        },
                        "closedByPullRequestsReferences": {
                            "totalCount": len(pull_request_nodes),
                            "nodes": pull_request_nodes,
                        },
                    },
                }
            }
        }
    )


def _guard_issue(client: GitHubClient, issue: WorkIssue) -> bool:
    return client.selection_is_current(
        repository=REPO,
        default_branch="main",
        universe_label=UNIVERSE_LABEL,
        work_label=WORK_LABEL,
        leaf_label=LEAF_LABEL,
        gate_label=GATE_LABEL,
        issue=issue,
    )


def _guard_closing_pull_request(issue: WorkIssue) -> dict:
    return {
        "number": 900,
        "state": "OPEN",
        "baseRefName": "main",
        "baseRepository": {"nameWithOwner": REPO},
        "closingIssuesReferences": {
            "totalCount": 1,
            "nodes": [
                {
                    "id": issue.node_id,
                    "repository": {"nameWithOwner": REPO},
                }
            ],
        },
    }


def test_selection_guard_accepts_unchanged_uncovered_issue():
    issue = _issue(10)
    calls: list[list[str]] = []

    def runner(args):
        calls.append(list(args))
        return _guard_response(issue)

    assert _guard_issue(GitHubClient(runner), issue)
    assert "-F" in calls[0]
    assert f"number={issue.number}" in calls[0]


@pytest.mark.parametrize(
    ("state", "updated_at"),
    [
        ("CLOSED", UPDATED_AT),
        ("OPEN", "2026-07-25T12:01:00Z"),
    ],
    ids=["closed", "updated"],
)
def test_selection_guard_rejects_closed_or_updated_issue(state, updated_at):
    issue = _issue(10)
    client = GitHubClient(lambda _args: _guard_response(issue, state=state, updated_at=updated_at))

    assert not _guard_issue(client, issue)


def test_selection_guard_rejects_new_default_branch_closing_pull_request():
    issue = _issue(10)
    response = _guard_response(
        issue,
        pull_requests=[_guard_closing_pull_request(issue)],
    )

    assert not _guard_issue(GitHubClient(lambda _args: response), issue)


def test_selection_guard_rejects_truncated_nested_closing_issue_references():
    issue = _issue(10)
    pull_request = _guard_closing_pull_request(issue)
    pull_request["closingIssuesReferences"]["totalCount"] = 2
    response = _guard_response(issue, pull_requests=[pull_request])

    with pytest.raises(WorkflowError, match=r"closing issue references.*truncated"):
        _guard_issue(GitHubClient(lambda _args: response), issue)


class _SnapshotClient:
    def __init__(self, snapshots, guard_results=()):
        self._snapshots = iter(snapshots)
        self._guard_results = iter(guard_results)
        self._current_raw_issues = None
        self.fetch_count = 0
        self.membership_fetch_count = 0
        self.pull_request_fetch_count = 0
        self.guarded_numbers: list[int] = []

    def resolve_repository(self, _repository):
        return REPO

    def fetch_work_universe(self, _repository, _universe_label):
        self.fetch_count += 1
        snapshot = next(self._snapshots)
        self._current_raw_issues = snapshot[1]
        return snapshot

    def fetch_work_membership(self, _repository, _work_label):
        self.membership_fetch_count += 1
        assert self._current_raw_issues is not None
        return self._current_raw_issues

    def fetch_open_pull_requests(self, _repository):
        self.pull_request_fetch_count += 1
        return []

    def selection_is_current(self, **kwargs):
        self.guarded_numbers.append(kwargs["issue"].number)
        return next(self._guard_results)


def _get_selection(client) -> selector.Selection:
    return get_selection(
        client=client,
        repository=REPO,
        universe_label=UNIVERSE_LABEL,
        work_label=WORK_LABEL,
        leaf_label=LEAF_LABEL,
        gate_label=GATE_LABEL,
    )


def test_get_selection_returns_stable_selected_issue_after_two_snapshots():
    client = _SnapshotClient(
        [
            ("main", [_raw_issue(10)]),
            ("main", [_raw_issue(10)]),
        ],
        guard_results=[True, True],
    )

    result = _get_selection(client)

    assert result.issue == _issue(10)
    assert client.fetch_count == 2
    assert client.membership_fetch_count == 2
    assert client.pull_request_fetch_count == 2
    assert client.guarded_numbers == [10, 10]


def test_get_selection_stabilizes_after_one_stale_guard():
    client = _SnapshotClient(
        [
            ("main", [_raw_issue(10)]),
            ("main", [_raw_issue(10)]),
            ("main", [_raw_issue(10)]),
            ("main", [_raw_issue(10)]),
        ],
        guard_results=[True, False, True, True],
    )

    result = _get_selection(client)

    assert result.issue == _issue(10)
    assert client.fetch_count == 4
    assert client.membership_fetch_count == 4
    assert client.pull_request_fetch_count == 4
    assert client.guarded_numbers == [10, 10, 10, 10]


def test_get_selection_rejects_oscillating_selected_issues():
    client = _SnapshotClient(
        [
            ("main", [_raw_issue(10)]),
            ("main", [_raw_issue(20)]),
            ("main", [_raw_issue(10)]),
            ("main", [_raw_issue(20)]),
        ],
        guard_results=[True, True, True, True],
    )

    with pytest.raises(WorkflowError, match="did not stabilize"):
        _get_selection(client)

    assert client.fetch_count == 4
    assert client.membership_fetch_count == 4
    assert client.pull_request_fetch_count == 4
    assert client.guarded_numbers == [10, 20, 10, 20]


def test_get_selection_uses_node_identity_for_consecutive_selection():
    original = _raw_issue(10)
    replacement = {**_raw_issue(10), "id": "I_replacement_10"}
    client = _SnapshotClient(
        [
            ("main", [original]),
            ("main", [replacement]),
            ("main", [replacement]),
        ],
        guard_results=[True, True, True],
    )

    result = _get_selection(client)

    assert result.issue is not None
    assert result.issue.number == 10
    assert result.issue.node_id == "I_replacement_10"
    assert client.fetch_count == 3
    assert client.membership_fetch_count == 3
    assert client.pull_request_fetch_count == 3
    assert client.guarded_numbers == [10, 10, 10]


def test_get_selection_retries_once_when_first_selected_issue_is_stale():
    client = _SnapshotClient(
        [
            ("main", [_raw_issue(10)]),
            ("main", [_raw_issue(20)]),
            ("main", [_raw_issue(20)]),
        ],
        guard_results=[False, True, True],
    )

    result = _get_selection(client)

    assert result.issue == _issue(20)
    assert client.fetch_count == 3
    assert client.membership_fetch_count == 3
    assert client.pull_request_fetch_count == 3
    assert client.guarded_numbers == [10, 20, 20]


def test_get_selection_errors_when_both_selected_snapshots_are_stale():
    client = _SnapshotClient(
        [
            ("main", [_raw_issue(10)]),
            ("main", [_raw_issue(20)]),
        ],
        guard_results=[False, False],
    )

    with pytest.raises(WorkflowError, match="changed repeatedly"):
        _get_selection(client)

    assert client.fetch_count == 2
    assert client.membership_fetch_count == 2
    assert client.guarded_numbers == [10, 20]


def test_get_selection_confirms_complete_with_two_snapshots():
    client = _SnapshotClient(
        [
            ("main", [_raw_issue(10, state="CLOSED")]),
            ("main", [_raw_issue(10, state="CLOSED")]),
        ]
    )

    result = _get_selection(client)

    assert result.status is SelectionStatus.COMPLETE
    assert client.fetch_count == 2
    assert client.membership_fetch_count == 2
    assert client.pull_request_fetch_count == 2
    assert client.guarded_numbers == []


def test_get_selection_confirms_complete_after_selected_issue_goes_stale():
    client = _SnapshotClient(
        [
            ("main", [_raw_issue(10)]),
            ("main", [_raw_issue(10, state="CLOSED")]),
            ("main", [_raw_issue(10, state="CLOSED")]),
        ],
        guard_results=[False],
    )

    result = _get_selection(client)

    assert result.status is SelectionStatus.COMPLETE
    assert client.fetch_count == 3
    assert client.membership_fetch_count == 3
    assert client.pull_request_fetch_count == 3
    assert client.guarded_numbers == [10]


def test_get_selection_supports_complete_stale_complete_sequence():
    client = _SnapshotClient(
        [
            ("main", [_raw_issue(10, state="CLOSED")]),
            ("main", [_raw_issue(10)]),
            ("main", [_raw_issue(10, state="CLOSED")]),
            ("main", [_raw_issue(10, state="CLOSED")]),
        ],
        guard_results=[False],
    )

    result = _get_selection(client)

    assert result.status is SelectionStatus.COMPLETE
    assert client.fetch_count == 4
    assert client.membership_fetch_count == 4
    assert client.pull_request_fetch_count == 4
    assert client.guarded_numbers == [10]


def test_get_selection_does_not_return_dependent_if_blocker_reopens():
    closed_blocker = {
        "id": "I_issue_10",
        "number": 10,
        "title": "Issue 10",
        "url": f"https://github.com/{REPO}/issues/10",
        "state": "CLOSED",
        "repository": {"nameWithOwner": REPO},
    }
    open_blocker = {**closed_blocker, "state": "OPEN"}
    client = _SnapshotClient(
        [
            (
                "main",
                [
                    _raw_issue(10, state="CLOSED"),
                    _raw_issue(20, blockers=[closed_blocker]),
                ],
            ),
            (
                "main",
                [
                    _raw_issue(10),
                    _raw_issue(20, blockers=[open_blocker]),
                ],
            ),
            (
                "main",
                [
                    _raw_issue(10),
                    _raw_issue(20, blockers=[open_blocker]),
                ],
            ),
        ],
        guard_results=[True, True, True],
    )

    result = _get_selection(client)

    assert result.issue == _issue(10)
    assert client.fetch_count == 3
    assert client.membership_fetch_count == 3
    assert client.pull_request_fetch_count == 3
    assert client.guarded_numbers == [20, 10, 10]


def test_cli_selected_output_is_exactly_one_issue_url(monkeypatch):
    selected = select_next([_issue(20, priority=0)])
    monkeypatch.setattr(selector, "get_selection", lambda **_kwargs: selected)

    result = CliRunner().invoke(selector.app, [])

    assert result.exit_code == 0
    assert result.stdout == f"{selected.issue.url}\n"
    assert result.stderr == ""


def test_cli_workflow_error_with_rich_markup_is_one_clean_stderr_line(monkeypatch):
    def fail_selection(**_kwargs):
        raise WorkflowError("bad [/oops]")

    monkeypatch.setattr(selector, "get_selection", fail_selection)

    result = CliRunner().invoke(selector.app, [])

    assert result.exit_code == 1
    assert result.stdout == ""
    assert result.stderr == "error: bad [/oops]\n"
