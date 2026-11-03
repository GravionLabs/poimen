import { describe, expect, it } from "vitest";
import { MAX_NOTIFICATIONS, NotificationCenter } from "./notification-center";

const label = (id: string) => `tab ${id}`;
const center = () => new NotificationCenter();

describe("NotificationCenter", () => {
  it("starts empty", () => {
    const c = center();
    expect(c.entries()).toEqual([]);
    expect(c.unread()).toBe(0);
  });

  it("records changes newest first with message, label and time", () => {
    const c = center();
    c.record([{ paneId: "p1", status: "blocked" }], label, 1000);
    c.record(
      [
        { paneId: "p2", status: "done" },
        { paneId: "p3", status: "done" },
      ],
      label,
      2000,
    );
    expect(c.entries().map((e) => e.paneId)).toEqual(["p3", "p2", "p1"]);
    expect(c.entries()[2]).toMatchObject({
      label: "tab p1",
      message: "tab p1 is waiting for you",
      time: 1000,
      read: false,
    });
    expect(c.entries()[0].message).toBe("tab p3 is done");
    expect(c.unread()).toBe(3);
  });

  it("ignores an empty change list", () => {
    const c = center();
    c.record([], label, 1);
    expect(c.entries()).toEqual([]);
  });

  it("keeps at most the newest MAX_NOTIFICATIONS entries", () => {
    const c = center();
    for (let i = 0; i < MAX_NOTIFICATIONS + 5; i++)
      c.record([{ paneId: `p${i}`, status: "done" }], label, i);
    expect(c.entries()).toHaveLength(MAX_NOTIFICATIONS);
    expect(c.entries()[0].paneId).toBe(`p${MAX_NOTIFICATIONS + 4}`);
    expect(c.entries()[MAX_NOTIFICATIONS - 1].paneId).toBe("p5");
  });

  it("marks one entry read", () => {
    const c = center();
    c.record([{ paneId: "p1", status: "done" }], label, 1);
    c.record([{ paneId: "p2", status: "done" }], label, 2);
    c.markRead(c.entries()[0].id);
    expect(c.entries().map((e) => e.read)).toEqual([true, false]);
    expect(c.unread()).toBe(1);
  });

  it("marks all read and keeps the entries", () => {
    const c = center();
    c.record([{ paneId: "p1", status: "blocked" }], label, 1);
    c.record([{ paneId: "p2", status: "done" }], label, 2);
    c.markAllRead();
    expect(c.unread()).toBe(0);
    expect(c.entries()).toHaveLength(2);
  });
});
