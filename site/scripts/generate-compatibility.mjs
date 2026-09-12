import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

// Always regenerate: production builds must never publish a cached observation.
const root = fileURLToPath(new URL("../../", import.meta.url));
const result = spawnSync(
  "uv",
  [
    "run",
    "--locked",
    "--project",
    "tools/ferric-tools",
    "python",
    "-m",
    "ferric_tools.compat.site_report",
  ],
  { cwd: root, stdio: "inherit" },
);
if (result.error) {
  console.error(`Could not generate compatibility results: ${result.error}`);
}
process.exit(result.status ?? 1);
