# Phase 001 Plan Adjustments

This document describes updates needed in `documents/FerricImplementationPlan.md` so the master plan accurately reflects what Phase 001 delivered and what Phase 2 should assume as its starting baseline.

## 1) Update crate/module ownership and layout (Sections 4.1, 4.2, 5.1, 9.3)

### Required adjustment

Revise the plan’s sample layout and ownership notes to reflect the implemented architecture:

- Value/symbol/string/encoding primitives are in `ferric-core`, not `ferric-runtime`.
- Rete code is implemented as flat modules (`alpha.rs`, `beta.rs`, `rete.rs`, `token.rs`, etc.) rather than the nested `src/rete/*` tree shown in the illustrative layout.

### Why

This was an intentional cycle-breaking change documented in Phase 001 notes and is now the real dependency shape Phase 2 will build on.

### Phase 2 implication

Any Phase 2 tasks that reference runtime-owned value primitives or nested rete file paths should be updated to the current core-owned/flat-module structure.

## 2) Update Stage 1 parser API contract (Section 8.2)

### Required adjustment

Change the Stage 1 parse API in the plan from:

- `parse_sexprs(...) -> Result<Vec<SExpr>, Vec<ParseError>>`

to:

- `parse_sexprs(...) -> ParseResult { exprs, errors }`

with explicit note that lex errors currently short-circuit into parse errors (no partial token-stream parse attempt).

### Why

This API shape is implemented and documented in Phase 001 notes; leaving the old signature in the master plan creates false inconsistency.

### Phase 2 implication

Stage 2 construct interpretation should consume `ParseResult` directly (or define a thin adapter) rather than expecting a `Result`-only boundary.

## 3) Update loader behavior contract for Phase 1 baseline (Sections 8.3, 9.2, Phase 1 exit text)

### Required adjustment

Document the actual Phase 1 loader/runtime surface:

- `Engine::load_str` / `Engine::load_file` return `Result<LoadResult, Vec<LoadError>>`.
- `LoadResult` includes asserted fact IDs, collected `RuleDef`s, and warnings.
- `deffacts` is accepted as batch-assert behavior in Phase 1.
- Rule ingestion remains S-expression-level (`RuleDef`), with no automatic rule-to-rete compilation yet.

### Why

All of the above is implemented and intentionally documented in Phase notes.

### Phase 2 implication

Phase 2 should explicitly include the `RuleDef -> compiled network` bridge as the next step, not assume it already exists.

## 4) Update engine API snapshot for current implemented subset (Section 9.2)

### Required adjustment

In the plan’s API section, annotate Phase 1 subset to include:

- `assert_ordered` convenience method (implemented)
- `assert(Fact)` (now implemented)
- `unsafe move_to_current_thread(&mut self)` transfer hook (implemented)

and clearly mark the rest of the full API (run/reset/call/modules/etc.) as later-phase surfaces.

### Why

The current implementation is a deliberate subset; making this explicit prevents perceived “missing API” noise during Phase 2 planning.

### Phase 2 implication

Phase 2 can focus on compilation/matching/runtime semantics without reopening settled Phase 1 API-baseline questions.

## 5) Keep/clarify O(1) retraction cleanup contract as implemented (Sections 6.6.1, 14.2)

### Required adjustment

Add a short status note that Phase 001 now performs owner-node-directed beta-memory cleanup (no all-memory scan during token removal).

### Why

This was previously a documented temporary divergence; it has now been remediated.

### Phase 2 implication

No special debt item is needed for beta-memory cleanup complexity at Phase 2 start.

## 6) Clarify invariant harness status in Phase 1/15.0 text

### Required adjustment

Update wording to reflect that Phase 001 now includes:

- token/alpha/beta/agenda internal consistency checks
- rete-level cross-structure consistency checks exercised during retraction-oriented tests

and explicitly list any invariants intentionally deferred because they depend on Phase 2+ features (negative/NCC/exists structures).

### Why

Current Phase 1 tests and debug checks are stronger than the original minimum wording and should be reflected as the new baseline.

### Phase 2 implication

Phase 2 should extend the existing invariant framework, not create parallel checks.

## 7) Adjust Phase 1 exit wording for rule capability precision (Section 15: Phase 1 exits)

### Required adjustment

Refine phrasing from broad “can define rules with simple patterns” to:

- Phase 1 can parse and retain minimal rule definitions and demonstrate alpha/beta/agenda propagation with programmatic network construction.
- Automatic compilation from parsed rule definitions is Phase 2 scope.

### Why

Current implementation and tests follow this split; explicit wording prevents scope confusion.

### Phase 2 implication

Phase 2 must treat compiler integration (`RuleDef`/Stage 2 AST -> rete graph) as first-class entrance work.
