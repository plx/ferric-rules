import * as assert from "node:assert/strict";
import { test } from "node:test";

import { findNonLfTextFiles } from "../../../../../scripts/node-package-text.mjs";

test("G-001 release package text accepts LF-only content", () => {
  assert.deepStrictEqual(
    findNonLfTextFiles([
      { path: "package.json", contents: '{\n  "name": "example"\n}\n' },
      { path: "native/index.js", contents: '"use strict";\n' },
    ]),
    [],
  );
});

test("G-001 release package text rejects carriage returns", () => {
  assert.deepStrictEqual(
    findNonLfTextFiles([
      { path: "package.json", contents: '{\r\n  "name": "example"\r\n}\r\n' },
      { path: "native/index.js", contents: '"use strict";\n' },
      { path: "native/targets.json", contents: "[]\r" },
    ]),
    ["package.json", "native/targets.json"],
  );
});
