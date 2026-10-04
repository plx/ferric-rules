# ferric-rules

`ferric-rules` is intended to be an *almost* drop-in replacement for the CLIPS rules engine.

It is written in Rust and has been designed for easy building and for easy embedding within other applications.

At this point we have a fully-functional prototype with all core functionality implemented and *apparently* working, and are continuing to focus on validation, polish, and performance.

We primarily use `just` to organize project-related commands, but feel free to make direct use of `cargo`, etc., when convenient.

Please always run `just preflight-pr` before opening a PR or pushing code to update a PR—let's find-and-fix formatting and linter issues locally, rather than in CI.

## Benchmarking

**Performance numbers in commit messages and PR descriptions must come from `cargo bench` (release profile) output.** Never report timings from `cargo test`, `cargo test --bench`, or debug-mode runs—these compile without optimizations and produce numbers 10–25x slower than release, which is what CI measures and what users experience.

Use the `just bench-*` targets (e.g. `just bench-join`, `just bench-waltz`) or `cargo bench -p ferric-rules` directly. These always compile in release mode with LTO.

When claiming performance improvements:
- Run `cargo bench` **before and after** the change, on the same machine, in the same profile.
- Quote the actual Criterion median values from the output, not theoretical estimates.
- Note the machine/environment if relevant (CI numbers may differ from local Apple Silicon results).

### Scaling regression checks

`just scaling-check` runs the thirteen `#[ignore]` tests in `crates/ferric-rules/tests/scaling_tests.rs` (release mode), which assert asymptotic scaling behavior of core operations: join propagation, engine run, retraction cascade, churn lifecycle, alpha fanout, exists support assertion, indexed NCC completion, independent negative cleanup, dormant focus selection, template multislot join, sequence constant pruning, bound sequence joins, and sequence negative admission. Each test measures at two input sizes (4x apart) and asserts the time ratio stays within bounds consistent with the expected complexity class. This catches full complexity-class regressions (e.g. O(N) → O(N²)) without relying on absolute timing thresholds.

## Repository hygiene

- Do not commit measurement dumps, evidence JSON, or audit/execution-record documents. Put numbers in the PR description; raw output belongs in CI artifacts or local scratch space.
- Do not write tests that assert the literal text of CI workflow YAML.
- Plans and work tracking live in GitHub issues, not in-repo plan documents. [`docs/history.md`](docs/history.md) is a short record of past phases; do not extend it into a status log.
