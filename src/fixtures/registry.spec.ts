import { describe, expect, it } from "vitest";
import { FIXTURES, findFixture, listFixtures } from "./registry";

describe("fixture registry", () => {
  it("has unique ids of the form <component>/<state>", () => {
    const ids = FIXTURES.map((f) => f.id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const id of ids) expect(id).toMatch(/^[a-z-]+\/[a-z-]+$/);
  });

  it("covers the components of the issue", () => {
    const components = new Set(FIXTURES.map((f) => f.id.split("/")[0]));
    expect([...components].sort()).toEqual([
      "context-menu",
      "dialog-shell",
      "space-card",
      "status-bar",
      "tab-bar",
    ]);
  });

  it("finds a fixture by id and lists the ids and titles", () => {
    expect(findFixture("tab-bar/states")?.title).toBe(
      "Active, preview, attention, unread and silent tabs",
    );
    expect(findFixture("nope/none")).toBeUndefined();
    expect(listFixtures()[0]).toMatchObject({ id: "dialog-shell/confirm" });
  });
});
