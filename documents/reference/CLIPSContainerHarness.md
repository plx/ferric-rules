# CLIPS Docker Harness for Behavioral Comparison

This repository now includes a reusable Docker-based CLIPS harness intended to support
future behavior comparisons between `ferric-rules` and upstream CLIPS.

## Components

- `docker/clips-reference/Dockerfile`
  - Minimal Debian Bookworm image.
  - Installs the `clips` package from apt.
- `scripts/clips-reference.sh`
  - `build` command for multi-platform images (`linux/amd64` + `linux/arm64`).
  - `run` command that mounts this repository, loads one or more `.clp` files, then
    executes a scripted sequence of CLIPS expressions.

## Build image

### Multi-arch build and push

```bash
scripts/clips-reference.sh build \
  --image ghcr.io/<org>/clips-reference \
  --tag v0.1.0
```

> This mode uses `docker buildx build --push`, which requires a configured registry.

### Local test build

```bash
scripts/clips-reference.sh build --load
```

This builds a local `linux/amd64` image and loads it into your local Docker daemon.

## Run CLIPS scripts

Load CLIPS files and run explicit operations:

```bash
scripts/clips-reference.sh run \
  --file path/to/rules.clp \
  --file path/to/facts.clp \
  --op '(reset)' \
  --op '(run)'
```

Run using an operations file (`one expression per line`):

```bash
scripts/clips-reference.sh run \
  --file path/to/rules.clp \
  --ops-file path/to/sequence.clp
```

If no operations are supplied, the harness defaults to:

```clips
(reset)
(run)
```

## Suggested comparison workflow

1. Keep canonical CLIPS fixtures in a dedicated folder (for example
   `documents/reference/fixtures/`).
2. Run the same fixtures through this CLIPS harness and (later) the ferric runtime.
3. Capture and diff outputs (agenda behavior, printed traces, final facts, etc.).
4. Promote stable fixture sets into regression tests once ferric feature parity expands.
