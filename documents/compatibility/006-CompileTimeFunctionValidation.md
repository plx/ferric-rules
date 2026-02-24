# 006 Compile-Time Function Validation

## Sequence Position
6/10 (medium; independent of parser features, but best done before broad control-flow rollout to tighten diagnostics).

## Behavioral Divergence
CLIPS rejects unknown function calls at load time with diagnostics like:
`[EXPRNPSR3] Missing function declaration for <name>.`

Ferric currently accepts many of these at load time and only emits runtime action diagnostics (or silently stores no precompiled runtime expression), which diverges from CLIPS load behavior.

## Apparent Ferric-Side Root Cause
In `crates/ferric-runtime/src/loader.rs`, `compile_rule_construct()` pre-translates actions via:
`from_action_expr(...).ok()`.

This drops translation errors and allows unresolved call sites to pass load. There is also no post-load pass validating that function names are resolvable against built-ins, `deffunction`, and `defgeneric` declarations in scope.

## Implementation Plan
1. Stop swallowing action translation errors.
- Replace `.ok()` in rule runtime action translation with explicit `Result` handling and span-rich `LoadError::Compile` messages.
- Ensure unknown function translation failures are not silently converted into `None` runtime actions.
- Caveat: this catches one class of gaps but may expose additional unresolved-call behavior in existing tests.

2. Add explicit callable-resolution validation pass.
- After construct load/registration, walk rule/deffunction/defmethod bodies recursively and validate each callable name.
- Resolve against:
  - evaluator built-ins (centralize list in one helper),
  - module-visible `deffunction`s,
  - module-visible `defgeneric`s.
- Emit CLIPS-style message shape (`[EXPRNPSR3] Missing function declaration for ...`) where applicable.
- Caveat: even with correct validation, some corpus files are intentionally non-standalone and may still fail when loaded in isolation.

3. Preserve legitimate extension points.
- For environments that inject external functions later, add an explicit loader option/allowlist path rather than silent acceptance.
- Keep default behavior CLIPS-compatible (strict validation).
- Caveat: policy decisions for extension hooks may still require follow-up based on embedding use-cases.

4. Update tests to assert load-time failure semantics.
- Convert runtime-only unknown-function tests to load-time expectation where CLIPS does so.
- Add module-visibility validation tests to ensure qualified/unqualified lookup matches current module rules.
- Caveat: test updates can still pass while message wording differs from CLIPS in edge cases.

## Test And Verification
1. Runtime/loader tests:
```bash
cargo test -p ferric-runtime unknown_function_in_rhs_produces_diagnostic
cargo test -p ferric-runtime phase4_integration_tests
```
(update expectations toward load-time validation diagnostics)

2. CLI smoke checks:
```bash
cargo run -p ferric-cli -- check tests/examples/clips-official/examples/sudoku/puzzles/grid2x2-p1.clp
cargo run -p ferric-cli -- check tests/examples/clips-executive/cx_plugins/protobuf_plugin/clips/protobuf.clp
```
Expected near-term outcome: ferric should fail at load time where CLIPS reports missing function declarations; files may still need multi-file loading context.
