import { describe, expect, it } from "vitest";
import {
  addTab,
  canIndent,
  canOutdent,
  duplicateLayout,
  editNode,
  editorGroups,
  emptyLayout,
  hasLayoutErrors,
  indentTab,
  layoutErrors,
  layoutFromTab,
  layoutUsers,
  moveTab,
  outdentTab,
  removeNode,
  removeTab,
  renameTab,
  spacePreview,
  splitNode,
  usedMessage,
  type LayoutNode,
  type LayoutTab,
  type NamedLayout,
} from "./named-layouts-edit";
import { layoutConfig, parseNamedLayouts, uniqueName } from "./named-layouts";
import type { Snapshot } from "./snapshot";
import type { SpaceTemplate } from "./space-templates";

const leaf = (name: string | null = null, command: string | null = null): LayoutNode => ({
  kind: "terminal",
  name,
  command,
  cwd: null,
});
const tab = (name: string, children: LayoutTab[] = [], root = leaf()): LayoutTab => ({
  name,
  root,
  children,
  place: null,
});
const template = (name: string, layout: SpaceTemplate["layout"]): SpaceTemplate => ({
  name,
  dir: null,
  remote: null,
  agent: "claude",
  model: null,
  layout,
});

describe("named layouts wire and config", () => {
  const wire = [
    {
      name: "review",
      layout: {
        tabs: [
          {
            name: "agent",
            root: { kind: "terminal", name: "agent", command: "claude", cwd: null },
            children: [
              {
                name: "tests",
                root: {
                  kind: "split",
                  direction: "down",
                  ratio: 0.3,
                  children: [
                    { kind: "terminal", name: null, command: "pnpm test", cwd: null },
                    { kind: "terminal", name: null, command: null, cwd: "src" },
                  ],
                },
                children: [],
                place: { direction: "right", of: null, ratio: 0.4 },
              },
            ],
            place: null,
          },
        ],
      },
    },
    { name: "broken", layout: { tabs: [] } },
    "junk",
  ];

  it("reads what the backend sends and skips malformed entries", () => {
    const [l, ...rest] = parseNamedLayouts(wire);
    expect(rest).toEqual([]);
    expect(l.name).toBe("review");
    expect(l.tabs[0].children[0].place).toEqual({ direction: "right", of: null, ratio: 0.4 });
    expect(parseNamedLayouts("x")).toEqual([]);
  });

  it("writes a flat tab where the tree is one terminal and a root where it is a split", () => {
    const [l] = parseNamedLayouts(wire);
    expect(layoutConfig(l)).toEqual({
      name: "review",
      tabs: [
        {
          name: "agent",
          command: "claude",
          children: [
            {
              name: "tests",
              root: {
                kind: "split",
                direction: "down",
                ratio: 0.3,
                children: [
                  { kind: "terminal", command: "pnpm test" },
                  { kind: "terminal", cwd: "src" },
                ],
              },
              place: { direction: "right", ratio: 0.4 },
            },
          ],
        },
      ],
    });
  });

  it("keeps a leaf name that differs from the tab name by writing a root", () => {
    const l: NamedLayout = { name: "a", tabs: [tab("x", [], leaf("other"))] };
    expect(layoutConfig(l)["tabs"]).toEqual([
      { name: "x", root: { kind: "terminal", name: "other" } },
    ]);
  });
});

describe("tab edits", () => {
  const base = [tab("a", [tab("a1"), tab("a2")]), tab("b")];

  it("adds top level and below a parent with a free name", () => {
    expect(addTab(base, null).map((t) => t.name)).toEqual(["a", "b", "tab"]);
    expect(addTab(base, [0])[0].children.map((t) => t.name)).toEqual(["a1", "a2", "tab"]);
    expect(addTab(addTab(base, null), null).at(-1)?.name).toBe("tab 2");
  });

  it("indents under the sibling before and outdents after the parent", () => {
    expect(canIndent([0])).toBe(false);
    expect(canIndent([1])).toBe(true);
    const indented = indentTab(base, [1]);
    expect(indented.map((t) => t.name)).toEqual(["a"]);
    expect(indented[0].children.map((t) => t.name)).toEqual(["a1", "a2", "b"]);
    expect(canOutdent([0])).toBe(false);
    const out = outdentTab(base, [0, 0]);
    expect(out.map((t) => t.name)).toEqual(["a", "a1", "b"]);
    expect(out[0].children.map((t) => t.name)).toEqual(["a2"]);
  });

  it("outdents from a third level into the grandparent", () => {
    const deep = [tab("a", [tab("a1", [tab("x"), tab("y")])])];
    const out = outdentTab(deep, [0, 0, 0]);
    expect(out[0].children.map((t) => t.name)).toEqual(["a1", "x"]);
    expect(out[0].children[0].children.map((t) => t.name)).toEqual(["y"]);
  });

  it("reorders among siblings and stops at the ends", () => {
    expect(moveTab(base, [0], 1).map((t) => t.name)).toEqual(["b", "a"]);
    expect(moveTab(base, [0], -1).map((t) => t.name)).toEqual(["a", "b"]);
    expect(moveTab(base, [0, 1], -1)[0].children.map((t) => t.name)).toEqual(["a2", "a1"]);
  });

  it("deletes a tab with its children", () => {
    expect(removeTab(base, [0]).map((t) => t.name)).toEqual(["b"]);
    expect(removeTab(base, [0, 0])[0].children.map((t) => t.name)).toEqual(["a2"]);
  });

  it("renames a tab and the places that point at it", () => {
    const tabs = [
      tab("a"),
      { ...tab("b"), place: { direction: "right" as const, of: "a", ratio: 0.5 } },
    ];
    const out = renameTab(tabs, [0], "main");
    expect(out[0].name).toBe("main");
    expect(out[1].place?.of).toBe("main");
  });
});

describe("split tree edits", () => {
  it("splits a leaf, edits it and removes it again", () => {
    const one = leaf();
    const two = splitNode(one, [], "right");
    expect(two.kind).toBe("split");
    const three = splitNode(two, [1], "down");
    expect(three.kind === "split" && three.children[1].kind).toBe("split");
    const named = editNode(three, [0], { command: "claude" });
    expect(named.kind === "split" && named.children[0]).toMatchObject({ command: "claude" });
    const ratio = editNode(three, [], { ratio: 0.7 });
    expect(ratio).toMatchObject({ ratio: 0.7 });
    const back = removeNode(three, [1, 1]);
    expect(back.kind === "split" && back.children[1].kind).toBe("terminal");
    expect(removeNode(one, [])).toBe(one);
  });
});

describe("layoutErrors", () => {
  it("accepts a good layout and names the problems of a bad one", () => {
    const good: NamedLayout = { name: "a", tabs: [tab("x", [tab("y")])] };
    expect(hasLayoutErrors(layoutErrors(good, []))).toBe(false);
    expect(layoutErrors({ ...good, name: " " }, []).name).toBeDefined();
    expect(layoutErrors(good, ["a"]).name).toContain("Another layout");
    expect(layoutErrors({ name: "a", tabs: [] }, []).tabs).toBe("Add a tab");
    const dup = layoutErrors({ name: "a", tabs: [tab("x"), tab("x")] }, []);
    expect(dup.tab[1]).toContain("Another tab");
    const noName = layoutErrors({ name: "a", tabs: [tab("")] }, []);
    expect(noName.tab[0]).toContain("needs a name");
  });

  it("checks places: unknown tab, itself, ratio and cycle", () => {
    const placed = (of: string | null, ratio = 0.5): LayoutTab => ({
      ...tab("p"),
      place: { direction: "right", of, ratio },
    });
    const run = (tabs: LayoutTab[]) => layoutErrors({ name: "a", tabs }, []);
    expect(run([tab("x"), placed("x")]).tab[1]).toBeUndefined();
    expect(run([tab("x"), placed("nope")]).tab[1]).toContain("not a tab");
    expect(run([tab("x"), placed("p")]).tab[1]).toContain("itself");
    expect(run([tab("x"), placed("x", 1)]).tab[1]).toContain("ratio");
    const cycle = run([
      { ...tab("a"), place: { direction: "right", of: "b", ratio: 0.5 } },
      { ...tab("b"), place: { direction: "right", of: "a", ratio: 0.5 } },
    ]);
    expect(cycle.tab[0]).toContain("cycle");
    const badSplit = run([
      tab("x", [], { kind: "split", direction: "right", ratio: 2, children: [leaf(), leaf()] }),
    ]);
    expect(badSplit.tab[0]).toContain("split ratio");
  });
});

describe("the delete check", () => {
  const templates = [
    template("one", "review"),
    template("two", null),
    template("three", "review"),
    template("four", "other"),
  ];

  it("names the templates that refer to a layout", () => {
    expect(layoutUsers("review", templates)).toEqual(["one", "three"]);
    expect(layoutUsers("free", templates)).toEqual([]);
    expect(usedMessage("review", templates, "delete")).toBe(
      'Cannot delete "review": used by templates one, three.',
    );
    expect(usedMessage("other", templates, "rename")).toBe(
      'Cannot rename "other": used by template four.',
    );
    expect(usedMessage("free", templates, "delete")).toBeNull();
  });
});

describe("names and copies", () => {
  it("finds a free name and duplicates without sharing the tree", () => {
    expect(uniqueName("a", ["a", "a 2"])).toBe("a 3");
    const l = emptyLayout("x");
    const copy = duplicateLayout(l, "x 2");
    copy.tabs[0].name = "changed";
    expect(l.tabs[0].name).toBe("main");
    expect(copy.name).toBe("x 2");
  });
});

describe("layoutFromTab", () => {
  const snapshot: Snapshot = {
    spaces: [],
    tabs: [
      {
        tab_id: "tab1",
        space_id: "s1",
        label: "work",
        number: 1,
        agent_status: "unknown",
        root: {
          kind: "split",
          split_id: "sp1",
          direction: "right",
          ratio: 0.6,
          children: [
            { kind: "terminal", terminal_id: "t1" },
            { kind: "terminal", terminal_id: "t2" },
          ],
        },
      },
      {
        tab_id: "tab2",
        space_id: "s1",
        label: "docs",
        number: 3,
        agent_status: "unknown",
        parent: "tab1",
        root: { kind: "terminal", terminal_id: "t3" },
      },
      {
        tab_id: "tab3",
        space_id: "s1",
        label: "docs",
        number: 2,
        agent_status: "unknown",
        parent: "tab1",
        root: { kind: "terminal", terminal_id: "t4" },
      },
      {
        tab_id: "tab4",
        space_id: "s1",
        label: "other",
        number: 4,
        agent_status: "unknown",
        root: { kind: "terminal", terminal_id: "t5" },
      },
    ],
    terminals: [
      {
        terminal_id: "t1",
        tab_id: "tab1",
        space_id: "s1",
        agent: "claude",
        agent_status: "unknown",
      },
      {
        terminal_id: "t2",
        tab_id: "tab1",
        space_id: "s1",
        label: "tests",
        agent_status: "unknown",
      },
    ] as Snapshot["terminals"],
  };

  it("builds the tree and the child tabs of the tab, nothing else", () => {
    const l = layoutFromTab(snapshot, "tab1", "saved")!;
    expect(l.name).toBe("saved");
    expect(l.tabs).toHaveLength(1);
    const root = l.tabs[0].root;
    expect(root).toMatchObject({ kind: "split", direction: "right", ratio: 0.6 });
    expect(root.kind === "split" && root.children[0]).toMatchObject({ command: "claude" });
    expect(root.kind === "split" && root.children[1]).toMatchObject({ name: "tests" });
    // Children in tab order; the equal labels become unique names.
    expect(l.tabs[0].children.map((t) => t.name)).toEqual(["docs", "docs 2"]);
    expect(layoutErrors(l, [])).toEqual({ tab: {} });
    expect(layoutFromTab(snapshot, "missing", "x")).toBeNull();
  });
});

describe("previews", () => {
  it("lists the tabs as sidebar rows", () => {
    const tabs = [tab("a", [tab("a1", [], leaf(null, "claude"))]), tab("b")];
    expect(spacePreview(tabs)).toEqual([
      { name: "a", depth: 0, detail: "shell" },
      { name: "a1", depth: 1, detail: "claude" },
      { name: "b", depth: 0, detail: "shell" },
    ]);
  });

  it("arranges editor groups from the places", () => {
    expect(editorGroups([])).toBeNull();
    const placed = (name: string, of: string | null, direction: "right" | "down"): LayoutTab => ({
      ...tab(name),
      place: { direction, of, ratio: 0.4 },
    });
    // Without places every tab shares the first group.
    expect(editorGroups([tab("a", [tab("a1")]), tab("b")])).toEqual({
      kind: "group",
      tabs: ["a", "a1", "b"],
    });
    const groups = editorGroups([
      tab("a", [placed("a1", null, "right")]),
      placed("b", "a1", "down"),
    ]);
    expect(groups).toEqual({
      kind: "split",
      direction: "right",
      ratio: 0.4,
      children: [
        { kind: "group", tabs: ["a"] },
        {
          kind: "split",
          direction: "down",
          ratio: 0.4,
          children: [
            { kind: "group", tabs: ["a1"] },
            { kind: "group", tabs: ["b"] },
          ],
        },
      ],
    });
  });
});
