# 05 — RHS actions: modify, retract, duplicate, bind

Companion code for [`docs/users-guide.md` §6](../../../docs/users-guide.md#6-rhs-actions-modify-retract-duplicate-bind).

Run from this directory:

```sh
just check-example
```

What it shows:

- A `counter` template fact captured by `?c <-` and rewritten via `modify`.
- Each `(tick)` fact captured by `?t <-` and removed via `retract`.
- Pulling the final slot value back into Rust via `get_fact_slot_by_name`.
