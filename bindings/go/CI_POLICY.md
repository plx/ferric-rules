# Go Bindings CI Policy

## Repeat-Run Stress Testing

Go bindings use `runtime.LockOSThread()` and CGo, making them sensitive to
goroutine-to-thread affinity issues that single-run tests may not catch. The
CI pipeline includes a **Go Stress Test** job that runs all Go binding tests
repeatedly with the race detector enabled (`go test -race -count=10 ./...`).

### When stress testing is required

The stress test job runs on **every push to `main` and every pull request**.
Any Go binding change should pass this job before merging.

### Running locally

Use the justfile target to reproduce the CI stress run locally:

```sh
just test-go-stress      # default: 10 iterations with -race
just test-go-stress 30   # custom iteration count
```

A count of 10 is the CI default. For thorough local validation (e.g., after
concurrency-related changes), use 30 or higher — the original due diligence
review used `-count=30` and surfaced intermittent failures at that level.

### Interpreting failures

Stress-test failures typically indicate:

- **Thread-affinity violations**: operations escaping a locked OS thread due
  to goroutine migration. Fix by ensuring all FFI calls happen within a
  `runtime.LockOSThread()` scope.
- **Race conditions**: concurrent access to shared state without proper
  synchronization. The `-race` flag will report the exact goroutines and
  memory locations involved.
- **Resource leaks**: C-allocated memory not freed promptly, causing
  use-after-free or double-free under repeated runs.

If a stress-test failure is not reproducible locally, increase the count
(`-count=50` or higher) or try running on a Linux VM to match CI conditions.

## Native ownership sanitizer

The **Go/C Lifecycle (AddressSanitizer)** job builds the Rust static library
and the cgo test binaries with AddressSanitizer on Linux. Its focused
regressions cover post-`Close` calls plus empty, mixed, deeply nested, large,
and failed multifield construction. LeakSanitizer is enabled so both successful
copy/free cycles and partial conversion failures must release every
Ferric-owned allocation.

Go slices passed to `AssertFact` or `AssertTemplate` remain caller-owned.
The binding borrows their converted elements only while calling
`ferric_value_multifield_copy`; the returned recursive tree is entirely
Ferric-owned and is released through `ferric_value_free`. No Go or C allocator
storage is transferred to Rust's value cleanup.

Run `just ffi-go-asan-harness` on a native Linux host to reproduce this job.
The broader operating-system and clean-consumer binding matrix is tracked
separately by FR-DIST-008.
