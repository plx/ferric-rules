# CLIPS Compatibility Test Harness

This directory contains fixture files and documentation for the Ferric engine's
CLIPS compatibility test suite. Tests live in
`crates/ferric/tests/clips_compat.rs` and use the fixtures under
`tests/clips_compat/fixtures/`.

## How to run

```
cargo test -p ferric --test clips_compat
```

## Directory structure

```
tests/clips_compat/
├── README.md              # This file
└── fixtures/
    ├── smoke.clp          # Minimal smoke fixture used by harness self-tests
    ├── core/              # Basic fact matching, retraction, and rule chaining
    ├── negation/          # NOT CE, EXISTS CE, and FORALL CE semantics
    ├── modules/           # Defmodule, focus stack, cross-module visibility
    ├── generics/          # Defgeneric, defmethod, call-next-method dispatch
    └── stdlib/            # Standard library functions (math, string, multifield, I/O)
```

Each subdirectory contains `.clp` fixture files (one per distinct semantic
behaviour) and a `.gitkeep` placeholder until real fixtures are added.

## Harness API

The harness is defined in `crates/ferric/tests/clips_compat.rs` and provides
the following public items.

### `CompatResult`

Returned by `run_clips_compat` and `run_clips_compat_file`. Holds:
- `rules_fired: usize` — number of rules that fired
- `output: String` — captured output from the `t` (stdout) channel
- `fact_count: usize` — number of user-visible facts after execution

### `CompatEngine`

Returned by `run_clips_compat_full`. Retains the live engine after execution
for richer post-run inspection:
- `rules_fired: usize` — number of rules that fired
- `output: String` — captured output from the `t` channel
- `fn fact_count(&self) -> usize` — count user-visible facts
- `fn has_fact(&self, relation: &str) -> bool` — check if any ordered fact
  with the given relation name exists
- `fn engine(&self) -> &Engine` — borrow the underlying engine

### Runner functions

| Function | Returns | Use when |
|---|---|---|
| `run_clips_compat(source)` | `CompatResult` | Most tests; engine is dropped after capture |
| `run_clips_compat_full(source)` | `CompatEngine` | You need to inspect working memory after the run |
| `run_clips_compat_file(name)` | `CompatResult` | Load a `.clp` fixture file |
| `assert_clips_compat(source, expected)` | `()` | One-liner output assertion |

`run_clips_compat_file` accepts subdirectory paths:
```rust
run_clips_compat_file("core/basic_match.clp")
run_clips_compat_file("negation/simple_not.clp")
```

### Assertion helpers

These operate on a `&CompatResult`:

```rust
assert_output_exact(&result, "expected output\n");
assert_rules_fired(&result, 3);
assert_fact_count_compat(&result, 5);
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

Corresponding test in `clips_compat.rs`:
```rust
#[test]
fn test_core_basic_match() {
    let result = run_clips_compat_file("core/basic_match.clp");
    assert_output_exact(&result, "colour is red\n");
    assert_rules_fired(&result, 1);
}
```

## Adding a new compatibility test

1. Create or choose the appropriate subdirectory under `fixtures/`.
2. Write a `.clp` fixture file that demonstrates the behaviour.
3. Add a `#[test]` function in `crates/ferric/tests/clips_compat.rs` that:
   - Calls `run_clips_compat_file("subdir/file.clp")` (or
     `run_clips_compat_full` if you need working-memory inspection).
   - Asserts the expected output and/or rule-fired count.
4. Run `cargo test -p ferric --test clips_compat` to verify.
5. Run `cargo clippy -p ferric --all-targets -- -D warnings` and
   `cargo fmt --all` before committing.
