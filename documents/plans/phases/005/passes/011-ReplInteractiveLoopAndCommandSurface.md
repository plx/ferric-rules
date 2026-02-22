# Pass 011: REPL Interactive Loop And Command Surface

## Objective

Implement a functional interactive REPL on top of the CLI/runtime pipeline.

## Scope

- `ferric repl` command and lifecycle.
- Multiline input and REPL command surface.
- Runtime/evaluator diagnostics for entered forms.

## Tasks

1. Implement REPL startup and interactive loop using `rustyline` with line editing and history support.
2. Add balanced-paren continuation for multiline form entry before evaluation.
3. Implement required REPL commands (`reset`, `run [n]`, `facts`, `agenda`, `clear`, `exit`).
4. Route entered forms through existing loader/evaluator paths so diagnostics retain source spans/context.
5. Add REPL tests for command behavior, multiline parsing, and error-reporting consistency.

## Definition Of Done

- REPL is interactive and supports required commands.
- Multiline input behavior is predictable and syntax-aware.
- Diagnostics in REPL remain source-located and consistent with non-interactive surfaces.

## Verification Commands

- `cargo test -p ferric-cli repl`
- `cargo test -p ferric-cli interactive`
- `cargo check -p ferric-cli`

## Handoff State

- Phase 5 interactive shell requirements are fully implemented.
