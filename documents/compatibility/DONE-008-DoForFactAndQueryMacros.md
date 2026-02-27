# 008 Do-For-Fact And Query Macros

## Sequence Position
8/10 (medium-to-large; introduces new syntax family and runtime iteration semantics).

## Behavioral Divergence
CLIPS supports fact-query macros whose first argument is a binding list, for example:
- `do-for-fact`
- `do-for-all-facts`
- `delayed-do-for-all-facts`
- `any-factp`
- `find-fact`
- `find-all-facts`

Ferric currently expects regular function-call syntax (leading symbol then positional args), so these forms fail early with `expected function name (symbol)`.

## Apparent Ferric-Side Root Cause
`interpret_function_call()` in `crates/ferric-parser/src/stage2.rs` requires `list[0]` to be a symbol and does not recognize macro-specific binding-list grammar.

Runtime execution in `crates/ferric-runtime/src/actions.rs` and evaluation in `crates/ferric-runtime/src/evaluator.rs` also lack dedicated semantics for fact query iteration, delayed execution, and fact-address-returning query forms.

## Implementation Plan
1. Add dedicated AST nodes for fact-query macro forms.
- Extend Stage 2 action representation with a structured type for query macros (binding specs, query expression, and body/action block).
- Keep regular function-call parsing intact for non-macro forms.
- Caveat: parsing support alone will not execute query macros correctly yet.

2. Implement parser support for binding-list signatures.
- Parse forms like `((?var template))` (and multi-binding variants where supported).
- Preserve spans for both binding declarations and query/body segments.
- Caveat: successful parse may still fail if runtime semantics are incomplete.

3. Implement runtime semantics in action/evaluator paths.
- `do-for-fact` and `do-for-all-facts`: iterate matching facts, bind variables, execute body.
- `delayed-do-for-all-facts`: evaluate selection first, execute body after capture.
- `any-factp`/`find-fact`/`find-all-facts`: return CLIPS-compatible truthy/fact collection results.
- Integrate with existing fact-base/query and module visibility constraints.
- Caveat: even with macro execution, slot-reference syntax and advanced query forms may still require follow-up work.

4. Add isolation tests before corpus-level validation.
- Parser tests for each macro signature.
- Runtime tests that distinguish immediate vs delayed mutation behavior.
- Tests for zero-match behavior and return values.
- Caveat: narrow regression tests will not cover every macro + control-flow interaction seen in external projects.

## Test And Verification
1. Unit/integration tests:
```bash
cargo test -p ferric-parser
cargo test -p ferric-runtime
```
(add targeted tests for all six macro forms)

2. External smoke checks:
```bash
cargo run -p ferric-cli -- check tests/examples/clips-executive/cx_tutorial_agents/clips/tf2_tracked_pose.clp
cargo run -p ferric-cli -- check tests/examples/rcll-refbox/src/games/rcll/setup.clp
```
Expected near-term outcome: macro parse errors disappear; additional runtime gaps may still surface in macro bodies.
