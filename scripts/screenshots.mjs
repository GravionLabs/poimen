// The guide screenshots (#1109): `pnpm screenshots [name...]` writes `site/public/screenshots/<name>-<theme>.png`
// for the light and the dark theme. The Angular app runs in headless Chromium with the demo backend instead of
// Tauri, a fixed clock, no animation and a fixed window, so two runs give the same images. A command the demo
// backend does not answer fails the run with its name.
import { mkdirSync } from "node:fs";
import { join, resolve } from "node:path";
import { THEMES, TARGETS, withBrowser } from "./browser/playwright.mjs";
import { TIME, VIEWPORT, VIEWS, baseUi } from "./screenshots/views.mjs";

const UI_STATE_KEY = "poimen.ui-state";
const args = process.argv.slice(2);
const outFlag = args.indexOf("--out");
const out = resolve(outFlag >= 0 ? args[outFlag + 1] : "site/public/screenshots");
const wanted = args.filter((a, i) => !a.startsWith("--") && args[i - 1] !== "--out");

async function main() {
  const unknown = wanted.filter((n) => !VIEWS.some((v) => v.name === n));
  if (unknown.length) {
    console.error(`unknown view: ${unknown.join(", ")} (${VIEWS.map((v) => v.name).join(", ")})`);
    return 2;
  }
  const views = wanted.length ? VIEWS.filter((v) => wanted.includes(v.name)) : VIEWS;
  mkdirSync(out, { recursive: true });
  const failures = [];
  const code = await withBrowser(TARGETS.demo, async ({ open }) => {
    for (const view of views) {
      for (const theme of THEMES) {
        const page = await open("/", theme, VIEWPORT, {
          time: Date.parse(TIME),
          reducedMotion: true,
          storage: { [UI_STATE_KEY]: baseUi(view) },
        });
        page.setDefaultTimeout(10_000);
        try {
          await view.run(page);
          // Let the signals settle: the stream bytes, the first paint of the panels.
          await page.evaluate(() => document.fonts.ready);
          await page.waitForTimeout(400);
          const missing = await page.evaluate(() => window.__demoFailures ?? []);
          if (missing.length) {
            failures.push(`${view.name}: no demo answer for ${[...new Set(missing)].join(", ")}`);
            continue;
          }
          const file = join(out, `${view.name}-${theme}.png`);
          await page.screenshot({ path: file, animations: "disabled", caret: "hide" });
          console.log(file);
        } catch (err) {
          failures.push(`${view.name} (${theme}): ${err.message.split("\n")[0]}`);
          // What the page showed when the view failed, for a look (git-ignored `tmp/`).
          mkdirSync("tmp/shots", { recursive: true });
          await page
            .screenshot({ path: `tmp/shots/failed-${view.name}-${theme}.png` })
            .catch(() => {});
        } finally {
          await page
            .context()
            .close()
            .catch(() => {});
        }
      }
    }
    return 0;
  });
  if (failures.length) {
    console.error(`\n${failures.length} failed:\n- ${failures.join("\n- ")}`);
    return 1;
  }
  return code;
}

main().then(
  (code) => process.exit(code),
  (err) => {
    console.error(err.message);
    process.exit(1);
  },
);
