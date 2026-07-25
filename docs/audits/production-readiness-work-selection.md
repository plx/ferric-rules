# Production-readiness automatic work selection

This document defines the operational workflow for progressing the
[production-readiness remediation program](2026-07-25-remediation-program.md)
one issue and one reviewable pull request at a time.

The selector uses live GitHub state. GitHub issue state, native `Blocked by`
relationships, labels, and pull requests that GitHub recognizes as closing an
issue are authoritative; issue-body checklists and the bootstrap inventory are
not scheduling inputs.

## Quick start

Prerequisites:

- an authenticated [GitHub CLI](https://cli.github.com/) session with read
  access to issues and pull requests;
- `uv`; and
- a checkout whose `origin` identifies the intended repository, or an explicit
  `--repo owner/name`.

Run:

```sh
just get-next-production-readiness-issue
```

The default output is deliberately one line:

| State | Output |
|---|---|
| Selected | The URL of exactly one issue |
| Complete | `All issues labeled workflow:production-readiness are complete or covered by an open pull request that will close them.` |
| Waiting | A message saying uncovered work is waiting for prerequisites or pull requests to merge |
| Invalid/error | A diagnostic on stderr and a nonzero exit |

Use machine-readable output when a controller needs to distinguish the states
without parsing prose:

```sh
just get-next-production-readiness-issue --json
```

If the returned leaf cannot safely proceed until an unmerged prerequisite
lands, exclude it for one invocation and select the next ready item:

```sh
just get-next-production-readiness-issue --exclude 123
```

`--exclude` is repeatable and does not mutate GitHub.

## Workflow labels

The selector validates three labels:

| Label | Population | Meaning |
|---|---:|---|
| `workflow:production-readiness` | 141 | Membership in automatic work selection |
| `workflow:production-readiness-leaf` | 126 | Independently implementable remediation |
| `workflow:production-readiness-gate` | 15 | Topic/component epic, final audit, or program gate |

The canonical `program:production-readiness` label defines the complete
universe. The selector requires its issue set to exactly match the set carrying
`workflow:production-readiness`, then requires every member to carry exactly
one leaf/gate label and exactly one `priority:p0` through `priority:p3` label. A
missing universe/membership label, a missing or multiple kind, or any
unknown/multiple `priority:*` label fails closed rather than silently dropping
work.

These labels are operational metadata. The `program:*`, `priority:*`, `type:*`,
`area:*`, `component:*`, `risk:*`, and `size/*` labels remain the audit and
reporting taxonomy.

## Selection semantics

The selector fetches all open and closed issues carrying the canonical universe
label, validates their workflow membership, fetches their complete native
`Blocked by` relationships, and fetches all open pull requests through GitHub
GraphQL. It does not parse issue bodies or PR prose itself.

An issue is **covered for selection** when either:

1. it is closed; or
2. an open pull request targeting this repository's default branch appears in
   GitHub's `closingIssuesReferences` for the issue.

Both ready-for-review and draft PRs count. A mention such as `Refs #123`, a
closed-unmerged PR, or a PR targeting a non-default branch does not count.

The workflow is intentionally one issue to one closing PR. A PR that closes
multiple open workflow issues, or an issue referenced by multiple open closing
PRs, is ambiguous and makes the selector fail closed until the relationships
are corrected.

For each uncovered issue:

1. Apply native dependency readiness.
   - A leaf blocker is scheduling-complete when it is closed or covered by an
     open closing PR. This permits a deliberate sequence of remediation PRs to
     continue before review and merge.
   - A gate blocker must be actually closed. Component epics, the final audit,
     and the top-level program cannot certify unmerged work.
2. Among ready issues, prefer `p0`, then `p1`, `p2`, and `p3`.
3. Prefer a leaf to a gate at the same priority.
4. Break the remaining tie by ascending issue number.

The distinction between **covered** and **landed** is intentional. A covered
leaf is not merged and does not satisfy an epic. When implementing a dependent
leaf before its prerequisite merges, the agent must either:

- make the dependent change independently against the default branch;
- intentionally stack it on the prerequisite and document the stack; or
- defer it and use `--exclude <issue>` to select another ready issue.

Do not copy an unmerged prerequisite into multiple unrelated PRs merely to keep
the selector moving.

Coverage is evaluated transitively. A closing PR attached prematurely to a leaf
whose own blockers are not covered claims that leaf, but it cannot unlock a
downstream leaf. Likewise, a premature closing PR cannot bypass gate readiness
or make the program appear complete.

## Closing-PR contract

The implementation PR for a selected leaf must include a GitHub closing
keyword that GitHub resolves to that issue, for example:

```text
Closes #123
```

The PR must target the repository's default branch. After opening or updating
the PR, verify the relationship rather than assuming body text was recognized:

```sh
gh pr view <pr-number> --json closingIssuesReferences
```

The next selector run will skip the covered issue. If a closing PR is closed
without merge or its closing keyword is removed, the issue becomes selectable
again.

This mechanism is a work-selection marker, not acceptance evidence. The issue
closes only when GitHub merges the closing PR to the default branch, and the
ticket's tests, validation, and acceptance criteria still govern review.

GitHub does not enforce the native issue graph as PR merge ordering. A
dependent PR **must not merge** until every native blocker issue is actually
closed. A stacked PR must name its prerequisite PRs, remain draft or otherwise
blocked from merge, and be rebased or retargeted onto the default branch after
its prerequisites land so its closing keyword can take effect safely.

## Epic and audit gates

GitHub does not automatically close a blocked issue when all of its blockers
close. Automatic closure would also bypass the epic-level integration and
acceptance criteria defined by this program.

The selector therefore treats each of the 15 organizing/audit issues as a gate:

1. it remains unavailable until every native blocker is actually closed;
2. it is then returned like any other selected issue;
3. the agent performs the gate's aggregate validation; and
4. the agent either closes it with retained evidence or opens a focused
   evidence/status PR containing `Closes #<gate>`.

The native graph then makes the next gate eligible:

```text
leaves → topic/component epics → final audit #223 → program epic #224
```

If all leaves have closing PRs but their component epics are still blocked by
open issues, the selector reports **waiting**, not complete.

## Maintaining the workflow universe

Any new or split remediation ticket must be added atomically to the scheduling
contract. Before moving acceptance work out of an existing ticket:

1. assign the production-readiness milestone;
2. add `program:production-readiness` and
   `workflow:production-readiness`;
3. add exactly one leaf/gate workflow label;
4. add exactly one priority and all applicable type/area/component/risk labels;
5. add native `Blocked by` relationships for its prerequisites; and
6. make every affected topic/component gate natively depend on the new ticket.

A body link alone is insufficient: it is not a selector input and could allow a
gate to close while deferred work remains invisible.

## Automation and concurrency

A suitable long-running goal can call
`just get-next-production-readiness-issue`, implement the returned ticket,
open a default-branch PR that closes it, verify the closing relationship, and
repeat.

The selector is read-only and is not an atomic multi-worker claim service. Two
agents can select the same issue between the query and creation of the first
closing PR. The intended initial workflow is one coordinating session. Before
parallel workers are introduced, add serialized claim coordination rather than
assuming labels or assignees are compare-and-swap locks.

Before printing a selected URL, the command requires the same issue to win two
consecutive full selections. This re-evaluates its native blockers and the
complete transitive chain of covered leaf prerequisites. Each selection also
performs a targeted fresh query to confirm that the issue is still open, has
unchanged workflow taxonomy, and has not acquired a default-branch closing PR.
If a targeted check detects a state race, the command retries once; repeated
selected-issue changes fail with a nonzero exit. A complete result independently
requires two consecutive complete snapshots. These checks narrow, but cannot
eliminate, the interval between the last GitHub query and the caller acting on
its output. If the returned issue is already closed or covered when work begins,
rerun the selector.

GitHub may take a few seconds to index a newly added closing reference. If the
same issue is returned immediately after opening its PR, verify
`closingIssuesReferences` and retry once before changing metadata.

## Alternate-label validation

The command accepts alternate labels so its real GitHub behavior can be tested
without contaminating production scheduling:

```sh
just get-next-production-readiness-issue \
  --repo plx/ferric-rules \
  --universe-label test:issue-selector \
  --work-label test:issue-selector \
  --leaf-label test:issue-selector-leaf \
  --gate-label test:issue-selector-gate
```

An integration smoke test should cover:

1. deterministic priority selection;
2. a native leaf dependency;
3. a PR that merely references an issue and therefore does not cover it;
4. an open draft PR with `Closes #N` that does cover it;
5. a dependent leaf becoming selectable after its prerequisite is covered;
6. a gate remaining unavailable until its blockers actually close;
7. the gate becoming selectable after closure; and
8. complete output after every remaining issue is covered.

Keep smoke-test issues, PRs, and branches clearly named and separate from the
production workflow labels. Close the test objects and delete their branches
after validation; their closed URLs provide durable evidence without leaving
fake work active.

### Recorded live smoke test

The alternate-label test was executed against `plx/ferric-rules` on
2026-07-25. It used three leaf fixtures
[#227](https://github.com/plx/ferric-rules/issues/227),
[#228](https://github.com/plx/ferric-rules/issues/228), and
[#229](https://github.com/plx/ferric-rules/issues/229), plus gate
[#230](https://github.com/plx/ferric-rules/issues/230). Issue #228 was
natively blocked by #227; gate #230 was natively blocked by all three leaves.

The observed sequence was:

| Transition | Selector result |
|---|---|
| Initial state | Selected independent p0 leaf #229, ahead of blocked p0 #228 and ready p1 #227 |
| Draft [PR #231](https://github.com/plx/ferric-rules/pull/231) said only `References #229` | Still selected #229 |
| PR #231 changed to `Closes #229` | Selected #227 |
| Draft [PR #232](https://github.com/plx/ferric-rules/pull/232) added `Closes #227` | Selected dependent leaf #228 while #227 remained open |
| Draft [PR #233](https://github.com/plx/ferric-rules/pull/233) added `Closes #228` | Reported waiting because gate blockers were still open |
| Draft [PR #234](https://github.com/plx/ferric-rules/pull/234) prematurely added `Closes #230` | Still reported waiting; a closing PR could not bypass gate prerequisites |
| PR #234 was closed unmerged | Continued waiting with the gate uncovered |
| Leaves #227–#229 were closed to simulate their PRs landing | Selected gate #230 |
| PR #234 was reopened with `Closes #230` | Reported complete with one open, covered gate |

Cleanup then closed PRs #231–#234 without merge, closed gate #230 as completed,
and deleted all four `test/issue-selector-*` branches. A final alternate-label
run reported complete with zero open fixtures. The test labels and closed
objects remain available for repeatable inspection without affecting the
production universe.
