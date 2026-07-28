# Cross-binding semantic conformance

`corpus.json` is the language-neutral contract shared by the Rust, C, Go,
Node, and Python adapters. `just bindings-conformance` runs every required
case through every adapter, compares normalized JSON observations, and fails
on missing results, unknown drift, or a deviation that has become stale.

The canonical result is the intended shared engine semantic. A binding may
temporarily or deliberately differ only through an exact `deviations` entry
that records:

- the expected normalized result;
- a rationale;
- the corpus version that introduced it; and
- the issue that owns the policy or remediation.

When an implementation starts matching the canonical result, the command
fails until its stale deviation is removed. This keeps the matrix honest as
FR-BIND-001, FR-BIND-002, FR-CABI-005, and the binding-specific follow-ups
land.

Every public semantic added to a binding must have a matrix case. The corpus
loader enforces the required semantic inventory, unique case IDs, complete
adapter participation, and versioned deviation metadata.

Adapters read a newline-delimited case-ID file and emit one JSON object per
line:

```json
{"case":"value.void","result":{"type":"void"}}
```

Adapter stdout is protocol-only. Diagnostics belong on stderr.
