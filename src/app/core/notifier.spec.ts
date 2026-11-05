import { Injector } from "@angular/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AgentStatus } from "./herdr";
import { notificationMessage } from "./notification-center";
import { DONE_DELAY_MS, Notifier, type NotifyContext } from "./notifier";
import { TauriBridge } from "./tauri";

function setup() {
  const sent: string[] = [];
  const extras: unknown[] = [];
  const ids: (number | undefined)[] = [];
  const removed: number[][] = [];
  const calls = { focus: 0 };
  let clickHandler: (extra: Record<string, unknown>) => void = () => undefined;
  const bridge = {
    removeNotifications: (list: number[]) => {
      removed.push(list);
      return Promise.resolve();
    },
    notify: (_title: string, body: string, extra?: Record<string, unknown>, id?: number) => {
      ids.push(id);
      sent.push(body);
      extras.push(extra);
      return Promise.resolve(true);
    },
    focusWindow: () => {
      calls.focus++;
      return Promise.resolve();
    },
    onNotificationClick: (handler: (extra: Record<string, unknown>) => void) => {
      clickHandler = handler;
      return Promise.resolve(() => undefined);
    },
  };
  const injector = Injector.create({
    providers: [{ provide: TauriBridge, useValue: bridge }, Notifier],
  });
  const status = new Map<string, AgentStatus>();
  const seen = new Set<string>();
  const ctx = (o: Partial<NotifyContext> = {}): NotifyContext => ({
    seen: (id) => seen.has(id),
    status: (id) => status.get(id) ?? "done",
    onBlocked: true,
    onDone: true,
    label: (id) => `tab ${id}`,
    now: 1000,
    ...o,
  });
  return {
    notifier: injector.get(Notifier),
    sent,
    extras,
    ids,
    removed,
    calls,
    click: (extra: Record<string, unknown>) => clickHandler(extra),
    ctx,
    status,
    seen,
  };
}

describe("Notifier", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("notifies blocked and done while the window is in the background", () => {
    const { notifier, sent, ctx } = setup();
    notifier.notify(
      [
        { paneId: "p1", status: "blocked" },
        { paneId: "p2", status: "done" },
      ],
      ctx(),
    );
    expect(sent).toEqual(["tab p1 is waiting for you"]);
    vi.advanceTimersByTime(DONE_DELAY_MS);
    expect(sent).toEqual(["tab p1 is waiting for you", "tab p2 is done"]);
  });

  it("words the system notification like the notification center", () => {
    const { notifier, sent, ctx } = setup();
    notifier.notify(
      [
        { paneId: "p1", status: "blocked" },
        { paneId: "p2", status: "done" },
      ],
      ctx(),
    );
    vi.advanceTimersByTime(DONE_DELAY_MS);
    expect(sent).toEqual([
      notificationMessage("tab p1", "blocked"),
      notificationMessage("tab p2", "done"),
    ]);
  });

  it("stays quiet for a pane the user sees, but not for another one", () => {
    const { notifier, sent, ctx, seen } = setup();
    seen.add("p1");
    notifier.notify([{ paneId: "p1", status: "blocked" }], ctx());
    expect(sent).toEqual([]);
    notifier.notify([{ paneId: "p2", status: "blocked" }], ctx());
    expect(sent).toHaveLength(1);
  });

  it("sends done only after the pane stayed done for the delay", () => {
    const { notifier, sent, ctx } = setup();
    notifier.notify([{ paneId: "p1", status: "done" }], ctx());
    vi.advanceTimersByTime(DONE_DELAY_MS - 1);
    expect(sent).toEqual([]);
    vi.advanceTimersByTime(1);
    expect(sent).toEqual(["tab p1 is done"]);
  });

  it("drops a done that flickers back to working before the delay", () => {
    const { notifier, sent, ctx, status } = setup();
    notifier.notify([{ paneId: "p1", status: "done" }], ctx());
    vi.advanceTimersByTime(2000);
    status.set("p1", "working");
    vi.advanceTimersByTime(DONE_DELAY_MS);
    expect(sent).toEqual([]);
    status.set("p1", "done");
    notifier.notify([{ paneId: "p1", status: "done" }], ctx());
    vi.advanceTimersByTime(DONE_DELAY_MS);
    expect(sent).toEqual(["tab p1 is done"]);
  });

  it("drops a due done when the user looked at the pane meanwhile", () => {
    const { notifier, sent, ctx, seen } = setup();
    notifier.notify([{ paneId: "p1", status: "done" }], ctx());
    seen.add("p1");
    vi.advanceTimersByTime(DONE_DELAY_MS);
    expect(sent).toEqual([]);
  });

  it("sends done once per pane until the user has seen it", () => {
    const { notifier, sent, ctx, seen } = setup();
    const done = [{ paneId: "p1", status: "done" as const }];
    notifier.notify(done, ctx());
    vi.advanceTimersByTime(DONE_DELAY_MS);
    notifier.notify(done, ctx());
    vi.advanceTimersByTime(DONE_DELAY_MS);
    expect(sent).toEqual(["tab p1 is done"]);
    seen.add("p1");
    notifier.notify([], ctx());
    seen.delete("p1");
    notifier.notify(done, ctx());
    vi.advanceTimersByTime(DONE_DELAY_MS);
    expect(sent).toEqual(["tab p1 is done", "tab p1 is done"]);
  });

  it("keeps sending blocked every time, without delay", () => {
    const { notifier, sent, ctx } = setup();
    const blocked = [{ paneId: "p1", status: "blocked" as const }];
    notifier.notify(blocked, ctx());
    notifier.notify(blocked, ctx());
    expect(sent).toHaveLength(2);
  });

  it("honors the per-kind settings", () => {
    const { notifier, sent, ctx } = setup();
    const changes = [
      { paneId: "p1", status: "blocked" as const },
      { paneId: "p2", status: "done" as const },
    ];
    notifier.notify(changes, ctx({ onDone: false }));
    vi.advanceTimersByTime(DONE_DELAY_MS);
    expect(sent).toEqual(["tab p1 is waiting for you"]);
  });

  it("sends the pane id with each notification", () => {
    const { notifier, extras, ctx } = setup();
    notifier.notify(
      [
        { paneId: "p1", status: "blocked" },
        { paneId: "p2", status: "done" },
      ],
      ctx(),
    );
    vi.advanceTimersByTime(DONE_DELAY_MS);
    expect(extras).toEqual([{ paneId: "p1" }, { paneId: "p2" }]);
  });

  it("opens the pane of the clicked notification, not the last one", async () => {
    const { notifier, click } = setup();
    const opened: string[] = [];
    await notifier.listen((id) => opened.push(id));
    click({ paneId: "p1" });
    click({ paneId: "p2" });
    expect(opened).toEqual(["p1", "p2"]);
  });

  it("brings the window to the front on a click", async () => {
    const { notifier, click, calls } = setup();
    await notifier.listen(() => undefined);
    click({ paneId: "p1" });
    expect(calls.focus).toBe(1);
  });

  it("ignores a click without a pane id", async () => {
    const { notifier, click, calls } = setup();
    const opened: string[] = [];
    await notifier.listen((id) => opened.push(id));
    click({});
    click({ paneId: 7 });
    expect(opened).toEqual([]);
    expect(calls.focus).toBe(0);
  });

  it("withdraws the notifications of a pane once the user sees it", () => {
    const { notifier, ids, removed, ctx, seen, status } = setup();
    status.set("p1", "blocked");
    status.set("p2", "blocked");
    notifier.notify(
      [
        { paneId: "p1", status: "blocked" },
        { paneId: "p2", status: "blocked" },
      ],
      ctx(),
    );
    expect(ids).toHaveLength(2);
    expect(new Set(ids).size).toBe(2);
    seen.add("p1");
    notifier.notify([], ctx());
    expect(removed).toEqual([[ids[0]]]);
    notifier.notify([], ctx());
    expect(removed).toHaveLength(1);
  });

  it("withdraws a done notification too and nothing for unseen panes", () => {
    const { notifier, ids, removed, ctx, seen } = setup();
    notifier.notify([{ paneId: "p1", status: "done" }], ctx());
    vi.advanceTimersByTime(DONE_DELAY_MS);
    notifier.notify([], ctx());
    expect(removed).toEqual([]);
    seen.add("p1");
    notifier.notify([], ctx());
    expect(removed).toEqual([[ids[0]]]);
  });

  it("withdraws a blocked notification when the agent resumes unseen", () => {
    const { notifier, ids, removed, ctx, status } = setup();
    status.set("p1", "blocked");
    notifier.notify([{ paneId: "p1", status: "blocked" }], ctx());
    notifier.notify([], ctx());
    expect(removed).toEqual([]);
    status.set("p1", "working");
    notifier.notify([], ctx());
    expect(removed).toEqual([[ids[0]]]);
  });

  it("withdraws a done notification when the pane leaves done", () => {
    const { notifier, ids, removed, ctx, status } = setup();
    status.set("p1", "done");
    notifier.notify([{ paneId: "p1", status: "done" }], ctx());
    vi.advanceTimersByTime(DONE_DELAY_MS);
    status.set("p1", "working");
    notifier.notify([], ctx());
    expect(removed).toEqual([[ids[0]]]);
  });

  it("withdraws and forgets the notifications of a closed pane", () => {
    const { notifier, ids, removed, ctx } = setup();
    notifier.notify([{ paneId: "p1", status: "blocked" }], ctx({ status: () => "blocked" }));
    notifier.notify([], ctx({ status: () => undefined }));
    expect(removed).toEqual([[ids[0]]]);
    notifier.notify([], ctx({ status: () => undefined }));
    expect(removed).toHaveLength(1);
  });

  it("rechecks on window focus and stops after the returned function is called", () => {
    const { notifier, ids, removed, ctx, seen, status } = setup();
    status.set("p1", "blocked");
    notifier.notify([{ paneId: "p1", status: "blocked" }], ctx());
    const win = new EventTarget();
    const stop = notifier.watchFocus(win, () => ctx());
    seen.add("p1");
    win.dispatchEvent(new Event("focus"));
    expect(removed).toEqual([[ids[0]]]);
    stop();
    notifier.notify([{ paneId: "p2", status: "blocked" }], ctx());
    seen.add("p2");
    win.dispatchEvent(new Event("focus"));
    expect(removed).toHaveLength(1);
  });
});
