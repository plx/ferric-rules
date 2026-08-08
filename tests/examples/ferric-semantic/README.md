# Pinned-CLIPS semantic differential matrix

This first-party corpus is the release-blocking differential lane for the
production-readiness regressions `FR-RETE-001` through `FR-RETE-018` and
`FR-LANG-001`/`002`. Every `.clp` file is a primary fixture with a version-2
structured oracle in `../compat-oracles.json`. Files ending in `.stage` are
additional digest-bound sources loaded by that fixture's canonical scenario;
they are not standalone corpus entries.

The lane always executes the same scenario against Ferric and the repository's
pinned CLIPS reference image. `../compat-semantic-policy.json` requires all 22
scenario IDs covering 20 audit IDs, verifies the measured reference binary and
library digests, and
allows a known divergence only when its classification, reason, mismatch
fields, and normalized Ferric observation fingerprint all match exactly. A
case that begins matching CLIPS makes its deviation stale and fails the gate
until the policy is removed.

Run the complete lane from the repository root:

```sh
docker build -t ferric-rules/clips-reference:latest docker/clips-reference/
cargo build --release -p ferric-rules-cli
just compat-semantic-lane
```

The ordinary Ferric-only regression suite is intentionally separate:

```sh
cargo test -p ferric-rules --test ferric_semantic_regressions
```

Do not copy reference output into an unstructured assertion. Each fixture must
retain at least one independently reviewable semantic effect, and every staged
source must remain declared, path-contained, size-bounded, and SHA-256-bound by
its oracle scenario.
