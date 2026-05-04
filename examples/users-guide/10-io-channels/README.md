# 10 — Input and output channels

Companion code for [`docs/users-guide.md` §11](../../../docs/users-guide.md#11-input-and-output-channels).

Run from this directory:

```sh
just check-example
```

What it shows:

- `engine.push_input("...")` queues lines for `(read)` / `(readline)`.
- `(format nil "...")` returns a string; `printout` is what actually writes.
- `get_output("t")` reads back what was written.
