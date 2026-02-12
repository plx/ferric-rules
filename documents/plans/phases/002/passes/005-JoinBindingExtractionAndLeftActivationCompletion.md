# Pass 005: Join Binding Extraction And Left Activation Completion

## Objective

Complete the core join runtime path by adding full binding extraction/propagation and left-activation behavior for compiled multi-pattern rules.

## Scope

- Variable/`VarId` binding generation from compiled patterns.
- Join evaluation and token binding extension behavior.
- Left activation parity with right activation.

## Tasks

1. Extend compilation to assign `VarId`s and produce join tests/binding metadata from rule patterns.
2. Implement binding extraction when incoming facts satisfy variable constraints.
3. Implement binding propagation/extension on token creation across join chains.
4. Add left-activation propagation so new parent tokens trigger downstream joins correctly.
5. Add tests for repeated-variable constraints, multi-join rules, and retraction correctness with propagated bindings.

## Definition Of Done

- Multi-pattern rules with variable sharing evaluate correctly.
- Both left and right activation paths produce equivalent match outcomes.
- Retraction cleanup remains correct for tokens created through extended binding paths.

## Verification Commands

- `cargo test -p ferric-core beta`
- `cargo test -p ferric-core rete`
- `cargo check --workspace`

## Handoff State

- Core positive-rule matching semantics are complete enough to support runtime firing.
