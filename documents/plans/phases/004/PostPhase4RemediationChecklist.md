# Phase 4 Post-Remediation Checklist

This checklist sequences remaining cleanup work into small, reversible commits.

## Baseline

- Safe batch already landed in commit `56f516a`.
- Constraint for all remaining work: no intentional observable behavior changes unless explicitly called out.

## Commit Queue

### R1: InterpretError kind accuracy

- Goal: stop hardcoding `InterpretErrorKind::ExpectedConstruct` in `InterpretError::expected`.
- Files: `crates/ferric-parser/src/stage2.rs`.
- Approach: add kind-aware constructor helpers or pass kind explicitly at call sites.
- Validation: `cargo test -p ferric-parser`; verify any tests asserting error kinds.
- Risk: low-to-medium (diagnostic shape changes).

### R2: Consolidate callable body execution path

- Goal: remove duplicated "translate + inner context + execute body" logic across user-function, generic, and `call-next-method` dispatch.
- Files: `crates/ferric-runtime/src/evaluator.rs`.
- Approach: extract one internal helper used by all dispatch paths.
- Validation: `cargo test -p ferric-runtime phase4_integration_tests::fixture_phase4_generic_dispatch -- --nocapture` and full `cargo test -p ferric-runtime`.
- Risk: medium (control-flow refactor in core evaluator path).

### R3: Introduce evaluator/action environment carrier

- Goal: replace large repeated parameter lists in RHS/action evaluation helpers with a shared context struct.
- Files: `crates/ferric-runtime/src/actions.rs` (and minimal touchpoints in callers).
- Approach: define an internal context struct containing shared refs and pass it through helper APIs.
- Validation: `cargo test -p ferric-runtime`; focus on phase2/phase3/phase4 integration fixtures.
- Risk: medium (signature churn).

### R4: Reduce hot-path cloning in activation execution

- Goal: avoid cloning full `Token` and `CompiledRuleInfo` in `execute_activation_actions`.
- Files: `crates/ferric-runtime/src/engine.rs` (possibly `crates/ferric-runtime/src/actions.rs`).
- Approach: restructure borrows and/or use cheap cloned handles (`Arc`/`Rc`) where needed.
- Validation: `cargo test -p ferric-runtime`; add micro-benchmark or counter-based sanity check if practical.
- Risk: medium-to-high (borrow/lifetime-sensitive code path).

### R5: Shared qualified-name representation across parser/runtime

- Goal: remove repeated runtime reparsing by carrying parsed qualified names from parser AST.
- Files: `crates/ferric-parser/src/stage2.rs`, `crates/ferric-runtime/src/evaluator.rs`, `crates/ferric-runtime/src/qualified_name.rs`, related construct types.
- Approach: introduce a shared parsed identifier type/adapter and migrate call sites incrementally.
- Validation: `cargo test --workspace`; phase4 module-qualified fixtures must remain green.
- Risk: high (cross-crate surface change).

### R6: Unify lexical/parse diagnostic structs

- Goal: reduce duplication between `LexError` and `ParseError` while keeping current messages/spans.
- Files: `crates/ferric-parser/src/error.rs`.
- Approach: introduce shared diagnostic payload or trait-backed display implementation.
- Validation: `cargo test -p ferric-parser`.
- Risk: low.

## Execution Rule

- Land each item as its own commit.
- After each commit: run the scoped tests listed above; if anything regresses, revert only that commit.
