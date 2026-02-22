# Pass 005: FFI Core `run`/`step`/`assert`/`retract` APIs

## Objective

Expose the core execution and fact-mutation API surface needed for practical embedding.

## Scope

- Execution APIs: `ferric_run`, `ferric_step`.
- Fact APIs: `ferric_assert_string`, `ferric_retract`.
- Out-parameter contracts and diagnostic propagation.

## Tasks

1. Implement `ferric_run` with `limit` conversion semantics (`-1` unlimited) and `out_fired` reporting.
2. Implement `ferric_step` with documented status signaling (fired/empty/halted).
3. Implement `ferric_assert_string` and `ferric_retract` with stable ID/not-found behavior.
4. Route parse/compile/runtime/action diagnostics through FFI without semantic reinterpretation or source-context loss.
5. Add integration tests from C-facing calls that exercise successful execution plus module/generic diagnostic failures from Phase 4.

## Definition Of Done

- Embedders can execute rules and mutate facts through core FFI APIs.
- Return codes and out-parameter behavior are stable and test-backed.
- Phase 4 diagnostics are visible through FFI unchanged in meaning/context.

## Verification Commands

- `cargo test -p ferric-ffi engine_api`
- `cargo test -p ferric-ffi diagnostic_parity`
- `cargo check -p ferric-ffi`

## Handoff State

- Core embedding workflow is available through C APIs.
