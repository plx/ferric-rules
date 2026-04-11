# TypeScript Binding Normative Contract (Revised)

Date: 2026-04-11
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

### 3.3 Worker Boundary
1. Worker transport `MUST` preserve the semantics in 3.1 and 3.2.
2. Canonical symbol wire representation `MUST` be:

```ts
{ __type: "FerricSymbol", value: string }
```

3. Transport layers `MUST` convert to/from this wire representation transparently.
4. Callers of `EngineHandle` and `EnginePool` `MUST NOT` need manual symbol marshalling.

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
2. In-flight cancellation `MUST` use cooperative batched halting with shared abort flag.
3. On in-flight cancellation, promise `MUST` resolve with partial `RunResult` and `haltReason = HaltRequested`.

### 7.2 EnginePool.evaluate
1. If already aborted before dispatch, `MUST` reject with `AbortError`.
2. If aborted while queued and request is not yet dispatched, `MUST` reject with `AbortError`.
3. If aborted during execution, `MUST` use same cooperative batched halting model.

### 7.3 EnginePool.do
1. If already aborted before dispatch, `MUST` reject with `AbortError`.
2. If aborted before callback completes, returned promise `MUST` reject with `AbortError`.
3. Proxy `run` operations issued during `do` `MUST` use cooperative batched halting when cancellation is active.

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

## 10. Required Test Gating

1. Every requirement ID in the Conformance Matrix sections A-E `MUST` have at least one automated test case.
2. All `FAIL` and `UNKNOWN` statuses from the matrix `MUST` be eliminated before declaring implementation complete.
3. Test suite requirements are defined in [Test Specification](/Users/prb/conductor/workspaces/ferric-rules/santo-domingo/docs/typescript-binding-test-spec.md).
