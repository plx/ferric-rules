/**
 * Loads the native napi-rs addon and re-exports its public surface.
 *
 * The native addon is built by napi-rs into one of two locations:
 *
 * 1. Development (monorepo): `crates/ferric-napi/` after running `napi build`.
 * 2. Installed package: `native/` directory bundled in `packages/ferric/`.
 *
 * We attempt the bundled path first, then fall back to the monorepo development
 * path. This lets the package work correctly both when installed from npm and
 * when used locally within the workspace.
 *
 * The native module must be loaded via `require()` because napi-rs produces
 * a CommonJS module that binds a `.node` native addon.
 */

import { resolve } from "node:path";

// ---------------------------------------------------------------------------
// Native class type declarations
// (The real runtime types come from the loaded native module; these
// declarations let TypeScript understand the shape without importing .d.ts
// files that may not exist during type-checking in CI.)
// ---------------------------------------------------------------------------

/** Native FerricSymbol constructor interface. */
export interface NativeFerricSymbolConstructor {
  new (value: string): NativeFerricSymbol;
}

/** A native FerricSymbol instance. */
export interface NativeFerricSymbol {
  readonly value: string;
  toString(): string;
  valueOf(): string;
}

/** Native Engine constructor interface. */
export interface NativeEngineConstructor {
  new (options?: { strategy?: number; encoding?: number; maxCallDepth?: number }): NativeEngine;
  fromSource(source: string, options?: { strategy?: number; encoding?: number; maxCallDepth?: number }): NativeEngine;
  fromSnapshot(data: Buffer, format?: number): NativeEngine;
  fromSnapshotFile(path: string, format?: number): NativeEngine;
}

/** Shape of a native Engine instance. */
export interface NativeEngine {
  load(source: string): void;
  loadFile(path: string): void;
  assertString(source: string): number[];
  assertFact(relation: string, ...fields: unknown[]): number;
  assertTemplate(templateName: string, slots: Record<string, unknown>): number;
  retract(factId: number): void;
  getFact(factId: number): unknown | null;
  facts(): unknown[];
  findFacts(relation: string): unknown[];
  getFactSlot(factId: number, slotName: string): unknown;
  run(limit?: number): { rulesFired: number; haltReason: number };
  step(): { ruleName: string } | null;
  halt(): void;
  reset(): void;
  clear(): void;
  readonly factCount: number;
  readonly isHalted: boolean;
  readonly agendaSize: number;
  readonly currentModule: string;
  readonly focus: string | null;
  readonly focusStack: string[];
  rules(): Array<{ name: string; salience: number }>;
  templates(): string[];
  modules(): string[];
  getGlobal(name: string): unknown | null;
  setFocus(moduleName: string): void;
  pushFocus(moduleName: string): void;
  getOutput(channel: string): string | null;
  clearOutput(channel: string): void;
  pushInput(line: string): void;
  readonly diagnostics: string[];
  clearDiagnostics(): void;
  serialize(format?: number): Buffer;
  saveSnapshot(path: string, format?: number): void;
  close(): void;
  [Symbol.dispose](): void;
}

// ---------------------------------------------------------------------------
// Native module loading
// ---------------------------------------------------------------------------

// eslint-disable-next-line @typescript-eslint/no-require-imports
const requireFn: NodeRequire = require;

function loadNativeModule(): Record<string, unknown> {
  const thisDir = __dirname;

  // Path 1: bundled native directory (installed package).
  // __dirname is packages/ferric/dist/, so ".." goes to packages/ferric/.
  const bundledPath = resolve(thisDir, "..", "native", "index.js");
  // Path 2: monorepo development build output.
  // __dirname is packages/ferric/dist/, so "../../.." goes to the workspace root.
  const developmentPath = resolve(thisDir, "..", "..", "..", "crates", "ferric-napi", "index.js");

  const attempts: string[] = [bundledPath, developmentPath];

  let lastError: unknown;
  for (const path of attempts) {
    try {
      return requireFn(path) as Record<string, unknown>;
    } catch (err) {
      lastError = err;
    }
  }

  // Both paths failed. Return an empty stub so that pure type-checking
  // and import of type-only exports don't crash. Runtime calls will fail
  // naturally when Engine/FerricSymbol are undefined.
  const errMsg =
    `[ferric] Could not load native addon. ` +
    `Tried: ${attempts.join(", ")}. ` +
    `Last error: ${String(lastError)}`;

  if (process.env["NODE_ENV"] !== "test") {
    // Emit a warning rather than throwing, so that type-only imports work.
    process.emitWarning(errMsg, "FerricNativeLoadWarning");
  }

  return {};
}

const nativeModule = loadNativeModule();

/**
 * The native Engine class exported by the napi-rs addon.
 *
 * This is the synchronous, thread-affine engine. All methods execute on
 * the calling thread. Use EngineHandle for async worker-backed access.
 */
export const Engine = nativeModule["Engine"] as NativeEngineConstructor | undefined;

/**
 * The native FerricSymbol class exported by the napi-rs addon.
 *
 * Construct explicit CLIPS symbols with `new FerricSymbol("foo")`.
 * Plain strings are mapped to CLIPS *strings* (quoted), not symbols.
 */
export const FerricSymbol = nativeModule["FerricSymbol"] as NativeFerricSymbolConstructor | undefined;

export default nativeModule;
