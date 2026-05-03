# 06 — User functions and generics

Companion code for [`docs/users-guide.md` §7](../../../docs/users-guide.md#7-user-functions-and-generics).

Run from this directory:

```sh
just check-example
```

What it shows:

- A `deffunction` (`celsius-to-fahrenheit`) used from a rule's RHS.
- A `defgeneric` with two `defmethod`s and `(call-next-method)` chaining.
- Calling `describe` on an INTEGER chains through to NUMBER (`int/number(7)`),
  while a FLOAT skips straight to NUMBER (`number(2.5)`).
