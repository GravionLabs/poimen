import { describe, expect, it } from "vitest";
import { ACTIVITY_VIEWS } from "./activity-bar";

describe("ACTIVITY_VIEWS", () => {
  it("offers Spaces first", () => {
    expect(ACTIVITY_VIEWS[0]).toMatchObject({ id: "spaces", label: "Spaces" });
  });

  it("offers the Explorer second", () => {
    expect(ACTIVITY_VIEWS[1]).toMatchObject({ id: "explorer", label: "Explorer" });
  });

  it("offers the Notifications third", () => {
    expect(ACTIVITY_VIEWS[2]).toMatchObject({ id: "notifications", label: "Notifications" });
  });

  it("gives each view its own icon", () => {
    const icons = ACTIVITY_VIEWS.map((v) => v.icon);
    expect(new Set(icons).size).toBe(icons.length);
  });

  it("has unique ids", () => {
    const ids = ACTIVITY_VIEWS.map((v) => v.id);
    expect(new Set(ids).size).toBe(ids.length);
  });
});
