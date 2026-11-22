// Helpers of the UI tests (#1156). The step helpers (`ready`, `openPane`, `terminalShows`) are the
// ones of the screenshots (`scripts/screenshots/views.mjs`), not copies.
import { expect, type Page } from "@playwright/test";
import { TIME } from "../scripts/screenshots/views.mjs";

export { button, openPane, ready, terminalShows } from "../scripts/screenshots/views.mjs";

/** One call of the app to the backend (`window.__demoCalls`). */
export interface Call {
  command: string;
  args?: Record<string, unknown>;
}

/** Collects what the page reports as broken: an uncaught error or a `console.error`. */
export function watchErrors(page: Page): () => string[] {
  const errors: string[] = [];
  page.on("pageerror", (err) => errors.push(`pageerror: ${err.message}`));
  page.on("console", (msg) => {
    if (msg.type() === "error") errors.push(`console.error: ${msg.text()}`);
  });
  return () => errors;
}

/** Saved UI state every test starts from (a wider sidebar, as the screenshots). */
async function startFrom(page: Page): Promise<() => string[]> {
  const errors = watchErrors(page);
  await page.clock.setFixedTime(Date.parse(TIME));
  await page.addInitScript(
    (state) => localStorage.setItem("poimen.ui-state", state),
    JSON.stringify({ sidebarWidth: 340 }),
  );
  return errors;
}

/** Opens the app on the demo variant (canned data) with the fixed clock. */
export async function openDemo(page: Page): Promise<() => string[]> {
  const errors = await startFrom(page);
  await page.goto("/");
  return errors;
}

/** Opens the app on the fake variant: it answers only `answers` (fixed values by command). */
export async function openFake(
  page: Page,
  answers: Record<string, unknown>,
): Promise<() => string[]> {
  const errors = await startFrom(page);
  await page.addInitScript((fake) => {
    window.__demoFake = fake;
  }, answers);
  await page.goto("/?backend=fake");
  return errors;
}

/** The commands the app called so far, in order. */
export const calls = (page: Page): Promise<Call[]> =>
  page.evaluate(() => (window as unknown as { __demoCalls?: Call[] }).__demoCalls ?? []);

/** Forgets the calls so far (a test asserts what one action called). */
export const clearCalls = (page: Page): Promise<void> =>
  page.evaluate(() => {
    (window as unknown as { __demoCalls?: Call[] }).__demoCalls = [];
  });

/** The commands the demo or fake had no answer for. */
export const failures = (page: Page): Promise<string[]> =>
  page.evaluate(() => (window as unknown as { __demoFailures?: string[] }).__demoFailures ?? []);

/** Fails with the names when a command had no answer. */
export async function expectNoFailures(page: Page): Promise<void> {
  expect([...new Set(await failures(page))]).toEqual([]);
}
