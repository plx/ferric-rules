# Retired production-readiness goal

The owner replaced this execution program on September 6, 2026. **Do not
restart the historical goal or use its epic approvals and audit exit criteria.**
Use [the rehabilitation execution record](rehabilitation-status.md) for the
supported surface, finite work, decisions, evidence, and current next action.

The previous goal is preserved in
[repository history at the rehabilitation baseline](https://github.com/plx/ferric-rules/blob/a38de6a852cce3f503467cd000ba7b182c4b5b30/docs/audits/production-readiness-remediation-goal.md).
Its completed fixes and issue history remain valid evidence. Retiring its
additional obligations does not mean the original production audit passed.

The existing `just get-next-production-readiness-issue` command is retained.
Its default cohort is now rehabilitation; see
[the work-selection contract](production-readiness-work-selection.md).
Do not run it during a cohort migration. One coordinator prepares and verifies
all labels and native dependencies before scheduling resumes.

Continue implementation, measurement, focused review, and merges under the
owner's rehabilitation authorization. Run `just preflight-pr` before opening a
PR and before pushing an update. Respect actual branch protections and required
reviews. No public package, tag, release, paid service, or external infrastructure
publication is part of this execution.
