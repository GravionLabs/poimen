import { describe, expect, it } from "vitest";
import {
  addPreset,
  changePreset,
  layoutOfTabs,
  presetLayout,
  presetNameError,
  presetsOfText,
  removePreset,
  replacePresetLayout,
  tabsOfLayout,
} from "./layout-config";

const NESTED = {
  tabs: [
    {
      name: "agent",
      command: "claude",
      children: [
        {
          name: "tests",
          place: { direction: "down", of: "agent", ratio: 0.3 },
          root: {
            kind: "split",
            direction: "right",
            ratio: 0.4,
            children: [
              { kind: "terminal", name: "unit", command: "pnpm test" },
              { kind: "terminal", name: "e2e", cwd: "e2e" },
            ],
          },
          children: [{ name: "logs", command: "tail -f log" }],
        },
      ],
    },
    { name: "shell" },
  ],
};

describe("layout config round trip", () => {
  it("keeps nested tabs, split trees and place unchanged through load and save", () => {
    const tabs = tabsOfLayout(NESTED)!;
    expect(tabs).not.toBeNull();
    expect(tabs[0].children[0].place).toEqual({ direction: "down", of: "agent", ratio: 0.3 });
    expect(tabs[0].children[0].children[0].name).toBe("logs");
    expect(layoutOfTabs(tabs)).toEqual(NESTED);
    // And once more from what was written.
    expect(layoutOfTabs(tabsOfLayout(layoutOfTabs(tabs))!)).toEqual(NESTED);
  });

  it("converts panes[] to the tab with the same split tree", () => {
    const tabs = tabsOfLayout({
      panes: [
        { name: "agent", command: "claude" },
        { name: "tests", split: { direction: "right", ratio: 0.6 } },
        { name: "shell", split: { from: "tests", direction: "down" } },
      ],
    })!;
    expect(tabs).toHaveLength(1);
    expect(tabs[0].name).toBe("agent");
    expect(layoutOfTabs(tabs).tabs[0]).toEqual({
      name: "agent",
      root: {
        kind: "split",
        direction: "right",
        ratio: 0.6,
        children: [
          { kind: "terminal", name: "agent", command: "claude" },
          {
            kind: "split",
            direction: "down",
            ratio: 0.5,
            children: [
              { kind: "terminal", name: "tests" },
              { kind: "terminal", name: "shell" },
            ],
          },
        ],
      },
    });
  });

  it("gives null for what the editor cannot show", () => {
    expect(tabsOfLayout(null)).toBeNull();
    expect(tabsOfLayout([1])).toBeNull();
    expect(tabsOfLayout({})).toBeNull();
    expect(tabsOfLayout({ tabs: [{ name: "a", root: { kind: "nope" } }] })).toBeNull();
    expect(
      tabsOfLayout({ panes: [{ name: "a" }, { name: "b", split: { from: "zzz" } }] }),
    ).toBeNull();
  });
});

describe("presets text", () => {
  const source = JSON.stringify([
    { name: "duo", panes: [{ name: "a" }, { name: "b", split: { direction: "right" } }], x: 1 },
    { name: "tabs", ...NESTED },
  ]);

  it("lists presets with their tabs", () => {
    const presets = presetsOfText(source)!;
    expect(presets.map((p) => p.name)).toEqual(["duo", "tabs"]);
    expect(presets[1].tabs).toHaveLength(2);
    expect(presetsOfText("")).toEqual([]);
    expect(presetsOfText("{")).toBeNull();
  });

  it("changes one preset and leaves the others, and unknown keys, alone", () => {
    const renamed = JSON.parse(changePreset(source, 0, { name: "pair" }));
    expect(renamed[0]).toMatchObject({ name: "pair", x: 1, panes: expect.any(Array) });
    expect(renamed[1]).toEqual({ name: "tabs", ...NESTED });
    const tabs = presetsOfText(source)![1].tabs!;
    const edited = JSON.parse(changePreset(source, 1, { tabs: [...tabs, tabs[1]] }));
    expect(edited[1].tabs).toHaveLength(3);
    expect(edited[0]).toEqual(JSON.parse(source)[0]);
  });

  it("converts a panes preset to tabs only when its tabs change", () => {
    const tabs = presetsOfText(source)![0].tabs!;
    const edited = JSON.parse(changePreset(source, 0, { tabs }));
    expect(edited[0].panes).toBeUndefined();
    expect(edited[0].tabs).toHaveLength(1);
    expect(edited[0].x).toBe(1);
  });

  it("adds, removes, exports and imports a preset layout", () => {
    const added = JSON.parse(addPreset(source, "new"));
    expect(added.map((p: { name: string }) => p.name)).toEqual(["duo", "tabs", "new"]);
    expect(removePreset(JSON.stringify([added[2]]), 0)).toBe("");
    expect(presetLayout(source, 1)).toEqual(NESTED);
    expect(presetLayout(source, 9)).toBeNull();
    const imported = JSON.parse(replacePresetLayout(source, 0, NESTED));
    expect(imported[0]).toEqual({ name: "duo", ...NESTED });
  });

  it("names blank and repeated preset names", () => {
    expect(presetNameError(["a", "b"])).toBeNull();
    expect(presetNameError(["a", " "])).toContain("name");
    expect(presetNameError(["a", "a "])).toContain('"a"');
  });
});
