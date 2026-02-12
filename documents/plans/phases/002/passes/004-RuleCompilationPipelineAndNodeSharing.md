# Pass 004: Rule Compilation Pipeline And Node Sharing

## Objective

Compile interpreted rules into rete structures with canonical alpha/join node sharing and explicit compiler orchestration.

## Scope

- `ReteCompiler` introduction and compile workflow from Section 6.7.
- Alpha and join canonical keying (`AlphaTestKey`, `JoinNodeKey`).
- Positive-pattern compile path only (negative/NCC/exists added later).

## Tasks

1. Add `ReteCompiler` with per-network key caches for alpha test paths and join structures.
2. Implement positive-pattern compilation pipeline: build alpha paths -> build beta joins -> connect terminal.
3. Request/initialize alpha memory indices needed by compiled joins.
4. Wire compile invocation into rule load/registration flow so newly loaded rules become executable.
5. Add tests for shared-node reuse across multiple rules and stable terminal registration.

## Definition Of Done

- Typed rules compile automatically into executable rete paths.
- Equivalent alpha/join structures are shared rather than duplicated.
- Compiler output is deterministic for identical rule inputs.

## Verification Commands

- `cargo test -p ferric-core rete`
- `cargo test -p ferric-runtime integration_tests`
- `cargo check --workspace`

## Handoff State

- End-to-end rule compilation exists for positive-pattern Phase 2 subset.
