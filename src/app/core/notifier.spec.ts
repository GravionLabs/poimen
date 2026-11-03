import { Injector } from "@angular/core";
import { describe, expect, it } from "vitest";
import { Notifier, type NotifyContext } from "./notifier";
import { TauriBridge } from "./tauri";

function setup() {
  const sent: string[] = [];
  const bridge = {
    notify: (_title: string, body: string) => {
      sent.push(body);
      return Promise.resolve(true);
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
  return { notifier: injector.get(Notifier), sent, ctx };
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

  it("returns the notified pane once when the window is focused soon after", () => {
    const { notifier, ctx } = setup();
    notifier.notify([{ paneId: "p1", status: "blocked" }], ctx({ now: 1000 }));
    expect(notifier.takeClicked(5000)).toBe("p1");
    expect(notifier.takeClicked(6000)).toBeNull();
  });

  it("ignores a focus long after the notification and when nothing was shown", () => {
    const { notifier, ctx } = setup();
    expect(notifier.takeClicked(1)).toBeNull();
    notifier.notify([{ paneId: "p1", status: "done" }], ctx({ now: 1000 }));
    expect(notifier.takeClicked(1000 + 31_000)).toBeNull();
  });

  it("does not arm a jump when the window was already focused", () => {
    const { notifier, ctx } = setup();
    notifier.notify([{ paneId: "p1", status: "blocked" }], ctx({ focused: true }));
    expect(notifier.takeClicked(1001)).toBeNull();
  });
});
