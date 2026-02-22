# ferric-rules
An embeddable rust rewrite of (most of) the CLIPS rules engine.

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
