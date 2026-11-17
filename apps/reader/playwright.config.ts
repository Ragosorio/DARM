import { defineConfig, devices } from "@playwright/test";

/**
 * End-to-end tests run against the production build of the reader
 * (`vite build` + `vite preview`), in three browser engines, with books that
 * are compiled from source by the real `darm` compiler in `e2e/global-setup.ts`.
 */
export default defineConfig({
  testDir: "./e2e",
  outputDir: "./test-results",
  globalSetup: "./e2e/global-setup.ts",
  timeout: 60_000,
  expect: { timeout: 10_000, toHaveScreenshot: { maxDiffPixelRatio: 0.02, animations: "disabled" } },
  fullyParallel: true,
  workers: 3,
  retries: 0,
  reporter: [["list"], ["json", { outputFile: "e2e/results/report.json" }]],
  snapshotPathTemplate: "{testDir}/snapshots/{projectName}/{arg}{ext}",
  use: {
    baseURL: "http://127.0.0.1:4173",
    viewport: { width: 1280, height: 800 },
    deviceScaleFactor: 1,
    colorScheme: "light",
    trace: "retain-on-failure",
  },
  webServer: {
    command: "npm run build && npm run preview -- --host 127.0.0.1",
    url: "http://127.0.0.1:4173",
    reuseExistingServer: true,
    timeout: 180_000,
  },
  projects: [
    // System Chrome: no browser download needed, and it is what most readers use.
    { name: "chromium", use: { ...devices["Desktop Chrome"], channel: "chrome", viewport: { width: 1280, height: 800 }, deviceScaleFactor: 1 } },
    // WebKit is the engine of Safari, of iOS, and of the macOS desktop shell.
    { name: "webkit", use: { ...devices["Desktop Safari"], viewport: { width: 1280, height: 800 }, deviceScaleFactor: 1 } },
    { name: "firefox", use: { ...devices["Desktop Firefox"], viewport: { width: 1280, height: 800 }, deviceScaleFactor: 1 } },
    // A phone: single-page mode, touch input, safe areas.
    { name: "phone", use: { ...devices["iPhone 14"], browserName: "webkit" }, testMatch: /(phone|read-through|interactive)\.spec\.ts/ },
  ],
});
