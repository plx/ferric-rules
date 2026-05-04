# 01 — A minimal embedding

Companion code for [`docs/users-guide.md` §2](../../../docs/users-guide.md#2-a-minimal-embedding).

Run from this directory:

```sh
just check-example
```

What it shows:

- `Engine::with_rules` parses, compiles, and resets in one call.
- `assert_ordered_symbol` is the convenience for "relation with a single symbol field."
- `RunLimit::Unlimited` runs until the agenda drains.
- `get_output("t")` returns whatever rules wrote to the standard output channel.
