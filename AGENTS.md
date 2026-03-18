# ferric-rules

`ferric-rules` is intended to be an *almost* drop-in replacement for the CLIPS rules engine.

It is written in Rust and has been designed for easy building and for easy embedding within other applications.

At this point we have a fully-functional prototype with all core functionality implemented and *apparently* working, and are continuing to focus on validation, polish, and performance.

We primarily use `just` to organize project-related commands, but feel free to make direct use of `cargo`, etc., when convenient.

Please always run `just preflight-pr` before opening a PR or pushing code to update a PR—let's find-and-fix formatting and linter issues locally, rather than in CI.

## Benchmarking

**Performance numbers in commit messages and PR descriptions must come from `cargo bench` (release profile) output.** Never report timings from `cargo test`, `cargo test --bench`, or debug-mode runs—these compile without optimizations and produce numbers 10–25x slower than release, which is what CI measures and what users experience.

Use the `just bench-*` targets (e.g. `just bench-join`, `just bench-waltz`) or `cargo bench -p ferric` directly. These always compile in release mode with LTO.

When claiming performance improvements:
- Run `cargo bench` **before and after** the change, on the same machine, in the same profile.
- Quote the actual Criterion median values from the output, not theoretical estimates.
- Note the machine/environment if relevant (CI numbers may differ from local Apple Silicon results).
