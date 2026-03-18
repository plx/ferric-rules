---
name: EvalContext field addition pattern
description: How to safely add optional fields to EvalContext across ~80 construction sites
type: feedback
---

## Adding Optional Fields to EvalContext

`EvalContext` has ~80 construction sites across the codebase. When adding a new optional field
(especially one that needs `None` in tests and non-production paths), follow this checklist:

### Files with EvalContext construction sites

1. `crates/ferric-runtime/src/evaluator.rs`:
   - 3 non-test inner-context sites: LoopForCount, Progn, `execute_callable_body`
     → propagate from parent: `fact_base: ctx.fact_base, template_defs: ctx.template_defs`
   - ~68 test sites ending with `input_buffer: None,`
   - ~8 test sites ending with `input_buffer: Some(&mut input_buffer),`
   → Use Python script to batch-insert `None` fields after `input_buffer:` lines
     (check for `None,` vs `as_deref_mut()` to distinguish test vs non-test)

2. `crates/ferric-runtime/src/loader.rs`:
   - 2 sites (lines ~1081 and ~1148) → pass `None` for both

3. `crates/ferric-runtime/src/actions.rs`:
   - 2 sites (line 62 `make_eval_context`, line 176 `eval_runtime_expr_with_bindings`)
     → wire engine fields: `fact_base: Some(&engine.fact_base), template_defs: Some(&engine.template_defs)`

### Python batch-update script

```python
import re

with open("crates/ferric-runtime/src/evaluator.rs") as f:
    lines = f.readlines()

result_lines = []
i = 0
while i < len(lines):
    line = lines[i]
    result_lines.append(line)
    stripped = line.rstrip()
    # Match test-context input_buffer lines (not the propagation ones)
    if re.match(r'\s+input_buffer: None,', stripped) or \
       re.match(r'\s+input_buffer: Some\(&mut input_buffer\),', stripped):
        j = i + 1
        while j < len(lines) and lines[j].strip() == '':
            j += 1
        if j < len(lines) and not lines[j].strip().startswith('fact_base:'):
            indent = len(line) - len(line.lstrip())
            indent_str = ' ' * indent
            result_lines.append(f'{indent_str}fact_base: None,\n')
            result_lines.append(f'{indent_str}template_defs: None,\n')
    i += 1

with open("crates/ferric-runtime/src/evaluator.rs", "w") as f:
    f.write(''.join(result_lines))
```

### Visibility rule for optional fields

If the field type uses a `pub(crate)` type (like `RegisteredTemplate`), the field itself
must also be `pub(crate)` not `pub`, even if the struct `EvalContext` is `pub`.
Otherwise clippy emits `private_interfaces` warning.

**Why:** Clippy enforces that public fields don't expose private types at the public API.
**How to apply:** When adding a field referencing a `pub(crate)` type, make the field `pub(crate)`.
