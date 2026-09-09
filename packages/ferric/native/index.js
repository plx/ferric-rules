"use strict";

const { existsSync } = require("node:fs");
const { resolve } = require("node:path");

const targets = require("./targets.json");
const packageMetadata = require("../package.json");
const {
  detectRuntimeTarget,
  selectDeclaredTarget,
} = require("./runtime-target");

const expectedVersion = packageMetadata.version;
const detectedRuntime = detectRuntimeTarget();
const detectedTarget = selectDeclaredTarget(targets, detectedRuntime);

function formatCause(error) {
  return String(error);
}

function verifyBindingVersion(binding, source) {
  if (
    binding === null ||
    typeof binding !== "object" ||
    typeof binding.nativePackageVersion !== "function"
  ) {
    throw new Error(
      `[ferric] Native addon from ${source} does not expose ` +
        "`nativePackageVersion()`. Reinstall matching Ferric packages.",
    );
  }

  const actualVersion = binding.nativePackageVersion();
  if (actualVersion !== expectedVersion) {
    throw new Error(
      `[ferric] Native addon version mismatch: JavaScript package is ` +
        `${expectedVersion}, but ${source} contains ${String(actualVersion)}. ` +
        "Install @ferric-rules/node and its native package at the same version.",
    );
  }
}

function loadNativeBinding() {
  // A source checkout keeps the development binary in crates/ferric-rules-napi.
  // This path cannot exist in a normal npm consumer install.
  const developmentBinary = resolve(
    __dirname,
    "..",
    "..",
    "..",
    "crates",
    "ferric-rules-napi",
    "ferric-rules-napi.node",
  );
  let developmentError;
  if (existsSync(developmentBinary)) {
    try {
      const binding = require(developmentBinary);
      verifyBindingVersion(binding, developmentBinary);
      return binding;
    } catch (error) {
      developmentError = error;
    }
  }

  try {
    const nativeMetadata = require(
      `${detectedTarget.packageName}/package.json`,
    );
    if (nativeMetadata.version !== expectedVersion) {
      throw new Error(
        `package metadata reports ${String(nativeMetadata.version)}`,
      );
    }

    const binding = require(detectedTarget.packageName);
    verifyBindingVersion(binding, detectedTarget.packageName);
    return binding;
  } catch (error) {
    const developmentContext = developmentError
      ? ` The development addon at ${developmentBinary} also failed: ` +
        `${formatCause(developmentError)}.`
      : "";
    throw new Error(
      `[ferric] Could not load ${detectedTarget.packageName}@${expectedVersion} ` +
        `for ${detectedTarget.id}. Ensure npm optional dependencies were not ` +
        `omitted and reinstall @ferric-rules/node.${developmentContext} ` +
        `Platform package cause: ${formatCause(error)}`,
      { cause: error },
    );
  }
}

const nativeBinding = loadNativeBinding();
// Capture the native class method moved off the prototype during addon init.
// Calling with an Engine receiver keeps V8's native receiver validation before
// napi-rs creates a shared Rust reference. Foreign wrapped objects cannot pass.
const nativeContinueRun = nativeBinding.__continueRun;
const continueRun = (engine, limit) => nativeContinueRun.call(engine, limit);

// napi-rs class instances (FerricSymbol) lose their native pointer when passed
// through Vec<JsUnknown> extraction. Convert them to tagged plain objects that
// the Rust boundary recognizes. This shape is distinct from the worker wire
// representation in packages/ferric/src/wire.ts.
const FerricSymbolClass = nativeBinding.FerricSymbol;
const byteLexemeClasses = ["FerricStringBytes", "FerricSymbolBytes", "FerricInstanceName"];

function marshalValue(value, depth = 0) {
  if (value === null || value === undefined) return value;
  for (const kind of byteLexemeClasses) {
    const ctor = nativeBinding[kind];
    if (typeof ctor === "function" && value instanceof ctor) {
      return { __ferric_lexeme: kind, bytes: value.bytes };
    }
  }
  if (value instanceof FerricSymbolClass) {
    return { __ferric_symbol: true, value: value.value };
  }
  if (Array.isArray(value)) {
    if (depth >= 128) throw new TypeError("multifield nesting exceeds 128 levels (cyclic values are unsupported)");
    return value.map((item) => marshalValue(item, depth + 1));
  }
  if (typeof value === "object") {
    const tag = Object.getOwnPropertyDescriptor(value, "__type");
    const payload = Object.getOwnPropertyDescriptor(value, "value");
    const bytes = Object.getOwnPropertyDescriptor(value, "bytes");
    if (byteLexemeClasses.includes(tag?.value) && bytes?.value instanceof Uint8Array) {
      return { __ferric_lexeme: tag.value, bytes: bytes.value };
    }
    if (tag?.value === "FerricSymbol" && typeof payload?.value === "string") {
      return { __ferric_symbol: true, value: payload.value };
    }
  }
  return value;
}

function marshalSlots(slots) {
  if (!slots || typeof slots !== "object") return slots;
  const output = {};
  for (const [key, value] of Object.entries(slots)) {
    output[key] = marshalValue(value);
  }
  return output;
}

const originalAssertFact = nativeBinding.Engine.prototype.assertFact;
nativeBinding.Engine.prototype.assertFact = function (relation, ...fields) {
  return originalAssertFact.call(this, relation, fields.map((value) => marshalValue(value)));
};

const originalAssertTemplate = nativeBinding.Engine.prototype.assertTemplate;
nativeBinding.Engine.prototype.assertTemplate = function (templateName, slots) {
  return originalAssertTemplate.call(this, templateName, marshalSlots(slots));
};

module.exports = { ...nativeBinding, __continueRun: continueRun };
