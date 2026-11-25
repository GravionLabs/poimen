import { describe, expect, it } from "vitest";
import { MAX_NOTIFICATIONS, NotificationCenter } from "./notification-center";
import type { ScheduleAlert } from "./schedules-model";

const label = (id: string) => `tab ${id}`;
const center = () => new NotificationCenter();

describe("NotificationCenter", () => {
  it("words a failure and records it like a block", () => {
    const c = center();
    c.record([{ paneId: "p1", status: "error" }], label, 1000);
    expect(c.entries()[0]).toMatchObject({ status: "error", message: "tab p1 failed" });
    expect(c.unread()).toBe(1);
  });

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

  it("records run alerts with the repository and machine to reveal", () => {
    const c = center();
    c.recordRuns([{ root: "/r", machine: "m", label: "M1", message: "M1: merge" }], 5);
    c.recordRuns([], 6);
    expect(c.entries()).toHaveLength(1);
    expect(c.entries()[0]).toMatchObject({
      message: "M1: merge",
      deliver: { root: "/r", machine: "m" },
      read: false,
    });
  });

  describe("observeSilent (#899)", () => {
    const silent = (...e: [string, number | undefined][]) => new Map(e);

    it("records one entry per transition, none for the first snapshot or a pane that stays silent", () => {
      const c = center();
      c.observeSilent(silent(["p1", 5000]), label, 1);
      c.observeSilent(silent(["p1", 5000]), label, 2);
      expect(c.entries()).toEqual([]);
      c.observeSilent(silent(["p1", undefined]), label, 3);
      c.observeSilent(silent(["p1", 9000]), label, 4);
      c.observeSilent(silent(["p1", 9000]), label, 5);
      expect(c.entries()).toHaveLength(1);
      expect(c.entries()[0]).toMatchObject({ paneId: "p1", status: "silent", read: false });
    });

    it("a cleared mark records nothing and a new pane is a baseline", () => {
      const c = center();
      c.observeSilent(silent(["p1", 5000]), label, 1);
      c.observeSilent(silent(["p1", undefined], ["p2", 7000]), label, 2);
      expect(c.entries()).toEqual([]);
    });
  });
});

describe("NotificationCenter.recordSchedules", () => {
  const alert = (over: Partial<ScheduleAlert> = {}): ScheduleAlert => ({
    id: 1,
    seq: 1,
    key: "k",
    dir: "/repo",
    mode: "loop",
    outcome: "skipped",
    reason: "paused",
    at_ms: 1000,
    count: 1,
    ...over,
  });
  const failed = (seq: number) =>
    alert({ id: seq, seq, outcome: "failed", reason: "exit code 3", at_ms: seq * 1000 });

  it("names the mode, directory and reason of a failed run, one entry each", () => {
    const c = center();
    c.recordSchedules([failed(2), failed(3)]);
    expect(c.entries().map((e) => e.time)).toEqual([3000, 2000]);
    expect(c.entries()[0]).toMatchObject({
      label: "loop · /repo",
      message: "Scheduled run loop in /repo failed: exit code 3",
      status: "error",
      paneId: "",
      read: false,
    });
    expect(c.unread()).toBe(2);
  });

  it("shows the count of a collapsed skip", () => {
    const c = center();
    c.recordSchedules([alert({ count: 4, at_ms: 9000 })]);
    expect(c.entries()).toHaveLength(1);
    expect(c.entries()[0]).toMatchObject({
      message: "Scheduled run loop in /repo skipped 4 times: paused",
      status: "silent",
      time: 9000,
    });
  });

  it("leaves the count out for a single skip", () => {
    const c = center();
    c.recordSchedules([alert()]);
    expect(c.entries()[0].message).toBe("Scheduled run loop in /repo skipped: paused");
  });

  it("keeps one entry per schedule and reason and adds up the acknowledged alerts", () => {
    const c = center();
    c.recordSchedules([alert({ count: 2 })]);
    c.markAllRead();
    c.recordSchedules([alert({ id: 5, seq: 5, at_ms: 5000 })]);
    expect(c.entries()).toHaveLength(1);
    expect(c.entries()[0]).toMatchObject({
      message: "Scheduled run loop in /repo skipped 3 times: paused",
      time: 5000,
      read: false,
    });
  });

  it("does not count an alert twice that changed before it was acknowledged", () => {
    const c = center();
    c.recordSchedules([alert({ count: 2 })]);
    c.recordSchedules([alert({ id: 4, seq: 4, count: 1 })]);
    c.recordSchedules([alert({ id: 4, seq: 6, count: 3, at_ms: 6000 })]);
    expect(c.entries().map((e) => e.message)).toEqual([
      "Scheduled run loop in /repo skipped 5 times: paused",
    ]);
  });

  it("adds an alert once when it is read again", () => {
    const c = center();
    c.recordSchedules([alert(), failed(2)]);
    const before = c.entries();
    c.recordSchedules([alert(), failed(2)]);
    expect(c.entries()).toEqual(before);
  });

  it("keeps skips of another reason or schedule apart", () => {
    const c = center();
    c.recordSchedules([
      alert(),
      alert({ id: 2, seq: 2, reason: "the limit of runs per day is reached" }),
      alert({ id: 3, seq: 3, key: "other" }),
    ]);
    expect(c.entries()).toHaveLength(3);
  });

  it("ignores an empty list", () => {
    const c = center();
    c.recordSchedules([]);
    expect(c.entries()).toEqual([]);
  });
});
