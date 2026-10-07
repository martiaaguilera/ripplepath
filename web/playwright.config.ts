import { defineConfig } from "@playwright/test";

// The smoke test runs against a real `ripplepath serve` (started by the e2e job in CI): the UI is only
// meaningful with the real API, so nothing here is mocked.
export default defineConfig({
  testDir: "e2e",
  timeout: 60_000,
  use: { baseURL: process.env.RIPPLEPATH_URL ?? "http://127.0.0.1:7878" },
  reporter: [["list"]],
});
