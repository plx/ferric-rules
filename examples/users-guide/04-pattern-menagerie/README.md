# 04 — Negation, existentials, and the pattern menagerie

Companion code for [`docs/users-guide.md` §5](../../../docs/users-guide.md#5-negation-existentials-and-the-pattern-menagerie).

Run from this directory:

```sh
just check-example
```

What it shows:

- `not`, `exists`, `forall`, NCC, and constraint connectives (`~`, `|`) all
  in one ruleset.
- Four scenarios driven from Rust, each with `engine.reset()` and an output
  channel clear so you can see exactly which rule fired in isolation.
