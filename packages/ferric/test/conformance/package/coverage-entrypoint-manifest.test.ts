/**
 * Coverage-entrypoint exhaustiveness guard.
 *
 * The 100% coverage gate (`npm run test:coverage`) runs a SINGLE process that
 * imports every conformance test via coverage-entrypoint.test.ts. Node otherwise
 * isolates each test file in its own process, which fragments branch coverage of
 * shared src/ modules (native.ts, wire.ts, engine-handle.ts, …). The entrypoint's
 * import list is hand-maintained, so a newly added test file could be silently
 * dropped from the coverage run — masking a real coverage hole or failing the
 * gate opaquely.
 *
 * This guard makes that failure mode loud: every executable conformance test on
 * disk must be imported by the coverage entrypoint, and every import in the
 * entrypoint must resolve to a file that still exists.
 *
 * (This replaces the former hand-maintained double-coverage manifest, whose
 * matrix-ID → file pairings were unverifiable bookkeeping that drifted out of
 * sync with the actual tests.)
 */
import { readFileSync, readdirSync } from "node:fs";
import { resolve, relative } from "node:path";
import { test } from "node:test";
import * as assert from "node:assert/strict";

const conformanceRoot = resolve(__dirname, "..");
const entrypointPath = resolve(conformanceRoot, "coverage-entrypoint.test.ts");

/** Recursively collect every `*.test.ts` file under `dir`. */
function listTestFiles(dir: string): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = resolve(dir, entry.name);
    if (entry.isDirectory()) {
      out.push(...listTestFiles(full));
    } else if (entry.name.endsWith(".test.ts")) {
      out.push(full);
    }
  }
  return out;
}

test("G-002 coverage entrypoint imports every executable conformance test", () => {
  const source = readFileSync(entrypointPath, "utf8");
  const imported = new Set(
    [...source.matchAll(/import\s+"(\.\/[^"]+)"/g)].map((m) =>
      resolve(conformanceRoot, m[1]),
    ),
  );

  const allOnDisk = listTestFiles(conformanceRoot);

  // The type-only suite (types/**) is validated by `tsc` (and now executed by
  // `test:runtime:types`); it is intentionally excluded from the runtime
  // coverage process. Everything else must be wired into the entrypoint.
  const mustBeImported = allOnDisk.filter(
    (f) => f !== entrypointPath && !relative(conformanceRoot, f).startsWith("types/"),
  );

  const missing = mustBeImported
    .filter((f) => !imported.has(f))
    .map((f) => relative(conformanceRoot, f));
  assert.deepStrictEqual(
    missing,
    [],
    "new conformance test files must be imported by coverage-entrypoint.test.ts " +
      "so the 100% coverage gate sees them",
  );

  // Every entrypoint import must resolve to a file that still exists, so a
  // renamed/deleted test cannot leave a dangling import behind.
  const onDiskSet = new Set(allOnDisk);
  const dangling = [...imported]
    .filter((f) => !onDiskSet.has(f))
    .map((f) => relative(conformanceRoot, f));
  assert.deepStrictEqual(
    dangling,
    [],
    "coverage-entrypoint.test.ts imports a test file that no longer exists",
  );
});
