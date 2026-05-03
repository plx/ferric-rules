# 07 — Globals

Companion code for [`docs/users-guide.md` §8](../../../docs/users-guide.md#8-globals).

Run from this directory:

```sh
just check-example
```

What it shows:

- A `defglobal` updated via `(bind ?*session-count* ...)` from a rule.
- The host reads it back with `engine.get_global("session-count")` —
  the bare name, **without** the surrounding `*`s.
