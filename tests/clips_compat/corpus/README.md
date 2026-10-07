# Granular CLIPS compatibility corpus

This is a systematic discovery and characterization suite for Ferric's targeted
CLIPS subset. Its 1010 small programs progress from individual features to
boundary cases and controlled interactions. Each program has a nonempty,
CLIPS-verified output oracle. There are 1002 clean conformance cases, 183 of which
reproduce a CLIPS error, and 8 active characterizations of documented
differences: CLIPS output that is not UTF-8, malformed `format` directives,
and equal-salience ties involving identical negative/NCC joins or multi-pattern
`exists`.

This is broad coverage, not a proof of complete CLIPS equivalence. The explicit
[coverage matrix](COVERAGE.md) records what is exercised, excluded, or still needs
another execution protocol. [GAPS.md](GAPS.md) links the 27 newly filed issues and
previously tracked defects. Those issues preserve the original discovery on
`eb24cc50`; the manifest records the refreshed observations. No engine behavior
is changed by this suite.

## Run

```sh
# Ordinary Rust/CI run: no Docker dependency, includes known-gap assertions.
just compat-corpus

# One feature or one program; an unmatched filter fails.
FERRIC_CORPUS_FILTER=patterns/010 just compat-corpus -- --nocapture

# Progress through basic, boundary, and interaction cases separately.
FERRIC_CORPUS_LEVEL=basic just compat-corpus -- --nocapture

# Recheck the CLIPS goldens with an actual local CLIPS 6.30 Docker image.
# Build the image first if needed:
# docker build -t ferric-rules/clips-reference:latest docker/clips-reference/
just compat-corpus-reference
just compat-corpus-reference --filter queries/ --report /tmp/clips-reference.json
just compat-corpus-reference --level boundary

# Capture Ferric observations for diagnosis (does not update expectations).
FERRIC_CORPUS_REPORT=/tmp/ferric-corpus.json just compat-corpus -- --nocapture
```

`cargo test --workspace` automatically runs
[`crates/ferric-rules/tests/compat_corpus/`](../../../crates/ferric-rules/tests/compat_corpus/main.rs).
Its [`host.rs`](../../../crates/ferric-rules/tests/compat_corpus/host.rs) holds
the few host-driven lifecycles this protocol cannot express: top-level
assertions and host fact operations between runs.
The existing [semantic differential lane](../../examples/ferric-semantic/README.md)
and [Ferric regression suite](../../../crates/ferric-rules/tests/ferric_semantic_regressions.rs)
provide complementary coverage and retain their own execution protocols.
The reference command requires Docker and CLIPS 6.30; it fails instead of falling
back to a Ferric-only run. Goldens are never regenerated automatically.

## Corpus contract

- Each `.clp` contains complete constructs, suitable for loading into a fresh
  engine. The harness supplies `load`, `reset`, and bounded `run` operations.
- The companion `.out` is exact CLIPS program output, including whitespace,
  numeric formatting, quoting, and line order. It must be nonempty and end in a
  newline. No sorting, float normalization, or whitespace trimming is applied.
  Outputs compare as bytes, so an oracle that is not UTF-8 can never match.
- Optional `.in` files supply input verbatim to CLIPS stdin, and the same lines
  through Ferric's `Engine::push_input`, where each CR or LF ends a line as it
  does for CLIPS `read`/`readline`. Input cases use exactly one reset.
- `manifest.json` registers every program exactly once, with `basic`, `boundary`,
  or `interaction` level and coverage tags. `resets: 2` repeats reset/run in the
  same engine; the golden concatenates both runs. This tests restoration of
  globals, deffacts, refraction, and derived working memory. `strategy:
  "breadth"` runs the program under the breadth strategy (CLIPS
  `(set-strategy breadth)`, Ferric `EngineConfig::with_strategy`).
- Prefer a single observable distinction per program. Use salience or phase
  facts where side-effect ordering matters, except when the case explicitly
  tests depth/breadth activation ties. Keep each problematic function or
  invalid index in its own program so an earlier error cannot conceal it.
- Mutation cases observe subsequent rule matches or query results. These are
  behavioral programs, not parser-only acceptance tests. General snapshots of
  every final fact or agenda entry are not currently part of this protocol.
- Programs must terminate well below 1,000 rule firings per reset/run, must not
  modify the statistics watch setting, and must not print CLIPS diagnostic-like
  messages (`[CODE123] message`). The reference wrapper uses unique frames and
  CLIPS statistics to distinguish complete execution from a truncated run.
- A program where CLIPS reports an error declares where. With `error: "load"`
  CLIPS rejects the program, the golden is its load diagnostic, and Ferric must
  reject the program at load. With `error: "run"` CLIPS halts on a run-time
  error; Ferric must report an error and print the golden without its CLIPS
  diagnostics. The runner strips only the run-time diagnostics listed in
  `diagnostic_offset` in
  [`compat_corpus/main.rs`](../../../crates/ferric-rules/tests/compat_corpus/main.rs),
  from the diagnostic to the end of its line; if it follows partially printed
  output, that program prefix is kept. Other bracketed text stays output, so a
  run-error golden with a new code needs that list extended after CLIPS
  verification. Ferric's own diagnostic text is never compared.
- Cases marked `recoverable_fact_notices: true` require successful execution
  despite CLIPS notices for missing, negative, or wrong-type fact designators.
  The full oracle retains the exact `[PRNTUTIL1]` missing-fact and
  `[ARGACCES5]` fact-designator notices; output comparison removes only those
  precise messages. Fatal slot/operand diagnostics are still errors. Ferric may omit
  the recoverable notices.
- Cases marked `recoverable_control_notices: true` allow only the exact
  recoverable clear-refusal and missing-module notices captured from CLIPS.
  Their golden retains the notices; comparison omits them from ordinary output
  and from Ferric's separate notice routers. Changed messages, fatal errors,
  load diagnostics, and undeclared cases remain failures.
- Cases marked `recoverable_random_notices: true` allow only the exact
  `MISCFUN2` wrong-count and `MISCFUN3` reversed-bounds notices. The oracle
  retains them; comparison removes them from ordinary output and from Ferric's
  notice router while requiring successful execution and the correct draws.
- Cases marked `recoverable_build_notices: true` allow only CLIPS's exact
  rejection of a `build` that would redefine a deftemplate in use: the
  `[CSTRCPSR4] Cannot redefine deftemplate NAME while it is in use.` message
  with its leading newline, and the `ERROR:` echo of the construct up to its
  module-qualified name. The oracle retains them. Comparison moves each one to
  the expected notices as its message line, and Ferric's own rejection text
  for the same deftemplate or ordered relation is compared as that line, so
  the number and order of rejected builds and their names must still agree.
  Such a case must otherwise succeed.
- Cases marked `recoverable_introspection_notices: true` allow only CLIPS's
  exact recoverable introspection notices: `[PRNTUTIL1] Unable to find
  deftemplate NAME.` and the `[ARGACCES5]` notice for a first argument of a
  `deftemplate-slot-*` function or a construct list that is not a deftemplate
  or defmodule name. The oracle retains them; comparison moves them to the
  expected notices, so Ferric must print the same notices, in order, on
  `werror`. Such a case must otherwise succeed.
- A separate exception is the two recoverable `[SCANNER1]` scanner notices (integer
  overflow, unterminated string). CLIPS prints them on its warning and error
  routers, interleaved with `t` in the oracle; the runner removes them from the
  oracle and compares them with Ferric's `wwarning` and `werror` output.

## Conformance versus characterization

A case without `gap` or `error` must load, reset, and run successfully, produce
exactly its CLIPS `.out`, and emit no Ferric action diagnostics.

Every conforming case that loads is then replayed in Ferric, in two tests of
their own. A replay from a snapshot restored before the first firing, in JSON
(unless the state holds a non-finite float) and CBOR, must reproduce the golden
exactly; CBOR replays also restore before each of the first 12 firings. A replay
with the rules loaded after the first reset (unless deffacts follow its first
rule), with and without a snapshot in between, must print the same lines in
any order: CLIPS itself orders the activations of rules loaded after reset
differently from a load, reset, run. These replays check Ferric's backfill and
persistence against the CLIPS golden.

A `gap` entry records the issue URL, a short summary, and the exact current Ferric
`phase`, `output`, and `diagnostics`. These cases run normally; they are not
ignored tests or broad expected-failure catches. A different diagnostic, changed
output, or unexpected match with CLIPS fails. After a separate engine fix,
remove the corresponding `gap` entry once the same `.out` oracle passes.

Some cases exercise different manifestations of one defect; those cases share
an issue. Diagnostic source locations intentionally form part of the current
characterization, so moving fixture code requires reviewing those locations.
Issue state on GitHub is not consulted at test time; the manifest's recorded
observation determines whether a case is a conformance check or an active gap.
The issue index also preserves discoveries that subsequent engine changes fix.

## Oracle provenance and safety

All current `.out` files were executed on CLIPS **6.30 (3/17/15)** using the local
`ferric-rules/clips-reference:latest` image. The image ID is recorded in the
manifest. It identifies this local build, not a portable registry digest.
`compat-corpus-reference` resolves its supplied image tag to an immutable local
ID once per run and records that ID and the actual version in an optional report.

Every program gets a separate Docker container, a read-only source mount, a
15-second default deadline, and a 1,000-firing bound. Timed-out containers are
removed explicitly. Load success, complete output/statistics frames, diagnostics,
and the firing bound are all checked before accepting output. During source
loading, the specific warnings for redefining the built-in MAIN module or a
deffunction, deftemplate, or defrule (with the defrule warning's `+j`/`=j`-style
suffixes), and the exact integer-overflow scanner notice are allowed.
Other diagnostic codes fail reference verification unless the case
declares an `error`. A load-error case is loaded with `load*`, which must fail
with a diagnostic; a run-error case must print at least one run-time diagnostic.

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
