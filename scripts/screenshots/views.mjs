// The views of the guide screenshots (#1109): what the app does before each picture. `scripts/screenshots.mjs`
// runs them in light and dark against the demo backend (`src/demo/poimen/`). A step uses what a user
// sees (a button name, a text), so a renamed label fails the run instead of shooting the wrong screen.

/** The instant the clock is fixed to; the demo data counts back from it (`src/demo/poimen/data.ts`). */
export const TIME = "2026-11-21T10:00:00Z";
/** The window of every picture. */
export const VIEWPORT = { width: 1280, height: 800 };

/** Saved UI state (`poimen.ui-state`) every view starts from: a wider sidebar. */
const BASE_UI = { sidebarWidth: 340 };

export const button = (page, name) => page.getByRole("button", { name, exact: true }).first();

/** Waits for the app to show the spaces. */
export async function ready(page) {
  await page.getByText("billing-api").first().waitFor();
}

/** Opens the pane row `name` of the sidebar (a double click pins the tab). */
export async function openPane(page, name, nth = 0) {
  await page.getByText(name, { exact: true }).nth(nth).dblclick();
}

/** Waits until the demo backend pushed the screens of the terminals `ids` (a canvas has no text to look for). */
export async function terminalShows(page, ...ids) {
  await page.waitForFunction(
    (want) =>
      want.every((id) => (document.body.dataset.demoTerminals ?? "").split(" ").includes(id)),
    ids,
  );
}

/** Opens the command palette from an open pane: a focused terminal keeps the keys for itself. */
async function openPalette(page) {
  await page.getByText("Spaces", { exact: true }).first().click();
  await page.keyboard.press("ControlOrMeta+Shift+P");
}

/** The sidebar may have revealed the open pane by then or not yet: shoot it from the top. */
async function scrollSidebarUp(page) {
  await page.evaluate(
    () =>
      new Promise((done) =>
        requestAnimationFrame(() =>
          requestAnimationFrame(() => {
            for (const el of document.querySelectorAll("app-sidebar, app-sidebar *"))
              el.scrollTop = 0;
            done();
          }),
        ),
      ),
  );
}

/**
 * Each view: `name` (the file is `<name>-<theme>.png`), `ui` (more saved UI state), `run(page)` and the
 * guide page that shows it.
 */
export const VIEWS = [
  {
    name: "sidebar",
    run: async (page) => {
      await ready(page);
    },
  },
  {
    name: "tabs",
    run: async (page) => {
      await ready(page);
      await openPane(page, "claude");
      await terminalShows(page, "t1", "t2");
    },
  },
  {
    name: "blocked",
    run: async (page) => {
      await ready(page);
      await page
        .getByRole("treeitem", { name: /waiting: permission/ })
        .last()
        .dblclick();
      await terminalShows(page, "t4");
    },
  },
  {
    name: "runs",
    run: async (page) => {
      await ready(page);
      await button(page, "Runs").click();
      await page.getByText("Writing the demo backend").first().waitFor();
    },
  },
  {
    name: "flow",
    ui: { panelOpen: true, panelTab: "flow", panelHeight: 340 },
    run: async (page) => {
      await ready(page);
      await openPane(page, "claude");
      await terminalShows(page, "t1", "t2");
      await page.locator("app-flow-view, .flow").first().waitFor();
    },
  },
  {
    name: "palette",
    run: async (page) => {
      await ready(page);
      await page.keyboard.press("ControlOrMeta+Shift+P");
      await page.getByText("Close Tab").first().waitFor();
    },
  },
  {
    name: "jump",
    run: async (page) => {
      await ready(page);
      await page.keyboard.press("ControlOrMeta+K");
      await page.getByText("Needs you").first().waitFor();
    },
  },
  {
    name: "agent-picker",
    run: async (page) => {
      await ready(page);
      await openPane(page, "claude");
      await terminalShows(page, "t1", "t2");
      await openPalette(page);
      await page.getByRole("tab", { name: "Skills" }).click();
      await page.getByText("Skill: deliver").first().click();
      await page.getByRole("dialog", { name: "Choose an agent" }).waitFor();
    },
  },
  {
    name: "terminal-search",
    run: async (page) => {
      await ready(page);
      await openPane(page, "claude");
      await terminalShows(page, "t1", "t2");
      await page.keyboard.press("ControlOrMeta+Shift+F");
      await page
        .getByRole("textbox", { name: /search|find/i })
        .first()
        .fill("demo");
      await page.getByText("1 of 2").waitFor();
    },
  },
  {
    name: "changes",
    run: async (page) => {
      await ready(page);
      await openPane(page, "claude");
      await terminalShows(page, "t1", "t2");
      await openPalette(page);
      await page.keyboard.type("Show Changes");
      await page.getByText("Show Changes").first().click();
      await page.locator(".cm-content").getByText("configurations").first().waitFor();
      await scrollSidebarUp(page);
    },
  },
  {
    name: "github",
    run: async (page) => {
      await ready(page);
      await button(page, "GitHub").click();
      await page.getByRole("button", { name: /^Issues/ }).click();
      await page.getByText("tooltip delay").first().waitFor();
    },
  },
  {
    name: "settings-server",
    run: async (page) => {
      await ready(page);
      await button(page, "Settings").click();
      await page.getByText("Server", { exact: true }).first().click();
      await page.getByText("Restart server…").waitFor();
    },
  },
  {
    name: "machines",
    run: async (page) => {
      await ready(page);
      await button(page, "Settings").click();
      await page.getByText("Machines", { exact: true }).first().click();
      await page.getByText("build-box").first().waitFor();
    },
  },
  {
    name: "schemas",
    run: async (page) => {
      await ready(page);
      await button(page, "Settings").click();
      await page.getByText("Color scheme").first().waitFor();
    },
  },
];

export const baseUi = (view) => JSON.stringify({ ...BASE_UI, ...(view.ui ?? {}) });
