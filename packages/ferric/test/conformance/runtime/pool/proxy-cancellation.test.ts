/**
 * EnginePool callback-proxy cancellation tests (FR-NODE-009 / E-016).
 *
 * Cancellation closes proxy admission immediately. Operations invoked before
 * that boundary remain accepted and drain before the lease is released; later
 * invocations reject without allocating or posting a Worker request.
 */
import { EventEmitter, getEventListeners } from "node:events";
import { setImmediate as yieldImmediate } from "node:timers/promises";
import { test } from "node:test";
import * as assert from "node:assert/strict";

import {
  EnginePool,
  HaltReason,
  type EngineProxy,
} from "../../../helpers/ferric";

interface Deferred {
  promise: Promise<void>;
  resolve: () => void;
}

function deferred(): Deferred {
  let resolve!: () => void;
  const promise = new Promise<void>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

function assertAbortError(error: unknown): boolean {
  assert.ok(error instanceof DOMException);
  assert.strictEqual(error.name, "AbortError");
  assert.strictEqual(error.message, "The operation was aborted");
  return true;
}

function assertInactiveProxyError(error: unknown): boolean {
  assert.ok(error instanceof Error);
  assert.strictEqual(
    error.message,
    "EngineProxy is no longer valid outside its EnginePool.do callback",
  );
  return true;
}

interface PostedMessage {
  id: number;
  method: string;
  args: unknown[];
}

class ControlledWorker extends EventEmitter {
  readonly messages: PostedMessage[] = [];
  readonly postActions: Array<() => void> = [];
  terminateCalls = 0;

  postMessage(message: PostedMessage): void {
    this.messages.push(message);
    this.postActions.shift()?.();
  }

  terminate(): Promise<number> {
    this.terminateCalls += 1;
    return Promise.resolve(0);
  }
}

function makePool(): {
  pool: EnginePool;
  worker: ControlledWorker;
  slot: any;
} {
  const worker = new ControlledWorker();
  const slot = (EnginePool as any).createSlot(worker);
  const pool = new (EnginePool as any)([slot]) as EnginePool;
  return { pool, worker, slot };
}

async function rejectionOf<T>(promise: Promise<T>): Promise<unknown> {
  try {
    await promise;
  } catch (error) {
    return error;
  }
  throw new Error("Expected Promise to reject");
}

async function waitFor(
  predicate: () => boolean,
  label: string,
): Promise<void> {
  for (let turn = 0; turn < 40; turn++) {
    if (predicate()) return;
    await yieldImmediate();
  }
  assert.fail(`Timed out waiting for ${label}`);
}

function instrumentLeaseRelease(lease: any): () => number {
  let releases = 0;
  const resolveReleased = lease.resolveReleased as () => void;
  lease.resolveReleased = () => {
    releases += 1;
    resolveReleased();
  };
  return () => releases;
}

function assertReleasedLease(lease: any, releases: number): void {
  assert.strictEqual(releases, 1, "the callback lease released more than once");
  assert.strictEqual(lease.active, false);
  assert.strictEqual(lease.released, true);
  assert.strictEqual(lease.pendingCalls, 0);
  assert.strictEqual(lease.queue.length, 0);
  assert.strictEqual(lease.drainWaiters.length, 0);
}

function responseFor(message: PostedMessage): unknown {
  switch (message.method) {
    case "assertFact":
      return 1n;
    case "assertString":
      return [1n];
    case "facts":
    case "findFacts":
      return [];
    case "getFact":
    case "step":
    case "getOutput":
      return null;
    case "__batched_run":
      return { rulesFired: 0, haltReason: HaltReason.AgendaEmpty, errors: [] };
    case "__evaluate":
      return {
        runResult: {
          rulesFired: 0,
          haltReason: HaltReason.AgendaEmpty,
          errors: [],
        },
        facts: [],
        output: {},
      };
    default:
      return undefined;
  }
}

async function driveWorkerUntil(
  worker: ControlledWorker,
  isDone: () => boolean,
): Promise<void> {
  let responded = 0;
  for (let turn = 0; turn < 80; turn++) {
    while (responded < worker.messages.length) {
      const message = worker.messages[responded++];
      worker.emit("message", { id: message.id, result: responseFor(message) });
    }
    if (isDone()) return;
    await yieldImmediate();
  }
  assert.fail("Timed out driving deterministic fake-worker responses");
}

test("E-016 abort invalidates a retained callback proxy before post-abort mutation", async () => {
  const pool = await EnginePool.create(
    [{ name: "rules", source: "" }],
    { threads: 1 },
  );
  const controller = new AbortController();
  const callbackEntered = deferred();
  const resumeCallback = deferred();
  const callbackFinished = deferred();
  let retained!: EngineProxy;
  let postAbortOutcome:
    | { status: "fulfilled"; factId: bigint }
    | { status: "rejected"; error: unknown }
    | undefined;
  let active: Promise<unknown> | undefined;

  try {
    active = pool.do(
      "rules",
      async (proxy) => {
        retained = proxy;
        callbackEntered.resolve();
        await resumeCallback.promise;
        try {
          const factId = await retained.assertFact("post-abort", 1);
          postAbortOutcome = { status: "fulfilled", factId };
        } catch (error) {
          postAbortOutcome = { status: "rejected", error };
        } finally {
          callbackFinished.resolve();
        }
      },
      { signal: controller.signal },
    );

    await callbackEntered.promise;
    controller.abort();
    await assert.rejects(active, assertAbortError);

    resumeCallback.resolve();
    await callbackFinished.promise;

    assert.strictEqual(postAbortOutcome?.status, "rejected");
    if (postAbortOutcome?.status === "rejected") {
      assertAbortError(postAbortOutcome.error);
    }

    const observed = await pool.do("rules", async (proxy) =>
      proxy.findFacts("post-abort"),
    );
    assert.strictEqual(
      observed.length,
      0,
      "a retained proxy mutated engine state after outer cancellation",
    );
  } finally {
    resumeCallback.resolve();
    await callbackFinished.promise.catch(() => undefined);
    if (active) await active.catch(() => undefined);
    await pool.close();
  }
});

test("E-016 abort before callback covers preflight queue and admission without leaks", async () => {
  const { pool, worker, slot } = makePool();
  const pending: Promise<unknown>[] = [];

  try {
    const preflight = new AbortController();
    const preflightBaseline = getEventListeners(preflight.signal, "abort").length;
    let preflightEntered = false;
    preflight.abort();
    await assert.rejects(
      pool.do("rules", async () => {
        preflightEntered = true;
      }, { signal: preflight.signal }),
      assertAbortError,
    );
    assert.strictEqual(preflightEntered, false);
    assert.strictEqual(slot.nextId, 0);
    assert.strictEqual(slot.activeLease, undefined);
    assert.strictEqual(slot.queue.length, 0);
    assert.strictEqual(
      getEventListeners(preflight.signal, "abort").length,
      preflightBaseline,
    );

    const blocker = pool.evaluate("rules", {});
    pending.push(blocker);
    assert.strictEqual(worker.messages.length, 1);

    const queuedAbort = new AbortController();
    const queuedBaseline = getEventListeners(queuedAbort.signal, "abort").length;
    let queuedEntered = false;
    const queued = pool.do("rules", async () => {
      queuedEntered = true;
    }, { signal: queuedAbort.signal });
    pending.push(queued);
    assert.strictEqual(slot.queue.length, 1);
    const queuedLease = slot.queue[0].lease;
    const queuedReleaseCount = instrumentLeaseRelease(queuedLease);

    queuedAbort.abort();
    await assert.rejects(queued, assertAbortError);
    await queuedLease.releasedPromise;
    assert.strictEqual(queuedEntered, false);
    assertReleasedLease(queuedLease, queuedReleaseCount());
    assert.strictEqual(slot.queue.length, 0);
    assert.strictEqual(
      getEventListeners(queuedAbort.signal, "abort").length,
      queuedBaseline,
    );

    const blockerMessage = worker.messages[0];
    worker.emit("message", {
      id: blockerMessage.id,
      result: responseFor(blockerMessage),
    });
    assert.deepStrictEqual(await blocker, {
      runResult: {
        rulesFired: 0,
        haltReason: HaltReason.AgendaEmpty,
        errors: [],
      },
      facts: [],
      output: {},
    });

    const admissionAbort = new AbortController();
    const admissionBaseline = getEventListeners(
      admissionAbort.signal,
      "abort",
    ).length;
    let admissionEntered = false;
    const admitted = pool.do("rules", async () => {
      admissionEntered = true;
    }, { signal: admissionAbort.signal });
    pending.push(admitted);
    const admittedLease = slot.activeLease;
    assert.ok(admittedLease);
    const admittedReleaseCount = instrumentLeaseRelease(admittedLease);

    admissionAbort.abort();
    await assert.rejects(admitted, assertAbortError);
    await admittedLease.releasedPromise;
    assert.strictEqual(admissionEntered, false);
    assertReleasedLease(admittedLease, admittedReleaseCount());
    assert.strictEqual(slot.activeLease, undefined);
    assert.strictEqual(slot.queue.length, 0);
    assert.strictEqual(slot.pending.size, 0);
    assert.strictEqual(slot.inflight, 0);
    assert.strictEqual(
      getEventListeners(admissionAbort.signal, "abort").length,
      admissionBaseline,
    );
  } finally {
    for (const message of worker.messages) {
      if (slot.pending.has(message.id)) {
        worker.emit("message", { id: message.id, result: responseFor(message) });
      }
    }
    await Promise.allSettled(pending);
    await pool.close();
  }
});

test("E-016 pre-aborted do does not select a worker or advance round robin", async () => {
  const workers = [new ControlledWorker(), new ControlledWorker()];
  const slots = workers.map((worker) => (EnginePool as any).createSlot(worker));
  const pool = new (EnginePool as any)(slots) as EnginePool;
  const controller = new AbortController();
  const listenerBaseline = getEventListeners(controller.signal, "abort").length;
  const callbackEntered = deferred();
  const resumeCallback = deferred();
  let selectedSlot: any;
  let valid: Promise<unknown> | undefined;

  try {
    // A two-slot non-zero sentinel makes an accidental pick observable: one
    // pick would wrap this counter to zero even if no request were posted.
    (pool as any).roundRobin = 1;
    controller.abort();
    await assert.rejects(
      pool.do("rules", async () => {
        assert.fail("a pre-aborted callback entered");
      }, { signal: controller.signal }),
      assertAbortError,
    );

    assert.strictEqual((pool as any).roundRobin, 1);
    assert.deepStrictEqual(
      slots.map((slot) => ({
        activeLease: slot.activeLease,
        queueLength: slot.queue.length,
        nextId: slot.nextId,
      })),
      [
        { activeLease: undefined, queueLength: 0, nextId: 0 },
        { activeLease: undefined, queueLength: 0, nextId: 0 },
      ],
    );
    assert.deepStrictEqual(workers.map((worker) => worker.messages.length), [0, 0]);
    assert.strictEqual(
      getEventListeners(controller.signal, "abort").length,
      listenerBaseline,
    );

    // The next valid admission must still select the sentinel slot. This
    // distinguishes a preserved selection cursor from a wrapped one.
    valid = pool.do("rules", async () => {
      selectedSlot = slots.find((slot) => slot.activeLease !== undefined);
      callbackEntered.resolve();
      await resumeCallback.promise;
      return "valid callback";
    });
    await callbackEntered.promise;
    assert.strictEqual(selectedSlot, slots[1]);
    assert.strictEqual((pool as any).roundRobin, 0);
    resumeCallback.resolve();
    assert.strictEqual(await valid, "valid callback");
  } finally {
    resumeCallback.resolve();
    if (valid) await valid.catch(() => undefined);
    await pool.close();
  }
});

test("E-016 abort between awaits rejects before request bookkeeping and releases once", async () => {
  const { pool, worker, slot } = makePool();
  const controller = new AbortController();
  const listenerBaseline = getEventListeners(controller.signal, "abort").length;
  const callbackEntered = deferred();
  const resumeCallback = deferred();
  const callbackFinished = deferred();
  let retained!: EngineProxy;
  let postAbortOutcome:
    | { status: "fulfilled"; value: unknown }
    | { status: "rejected"; error: unknown }
    | undefined;
  let outer: Promise<unknown> | undefined;
  let lease: any;

  try {
    outer = pool.do("rules", async (proxy) => {
      retained = proxy;
      callbackEntered.resolve();
      await resumeCallback.promise;
      try {
        postAbortOutcome = {
          status: "fulfilled",
          value: await retained.facts(),
        };
      } catch (error) {
        postAbortOutcome = { status: "rejected", error };
      } finally {
        callbackFinished.resolve();
      }
    }, { signal: controller.signal });
    await callbackEntered.promise;
    lease = slot.activeLease;
    assert.ok(lease);
    const releaseCount = instrumentLeaseRelease(lease);
    await waitFor(
      () => getEventListeners(controller.signal, "abort").length > listenerBaseline,
      "outer do abort listener",
    );

    const beforeCancelledCall = {
      messages: worker.messages.length,
      nextId: slot.nextId,
      pending: slot.pending.size,
      inflight: slot.inflight,
      ownerQueue: lease.queue.length,
    };
    controller.abort();
    await assert.rejects(outer, assertAbortError);
    assert.strictEqual(slot.activeLease, lease, "abort released a running callback");

    resumeCallback.resolve();
    await waitFor(
      () => postAbortOutcome !== undefined || worker.messages.length > 0,
      "post-abort proxy outcome",
    );
    for (const message of worker.messages) {
      if (slot.pending.has(message.id)) {
        worker.emit("message", { id: message.id, result: responseFor(message) });
      }
    }
    await callbackFinished.promise;
    await lease.releasedPromise;

    assert.strictEqual(postAbortOutcome?.status, "rejected");
    if (postAbortOutcome?.status === "rejected") {
      assertAbortError(postAbortOutcome.error);
    }
    assert.deepStrictEqual(
      {
        messages: worker.messages.length,
        nextId: slot.nextId,
        pending: slot.pending.size,
        inflight: slot.inflight,
        ownerQueue: lease.queue.length,
      },
      beforeCancelledCall,
      "a post-abort invocation reached request bookkeeping",
    );
    assertReleasedLease(lease, releaseCount());
    assert.strictEqual(slot.activeLease, undefined);
    assert.strictEqual(
      getEventListeners(controller.signal, "abort").length,
      listenerBaseline,
    );

    await assert.rejects(() => retained.facts(), assertInactiveProxyError);
  } finally {
    resumeCallback.resolve();
    for (const message of worker.messages) {
      if (slot.pending.has(message.id)) {
        worker.emit("message", { id: message.id, result: responseFor(message) });
      }
    }
    await callbackFinished.promise.catch(() => undefined);
    if (outer) await outer.catch(() => undefined);
    await pool.close();
  }
});

test("E-016 every proxy method rejects AbortError while an aborted callback is active", async () => {
  const { pool, worker, slot } = makePool();
  const controller = new AbortController();
  const listenerBaseline = getEventListeners(controller.signal, "abort").length;
  const callbackEntered = deferred();
  const resumeCallback = deferred();
  let retained!: EngineProxy;
  let outer: Promise<unknown> | undefined;
  let lease: any;

  try {
    outer = pool.do("rules", async (proxy) => {
      retained = proxy;
      callbackEntered.resolve();
      await resumeCallback.promise;
    }, { signal: controller.signal });
    await callbackEntered.promise;
    lease = slot.activeLease;
    assert.ok(lease);
    const releaseCount = instrumentLeaseRelease(lease);

    controller.abort();
    await assert.rejects(outer, assertAbortError);

    const beforeCalls = {
      messages: worker.messages.length,
      nextId: slot.nextId,
      pending: slot.pending.size,
      inflight: slot.inflight,
      ownerQueue: lease.queue.length,
    };
    const calls: Array<[string, () => Promise<unknown>]> = [
      ["load", () => retained.load("(defrule blocked (never) =>)")],
      ["assertString", () => retained.assertString("(blocked-string 1)")],
      ["assertFact", () => retained.assertFact("blocked-fact", 1)],
      ["assertTemplate", () => retained.assertTemplate("blocked", { value: 1 })],
      ["retract", () => retained.retract(1n)],
      ["getFact", () => retained.getFact(1n)],
      ["facts", () => retained.facts()],
      ["findFacts", () => retained.findFacts("blocked-fact")],
      // Cancellation must win before method-specific validation.
      ["run", () => retained.run({ limit: Number.NaN })],
      ["step", () => retained.step()],
      ["halt", () => retained.halt()],
      ["reset", () => retained.reset()],
      ["clear", () => retained.clear()],
      ["getOutput", () => retained.getOutput("t")],
      ["clearOutput", () => retained.clearOutput("t")],
      ["pushInput", () => retained.pushInput("blocked")],
    ];

    let outcomes: PromiseSettledResult<unknown>[] | undefined;
    void Promise.allSettled(calls.map(([, call]) => call())).then((settled) => {
      outcomes = settled;
    });
    await driveWorkerUntil(worker, () => outcomes !== undefined);
    assert.ok(outcomes);
    for (let index = 0; index < calls.length; index++) {
      const [method] = calls[index];
      const outcome: PromiseSettledResult<unknown> = outcomes[index];
      assert.strictEqual(outcome.status, "rejected", method);
      if (outcome.status === "rejected") {
        assertAbortError(outcome.reason);
      }
    }
    assert.deepStrictEqual(
      {
        messages: worker.messages.length,
        nextId: slot.nextId,
        pending: slot.pending.size,
        inflight: slot.inflight,
        ownerQueue: lease.queue.length,
      },
      beforeCalls,
      "an aborted callback proxy allocated or dispatched a request",
    );
    assert.strictEqual(
      getEventListeners(controller.signal, "abort").length,
      listenerBaseline,
    );

    resumeCallback.resolve();
    await lease.releasedPromise;
    assertReleasedLease(lease, releaseCount());
    await assert.rejects(() => retained.facts(), assertInactiveProxyError);
  } finally {
    resumeCallback.resolve();
    for (const message of worker.messages) {
      if (slot.pending.has(message.id)) {
        worker.emit("message", { id: message.id, result: responseFor(message) });
      }
    }
    if (outer) await outer.catch(() => undefined);
    await pool.close();
  }
});

test("E-016 accepted immediate and lease-queued mutations drain and apply after abort", async () => {
  const { pool, worker, slot } = makePool();
  const controller = new AbortController();
  const listenerBaseline = getEventListeners(controller.signal, "abort").length;
  const acceptedIssued = deferred();
  let callbackFinished = false;
  let acceptedOutcomes: PromiseSettledResult<unknown>[] | undefined;
  let postAbortOutcome:
    | { status: "fulfilled"; value: unknown }
    | { status: "rejected"; error: unknown }
    | undefined;
  let outer: Promise<unknown> | undefined;
  let followUp: Promise<unknown> | undefined;
  let lease: any;
  const facts: Array<{ id: bigint; relation: string; fields: unknown[] }> = [];
  let nextFactId = 1n;
  let responded = 0;

  const respondAvailable = (): void => {
    while (responded < worker.messages.length) {
      const message = worker.messages[responded++];
      let result: unknown;
      if (message.method === "assertFact") {
        const fact = {
          id: nextFactId++,
          relation: message.args[1] as string,
          fields: message.args.slice(2),
        };
        facts.push(fact);
        result = fact.id;
      } else if (message.method === "reset") {
        facts.splice(0);
        result = undefined;
      } else if (message.method === "findFacts") {
        const relation = message.args[1];
        result = facts.filter((fact) => fact.relation === relation);
      } else {
        result = responseFor(message);
      }
      worker.emit("message", { id: message.id, result });
    }
  };

  try {
    outer = pool.do("rules", async (proxy) => {
      const accepted = [
        proxy.assertFact("accepted-order", 1),
        proxy.reset(),
        proxy.assertFact("accepted-order", 3),
      ];
      acceptedIssued.resolve();
      acceptedOutcomes = await Promise.allSettled(accepted);
      try {
        postAbortOutcome = {
          status: "fulfilled",
          value: await proxy.assertFact("accepted-order", 99),
        };
      } catch (error) {
        postAbortOutcome = { status: "rejected", error };
      } finally {
        callbackFinished = true;
      }
    }, { signal: controller.signal });
    await acceptedIssued.promise;
    lease = slot.activeLease;
    assert.ok(lease);
    const releaseCount = instrumentLeaseRelease(lease);
    assert.strictEqual(worker.messages.length, 1, "first accepted call was not immediate");
    assert.strictEqual(lease.queue.length, 2, "owner calls were not FIFO queued");
    assert.strictEqual(lease.pendingCalls, 3);

    controller.abort();
    await assert.rejects(outer, assertAbortError);
    assert.strictEqual(slot.activeLease, lease);

    for (let turn = 0; turn < 40 && !callbackFinished; turn++) {
      respondAvailable();
      await yieldImmediate();
    }
    respondAvailable();
    assert.strictEqual(callbackFinished, true, "callback did not finish after accepted drain");
    await lease.releasedPromise;

    const ownerMethods = worker.messages.map((message) => message.method);
    assert.deepStrictEqual(
      ownerMethods,
      ["assertFact", "reset", "assertFact"],
      "abort dequeued accepted work or dispatched a later invocation",
    );
    assert.ok(acceptedOutcomes);
    assert.deepStrictEqual(
      acceptedOutcomes.map((outcome) => outcome.status),
      ["fulfilled", "fulfilled", "fulfilled"],
    );
    assert.strictEqual(postAbortOutcome?.status, "rejected");
    if (postAbortOutcome?.status === "rejected") {
      assertAbortError(postAbortOutcome.error);
    }
    assertReleasedLease(lease, releaseCount());
    assert.strictEqual(
      getEventListeners(controller.signal, "abort").length,
      listenerBaseline,
    );

    followUp = pool.do("rules", async (proxy) =>
      proxy.findFacts("accepted-order"),
    );
    await waitFor(
      () => worker.messages.length > ownerMethods.length,
      "post-cancellation state observation",
    );
    respondAvailable();
    const observed = (await followUp) as Array<{ fields: unknown[] }>;
    assert.deepStrictEqual(
      observed.map((fact) => fact.fields[0]),
      [3],
      "accepted pre-abort mutation effects did not drain in invocation order",
    );
  } finally {
    respondAvailable();
    if (outer) await outer.catch(() => undefined);
    if (followUp) await followUp.catch(() => undefined);
    await pool.close();
  }
});

test("E-016 an accepted run keeps cooperative native cancellation and its partial result", async () => {
  const { pool, worker, slot } = makePool();
  const controller = new AbortController();
  const listenerBaseline = getEventListeners(controller.signal, "abort").length;
  const runIssued = deferred();
  let retained!: EngineProxy;
  let runOutcome:
    | { status: "fulfilled"; value: unknown }
    | { status: "rejected"; error: unknown }
    | undefined;
  let postRunOutcome:
    | { status: "fulfilled"; value: unknown }
    | { status: "rejected"; error: unknown }
    | undefined;
  let callbackFinished = false;
  let outer: Promise<unknown> | undefined;
  let recovery: Promise<unknown> | undefined;
  let lease: any;

  try {
    outer = pool.do("rules", async (proxy) => {
      retained = proxy;
      const acceptedRun = proxy.run({ limit: 9 });
      runIssued.resolve();
      try {
        runOutcome = { status: "fulfilled", value: await acceptedRun };
      } catch (error) {
        runOutcome = { status: "rejected", error };
      }
      try {
        postRunOutcome = {
          status: "fulfilled",
          value: await proxy.halt(),
        };
      } catch (error) {
        postRunOutcome = { status: "rejected", error };
      } finally {
        callbackFinished = true;
      }
    }, { signal: controller.signal });
    await runIssued.promise;
    lease = slot.activeLease;
    assert.ok(lease);
    const releaseCount = instrumentLeaseRelease(lease);
    assert.strictEqual(worker.messages.length, 1);
    const runMessage = worker.messages[0];
    assert.strictEqual(runMessage.method, "__batched_run");
    const abortBuffer = new Int32Array(runMessage.args[2] as SharedArrayBuffer);
    assert.strictEqual(Atomics.load(abortBuffer, 0), 0);

    controller.abort();
    await assert.rejects(outer, assertAbortError);
    assert.strictEqual(Atomics.load(abortBuffer, 0), 1);
    assert.strictEqual(slot.activeLease, lease);

    const partial = {
      rulesFired: 2,
      haltReason: HaltReason.HaltRequested,
      errors: [],
    };
    worker.emit("message", { id: runMessage.id, result: partial });
    await driveWorkerUntil(worker, () => callbackFinished);
    await lease.releasedPromise;

    assert.deepStrictEqual(runOutcome, {
      status: "fulfilled",
      value: partial,
    });
    assert.strictEqual(postRunOutcome?.status, "rejected");
    if (postRunOutcome?.status === "rejected") {
      assertAbortError(postRunOutcome.error);
    }
    assert.strictEqual(
      worker.messages.length,
      1,
      "a proxy call after the accepted run reached the worker",
    );
    assertReleasedLease(lease, releaseCount());
    assert.strictEqual(
      getEventListeners(controller.signal, "abort").length,
      listenerBaseline,
    );
    await assert.rejects(() => retained.run(), assertInactiveProxyError);

    recovery = pool.do("rules", async (proxy) => proxy.run());
    await waitFor(
      () => worker.messages.length === 2,
      "post-cancellation run recovery",
    );
    const recoveryMessage = worker.messages[1];
    const recoveryResult = responseFor(recoveryMessage);
    worker.emit("message", {
      id: recoveryMessage.id,
      result: recoveryResult,
    });
    assert.deepStrictEqual(await recovery, {
      rulesFired: 0,
      haltReason: HaltReason.AgendaEmpty,
      errors: [],
    });
  } finally {
    for (const message of worker.messages) {
      if (slot.pending.has(message.id)) {
        worker.emit("message", { id: message.id, result: responseFor(message) });
      }
    }
    if (outer) await outer.catch(() => undefined);
    if (recovery) await recovery.catch(() => undefined);
    await pool.close();
  }
});

test("E-016 abort during run validation rejects before request allocation", async () => {
  const { pool, worker, slot } = makePool();
  const controller = new AbortController();
  const listenerBaseline = getEventListeners(controller.signal, "abort").length;
  const callObserved = deferred();
  const resumeCallback = deferred();
  let callReason: unknown;
  let getterReads = 0;
  let outer: Promise<unknown> | undefined;
  let lease: any;

  try {
    outer = pool.do("rules", async (proxy) => {
      const options = {
        get limit(): number {
          getterReads += 1;
          controller.abort();
          return 1;
        },
      };
      let pending!: Promise<unknown>;
      assert.doesNotThrow(() => {
        pending = proxy.run(options);
      });
      callReason = await rejectionOf(pending);
      callObserved.resolve();
      await resumeCallback.promise;
    }, { signal: controller.signal });
    await callObserved.promise;
    lease = slot.activeLease;
    assert.ok(lease);
    const releaseCount = instrumentLeaseRelease(lease);

    await assert.rejects(outer, assertAbortError);
    assertAbortError(callReason);
    assert.strictEqual(getterReads, 1);
    assert.strictEqual(worker.messages.length, 0);
    assert.strictEqual(slot.nextId, 0);
    assert.strictEqual(slot.pending.size, 0);
    assert.strictEqual(slot.inflight, 0);
    assert.strictEqual(lease.pendingCalls, 0);
    assert.strictEqual(lease.queue.length, 0);

    resumeCallback.resolve();
    await lease.releasedPromise;
    assertReleasedLease(lease, releaseCount());
    assert.strictEqual(
      getEventListeners(controller.signal, "abort").length,
      listenerBaseline,
    );
  } finally {
    resumeCallback.resolve();
    if (outer) await outer.catch(() => undefined);
    await pool.close();
  }
});

test("E-016 accepted queued calls retain exact send and terminal outcomes after abort", async (t) => {
  const cases = [
    {
      label: "synchronous send failure",
      expectedStatuses: ["fulfilled", "rejected"],
      trigger(
        worker: ControlledWorker,
        exactError: Error,
      ): void {
        worker.postActions.push(() => {
          throw exactError;
        });
        worker.emit("message", { id: worker.messages[0].id, result: [] });
      },
    },
    {
      label: "terminal failure",
      expectedStatuses: ["rejected", "rejected"],
      trigger(
        worker: ControlledWorker,
        exactError: Error,
      ): void {
        worker.emit("error", exactError);
      },
    },
  ] as const;

  for (const item of cases) {
    await t.test(item.label, async () => {
      const { pool, worker, slot } = makePool();
      const controller = new AbortController();
      const listenerBaseline = getEventListeners(
        controller.signal,
        "abort",
      ).length;
      const acceptedIssued = deferred();
      const exactError = item.label === "synchronous send failure"
        ? new DOMException("accepted send failed", "DataCloneError")
        : new Error("accepted worker became terminal");
      let acceptedOutcomes: PromiseSettledResult<unknown>[] | undefined;
      let callbackFinished = false;
      let outer: Promise<unknown> | undefined;
      let lease: any;

      try {
        outer = pool.do("rules", async (proxy) => {
          const accepted = [proxy.facts(), proxy.facts()];
          acceptedIssued.resolve();
          acceptedOutcomes = await Promise.allSettled(accepted);
          callbackFinished = true;
        }, { signal: controller.signal });
        await acceptedIssued.promise;
        lease = slot.activeLease;
        assert.ok(lease);
        const releaseCount = instrumentLeaseRelease(lease);
        assert.strictEqual(worker.messages.length, 1);
        assert.strictEqual(lease.queue.length, 1);
        assert.strictEqual(lease.pendingCalls, 2);

        controller.abort();
        await assert.rejects(outer, assertAbortError);
        item.trigger(worker, exactError);
        await waitFor(() => callbackFinished, `${item.label} callback outcome`);
        await lease.releasedPromise;

        assert.ok(acceptedOutcomes);
        assert.deepStrictEqual(
          acceptedOutcomes.map((outcome) => outcome.status),
          [...item.expectedStatuses],
        );
        for (const outcome of acceptedOutcomes) {
          if (outcome.status === "rejected") {
            assert.strictEqual(
              outcome.reason,
              exactError,
              "abort replaced an accepted call's own failure",
            );
          }
        }
        assertReleasedLease(lease, releaseCount());
        assert.strictEqual(
          getEventListeners(controller.signal, "abort").length,
          listenerBaseline,
        );
      } finally {
        for (const message of worker.messages) {
          if (slot.pending.has(message.id)) {
            worker.emit("message", { id: message.id, result: [] });
          }
        }
        if (outer) await outer.catch(() => undefined);
        await pool.close();
      }
    });
  }
});

test("E-016 callback settlement wins before an accepted call drains", async () => {
  const { pool, worker, slot } = makePool();
  const controller = new AbortController();
  const listenerBaseline = getEventListeners(controller.signal, "abort").length;
  let retained!: EngineProxy;
  let accepted!: Promise<unknown>;
  let lease: any;
  let outer: Promise<unknown> | undefined;

  try {
    outer = pool.do("rules", async (proxy) => {
      retained = proxy;
      accepted = proxy.facts();
      return "callback result";
    }, { signal: controller.signal });
    lease = slot.activeLease;
    assert.ok(lease);
    const releaseCount = instrumentLeaseRelease(lease);

    await waitFor(
      () => !lease.active &&
        getEventListeners(controller.signal, "abort").length === listenerBaseline,
      "pool-observed callback settlement",
    );
    assert.strictEqual(lease.pendingCalls, 1);
    assert.strictEqual(slot.activeLease, lease);

    controller.abort();
    const beforeRetainedCall = {
      messages: worker.messages.length,
      nextId: slot.nextId,
      pending: slot.pending.size,
      inflight: slot.inflight,
    };
    await assert.rejects(() => retained.facts(), assertInactiveProxyError);
    assert.deepStrictEqual(
      {
        messages: worker.messages.length,
        nextId: slot.nextId,
        pending: slot.pending.size,
        inflight: slot.inflight,
      },
      beforeRetainedCall,
    );

    const acceptedMessage = worker.messages[0];
    worker.emit("message", { id: acceptedMessage.id, result: [] });
    assert.deepStrictEqual(await accepted, []);
    assert.strictEqual(await outer, "callback result");
    await lease.releasedPromise;
    assertReleasedLease(lease, releaseCount());
    assert.strictEqual(
      getEventListeners(controller.signal, "abort").length,
      listenerBaseline,
    );
  } finally {
    for (const message of worker.messages) {
      if (slot.pending.has(message.id)) {
        worker.emit("message", { id: message.id, result: responseFor(message) });
      }
    }
    if (outer) await outer.catch(() => undefined);
    await pool.close();
  }
});

test("E-016 inactive, terminal, and closed proxy states precede cancellation", async () => {
  const { pool, worker, slot } = makePool();
  const controller = new AbortController();
  const lease = await (pool as any).acquireLease(slot);
  const proxy = (pool as any).makeProxy(
    "rules",
    slot,
    lease,
    controller.signal,
  ) as EngineProxy;
  const terminalError = new Error("terminal exact identity");
  controller.abort();
  const baseline = {
    messages: worker.messages.length,
    nextId: slot.nextId,
    pending: slot.pending.size,
    inflight: slot.inflight,
  };

  try {
    slot.state = { kind: "failed", error: terminalError };
    assert.strictEqual(
      await rejectionOf(proxy.run({ limit: Number.NaN })),
      terminalError,
    );

    slot.state = { kind: "terminating" };
    await assert.rejects(
      () => proxy.run({ limit: Number.NaN }),
      /EnginePool has been closed/,
    );

    slot.state = { kind: "running" };
    await (pool as any).releaseLease(slot, lease);
    await assert.rejects(
      () => proxy.run({ limit: Number.NaN }),
      assertInactiveProxyError,
    );
    assert.deepStrictEqual(
      {
        messages: worker.messages.length,
        nextId: slot.nextId,
        pending: slot.pending.size,
        inflight: slot.inflight,
      },
      baseline,
    );
  } finally {
    slot.state = { kind: "running" };
    await (pool as any).releaseLease(slot, lease);
    await pool.close();
  }
});

test("E-016 seeded schedules preserve the cancellation admission boundary", async () => {
  const initialSeed = 0x140e016;
  let seed = initialSeed;
  const next = (): number => {
    seed = (Math.imul(seed, 1_664_525) + 1_013_904_223) >>> 0;
    return seed;
  };

  for (let iteration = 0; iteration < 16; iteration++) {
    const { pool, worker, slot } = makePool();
    const controller = new AbortController();
    const listenerBaseline = getEventListeners(controller.signal, "abort").length;
    const acceptedCount = next() % 4;
    const useInvalidRun = (next() & 1) === 1;
    const callbackEntered = deferred();
    const resumeCallback = deferred();
    let acceptedOutcomes: PromiseSettledResult<unknown>[] | undefined;
    let blockedOutcome:
      | { status: "fulfilled"; value: unknown }
      | { status: "rejected"; error: unknown }
      | undefined;
    let callbackFinished = false;
    let lease: any;
    let outer: Promise<unknown> | undefined;

    try {
      outer = pool.do("rules", async (proxy) => {
        const accepted = Array.from(
          { length: acceptedCount },
          () => proxy.facts(),
        );
        callbackEntered.resolve();
        await resumeCallback.promise;
        try {
          blockedOutcome = {
            status: "fulfilled",
            value: useInvalidRun
              ? await proxy.run({ limit: Number.NaN })
              : await proxy.reset(),
          };
        } catch (error) {
          blockedOutcome = { status: "rejected", error };
        }
        acceptedOutcomes = await Promise.allSettled(accepted);
        callbackFinished = true;
      }, { signal: controller.signal });
      await callbackEntered.promise;
      lease = slot.activeLease;
      assert.ok(lease);
      const releaseCount = instrumentLeaseRelease(lease);

      controller.abort();
      await assert.rejects(outer, assertAbortError);
      resumeCallback.resolve();
      await driveWorkerUntil(worker, () => callbackFinished);
      await lease.releasedPromise;

      assert.strictEqual(
        blockedOutcome?.status,
        "rejected",
        `seed ${initialSeed.toString(16)} iteration ${iteration}: post-abort call`,
      );
      if (blockedOutcome?.status === "rejected") {
        assertAbortError(blockedOutcome.error);
      }
      assert.ok(acceptedOutcomes);
      assert.deepStrictEqual(
        acceptedOutcomes.map((outcome) => outcome.status),
        Array.from({ length: acceptedCount }, () => "fulfilled"),
        `seed ${initialSeed.toString(16)} iteration ${iteration}: accepted drain`,
      );
      assert.strictEqual(
        worker.messages.length,
        acceptedCount,
        `seed ${initialSeed.toString(16)} iteration ${iteration}: dispatch count`,
      );
      assertReleasedLease(lease, releaseCount());
      assert.strictEqual(slot.pending.size, 0);
      assert.strictEqual(slot.inflight, 0);
      assert.strictEqual(
        getEventListeners(controller.signal, "abort").length,
        listenerBaseline,
      );
    } finally {
      resumeCallback.resolve();
      for (const message of worker.messages) {
        if (slot.pending.has(message.id)) {
          worker.emit("message", { id: message.id, result: responseFor(message) });
        }
      }
      if (outer) await outer.catch(() => undefined);
      await pool.close();
    }
  }
});
