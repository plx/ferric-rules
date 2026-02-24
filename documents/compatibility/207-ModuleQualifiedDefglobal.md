# 207 Module-Qualified Defglobal

## Behavioral Divergence
CLIPS allows an optional module name prefix in `defglobal`:
```clips
(defmodule MAIN (export ?ALL))
(defglobal MAIN ?*proximity* = 9)
```

Ferric rejects the module-qualified form with:
```
interpret error: expected global variable name (?*name*), found Atom(Symbol("MAIN"), ...) at line 2, column 12
```

## Affected Files (3-4)
- `generated/test-suite-segments/co-drtest08-11.clp`
- `generated/test-suite-segments/t64x-drtest08-11.clp`
- `missionaries-cannibals/src/CLIPS_cannibles_and_missionaries.clp`

## Apparent Ferric-Side Root Cause
`crates/ferric-parser/src/stage2.rs` or `crates/ferric-runtime/src/loader.rs` — the `defglobal` parser expects the first token after `(defglobal` to be a global variable name (`?*name*`). It does not handle the optional module-name prefix that CLIPS allows.

In CLIPS, the syntax is: `(defglobal [<module-name>] <global-assignment>*)` where `<module-name>` is an optional symbol that specifies which module the globals belong to. If omitted, the current module is used.

## Implementation Plan
1. Add optional module-name parsing to defglobal.
   - After `(defglobal`, peek at the next token. If it is a symbol (not a global variable `?*...*`), treat it as a module name and consume it.
   - Then continue parsing the remaining `?*name* = value` pairs as before.
   - Use the module name to scope the global variables to the specified module.
   - Caveat: if the current module system does not support per-module global scoping, the module name can be recorded but the global stored in the default/current scope.

2. Validate the module name.
   - If the module name refers to a module that hasn't been defined yet, either:
     a. Report an error (strict mode), or
     b. Accept it and assume the module will be defined later (lenient mode, matching CLIPS behavior).
   - Caveat: CLIPS is lenient and does not require the module to be pre-defined for defglobal.

## Test And Verification
1. Parser unit tests:
```bash
cargo test -p ferric-parser defglobal_module_qualified
```

2. Compatibility smoke checks:
```bash
cargo run -p ferric -- check tests/generated/test-suite-segments/co-drtest08-11.clp
cargo run -p ferric -- check tests/examples/missionaries-cannibals/src/CLIPS_cannibles_and_missionaries.clp
```
Expected: "expected global variable name" error disappears; module-qualified defglobal parses and the global is accessible.
