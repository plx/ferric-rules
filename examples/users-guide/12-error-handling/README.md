# 12 — Error handling

Companion code for [`docs/users-guide.md` §13](../../../docs/users-guide.md#13-error-handling).

Run from this directory:

```sh
just check-example
```

What it shows:

- A fatal `InitError` from `Engine::with_rules` when the source can't parse.
- An action diagnostic from division by zero in a rule. The run stops with
  `HaltReason::ActionError`; the engine remains usable, and the diagnostic is
  available via `engine.action_diagnostics()` until the next engine call that
  clears diagnostics.
- `clear_action_diagnostics()` for explicit cleanup after inspection.
