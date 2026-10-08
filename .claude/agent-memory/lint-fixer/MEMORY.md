# ferric-rules Lint Fixer Memory

## Project Overview
- Rust project: CLIPS rules engine implementation (ferric-rules)
- Workspace members are declared in root `Cargo.toml`; `docs/project-overview.md` maps their roles. The facade crate is `ferric-rules`.
- Uses `cargo clippy` for linting

## Lint Patterns

### Common Clippy Warnings Fixed
1. **needless_raw_string_hashes**: Raw strings with `#` prefix are unnecessary when the string doesn't contain `"`. Change `r#"..."#` to `r"..."`
2. **approx_constant**: Use the appropriate standard-library constant when that is the intended value. A scoped allow is appropriate only when a specific decimal test input is intentional.
3. **redundant_closure_for_method_calls**: Replace `.map(|n| n.to_string())` with `.map(std::string::ToString::to_string)`
4. **uninlined_format_args**: In format! macros, inline variables directly instead of passing as positional args: `format!("{x}")` instead of `format!("{}", x)`
5. **ref_option**: Use `Option<&T>` instead of `&Option<T>` in function parameters. Requires changing `.clone()` to `.cloned()` for owned copies, and callers pass `.as_ref()` instead of `&opt`
6. **cast_precision_loss**: Use `From` for supported lossless conversions. There is no `From<i64> for f64`; when CLIPS numeric semantics require that conversion, use an intentional cast with a narrowly justified allow, as in the runtime evaluator.
7. **cast_possible_truncation**: `f64 as i64` casts; allow with `#[allow(clippy::cast_possible_truncation)]` when intentional (e.g., integer division semantics)
8. **float_cmp**: Choose the comparison required by the contract: approximate tolerance for numerical error, or `to_bits()` for exact stored-bit identity. Use a scoped allow only when direct numeric equality is intentional.
9. **doc_markdown**: Type names in doc comments need backticks (e.g., `VarMap` not VarMap)
10. **format_push_string**: `facts.push_str(&format!(...))` should use `writeln!` or `write!` instead. Requires `use std::fmt::Write;` import. Use `writeln!` when format ends with `\n`, `write!` otherwise.

### thiserror `#[error]` and ref_option interaction
- When changing `format_span(span: &Option<T>)` to `format_span(span: Option<&T>)`, the thiserror `#[error]` attributes that call `format_span(.span)` need to change to `format_span(.span.as_ref())` since `.span` in thiserror context gives `&Option<T>`

## Files and Conventions
- Cargo workspace with `--workspace --all-targets` flag for comprehensive checking
- Test module lint checks pass with `cargo clippy --workspace --all-targets -- -D warnings`
- Property-based tests in `proptests` module
- Root `[workspace.lints.clippy]` sets `all` to deny and `pedantic` to warn, with specific allows. Most workspace members inherit the root tables through `[lints] workspace = true`. The exceptions are `ferric-rules-ffi`, `ferric-rules-napi` and `ferric-rules-python`: they need `unsafe_code = "allow"` (the workspace denies it), so they declare crate-local `[lints.rust]`/`[lints.clippy]` tables that mirror the workspace Clippy levels. napi and python also allow `new_without_default`, `used_underscore_binding` and `needless_pass_by_value`, and napi repeats its whole Clippy set as crate attributes in `src/lib.rs`. A change to the root Clippy table must be copied into those three `Cargo.toml` files (and the napi attributes); a lint-level change for one of those crates goes in its own `Cargo.toml`. The `just clippy` recipe adds `-D warnings`.
