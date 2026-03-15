# ferric-rules Lint Fixer Memory

## Project Overview
- Rust project: CLIPS rules engine implementation (ferric-rules)
- Workspace structure with multiple crates (ferric-runtime, ferric, ferric-core, ferric-parser)
- Uses `cargo clippy` for linting

## Lint Patterns

### Common Clippy Warnings Fixed
1. **needless_raw_string_hashes**: Raw strings with `#` prefix are unnecessary when the string doesn't contain `"`. Change `r#"..."#` to `r"..."`
2. **approx_constant**: Comparisons like `(f - 3.14).abs() < 0.001` in tests should be wrapped with `#[allow(clippy::approx_constant)]`
3. **redundant_closure_for_method_calls**: Replace `.map(|n| n.to_string())` with `.map(std::string::ToString::to_string)`
4. **uninlined_format_args**: In format! macros, inline variables directly instead of passing as positional args: `format!("{x}")` instead of `format!("{}", x)`
5. **ref_option**: Use `Option<&T>` instead of `&Option<T>` in function parameters. Requires changing `.clone()` to `.cloned()` for owned copies, and callers pass `.as_ref()` instead of `&opt`
6. **cast_precision_loss**: `i64 as f64` casts; use `f64::from(i)` instead; allow with `#[allow(clippy::cast_precision_loss)]` if necessary
7. **cast_possible_truncation**: `f64 as i64` casts; allow with `#[allow(clippy::cast_possible_truncation)]` when intentional (e.g., integer division semantics)
8. **float_cmp**: Direct `==` comparison of f64 values; allow with `#[allow(clippy::float_cmp)]` when combined with epsilon-based comparison as fallback
9. **doc_markdown**: Type names in doc comments need backticks (e.g., `VarMap` not VarMap)
10. **format_push_string**: `facts.push_str(&format!(...))` should use `writeln!` or `write!` instead. Requires `use std::fmt::Write;` import. Use `writeln!` when format ends with `\n`, `write!` otherwise.

### thiserror `#[error]` and ref_option interaction
- When changing `format_span(span: &Option<T>)` to `format_span(span: Option<&T>)`, the thiserror `#[error]` attributes that call `format_span(.span)` need to change to `format_span(.span.as_ref())` since `.span` in thiserror context gives `&Option<T>`

## Files and Conventions
- Cargo workspace with `--workspace --all-targets` flag for comprehensive checking
- Test module lint checks pass with `cargo clippy --workspace --all-targets -- -D warnings`
- Property-based tests in `proptests` module
- Project uses `#![deny(clippy::all, clippy::pedantic)]` at workspace level
