# Pass 004: Stage 1 Lexer And S-Expression Parser

## Objective

Implement Stage 1 parsing (lexing + S-expression tree + spans + recovery) so `.clp` sources can be ingested structurally.

## Scope

- Parser architecture Section 8.1 and Stage 1 details Section 8.2.
- Error recovery behavior and span tracking.

## Tasks

1. Implement tokenization for core CLIPS lexical forms needed in Phase 1.
2. Implement `Span`/`Position`/`FileId` primitives and source mapping.
3. Implement `SExpr` + atom variants and list parsing.
4. Implement `parse_sexprs(source, file_id) -> Result<Vec<SExpr>, Vec<ParseError>>`.
5. Add recovery behavior for malformed parentheses/token sequences.
6. Add parser tests for:
   - valid nested forms,
   - malformed input with multiple reported errors,
   - span correctness for representative forms.

## Definition Of Done

- Parser can produce S-expression trees for valid `.clp` snippets.
- Parser returns meaningful multi-error diagnostics with source locations.

## Verification Commands

- `cargo test -p ferric-parser`
- `cargo check --workspace`

## Handoff State

- Stage 1 parser is stable enough for loader integration in Pass 005.
