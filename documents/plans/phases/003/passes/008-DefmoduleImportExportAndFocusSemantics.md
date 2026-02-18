# Pass 008: `defmodule` Import/Export And Focus Semantics

## Objective

Implement module scoping and import/export behavior so definitions resolve deterministically across modules and focus changes.

## Scope

- Runtime module registry and visibility rules.
- Import/export resolution for shared symbols across modules.
- Focus-stack/current-module integration for rule execution.

## Tasks

1. Implement runtime storage for module definitions and import/export tables.
2. Enforce deterministic name resolution for rules/templates/functions/globals across module boundaries.
3. Integrate module semantics with `focus`/`set_focus` behavior and agenda selection expectations.
4. Emit source-located diagnostics for unresolved imports, ambiguous symbols, and invalid module references.
5. Add integration tests for cross-module visibility, focus changes, and rule firing behavior under module boundaries.

## Definition Of Done

- Modules and import/export behavior are executable and deterministic.
- Focus operations respect module semantics without bypassing agenda invariants.
- Cross-module resolution failures produce clear diagnostics.

## Verification Commands

- `cargo test -p ferric-runtime execution`
- `cargo test -p ferric-runtime integration_tests`
- `cargo check --workspace`

## Handoff State

- Module system baseline is complete for advanced language features.

