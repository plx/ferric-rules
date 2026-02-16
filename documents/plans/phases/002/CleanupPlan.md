# Phase 002 Focused Cleanup Plan

## Goal

Improve code quality, maintainability, and idiomatic Rust usage for the Phase 2 implementation **without changing externally observable behavior**.

## Scope Guardrails

- Preserve current Phase 2 semantics and test outcomes.
- No new language features in this cleanup pass.
- Any behavior-affecting change requires explicit sign-off and separate tracking.

## Priority Order

### 1) Validation-path consolidation (highest)

- Consolidate compiler validation entry paths so `compile_rule` and `compile_conditions` use a single internal validation flow with minimal duplication.
- Centralize unsupported-structure error construction to avoid repeated string templates.
- Add focused unit tests for validation equivalence between rule-based and condition-based compile APIs.

**Exit criteria**
- No duplicated validation branches for the same rule shape.
- Tests prove identical validation outcomes across compile entry points.

### 2) Parser pattern interpretation refactor

- Continue decomposition of `stage2` pattern parsing into small helpers (`conditional`, `relation`, `template`, `ordered`).
- Reduce branching density in `interpret_pattern` call chain to improve readability and lower future regression risk.
- Keep existing parser error spans/messages stable where practical.

**Exit criteria**
- Pattern interpretation logic split into focused helpers with clear responsibilities.
- Existing parser test suite remains green with no semantic drift.

### 3) Loader translation ergonomics

- Standardize loader compile-error helpers to avoid ad hoc formatting and reduce per-branch boilerplate.
- Minimize repetitive unsupported-form plumbing in `translate_pattern` and `translate_constraint`.
- Add/adjust table-driven tests for unsupported forms to make future extensions safer.

**Exit criteria**
- Translation code paths are shorter and less repetitive.
- Unsupported-form regression tests are easy to extend.

### 4) Compiler/rete readability and invariant coverage

- Extract local helper routines where join/NCC compilation paths are dense.
- Add targeted tests for join-sharing cache behavior under mixed-rule compilation order.
- Add brief inline comments only where invariants are non-obvious (especially NCC partner/result ownership paths).

**Exit criteria**
- Join/NCC code paths are easier to follow with unchanged behavior.
- Additional tests lock in structural sharing and NCC invariants.

### 5) Quality-gate hardening

- Ensure CI runs and enforces:
  - `cargo fmt --all --check`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo test --workspace`
  - `cargo check --workspace --all-targets`
- Add contributor-facing note (if missing) that local changes should pass the same gate set before commit.

**Exit criteria**
- Gate set is documented and consistently enforced in CI.
- Local/CI mismatch risk is reduced.

## Execution Plan

1. Land cleanup items 1-2 first (highest maintainability payoff, lowest behavior risk).
2. Land items 3-4 as small, reviewable commits with tests.
3. Finish with item 5 and verify full gate pass.

## Done Definition

- Full workspace gate set passes.
- No behavior regressions in phase fixtures/integration tests.
- Cleanup changes are documented in phase notes with a short before/after summary.
