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

## Files and Conventions
- Cargo workspace with `--workspace --all-targets` flag for comprehensive checking
- Test module lint checks pass with `cargo clippy --workspace --all-targets -- -D warnings`
- Property-based tests in `proptests` module
