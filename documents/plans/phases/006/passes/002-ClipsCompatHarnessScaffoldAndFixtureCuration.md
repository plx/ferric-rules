# Pass 002: CLIPS Compatibility Harness Scaffold And Fixture Curation

## Objective

Create the CLIPS compatibility harness and curated fixture inventory that all compatibility passes will extend.

## Scope

- Compatibility runner structure and fixture discovery model.
- Initial fixture taxonomy by semantic domain.
- Deterministic execution conventions for compatibility assertions.

## Tasks

1. Implement compatibility harness scaffolding under `tests/clips_compat` (shared runner utilities, fixture loader, expected-outcome assertions).
2. Curate initial fixture sets by category: core matching/retraction, negation/exists/NCC, module/global resolution, generic dispatch, stdlib behavior.
3. Define deterministic assertion conventions for outputs/diagnostics (normalization rules, stable ordering expectations where applicable).
4. Add harness docs describing how to add a new compatibility case and expected artifacts.
5. Add a smoke compatibility job/command path proving harness wiring works in CI/local runs.

## Definition Of Done

- Compatibility harness is runnable and extensible.
- Fixture inventory is organized by semantic domain.
- At least one compatibility smoke path executes end-to-end.

## Verification Commands

- `cargo test --test clips_compat_smoke`
- `cargo check --workspace`

## Handoff State

- Core compatibility semantics can now be added incrementally without harness rework.
