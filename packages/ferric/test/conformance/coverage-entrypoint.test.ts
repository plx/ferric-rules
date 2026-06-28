/**
 * Aggregate coverage entrypoint.
 *
 * Node's default test isolation runs each test file in a separate process. That
 * is useful for normal testing, but branch coverage for shared modules becomes
 * artificially fragmented because each isolated process imports the same
 * binding files and only exercises that file's local branch subset.
 *
 * The coverage script runs this single entrypoint instead, so the same
 * conformance tests execute in one test process while worker-thread coverage is
 * still collected from real EngineHandle/EnginePool workers.
 */

import "./package/additional.test.ts";
import "./package/coverage-entrypoint-manifest.test.ts";
import "./package/loader-paths.test.ts";
import "./package/package-smoke.test.ts";
import "./package/public-api-property.test.ts";
import "./package/wire-property.test.ts";
import "./package/worker-entrypoint-guards.test.ts";

import "./runtime/pool/additional.test.ts";
import "./runtime/pool/cancellation.test.ts";
import "./runtime/pool/close-semantics.test.ts";
import "./runtime/pool/create-cleanup.test.ts";
import "./runtime/pool/pool-internals.test.ts";
import "./runtime/pool/pool-smoke.test.ts";
import "./runtime/pool/protocol-direct.test.ts";
import "./runtime/pool/run-limit.test.ts";
import "./runtime/pool/thread-default.test.ts";
import "./runtime/pool/wire-conversion.test.ts";

import "./runtime/sync/dispose.test.ts";
import "./runtime/sync/engine-smoke.test.ts";
import "./runtime/sync/error-mapping.test.ts";
import "./runtime/sync/integer-boundary.test.ts";
import "./runtime/sync/lifecycle.test.ts";
import "./runtime/sync/native-api-completion.test.ts";
import "./runtime/sync/run-limit.test.ts";
import "./runtime/sync/snapshot.test.ts";

import "./runtime/worker/additional.test.ts";
import "./runtime/worker/create-validation.test.ts";
import "./runtime/worker/error-mapping.test.ts";
import "./runtime/worker/handle-api-completion.test.ts";
import "./runtime/worker/handle-internals.test.ts";
import "./runtime/worker/handle-smoke.test.ts";
import "./runtime/worker/protocol-direct.test.ts";
import "./runtime/worker/run-limit.test.ts";
import "./runtime/worker/wire-conversion.test.ts";
