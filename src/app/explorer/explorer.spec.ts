// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { HerdrStore } from "../core/herdr-store";
import { SpaceActions } from "../core/space-actions";
import { DEFAULT_SETTINGS, SettingsStore } from "../core/settings";
import { TauriBridge } from "../core/tauri";
import { UI_STATE_KEY, UI_STORAGE } from "../core/ui-state";
import { Explorer, explorerNodes, spaceDirs, type DirEntry } from "./explorer";

const dir = (path: string): DirEntry => ({ path, name: path.split("/").pop()! });

describe("explorerNodes", () => {
  it("labels roots by path and keeps unloaded folders expandable", () => {
    const [root] = explorerNodes(["~"], new Map(), new Set());
    expect(root).toMatchObject({ key: "~", label: "~", leaf: false, expanded: false });
    expect(root.children).toEqual([]);
  });

  it("nests loaded children by name and marks the expanded ones", () => {
    const children = new Map([["/r", [dir("/r/a"), dir("/r/b")]]]);
    const [root] = explorerNodes(["/r"], children, new Set(["/r"]));
    expect(root.expanded).toBe(true);
    expect(root.children!.map((c) => [c.key, c.label])).toEqual([
      ["/r/a", "a"],
      ["/r/b", "b"],
    ]);
  });

  it("makes a loaded empty folder a leaf", () => {
    const [root] = explorerNodes(["/r"], new Map([["/r", []]]), new Set());
    expect(root.leaf).toBe(true);
  });
});

const tree: Record<string, DirEntry[]> = {
  "/r": [dir("/r/a"), dir("/r/locked")],
  "/r/a": [dir("/r/a/deep")],
  "/r/locked": [],
};

async function setup(stored?: object, roots: string[] = ["/r"], spaceCwds: string[] = []) {
  const create = vi.fn(async () => undefined);
  const setExplorerRoot = vi.fn(async () => undefined);
  TestBed.resetTestingModule();
  const calls: { path: string; hidden: boolean }[] = [];
  const data = new Map<string, string>();
  if (stored) data.set(UI_STATE_KEY, JSON.stringify(stored));
  TestBed.configureTestingModule({
    providers: [
      {
        provide: TauriBridge,
        useValue: {
          invoke: vi.fn(async (_: string, args: { path: string; hidden: boolean }) => {
            calls.push(args);
            if (args.path === "/r/locked") throw new Error("permission denied");
            return tree[args.path] ?? [];
          }),
        },
      },
      { provide: SpaceActions, useValue: { create } },
      {
        provide: HerdrStore,
        useValue: {
          snapshot: signal({ workspaces: [], tabs: [], panes: spaceCwds.map((cwd) => ({ cwd })) }),
        },
      },
      {
        provide: SettingsStore,
        useValue: {
          settings: signal({ ...DEFAULT_SETTINGS, explorerRoots: roots }),
          setExplorerRoot,
        },
      },
      {
        provide: UI_STORAGE,
        useValue: {
          getItem: (k: string) => data.get(k) ?? null,
          setItem: (k: string, v: string) => void data.set(k, v),
        },
      },
    ],
  });
  TestBed.overrideComponent(Explorer, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync("src/app/explorer/explorer.html", "utf8"),
    },
  });
  const fixture = TestBed.createComponent(Explorer);
  const root = fixture.nativeElement as HTMLElement;
  const settle = async () => {
    for (let i = 0; i < 4; i++) {
      fixture.detectChanges();
      await fixture.whenStable();
    }
  };
  await settle();
  const labels = () =>
    [...root.querySelectorAll('[role="treeitem"]')].map((e) =>
      e.querySelector(".hx-tree-node-label")?.textContent?.trim(),
    );
  const toggle = async (label: string) => {
    const item = [...root.querySelectorAll('[role="treeitem"]')].find(
      (e) => e.querySelector(".hx-tree-node-label")?.textContent?.trim() === label,
    );
    item!.querySelector<HTMLElement>(".hx-tree-node-toggle")!.click();
    await settle();
  };
  const item = (label: string) =>
    [...root.querySelectorAll('[role="treeitem"]')].find(
      (e) => e.querySelector(".hx-tree-node-label")?.textContent?.trim() === label,
    )!;
  /** Opens the context menu of `label` and returns the texts of its items. */
  const menu = async (label: string) => {
    item(label)
      .querySelector(".dir")!
      .dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true }));
    await settle();
    return [...root.querySelectorAll<HTMLElement>('[role="menuitem"]')].map((e) =>
      e.textContent!.trim(),
    );
  };
  const click = async (text: string) => {
    [...root.querySelectorAll<HTMLElement>('[role="menuitem"]')]
      .find((e) => e.textContent!.trim() === text)!
      .click();
    await settle();
  };
  return { calls, data, labels, toggle, create, item, root, settle, menu, click, setExplorerRoot };
}

describe("Explorer with the real hx-tree", () => {
  beforeAll(() => {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  });

  it("shows the configured root and lists nothing below it until it expands", async () => {
    const { labels, calls } = await setup();
    expect(labels()).toEqual(["/r"]);
    expect(calls).toEqual([{ path: "/r", hidden: false }]);
  });

  it("expands a folder to its subfolders and remembers it", async () => {
    const { labels, toggle, data } = await setup();
    await toggle("/r");
    expect(labels()).toEqual(["/r", "a", "locked"]);
    expect(JSON.parse(data.get(UI_STATE_KEY)!).expandedDirs).toEqual(["/r"]);
  });

  it("shows an unreadable folder without children and without an error", async () => {
    const { labels, toggle } = await setup();
    await toggle("/r");
    await toggle("locked");
    expect(labels()).toEqual(["/r", "a", "locked"]);
  });

  it("restores the expanded folders after a restart", async () => {
    const { labels } = await setup({ expandedDirs: ["/r", "/r/a"] });
    expect(labels()).toEqual(["/r", "a", "deep", "locked"]);
  });

  it("restores expanded folders under a ~ root, whose children have absolute paths", async () => {
    tree["~"] = [dir("/home/x/code")];
    tree["/home/x/code"] = [dir("/home/x/code/app")];
    const { labels } = await setup({ expandedDirs: ["~", "/home/x/code"] }, ["~"]);
    expect(labels()).toEqual(["~", "code", "app"]);
  });

  it("marks directories that have a space", async () => {
    const { toggle, item } = await setup(undefined, ["/r"], ["/r/a/"]);
    await toggle("/r");
    expect(item("a").querySelector(".has-space")).not.toBeNull();
    expect(item("locked").querySelector(".has-space")).toBeNull();
  });

  it("creates a space from the context menu", async () => {
    const { toggle, item, root, create, settle } = await setup();
    await toggle("/r");
    item("a")
      .querySelector(".dir")!
      .dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true }));
    await settle();
    root.querySelector<HTMLElement>('[role="menuitem"]')!.click();
    await settle();
    expect(create).toHaveBeenCalledWith("/r/a", "");
    expect(root.querySelector('[role="menu"]')).toBeNull();
  });

  it("sets a folder as the root it lies under", async () => {
    tree["~"] = [dir("/home/x/Source")];
    const { toggle, menu, click, setExplorerRoot, root } = await setup(undefined, ["/r", "~"]);
    await toggle("~");
    expect(await menu("Source")).toEqual(["Create space here", "Set as root"]);
    await click("Set as root");
    expect(setExplorerRoot).toHaveBeenCalledWith(1, "/home/x/Source");
    expect(root.querySelector('[role="menu"]')).toBeNull();
  });

  it("moves a root up to its parent folder", async () => {
    const { menu, click, setExplorerRoot } = await setup();
    expect(await menu("/r")).toEqual(["Create space here", "Show parent folder"]);
    await click("Show parent folder");
    expect(setExplorerRoot).toHaveBeenCalledWith(0, "/r/..");
  });

  it("offers no parent folder for the file system root", async () => {
    const { menu } = await setup(undefined, ["/"]);
    expect(await menu("/")).toEqual(["Create space here"]);
  });

  it("creates (or focuses) a space on double-click", async () => {
    const { toggle, item, create } = await setup();
    await toggle("/r");
    item("a")
      .querySelector(".dir")!
      .dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    expect(create).toHaveBeenCalledWith("/r/a", "");
  });
});

describe("spaceDirs", () => {
  it("collects cwd and foreground cwd without trailing slashes and skips empty ones", () => {
    expect(
      spaceDirs([{ cwd: "/a/", foreground_cwd: "/a/b" }, { cwd: "" }, {}, { cwd: "/" }]),
    ).toEqual(new Set(["/a", "/a/b", "/"]));
  });
});
