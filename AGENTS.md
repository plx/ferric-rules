# ferric-rules

`ferric-rules` is intended to be an *almost* drop-in replacement for the CLIPS rules engine.

It is written in Rust and has been designed for easy building and for easy embedding within other applications.

At this point we have a fully-functional prototype with all core functionality implemented and *apparently* working, and are continuing to focus on validation, polish, and performance.

We primarily use `just` to organize project-related commands, but feel free to make direct use of `cargo`, etc., when convenient.

Please always run `just preflight-pr` before opening a PR or pushing code to update a PR—let's find-and-fix formatting and linter issues locally, rather than in CI.
