# Granular CLIPS compatibility corpus

This is a systematic discovery and characterization suite for Ferric's targeted
CLIPS subset. Its 219 small programs progress from individual features to
boundary cases and controlled interactions. Each program has a nonempty,
CLIPS-verified output oracle. There are 153 clean conformance cases and 66 active
characterizations of known differences on checkout `eb24cc50`.

This is broad coverage, not a proof of complete CLIPS equivalence. The explicit
[coverage matrix](COVERAGE.md) records what is exercised, excluded, or still needs
another execution protocol. [GAPS.md](GAPS.md) links the 27 newly filed issues and
previously tracked defects. No engine behavior is changed by this suite.

## Run

```sh
# Ordinary Rust/CI run: no Docker dependency, includes known-gap assertions.
just compat-corpus

# One feature or one program; an unmatched filter fails.
FERRIC_CORPUS_FILTER=patterns/010 just compat-corpus -- --nocapture

# Progress through basic, boundary, and interaction cases separately.
FERRIC_CORPUS_LEVEL=basic just compat-corpus -- --nocapture

# Recheck the CLIPS goldens with an actual local CLIPS 6.30 Docker image.
# Build the image first if needed: just clips-build --load
just compat-corpus-reference
just compat-corpus-reference --filter queries/ --report /tmp/clips-reference.json
just compat-corpus-reference --level boundary

# Capture Ferric observations for diagnosis (does not update expectations).
FERRIC_CORPUS_REPORT=/tmp/ferric-corpus.json just compat-corpus -- --nocapture
```

`cargo test --workspace` automatically runs `crates/ferric/tests/compat_corpus.rs`.
The original `clips_compat` integration suite remains available and unchanged.
The reference command requires Docker and CLIPS 6.30; it fails instead of falling
back to a Ferric-only run. Goldens are never regenerated automatically.

## Corpus contract

- Each `.clp` contains complete constructs, suitable for loading into a fresh
  engine. The harness supplies `load`, `reset`, and bounded `run` operations.
- The companion `.out` is exact CLIPS program output, including whitespace,
  numeric formatting, quoting, and line order. It must be nonempty and end in a
  newline. No sorting, float normalization, or whitespace trimming is applied.
- Optional `.in` files supply input verbatim to CLIPS stdin, and the same lines
  through Ferric's `Engine::push_input`. Input cases use exactly one reset.
- `manifest.json` registers every program exactly once, with `basic`, `boundary`,
  or `interaction` level and coverage tags. `resets: 2` repeats reset/run in the
  same engine; the golden concatenates both runs. This tests restoration of
  globals, deffacts, refraction, and derived working memory.
- Prefer a single observable distinction per program. Use salience or phase
  facts where side-effect ordering matters. Keep each problematic function or
  invalid index in its own program so an earlier error cannot conceal it.
- Mutation cases observe subsequent rule matches or query results. These are
  behavioral programs, not parser-only acceptance tests. General snapshots of
  every final fact or agenda entry are not currently part of this protocol.
- Programs must terminate well below 1,000 rule firings per reset/run, must not
  modify the statistics watch setting, and must not print CLIPS diagnostic-like
  messages (`[CODE123] message`). The reference wrapper uses unique frames and
  CLIPS statistics to distinguish complete execution from a truncated run.

## Conformance versus characterization

A case without `gap` must load, reset, and run successfully, produce exactly its
CLIPS `.out`, and emit no Ferric action diagnostics.

A `gap` entry records the issue URL, a short summary, and the exact current Ferric
`phase`, `output`, and `diagnostics`. These cases run normally; they are not
ignored tests or broad expected-failure catches. A different diagnostic, changed
output, or unexpected match with CLIPS fails. After a separate engine fix,
remove the corresponding `gap` entry once the same `.out` oracle passes.

Some cases exercise different manifestations of one defect; those cases share
an issue. Diagnostic source locations intentionally form part of the current
characterization, so moving fixture code requires reviewing those locations.
Issue state on GitHub is not consulted at test time: this worktree predates some
fixes and consolidated issues on the default branch.

## Oracle provenance and safety

All current `.out` files were executed on CLIPS **6.30 (3/17/15)** using the local
`ferric-rules/clips-reference:latest` image. The image ID is recorded in the
manifest. It identifies this local build, not a portable registry digest.
`compat-corpus-reference` resolves its supplied image tag to an immutable local
ID once per run and records that ID and the actual version in an optional report.

Every program gets a separate Docker container, a read-only source mount, a
15-second default deadline, and a 1,000-firing bound. Timed-out containers are
removed explicitly. Load success, complete output/statistics frames, diagnostics,
and the firing bound are all checked before accepting output. The specific
CLIPS warning about redefining the built-in MAIN module is allowed for import
fixtures; other diagnostic codes fail reference verification.

The Rust runner has the same firing bound. These are bounded authored programs;
the runner is not a sandbox for arbitrary nonterminating procedural code.

## Extend incrementally

1. Add a focused `.clp` and proposed `.out`, plus `.in` only when necessary.
2. Register its relative path, level, and coverage tags in `manifest.json`.
3. Run the reference verifier with `--filter`; correct invalid CLIPS source or
   mistaken expectations before judging Ferric behavior.
4. Run the Rust suite with `FERRIC_CORPUS_FILTER` and optionally save a report.
5. Minimize any discrepancy and check existing issues. File a separate issue for
   each new semantic gap with source, exact outputs/diagnostics, reference
   version, Ferric revision, reproduction commands, and acceptance criteria.
6. Add an explicit `gap` observation only after confirming the discrepancy.
   Update the coverage matrix and run both corpus commands.
