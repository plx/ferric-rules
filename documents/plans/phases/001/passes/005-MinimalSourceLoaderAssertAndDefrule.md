# Pass 005: Minimal Source Loader For `assert` And `defrule`

## Objective

Connect Stage 1 parser output to engine loading with a minimal top-level construct loader that supports Phase 1 targets.

## Scope

- `Engine::load_str` and `Engine::load_file` minimal behavior.
- Top-level S-expression interpretation for `(assert ...)` and `(defrule ...)` only.

## Tasks

1. Implement source-loading path from file/string into Stage 1 parser.
2. Add minimal top-level construct dispatcher over parsed S-expressions.
3. Implement `(assert ...)` top-level handling into fact assertion path.
4. Implement `(defrule ...)` top-level handling as minimal rule registration structure (S-expression level; not full Stage 2 grammar).
5. Reject unsupported top-level forms with source-located diagnostics.
6. Add integration fixtures/tests:
   - successful load with assert/rule forms,
   - diagnostic behavior on unsupported forms.

## Definition Of Done

- Minimal loader supports the two required top-level forms.
- `.clp` file path and in-memory string loading both work.

## Verification Commands

- `cargo test --workspace`
- `cargo check --workspace`

## Handoff State

- Engine can ingest basic sources needed for Phase 1 integration testing.
