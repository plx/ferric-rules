# Pass 005: `deffunction`/`defgeneric` Conflict Diagnostics

## Objective

Replace Phase 3 precedence behavior with explicit definition-time conflict diagnostics for same-name `deffunction` and `defgeneric` definitions.

## Scope

- Loader-time conflict detection policy and diagnostics.
- Both definition orders (`deffunction` then `defgeneric`, and vice versa).
- No dispatch-order behavior changes in this pass.

## Tasks

1. Define and implement conflict detection for same-name `deffunction`/`defgeneric` definitions under module namespace rules.
2. Apply checks during construct registration before runtime dispatch tables are mutated.
3. Emit source-located diagnostics that clearly identify both symbol name and conflicting construct kinds.
4. Add regression tests for both definition orders and mixed-module visibility edge cases where relevant.
5. Remove or update any legacy precedence assumptions from docs/tests.

## Definition Of Done

- Same-name `deffunction`/`defgeneric` conflicts fail at definition time.
- Diagnostics are stable, explicit, and source-located.
- Legacy precedence behavior is no longer observable.

## Verification Commands

- `cargo test -p ferric-runtime loader`
- `cargo test -p ferric-runtime phase3_integration_tests`
- `cargo check --workspace`

## Handoff State

- Callable namespace conflict policy is compatibility-aligned and enforceable.
