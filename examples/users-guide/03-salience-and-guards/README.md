# 03 — Priority and mutual exclusion: salience + guard facts

Companion code for [`docs/users-guide.md` §4](../../../docs/users-guide.md#4-priority-and-mutual-exclusion-salience--guard-facts).

Run from this directory:

```sh
just check-example
```

What it shows:

- Three rules at salience 100/50/10 form a priority-ordered classifier.
- Each rule asserts a `(decision-made)` guard fact so only the winner fires.
- The Rust driver runs the same engine against three input scenarios with
  `engine.reset()` between them.
