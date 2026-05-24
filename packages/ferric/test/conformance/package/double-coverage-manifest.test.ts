/**
 * Double-coverage manifest.
 *
 * Every conformance-matrix item must have both:
 * - a manual test: an explicit, named scenario with comments explaining the
 *   behavior being protected; and
 * - a property-style test: a generated/table-driven corpus that checks the
 *   same contract across multiple values, modes, or protocol frames.
 *
 * This test keeps that pairing auditable as the matrix changes.
 */
import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { test } from "node:test";
import * as assert from "node:assert/strict";

const conformanceRoot = resolve(__dirname, "..");
const matrixPath = resolve(
  __dirname,
  "../../../../../docs/typescript-binding-conformance-matrix.md",
);

const DOUBLE_COVERAGE: Readonly<Record<string, {
  manual: readonly string[];
  property: readonly string[];
}>> = {
  "N-01": {
    manual: ["runtime/sync/run-limit.test.ts", "runtime/worker/run-limit.test.ts"],
    property: ["runtime/worker/protocol-direct.test.ts", "runtime/pool/protocol-direct.test.ts"],
  },
  "N-02": {
    manual: ["runtime/pool/run-limit.test.ts"],
    property: ["runtime/pool/run-limit.test.ts", "runtime/pool/additional.test.ts"],
  },
  "N-03": {
    manual: ["runtime/pool/cancellation.test.ts"],
    property: ["runtime/pool/pool-internals.test.ts"],
  },
  "N-04": {
    manual: ["runtime/pool/close-semantics.test.ts"],
    property: ["runtime/pool/pool-internals.test.ts"],
  },
  "N-05": {
    manual: ["runtime/worker/wire-conversion.test.ts", "runtime/pool/wire-conversion.test.ts"],
    property: ["runtime/worker/protocol-direct.test.ts", "runtime/pool/protocol-direct.test.ts"],
  },
  "N-06": {
    manual: ["types/api-surface.test.ts", "package/package-smoke.test.ts"],
    property: ["package/public-api-property.test.ts"],
  },

  "A-001": {
    manual: ["types/api-surface.test.ts"],
    property: ["package/public-api-property.test.ts"],
  },
  "A-002": {
    manual: ["types/api-surface.test.ts"],
    property: ["package/public-api-property.test.ts"],
  },
  "A-003": {
    manual: ["types/api-surface.test.ts"],
    property: ["types/api-surface.test.ts"],
  },
  "A-004": {
    manual: ["types/api-surface.test.ts"],
    property: ["package/public-api-property.test.ts"],
  },
  "A-005": {
    manual: ["runtime/sync/dispose.test.ts", "runtime/sync/native-api-completion.test.ts"],
    property: ["package/public-api-property.test.ts"],
  },
  "A-006": {
    manual: ["types/api-surface.test.ts", "runtime/worker/handle-api-completion.test.ts"],
    property: ["package/public-api-property.test.ts"],
  },

  "B-001": {
    manual: ["runtime/sync/engine-smoke.test.ts"],
    property: ["package/loader-paths.test.ts"],
  },
  "B-002": {
    manual: ["runtime/worker/wire-conversion.test.ts", "runtime/pool/wire-conversion.test.ts"],
    property: ["package/wire-property.test.ts", "runtime/pool/protocol-direct.test.ts"],
  },
  "B-003": {
    manual: ["runtime/sync/engine-smoke.test.ts"],
    property: ["runtime/worker/protocol-direct.test.ts"],
  },
  "B-004": {
    manual: ["runtime/worker/wire-conversion.test.ts", "runtime/pool/wire-conversion.test.ts"],
    property: ["runtime/worker/protocol-direct.test.ts", "runtime/pool/protocol-direct.test.ts"],
  },
  "B-005": {
    manual: ["runtime/sync/engine-smoke.test.ts"],
    property: ["package/wire-property.test.ts"],
  },
  "B-006": {
    manual: ["runtime/sync/integer-boundary.test.ts"],
    property: ["package/wire-property.test.ts"],
  },
  "B-007": {
    manual: ["runtime/sync/integer-boundary.test.ts"],
    property: ["runtime/sync/integer-boundary.test.ts"],
  },
  "B-008": {
    manual: ["runtime/sync/engine-smoke.test.ts"],
    property: ["runtime/worker/protocol-direct.test.ts"],
  },
  "B-009": {
    manual: ["runtime/sync/engine-smoke.test.ts"],
    property: ["runtime/worker/handle-api-completion.test.ts"],
  },

  "C-001": {
    manual: ["runtime/sync/error-mapping.test.ts", "runtime/worker/error-mapping.test.ts"],
    property: ["runtime/sync/native-api-completion.test.ts"],
  },
  "C-002": {
    manual: ["runtime/sync/error-mapping.test.ts"],
    property: ["runtime/sync/native-api-completion.test.ts"],
  },
  "C-003": {
    manual: ["runtime/sync/error-mapping.test.ts", "runtime/sync/native-api-completion.test.ts"],
    property: ["runtime/sync/native-api-completion.test.ts"],
  },
  "C-004": {
    manual: ["runtime/worker/error-mapping.test.ts", "runtime/pool/pool-internals.test.ts"],
    property: ["package/wire-property.test.ts", "runtime/worker/handle-internals.test.ts"],
  },
  "C-005": {
    manual: ["runtime/worker/error-mapping.test.ts"],
    property: ["runtime/worker/handle-internals.test.ts", "runtime/pool/pool-internals.test.ts"],
  },

  "D-001": {
    manual: ["runtime/worker/handle-smoke.test.ts", "runtime/worker/handle-api-completion.test.ts"],
    property: ["runtime/worker/handle-api-completion.test.ts", "runtime/worker/protocol-direct.test.ts"],
  },
  "D-002": {
    manual: ["runtime/sync/snapshot.test.ts", "runtime/worker/additional.test.ts"],
    property: ["runtime/worker/additional.test.ts"],
  },
  "D-003": {
    manual: ["runtime/worker/create-validation.test.ts"],
    property: ["runtime/worker/create-validation.test.ts"],
  },
  "D-004": {
    manual: ["runtime/worker/handle-smoke.test.ts"],
    property: ["runtime/worker/run-limit.test.ts"],
  },
  "D-005": {
    manual: ["runtime/worker/additional.test.ts"],
    property: ["runtime/pool/cancellation.test.ts"],
  },
  "D-006": {
    manual: ["runtime/sync/run-limit.test.ts", "runtime/worker/run-limit.test.ts"],
    property: ["runtime/worker/protocol-direct.test.ts", "runtime/pool/pool-internals.test.ts"],
  },
  "D-007": {
    manual: ["runtime/worker/additional.test.ts", "runtime/worker/handle-internals.test.ts"],
    property: ["runtime/worker/handle-internals.test.ts"],
  },

  "E-001": {
    manual: ["runtime/pool/thread-default.test.ts", "runtime/pool/pool-internals.test.ts"],
    property: ["runtime/pool/additional.test.ts"],
  },
  "E-002": {
    manual: ["runtime/pool/pool-smoke.test.ts", "runtime/pool/run-limit.test.ts"],
    property: ["runtime/pool/additional.test.ts", "runtime/pool/protocol-direct.test.ts"],
  },
  "E-003": {
    manual: ["runtime/pool/pool-smoke.test.ts", "runtime/pool/cancellation.test.ts"],
    property: ["runtime/pool/run-limit.test.ts"],
  },
  "E-004": {
    manual: ["runtime/pool/cancellation.test.ts", "runtime/pool/pool-internals.test.ts"],
    property: ["runtime/pool/pool-internals.test.ts"],
  },
  "E-005": {
    manual: ["runtime/pool/cancellation.test.ts"],
    property: ["runtime/pool/protocol-direct.test.ts"],
  },
  "E-006": {
    manual: ["runtime/pool/cancellation.test.ts", "runtime/pool/pool-internals.test.ts"],
    property: ["runtime/pool/pool-internals.test.ts"],
  },
  "E-007": {
    manual: ["runtime/pool/additional.test.ts", "runtime/pool/protocol-direct.test.ts"],
    property: ["runtime/pool/additional.test.ts", "runtime/pool/protocol-direct.test.ts"],
  },
  "E-008": {
    manual: ["runtime/pool/close-semantics.test.ts", "runtime/pool/pool-internals.test.ts"],
    property: ["runtime/pool/pool-internals.test.ts"],
  },
  "E-009": {
    manual: ["runtime/pool/close-semantics.test.ts", "runtime/pool/pool-internals.test.ts"],
    property: ["runtime/pool/pool-internals.test.ts"],
  },

  "F-001": {
    manual: ["runtime/sync/lifecycle.test.ts"],
    property: ["runtime/sync/dispose.test.ts"],
  },
  "F-002": {
    manual: ["runtime/sync/lifecycle.test.ts"],
    property: ["runtime/sync/lifecycle.test.ts"],
  },
  "F-003": {
    manual: ["runtime/worker/handle-smoke.test.ts", "runtime/worker/handle-internals.test.ts"],
    property: ["runtime/worker/handle-internals.test.ts"],
  },
  "F-004": {
    manual: ["runtime/pool/pool-smoke.test.ts", "runtime/pool/pool-internals.test.ts"],
    property: ["runtime/pool/pool-internals.test.ts"],
  },

  "G-001": {
    manual: ["package/package-smoke.test.ts", "package/additional.test.ts"],
    property: ["package/public-api-property.test.ts"],
  },
  "G-002": {
    manual: ["package/loader-paths.test.ts", "package/package-smoke.test.ts"],
    property: ["package/loader-paths.test.ts"],
  },
  "G-003": {
    manual: ["package/package-smoke.test.ts", "types/api-surface.test.ts"],
    property: ["package/public-api-property.test.ts"],
  },
  "G-004": {
    manual: ["package/additional.test.ts", "package/wire-property.test.ts"],
    property: ["package/wire-property.test.ts"],
  },
};

// ---------------------------------------------------------------------------
// G-004 property-style manifest gate: every matrix item has double coverage
// ---------------------------------------------------------------------------
test("G-004 property-style double-coverage manifest matches the conformance matrix", () => {
  const matrix = readFileSync(matrixPath, "utf8");
  const ids = [...new Set(
    [...matrix.matchAll(/`([A-G]-\d{3}|N-\d{2})`/g)].map((match) => match[1]),
  )].sort();

  const manifestIds = Object.keys(DOUBLE_COVERAGE).sort();
  assert.deepStrictEqual(manifestIds, ids);

  for (const id of ids) {
    const entry = DOUBLE_COVERAGE[id];
    assert.ok(entry.manual.length > 0, `${id} must have manual coverage`);
    assert.ok(entry.property.length > 0, `${id} must have property coverage`);

    for (const file of [...entry.manual, ...entry.property]) {
      assert.ok(
        existsSync(resolve(conformanceRoot, file)),
        `${id} references missing test file ${file}`,
      );
    }
  }
});
