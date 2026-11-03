import { Injector } from "@angular/core";
import { describe, expect, it } from "vitest";
import { Notifier, type NotifyContext } from "./notifier";
import { TauriBridge } from "./tauri";

function setup() {
  const sent: string[] = [];
  const extras: unknown[] = [];
  const calls = { focus: 0 };
  let clickHandler: (extra: Record<string, unknown>) => void = () => undefined;
  const bridge = {
    notify: (_title: string, body: string, extra?: Record<string, unknown>) => {
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
  const ctx = (o: Partial<NotifyContext> = {}): NotifyContext => ({
    viewed: new Set(),
    focused: false,
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
    calls,
    click: (extra: Record<string, unknown>) => clickHandler(extra),
    ctx,
  };
}

describe("Notifier", () => {
  it("notifies blocked and done while the window is in the background", () => {
    const { notifier, sent, ctx } = setup();
    notifier.notify(
      [
        { paneId: "p1", status: "blocked" },
        { paneId: "p2", status: "done" },
      ],
      ctx(),
    );
    expect(sent).toEqual(["tab p1 is waiting for you", "tab p2 is done"]);
  });

  it("stays quiet for a visible tab in a focused window, but not for another tab", () => {
    const { notifier, sent, ctx } = setup();
    const changes = [{ paneId: "p1", status: "blocked" as const }];
    notifier.notify(changes, ctx({ focused: true, viewed: new Set(["p1"]) }));
    expect(sent).toEqual([]);
    notifier.notify(changes, ctx({ focused: true, viewed: new Set(["p9"]) }));
    expect(sent).toHaveLength(1);
  });

  it("notifies a viewed tab when the window is not focused", () => {
    const { notifier, sent, ctx } = setup();
    notifier.notify([{ paneId: "p1", status: "done" }], ctx({ viewed: new Set(["p1"]) }));
    expect(sent).toHaveLength(1);
  });

  it("honors the per-kind settings", () => {
    const { notifier, sent, ctx } = setup();
    const changes = [
      { paneId: "p1", status: "blocked" as const },
      { paneId: "p2", status: "done" as const },
    ];
    notifier.notify(changes, ctx({ onDone: false }));
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
});
