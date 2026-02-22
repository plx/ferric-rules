# ferric-rules
An embeddable rust rewrite of (most of) the CLIPS rules engine.

## Pre-flight checks
Run the same command surface used by CI before opening a PR:

```bash
./scripts/preflight.sh all
```

You can also run an individual gate:

```bash
./scripts/preflight.sh clippy
```

## CLIPS reference harness

To support behavioral comparisons against upstream CLIPS, this repository includes a
Docker-based reference harness:

- `docker/clips-reference/Dockerfile`
- `scripts/clips-reference.sh`
- `documents/reference/CLIPSContainerHarness.md`

Quick start:

```bash
# Build local image
scripts/clips-reference.sh build --load

# Run CLIPS with one or more source files and operations
scripts/clips-reference.sh run --file path/to/rules.clp --op '(reset)' --op '(run)'
```

