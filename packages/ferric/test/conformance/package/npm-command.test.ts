import * as assert from "node:assert/strict";
import { test } from "node:test";

import { npmInvocation } from "../../../../../scripts/npm-command.mjs";

test("G-001 launches the Windows npm command shim through a shell", () => {
  assert.deepStrictEqual(npmInvocation("win32"), {
    command: "npm.cmd",
    shell: true,
  });
});

test("G-001 launches npm directly on POSIX platforms", () => {
  assert.deepStrictEqual(npmInvocation("linux"), {
    command: "npm",
    shell: false,
  });
  assert.deepStrictEqual(npmInvocation(), npmInvocation(process.platform));
});
