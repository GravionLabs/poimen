import type { TemplateRef } from "@angular/core";
import { describe, expect, it } from "vitest";
import { ACTIVITY_ENTRIES, SIDEBAR_VIEW_IDS, activityItems, tabOf } from "./activity-items";

const tpl = (name: string) => ({ name }) as unknown as TemplateRef<unknown>;
const icons = Object.fromEntries(ACTIVITY_ENTRIES.map((e) => [e.id, tpl(e.id)]));

describe("activityItems", () => {
  const items = activityItems(icons);

  it("offers Spaces, Explorer and Notifications first, as views", () => {
    expect(items.slice(0, 3)).toMatchObject([
      { id: "spaces", label: "Spaces", kind: "view", position: "top" },
      { id: "explorer", label: "Explorer", kind: "view", position: "top" },
      { id: "notifications", label: "Notifications", kind: "view", position: "top" },
    ]);
  });

  it("makes GitHub and Runs actions", () => {
    expect(items.find((i) => i.id === "github")).toMatchObject({ kind: "action", position: "top" });
    expect(items.find((i) => i.id === "deliver")).toMatchObject({
      label: "Runs",
      kind: "action",
    });
  });

  it("puts Settings at the bottom as an action", () => {
    expect(items.at(-1)).toMatchObject({ id: "settings", kind: "action", position: "bottom" });
  });

  it("passes the icon templates and has unique ids", () => {
    for (const item of items) expect(item.icon).toBe(icons[item.id]);
    expect(new Set(items.map((i) => i.id)).size).toBe(items.length);
  });

  it("skips an entry without a template", () => {
    expect(activityItems({}).length).toBe(0);
  });
});

describe("tabOf and SIDEBAR_VIEW_IDS", () => {
  it("names the tab of an action and none for a view", () => {
    expect(tabOf("github")).toBe("github");
    expect(tabOf("deliver")).toBe("deliver");
    expect(tabOf("settings")).toBe("settings");
    expect(tabOf("spaces")).toBeUndefined();
    expect(tabOf("nope")).toBeUndefined();
  });

  it("lists the sidebar views", () => {
    expect(SIDEBAR_VIEW_IDS).toEqual(["spaces", "explorer", "notifications"]);
  });
});
