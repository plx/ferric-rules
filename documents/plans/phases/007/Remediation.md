# Phase 007 Remediation Plan: Go Bindings Hardening

## Scope

This document tracks remediation work for issues found in the initial Go bindings delivery from Phase 007.

Goals:
- Eliminate correctness bugs in the public Go API.
- Restore alignment with the Phase 007 plan's intended semantics.
- Add stronger static-analysis coverage in local workflows and CI.

## Findings To Remediate

1. Multifield wire inputs fail at assertion time (`WireValueMultifield` is accepted in the wire schema but rejected by assertion conversion).
2. Structured assertion paths leak C-allocated value memory for symbol/string values.
3. Coordinator shutdown can drop accepted work and return "closed" for in-flight requests.
4. `Evaluate` maps CLIPS `"t"` output to `"stdout"` instead of reading the `"stdout"` channel directly.
5. `Evaluate` and `EvaluateNative` panic on nil request pointers.
6. `Engine.Close` marks an engine closed before confirming free success, making close errors unrecoverable.
7. Error translation does not map all exposed sentinel categories (notably thread-violation and invalid-argument).
8. Minor intent drift from plan extension seams (dispatch-policy evolution surface and explicit version targets).

## Remediation Order

1. API safety + crash prevention
- Add nil guards in `Manager.Evaluate` and `Manager.EvaluateNative` returning `ErrInvalidArgument`-class errors.
- Add tests covering nil-request handling.

2. Memory ownership correctness
- Ensure all `ffi.Value` instances created for assertion are released after FFI calls (including error paths).
- Add regression tests that exercise symbol/string/template assertion loops to prevent reintroduction.

3. Lifecycle and shutdown semantics
- Rework coordinator shutdown flow so queued/accepted requests are drained deterministically.
- Ensure `Do` returns request results for accepted work, and only fails fast for requests not accepted.
- Add race and shutdown-order tests matching the documented contract.

4. Execution contract + channel correctness
- Align output channel handling with stable wire contract (`stdout`/`stderr`) while preserving CLIPS `"t"` compatibility where appropriate.
- Add tests for output-channel mapping behavior.

5. Engine close semantics
- Update `Engine.Close` so "closed" state is committed only after successful free, with explicit handling for thread-affinity failures.
- Add close-retry and thread-violation tests.

6. Error taxonomy completion
- Map missing FFI error codes to concrete/sentinel Go errors.
- Add `errors.Is` coverage tests for thread violation and invalid argument paths.

7. Plan-intent cleanups
- Reconcile dispatch-policy surface with intended extension seam or document intentional deviation.
- Reconcile version target wording and implementation metadata.

## Static Analysis Program

Add and enforce one local/CI command (`just go-lint`) that runs:
- `golangci-lint` with `--enable-all`
- `workflowcheck` (Temporal determinism checker)
- `golangci-lint-temporalio` via `go vet -vettool` (Temporal invocation/type checks)

Expected coverage against findings:
- Likely: nil-request panic risks (item 5) via nilness/static checks.
- Possible but unlikely: some close/error-path issues via generalized linters.
- Unlikely: semantic runtime issues (items 1-4, 6-8), which still require focused tests and code review.

## Exit Criteria

- All remediation items above are implemented.
- New/updated tests pass (`go test ./...` and `go test -race ./...`).
- `just go-lint` passes locally and in CI.
- Plan/progress docs reflect final reconciled behavior.
