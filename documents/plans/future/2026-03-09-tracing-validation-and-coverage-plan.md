# Tracing Coverage, Validation, and Performance Plan (2026-03-09)

## Context
Tracing has been integrated across core/runtime surfaces. Immediate stabilization now avoids per-frame span growth in recursive `eval` paths. Next work should tighten event quality, test coverage, and performance characterization.

## Objectives
- Define the minimum event/span contract we rely on for debugging/profiling.
- Add tests that assert critical trace signals are emitted.
- Quantify tracing overhead with feature off vs feature on.

## Trace Contract (Initial)
### Required spans/events
- `eval_root` span: exactly one per top-level evaluator entry.
- `eval_call` event: one per call expression.
- engine lifecycle: `engine_load_str_complete` / `engine_run_complete`.
- rule/action outcomes: skipped-by-test, action errors, short-circuit on reset/clear.
- recursion-limit event: emitted when depth guard triggers.

### Optional follow-up events
- richer callable dispatch metadata (method counts, selected method rank).
- structured counters for per-run event aggregation.

## Testing Plan
### Unit-level tracing tests
- evaluator tests with an in-memory subscriber/layer to assert:
  - root span count per top-level eval call,
  - recursive call events are present,
  - root guard resets correctly after errors.

### Integration-level tracing tests
- add tracing-feature tests that run small programs and assert lifecycle events:
  - load success/failure emits expected completion/failure events,
  - run termination reason maps to emitted `engine_run_complete` fields.

### Negative-path tests
- test-condition eval failures emit diagnostic events.
- action execution errors emit per-action events.

## Performance Plan
### Off-path (`tracing` feature disabled)
- ensure new tracing control flow is `#[cfg(feature = "tracing")]` gated.
- benchmark no-feature builds against prior baseline for evaluator-heavy workloads.

### On-path (`tracing` feature enabled)
- benchmark with no subscriber, fmt subscriber, and chrome layer.
- measure runtime overhead, allocation deltas, and event volume on representative suites.

## CI / Rollout
1. Add tracing-feature test target in CI (`cargo test -p ferric-runtime --features tracing`).
2. Keep perf monitoring in benchmark CI; add tracing-on benchmark job as informational.
3. Require trace-contract tests for future instrumentation changes.

## Exit Criteria
- Contract tests cover critical evaluator and engine lifecycle signals.
- No stack-overflow regression in tracing-enabled recursion tests.
- Off-path overhead remains statistically insignificant.
