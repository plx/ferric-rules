# 214 Missing Compile-Time Validation Warnings

## Behavioral Divergence
CLIPS performs compile-time validation that produces warning messages for various constructs — type constraint mismatches, module import/export violations, argument type errors, etc. These are typically warnings (not fatal errors) that appear in the output alongside normal program output. Ferric silently accepts these constructs, producing no output where CLIPS produces warning text.

This causes output-mismatch for files that are specifically testing CLIPS's warning/error reporting.

Examples of CLIPS warnings that ferric does not emit:
```
[RULECSTR2] Slot types in pattern #2 of rule foo are incompatible
[CSTRNPSR1] The allowed-values attribute conflicts with the type attribute
[MODULPSR1] Module "FOO" was not exported
[DEFAULT1] The default value for a single field slot must be a single field value
[ARGACCES3] Function + expected argument #1 to be of type integer or float
```

## Affected Files (~19)
These overlap with fix 213 — once the error regex is broadened, most of these will be reclassified as `clips-error`. The remaining cases where ferric should emit warnings are:
- Type constraint validation in rules (`[RULECSTR2]`)
- Slot facet conflict detection (`[CSTRNPSR1]`)
- Module import/export validation (`[MODULPSR1]`)
- Default value validation (`[DEFAULT1]`)

## Apparent Ferric-Side Root Cause
Ferric does not implement constraint propagation or type checking at compile time. In CLIPS, the constraint system:
1. Tracks type information (INTEGER, FLOAT, SYMBOL, STRING, etc.) for each slot.
2. Propagates constraints through variable bindings across patterns.
3. Warns when constraints conflict (e.g., a variable constrained to INTEGER in one pattern used in a SYMBOL-only slot in another).
4. Validates module import/export at defmodule parse time.

Ferric skips all of these checks, accepting constructs unconditionally.

## Implementation Plan
This is a large feature area. Recommended approach:

1. Module import/export validation (highest value).
   - At construct load time, verify that referenced constructs from other modules are properly exported/imported.
   - This catches `[MODULPSR1]` style errors.
   - Caveat: requires tracking the export list of each module.

2. Default value validation.
   - When a deftemplate slot has `(type INTEGER)` and `(default "hello")`, warn about the type mismatch.
   - When a single-field slot has a multi-value default, warn.
   - Caveat: straightforward validation at template definition time.

3. Type constraint propagation (deferred — complex).
   - Full constraint propagation through variable bindings is a significant compiler feature.
   - Defer this to a later phase — it produces warnings only, not functional differences.
   - Caveat: without this, some test files will always show output differences.

## Priority Note
This is a low-priority fix for engine functionality. Most affected files are testing CLIPS's validation system specifically. Fix 213 (broadening the error regex) will reclassify most of these files as `clips-error`, removing them from the output-mismatch category. The remaining genuine differences are validation warnings that don't affect rule execution.

## Test And Verification
After implementing any validation:
```bash
cargo run -p ferric -- check tests/generated/test-suite-segments/co-drtest08-13.clp 2>&1
```
Expected: warning messages similar to CLIPS's `[RULECSTR2]` output.
