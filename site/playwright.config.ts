import { defineConfig, devices } from "@playwright/test";

const basePath: string = "/ferric-rules";
const normalizedBasePath = basePath === "/" ? "" : basePath;
const port = process.env.SITE_TEST_PORT ?? "4321";
const origin = `http://127.0.0.1:${port}`;
const localSiteUrl = `${origin}${normalizedBasePath}/`;
const dotReporter = ["dot"] as const;
const htmlReporter = ["html", { open: "never" }] as const;
const listReporter = ["list"] as const;

export default defineConfig({
  testDir: "./tests",
  fullyParallel: true,
  timeout: 30_000,
  expect: {
    timeout: 5_000,
  },
  reporter: process.env.CI
    ? [dotReporter, htmlReporter]
    : [listReporter, htmlReporter],
  use: {
    baseURL: origin,
    trace: "on-first-retry",
  },
  webServer: {
    // Astro 7 can run preview as a detached background daemon in agent- and
    // CI-like environments, but Playwright requires this child process to stay
    // attached. The preview script uses Vite's foreground server for the exact
    // production artifact that is deployed.
    command: `npm run build && npm run preview -- --host 127.0.0.1 --port ${port}`,
    url: localSiteUrl,
    reuseExistingServer: !process.env.CI,
    // A clean build compiles Ferric and runs every probe in both engines.
    timeout: 900_000,
  },
  projects: [
    {
      name: "mobile",
      use: {
        browserName: "chromium",
        viewport: { width: 390, height: 844 },
        deviceScaleFactor: 3,
        isMobile: true,
        hasTouch: true,
      },
    },
    {
      name: "tablet",
      use: {
        browserName: "chromium",
        viewport: { width: 820, height: 1180 },
        deviceScaleFactor: 2,
        isMobile: true,
        hasTouch: true,
      },
    },
    {
      name: "desktop",
      use: {
        browserName: "chromium",
        ...devices["Desktop Chrome"],
        viewport: { width: 1440, height: 1000 },
      },
    },
  ],
});
