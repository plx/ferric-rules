# ferric-rules Compatibility Roadmap

Status of CLIPS compatibility deficits originally identified in a bulk
assessment of 5,802 `.clp` files from 16 sources.

Original assessment date: 2026-02-23 (ferric @ commit 0adf176).
**Updated: 2026-03-15** — 8 of the original 10 items have been implemented.

---

## Completed Items

| # | Issue | Status |
|---|-------|--------|
| 1 | Bracket `[`/`]` in symbols | **Done** — `is_symbol_char()` includes `[` and `]` |
| 2 | Connective constraints (`&`, `\|`, `~`) | **Done** — `interpret_constraint_sequence()` handles precedence |
| 3 | Implicit `(initial-fact)` for empty rules | **Done** — loader injects at `loader.rs:2578-2591` |
| 4 | `(and ...)` conditional element | **Done** — top-level flattened; NCC inside `(not ...)` at `loader.rs:2656` |
| 5 | `(field ...)` slot alias | **Done** — `stage2.rs:2739` accepts `"slot" \| "field"` |
| 7 | `do-for-fact` query macros | **Done** — full parser + runtime for all 6 query forms |
| 8 | `if`/`then`/`else` | **Done** — `ActionExpr::If` with parser + runtime |
| 9 | `while`/`loop-for-count`/`foreach`/`switch` | **Done** — all control flow forms implemented |

---

## Remaining Items

### Still Open from Original Roadmap

| # | Issue | Files | Effort | Notes |
|---|-------|------:|--------|-------|
| 6 | Complex constraint expressions | 5 | Medium | Some nested forms still rejected |
| 10 | Compile-time function validation | 142 | Medium | Behavioral divergence, not a blocker |

### Completed: Standard Library Gaps (2026-03-15)

| Issue | Status |
|-------|--------|
| Math transcendentals (sqrt, sin, cos, etc.) | **Done** — 27 functions added to evaluator |
| String functions (str-index, upcase, lowcase, etc.) | **Done** — 6 functions + string-to-field, explode$ |
| Multifield functions (insert$, delete$, replace$, etc.) | **Done** — 7 functions (insert$, delete$, replace$, first$, rest$, sort, funcall) |
| Fact introspection (fact-index, fact-slot-value, etc.) | **Done** — 5 functions with EvalContext fact_base/template_defs |
| funcall | **Done** — dynamic dispatch through builtin → user-function → generic chain |
| load-facts / save-facts | **Done** — .fct file round-trip via actions.rs |

### Remaining: Infrastructure

| Issue | Effort | Notes |
|-------|--------|-------|
| batch command interpreter | Large | Needed to run .bat test suite files |
| File I/O (open, close, etc.) | Medium | Router extension for file handles |
| eval / build | Medium-Large | Runtime compilation |

### Explicitly Skipped

| Feature | Decision | Rationale |
|---------|----------|-----------|
| COOL object system | **Skip permanently** | Enormous effort; target use cases don't need OOP |
| Truth Maintenance (logical CE) | **Skip** | Performance/serialization costs outweigh benefits |
| Simplicity/Complexity/Random strategies | **Deferred** | Rarely used in practice |
