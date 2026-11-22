// UI end-to-end tests (#1156): the Angular app in headless Chromium against the demo build (`ng run
// poimen:serve:demo`, #1109), in the light and the dark theme, with a fixed clock and reduced motion.
// `pnpm e2e` runs them; the variants of the backend are explained in `docs/agents/frontend.md`.
import { defineConfig } from "@playwright/test";

const PORT = 1422;
/** The instant the clock is fixed to (the demo data counts back from it); same as `scripts/screenshots/views.mjs`. */
const TIME = "2026-11-21T10:00:00Z";

const use = {
  baseURL: `http://localhost:${PORT}`,
  viewport: { width: 1280, height: 800 },
  reducedMotion: "reduce" as const,
  trace: "retain-on-failure" as const,
};

export default defineConfig({
  testDir: "e2e",
  outputDir: "tmp/e2e/results",
  reporter: process.env["CI"]
    ? [["list"], ["html", { outputFolder: "tmp/e2e/report", open: "never" }]]
    : [["list"]],
  forbidOnly: !!process.env["CI"],
  retries: process.env["CI"] ? 1 : 0,
  timeout: 30_000,
  expect: { timeout: 10_000 },
  webServer: {
    command: `pnpm exec ng run poimen:serve:demo --port ${PORT}`,
    url: `http://localhost:${PORT}/`,
    reuseExistingServer: !process.env["CI"],
    timeout: 180_000,
  },
  projects: [
    { name: "light", use: { ...use, colorScheme: "light" } },
    { name: "dark", use: { ...use, colorScheme: "dark" } },
  ],
  metadata: { time: TIME },
});
