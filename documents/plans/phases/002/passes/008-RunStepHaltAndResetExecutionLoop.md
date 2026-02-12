# Pass 008: `run`, `step`, `halt`, And `reset` Execution Loop

## Objective

Implement the engine execution loop so compiled activations can be fired under run limits and operational controls.

## Scope

- Runtime execution APIs from Section 9.2 (`run`, `step`, `halt`, `reset`).
- Terminal activation dispatch and run-result accounting.
- Reset behavior for fact clearing and deffacts reassertion.

## Tasks

1. Implement `Engine::step` to pop the next activation, fire one rule action list, and report result state.
2. Implement `Engine::run` with `RunLimit` handling and halted/empty-agenda termination behavior.
3. Implement `Engine::halt` request state and check it between firings.
4. Implement `Engine::reset` semantics: clear runtime fact/token/agenda state and reassert registered `deffacts`.
5. Add tests for run limits, halt behavior, empty agenda behavior, and reset/reassert cycles.

## Definition Of Done

- Engine can execute compiled activations through `run` and `step`.
- Halt and run-limit behavior is deterministic and test-backed.
- Reset semantics are correct for Phase 2 construct set.

## Verification Commands

- `cargo test -p ferric-runtime engine`
- `cargo test -p ferric-runtime integration_tests`
- `cargo check --workspace`

## Handoff State

- Runtime execution control flow is in place for action execution work.
