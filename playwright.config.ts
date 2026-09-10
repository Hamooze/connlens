import { defineConfig } from "@playwright/test";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { createRequire } from "node:module";

const vite = join(dirname(createRequire(import.meta.url).resolve("vite/package.json")), "bin/vite.js");

// These are browser UI/IPC-contract tests. Native CLI and bundles are checked separately.
export default defineConfig({
  testDir: "./e2e",
  outputDir: process.env.CONNLENS_E2E_OUTPUT ?? join(tmpdir(), "connlens-playwright"),
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  reporter: "list",
  use: {
    baseURL: "http://127.0.0.1:1427",
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
  projects: [
    { name: "macos-webkit", use: { browserName: "webkit", viewport: { width: 360, height: 520 } }, metadata: { platform: "macos" } },
    { name: "windows-chromium", use: { browserName: "chromium", viewport: { width: 390, height: 520 } }, metadata: { platform: "windows" } },
    { name: "linux-webkit", use: { browserName: "webkit", viewport: { width: 390, height: 520 } }, metadata: { platform: "linux" } },
  ],
  webServer: {
    command: `"${process.execPath}" "${vite}" preview --host 127.0.0.1 --port 1427 --strictPort`,
    url: "http://127.0.0.1:1427",
    reuseExistingServer: false,
  },
});
