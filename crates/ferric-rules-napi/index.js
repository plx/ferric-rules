/*
 * Development entrypoint.
 *
 * Keep the release loader single-sourced under packages/ferric/native so the
 * exact code exercised in the monorepo is also shipped in the npm tarball.
 */
module.exports = require("../../packages/ferric/native/index.js");
