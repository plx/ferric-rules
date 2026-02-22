# Pass 003: Cross-Module `deffunction` And `defglobal` Visibility Enforcement

## Objective

Enforce `defmodule` import/export visibility rules for cross-module `deffunction` and `defglobal` usage on unqualified lookup paths.

## Scope

- Runtime ownership tracking for functions and globals by module.
- Visibility checks on callable/global lookup paths.
- Source-located diagnostics for visibility violations.

## Tasks

1. Record owning module metadata for registered `deffunction` and `defglobal` definitions.
2. Update function/global resolution paths to require visibility when crossing module boundaries.
3. Apply visibility checks uniformly for RHS evaluation, `test` CE evaluation, deffunction bodies, and generic method bodies.
4. Emit explicit diagnostics for not-visible lookups, including caller/callee module context.
5. Add integration coverage for visible and not-visible cross-module function/global scenarios.

## Definition Of Done

- Cross-module function/global access obeys module visibility rules.
- Visibility failures are explicit and source-located.
- Existing same-module behavior remains unchanged.

## Verification Commands

- `cargo test -p ferric-runtime evaluator`
- `cargo test -p ferric-runtime phase3_integration_tests`
- `cargo check --workspace`

## Handoff State

- Unqualified cross-module function/global behavior is compatibility-aligned and test-backed.
