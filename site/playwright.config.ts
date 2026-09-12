import { defineConfig, devices } from "@playwright/test";

const basePath: string = "/ferric-rules";
const normalizedBasePath = basePath === "/" ? "" : basePath;
const localSiteUrl = `http://127.0.0.1:4321${normalizedBasePath}/`;
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
    baseURL: "http://127.0.0.1:4321",
    trace: "on-first-retry",
  },
  webServer: {
    // Astro 7 can run preview as a detached background daemon in agent- and
    // CI-like environments, but Playwright requires this child process to stay
    // attached. The preview script uses Vite's foreground server for the exact
    // production artifact that is deployed.
    command: "npm run build && npm run preview -- --host 127.0.0.1 --port 4321",
    url: localSiteUrl,
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
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
