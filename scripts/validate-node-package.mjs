#!/usr/bin/env node

import { validateNodePackage } from "./node-package-lib.mjs";

const { targets, version } = await validateNodePackage();
console.log(
  `validated @ferric-rules/node@${version} with ${targets.length} ` +
    "version-locked native packages",
);
