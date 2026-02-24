# 007 And Conditional Element

## Sequence Position
7/10 (medium; partial support exists for `not(and ...)`, but standalone `and` still diverges).

## Behavioral Divergence
CLIPS treats `(and ...)` as a grouping conditional element. Top-level `and` should be equivalent to listing those subpatterns directly, while `not(and ...)` expresses conjunctive negation.

Ferric currently parses `Pattern::And`, and already supports `not(and ...)` translation, but still rejects standalone top-level `and` in loader translation.

## Apparent Ferric-Side Root Cause
`crates/ferric-runtime/src/loader.rs`:
- `translate_condition()` explicitly errors on top-level `Pattern::And`.
- `translate_pattern()` also treats `Pattern::And` as unsupported except within the `not(and ...)` branch.

So parser support exists, but loader translation remains intentionally restricted.

## Implementation Plan
1. Flatten top-level `Pattern::And` during rule translation.
- In `translate_rule_construct()`, detect top-level `Pattern::And` and push each child pattern as an independent translated condition in original order.
- Keep existing `not(and ...)` NCC translation path unchanged.
- Caveat: flattening may still reveal unsupported child pattern forms that were previously hidden by early rejection.

2. Preserve semantic checks for invalid nested forms.
- Retain clear errors for unsupported combinations (for example nested negation patterns that core compiler still cannot represent).
- Ensure spans reference the relevant subpattern.
- Caveat: correctness here reduces false positives but does not guarantee complete CLIPS parity for every nested CE arrangement.

3. Add loader and integration tests for top-level `and`.
- Add positive tests for:
  - `(and (test ...))`
  - `(and (initial-fact) (test ...))`
  - mixed fact/test combinations.
- Keep existing `not(and ...)` tests passing.
- Caveat: these tests only prove selected patterns and may not cover all test-suite corner cases.

4. Validate with official test-suite files using `and` CEs.
- Use `joinshre.clp`, `pataddtn.clp`, and `tceplace.clp` for smoke checks.
- Caveat: these files may still fail on unrelated features even after `and` support lands.

## Test And Verification
1. Unit/integration tests:
```bash
cargo test -p ferric-parser interpret_negation_conjunction_pattern
cargo test -p ferric-runtime load_rule_with_not_and_compiles
```
(add new positive tests for standalone `and` behavior)

2. CLI smoke checks:
```bash
cargo run -p ferric-cli -- check tests/examples/clips-official/test_suite/joinshre.clp
cargo run -p ferric-cli -- check tests/examples/clips-official/test_suite/tceplace.clp
```
Expected near-term outcome: standalone `and` compile rejection should disappear; remaining incompatibilities may still block full execution.
