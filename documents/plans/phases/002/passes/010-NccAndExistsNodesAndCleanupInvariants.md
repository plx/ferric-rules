# Pass 010: NCC And Exists Nodes, Plus Cleanup Invariants

## Objective

Implement conjunction-negation (`NCC`) and `exists` semantics with full cleanup hooks and retraction correctness.

## Scope

- `BetaNode::Ncc`, `BetaNode::NccPartner`, `NccMemory` (Section 7.3).
- Exists memory/support tracking (Section 7.4).
- Cleanup callback integration for all new side-memory indices.

## Tasks

1. Implement NCC subnetwork structures and owner/result tracking memory.
2. Compile `(not (and ...))` into NCC node + partner/subnetwork topology.
3. Implement exists memory (`support_count`, `satisfied`, `fact_to_tokens`) and compile/runtime wiring for `(exists <pattern>)`.
4. Ensure cascade cleanup dispatch reaches NCC and exists memories with idempotent, no-panic behavior.
5. Extend invariant checks/tests to cover NCC/exists side indices and retract-all cleanup behavior.

## Definition Of Done

- `(not (and ...))` and `(exists ...)` semantics work under assert/retract churn.
- NCC/exists side indices remain free of stale IDs after cascade retraction.
- Retraction invariants now cover all Phase 2 memory types.

## Verification Commands

- `cargo test -p ferric-core rete`
- `cargo test -p ferric-runtime integration_tests`
- `cargo check --workspace`

## Handoff State

- Phase 2 negation/existential runtime coverage is complete.
