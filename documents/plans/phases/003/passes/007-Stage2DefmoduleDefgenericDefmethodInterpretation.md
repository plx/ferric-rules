# Pass 007: Stage 2 `defmodule`, `defgeneric`, And `defmethod` Interpretation

## Objective

Add typed Stage 2 interpretation support for remaining Phase 3 top-level constructs: `defmodule`, `defgeneric`, and `defmethod`.

## Scope

- Parser AST representation for module import/export and generic/method declarations.
- Source-located interpretation diagnostics for malformed construct shapes.
- Loader ingestion hooks that retain interpreted definitions.

## Tasks

1. Extend Stage 2 `Construct` variants and typed structs for defmodule/defgeneric/defmethod.
2. Parse module declarations with import/export forms into explicit typed representations.
3. Parse generic and method declarations (name, parameters/restrictions, body) into typed forms.
4. Add diagnostics for invalid declaration forms, duplicate names, and unsupported declaration clauses.
5. Extend loader construct dispatch to store these definitions for runtime passes 008-009.

## Definition Of Done

- Remaining Phase 3 constructs parse into typed, span-rich Stage 2 representations.
- Malformed constructs fail fast with source-located interpretation errors.
- Loader no longer treats these constructs as generic unsupported stubs.

## Verification Commands

- `cargo test -p ferric-parser stage2`
- `cargo test -p ferric-runtime loader`
- `cargo check --workspace`

## Handoff State

- Parser/loader surfaces are ready for module and generic runtime semantics.

