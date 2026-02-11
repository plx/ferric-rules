# Pass 002: Runtime Values, Symbols, And Encoding

## Objective

Implement core runtime value primitives and encoding-aware symbol/string behavior needed by all later engine work.

## Scope

- Value model from Section 5.1.
- `AtomKey` behavior from Section 5.1.1.
- `SymbolTable` and encoding policy hooks from Sections 2.4 and 9.3.

## Tasks

1. Implement `Value`, `FerricString`, `Multifield`, and `ExternalAddress`.
2. Implement `AtomKey` with exact float bit semantics (`f64::to_bits()` behavior).
3. Implement symbol identity types (`Symbol`, `SymbolId`) and symbol table intern/resolve paths.
4. Add `EngineConfig` subset needed for string/symbol encoding mode.
5. Implement encoding-checked constructors (`intern_symbol`, `create_string`) in runtime surface.
6. Add unit tests for:
   - ASCII mode enforcement.
   - UTF-8 mode acceptance.
   - `-0.0` vs `+0.0` distinction for `AtomKey`.
   - No implicit normalization in string equality/ordering.

## Definition Of Done

- Runtime value and symbol code compiles and has focused test coverage.
- Encoding behavior is deterministic and matches documented semantics.

## Verification Commands

- `cargo test -p ferric-runtime`
- `cargo check --workspace`

## Handoff State

- All value/symbol primitives needed by fact and parser integration are available.
