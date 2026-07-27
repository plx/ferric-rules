# Production-readiness remediation goal

## Copy-paste goal

After the pull request that adds this runbook has merged, start a new Codex
session from a clean, current `main` checkout and submit:

```text
/goal Complete the work in `docs/audits/production-readiness-remediation-goal.md`.
```

This is an execution goal, not a request to edit or summarize this document.
The goal remains active until the terminal completion criteria below are met.
Do not mark it complete merely because every remaining issue has an open pull
request.

## Required outcome

Complete the production-readiness program rooted at
[#224](https://github.com/plx/ferric-rules/issues/224), including every current
or subsequently discovered issue in its workflow universe. Execute the program
end to end as an ordered sequence of small, reviewable pull requests:

- work on exactly one selected issue at a time;
- use intentional, shallow PR stacks when selected work depends on an open
  prerequisite PR;
- base independent work on current `main`;
- merge every stack in dependency order;
- close every workflow issue through its own merged PR;
- complete all topic and component gates;
- execute the independent re-audit against one immutable candidate; and
- publish and verify the declared release artifacts only when the audit,
  issue contracts, and explicit release authorization permit it.

The initial workflow contains 126 leaves and 15 gates. GitHub's live workflow
labels and native dependency graph, not the bootstrap counts, define the
current universe after work begins.

The static documentation site and landing page remain outside the audit scope
except where a ticket explicitly concerns package boundaries, release
contents, public claims, examples, or links to released artifacts. Do not
broaden an issue merely because adjacent cleanup is convenient.

## Scope and authority

Invoking this goal authorizes the normal work in `plx/ferric-rules` needed to
complete the program:

- inspect and modify repository files;
- run local and GitHub-hosted validation;
- create issue-specific branches, commits, draft PRs, and intentional stacks;
- update a PR in response to review or CI;
- create self-contained workflow issues for genuinely new findings and
  maintain their labels, milestone, native blockers, and gate relationships
  under the work-selection maintenance contract;
- merge an ordinary remediation or evidence PR after its prerequisites,
  required review, and required checks are satisfied; and
- delete a merged ticket branch after confirming that no descendant stack
  still needs it.

That authority does **not** permit:

- bypassing branch protection, required reviews, or failing checks;
- using administrator overrides or weakening a test, dependency, acceptance
  criterion, or audit gate to make progress;
- directly changing a workflow issue's state to closed;
- exposing credentials or committing sensitive audit evidence;
- publishing a stable crate, package, module tag, SDK, GitHub Release, or
  other irreversible external artifact without the release checkpoint below;
- inventing an owner, approval, support promise, platform claim, or account
  control; or
- writing to another repository or service unless the selected issue and user
  have established the exact destination and authority.

Do not stop merely because the program spans many turns or compactions.
Persist through ordinary implementation, CI, review, merge, and stack
maintenance. Pause only at a defined approval checkpoint or a genuine
unresolved blocker.

## Preconditions

Before selecting the first remediation issue:

1. Confirm the pull request adding this runbook has merged and the checkout
   contains the issue taxonomy, selector, audit records, and runbook on current
   remote `main`.
2. Start from a clean checkout of current remote `main`. Preserve unrelated
   user changes and use a separate worktree when necessary.
3. Confirm `gh auth status`, repository identity, the default branch, and
   write access.
4. Run the selector's focused offline tests:

   ```sh
   cd tools/ferric-tools
   uv run pytest tests/test_next_production_readiness_issue.py
   ```

5. Run the live selector once:

   ```sh
   just get-next-production-readiness-issue --json
   ```

6. Confirm that the canonical program cohort, workflow cohort, leaf/gate
   classification, priorities, and native dependency graph validate. Do not
   alter labels or relationships merely to obtain a preferred first issue.

If the runbook PR is not merged, GitHub authentication is unavailable, or the
selector fails closed, report that condition and wait. Do not substitute a
hand-written issue order.

## Required guidance

At the beginning of the goal, read:

1. [`AGENTS.md`](../../AGENTS.md) and [`CLAUDE.md`](../../CLAUDE.md);
2. [`README.md`](../../README.md);
3. the baseline
   [production-readiness due-diligence review](2026-07-25-due-diligence.md);
4. the
   [production-readiness remediation program](2026-07-25-remediation-program.md);
5. the
   [work-selection operator contract](production-readiness-work-selection.md);
6. the
   [post-remediation re-audit playbook](production-readiness-reaudit.md);
7. top-level program
   [#224](https://github.com/plx/ferric-rules/issues/224); and
8. the complete body, comments, native blockers, parent/child relationships,
   and linked prior art for the issue selected in the current loop.

Also read any contributor, security, release, support, compatibility, binding,
or normative-contract guidance added by earlier remediation PRs. Shared
guidance can evolve during this goal.

Re-read the selected ticket and relevant shared guidance after compaction,
handoff, a material review change, or a changed GitHub dependency graph. Do
not rely on remembered acceptance criteria.

Apply instructions in this order:

1. current user, system, and repository safety instructions;
2. the selected issue's explicit required behavior and acceptance criteria;
3. `AGENTS.md` and repository conventions;
4. the work-selection contract and this runbook;
5. the due-diligence report and re-audit playbook; and
6. historical specifications, plans, or prior-art PRs.

When sources appear to conflict, inspect the current implementation, tests,
issue history, and linked decisions. Resolve the conflict explicitly in the PR
or ask the user when it would materially change a public contract. Do not
choose an interpretation merely because it is easier to implement.

The 2026-07-25 audit is immutable historical evidence about
`dd366eb65a85e2138b8103e719e9fe0b8f52f921`, not a substitute for inspecting
current `main`.

## Non-negotiable workflow rules

1. **The live selector chooses work.** Do not manually choose a more appealing
   issue. Do not use `--exclude` to evade priority or dependencies. A temporary
   exclusion is acceptable only for a documented, issue-specific constraint
   while another genuinely ready issue can progress.
2. **Implement one issue at a time.** Other PRs may await review or merge, but
   do not implement several workflow tickets concurrently. Bounded research or
   independent review may run in parallel; separate agents must not each
   implement a different workflow issue.
3. **One PR closes exactly one workflow issue.** Split combined fixes unless
   the selected ticket itself requires inseparable work.
4. **Every workflow PR targets `main`.** A branch may descend from an open
   prerequisite branch, but its GitHub PR must still target `main`; the selector
   recognizes closing coverage only on the default branch.
5. **GitHub closes issues through merged PRs.** Never use `gh issue close`, an
   issue-state API mutation, the UI close action, or equivalent automation for
   #224 or any workflow child.
6. **Open PR coverage is not landed work.** Coverage may sequence dependent
   leaves, but it never permits an out-of-order merge and never satisfies a
   gate.
7. **Tests prove the defect when applicable.** Add a regression that fails for
   the intended reason before the fix and passes afterward. Preserve the exact
   red-before-fix command and concise result in the PR.
8. **Do not weaken evidence.** Never delete or relax a test, dependency,
   acceptance criterion, workflow label, branch rule, benchmark oracle, or
   audit gate merely to obtain a green check or different selector result.
9. **Keep public contracts aligned.** Update authoritative compatibility,
   limits, API, ownership, platform, packaging, and migration documentation in
   the same PR whenever behavior changes.
10. **No silent scope absorption.** Give a separately actionable discovery its
    own self-contained issue and dependency metadata unless it is necessary to
    satisfy the selected ticket's existing acceptance criteria.
11. **Benchmark claims use release evidence.** Any performance number in an
    issue, commit, PR, audit, or release note must come from release-profile
    `cargo bench` output. Improvement claims require before and after Criterion
    medians from the same machine and profile.
12. **Run the repository preflight before every push.** Run
    `just preflight-pr` before opening a PR and before pushing any update to an
    existing PR, in addition to the ticket-specific validation.

For this goal, the no-direct-close rule is stricter than the work-selection
guide's general allowance for evidence-backed gate closure. Every leaf and
gate closes through a dedicated merged PR. Reopening invalidated evidence is
permitted where this runbook requires it; the issue must later close again
through a new dedicated PR.

## Pull-request stack contract

The program is a sequence of small, default-branch-targeted ancestry stacks,
not one giant PR and not a 141-branch chain. Git ancestry may stack on a
predecessor head; the GitHub PR base may not.

### Starting a branch

- If the selected issue needs no code from an open prerequisite PR, update
  remote state and create its branch from current remote `main`.
- If a covered prerequisite made the issue selectable and the implementation
  needs that unmerged work, create the branch from the exact prerequisite PR
  head and record the ancestry.
- If the selected issue can be implemented and tested against current `main`,
  prefer a new independent stack even when another PR is open.
- Use an issue-specific name such as
  `agent/issue-104-leading-negative-ce`. Never reuse a merged or abandoned
  ticket branch.

Every PR must target `main`. A dependent PR may temporarily show ancestor
commits and changes; its Stack section must say so clearly.

Keep at most one unmerged descendant above a predecessor PR. Before preparing
a third level, merge and restack from the bottom so each open diff remains
reviewable. Revalidate every affected descendant and update its branch-point
record after rewriting ancestry.

### Stack metadata

Every stacked PR body must identify:

- the immediate predecessor PR, or `none`;
- all earlier PRs whose commits are present;
- the required merge order;
- whether its tests require the predecessor's code; and
- the exact commit or branch from which it was created.

Use ordinary references such as `Refs #N` for related workflow issues. Only
the selected issue receives a closing keyword.

### Merge order

- Merge from the bottom of a stack upward.
- Never merge a dependent PR while a semantic prerequisite issue is open.
- Require the predecessor PR to be merged, not merely approved or green.
- After a predecessor merges, update the next PR on current `main`, remove
  already-landed ancestor commits from its diff, resolve conflicts, and rerun
  every affected check.
- If history rewriting is necessary, use `--force-with-lease` only on the
  goal's own verified ticket branch and only after confirming that no other
  unpublished work depends on its old head. Never use an unguarded force push.
- Re-verify the child PR's base, diff, closing relationship, checks, and review
  state after a rebase or branch update.
- Merge an independent stack when it is approved and green; do not retain a
  deep global stack for the appearance of continuous sequencing.

Gates are never stacked on merely covered requirements. A gate branch begins
only after every blocker required by the selector is actually closed.

## The one-issue loop

Repeat this loop until the terminal criteria are satisfied.

### 1. Reconcile live state

Sync remote state and run:

```sh
just get-next-production-readiness-issue --json
```

Interpret the result carefully:

- `selected`: work only on the returned issue.
- `waiting`: inspect open PRs, reviews, CI, and merge order. Finish or merge
  the blocking stack; do not relabel work to manufacture readiness.
- `complete` with a nonzero `open_count`: the queue is fully covered by open
  closing PRs, not finished. Complete reviews and merge the remaining PRs.
- `complete` with `open_count: 0`: proceed to the terminal cross-checks.
- error or nonzero exit: diagnose taxonomy, graph, pagination, authentication,
  or GitHub state. Do not guess.

If the selector returns an issue that already has implementation in progress,
verify whether GitHub indexed the intended closing PR. Repair the PR metadata
or wait for indexing instead of opening a duplicate.

### 2. Establish the ticket contract

Read the issue and all linked guidance. Create a private working checklist
that maps:

- every acceptance criterion to a code, test, documentation, or retained
  evidence change;
- every required validation command to a planned run;
- every dependency to a closed issue or named stack predecessor;
- every non-goal to a scope boundary;
- any public compatibility or support decision to its required approver; and
- every external or irreversible action to its authorization checkpoint.

Inspect current source and tests rather than assuming the audited revision
still describes `main`. Search for overlapping open PRs and any prior art
linked from the ticket.

### 3. Capture the before state

Before implementation:

- reproduce the defect or missing control on the appropriate vulnerable
  revision when the ticket requires it;
- add or design the regression that fails for the intended reason;
- record the exact command, exit status, and concise result;
- distinguish environmental failure from proof of the defect; and
- explain when red-before-fix testing is genuinely inapplicable, such as a
  pure documentation, contract-decision, or governance ticket.

Do not leave the final PR red. Use a separate worktree or another reversible
local step when proving old behavior would otherwise disrupt the
implementation branch.

For performance work, capture the before measurement with the ticket's
release-profile `cargo bench` or `just bench-*` command before changing the
implementation. Use the same machine and release profile for the after
measurement.

### 4. Implement only the selected issue

Make the smallest complete change that satisfies the ticket. Preserve
unrelated user work. Follow repository formatting and architecture, fail
closed where required, and update every affected user-facing or maintainer
contract.

If implementation reveals a separate defect:

- determine whether it is required to satisfy the current acceptance criteria;
- if not, create a self-contained issue with reproduction, impact, required
  direction, pre-fix regression expectations, validation, acceptance criteria,
  labels, milestone, native blockers, and affected gates;
- add it atomically to the workflow universe under the work-selection
  contract; and
- rerun the selector after the current ticket reaches a stable PR boundary.

Do not hide a substantive audit finding inside an unrelated PR.

### 5. Validate before publication

Run every ticket-specific command. Then run the repository's required PR gate:

```sh
just preflight-pr
```

Run any additional compatibility, all-feature, release-mode, platform,
binding, package, sanitizer, Miri, fuzz, property, soak, or documentation
checks required by the ticket. Use the re-audit playbook to choose
surface-specific checks when the issue body is less explicit.

For performance claims, run the applicable release-profile benchmark:

```sh
cargo bench -p ferric-rules
# or the focused `just bench-*` recipe named by the ticket
```

Never report timing from `cargo test`, `cargo test --bench`, or a debug build.
Do not claim that an unavailable platform, tool, credential, or environment
passed. Record the limitation and use CI or request the required environment.

Inspect the final diff for unrelated changes, generated artifacts, secrets,
debugging output, stale documentation, and unsupported claims.

### 6. Commit and open one draft PR

Commit only the selected ticket's files. Push its issue-specific branch and
open one draft PR targeting `main`.

For an ordinary implementation or gate-evidence PR, use this body structure:

```markdown
Closes #<selected-issue>

## Scope

<Stable remediation ID, what this ticket changes, and why>

## Stack

- Immediate predecessor: <PR URL or none>
- Earlier included PRs: <URLs or none>
- Required merge order: <bottom to top>
- Tests require predecessor code: <yes/no>
- Branch point: <commit>

## Red-before-fix evidence

<Command and concise failing result, or reason not applicable>

## Validation

- `just preflight-pr` — <result>
- `<ticket-specific command>` — <result>

## Acceptance criteria

<Map every issue criterion to evidence in this PR>

## Residual risks

<None, or explicit limitations and follow-up issue links>
```

The PR body must contain exactly one GitHub closing keyword for exactly the
selected workflow issue. Put that keyword exactly once in the designated PR
body. Never put `Closes`, `Fixes`, `Resolves`, or another closing keyword for a
workflow issue in a commit message, PR title, comment, review, stack metadata,
or external-repository PR. Use `Refs #N` there.

Preparatory audit or release PRs that do not yet satisfy a selected gate use
only non-closing references. The closing keyword belongs only in the dedicated
in-repository evidence PR after that gate's acceptance criteria are true.

### 7. Verify GitHub's closing relationship

After opening or editing the PR, allow GitHub to index it and run:

```sh
gh pr view <pr-number> \
  --json baseRefName,headRefName,closingIssuesReferences,isDraft,state
```

Require all of the following before moving to another issue:

- state is `OPEN`;
- base is `main`;
- the PR remains a draft until it is ready for review;
- `closingIssuesReferences` contains exactly the selected issue; and
- the selected issue remains open.

Rerun the selector and confirm it observes the claim before beginning another
selected leaf. If a preparatory PR intentionally has no closing keyword,
require `closingIssuesReferences` to contain no workflow issue and remain on
the selected gate workflow.

If an assertion fails, correct the PR before continuing. Do not close the
issue manually as a substitute.

### 8. Complete review and CI

Monitor every required check. Read all review comments and inline threads,
implement actionable corrections, rerun affected focused checks and
`just preflight-pr`, and keep the PR body and stack metadata current.

Mark the PR ready only when:

- its final diff is limited to the selected ticket;
- every stack predecessor has merged;
- it is updated on current `main` and no ancestor-only change remains;
- all acceptance criteria have evidence;
- local and required hosted checks pass;
- the closing relationship remains exact; and
- every unresolved review concern is fixed or answered with a concrete
  rationale.

Do not dismiss a failing check as flaky without reproducing and documenting
the evidence. Do not merge around a review request.

### 9. Merge safely

Normal in-repository remediation and evidence merges are within this goal's
scope. Merge only when:

- every semantic and stack predecessor has merged;
- branch protection and required approvals are satisfied;
- all required checks are green on the final head;
- the final PR targets `main` and closes exactly one issue; and
- no decision, audit-independence, or release checkpoint applies.

If GitHub requires no human approval, a routine implementation or gate-evidence
PR may merge only after all checks pass and a reviewer independent of its
implementation has examined the final diff and acceptance evidence without an
unresolved blocker. Public-contract decisions, accepted-risk decisions,
protected settings, audit verdicts, and irreversible releases retain their
explicit approval requirements.

Use the repository's configured merge method. Never use an administrator
bypass. After merge:

1. poll GitHub for a bounded period to allow closing-reference and timeline
   indexing;
2. verify GitHub automatically changed the selected issue to closed;
3. inspect the closing PR relationship or issue timeline to confirm that the
   merged PR caused closure;
4. if the relationship remains absent after bounded refetching, do **not**
   close the issue manually—report the failure and request user direction;
5. update and revalidate the next descendant PR, if one exists;
6. remove the merged branch when no descendant needs it; and
7. return to the live selector.

An open PR, merged commit, checked checkbox, or passing test is not sufficient
if the workflow issue remains open.

## Decision and administrative checkpoints

The selector determines scheduling readiness; it does not supply maintainer
judgment, legal approval, credentials, platform access, release ownership, or
support policy.

- When a ticket requires choosing a CLIPS-compatible behavior, public API,
  MSRV, snapshot policy, ABI, supported platform/runtime matrix, packaging
  model, or other binding contract, research and propose one concrete
  direction. Obtain explicit maintainer approval before merging a materially
  breaking or externally binding decision.
- Repository rules, protected environments, registry trusted publishing,
  signing, provenance, and secret configuration must be verified by an
  authorized owner. Never invent a setting or include a credential in logs.
- Dependency-license, third-party-fixture, or release-identity judgments
  require evidence and the appropriate maintainer or legal decision.
- Real-device, native-platform, sanitizer, fuzz-duration, shadow-traffic, soak,
  and rollback requirements may need environments outside the current
  checkout. Record which evidence was produced where and by whom.
- Any `PASS WITH ACCEPTED RISKS` verdict requires the owner, expiry,
  user-visible documentation, mitigation, and release approval required by
  the re-audit playbook and issue #223.

A checkpoint is not permission to abandon the goal. Resume the same selected
issue after the decision, credentialed action, or external evidence is
recorded.

## Gate and release rules

### Topic and component gates

The 15 workflow gates are:

- topic gate [#225](https://github.com/plx/ferric-rules/issues/225);
- component gates
  [#211](https://github.com/plx/ferric-rules/issues/211),
  [#212](https://github.com/plx/ferric-rules/issues/212),
  [#213](https://github.com/plx/ferric-rules/issues/213),
  [#214](https://github.com/plx/ferric-rules/issues/214),
  [#215](https://github.com/plx/ferric-rules/issues/215),
  [#216](https://github.com/plx/ferric-rules/issues/216),
  [#217](https://github.com/plx/ferric-rules/issues/217),
  [#218](https://github.com/plx/ferric-rules/issues/218),
  [#219](https://github.com/plx/ferric-rules/issues/219),
  [#220](https://github.com/plx/ferric-rules/issues/220),
  [#221](https://github.com/plx/ferric-rules/issues/221), and
  [#222](https://github.com/plx/ferric-rules/issues/222);
- independent audit gate [#223](https://github.com/plx/ferric-rules/issues/223);
  and
- program gate [#224](https://github.com/plx/ferric-rules/issues/224).

Start a topic or component gate only when selected by the live tool after all
of its native blockers close. Execute its aggregate acceptance criteria and
create a dedicated evidence PR that closes only that gate.

Use the artifact named by the gate ticket. If it names no repository artifact,
add a concise dated record under `docs/audits/` mapping every acceptance
criterion to merged PRs, commands, hosted checks, and retained evidence. Do not
open an empty or no-op PR merely to obtain a closing relationship.

If a new finding invalidates a closed gate, reopen that gate, attach the new
issue through the correct native relationships, and block downstream audit or
release work. After the new work closes, rerun the gate's aggregate criteria
and close it again through a new evidence PR. Never leave a stale closed gate
as apparent proof.

### Independent re-audit gate #223

Execute #223 only after all 12 component-epic blockers close. Freeze an exact
candidate commit and declared release surface. Run the audit from a fresh
checkout and fresh session/context, using an auditor who did not implement the
remediation sequence and separate compatibility, safety, performance, and
release reviewers as required by the playbook.

Follow every applicable section of
[`production-readiness-reaudit.md`](production-readiness-reaudit.md). Audit
the built package artifacts, not developer-worktree substitutes. Retain the
checksummed evidence manifest and the command, environment, differential,
sanitizer, fuzz, Miri, binding, package, benchmark, soak, fault, shadow, and
rollback evidence required by the declared surface.

An audit-in-progress PR uses only `Refs #223`. Add `Closes #223` only to the
dedicated final report PR after the committed decision records:

- `PASS` with every blocking gate passed; or
- `PASS WITH ACCEPTED RISKS` with every exception explicitly owned, expiring,
  documented, mitigated, and approved as required by #223 and the playbook.

A `FAIL` or `INCONCLUSIVE` decision must not close #223 or authorize a
production-readiness claim. Create separate workflow issues for substantive
findings; reconnect them to the affected gates with native dependencies;
reopen any invalidated gates; and fix them outside the audit PR. Freeze a new
candidate and rerun all affected audit evidence afterward.

Every candidate-affecting change must land before the #223 candidate is
frozen. This includes source, tests, manifests, lockfiles, versions, package
inputs, generated declarations, release workflows, and release controls. The
audit report may land afterward as evidence while naming the exact earlier
candidate commit.

After #223 closes, no candidate-affecting change may land before stable
publication. If one becomes necessary, stop publication, reopen #223 and every
affected component gate, land and validate the change, freeze a new candidate,
and run a fresh independent audit. An old PASS cannot authorize changed bytes.

### Staged artifacts and irreversible publication

Distribution and release tickets may build, pack, sign, repair, inspect,
install, and smoke-test exact release artifacts before #223. Prefer dry runs,
local registries, TestPyPI, prerelease channels, or other explicitly
non-production staging when those satisfy the ticket contract. The artifacts
audited by #223 must be byte-identical to the artifacts intended for stable
publication, or be reproducibly tied to the same immutable candidate with a
documented signing/provenance step.

Treat stable publication to crates.io, npm, PyPI, repository tags, a GitHub
Release, a Go module version, or another public package/SDK channel as an
irreversible external action. Immediately before the first such action,
present the user with:

- the exact audited candidate commit;
- the #223 signed decision and evidence-manifest digest;
- the declared version and complete release surface;
- the exact crates, CLI artifacts, C SDK, Python distributions, Node packages
  and native addons, Go module/tag, and GitHub artifacts to be published;
- artifact checksums and provenance/signing evidence;
- the protected workflow, environment, and registry identities that will
  publish;
- dry-run and clean-install evidence for every declared surface; and
- every remaining accepted risk or deviation.

Obtain explicit maintainer confirmation unless a protected release environment
itself supplies the required human approval for this exact candidate and
artifact set. The broad `/goal` invocation does not waive final release
authorization.

Publish only the surfaces defined by the landed issue contracts and #223
release declaration. Do not infer a registry, owner, version, target, support
promise, or release channel. Verify the exact live artifacts in clean
consumers after publication and retain their immutable URLs and digests.

If any stable artifact differs from the audited bytes or reproducible
candidate derivation, stop, record the discrepancy, and invalidate the audit
before continuing.

### Final program gate #224

Issue #224 is last. Start it only after #223 has closed with an approved
production-readiness decision and the stable release checkpoint has completed
for the declared surfaces.

Run #224's completion criteria, verify live clean-consumer installation for
every published package and artifact, and ensure public CLIPS compatibility,
platform, safety, concurrency, ownership, and performance claims do not exceed
the retained evidence. Commit a dated final program record under
`docs/audits/` that links:

- every topic and component gate;
- the #223 decision and evidence manifest;
- the immutable candidate, release tag, and release artifacts;
- registry/package pages and artifact checksums;
- clean-install and smoke evidence; and
- any accepted risks, owners, expiry dates, and follow-up issues.

Close #224 only through that dedicated merged evidence PR. If the selected
release surface intentionally excludes stable public publication, obtain an
explicitly approved revision to #224's completion contract before claiming the
program is complete.

## Continuity across turns and compaction

GitHub and committed files are the durable source of truth. Never rely only on
conversation memory or an untracked note.

At every handoff or resumed turn:

1. reread this runbook and the work-selection contract;
2. inspect `git status`, current branch, upstream, and worktree ownership;
3. inspect the selected issue and current PR;
4. record the issue number and stable ID, branch, PR URL, stack predecessor,
   final test status, review status, and next action in the goal progress
   update;
5. verify those facts against GitHub rather than assuming they are unchanged;
   and
6. continue the current one-issue loop before selecting more work.

Keep every unfinished change on a named, pushed ticket branch or in a clearly
reported local worktree. Do not leave critical progress only in temporary
files.

## Stop and ask conditions

Pause for user direction when:

- the selected ticket contains a material public-contract decision with
  multiple valid outcomes and no decision is recorded;
- satisfying the ticket requires a destructive migration or external state
  not authorized here;
- branch protection, required review, or a genuine failing check cannot be
  satisfied without an override;
- the selector repeatedly fails closed and safe read-only investigation cannot
  establish why;
- a dependency, workflow membership, or closing relationship appears wrong
  and changing it would alter program scope;
- a required credential, registry, native platform, hardware environment,
  traffic source, repository, owner, approver, or legal decision is
  unavailable;
- #223 lacks the required independent auditor or reviewers;
- a `PASS WITH ACCEPTED RISKS` decision lacks a required owner, expiry,
  documentation, mitigation, or approval;
- stable publication reaches the irreversible release checkpoint; or
- the intended release version, channel, destination, or support surface is
  not explicit.

Do not ask merely because a ticket is difficult, a stack needs rebasing, CI
takes time, or the program is long.

## Terminal completion criteria

Mark the goal complete only when all of the following are true:

- every issue in the live `workflow:production-readiness` cohort, including
  #224 and every issue discovered during remediation or re-audit, is closed;
- every workflow issue timeline shows closure by its dedicated merged PR, not
  a direct state change;
- the selector returns `status: complete`, `open_count: 0`,
  `covered_count: 0`, and `ready_count: 0`;
- no remediation, gate, audit, release-evidence, or intentional stack PR
  remains open;
- merged ticket branches are removed unless repository policy retains them;
- #223 records `PASS` or explicitly approved `PASS WITH ACCEPTED RISKS` for
  the exact released candidate, with the immutable evidence bundle retained;
- the released source, version/tag, checksums, provenance, packages, native
  artifacts, and declared support surface all agree;
- clean consumers can install and smoke every declared Rust/CLI, C, Python,
  Node, and Go release surface;
- public compatibility, platform, concurrency, safety, ownership, and
  performance claims match the retained evidence;
- a clean checkout of final `main` passes `just preflight-pr` and every
  additional final audit/release validation required by the declared surface;
  and
- the final response provides issue, PR, audit, release, artifact, checksum,
  and validation links sufficient for another maintainer to reproduce the
  result.

Queue `complete` with covered open issues is not terminal completion. A
successful package upload while #223 or #224 remains open is not terminal
completion. Do not mark the goal achieved early.
