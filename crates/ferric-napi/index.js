/* auto-generated napi-rs loader — loads the platform-specific .node addon */
const { existsSync } = require("fs");
const { join } = require("path");

const { platform, arch } = process;

let nativeBinding = null;
let localFileExisted = false;
let loadError = null;

// In development: the .node file is in the same directory
const devPath = join(__dirname, "ferric-napi.node");
if (existsSync(devPath)) {
  localFileExisted = true;
  try {
    nativeBinding = require(devPath);
  } catch (e) {
    loadError = e;
  }
}

// Fallback: try platform-specific package (npm install scenario)
if (!nativeBinding) {
  const triples = {
    "darwin-arm64": "@ferric-rules/napi-darwin-arm64",
    "darwin-x64": "@ferric-rules/napi-darwin-x64",
    "linux-x64": "@ferric-rules/napi-linux-x64-gnu",
    "win32-x64": "@ferric-rules/napi-win32-x64-msvc",
  };
  const key = `${platform}-${arch}`;
  const pkg = triples[key];
  if (pkg) {
    try {
      nativeBinding = require(pkg);
    } catch (e) {
      loadError = e;
    }
  }
}

if (!nativeBinding) {
  if (loadError) {
    throw loadError;
  }
  throw new Error(
    `Failed to load native binding for ${platform}-${arch}. ` +
      (localFileExisted
        ? "The .node file existed but could not be loaded."
        : "No .node file found. Run `napi build` first.")
  );
}

// ---------------------------------------------------------------------------
// FerricSymbol marker conversion
// ---------------------------------------------------------------------------
// napi-rs class instances (FerricSymbol) lose their native pointer when
// passed through Vec<JsUnknown> extraction.  Convert them to tagged plain
// objects that the Rust code can recognise via a simple property check.

const FerricSymbolClass = nativeBinding.FerricSymbol;

/**
 * Recursively convert any FerricSymbol instances in a value tree to tagged
 * plain objects: { __ferric_symbol: true, value: string }.
 */
function marshalValue(v) {
  if (v === null || v === undefined) return v;
  if (v instanceof FerricSymbolClass) {
    return { __ferric_symbol: true, value: v.value };
  }
  if (Array.isArray(v)) {
    return v.map(marshalValue);
  }
  return v;
}

/**
 * Marshal all values in a slots object.
 */
function marshalSlots(slots) {
  if (!slots || typeof slots !== "object") return slots;
  const out = {};
  for (const [k, v] of Object.entries(slots)) {
    out[k] = marshalValue(v);
  }
  return out;
}

// ---------------------------------------------------------------------------
// assertFact: rest params + symbol marshalling
// ---------------------------------------------------------------------------
const _origAssertFact = nativeBinding.Engine.prototype.assertFact;
nativeBinding.Engine.prototype.assertFact = function (relation, ...fields) {
  return _origAssertFact.call(this, relation, fields.map(marshalValue));
};

// ---------------------------------------------------------------------------
// assertTemplate: symbol marshalling for slot values
// ---------------------------------------------------------------------------
const _origAssertTemplate = nativeBinding.Engine.prototype.assertTemplate;
nativeBinding.Engine.prototype.assertTemplate = function (templateName, slots) {
  return _origAssertTemplate.call(this, templateName, marshalSlots(slots));
};

module.exports = nativeBinding;
