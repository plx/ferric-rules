# Go Bindings CI Policy

## What CI runs

The **Go Bindings** job in `.github/workflows/ci.yml` runs on every pull
request to `main` and every push to `main` (changes confined to `site/` skip
it). It:

1. builds the Rust static library and copies it with the generated header into
   `internal/ffi/lib/` (`just build-go-ffi`);
2. verifies that the committed `ferric.h` copies match the generated header
   (`just check-ffi-header`);
3. runs `golangci-lint` (the version is pinned in both the workflow and the
   justfile's `golangci_lint_version`);
4. runs the full Go test suite once with the race detector
   (`just test-go-race`, i.e. `go test -race -v ./...`).

Any Go binding change should pass this job before merging. Reproduce it
locally with `just go-lint` and `just test-go-race`; `just preflight-pr`
includes the lint step.

## Repeat-run stress testing (local)

Go bindings use CGo and serialized native ownership whose lifecycle races may
not appear in a single run. CI runs the suite once; repeat runs are a local
tool for concurrency-related changes:

```sh
just test-go-stress      # default: 10 iterations with -race
just test-go-stress 30   # custom iteration count
```

For thorough validation after concurrency-related changes, use 30 or higher —
the original due diligence review used `-count=30` and surfaced intermittent
failures at that level.

### Interpreting failures

Race-detector and repeat-run failures typically indicate:

- **Native overlap or stale handles**: every raw handle operation must remain
  inside its serialized lifetime lease through output/error copying. Goroutine
  migration is supported; constructors pin only while copying TLS diagnostics.
- **Race conditions**: concurrent access to shared state without proper
  synchronization. The `-race` flag will report the exact goroutines and
  memory locations involved.
- **Resource leaks**: C-allocated memory not freed promptly, causing
  use-after-free or double-free under repeated runs.

If a failure is not reproducible locally, increase the count (`-count=50` or
higher) or try running on a Linux VM to match CI conditions.

## Native ownership sanitizer

The **FFI Harnesses** job in `ci.yml` includes a **Go/C lifecycle
(AddressSanitizer)** step that builds the Rust static library and the cgo test
binaries with AddressSanitizer on Linux. Its focused regressions cover
post-`Close` calls plus empty, mixed, deeply nested, large, and failed
multifield construction. LeakSanitizer is enabled so both successful copy/free
cycles and partial conversion failures must release every Ferric-owned
allocation.

Go slices passed to `AssertFact` or `AssertTemplate` remain caller-owned.
The binding borrows their converted elements only while calling
`ferric_value_multifield_copy`; the returned recursive tree is entirely
Ferric-owned and is released through `ferric_value_free`. No Go or C allocator
storage is transferred to Rust's value cleanup.

Run `just ffi-go-asan-harness` on a native Linux host to reproduce this step.
The broader operating-system and clean-consumer binding matrix is tracked
separately by FR-DIST-008.
