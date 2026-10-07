# 10 — Input and output channels

Companion code for [`docs/users-guide.md` §11](../../../docs/users-guide.md#11-input-and-output-channels).

Run from this directory:

```sh
just check-example
```

What it shows:

- `engine.push_input("...")` queues lines for `(read)` / `(readline)`.
- `(format t ...)` writes and returns its string; `(format nil ...)` only
  returns it, so it can be embedded in `printout`.
- `get_output("t")` reads back what was written.
