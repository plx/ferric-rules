# Pass 006: Negative Node (Single Pattern) And Blocker Tracking

## Objective

Implement single-pattern negation with explicit blocker tracking and correct assert/retract behavior.

## Scope

- `BetaNode::Negative` and `NegativeMemory` from Section 7.2.
- Compile path for `(not <single-pattern>)`.
- Retraction callback dispatch integration for negative memory structures.

## Tasks

1. Add negative node data structures and memory (`unblocked_tokens`, `blockers`, `blocked_by`).
2. Compile single-pattern `not` into negative nodes with required alpha subscriptions and join tests.
3. Implement negative-node runtime handlers for token arrival, blocking fact assert, and fact retract/unblock.
4. Wire `token_retracted(token_id)` cleanup callbacks into cascade dispatch and prune empty `blocked_by` sets.
5. Add tests covering blocked/unblocked transitions, activation removal on new blockers, and re-satisfaction after blocker retraction.

## Definition Of Done

- `(not <single-pattern>)` works under assert/retract churn.
- Negative memory cleanup is idempotent and free of stale token references.
- Invariant checks include negative-node structures.

## Verification Commands

- `cargo test -p ferric-core rete`
- `cargo test -p ferric-runtime integration_tests`
- `cargo check --workspace`

## Handoff State

- Negative single-pattern semantics are operational and structurally verified.
