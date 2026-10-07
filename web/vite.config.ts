import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [react()],
  server: {
    // `ripplepath serve` hosts the API; the dev server proxies to it so the UI uses same-origin
    // requests in development exactly as in production.
    proxy: { "/api": "http://127.0.0.1:7878" },
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test/setup.ts"],
    include: ["src/**/*.test.{ts,tsx}"],
  },
});
