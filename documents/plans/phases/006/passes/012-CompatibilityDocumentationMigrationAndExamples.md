# Pass 012: Compatibility Documentation, Migration, And Examples

## Objective

Deliver release-grade documentation and examples aligned to implemented behavior and locked compatibility contracts.

## Scope

- `docs/compatibility.md` covering the required Section 16 compatibility topics (independent of exact heading numbering).
- Migration guidance for CLIPS users.
- Embedding and CLI JSON examples aligned to current FFI/CLI surfaces.

## Tasks

1. Create/update `docs/compatibility.md` to fully cover supported constructs, unsupported features, nesting restrictions, string/symbol semantics, migration context, activation ordering, external interface contracts, and machine-readable CLI diagnostics.
2. Document canonical external API usage for wrappers (`ferric_engine_*`, configured constructor/config enums, action diagnostics retrieval APIs, thread-affinity exceptions).
3. Add copy-to-buffer and fact-id round-trip guidance with concrete examples matching implemented behavior.
4. Add `run --json` / `check --json` examples and field-level contract notes for CI/tooling consumers.
5. Create/update migration/examples docs (`docs/migration.md`, selected README/examples snippets) to reflect Phase 6-final behavior.

## Definition Of Done

- Compatibility and migration documentation is complete, internally consistent, and aligned with tests.
- User-facing examples compile conceptually against current APIs/CLI behavior.

## Verification Commands

- Documentation lint/spell checks (if configured)
- `cargo test --workspace` (to ensure examples/fixtures remain aligned)

## Handoff State

- Documentation and examples are release-ready and contract-accurate.
