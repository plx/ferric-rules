# Rust Code Writer Memory

## Project Structure: ferric-rules

This is a Cargo workspace with four crates:
- `ferric` — facade crate that re-exports from the others
- `ferric-core` — shared types, fact storage, pattern matching (core engine internals)
- `ferric-parser` — parser (Stage 1 S-expression parser complete, Stage 2 construct interpreter added)
- `ferric-runtime` — Engine, EngineConfig, execution environment, and source loader

## Key Architectural Decisions

### Type Ownership and Dependencies
- **Shared primitive types belong in `ferric-core`**: Value, Symbol, FerricString, StringEncoding, EncodingError
- **Engine-level types belong in `ferric-runtime`**: Engine, EngineConfig, EngineError
- This avoids circular dependencies (runtime depends on core, not vice versa)

### Symbol Interning Pattern
**CRITICAL**: Always use a single `SymbolTable` instance for all symbols in a test. Each new table starts from index 0, so symbols from different tables will appear equal when compared by id.

### SlotMap Key Creation in Tests
**CRITICAL**: Never use `TemplateId::default()` (or any SlotMap key `::default()`) more than once — all defaults are equal. Create distinct keys via a temporary `SlotMap::with_key()`.

### BindingSet Requirements
`BindingSet` requires `Clone` and `Debug` derives (added in Pass 006 for Token storage).

### Value Equality in Tests
`Value` intentionally does NOT implement `PartialEq` (due to float semantics). In tests:
- Use `structural_eq()` for value comparisons
- Use `matches!` pattern matching instead of `assert_eq!`
- For optional values, use `.is_none()` or `.is_some()` instead of comparing to None

### SmallVec and Value
`Value` does not implement `Copy`, so avoid using `SmallVec::from_slice()` which requires `Copy`. Instead push individually.

### SExpr Type (ferric-parser)
`SExpr` does not implement `Copy`. Use `.to_vec()` to clone slices.

## Clippy Fixes

### Derivable Impls
When a Default impl just sets all fields to their defaults, use `#[derive(Default)]` instead of manual impl.

### Integer Truncation
When intentionally truncating `i64` to `i32`, use `#[allow(clippy::cast_possible_truncation)]`.

### Boolean to Int Conversion
Prefer `usize::from(boolean)` over if-else for 0/1 conversion.

## Benchmarking (Phase 6)

- Benchmark suite lives in `crates/ferric/benches/engine_bench.rs`
- `criterion = { version = "0.5", features = ["html_reports"] }` in workspace deps
- `engine.run()` takes `RunLimit::Unlimited` (enum), NOT an integer like `-1`
- `engine.reset()` returns `Result<(), EngineError>` — must `.unwrap()` in benchmarks
- `criterion 0.5` does NOT support `--quick` flag; use `-- --test` for smoke-runs
- Raw string literals with no embedded `"`: use `r"..."` not `r#"..."#` (clippy: `needless_raw_string_hashes`)
- Run benches: `cargo bench -p ferric`; smoke-run: `cargo bench -p ferric --bench engine_bench -- --test`
- HTML reports in `target/criterion/`

## Phase 3 Patterns

See `phase3-pass-notes.md` for detailed notes on each Phase 3 pass.

### EvalContext Borrow Pattern
`EvalContext` holds `&'a mut SymbolTable`. `from_action_expr` also needs `&mut SymbolTable`.
These cannot coexist — translate first, then construct context.

### Conflict Resolution Strategies (Pass 007)
- **Depth**: Higher salience > Higher timestamp > Higher seq
- **Breadth**: Higher salience > Lower timestamp > Higher seq
- **LEX**: Higher salience > Lexicographic recency > Higher seq
- **MEA**: Higher salience > First-pattern recency > LEX tiebreak > Higher seq

### Pass 007 (parser): Defmodule / Defgeneric / Defmethod Stage 2 Interpretation
- `?ALL` / `?NONE` are parsed as `Atom::SingleVar("ALL")` / `Atom::SingleVar("NONE")`
- `defgeneric` takes only name + optional comment — NO parameter list in the defgeneric itself

### Pass 008: Defmodule / Focus Stack
- **CRITICAL BUG**: In `load_str`, capture `current_module` alongside each rule during the first pass
- Focus stack initialized with `[MAIN]`; `run()` uses `pop_matching` for focus-aware activation

See `phase3-pass-notes.md` and `phase4-pass-notes.md` for detailed notes.

## Phase 4 Notes (see `phase4-pass-notes.md`)

### RegisteredTemplate Fields (CRITICAL)
- `slot_names: Vec<String>` (NOT `slots`), `slot_index: HashMap<String, usize>`, `defaults: Vec<Value>`
- `TemplateFact.slots` is `Box<[Value]>` (positional, NOT a HashMap)

### Clippy
- `r#"..."#` with no embedded `"` → use bare `r"..."` (needless_raw_string_hashes)
- `match_same_arms`: merge identical arms with `|`

### Pass 002: Module-Qualified Name Scaffold
- Lexer `lex_symbol()` extended: when `ch == ':'`, check `peek_ahead(1)==':'` to absorb `::` into the symbol
- New `crates/ferric-runtime/src/qualified_name.rs`: `QualifiedName` enum + `parse_qualified_name()`
- Test count: 829 after pass 002

### Pass 003: Cross-Module Visibility Enforcement
- `Engine` gained: `function_modules`, `global_modules`, `generic_modules` (all `HashMap<String, ModuleId>`)
- `EvalContext` gained 5 new fields: `current_module`, `module_registry`, `function_modules`, `global_modules`, `generic_modules`
- All `EvalContext` constructions in tests need these 5 fields — use `ModuleRegistry::new()` + empty `HashMap`s
- **Test helper pattern**: update `test_ctx()` to return 9-element tuple including `ModuleRegistry` and a shared empty `HashMap<String, ModuleId>` (reused for all three module maps)
- **Module re-registration**: `(defmodule MAIN (import X ...))` is valid CLIPS — re-defining a module updates its import/export specs. The loader should call `module_registry.register()` unconditionally (it handles update internally). The "duplicate defmodule" error was incorrect behavior.
- **`GlobalConstruct.globals`** field (not `.definitions`) holds `Vec<GlobalDefinition>`
- `defmethod` without a prior `defgeneric` auto-creates the generic; record its module in `generic_modules` in both the `Construct::Generic` and `Construct::Method` arms of the loader
- `execute_single_action` grows to >100 lines with 4 new parameters — add `#[allow(clippy::too_many_lines)]`
- `eval()` also grows to >100 lines with visibility checks — add `#[allow(clippy::too_many_lines)]`
- Visibility check for `GlobalVar`: check BEFORE accessing; returns `NotVisible` error
- Visibility check for `Call`: check after finding the function/generic but BEFORE dispatching
- Inner contexts in `dispatch_user_function` and `dispatch_generic` use the function's/generic's own module
- Test count: 834 after pass 003

### Pass 004: Module-Qualified Callable And Global Lookup Diagnostics
- Added `dispatch_qualified_call()` and `resolve_qualified_global()` to `evaluator.rs`
- Added `if name.contains("::")` early-return in both `RuntimeExpr::Call` and `RuntimeExpr::GlobalVar` arms of `eval()`
- Qualified calls bypass builtins entirely (`MAIN::+` errors rather than resolving to builtin `+`)
- Error hierarchy: unknown module → `TypeError`; wrong module ownership → `UnknownFunction`; not visible → `NotVisible`
- Test count: 840 after pass 004 (6 new tests added to `phase4_integration_tests.rs`)

### Pass 006: Generic Specificity Ranking and Method Ordering
- Added `restriction_concrete_type_count()` and `compare_method_specificity()` to `evaluator.rs`
- Updated `dispatch_generic()`: collect all applicable methods, sort by specificity, pick first
- **CRITICAL**: `printout` is an **action**, not an expression — it lives in `actions.rs` and is NOT available inside `defmethod` bodies. Method bodies are pure expressions evaluated by `eval()`. Use methods that RETURN values, then use `printout` in the rule RHS to print the return value.
- **Defmethod wildcard syntax**: `$?rest` is a bare atom in the param list, NOT wrapped in parens. Correct: `(defmethod f ((?x INTEGER) $?rest) ...)`. WRONG: `(defmethod f ((?x INTEGER) ($?rest)) ...)`.
- `compare_method_specificity` uses `if ord != Equal { return ord; }` pattern (not `match ... => continue`) to avoid clippy::needless_continue
- Test count: 418 after pass 006 (15 new tests: 9 unit tests in evaluator.rs, 6 integration tests)

### Pass 007: call-next-method Dispatch Chain
- Added `MethodChain` struct to `evaluator.rs` and `method_chain: Option<MethodChain>` field to `EvalContext`
- `dispatch_generic`: clones applicable methods into owned `Vec`, builds `MethodChain`, passes `Some(chain)` to inner_ctx
- Added `dispatch_call_next_method()` function: increments `current_index`, errors if no next method or outside generic
- `call-next-method` check added BEFORE the `name.contains("::")` check in `RuntimeExpr::Call` arm of `eval()`
- **Key pattern**: to update all ~35 test EvalContext sites at once, use `replace_all=true` targeting the last common field + `};`
- Test count: 422 after pass 007 (4 new integration tests in `phase4_integration_tests.rs`)

### Pass 008: Predicate, Math, and Type Surface Parity
- Added 6 new builtins to `dispatch_builtin` in `evaluator.rs`: `lexemep`, `multifieldp`, `evenp`, `oddp`, `integer`, `float`
- `evenp`/`oddp` use direct `eval(ctx, &args[0])` (not `eval_args`) since they need the value for type error
- `builtin_to_integer` needs `#[allow(clippy::cast_possible_truncation)]` for `f as i64`
- `builtin_to_float`/`builtin_to_integer` need `#[allow(clippy::cast_precision_loss)]` for `n as f64`
- **CRITICAL Clippy**: `3.14` in tests triggers `clippy::approx_constant` (it's close to PI). Use `3.5` instead.
- **CRITICAL**: Integration test strings containing embedded `"` (e.g. `" "` or `" is even"`) must use `r#"..."#` not `r"..."` — the inner `"` terminates a basic raw string prematurely.
- `$?rest` multi-field variables in patterns are not yet supported (pass 010); test `multifieldp` FALSE case instead of TRUE
- Test count: 453 after pass 008 (31 new unit tests + 5 integration tests)

### Pass 009: String and Symbol Function Surface
- Added 4 new builtins: `str-cat`, `sym-cat`, `str-length`, `sub-string`
- **Key pattern**: `str-cat`/`sym-cat` need `&mut EvalContext` to resolve symbol names via `symbol_table.resolve_symbol_str()`. A shared `concat_values_to_string(ctx, values, buf)` helper handles all types.
- **Clippy: `usize as i64`** in `str-length` → `#[allow(clippy::cast_possible_wrap)]`
- **Clippy: `i64 as usize`** in `sub-string` → `#[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]` (safe after bounds check)
- **Clippy: `make_empty` closure** capturing `ctx` — rename to `make_empty_string` to avoid confusion, and pass `ctx` explicitly since the closure can't capture `&mut EvalContext` while also borrowing it later.
- **CRITICAL**: Do NOT write duplicate function definitions when editing — review inserted text to avoid creating two copies of `builtin_str_cat`, etc.
- Float formatting in `str-cat`: reuse same logic as `format_printout_value` — if `f.fract() == 0.0`, format as `"{f:.1}"`, else `f.to_string()`.
- Test count: 487 after pass 009 (24 new unit tests + 10 integration tests)

### Pass 010: Multifield Function Surface
- Added 5 new builtins: `create$`, `length$`, `nth$`, `member$`, `subsetp`
- **`let...else` required**: Clippy enforces `let Value::Multifield(mf) = &values[n] else { ... };` instead of `match`
- **Safe 1-based index**: `usize::try_from(index - 1).ok().filter(|&i| i < mf.len())` handles negative/zero/OOB safely
- **`create$` flattens**: nested multifields are flattened into result
- **`member$` returns position integer** (not TRUE) when found; `clips_bool(false, ...)` when not found
- Test count: 527 ferric-runtime after pass 010 (40 new unit tests + 9 integration tests)

### Pass 011: I/O and Environment Function Surface
- Added `input_buffer: VecDeque<String>` to Engine; threaded as `Option<&'a mut VecDeque<String>>` in EvalContext
- EvalContext field addition requires updating ALL construction sites — use Python/sed scripting for 60+ sites
- `execute_actions` return expanded to `(bool, bool, bool, Vec<ActionError>)`: (fired, reset_requested, clear_requested, errors)
- `input_buffer` is live I/O; do NOT clear it in `reset()` (input queued before reset should survive)
- **Reset-from-RHS semantics**: apply reset then return `HaltReason::HaltRequested` — do NOT continue the run loop (causes infinite recursion)
- `FerricString::new(s, encoding)` → returns `Result<_, EncodingError>`; use `.map_err(|_| EvalError::TypeError{...})?`
- **CRITICAL Clippy**: `2.71828` approximates Euler's `e` — flagged by `clippy::approx_constant`. Use `1.5` or other non-special floats in tests
- **CRITICAL Clippy**: `items_after_statements` forbids nested `fn` defs after statements — extract to module-level helpers
- `builtin_format` uses `%d %f %e %g %s %n %r %%` with optional width/precision like `%10d`, `%-10s`, `%6.2f`
- `builtin_read` / `builtin_readline` return `Symbol("EOF")` when `input_buffer` is None or empty
- Test count: 557 after pass 011 (19 unit tests + 10 integration tests in `phase4_integration_tests.rs`)

### Pass 012: Agenda and Focus Query Function Surface
- `get-focus` / `get-focus-stack` added to `dispatch_builtin` in evaluator.rs; `Value::Multifield` = `Box<Multifield>` (NOT `Box<[Value]>`)
- `list-focus-stack`, `agenda`, `run` actions added in actions.rs; add `all_rule_info: &HashMap<RuleId, CompiledRuleInfo>` param
- **Borrow fix in engine.rs**: `.cloned()` on `self.rule_info.get(&rule_id)` so `&self.rule_info` can be passed as `all_rule_info`
- **Clippy**: `#[allow(clippy::unnecessary_wraps)]` for helpers that always return `Ok(())`; use `.map_or()` not `.map().unwrap_or()`
- Test count: 569 ferric-runtime (1015+ total) after pass 012

## Phase 5 Notes

See `phase5-pass-notes.md` for detailed notes on each Phase 5 pass.

### Key Phase 5 Patterns

- Two-step borrow pattern (engine.rs): validate ptr (shared ref) → thread check → validate ptr (mut ref)
- Read-only fact queries (`fact_count`, `get_fact_field`) use shared ref only; `facts()` does its own thread check
- `FerricValue::void()` used as `..FerricValue::void()` struct base for type conversions
- Multifield box-slice allocation: `into_boxed_slice()` → `Box::into_raw()` → `.cast::<FerricValue>()` (NOT `as *mut`)
- FactId to u64 in tests: `fact_id.data().as_ffi()` (requires `slotmap::Key as _` import)
- Test count: 110 ferric-ffi after pass 009

### Pass 012: Phase 5 Integration and Exit Validation
- Added `crates/ferric-ffi/src/tests/diagnostic_parity.rs` with 6 FFI tests
- Registered as `mod diagnostic_parity;` in `crates/ferric-ffi/src/tests.rs`
- Added 2 CLI diagnostic parity tests to `crates/ferric-cli/tests/cli_integration.rs`
- `cargo fmt` auto-applies after writing: always run `cargo fmt --all` then `cargo fmt --all --check`
- Do NOT leave stray `let _ = ();` lines from edit mistakes — re-read after every Edit
- Total test count: 1200 after pass 012

## Phase 6 Benchmarks

### Constraint Limitations (for benchmark CLIPS source)
- **`?nh&~?ph` compound constraints NOT supported**: The parser returns an error for connective atoms in constraints. Use `test (neq ?nh ?ph)` as a workaround.
- **`~literal` IS supported**: `(slot ~value)` compiles to `ConstantTestType::NotEqual` — works fine.
- **`not` with template patterns IS supported**: `(not (edge (label unknown)))` works correctly.
- **`modify` IS supported**: Tested in phase3 integration tests and benchmark smoke-runs.
- **Template pattern with wildcard slot IS supported**: `(not (seating (seat ?) (guest ?next)))` works.

### Waltz and Manners Benchmark Files
- `crates/ferric/benches/waltz_bench.rs` — 5-junction scene, labels edges convex/concave/boundary
- `crates/ferric/benches/manners_bench.rs` — 8-guest seating, uses `test (neq ...)` instead of `&~`
- Both registered in `crates/ferric/Cargo.toml` as `[[bench]]` entries with `harness = false`
- Each exposes two benchmarks: full load+reset+run, and reset+run-only variants
- Smoke-test: `cargo bench -p ferric --bench waltz_bench -- --test`
