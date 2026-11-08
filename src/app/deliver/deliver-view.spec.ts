// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { ACTIVITY_VIEWS } from "../activity-bar/activity-bar";
import { POIMEN_COMMANDS } from "../core/command-palette";
import { DELIVER_TAB, EditorTabs, tabKind } from "../core/editor-tabs";
import type { Terminal } from "../core/herdr";
import { TauriBridge } from "../core/tauri";
import { tauri } from "../core/tauri.mock";
import { UI_STORAGE } from "../core/ui-state";
import { Deliver, loopPane, milestoneNumber } from "./deliver";
import { DeliverView, prLabel } from "./deliver-view";
import type { DeliverRead } from "./deliver-status";

const opener = vi.hoisted(() => ({ openUrl: vi.fn() }));
vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("../core/tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("../core/tauri.mock")).tauri.listen,
}));
vi.mock("@tauri-apps/plugin-opener", () => opener);
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

beforeAll(() => {
  try {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  } catch {
    // already initialized by the shared setup
  }
});

const read = (name: string): DeliverRead => ({
  kind: "ok",
  status: JSON.parse(readFileSync(`src/app/deliver/fixtures/status-${name}.json`, "utf8")),
});

async function render() {
  TestBed.resetTestingModule();
  TestBed.overrideComponent(DeliverView, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync("src/app/deliver/deliver-view.html", "utf8"),
    },
  });
  const fixture = TestBed.createComponent(DeliverView);
  const deliver = TestBed.inject(Deliver);
  const el = fixture.nativeElement as HTMLElement;
  const show = async () => {
    await fixture.whenStable();
    fixture.detectChanges();
  };
  await show();
  return { fixture, deliver, el, show };
}

const text = (el: Element | null) => el?.textContent?.replace(/\s+/g, " ").trim() ?? "";
const button = (el: HTMLElement, label: string) =>
  [...el.querySelectorAll("button")].find((b) => text(b).includes(label))!;

beforeEach(() => {
  tauri.reset();
  opener.openUrl.mockReset();
});

describe("Deliver panel", () => {
  it("explains where loops come from while there are none", async () => {
    const { el } = await render();
    expect(text(el.querySelector("[data-testid=empty]"))).toContain(".agents/events/<run>.jsonl");
    expect(el.querySelectorAll("[data-testid=card]")).toHaveLength(0);
  });

  it("shows a running loop: milestone, progress, current PBI and step, delivered PBIs", async () => {
    const { el, deliver, show } = await render();
    deliver.apply("/src/skills", read("running"));
    deliver.now.set(Date.parse("2026-11-06T10:43:37Z"));
    await show();
    expect(el.querySelectorAll("[data-testid=card]")).toHaveLength(1);
    expect(text(el.querySelector("h2"))).toContain("M4 Deliver loop");
    expect(text(el.querySelector("[data-testid=progress]"))).toBe("3/9 closed");
    expect(el.querySelector(".fill")?.getAttribute("style")).toContain("33");
    expect(text(el.querySelector("[data-testid=current]"))).toContain("#332 [PBI] feat(delivery)");
    expect(text(el.querySelector("[data-testid=step]"))).toBe("step 3/7");
    expect(text(el.querySelector("[data-testid=current]"))).toContain("implement: tests green");
    expect(text(el.querySelector("[data-testid=state]"))).toBe("running, updated 2 min ago");
    expect(text(el.querySelector("[data-testid=delivered]"))).toContain("#325 → PR #340");
    expect(el.querySelector("[data-testid=needs-you]")).toBeNull();
    expect(el.querySelector("[data-testid=start]")).toBeNull();
  });

  it("shows no pull request link for a delivered entry whose prUrl is the issue URL (#538)", async () => {
    const { el, deliver, show } = await render();
    const issue = "https://github.com/GravionLabs/poimen/issues/334";
    const file = read("running");
    if (file.kind !== "ok") throw new Error("fixture");
    deliver.apply("/src/skills", {
      ...file,
      status: { ...(file.status as object), delivered: [{ pbi: 334, url: issue, prUrl: issue }] },
    });
    await show();
    const row = text(el.querySelector("[data-testid=delivered]"));
    expect(row).toContain("#334");
    expect(row).not.toContain(issue);
    expect(row).not.toContain("PR #");
    expect(el.querySelectorAll("[data-testid=delivered] button")).toHaveLength(1);
  });

  it("lists blocked, paused and needs-you items with their reasons", async () => {
    const { el, deliver, show } = await render();
    deliver.apply("/src/skills", read("blocked"));
    await show();
    expect(text(el.querySelector("[data-testid=blocked]"))).toContain("CI red on the PR");
    expect(text(el.querySelector("[data-testid=paused]"))).toContain("review round 2");
    expect(text(el.querySelector("[data-testid=needs-you]"))).toContain(
      "decide whether the schema field names are final",
    );
    expect(el.querySelector("[data-testid=current]")).toBeNull();
  });

  it("shows the release pull request and how long ago the run finished", async () => {
    const { el, deliver, show } = await render();
    deliver.apply("/src/skills", read("done"));
    const done = JSON.parse(readFileSync("src/app/deliver/fixtures/status-done.json", "utf8")) as {
      updatedAt: string;
      release: { prUrl: string };
    };
    deliver.now.set(Date.parse(done.updatedAt) + 12 * 60_000);
    await show();
    expect(text(el.querySelector("[data-testid=state]"))).toBe("finished 12 min ago");
    expect(text(el.querySelector("[data-testid=release]"))).toContain(prLabel(done.release.prUrl));
    expect(el.querySelector("[data-testid=start]")).not.toBeNull();
  });

  it("marks a running loop without updates for over an hour as stale", async () => {
    const { el, deliver, show } = await render();
    deliver.apply("/src/skills", read("running"));
    deliver.now.set(Date.parse("2026-11-06T10:41:37Z") + 61 * 60_000);
    await show();
    expect(text(el.querySelector("[data-testid=state]"))).toContain("stale");
    expect(el.querySelector(".card.state-stale")).not.toBeNull();
  });

  it("shows a hint for an unknown schema version and for a broken file", async () => {
    const { el, deliver, show } = await render();
    deliver.apply("/a", { kind: "unsupported", schemaVersion: 2 });
    deliver.apply("/b", { kind: "invalid", error: "expected value at line 1" });
    await show();
    expect(text(el.querySelector("[data-testid=unsupported]"))).toContain("schema version 2");
    expect(text(el.querySelector("[data-testid=invalid]"))).toContain("expected value");
    expect(el.querySelectorAll("[data-testid=card]")).toHaveLength(2);
  });

  it("drops the card when the file disappears", async () => {
    const { el, deliver, show } = await render();
    deliver.apply("/src/skills", read("running"));
    deliver.apply("/src/skills", { kind: "missing" });
    await show();
    expect(el.querySelectorAll("[data-testid=card]")).toHaveLength(0);
  });

  it("opens the milestone, issue and pull request links in the browser, GitHub only", async () => {
    const { el, deliver, show } = await render();
    deliver.apply("/src/skills", read("running"));
    await show();
    button(el, "Open milestone").click();
    expect(opener.openUrl).toHaveBeenCalledWith(
      "https://github.com/GravionLabs/skills/milestone/4",
    );
    button(el, "#332").click();
    expect(opener.openUrl).toHaveBeenLastCalledWith(
      "https://github.com/GravionLabs/skills/issues/332",
    );
    opener.openUrl.mockClear();
    await deliver.openUrl("https://evil.example/x");
    await deliver.openUrl(null);
    expect(opener.openUrl).not.toHaveBeenCalled();
  });

  it("focuses the terminal of the loop when there is one", async () => {
    const { fixture, el, deliver, show } = await render();
    deliver.apply("/src/skills", read("running"));
    vi.spyOn(deliver, "paneOf").mockReturnValue("t7");
    const chosen: string[] = [];
    fixture.componentInstance.paneChosen.subscribe((id) => chosen.push(id));
    await show();
    button(el, "Focus terminal").click();
    expect(chosen).toEqual(["t7"]);
  });

  it("starts the loop for the milestone through the deliver-loop task mode", async () => {
    tauri.answer("task_plan", () => {
      throw new Error("no task mode named deliver-loop");
    });
    const { el, deliver, show } = await render();
    deliver.apply("/src/skills", read("done"));
    await show();
    button(el, "Start loop").click();
    await vi.waitFor(() => expect(tauri.calls("task_plan")).toHaveLength(1));
    expect(tauri.calls("task_plan")[0]).toMatchObject({
      repoDir: "/src/skills",
      mode: "deliver-loop",
      issue: { nr: 3, title: "M3 Roadmap and release" },
    });
  });
});

describe("Deliver panel with progress events", () => {
  const events = (...names: string[]): DeliverRead => ({
    kind: "events",
    runs: names.map((n) =>
      JSON.parse(readFileSync(`src/app/deliver/fixtures/events-${n}.json`, "utf8")),
    ),
  });

  it("shows a loop and an interactive session side by side, colored by tone", async () => {
    const { el, deliver, show } = await render();
    deliver.apply("/r", events("blocked", "session"));
    await show();
    const cards = [...el.querySelectorAll("[data-testid=card]")];
    expect(cards).toHaveLength(2);
    expect(cards.map((c) => c.getAttribute("data-tone"))).toEqual(["danger", "neutral"]);
    expect(cards.map((c) => text(c.querySelector("[data-testid=source]")))).toEqual([
      "loop",
      "session",
    ]);
    expect(text(cards[1].querySelector("h2"))).toContain("Interactive deliver");
    expect(cards[1].querySelector("[data-testid=progress]")).toBeNull();
    expect(cards[1].querySelector("[data-testid=start]")).toBeNull();
  });

  it("shows the RC and the open RC test as needs you without marking the run blocked", async () => {
    const { el, deliver, show } = await render();
    deliver.apply("/r", events("rc-test"));
    await show();
    expect(text(el.querySelector("[data-testid=rc]"))).toBe("3.2.0-rc.1");
    expect(text(el.querySelector("[data-testid=release]"))).toContain("PR #391");
    expect(text(el.querySelector("[data-testid=needs-you]"))).toContain("#400 RC test");
    expect(el.querySelector("[data-testid=blocked]")).toBeNull();
    expect(el.querySelector("[data-testid=card]")?.getAttribute("data-tone")).toBe("success");
    button(el, "3.2.0-rc.1").click();
    expect(opener.openUrl).toHaveBeenCalledWith(
      "https://github.com/GravionLabs/skills/releases/tag/v3.2.0-rc.1",
    );
  });

  it("shows an abandoned run as stale", async () => {
    const { el, deliver, show } = await render();
    deliver.apply("/r", events("abandoned"));
    deliver.now.set(Date.parse("2026-11-07T12:00:00Z"));
    await show();
    expect(el.querySelector(".card.state-stale")).not.toBeNull();
    expect(text(el.querySelector("[data-testid=state]"))).toContain("stale");
  });

  it("hints at events of a version it does not know, keeping the readable runs", async () => {
    const { el, deliver, show } = await render();
    deliver.apply("/r", { ...events("blocked"), unknownVersion: 2 } as DeliverRead);
    await show();
    expect(text(el.querySelector("[data-testid=events-version]"))).toContain("version 2");
    expect(el.querySelectorAll("[data-testid=card]")).toHaveLength(2);
  });

  it("keeps the order of the runs on an update and drops the runs that are gone", async () => {
    const { deliver } = await render();
    deliver.apply("/r", events("blocked", "session"));
    const order = () =>
      deliver.cards().map((c) => (c.state.kind === "ok" ? c.state.status.source : c.state.kind));
    expect(order()).toEqual(["loop", "session"]);
    deliver.apply("/r", events("session", "blocked"));
    expect(order()).toEqual(["loop", "session"]);
    deliver.apply("/r", events("session"));
    expect(order()).toEqual(["session"]);
  });

  it("does not notify for the first read and notifies for a new decision of a run", async () => {
    const { deliver } = await render();
    const notify = vi.spyOn(TestBed.inject(TauriBridge), "notify").mockResolvedValue(true);
    deliver.apply("/r", events("session"));
    expect(notify).not.toHaveBeenCalled();
    const next = JSON.parse(readFileSync("src/app/deliver/fixtures/events-session.json", "utf8"));
    next.needsYou = [{ number: 5, url: "u", reason: "pick one" }];
    deliver.apply("/r", { kind: "events", runs: [next] });
    expect(notify.mock.calls.map((c) => c[1])).toEqual(["#5 needs you: pick one"]);
  });
});

describe("Deliver service", () => {
  it("receives deliver events once started, and stops listening", async () => {
    const { deliver } = await render();
    const stop = deliver.start();
    await vi.waitFor(() => expect(tauri.listen).toHaveBeenCalledWith("deliver", expect.anything()));
    tauri.emit("deliver", { root: "/r", read: read("running") });
    expect(deliver.cards()).toHaveLength(1);
    stop();
    await vi.waitFor(() => expect(tauri.unlisten).toHaveBeenCalledWith("deliver"));
    tauri.emit("deliver", { root: "/r2", read: read("running") });
    expect(deliver.cards()).toHaveLength(1);
  });

  it("watches the directories once and forgets cards of repositories that are gone", async () => {
    tauri.answer("deliver_watch", () => ["/a"]);
    const { deliver } = await render();
    deliver.apply("/a", read("running"));
    deliver.apply("/gone", read("done"));
    await deliver.watch(["/a/x", "/a", "/a/x"]);
    await deliver.watch(["/a", "/a/x"]);
    expect(tauri.calls("deliver_watch")).toEqual([{ dirs: ["/a/x", "/a"] }]);
    expect(deliver.cards().map((c) => c.root)).toEqual(["/a"]);
  });

  it("tries again when the watch fails", async () => {
    tauri.answer("deliver_watch", () => {
      throw new Error("boom");
    });
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
    const { deliver } = await render();
    await deliver.watch(["/a"]);
    await deliver.watch(["/a"]);
    expect(tauri.calls("deliver_watch")).toHaveLength(2);
  });

  describe("notifications", () => {
    const set = (name: string, over: Record<string, unknown>): DeliverRead => ({
      kind: "ok",
      status: {
        ...JSON.parse(readFileSync(`src/app/deliver/fixtures/status-${name}.json`, "utf8")),
        ...over,
      },
    });

    it("notifies when a run becomes blocked, needs a decision, opens the release PR or ends", async () => {
      const { deliver } = await render();
      const notify = vi.spyOn(TestBed.inject(TauriBridge), "notify").mockResolvedValue(true);
      deliver.apply("/r", read("running"));
      expect(notify).not.toHaveBeenCalled();
      deliver.apply(
        "/r",
        set("running", { needsYou: [{ number: 9, url: "u", reason: "pick one" }] }),
      );
      deliver.apply(
        "/r",
        set("running", {
          needsYou: [{ number: 9, url: "u", reason: "pick one" }],
          blocked: [{ number: 335, url: "u", reason: "CI red" }],
        }),
      );
      deliver.apply(
        "/r",
        set("running", {
          release: { reviewRound: 1, prUrl: "https://github.com/o/n/pull/9" },
          closed: 9,
          result: "done",
        }),
      );
      expect(notify.mock.calls.map((c) => c[1])).toEqual([
        "#9 needs you: pick one",
        "#335 is blocked: CI red",
        "The release pull request is open",
        "The run is done",
      ]);
      expect(notify.mock.calls[0][2]).toEqual({ deliverRoot: "/r" });
    });

    it("does not notify for the first read of a repository", async () => {
      const { deliver } = await render();
      const notify = vi.spyOn(TestBed.inject(TauriBridge), "notify").mockResolvedValue(true);
      deliver.apply("/r", read("blocked"));
      expect(notify).not.toHaveBeenCalled();
    });
  });
});

describe("helpers", () => {
  const pane = (id: string, cwd: string, agent?: string): Terminal =>
    ({ terminal_id: id, tab_id: "t", space_id: "s", agent_status: "idle", cwd, agent }) as Terminal;

  it("finds the agent pane of the loop inside the repository, preferring its harness", () => {
    const panes = [
      pane("shell", "/src/skills"),
      pane("other", "/src/skills/sub", "copilot"),
      pane("loop", "/src/skills", "claude"),
      pane("elsewhere", "/src/skills-other", "claude"),
    ];
    expect(loopPane(panes, "/src/skills", "claude")).toBe("loop");
    expect(loopPane(panes, "/src/skills", "codex")).toBe("other");
    expect(loopPane([panes[0], panes[3]], "/src/skills", "claude")).toBeUndefined();
  });

  it("reads the milestone number of a key", () => {
    expect(milestoneNumber("M5")).toBe(5);
    expect(milestoneNumber("M5.1")).toBe(5);
    expect(milestoneNumber("x")).toBe(0);
  });

  it("labels a pull request URL", () => {
    expect(prLabel("https://github.com/o/n/pull/343")).toBe("PR #343");
    expect(prLabel("https://example.com")).toBe("https://example.com");
  });
});

describe("entry points", () => {
  it("is a tab in the activity bar, the palette and the editor tabs", () => {
    expect(ACTIVITY_VIEWS.find((v) => v.id === "deliver")).toMatchObject({
      label: "Deliver",
      tab: "deliver",
    });
    expect(POIMEN_COMMANDS).toContainEqual({ id: "openDeliver", title: "Open Deliver" });
    expect(tabKind(DELIVER_TAB)).toBe("deliver");
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({
      providers: [
        { provide: UI_STORAGE, useValue: { getItem: () => null, setItem: () => undefined } },
      ],
    });
    const tabs = TestBed.inject(EditorTabs);
    tabs.openDeliver();
    tabs.openDeliver();
    expect(tabs.tabs().filter((t) => t.paneId === DELIVER_TAB)).toEqual([
      { paneId: DELIVER_TAB, pinned: true },
    ]);
  });
});
