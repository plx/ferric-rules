# FR-COMPAT-EPIC closure receipt

- Audit date: 2026-08-08
- Epic: [#213, FR-COMPAT-EPIC](https://github.com/plx/ferric-rules/issues/213)
- Candidate: `b04e94e876193a0944ab3f5bea47948c76d6dce8` on `main`
- Audit source: [2026-07-25 due diligence](2026-07-25-due-diligence.md)
- Verdict: the compatibility-evidence implementation and its isolated pull-request
  enforcement gate satisfy the epic acceptance criteria. No additional gate code
  is required to close #213.

At the time of this audit, #213 remained open with its eight Markdown checklist
boxes unchecked. It had no assignee, issue comment, claim, linked branch, or direct
closing pull request. Those presentation fields are stale: GitHub's native
dependency graph lists exactly the eight child issues below as blockers, and every
one is closed. The epic in turn blocks the independent final audit in
[#223](https://github.com/plx/ferric-rules/issues/223). The pull request carrying
this receipt is therefore the smallest honest direct closure for #213.

## Child issue and merge receipt

| Stable ID | Child issue | Closing pull request | Merge commit | Child closed (UTC) |
|---|---|---|---|---|
| `FR-COMPAT-001` | [#88](https://github.com/plx/ferric-rules/issues/88) | [#240](https://github.com/plx/ferric-rules/pull/240) | `f91a4f2bf76bfd54f727c4b05fd262f6bdabea21` | 2026-07-26 11:48:44 |
| `FR-COMPAT-002` | [#89](https://github.com/plx/ferric-rules/issues/89) | [#241](https://github.com/plx/ferric-rules/pull/241) | `40d795eb558a3563e73b9755a6b377752ffe70b9` | 2026-07-26 13:11:00 |
| `FR-COMPAT-003` | [#90](https://github.com/plx/ferric-rules/issues/90) | [#242](https://github.com/plx/ferric-rules/pull/242) | `14573451b1410cca2f8da498713497fbf39b0b7a` | 2026-07-26 14:27:43 |
| `FR-COMPAT-004` | [#91](https://github.com/plx/ferric-rules/issues/91) | [#243](https://github.com/plx/ferric-rules/pull/243) | `02401aaa34ad067c5aca425013554ec661899658` | 2026-07-26 18:31:08 |
| `FR-COMPAT-005` | [#119](https://github.com/plx/ferric-rules/issues/119) | [#267](https://github.com/plx/ferric-rules/pull/267) | `a4a01e04011137d9eb65aeb21caf4792871a445d` | 2026-08-08 03:04:28 |
| `FR-COMPAT-006` | [#120](https://github.com/plx/ferric-rules/issues/120) | [#269](https://github.com/plx/ferric-rules/pull/269) | `7d8d3ec1fa59fd2e46299e350a53427258cb47fe` | 2026-08-08 06:56:41 |
| `FR-COMPAT-007` | [#92](https://github.com/plx/ferric-rules/issues/92) | [#270](https://github.com/plx/ferric-rules/pull/270) | `b04e94e876193a0944ab3f5bea47948c76d6dce8` | 2026-08-08 10:16:38 |
| `FR-COMPAT-008` | [#93](https://github.com/plx/ferric-rules/issues/93) | [#268](https://github.com/plx/ferric-rules/pull/268) | `05994ee66c559ba180c9f94ea8a52c9744700771` | 2026-08-08 05:27:27 |

Each listed pull request explicitly closes its child issue. None of the child
issues has an issue comment or a competing closing pull request.

## Acceptance mapping

| Epic requirement | Evidence at the candidate |
|---|---|
| Complete `FR-COMPAT-001` through `FR-COMPAT-008` | All eight native blockers are closed by the exact merges above. The implementation spans digest-bound generated harnesses, repository-contained composed input, a collision-safe `MAIN` verifier, structured oracles, phase-aware diagnostics, lexical scanning, blocking CI, and the pinned semantic lane. |
| Publish a versioned manifest contract with executable harness provenance | [Compatibility assessment oracles](../compatibility-assessment.md) documents fail-closed manifest schema v3 and oracle protocol v1. [`_harness.py`](../../tools/ferric-tools/src/ferric_tools/_harness.py) emits harness generation v2 plus source, composed-input, and harness digests; [`run.py`](../../tools/ferric-tools/src/ferric_tools/compat/run.py) validates that contract before either engine runs. |
| Preserve structured phase, state, and output oracles | [`compat-oracles.json`](../../tests/examples/compat-oracles.json) is a versioned registry of v1/v2 fixture declarations. [`oracle.py`](../../tools/ferric-tools/src/ferric_tools/compat/oracle.py) requires authenticated lifecycle evidence and feature-specific firings, effects, facts, focus/global state, output, diagnostics, and termination as declared. Equal process output alone cannot establish equivalence. |
| Allow only explicit normalization | The only supported normalizers are `fact-ids`, `fact-order`, and `float-format`; declarations select them per fixture. No global normalizer is applied. |
| Pin and identify the CLIPS reference | [`compat-semantic-policy.json`](../../tests/examples/compat-semantic-policy.json) pins suite `1.1.0`, CLIPS 6.30 package `6.30-4.1`, the base-image digest, and per-platform binary/library digests. The clean-main artifact independently records the observed image and binary/library identities below. |
| Make the requested red cases durable | [`test_compat_oracle.py`](../../tools/ferric-tools/tests/test_compat_oracle.py) rejects one-character drift, equal output with different final facts, missing completion, a verifier-only no-op, and zero fixture firings. [`test_compat_scan.py`](../../tools/ferric-tools/tests/test_compat_scan.py) covers trailing non-`MAIN` modules, verifier-name collisions, and a composed source outside the CLIPS mount. [`test_clips_oracle.py`](../../tools/ferric-tools/tests/test_clips_oracle.py) and [`test_compat_projection.py`](../../tools/ferric-tools/tests/test_compat_projection.py) preserve parse/load/reset/run phases. [`test_clips_parser.py`](../../tools/ferric-tools/tests/test_clips_parser.py) proves commands in strings, comments, and symbol substrings do not count. The closing pull requests retain the corresponding pre-fix red reproductions; the tests now lock the fixed behavior. |
| Reproduce the complete workflow from a clean checkout | The clean-main compatibility job below checked out `b04e94e`, built the candidate and pinned CLIPS image, recorded both provenances, scanned, generated and verified harnesses, ran both engines, enforced policy, reported, finalized, and uploaded evidence. Every phase completed successfully. |
| Reject vacuous equivalence and require a feature-specific effect | [`compat-ci-policy.json`](../../tests/examples/compat-ci-policy.json) requires the intentionally empty-output control to produce `MAIN::result = 42` under harness generation v2. The clean-main gate accepted 12 equivalent outcomes only with valid oracle evidence; the artifact reports zero gate failures. |
| Execute every selected harness under both engines | `--require-selected` produced all 23 policy/registry outcomes. The other 639 testable corpus entries remained explicit `pending_without_oracle`; none was silently promoted to equivalent. Candidate and CLIPS results are bound to their recorded digests. |
| Preserve diagnostic phases | [`diagnostics.py`](../../tools/ferric-tools/src/ferric_tools/compat/diagnostics.py) defines the versioned parse/load/reset/run taxonomy. The authenticated adapters retain active phase, raw diagnostic output, timeout/signal termination, and fail closed on unknown or malformed evidence. |
| Block CI on unexplained claimed-subset divergence | [`compat-standalone.yml`](../../.github/workflows/compat-standalone.yml) orders scan, generation, verification, dual-engine assessment, policy enforcement, reporting, and retained evidence. [`pr-assessment.yml`](../../.github/workflows/pr-assessment.yml) publishes the stable aggregate context `PR Compatibility Gate`. The active ruleset below requires that exact GitHub Actions context on `main`. |
| Retain candidate and CLIPS digests | Artifact `9020420327` contains both provenance JSON files, manifest v3, JSON/Markdown gate reports, TSV/Markdown assessment reports, all 11 divergent input plans, and final workflow status: 19 files in total. Exact observed identities are recorded below. |
| Keep semantic remediation visible rather than hiding it | [`compat-semantic-policy.json`](../../tests/examples/compat-semantic-policy.json) names exact equivalent or issue-linked divergent outcomes. The gate accepted 11 known deviations and no unexplained deviation; those semantic issues remain independent of this evidence-tooling epic. |

## Clean-main assessment and retained artifact

The authoritative clean-main evidence is [Main Assessment run
31252435007](https://github.com/plx/ferric-rules/actions/runs/31252435007) at the
candidate commit. Both jobs in the run are terminal with conclusion `success`.
Its [Compatibility Gate job
93090739123](https://github.com/plx/ferric-rules/actions/runs/31252435007/job/93090739123)
is terminal with conclusion `success`.

| Phase | Terminal result |
|---|---|
| Scan | 1,229 fixtures: 662 pending testable and 567 incompatible. |
| Generate and verify | 213 library-only fixtures considered; 181 harnesses generated and verified, 22 external-path and 10 empty-source entries explicitly skipped. |
| Dual-engine assessment | 23 claimed outcomes completed: 12 equivalent and 11 divergent; 639 testable entries remained `pending_without_oracle`. |
| Policy gate | `passed`: 23 claimed outcomes, 11 exact known deviations, zero failures. |
| Retention | [`compat-report`, artifact 9020420327](https://github.com/plx/ferric-rules/actions/runs/31252435007/artifacts/9020420327), 19 files, 1,778,800 compressed bytes, artifact digest `sha256:d31a9de130cc7ed8df44f2fafb6c9ae30a5e0d28a6ce1ae5b3b881561131b9d7`. |

The retained provenance files contain:

| Subject | Observed identity |
|---|---|
| Candidate source | commit `b04e94e876193a0944ab3f5bea47948c76d6dce8` |
| Candidate binary | SHA-256 `62b03cf11fe6c99622d23fe5acf79f34375376c9cba9871e5135b24277ec7d7a` |
| CLIPS reference | CLIPS 6.30, Debian package `6.30-4.1`, `linux/amd64` |
| CLIPS binary | SHA-256 `39e6bb1465ec0fdf342b21b244f4d2ac182b4849be2ce46c079648611b83fcf0` |
| CLIPS library | SHA-256 `78a49ecf81a6c8339b81ee93734d1a0b87934ad17930045886acd6b61e21923f` |
| Base image | `debian:bookworm-slim@sha256:abd67ffcfa541b485a3dff59865ab629aa048a6c613e639d36e7456b0b229241` |
| Built image | `sha256:af1c8efee10a5315b54342958aa4b8b11db858a6f46c85fcdf5b1a5cc7a3f140` |

The 11 deviations are the exact policy entries `FR-RETE-008`,
`FR-RETE-008-BREADTH`, `FR-RETE-009`, `FR-RETE-009-MEA`, `FR-RETE-010`,
`FR-RETE-011`, `FR-RETE-012`, `FR-RETE-015`, `FR-RETE-016`, `FR-RETE-017`,
and `FR-RETE-018`. They link to
[#154](https://github.com/plx/ferric-rules/issues/154),
[#155](https://github.com/plx/ferric-rules/issues/155),
[#156](https://github.com/plx/ferric-rules/issues/156),
[#157](https://github.com/plx/ferric-rules/issues/157),
[#158](https://github.com/plx/ferric-rules/issues/158),
[#160](https://github.com/plx/ferric-rules/issues/160),
[#192](https://github.com/plx/ferric-rules/issues/192),
[#193](https://github.com/plx/ferric-rules/issues/193), and
[#161](https://github.com/plx/ferric-rules/issues/161); they are disclosed
evidence, not equivalence claims.

## Isolated pull-request enforcement

Repository ruleset
[`require-pr-compatibility-gate` (ID 20584155)](https://github.com/plx/ferric-rules/rules/20584155)
was created and activated at 2026-08-08 10:15:48 UTC. Its exact scope is:

- target `branch`, include only `refs/heads/main`, with no exclusions or bypass
  actors;
- one rule only: `required_status_checks`;
- required context `PR Compatibility Gate`, GitHub Actions integration ID
  `15368`;
- `strict: false` and `do_not_enforce_on_create: true`.

GitHub's evaluated rules for `main` include that rule. The matching check on
[PR #270](https://github.com/plx/ferric-rules/pull/270) completed successfully
under integration ID `15368` in [run 31248281262, job
93080604818](https://github.com/plx/ferric-rules/actions/runs/31248281262/job/93080604818).
This connects the required context to the workflow that actually produces it,
without adding unrelated branch policy.

## Audit-era command replacements

The literal validation block in #213 describes the pre-remediation repository.
Use the current interfaces when reproducing this receipt:

| Audit-era command(s) | Result at `b04e94e` | Current replacement and observed result |
|---|---|---|
| `uv run --project tools/ferric-tools pytest -v` | From the repository root, pytest also collects the Python-binding suite and fails collection because the extension module `ferric` has not been built. | `uv run --project tools/ferric-tools pytest -v tools/ferric-tools/tests` — 631 passed. |
| `just harness-gen`; `just compat-scan`; `just compat-run`; `just compat-report` | The individual recipes remain useful, but this audit-era order generates before scanning and omits current verification, selection, provenance, and policy arguments. | `just assess-compatibility` is the supported atomic local sequence. The equivalent clean-main workflow phases all passed in run `31252435007`. |
| `cargo test -p ferric --test clips_compat -- --nocapture` | Fails because the current workspace has no package `ferric` or test target `clips_compat`. | `cargo test -p ferric-rules --test ferric_semantic_regressions -- --nocapture` — 80 passed. |

These replacements update the command surface; they do not weaken the original
acceptance criteria. The clean-main job and retained artifact above are the
authoritative end-to-end evidence.

## Closure decision

All native dependencies are clear, the complete compatibility-evidence path is
non-vacuous and reproducible, unexplained claimed-subset drift is blocking, and
the retained artifact identifies both engines. The remaining known semantic
differences continue under their own `FR-RETE-*` issues and are intentionally not
folded into this tooling epic. The receipt's merge may close #213; no source,
oracle, workflow, or ruleset change remains for this epic.
