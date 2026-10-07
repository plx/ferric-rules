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
platform bundled Go distribution is deferred.

## API

The package is built around one type, `Engine`, created with `NewEngine`
(optionally `WithSource`, `WithSnapshot`, `WithStrategy`, `WithEncoding`,
`WithMaxCallDepth`) and released with `Close`. Engine methods serialize native
access internally, so a single `Engine` may be used from several goroutines;
for parallel work, create one `Engine` per goroutine. See the package examples
(`example_test.go`) for typical use.

- `Run` and `RunWithLimit` take a `context.Context`. A cancelable context is
  checked between batches of at most 100 rule firings; cancellation returns
  the partial `RunResult` and an error wrapping `ctx.Err()`.
- `RunWithLimit` accepts zero for unlimited work and positive limits. Negative
  limits return a typed `InvalidArgumentError` before running.
- `Step` fires at most one activation and reports whether a rule fired.
- `Serialize` / `WithSnapshot` round-trip an engine. `WithSnapshot` rejects
  nil/empty bytes, combination with `WithSource` (even an empty source), and
  explicit configuration overrides; a restored engine uses its saved
  configuration.
- Most convenience methods that drop errors (for example `Rules`,
  `AgendaSize`, `GetOutput`) have `...E` variants that also report errors
  such as `ErrEngineClosed`.
- Rule-created fact addresses (`?f`, including `<Dummy Fact>` slot defaults
  and addresses inside multifields) have no Go representation; the C ABI
  rejects them. `GetGlobal` and `GetFact` return an error for a value holding
  one, and `Facts` fails as a whole while any fact holds one (`FindFacts` when
  one of its facts does). Use fact IDs and application keys instead.
