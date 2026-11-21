// Shared Playwright setup (#1020): the browser install, the dev server, the light and dark theme and the
// output folder. `scripts/fixtures.mjs` uses it for component fixtures; whole-app shots (#1109) call
// `withBrowser` with the `app` target (port 1420) instead of the `fixtures` one.
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { resolve } from "node:path";
import { chromium } from "playwright";

export const THEMES = ["light", "dark"];
/** Where the pictures go (git-ignored). */
export const OUT_DIR = resolve("tmp/shots");

/** The dev servers of `angular.json`: component fixtures and the app. */
export const TARGETS = {
  fixtures: { name: "poimen:fixtures", port: 1421 },
  app: { name: "poimen:serve", port: 1420 },
};

async function reachable(url) {
  try {
    return (await fetch(url)).ok;
  } catch {
    return false;
  }
}

/** Installs Chromium when this Playwright version has none (a no-op when it is there). */
export async function ensureBrowser() {
  const { existsSync } = await import("node:fs");
  if (existsSync(chromium.executablePath())) return;
  console.error("Installing Chromium for Playwright…");
  await new Promise((done, fail) => {
    const p = spawn("pnpm", ["exec", "playwright", "install", "chromium"], { stdio: "inherit" });
    p.on("exit", (code) => (code === 0 ? done() : fail(new Error(`playwright install: ${code}`))));
  });
}

/** Ends `pnpm` and the `ng` it started (the process group; `taskkill` on Windows). */
function killTree(child) {
  if (!child.pid) return;
  if (process.platform === "win32") spawn("taskkill", ["/pid", String(child.pid), "/T", "/F"]);
  else process.kill(-child.pid, "SIGTERM");
}

/** Starts `ng run <target>` unless it already answers; resolves with the URL and a stop function. */
export async function ensureServer(target) {
  const url = `http://localhost:${target.port}/`;
  if (await reachable(url)) return { url, stop: async () => {} };
  const child = spawn("pnpm", ["exec", "ng", "run", target.name, "--port", String(target.port)], {
    stdio: ["ignore", "ignore", "inherit"],
    detached: process.platform !== "win32",
  });
  let exited = false;
  child.on("exit", () => (exited = true));
  for (let i = 0; i < 600; i++) {
    if (await reachable(url)) return { url, stop: async () => killTree(child) };
    if (exited) throw new Error(`ng run ${target.name} exited`);
    await new Promise((r) => setTimeout(r, 200));
  }
  killTree(child);
  throw new Error(`ng run ${target.name} did not answer on ${url}`);
}

/**
 * Runs `fn({ open, url })` with a headless Chromium and the dev server of `target`. `open(path, theme,
 * viewport)` returns a page in that theme (`prefers-color-scheme` and the `app-dark` class of HxTheme).
 */
export async function withBrowser(target, fn) {
  await ensureBrowser();
  const server = await ensureServer(target);
  const browser = await chromium.launch();
  const open = async (path, theme, viewport = { width: 900, height: 600 }) => {
    const context = await browser.newContext({ colorScheme: theme, viewport });
    const page = await context.newPage();
    page.on("pageerror", (err) => console.error(`page error: ${err.message}`));
    await page.goto(new URL(path, server.url).href);
    return page;
  };
  try {
    return await fn({ open, url: server.url });
  } finally {
    await browser.close();
    await server.stop();
  }
}

/** The output folder, created. */
export function outDir() {
  mkdirSync(OUT_DIR, { recursive: true });
  return OUT_DIR;
}
