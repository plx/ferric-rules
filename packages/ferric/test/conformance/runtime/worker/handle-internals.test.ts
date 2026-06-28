/**
 * EngineHandle defensive-branch tests with a fake Worker.
 *
 * These target error reconstruction and pending-request cleanup paths that are
 * difficult to force through a real Worker without races.
 */
import { EventEmitter } from "node:events";
import { test } from "node:test";
import * as assert from "node:assert/strict";

import {
  EngineHandle,
  FerricError,
} from "../../../helpers/ferric";

class FakeWorker extends EventEmitter {
  readonly messages: any[] = [];
  terminateCalls = 0;

  postMessage(message: any): void {
    this.messages.push(message);
  }

  terminate(): Promise<number> {
    this.terminateCalls += 1;
    return Promise.resolve(0);
  }
}

function makeHandle(): { handle: EngineHandle; worker: FakeWorker } {
  const worker = new FakeWorker();
  const handle = new (EngineHandle as any)(worker) as EngineHandle;
  return { handle, worker };
}

// ---------------------------------------------------------------------------
// C-004 table-driven reconstruction for special worker error payloads
// ---------------------------------------------------------------------------
test("C-004 table-driven EngineHandle reconstructs special worker errors", async () => {
  const cases = [
    {
      payload: { name: "AbortError", message: "aborted", code: "ABORT_ERR" },
      verify: (err: any) => {
        assert.ok(err instanceof DOMException);
        assert.strictEqual(err.name, "AbortError");
      },
    },
    {
      payload: { name: "TypeError", message: "bad type", code: "ERR_TYPE" },
      verify: (err: any) => assert.ok(err instanceof TypeError),
    },
    {
      payload: { name: "UnknownWorkerError", message: "custom", code: "CUSTOM" },
      verify: (err: any) => {
        assert.ok(err instanceof FerricError);
        assert.strictEqual(err.name, "UnknownWorkerError");
        assert.strictEqual(err.code, "CUSTOM");
      },
    },
  ];

  // Generated payloads cover the same property for every non-registry branch:
  // the public promise rejects with an Error instance preserving name/message.
  for (const item of cases) {
    const { handle, worker } = makeHandle();
    const pending = handle.facts();
    worker.emit("message", {
      id: worker.messages[0].id,
      error: item.payload,
    });

    await assert.rejects(pending, (err: any) => {
      item.verify(err);
      assert.strictEqual(err.message, item.payload.message);
      return true;
    });
  }
});

// ---------------------------------------------------------------------------
// C-004 manual reconstruction: malformed worker error payloads are explicit
// ---------------------------------------------------------------------------
test("C-004 EngineHandle reconstructs missing worker error payloads", async () => {
  const { handle, worker } = makeHandle();
  const pending = handle.facts();

  // The protocol treats a present error property as an error frame even when a
  // worker bug omitted the payload; callers should reject rather than resolve.
  worker.emit("message", {
    id: worker.messages[0].id,
    error: undefined,
  });

  await assert.rejects(pending, /Unknown worker error/);
});

// ---------------------------------------------------------------------------
// D-001 manual protocol guard: stray worker replies are ignored
// ---------------------------------------------------------------------------
test("D-001 EngineHandle ignores replies for unknown request ids", () => {
  const { worker } = makeHandle();

  // A late response for an already-settled request must not disturb the
  // pending map or throw synchronously on the event handler.
  assert.doesNotThrow(() => {
    worker.emit("message", { id: 999, result: "late" });
  });
});

// ---------------------------------------------------------------------------
// F-003 manual cleanup: Worker error rejects pending EngineHandle requests
// ---------------------------------------------------------------------------
test("F-003 EngineHandle rejects pending requests when worker emits error", async () => {
  const { handle, worker } = makeHandle();
  const pending = handle.facts();

  // A Worker-level error has no request id; every pending request must reject
  // so callers cannot hang indefinitely.
  const failure = new Error("worker exploded");
  worker.emit("error", failure);

  await assert.rejects(pending, /worker exploded/);
});

// ---------------------------------------------------------------------------
// F-003 table-driven cleanup: Worker exit rejects pending requests
// ---------------------------------------------------------------------------
test("F-003 table-driven EngineHandle rejects pending requests on worker exit", async () => {
  for (const [code, pattern] of [
    [0, /exited before responding/],
    [7, /unexpectedly with code 7/],
  ] as const) {
    const { handle, worker } = makeHandle();
    const pending = handle.facts();
    worker.emit("exit", code);
    await assert.rejects(pending, pattern);
  }
});

// ---------------------------------------------------------------------------
// F-003 manual cleanup: close rejects pending and later calls use call() guard
// ---------------------------------------------------------------------------
test("F-003 EngineHandle close rejects pending requests and guards later calls", async () => {
  const { handle, worker } = makeHandle();
  const pending = handle.facts();

  await handle.close();
  await assert.rejects(pending, /EngineHandle closed/);
  assert.strictEqual(worker.terminateCalls, 1);

  // Calling a normal method after close exercises the shared call() guard,
  // distinct from run()'s explicit pre-check.
  await assert.rejects(() => handle.facts(), /EngineHandle has been closed/);
  await assert.rejects(() => handle.run(), /EngineHandle has been closed/);
});

// ---------------------------------------------------------------------------
// D-007 manual serialize fallback: Buffer results are returned unchanged
// ---------------------------------------------------------------------------
test("D-007 EngineHandle.serialize returns Buffer results unchanged", async () => {
  const { handle } = makeHandle();
  const expected = Buffer.from("snapshot");
  (handle as any).call = async () => expected;

  // Real workers transfer ArrayBuffers, but the method also supports a Buffer
  // result for compatibility with direct/mocked transports.
  assert.strictEqual(await handle.serialize(), expected);
});

test("D-007 EngineHandle.serialize converts ArrayBuffer results to Buffer", async () => {
  const { handle } = makeHandle();
  const source = Uint8Array.from([1, 2, 3, 4]).buffer;
  (handle as any).call = async () => source;

  // Real worker transports use this zero-copy ArrayBuffer branch.
  const actual = await handle.serialize();
  assert.ok(Buffer.isBuffer(actual));
  assert.deepStrictEqual([...actual], [1, 2, 3, 4]);
});
