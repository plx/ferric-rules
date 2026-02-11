# Pass 003: Facts, Bindings, And Engine Skeleton

## Objective

Build foundational engine data structures and APIs for facts/assert/retract with thread-affinity groundwork.

## Scope

- Fact model and fact storage from Section 5.2.
- Variable/binding scaffolding from Section 5.4 (minimal subset required now).
- Engine shell from Section 9.2 (Phase 1 subset only).

## Tasks

1. Implement `FactId` via `slotmap::new_key_type!`, `Fact`, `FactEntry`, and `FactBase`.
2. Add basic fact indices (`by_relation`, `by_template`) and timestamp management.
3. Implement minimal `VarId`, `VarMap`, `BindingSet` types needed by token/join work.
4. Create engine scaffold with:
   - `Engine::new`
   - `assert`
   - `retract`
   - fact query helpers (`get_fact`, iterator)
5. Introduce thread-affinity structure:
   - compile-time `!Send + !Sync` marker strategy
   - `creator_thread_id` tracking and entry-point check hook
6. Add unit tests for fact insertion/removal index correctness and timestamp progression.

## Definition Of Done

- Engine can store and retract facts safely.
- Data model required for parser/loader and rete work is in place.

## Verification Commands

- `cargo test -p ferric-runtime`
- `cargo check --workspace`

## Handoff State

- Assert/retract API exists and is test-backed.
- No stale fact index entries after retraction in implemented paths.
