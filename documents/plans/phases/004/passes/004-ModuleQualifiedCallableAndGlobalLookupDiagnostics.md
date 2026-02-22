# Pass 004: Module-Qualified Callable And Global Lookup Diagnostics

## Objective

Implement executable `MODULE::name` resolution for callable/global references with deterministic behavior and source-located diagnostics.

## Scope

- Qualified lookup for functions/globals (and related callable resolution paths).
- Module existence, symbol existence, and visibility/error handling.
- Interaction between qualified and unqualified resolution precedence.

## Tasks

1. Implement runtime lookup paths for qualified callable/global references.
2. Enforce the module-resolution contract for qualified references (including import/export checks where required by plan semantics).
3. Emit diagnostics for unknown module, unknown qualified symbol, and not-visible qualified symbol cases.
4. Ensure qualified references never silently degrade to unqualified lookup.
5. Add integration tests covering mixed qualified/unqualified behavior and failure-path diagnostics.

## Definition Of Done

- Qualified callable/global references resolve deterministically.
- Qualified lookup failures are explicit and source-located.
- No silent fallback behavior exists for malformed/unresolved qualified names.

## Verification Commands

- `cargo test -p ferric-runtime loader`
- `cargo test -p ferric-runtime evaluator`
- `cargo check --workspace`

## Handoff State

- Module-qualified name resolution baseline is complete for callable/global paths.
