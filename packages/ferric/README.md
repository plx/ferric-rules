# @ferric-rules/node

Ferric's Node 22+ bindings provide a direct synchronous `Engine`, an asynchronous
`EngineHandle` using one worker, and an `EnginePool` for parallel evaluation.
The package supports CommonJS require and ESM named/dynamic import.

```typescript
import { EngineHandle, FerricSymbol } from "@ferric-rules/node";

const engine = await EngineHandle.create({
  source: "(defrule choose (candidate ?name) => (assert (selected ?name)))",
});
await engine.assertFact("candidate", new FerricSymbol("welcome"));
const result = await engine.run({ limit: 100 });
const facts = await engine.facts();
const snapshot = await engine.serialize(); // recommended CBOR
await engine.close();
const restored = await EngineHandle.create({ snapshot: { data: snapshot } });
await restored.close();
```

Use the worker API for potentially long load/run/snapshot work: direct `Engine`
methods block the calling event loop. Worker run cancellation is cooperative
between bounded batches and preserves the existing partial-result contract.
Handle close terminates its worker and rejects pending work; pool close drains
admitted work before terminating workers. Concurrent close calls share one
completion barrier, including cleanup failures.

Plain strings are CLIPS strings; `FerricSymbol` or canonical wire symbols are
CLIPS symbols. Arrays are multifields; integers beyond the JavaScript safe
integer range must use signed 64-bit `bigint`. Unsafe integral numbers are
rejected, including values formerly guessed as floats. Run limits and counts
are safe-integer numbers. Raw fact IDs are opaque bigint handles belonging
to their engine; persist application keys and query fresh IDs after restore.
Stored null/void values, including nested ones, are rejected. Use a `FerricSymbol("nil")` for the CLIPS symbol. Multifields allow at most 32 nested levels and one million values per assertion. External addresses are rejected instead of silently becoming null.

Configuration and format selectors reject fractional/out-of-range numbers.
`maxCallDepth` accepts integers from zero through 4294967295; zero disallows
user-function calls. The requested default is 64, while evaluation applies an
effective ceiling of 32 user calls even when a higher limit is requested.

Static factories always return `Engine`, including calls through subclasses.

Pre-1.0 migration: the runtime minimum is now Node 22; implicit snapshots now use
CBOR. Other explicitly selected formats remain experimental and obey the native
snapshot version policy. Incompatible legacy raw snapshots are rejected rather
than erased or silently migrated. Syntax/interpretation failures use
`FerricParseError`, unsupported/invalid constructs use `FerricCompileError`,
file failures use `FerricIOError`, and action/runtime failures preserve their
runtime diagnostics. Native error conversion retains the original cause.

Repository `just node-package-smoke` creates real tarballs and verifies them in
an offline temporary consumer, including CJS/ESM/type resolution and the shared
launch-selection plus snapshot/resume example. Local artifacts are sufficient;
this project does not require public npm publication for embedding validation.

`FerricStringBytes`, `FerricSymbolBytes`, and `FerricInstanceName` accept and copy a `Uint8Array`. Their `.bytes` getter returns a copy; `.value` and `toString()` check UTF-8 and throw on invalid text. Native, worker, and pool fact transport preserves CLIPS types and bytes. Returned UTF-8 STRING/SYMBOL values keep the existing string/FerricSymbol representation; invalid UTF-8 uses the explicit byte wrappers. `getOutputBytes(channel)` retrieves exact output through `Engine`, `EngineHandle`, and pool proxies. Text `getOutput` remains checked. Pool evaluation returns every captured byte sequence in `outputBytes`, while `output` contains only valid UTF-8 text. Raw fact data and output survive all supported snapshot codecs.

Instance-name values retain their type and bytes without creating COOL objects. Use `instance-namep` or the binding's distinct value type for classification. `type` and restricted generic dispatch on a missing instance report an action error and stop later actions. The runtime conversions accept either symbol or name values; CLIPS's additional static restriction on certain literal conversion calls is not enforced yet.
