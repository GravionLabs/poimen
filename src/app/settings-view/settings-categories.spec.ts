import { describe, expect, it } from "vitest";
import {
  allPages,
  DEFAULT_SETTINGS_PAGE,
  filterGroups,
  findPage,
  matches,
} from "./settings-categories";

describe("settings categories", () => {
  it("maps every existing section to one page under a group", () => {
    const tree = Object.fromEntries(
      filterGroups("").map((g) => [g.title, g.pages.map((p) => p.title)]),
    );
    expect(tree).toEqual({
      "Appearance & Behavior": ["Appearance"],
      Workspace: ["Explorer", "Spaces", "New space layout", "Trusted projects"],
      Backend: ["Server", "Machines", "Agents"],
    });
  });

  it("finds a page with its group, or null", () => {
    expect(findPage("herdr")?.group.title).toBe("Backend");
    expect(findPage("nope")).toBeNull();
    expect(findPage(null)).toBeNull();
    expect(findPage(DEFAULT_SETTINGS_PAGE)?.page.title).toBe("Appearance");
    expect(allPages()).toHaveLength(8);
  });

  it("resolves the retired color-scheme page to appearance", () => {
    expect(findPage("color-scheme")?.page.id).toBe("appearance");
    expect(findPage("color-scheme")?.group.id).toBe("appearance-behavior");
    expect(findPage("color-scheme")).toEqual(findPage("appearance"));
    expect(findPage("color-scheme", [])).toBeNull();
  });

  it("finds the scheme settings on the appearance page", () => {
    expect(filterGroups("single scheme").flatMap((g) => g.pages.map((p) => p.id))).toEqual([
      "appearance",
    ]);
    expect(filterGroups("dark schemes").flatMap((g) => g.pages.map((p) => p.id))).toEqual([
      "appearance",
    ]);
  });

  it("finds the spaces page by template, space and agent", () => {
    for (const query of ["template", "space", "agent"]) {
      expect(
        filterGroups(query).flatMap((g) => g.pages.map((p) => p.id)),
        query,
      ).toContain("spaces");
    }
  });

  it("matches ignoring case and surrounding blanks; an empty query matches nothing", () => {
    expect(matches("Show hidden folders", "  HIDDEN ")).toBe(true);
    expect(matches("Show hidden folders", "")).toBe(false);
    expect(matches("Show hidden folders", "visible")).toBe(false);
  });

  it("filters by page title, keeping the group", () => {
    expect(filterGroups("trusted").map((g) => [g.title, g.pages.map((p) => p.id)])).toEqual([
      ["Workspace", ["trusted-projects"]],
    ]);
  });

  it("filters by the label of a setting", () => {
    expect(filterGroups("hidden folders").flatMap((g) => g.pages.map((p) => p.id))).toEqual([
      "explorer",
    ]);
  });

  it("hides groups without a match and returns all for a blank query", () => {
    expect(filterGroups("zzz")).toEqual([]);
    expect(filterGroups("   ")).toHaveLength(3);
  });
});
