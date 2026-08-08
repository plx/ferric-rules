"use strict";

function formatCause(error) {
  return String(error);
}

function detectLibcFamily() {
  const detector = require("detect-libc");
  if (typeof detector?.familySync !== "function") {
    throw new Error("detect-libc familySync export is unavailable");
  }
  const family = detector.familySync();
  if (family === "glibc") return "glibc";
  if (family === "musl") return "musl";
  return family;
}

function formatRuntimeTarget(runtime) {
  if (runtime.platform === "linux") {
    const abi =
      runtime.libc === "glibc"
        ? "gnu"
        : runtime.libc === "musl"
          ? "musl"
          : "unknown";
    return `${runtime.platform}-${runtime.arch}-${abi}`;
  }

  const abi = runtime.platform === "win32" ? "-msvc" : "";
  return `${runtime.platform}-${runtime.arch}${abi}`;
}

function detectRuntimeTarget({
  platform = process.platform,
  arch = process.arch,
  detectLibc = detectLibcFamily,
} = {}) {
  if (platform !== "linux") {
    const runtime = { platform, arch, libc: undefined, error: undefined };
    return { ...runtime, id: formatRuntimeTarget(runtime) };
  }

  let libc = null;
  let error;
  try {
    const detected = detectLibc();
    if (detected === "glibc" || detected === "musl") {
      libc = detected;
    } else if (detected !== null) {
      error = new Error(
        `detect-libc returned unsupported family ${String(detected)}`,
      );
    }
  } catch (cause) {
    error = cause;
  }

  const runtime = { platform, arch, libc, error };
  return { ...runtime, id: formatRuntimeTarget(runtime) };
}

function targetMatchesRuntime(target, runtime) {
  if (target.platform !== runtime.platform || target.arch !== runtime.arch) {
    return false;
  }
  if (runtime.platform !== "linux") return target.libc === undefined;
  return (
    (runtime.libc === "glibc" || runtime.libc === "musl") &&
    Array.isArray(target.libc) &&
    target.libc.length === 1 &&
    target.libc[0] === runtime.libc
  );
}

function selectDeclaredTarget(targets, runtime) {
  const detected = formatRuntimeTarget(runtime);
  const supported = targets.map((target) => target.id).join(", ");
  const matches = targets.filter((target) =>
    targetMatchesRuntime(target, runtime),
  );

  if (matches.length === 0) {
    const detectionContext = runtime.error
      ? ` Libc detection cause: ${formatCause(runtime.error)}.`
      : "";
    throw new Error(
      `[ferric] Unsupported native target ${detected}. ` +
        `Supported targets: ${supported}.${detectionContext}`,
      { cause: runtime.error },
    );
  }
  if (matches.length !== 1) {
    const matched = matches.map((target) => target.id).join(", ");
    throw new Error(
      `[ferric] Ambiguous native target ${detected}: matched ${matched}. ` +
        `Supported targets: ${supported}.`,
    );
  }
  return matches[0];
}

module.exports = {
  detectRuntimeTarget,
  formatRuntimeTarget,
  selectDeclaredTarget,
  targetMatchesRuntime,
};
