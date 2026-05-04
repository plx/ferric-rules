# User's Guide examples

Runnable companions to [`docs/users-guide.md`](../../docs/users-guide.md).
Each subfolder is a small Cargo binary that demonstrates one section of the
guide end-to-end. The folder index follows the guide's section numbering.

| Section | Example folder |
| ------: | --------------------------------------------------------------- |
|     §2  | [`01-minimal-embedding/`](01-minimal-embedding/)                 |
|     §3  | [`02-ordered-vs-template/`](02-ordered-vs-template/)             |
|     §4  | [`03-salience-and-guards/`](03-salience-and-guards/)             |
|     §5  | [`04-pattern-menagerie/`](04-pattern-menagerie/)                 |
|     §6  | [`05-rhs-actions/`](05-rhs-actions/)                             |
|     §7  | [`06-functions-and-generics/`](06-functions-and-generics/)       |
|     §8  | [`07-globals/`](07-globals/)                                     |
|     §9  | [`08-modules-and-focus/`](08-modules-and-focus/)                 |
|     §10 | [`09-driving-from-rust/`](09-driving-from-rust/)                 |
|     §11 | [`10-io-channels/`](10-io-channels/)                             |
|     §12 | [`11-configuration/`](11-configuration/)                         |
|     §13 | [`12-error-handling/`](12-error-handling/)                       |
|     §14 | [`13-snapshots/`](13-snapshots/)                                 |
|     §15 | [`14-pipeline/`](14-pipeline/)                                   |

## Running

From any example folder:

```sh
just build-example     # cargo build the example
just check-example     # cargo build + run the example
```

From the workspace root:

```sh
just build-examples       # build every example
just check-examples       # build + run every example
just check-examples-sync  # verify guide snippets are in sync
```

CI runs `check-examples` and `check-examples-sync` on every PR — see
`.github/workflows/ci.yml`.

## Layout

Each example folder follows the same shape:

```
NN-slug/
├── Cargo.toml         # binary crate that depends on the `ferric` facade
├── README.md          # links back to the matching guide section
├── justfile           # `build-example` / `check-example` recipes
├── rules/             # `.clp` rule sources, loaded via include_str!
│   └── *.clp
└── src/main.rs        # Rust driver for the rules
```

Trivial "hello world" examples inline the rule source as a string literal
in `main.rs`; everything else lives in `rules/` so the rule logic stays
readable and re-usable.

## Keeping the guide in sync

Each runnable code block in the guide may carry an HTML-comment marker
immediately above the fence, e.g.

````markdown
<!-- example: 01-minimal-embedding/src/main.rs -->
```rust
fn main() { /* ... */ }
```
````

The `users-guide-sync` tool (`tools/users-guide-sync/`) reads the guide,
finds every marked block, and verifies (after light whitespace
normalization: trimming lines and dropping blank lines) that the snippet
appears in the named example file. If guide and example drift apart, CI
fails with a pointer to the offending block. Untagged blocks are ignored.
