# CLIPS compatibility discovery corpus

The [granular corpus](corpus/README.md) provides reference-verified CLIPS programs,
coverage levels, exact output oracles, and active known-gap characterizations.
Run `just compat-corpus` for Ferric and `just compat-corpus-reference` to recheck
the CLIPS 6.30 oracles.

The existing [semantic differential lane](../examples/ferric-semantic/README.md)
and `crates/ferric-rules/tests/ferric_semantic_regressions.rs` remain complementary.
