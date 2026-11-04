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
      "Appearance & Behavior": ["Appearance", "Color scheme", "Notifications"],
      Workspace: ["Explorer", "New space layout", "Trusted projects"],
      Backend: ["herdr", "Agents"],
    });
  });

  it("finds a page with its group, or null", () => {
    expect(findPage("herdr")?.group.title).toBe("Backend");
    expect(findPage("nope")).toBeNull();
    expect(findPage(null)).toBeNull();
    expect(findPage(DEFAULT_SETTINGS_PAGE)?.page.title).toBe("Appearance");
    expect(allPages()).toHaveLength(8);
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
    expect(filterGroups("agent is done").flatMap((g) => g.pages.map((p) => p.id))).toEqual([
      "notifications",
    ]);
  });

  it("hides groups without a match and returns all for a blank query", () => {
    expect(filterGroups("zzz")).toEqual([]);
    expect(filterGroups("   ")).toHaveLength(3);
  });
});
