# Ferric Semantic Regression Harness

This directory contains fixture files and documentation for the Ferric engine's
internal CLIPS-language semantic regression suite. It lives inside the
`ferric-rules` package so the packaged crate can run its complete test suite
after extraction. This suite executes Ferric only; the repository's external
differential lane owns claims about behavior relative to reference CLIPS.

## How to run

```
cargo test -p ferric-rules --test ferric_semantic_regressions
```

## Directory structure

```
crates/ferric-rules/tests/fixtures/
├── README.md              # This file
├── smoke.clp              # Minimal smoke fixture used by harness self-tests
├── core/                  # Basic fact matching, retraction, and rule chaining
├── negation/              # NOT CE, EXISTS CE, and FORALL CE semantics
├── modules/               # Defmodule, focus stack, cross-module visibility
├── generics/              # Defgeneric, defmethod, call-next-method dispatch
└── stdlib/                # Standard library functions (math, string, multifield, I/O)
```

Each subdirectory contains `.clp` fixture files (one per distinct semantic
behaviour) and a `.gitkeep` placeholder until real fixtures are added.

## Harness API

The harness is defined in
`crates/ferric-rules/tests/ferric_semantic_regressions.rs` and provides the
following public items.

### `RegressionResult`

Returned by `run_ferric_semantic_regression` and
`run_ferric_semantic_regression_file`. Holds:
- `rules_fired: usize` — number of rules that fired
- `output: String` — captured output from the `t` (stdout) channel
- `fact_count: usize` — number of user-visible facts after execution

### `RegressionEngine`

Returned by `run_ferric_semantic_regression_full`. Retains the live engine
after execution for richer post-run inspection:
- `rules_fired: usize` — number of rules that fired
- `output: String` — captured output from the `t` channel
- `fn fact_count(&self) -> usize` — count user-visible facts
- `fn has_fact(&self, relation: &str) -> bool` — check if any ordered fact
  with the given relation name exists
- `fn engine(&self) -> &Engine` — borrow the underlying engine

### Runner functions

| Function | Returns | Use when |
|---|---|---|
| `run_ferric_semantic_regression(source)` | `RegressionResult` | Most tests; engine is dropped after capture |
| `run_ferric_semantic_regression_full(source)` | `RegressionEngine` | You need to inspect working memory after the run |
| `run_ferric_semantic_regression_file(name)` | `RegressionResult` | Load a `.clp` fixture file |
| `assert_ferric_semantic_regression(source, expected)` | `()` | One-line output assertion |

Semantic-regression runs use a bounded firing limit to prevent non-quiescing
fixtures from spinning indefinitely:
- Default cap: `10_000` rule firings per run
- Local override: `FERRIC_SEMANTIC_REGRESSION_RUN_LIMIT=<N>`

If a fixture reaches the cap, the harness fails with an explicit
non-quiescence message.

`run_ferric_semantic_regression_file` accepts subdirectory paths:
```rust
run_ferric_semantic_regression_file("core/basic_match.clp")
run_ferric_semantic_regression_file("negation/simple_not.clp")
```

### Assertion helpers

These operate on a `&RegressionResult`:

```rust
assert_output_exact(&result, "expected output\n");
assert_rules_fired(&result, 3);
assert_fact_count(&result, 5);
```

All helpers panic with a descriptive message on mismatch.

## Fixture file conventions

- One `.clp` file per distinct semantic behaviour.
- Files should be self-contained: include all `deffacts`, `defrule`, and other
  constructs needed to demonstrate the behaviour under test.
- Use `printout t ... crlf` to produce output that tests can assert against.
- Name files descriptively: `basic_match.clp`, `retract_cycle.clp`, etc.
- Keep fixtures small and focused — a single fixture should test one concept.

### Example fixture

`fixtures/core/basic_match.clp`:
```clips
(deffacts startup (colour red))

(defrule report-colour
    (colour ?c)
    =>
    (printout t "colour is " ?c crlf))
```

Corresponding test in `ferric_semantic_regressions.rs`:
```rust
#[test]
fn test_core_basic_match() {
    let result = run_ferric_semantic_regression_file("core/basic_match.clp");
    assert_output_exact(&result, "colour is red\n");
    assert_rules_fired(&result, 1);
}
```

## Adding a new semantic regression

1. Create or choose the appropriate subdirectory under `fixtures/`.
2. Write a `.clp` fixture file that demonstrates the behaviour.
3. Add a `#[test]` function in
   `crates/ferric-rules/tests/ferric_semantic_regressions.rs` that:
   - Calls `run_ferric_semantic_regression_file("subdir/file.clp")` (or
     `run_ferric_semantic_regression_full` if you need working-memory
     inspection).
   - Asserts the expected output and/or rule-fired count.
4. Run `cargo test -p ferric-rules --test ferric_semantic_regressions` to
   verify.
5. Run `cargo clippy -p ferric-rules --all-targets -- -D warnings` and
   `cargo fmt --all` before committing.
