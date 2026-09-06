# Rehabilitation work selection

The September 6 owner instructions replace the historical production-readiness
program. [The execution record](rehabilitation-status.md) defines the finite
supported scope and current migration status. The selector implementation and
native GitHub dependency model are retained; no new scheduler is needed.

## Use

With authenticated `gh`, `uv`, and the intended repository checkout:

```sh
just get-next-production-readiness-issue --json
```

The command name remains for existing callers. Its defaults now use:

| Role | Label |
| --- | --- |
| Universe | `program:rehabilitation` |
| Work membership | `workflow:rehabilitation` |
| Implementation item | `workflow:rehabilitation-leaf` |
| Integration gate, if needed | `workflow:rehabilitation-gate` |

Every universe member must carry the work label, exactly one leaf/gate label,
and exactly one `priority:p0` through `priority:p3` label. Missing or empty
membership fails closed. Historical `program:production-readiness` labels
preserve the audit inventory and are not the active default queue.

One coordinator prepares the entire migration before applying it, saves the
prior labels/bodies/native graph, updates overlapping issues and removes obsolete
native blockers, then verifies live membership and dependency edges. Do not run
selectors or start another executor against a partially migrated graph. Retire
superseded/out-of-scope issues as **not planned**, with a truthful explanation;
never describe those retirements as implemented completion.

## Existing scheduling contract

The selector reads all open and closed cohort issues, complete native `Blocked
by` relationships, and GitHub's `closingIssuesReferences` for open PRs targeting
the repository default branch. Issue-body checklists are not dependencies.

- A closed issue is covered. An open default-branch closing PR also covers a
  leaf for selection, including a draft, but is not a landed prerequisite.
- A PR mentioning `Refs #N` does not claim the issue. Verify a closing keyword
  with `gh pr view N --json closingIssuesReferences`.
- One closing PR covers one active cohort item. Consolidate overlapping issues
  before scheduling. Multiple closing PRs for one active item, or one PR closing
  several active items, fail validation.
- Closed/covered leaf blockers permit selection. Gate blockers must be closed.
  Coverage is transitive: a premature closing PR cannot unlock its descendants.
- Select ready p0 before p1/p2/p3, leaves before gates at equal priority, then
  lowest issue number. `--exclude N` is repeatable and changes no GitHub state.
- Every native prerequisite must actually be closed before dependent merge.
  Prefer sequential merges; shallow stacks must name prerequisites and be
  rebased onto main after those prerequisites land.
- Selected/complete state is checked twice, with a fresh targeted issue query.
  This reduces races but is not a multiworker claim lock.
- Complete output means all items are closed or covered, **not** that the
  rehabilitation is complete. The execution record's required outcomes and
  merged implementation/evidence determine completion.

Run focused offline validation when changing this contract:

```sh
uv run --project tools/ferric-tools pytest tools/ferric-tools/tests/test_next_production_readiness_issue.py
```

Alternate `--universe-label`, `--work-label`, `--leaf-label`, and `--gate-label`
options remain available. Existing offline tests cover membership, dependencies,
PR coverage, races, and malformed graphs. Historical live smoke evidence is
preserved in [the baseline work-selection document](https://github.com/plx/ferric-rules/blob/a38de6a852cce3f503467cd000ba7b182c4b5b30/docs/audits/production-readiness-work-selection.md)
and issues #227–#230/PRs #231–#234.

## Review and merge

Run `just preflight-pr` before PR creation and before pushing updates; inspect
automatic edits. Run relevant core/feature, compatibility, binding consumer,
dependency, scaling and safety checks. Obtain fresh review for consequential
changes, particularly unsafe/threading/FFI boundaries, and address findings.
Respect actual required checks/reviews without treating them as the entire
quality bar. Ordinary scope tradeoffs and issue retirement are authorized by
the owner; recurring epic approval or waiver-renewal checkpoints are retired.
