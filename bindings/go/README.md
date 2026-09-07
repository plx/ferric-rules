# Go source build

The module path is `github.com/plx/ferric-rules/bindings/go`. Earlier source
snapshots used the nonexistent `github.com/prb/ferric-rules/bindings/go` path;
update imports and local `replace` directives when upgrading.

Build from a checkout of the same Ferric revision with Rust, Go (the version
in `go.mod`), a C compiler, and `just` installed:

```sh
just build-go-ffi
just test-go-race
```

This builds the native static library for the current host and copies it beside
the checked C header. A Go consumer can then use a local module replacement:

```sh
go mod edit -replace github.com/plx/ferric-rules/bindings/go=/absolute/path/to/ferric-rules/bindings/go
go get github.com/plx/ferric-rules/bindings/go
```

The static library must match the source/header revision and target. Cross
platform bundled Go distribution and expanded feature parity are deferred.
Existing `Engine`, `PinnedEngine`, `Manager`, and `Coordinator` APIs remain
available; their cancellation and worker behavior is described in Go API docs.

## Input and shutdown changes

`RunWithLimit` and `EvaluateRequest.Limit` accept zero for unlimited work and
positive limits. Negative limits now return a typed `InvalidArgumentError`
before running or queuing work.

`WithSnapshot` rejects nil/empty bytes, combination with `WithSource` (even an
empty source), and explicit configuration overrides. A restored engine uses
its saved configuration. Supply only the snapshot and its matching format.
Every concurrent `Coordinator.Close` call waits for admitted work and worker
cleanup to finish; shutdown remains idempotent. Call coordinator shutdown
outside its own `Manager.Do` callbacks, since it waits for those callbacks.
