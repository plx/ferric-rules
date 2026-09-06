import { FerricRuntimeError } from "./types";

/**
 * Shared runtime validation for public run/evaluate limits.
 */

function invalidLimit(context: string): never {
  throw new TypeError(
    `${context}: 'limit' must be a finite non-negative integer in the safe integer range`,
  );
}

/**
 * Validate the `run()` limit contract:
 * - `undefined` / `null` => unlimited
 * - `0` => zero firings
 * - positive integers => bounded run
 */
export function normalizeRunLimit(
  limit: unknown,
  context: string,
): number | undefined | null {
  if (limit === undefined || limit === null) {
    return limit;
  }

  if (
    typeof limit !== "number" ||
    !Number.isFinite(limit) ||
    !Number.isSafeInteger(limit) ||
    limit < 0
  ) {
    invalidLimit(context);
  }

  return limit;
}

/**
 * Validate the `evaluate()` limit contract:
 * - `undefined` / `null` / `0` => unlimited
 * - positive integers => bounded run
 */
export function normalizeEvaluateLimit(
  limit: unknown,
  context: string,
): number | undefined {
  if (limit === undefined || limit === null || limit === 0) {
    return undefined;
  }

  return normalizeRunLimit(limit, context) as number;
}

/** Preserve exact progress across worker batches instead of silently rounding. */
export function addFiredCount(total: number, chunk: number): number {
  const result = total + chunk;
  if (!Number.isSafeInteger(chunk) || chunk < 0 || !Number.isSafeInteger(result)) {
    throw new FerricRuntimeError("fired count exceeds JavaScript safe integer range");
  }
  return result;
}
