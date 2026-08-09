/**
 * EnginePool callback-lease isolation tests (FR-NODE-003 / E-012).
 */
import { setImmediate as yieldImmediate } from "node:timers/promises";
import { test } from "node:test";
import * as assert from "node:assert/strict";

import type { EngineProxy } from "../../../helpers/ferric";
import { EnginePool } from "../../../helpers/ferric";

const RETAINED_PROXY_ERROR =
  "EngineProxy is no longer valid outside its EnginePool.do callback";
const REENTRANT_ERROR =
  /cannot be called from within an active EnginePool\.do callback on the same pool/;

interface Deferred<T> {
  promise: Promise<T>;
  resolve: (value: T | PromiseLike<T>) => void;
  reject: (reason?: unknown) => void;
}

function deferred<T = void>(): Deferred<T> {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

async function yieldTurns(count = 1): Promise<void> {
  for (let i = 0; i < count; i++) {
    await yieldImmediate();
  }
}

function assertRetainedProxyError(error: unknown): boolean {
  assert.ok(error instanceof Error);
  assert.strictEqual(error.name, "Error");
  assert.strictEqual(error.message, RETAINED_PROXY_ERROR);
  return true;
}

// ---------------------------------------------------------------------------
// E-012: the whole worker, including its other named engines, is leased
// ---------------------------------------------------------------------------
test("E-012 one worker excludes same-spec, different-spec, and evaluate work", { timeout: 5_000 }, async () => {
  const pool = await EnginePool.create(
    [
      { name: "alpha", source: "" },
      { name: "beta", source: "" },
    ],
    { threads: 1 },
  );
  const firstEntered = deferred();
  const releaseFirst = deferred();
  let secondEntered = false;
  let evaluateSettled = false;
  const completionOrder: string[] = [];
  let first: Promise<unknown> | undefined;
  let second: Promise<unknown> | undefined;
  let evaluation: Promise<unknown> | undefined;

  try {
    first = pool.do("alpha", async (proxy) => {
      await proxy.reset();
      await proxy.assertFact("lease-owner", "alpha");
      firstEntered.resolve();
      await releaseFirst.promise;
      const owners = await proxy.findFacts("lease-owner");
      return owners.map((fact) => fact.fields[0]);
    });
    await firstEntered.promise;

    second = pool.do("beta", async (proxy) => {
      secondEntered = true;
      await proxy.reset();
      await proxy.assertFact("lease-owner", "beta");
      return "second";
    }).then((value) => {
      completionOrder.push("do:beta");
      return value;
    });
    evaluation = pool.evaluate("alpha", {
      facts: [
        {
          kind: "ordered",
          relation: "evaluation",
          fields: [1],
        },
      ],
    }).then((value) => {
      completionOrder.push("evaluate:alpha");
      return value;
    });
    void evaluation.then(
      () => { evaluateSettled = true; },
      () => { evaluateSettled = true; },
    );

    await yieldTurns(2);
    assert.strictEqual(secondEntered, false, "a waiting do callback started inside the lease");
    assert.strictEqual(evaluateSettled, false, "evaluate reached a leased worker");

    releaseFirst.resolve();
    assert.deepStrictEqual(await first, ["alpha"]);
    assert.strictEqual(await second, "second");
    const evaluated = await evaluation;
    assert.ok(evaluated.facts.some((fact) => fact.relation === "evaluation"));
    assert.deepStrictEqual(completionOrder, ["do:beta", "evaluate:alpha"]);
  } finally {
    releaseFirst.resolve();
    await Promise.allSettled(
      [first, second, evaluation].filter((promise): promise is Promise<unknown> => promise !== undefined),
    );
    await pool.close();
  }
});

// ---------------------------------------------------------------------------
// E-012: owner requests bypass unrelated waiters without sacrificing FIFO
// ---------------------------------------------------------------------------
test("E-012 lease owner keeps dispatch access after another callback queues", { timeout: 5_000 }, async () => {
  const pool = await EnginePool.create([{ name: "rules", source: "" }], { threads: 1 });
  const ownerEntered = deferred();
  const continueOwner = deferred();
  let waiterEntered = false;
  let owner: Promise<unknown> | undefined;
  let waiter: Promise<unknown> | undefined;

  try {
    owner = pool.do("rules", async (proxy) => {
      await proxy.reset();
      ownerEntered.resolve();
      await continueOwner.promise;

      // These owner requests must not queue behind the lease waiter. A single
      // undifferentiated FIFO would deadlock here.
      await proxy.assertFact("lease-owner", 1);
      return (await proxy.findFacts("lease-owner")).map((fact) => fact.fields[0]);
    });
    await ownerEntered.promise;

    waiter = pool.do("rules", async (proxy) => {
      waiterEntered = true;
      await proxy.reset();
      return "waiter";
    });
    await yieldTurns(2);
    assert.strictEqual(waiterEntered, false);

    continueOwner.resolve();
    assert.deepStrictEqual(await owner, [1]);
    assert.strictEqual(await waiter, "waiter");
  } finally {
    continueOwner.resolve();
    await Promise.allSettled(
      [owner, waiter].filter((promise): promise is Promise<unknown> => promise !== undefined),
    );
    await pool.close();
  }
});

test("E-012 one-worker lease waiters enter FIFO without starvation", { timeout: 5_000 }, async () => {
  const pool = await EnginePool.create([{ name: "rules", source: "" }], { threads: 1 });
  const holderEntered = deferred();
  const releaseHolder = deferred();
  const starts: string[] = [];
  let holder: Promise<unknown> | undefined;
  let waiters: Array<Promise<unknown>> = [];

  try {
    holder = pool.do("rules", async () => {
      holderEntered.resolve();
      await releaseHolder.promise;
    });
    await holderEntered.promise;

    waiters = ["B", "C", "D"].map((label) =>
      pool.do("rules", async (proxy) => {
        starts.push(label);
        await proxy.facts();
        return label;
      }),
    );

    await yieldTurns(2);
    assert.deepStrictEqual(starts, [], "a queued callback entered before lease release");

    releaseHolder.resolve();
    await holder;
    assert.deepStrictEqual(await Promise.all(waiters), ["B", "C", "D"]);
    assert.deepStrictEqual(starts, ["B", "C", "D"]);
  } finally {
    releaseHolder.resolve();
    await Promise.allSettled(
      [holder, ...waiters].filter((promise): promise is Promise<unknown> => promise !== undefined),
    );
    await pool.close();
  }
});

test("E-012 root FIFO keeps an older evaluate ahead of a later lease", { timeout: 5_000 }, async () => {
  const pool = await EnginePool.create([{ name: "rules", source: "" }], { threads: 1 });
  const holderEntered = deferred();
  const releaseHolder = deferred();
  let laterLeaseEntered = false;
  let holder: Promise<unknown> | undefined;
  let evaluation: Promise<unknown> | undefined;
  let laterLease: Promise<unknown> | undefined;

  try {
    holder = pool.do("rules", async () => {
      holderEntered.resolve();
      await releaseHolder.promise;
    });
    await holderEntered.promise;

    evaluation = pool.evaluate("rules", {
      facts: [
        {
          kind: "ordered",
          relation: "older-evaluate",
          fields: [1],
        },
      ],
    });
    laterLease = pool.do("rules", async (proxy) => {
      laterLeaseEntered = true;
      return (await proxy.findFacts("older-evaluate")).map(
        (fact) => fact.fields[0],
      );
    });

    assert.strictEqual(laterLeaseEntered, false);
    releaseHolder.resolve();
    await holder;

    const evaluated = await evaluation as Awaited<ReturnType<EnginePool["evaluate"]>>;
    assert.ok(evaluated.facts.some((fact) => fact.relation === "older-evaluate"));
    // The worker admits the next root item from its message handler before the
    // older Promise continuation runs. Persisted engine state is therefore the
    // deterministic execution-order oracle: the later lease can observe this
    // fact only if the older evaluate ran first.
    assert.deepStrictEqual(await laterLease, [1]);
  } finally {
    releaseHolder.resolve();
    await Promise.allSettled(
      [holder, evaluation, laterLease].filter(
        (promise): promise is Promise<unknown> => promise !== undefined,
      ),
    );
    await pool.close();
  }
});

// ---------------------------------------------------------------------------
// E-012: every worker can host one lease, never two
// ---------------------------------------------------------------------------
test("E-012 two workers isolate two active callbacks from later callbacks", { timeout: 5_000 }, async () => {
  const pool = await EnginePool.create([{ name: "rules", source: "" }], { threads: 2 });
  const entered = [deferred(), deferred()];
  const release = [deferred(), deferred()];
  const laterStarts: string[] = [];
  let active: Array<Promise<unknown>> = [];
  let later: Array<Promise<unknown>> = [];

  try {
    active = ["A", "B"].map((label, index) =>
      pool.do("rules", async (proxy) => {
        await proxy.reset();
        await proxy.assertFact("lease-owner", label);
        entered[index].resolve();
        await release[index].promise;
        return (await proxy.findFacts("lease-owner")).map((fact) => fact.fields[0]);
      }),
    );
    await Promise.all(entered.map((item) => item.promise));

    later = ["C", "D"].map((label) =>
      pool.do("rules", async (proxy) => {
        laterStarts.push(label);
        await proxy.reset();
        await proxy.assertFact("lease-owner", label);
        return label;
      }),
    );

    await yieldTurns(2);
    assert.deepStrictEqual(laterStarts, [], "a third callback entered a leased worker");

    release[0].resolve();
    release[1].resolve();
    assert.deepStrictEqual(await Promise.all(active), [["A"], ["B"]]);
    assert.deepStrictEqual(await Promise.all(later), ["C", "D"]);
    assert.deepStrictEqual(laterStarts.sort(), ["C", "D"]);
  } finally {
    release[0].resolve();
    release[1].resolve();
    await Promise.allSettled([...active, ...later]);
    await pool.close();
  }
});

// ---------------------------------------------------------------------------
// E-012: proxy calls are serialized in invocation order inside the lease
// ---------------------------------------------------------------------------
test("E-012 parallel proxy calls preserve invocation order ahead of waiters", { timeout: 5_000 }, async () => {
  const pool = await EnginePool.create([{ name: "rules", source: "" }], { threads: 1 });
  const ownerEntered = deferred();
  const issueParallelCalls = deferred();
  let waiterEntered = false;
  let owner: Promise<unknown> | undefined;
  let waiter: Promise<unknown> | undefined;

  try {
    owner = pool.do("rules", async (proxy) => {
      await proxy.reset();
      ownerEntered.resolve();
      await issueParallelCalls.promise;

      const assertedBeforeReset = proxy.assertFact("ordered", 1);
      const reset = proxy.reset();
      const assertedAfterReset = proxy.assertFact("ordered", 3);
      await Promise.all([assertedBeforeReset, reset, assertedAfterReset]);
      return (await proxy.findFacts("ordered")).map((fact) => fact.fields[0]);
    });
    await ownerEntered.promise;

    waiter = pool.do("rules", async () => {
      waiterEntered = true;
    });
    await yieldTurns(2);
    assert.strictEqual(waiterEntered, false);

    issueParallelCalls.resolve();
    assert.deepStrictEqual(await owner, [3]);
    await waiter;
  } finally {
    issueParallelCalls.resolve();
    await Promise.allSettled(
      [owner, waiter].filter((promise): promise is Promise<unknown> => promise !== undefined),
    );
    await pool.close();
  }
});

test("E-012 normal callback settlement drains accepted unawaited proxy calls", async () => {
  const pool = await EnginePool.create([{ name: "rules", source: "" }], { threads: 1 });
  let acceptedCalls!: Promise<unknown[]>;
  let acceptedCallsSettled = false;

  try {
    const result = await pool.do("rules", async (proxy) => {
      const assertedBeforeReset = proxy.assertFact("accepted-order", 1);
      const reset = proxy.reset();
      const assertedAfterReset = proxy.assertFact("accepted-order", 3);
      acceptedCalls = Promise.all([assertedBeforeReset, reset, assertedAfterReset]);
      void acceptedCalls.then(
        () => { acceptedCallsSettled = true; },
        () => { acceptedCallsSettled = true; },
      );
      return "callback-return";
    });

    assert.strictEqual(result, "callback-return");
    assert.strictEqual(
      acceptedCallsSettled,
      true,
      "do resolved before an operation accepted during the callback settled",
    );
    await acceptedCalls;

    const values = await pool.do("rules", async (proxy) =>
      (await proxy.findFacts("accepted-order")).map((fact) => fact.fields[0]),
    );
    assert.deepStrictEqual(values, [3]);
  } finally {
    await pool.close();
  }
});

// ---------------------------------------------------------------------------
// E-012: callback settlement invalidates and releases exactly once
// ---------------------------------------------------------------------------
test("E-012 synchronous and asynchronous callback failures release their leases", async () => {
  const pool = await EnginePool.create([{ name: "rules", source: "" }], { threads: 1 });

  try {
    const cases = [
      {
        label: "synchronous throw",
        invoke: (proxy: EngineProxy): Promise<never> => {
          void proxy;
          throw new Error("sync callback failure");
        },
        message: /sync callback failure/,
      },
      {
        label: "asynchronous rejection",
        invoke: async (proxy: EngineProxy): Promise<never> => {
          await proxy.assertFact("persists-after-error", "async");
          throw new Error("async callback failure");
        },
        message: /async callback failure/,
      },
    ];

    for (const item of cases) {
      let retained!: EngineProxy;
      await assert.rejects(
        () => pool.do("rules", ((proxy: EngineProxy) => {
          retained = proxy;
          return item.invoke(proxy);
        }) as (proxy: EngineProxy) => Promise<never>),
        item.message,
        item.label,
      );
      await assert.rejects(() => retained.facts(), assertRetainedProxyError);

      const followUp = await pool.do("rules", async (proxy) => {
        const persisted = await proxy.findFacts("persists-after-error");
        await proxy.reset();
        return persisted.length;
      });
      assert.strictEqual(followUp, item.label === "asynchronous rejection" ? 1 : 0);
    }

    let nativeFailureProxy!: EngineProxy;
    await assert.rejects(
      () => pool.do("rules", async (proxy) => {
        nativeFailureProxy = proxy;
        await proxy.load("(");
      }),
      /parse|expected|unexpected/i,
    );
    await assert.rejects(() => nativeFailureProxy.facts(), assertRetainedProxyError);
    await assert.doesNotReject(() => pool.do("rules", async (proxy) => proxy.reset()));
  } finally {
    await pool.close();
  }
});

// ---------------------------------------------------------------------------
// E-012: queued and active host abort preserve the lease boundary
// ---------------------------------------------------------------------------
test("E-012 abort while waiting removes the lease waiter without invoking it", { timeout: 5_000 }, async () => {
  const pool = await EnginePool.create([{ name: "rules", source: "" }], { threads: 1 });
  const holderEntered = deferred();
  const releaseHolder = deferred();
  const abortController = new AbortController();
  let abortedCallbackEntered = false;
  let finalCallbackEntered = false;
  let holder: Promise<unknown> | undefined;
  let aborted: Promise<unknown> | undefined;
  let final: Promise<unknown> | undefined;

  try {
    holder = pool.do("rules", async () => {
      holderEntered.resolve();
      await releaseHolder.promise;
    });
    await holderEntered.promise;

    aborted = pool.do("rules", async () => {
      abortedCallbackEntered = true;
    }, { signal: abortController.signal });
    final = pool.do("rules", async () => {
      finalCallbackEntered = true;
      return "final";
    });

    abortController.abort();
    await assert.rejects(aborted, (error: any) => error?.name === "AbortError");
    assert.strictEqual(abortedCallbackEntered, false);
    assert.strictEqual(finalCallbackEntered, false);

    releaseHolder.resolve();
    await holder;
    assert.strictEqual(await final, "final");
    assert.strictEqual(finalCallbackEntered, true);
  } finally {
    releaseHolder.resolve();
    await Promise.allSettled(
      [holder, aborted, final].filter((promise): promise is Promise<unknown> => promise !== undefined),
    );
    await pool.close();
  }
});

test("E-012 abort races at lease admission never strand the slot", async () => {
  const pool = await EnginePool.create([{ name: "rules", source: "" }], { threads: 1 });

  try {
    const admissionAbort = new AbortController();
    let admissionCallbackEntered = false;
    const abortedAtAdmission = pool.do("rules", async () => {
      admissionCallbackEntered = true;
    }, { signal: admissionAbort.signal });
    // acquireLease() resolves synchronously for an idle slot, but do() resumes
    // in a microtask. Abort in that deliberate admission window.
    admissionAbort.abort();
    await assert.rejects(abortedAtAdmission, (error: any) => error?.name === "AbortError");
    assert.strictEqual(admissionCallbackEntered, false);

    const callbackAbort = new AbortController();
    let callbackEntered = false;
    const abortedByCallback = pool.do("rules", async () => {
      callbackEntered = true;
      // This fires before do() installs its caller-facing completion listener.
      callbackAbort.abort();
    }, { signal: callbackAbort.signal });
    await assert.rejects(abortedByCallback, (error: any) => error?.name === "AbortError");
    assert.strictEqual(callbackEntered, true);

    await assert.doesNotReject(() => pool.do("rules", async (proxy) => proxy.facts()));
  } finally {
    await pool.close();
  }
});

test("E-012 active abort keeps the lease until the callback really settles", { timeout: 5_000 }, async () => {
  const pool = await EnginePool.create([{ name: "rules", source: "" }], { threads: 1 });
  const callbackEntered = deferred();
  const releaseCallback = deferred();
  const callbackDone = deferred();
  const abortController = new AbortController();
  let waiterEntered = false;
  let active: Promise<unknown> | undefined;
  let waiter: Promise<unknown> | undefined;

  try {
    active = pool.do("rules", async () => {
      callbackEntered.resolve();
      try {
        await releaseCallback.promise;
      } finally {
        callbackDone.resolve();
      }
    }, { signal: abortController.signal });
    await callbackEntered.promise;

    abortController.abort();
    await assert.rejects(active, (error: any) => error?.name === "AbortError");

    waiter = pool.do("rules", async () => {
      waiterEntered = true;
      return "waiter";
    });
    await yieldTurns(2);
    assert.strictEqual(waiterEntered, false, "abort released a still-running callback lease");

    releaseCallback.resolve();
    await callbackDone.promise;
    assert.strictEqual(await waiter, "waiter");
  } finally {
    releaseCallback.resolve();
    await Promise.allSettled(
      [active, waiter].filter((promise): promise is Promise<unknown> => promise !== undefined),
    );
    await pool.close();
  }
});

// ---------------------------------------------------------------------------
// E-012: close treats an admitted callback as in-flight work
// ---------------------------------------------------------------------------
test("E-012 close waits for an active lease and rejects waiting work", { timeout: 5_000 }, async () => {
  const pool = await EnginePool.create(
    [
      { name: "active", source: "" },
      { name: "waiting", source: "" },
    ],
    { threads: 1 },
  );
  const activeEntered = deferred();
  const finishActive = deferred();
  let waitingCallbackEntered = false;
  let closeSettled = false;
  let closing: Promise<void> | undefined;

  const active = pool.do("active", async (proxy) => {
    await proxy.reset();
    activeEntered.resolve();
    await finishActive.promise;

    // This belongs to an already-admitted callback and remains allowed while
    // close rejects new/unleased submissions.
    await proxy.assertFact("completed-during-close", 1);
    return (await proxy.findFacts("completed-during-close")).length;
  });
  await activeEntered.promise;

  const waiting = pool.do("waiting", async () => {
    waitingCallbackEntered = true;
  });
  const evaluation = pool.evaluate("waiting", {});
  const waitingOutcome = waiting.then(
    () => ({ status: "resolved" as const }),
    (error: unknown) => ({ status: "rejected" as const, error }),
  );
  const evaluateOutcome = evaluation.then(
    () => ({ status: "resolved" as const }),
    (error: unknown) => ({ status: "rejected" as const, error }),
  );

  try {
    await yieldTurns(2);
    closing = pool.close().finally(() => { closeSettled = true; });
    await yieldTurns(2);
    assert.strictEqual(closeSettled, false, "close ignored an admitted callback lease");
    assert.strictEqual(waitingCallbackEntered, false);

    const waitingResult = await waitingOutcome;
    assert.strictEqual(waitingResult.status, "rejected");
    assert.match((waitingResult as { error: Error }).error.message, /closed/i);
    const evaluateResult = await evaluateOutcome;
    assert.strictEqual(evaluateResult.status, "rejected");
    assert.match((evaluateResult as { error: Error }).error.message, /closed/i);

    finishActive.resolve();
    assert.strictEqual(await active, 1);
    await closing;
    assert.strictEqual(closeSettled, true);
  } finally {
    finishActive.resolve();
    await Promise.allSettled([active, waiting, evaluation]);
    if (closing) {
      await closing.catch(() => undefined);
    } else {
      await pool.close();
    }
  }
});

// ---------------------------------------------------------------------------
// E-012: proxy validity ends before do() settles to its caller
// ---------------------------------------------------------------------------
test("E-012 every retained proxy method rejects one stable lifetime error", async () => {
  const pool = await EnginePool.create([{ name: "rules", source: "" }], { threads: 1 });
  let retained!: EngineProxy;

  try {
    await pool.do("rules", async (proxy) => {
      retained = proxy;
      await proxy.reset();
      await proxy.assertFact("baseline", 1);
    });

    const calls: Array<[string, () => Promise<unknown>]> = [
      ["load", () => retained.load("(defrule leaked (never) =>)")],
      ["assertString", () => retained.assertString("(retained-string 1)")],
      ["assertFact", () => retained.assertFact("retained-fact", 1)],
      ["assertTemplate", () => retained.assertTemplate("missing", { value: 1 })],
      ["retract", () => retained.retract(1n)],
      ["getFact", () => retained.getFact(1n)],
      ["facts", () => retained.facts()],
      ["findFacts", () => retained.findFacts("baseline")],
      // Lifetime failure deliberately wins over method-specific validation.
      ["run", () => retained.run({ limit: Number.NaN })],
      ["step", () => retained.step()],
      ["halt", () => retained.halt()],
      ["reset", () => retained.reset()],
      ["clear", () => retained.clear()],
      ["getOutput", () => retained.getOutput("t")],
      ["clearOutput", () => retained.clearOutput("t")],
      ["pushInput", () => retained.pushInput("retained")],
    ];

    for (const [method, call] of calls) {
      await assert.rejects(call, assertRetainedProxyError, method);
    }

    const baseline = await pool.do("rules", async (proxy) => proxy.findFacts("baseline"));
    assert.strictEqual(baseline.length, 1, "a retained mutator reached the worker");
  } finally {
    await pool.close();
  }
});

// ---------------------------------------------------------------------------
// E-012: same-pool reentrancy is rejected consistently instead of deadlocking
// ---------------------------------------------------------------------------
test("E-012 same-pool do evaluate and close reject inside callbacks", { timeout: 5_000 }, async () => {
  for (const threads of [1, 2]) {
    const pool = await EnginePool.create([{ name: "rules", source: "" }], { threads });
    try {
      const result = await pool.do("rules", async (proxy) => {
        await assert.rejects(
          () => pool.do("rules", async () => "nested"),
          REENTRANT_ERROR,
          `nested do with threads=${threads}`,
        );
        await assert.rejects(
          () => pool.evaluate("rules", {}),
          REENTRANT_ERROR,
          `nested evaluate with threads=${threads}`,
        );
        await assert.rejects(
          () => pool.close(),
          REENTRANT_ERROR,
          `nested close with threads=${threads}`,
        );

        await proxy.reset();
        await proxy.assertFact("outer-still-valid", threads);
        return (await proxy.findFacts("outer-still-valid")).length;
      });
      assert.strictEqual(result, 1);
    } finally {
      await pool.close();
    }
  }
});

test("E-012 returned thenable assimilation preserves same-pool reentrancy guards", async () => {
  const pool = await EnginePool.create([{ name: "rules", source: "" }], { threads: 1 });
  let nestedOutcome!: Promise<{ status: "fulfilled" } | { status: "rejected"; error: unknown }>;

  try {
    const value = await pool.do("rules", () => ({
      then(resolve: (value: string) => void): void {
        nestedOutcome = pool.evaluate("rules", {}).then(
          () => ({ status: "fulfilled" as const }),
          (error: unknown) => ({ status: "rejected" as const, error }),
        );
        resolve("callback result");
      },
    }) as Promise<string>);

    assert.strictEqual(value, "callback result");
    const outcome = await nestedOutcome;
    assert.strictEqual(outcome.status, "rejected");
    if (outcome.status === "rejected") {
      assert.match(String(outcome.error), REENTRANT_ERROR);
    }

    const reused = await pool.do("rules", async (proxy) => {
      await proxy.reset();
      await proxy.assertFact("thenable-reuse", 1);
      return (await proxy.findFacts("thenable-reuse")).length;
    });
    assert.strictEqual(reused, 1);
  } finally {
    await pool.close();
  }
});

test("E-012 a do callback may use a distinct EnginePool", async () => {
  const outerPool = await EnginePool.create([{ name: "rules", source: "" }], { threads: 1 });
  const innerPool = await EnginePool.create([{ name: "rules", source: "" }], { threads: 1 });

  try {
    const result = await outerPool.do("rules", async (outerProxy) => {
      await outerProxy.reset();
      return innerPool.do("rules", async (innerProxy) => {
        await innerProxy.reset();
        await innerProxy.assertFact("cross-pool", 1);
        return (await innerProxy.findFacts("cross-pool")).length;
      });
    });
    assert.strictEqual(result, 1);
  } finally {
    await Promise.all([outerPool.close(), innerPool.close()]);
  }
});

// ---------------------------------------------------------------------------
// E-012: seeded scheduling stress (fixed seed is included in every failure)
// ---------------------------------------------------------------------------
function makePrng(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (Math.imul(state, 1_664_525) + 1_013_904_223) >>> 0;
    return state;
  };
}

test("E-012 seeded randomized awaits never mix callback sessions", { timeout: 20_000 }, async () => {
  const seed = 0x1350cafe;
  const random = makePrng(seed);

  for (const threads of [1, 2, 4]) {
    const pool = await EnginePool.create([{ name: "rules", source: "" }], { threads });
    try {
      for (let round = 0; round < 3; round++) {
        const schedules = Array.from({ length: 16 }, (_, index) => ({
          id: round * 100 + index,
          afterReset: 1 + (random() % 3),
          afterAssert: 1 + (random() % 3),
        }));

        const observed = await Promise.all(
          schedules.map((schedule, index) =>
            pool.do("rules", async (proxy) => {
              await proxy.reset();
              await yieldTurns(schedule.afterReset);
              await proxy.assertFact("lease-session", schedule.id);
              await yieldTurns(schedule.afterAssert);
              const sessions = await proxy.findFacts("lease-session");
              return sessions.map((fact) => fact.fields[0]);
            }).catch((error: unknown) => {
              throw new Error(
                `seed=0x${seed.toString(16)} threads=${threads} round=${round} index=${index} rejected`,
                { cause: error },
              );
            }),
          ),
        );

        for (let index = 0; index < schedules.length; index++) {
          assert.deepStrictEqual(
            observed[index],
            [schedules[index].id],
            `seed=0x${seed.toString(16)} threads=${threads} round=${round} index=${index}`,
          );
        }
      }
    } finally {
      await pool.close();
    }
  }
});
