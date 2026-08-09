# TypeScript Binding Normative Contract (Revised)

Date: 2026-04-11
Updated: 2026-08-09 (FR-NODE-002 logical-run semantics)
Status: Draft for reimplementation

Companion documents:
- [Architecture](/Users/prb/conductor/workspaces/ferric-rules/santo-domingo/docs/typescript-binding-architecture.md)
- [Conformance Matrix](/Users/prb/conductor/workspaces/ferric-rules/santo-domingo/docs/typescript-binding-conformance-matrix.md)
- [Test Specification](/Users/prb/conductor/workspaces/ferric-rules/santo-domingo/docs/typescript-binding-test-spec.md)

## 1. Normative Language
The keywords `MUST`, `MUST NOT`, `SHOULD`, and `MAY` are normative.

If this contract conflicts with legacy design docs, this contract wins.

## 2. Public Package Contract

### 2.1 Exports
1. Package entrypoint `@ferric-rules/node` `MUST` export concrete runtime values:
   - `Engine` (class)
   - `FerricSymbol` (class)
   - `EngineHandle` (class)
   - `EnginePool` (class)
2. These exports `MUST NOT` be typed as possibly `undefined` in public `.d.ts`.
3. Enums in public package declarations `MUST` be regular TS enums, not `const enum`.

### 2.2 Public Types
1. `ClipsValue` `MUST` include `FerricSymbol` in the public API type union.
2. Wire-only transport types `MUST NOT` replace public API value types.
3. Public API examples from this contract `MUST` compile under `tsc --strict`.
4. The package `MUST` export `FactId = bigint` and
   `FactIdInput = FactId | number`.

## 3. Value Conversion Contract

### 3.1 JS -> CLIPS
1. `FerricSymbol` -> CLIPS Symbol.
2. `string` -> CLIPS String (quoted).
3. `number` -> CLIPS Integer when integral and within `i64`; otherwise Float.
4. `bigint` -> CLIPS Integer.
5. `boolean` -> CLIPS Symbols `TRUE` / `FALSE`.
6. `Array` -> CLIPS Multifield recursively.
7. `null` and `undefined` -> CLIPS Void.

### 3.2 CLIPS -> JS
1. CLIPS Symbol -> `FerricSymbol`.
2. CLIPS String -> `string`.
3. CLIPS Integer in safe range `[-(2^53-1), 2^53-1]` -> `number`.
4. CLIPS Integer outside safe range -> `bigint`.
5. CLIPS Float -> `number`.
6. CLIPS Multifield -> `ClipsValue[]` recursively.
7. CLIPS Void and ExternalAddress -> `null`.

### 3.3 Fact Identifiers
1. Every fact ID returned by `assertString`, `assertFact`, or `assertTemplate`,
   and every `Fact.id` snapshot property, `MUST` be a `FactId` (`bigint`),
   regardless of magnitude.
2. Every API that accepts a fact ID `MUST` accept `FactIdInput` and apply the
   same conversion rules.
3. A `bigint` input `MUST` be accepted only when it is in the unsigned 64-bit
   range.
4. A legacy `number` input `MUST` be accepted only when it is finite,
   integral, non-negative, and no greater than `Number.MAX_SAFE_INTEGER`.
5. Invalid and unsafe numeric inputs `MUST` fail with a targeted argument
   error and `MUST NOT` be rounded or truncated.
6. Fact-ID representation is distinct from the adaptive `number`/`bigint`
   representation for CLIPS integer values and from run counts and limits.

### 3.4 Worker Boundary
1. Worker transport `MUST` preserve the semantics in 3.1, 3.2, and 3.3.
2. Canonical symbol wire representation `MUST` be:

```ts
{ __type: "FerricSymbol", value: string }
```

3. Transport layers `MUST` convert to/from this wire representation transparently.
4. Callers of `EngineHandle` and `EnginePool` `MUST NOT` need manual symbol marshalling.
5. `EngineHandle` and `EnginePool` `MUST` preserve `FactId` values as `bigint`
   through structured clone in both request and response directions.

## 4. API Semantics

### 4.1 Engine
1. `Engine` methods are synchronous and execute on caller thread.
2. `Engine.fromSource(source, options)` `MUST` be equivalent to `new Engine(options); load(source); reset();`.
3. `Engine.close()` `MUST` be idempotent.
4. After `close()`, all operational methods/getters `MUST` throw deterministic errors.
5. `Engine` `MUST` support `[Symbol.dispose](): void`, equivalent to `close()`.

### 4.2 EngineHandle
1. `EngineHandle.create({ source })` `MUST` load and reset source before resolve.
2. `EngineHandle.create({ snapshot })` `MUST` restore from snapshot before resolve.
3. `source` and `snapshot` `MUST` be mutually exclusive; passing both `MUST` reject with argument error.
4. `EngineHandle.close()` `MUST` be idempotent.
5. `EngineHandle` `MUST` support `[Symbol.asyncDispose]()` and delegate to `close()`.

### 4.3 EnginePool
1. `EnginePool.create(..., { threads })` `MUST` default to `threads = 1` when omitted.
2. `evaluate(spec, req)` `MUST` perform: `reset -> assert facts -> run -> collect facts/output`.
3. `EnginePool.close()` `MUST`:
   - reject new requests after close starts,
   - allow already-dispatched requests to settle,
   - then terminate workers.
4. `EnginePool.close()` `MUST` be idempotent.
5. `EnginePool` `MUST` support `[Symbol.asyncDispose]()` and delegate to `close()`.

## 5. Run Limit Semantics

1. For `Engine.run(limit?)` and `EngineHandle.run({limit})`:
   - omitted or `undefined` => unlimited,
   - `0` => zero firings,
   - positive integer => max firings.
2. For `EvaluateRequest.limit` in `EnginePool.evaluate`:
   - omitted or `0` => unlimited,
   - positive integer => max firings.
3. These semantics `MUST` be documented and tested explicitly.
4. Every uncanceled public run invocation `MUST` begin with one fresh native
   run. Starting a fresh run clears the previous halt request and action
   diagnostics, but does not otherwise clear working memory or the agenda.
5. `EngineHandle.run({ limit: 0 })` and `EngineProxy.run({ limit: 0 })` `MUST`
   invoke a fresh native zero-limit run rather than synthesize a result. They
   `MUST` fire zero rules, return `LimitReached`, and perform the same fresh-run
   execution-state reset as `Engine.run(0)`.

### 5.1 Worker Logical-Run Batching

1. A worker-backed run `MAY` split one public run into bounded chunks to poll
   for host cancellation.
2. The first executed chunk `MUST` start the fresh logical run. Every later
   chunk in that invocation `MUST` continue that same logical run and `MUST NOT`
   clear its halt request or accumulated action diagnostics.
3. Per-chunk fired counts `MUST` be accumulated into the public run total.
4. Absent host cancellation, synchronous, `EngineHandle`, and `EnginePool`
   execution `MUST` agree in total fired count, halt reason, halted state,
   agenda state, and action diagnostics.
5. If a count-limited chunk's last activation requests a halt, the worker
   `MUST` observe the pending halt before submitting another continuation; it
   `MUST NOT` fire a post-halt activation.
6. After a native chunk returns a terminal reason, that reason `MUST` be
   returned immediately. After `LimitReached`, the worker `MUST` apply this
   precedence before continuing:
   1. an exhausted caller limit returns `LimitReached`, matching synchronous
      execution even if the limit-th activation also set the halt latch;
   2. a host abort stops the logical run under section 7;
   3. a pending native halt returns `HaltRequested`;
   4. otherwise, the worker submits a continuation chunk.
7. A later public `run()` invocation `MUST` start fresh, never continue a prior
   completed or host-canceled invocation.
8. Logical-run continuation is an internal implementation mechanism and
   `MUST NOT` add or change a public method, result type, enum member, or
   versioned API contract.

## 6. Error Contract

### 6.1 Error Classes
The following classes `MUST` exist and be constructible in JS:
- `FerricError`
- `FerricParseError`
- `FerricCompileError`
- `FerricRuntimeError`
- `FerricFactNotFoundError`
- `FerricTemplateNotFoundError`
- `FerricSlotNotFoundError`
- `FerricModuleNotFoundError`
- `FerricEncodingError`
- `FerricSerializationError`

### 6.2 Mapping Rules
1. Native failures `MUST` map to the correct class above.
2. Worker responses `MUST` include stable payload:

```ts
{
  id: number;
  error?: {
    name: string;   // class name above or AbortError
    code: string;   // stable machine code
    message: string;
  }
}
```

3. Worker-side reconstruction `MUST` instantiate the class identified by `name`.
4. Unknown error names `MUST` degrade to `FerricError` while preserving `code` and `message`.

## 7. Cancellation Contract

### 7.1 EngineHandle.run
1. If signal is already aborted before dispatch, `MUST` reject with `AbortError`.
2. In-flight cancellation `MUST` use cooperative logical-run batching with a
   shared, out-of-band abort flag.
3. On in-flight cancellation, the worker `MUST` stop submitting continuation
   chunks and `MUST NOT` call native `halt()` or otherwise set the engine halt
   latch merely to represent host cancellation.
4. The promise `MUST` preserve the existing public projection by resolving
   with a partial `RunResult` and `haltReason = HaltRequested`.
5. Host cancellation does not guarantee an unhalted engine: a completed chunk
   may itself have executed a rule-side halt. The next public run `MUST` start
   fresh and clear the documented execution state.

### 7.2 EnginePool.evaluate
1. If already aborted before dispatch, `MUST` reject with `AbortError`.
2. If aborted while queued and request is not yet dispatched, `MUST` reject with `AbortError`.
3. If aborted during execution, `MUST` use the same cooperative logical-run
   batching and out-of-band cancellation model as `EngineHandle.run`.
4. A partial `EvaluateResult.runResult` produced by in-flight host cancellation
   `MUST` retain the existing `HaltRequested` projection without setting the
   native halt latch merely to represent that cancellation.

### 7.3 EnginePool.do
1. If already aborted before dispatch, `MUST` reject with `AbortError`.
2. If aborted before callback completes, returned promise `MUST` reject with `AbortError`.
3. Proxy `run` operations issued during `do` `MUST` use the same fresh-run and
   continuation contract when cancellation is active.
4. Rejecting the outer `do` promise for host cancellation `MUST NOT` require
   setting the proxied native engine's halt latch.

## 8. Worker Protocol Contract

1. Main->worker request shape `MUST` be:

```ts
interface WorkerRequest {
  id: number;
  method: string;
  args: unknown[];
}
```

2. Worker->main response shape `MUST` be:

```ts
interface WorkerResponse {
  id: number;
  result?: unknown;
  error?: {
    name: string;
    code: string;
    message: string;
  };
}
```

3. Request IDs `MUST` be unique per worker slot among in-flight requests.
4. Snapshot payload transfers `SHOULD` use `ArrayBuffer` transfer for zero-copy.

## 9. Packaging and Runtime Load Contract

1. Missing native binaries at runtime `MUST` fail fast for value imports (`Engine`, `FerricSymbol`), not expose optional undefined runtime API.
2. Package `npm test` `MUST` execute non-zero binding tests.
3. CI `MUST` fail if binding test count is zero.
4. The main package `MUST` ship its platform-neutral native loader and declare
   every native payload package as an optional dependency at the exact same
   version as the main package.
5. Every native addon `MUST` embed its Ferric package version. The loader
   `MUST` reject mismatched main-package metadata, native-package metadata, or
   embedded addon versions before exposing an engine.
6. Unsupported targets `MUST` fail with an actionable error that names the
   detected OS and architecture, the detected libc on Linux, and the supported
   target alternatives. A missing optional package `MUST` name the full
   detected target and exact expected package.
7. CI `MUST` pack, install, and execute the exact main and native tarballs from
   a clean consumer directory without a source checkout or native build.
8. The declared target set is the versioned `native/targets.json` file.
   Extending the platform, architecture, or libc matrix requires the
   validation in this contract.
9. Every declared Linux target `MUST` name exactly one npm libc selector. The
   loader and release-artifact smoke `MUST` distinguish glibc from musl and
   fail before loading a native package when libc detection is inconclusive.
10. CI `MUST` build, pack, install, load, and run the exact artifact for every
    declared OS, architecture, and libc combination on a matching runtime.

## 10. Required Test Gating

1. Every requirement ID in the Conformance Matrix sections A-E `MUST` have at least one automated test case.
2. All `FAIL` and `UNKNOWN` statuses from the matrix `MUST` be eliminated before declaring implementation complete.
3. Test suite requirements are defined in [Test Specification](/Users/prb/conductor/workspaces/ferric-rules/santo-domingo/docs/typescript-binding-test-spec.md).
