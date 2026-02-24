# 102 Or Conditional Element

## Sequence Position
2/9 (high impact — resolves a subset of the ~350 "bare connective" errors; parser + loader/compiler work; benefits from 101 being done first since many files use both `logical` and `or` CEs).

## Behavioral Divergence
CLIPS supports `(or ...)` as a conditional element at the rule LHS level, meaning "match if any one of these sub-patterns matches." Ferric does not recognize `or` as a CE keyword, so `(or (pattern1) (pattern2))` is misinterpreted as an ordered pattern with relation name `or`, leading to "bare connective in pattern" errors when inner patterns contain assignment (`<-`) or other CE-level constructs.

Example from the corpus:
```clips
(defrule example
   (or (engine-starts No)
       (engine-rotates Yes))
   =>
   (printout t "Engine problem detected" crlf))
```

Nested with `logical` and variable assignment:
```clips
(defrule partial-OR2-gwhip[10]-1
   (declare (salience ?*partial-OR2-gwhip[10]-salience-1*))
   (logical
      ?ORk <- (ORk-chain
         (type partial-OR2-gwhip)
         (context ?cont)
         (length 9)
         ...))
   =>
   ...)
```
Here `?ORk <- (ORk-chain ...)` is a pattern-bound variable assignment inside a `(logical ...)` block; the `<-` token becomes a "bare connective" error.

## Apparent Ferric-Side Root Cause
`crates/ferric-parser/src/stage2.rs` — `interpret_conditional_pattern()` does not have a case for `"or"`. The word `or` falls through to ordered-pattern interpretation. Sub-expressions with `<-` assignment or nested CEs become invalid constraints.

Downstream, `crates/ferric-runtime/src/loader.rs` has no `Pattern::Or` translation path. Even if parsed, `or` CEs require either Rete-level disjunction or rule duplication during compilation.

## Implementation Plan
1. Add `Pattern::Or` variant to the Stage 2 AST.
- Add `Or(Vec<Pattern>, Span)` to the `Pattern` enum.
- In `interpret_conditional_pattern()`, add an `"or"` match arm that recursively interprets sub-patterns.
- CLIPS requires at least two sub-patterns in `(or ...)`.
- Caveat: parsing alone does not provide disjunction semantics.

2. Implement `Pattern::Or` in the loader via rule duplication.
- In `crates/ferric-runtime/src/loader.rs`, when an `(or P1 P2 ... Pn)` CE is encountered during rule translation, duplicate the rule into N variants, each with one branch of the `or`.
- This is the standard CLIPS compilation strategy: `(or A B)` in a rule creates two internal rules with the same RHS.
- Ensure the duplicated rules share the same salience, module, and declaration properties.
- Caveat: rule duplication can increase Rete node count; large `or` CEs with many branches will generate many internal rules.

3. Add support for pattern-level variable assignment (`?var <- (pattern)`) within nested CEs.
- This is a prerequisite for many corpus files that combine `(logical ...)` with `?var <- (pattern)`.
- The parser should recognize `<-` within a CE context as a binding annotation, not a constraint connective.
- In `interpret_conditional_pattern()` sub-pattern processing, detect the `?var <- expr` form and produce `Pattern::Assigned(var, Box<Pattern>)` (or use the existing assignment mechanism if one exists).
- Caveat: assignment support is orthogonal to `or` CE but commonly co-occurs.

4. Add parser and loader tests.
- Parser: `(or (a) (b))` produces `Pattern::Or` with two children.
- Parser: `(or (a) (not (b)))` nests correctly.
- Loader: rule with `(or (a) (b))` compiles into two internal rules.
- Loader: verify both branches fire independently.
- Caveat: full corpus coverage depends on other fixes (e.g., `logical`, `switch`).

## Test And Verification
1. Parser unit tests:
```bash
cargo test -p ferric-parser interpret_or_ce
```

2. Loader/runtime tests:
```bash
cargo test -p ferric-runtime or_conditional_element
```

3. Compatibility smoke checks:
```bash
cargo run -p ferric-cli -- check tests/examples/clips-official/clipsjni/examples/AutoDemo/autodemo.clp
cargo run -p ferric-cli -- check 'tests/examples/csp-rules-v2.1/CSP-Rules-Generic/CHAIN-RULES-EXOTIC/PARTIAL-OR2-G-WHIPS/Partial-OR2-gWhips[10].clp'
```
Expected near-term outcome: "bare connective in pattern" errors from `(or ...)` and `?var <-` disappear; files may still fail for separate reasons.
