# Pass 003: Stage 2 `deftemplate`, `defrule`, And `deffacts` Interpretation

## Objective

Implement Phase 2 construct interpretation for the required top-level CLIPS forms and switch loader ingestion to typed construct outputs.

## Scope

- Construct semantics for `deftemplate`, `defrule`, and `deffacts`.
- Loader/runtime integration for typed construct ingestion.
- Source-located diagnostics for malformed forms.

## Tasks

1. Implement `interpret_template`, `interpret_rule`, and `interpret_facts` for the supported Phase 2 grammar subset.
2. Interpret rule declarations needed in Phase 2 (`salience`, baseline declaration fields).
3. Replace S-expression-level rule placeholder storage with typed rule/template/deffacts registration structures.
4. Update `Engine::load_str` / `Engine::load_file` to execute Stage 1 -> Stage 2 and return aggregated diagnostics.
5. Add tests for valid and invalid top-level forms, including span-accurate error reporting.

## Definition Of Done

- Loading source yields typed constructs for all Phase 2-required top-level forms.
- Invalid forms fail with clear, source-located errors.
- Runtime stores typed construct definitions ready for compilation.

## Verification Commands

- `cargo test -p ferric-runtime loader`
- `cargo test -p ferric-parser`
- `cargo check --workspace`

## Handoff State

- Phase 2 construct interpretation is functional and connected to loader entry points.
