// Component fixtures (#1020): `pnpm fixtures:list` prints the ids, `pnpm fixtures:shot <id>` writes
// `tmp/shots/<id>.<theme>.png` for the light and the dark theme without Tauri. `--all` shoots every fixture.
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { TARGETS, THEMES, outDir, withBrowser } from "./browser/playwright.mjs";

const [command, ...rest] = process.argv.slice(2);
const wanted = rest.filter((a) => !a.startsWith("--"));

async function list({ open }) {
  const page = await open("/?list", "light");
  await page.waitForFunction(() => document.body.dataset.fixtureReady === "list");
  return page.evaluate(() => window.__fixtures);
}

async function main() {
  if (command !== "list" && command !== "shot") {
    console.error("usage: fixtures.mjs list | shot <id>... | shot --all");
    return 2;
  }
  if (command === "shot" && !wanted.length && !rest.includes("--all")) {
    console.error("usage: fixtures.mjs shot <id>... | shot --all  (ids: pnpm fixtures:list)");
    return 2;
  }
  return withBrowser(TARGETS.fixtures, async (env) => {
    const all = await list(env);
    if (command === "list") {
      for (const f of all) console.log(`${f.id}\t${f.title}`);
      return 0;
    }
    const ids = rest.includes("--all") ? all.map((f) => f.id) : wanted;
    const unknown = ids.filter((id) => !all.some((f) => f.id === id));
    if (unknown.length) {
      console.error(`unknown fixture: ${unknown.join(", ")} (pnpm fixtures:list)`);
      return 1;
    }
    const out = outDir();
    for (const id of ids) {
      const { viewport } = all.find((f) => f.id === id);
      for (const theme of THEMES) {
        const page = await env.open(`/?fixture=${encodeURIComponent(id)}`, theme, viewport);
        await page.waitForFunction(() => document.body.dataset.fixtureReady);
        const state = await page.evaluate(() => document.body.dataset.fixtureReady);
        if (state !== id) throw new Error(`fixture ${id} did not render (${state})`);
        const file = join(out, `${id}.${theme}.png`);
        mkdirSync(dirname(file), { recursive: true });
        await page.screenshot({ path: file });
        await page.context().close();
        console.log(file);
      }
    }
    return 0;
  });
}

main().then(
  (code) => process.exit(code),
  (err) => {
    console.error(err.message);
    process.exit(1);
  },
);
