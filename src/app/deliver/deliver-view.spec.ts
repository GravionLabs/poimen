// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { ACTIVITY_VIEWS } from "../activity-bar/activity-bar";
import { POIMEN_COMMANDS } from "../core/command-palette";
import { DELIVER_TAB, EditorTabs, tabKind } from "../core/editor-tabs";
import type { Terminal } from "../core/snapshot";
import { tauri } from "../core/tauri.mock";
import { UI_STORAGE } from "../core/ui-state";
import { SpacesStore } from "../core/spaces-store";
import { Machines } from "../core/machines";
import { Deliver, loopPane, milestoneNumber } from "./deliver";
import { DeliverView, linkLabel, prLabel, stepText } from "./deliver-view";
import { cardsOf, type DeliverCard, type DeliverRead, type DeliverStatus } from "./deliver-status";

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

const events = (...names: string[]): DeliverRead => ({
  kind: "events",
  runs: names.map((n) =>
    JSON.parse(readFileSync(`src/app/deliver/fixtures/events-${n}.json`, "utf8")),
  ),
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
  it("explains where runs come from while there are none", async () => {
    const { el } = await render();
    expect(text(el.querySelector("[data-testid=empty]"))).toContain(".agents/events/<run>.jsonl");
    expect(el.querySelectorAll("[data-testid=card]")).toHaveLength(0);
  });

  it("shows a run from the core: subject, progress, open tasks with step, links", async () => {
    const { el, deliver, show } = await render();
    deliver.apply("/r", events("milestone"));
    deliver.now.set(Date.parse("2026-11-08T12:00:00Z"));
    await show();
    expect(el.querySelectorAll("[data-testid=card]")).toHaveLength(1);
    expect(text(el.querySelector("h2"))).toContain("M4 Deliver loop");
    expect(text(el.querySelector("[data-testid=progress]"))).toBe("3/9 issues");
    expect(el.querySelector(".fill")?.getAttribute("style")).toContain("33");
    const tasks = [...el.querySelectorAll("[data-testid=task]")].map((t) => text(t));
    expect(tasks).toHaveLength(2);
    expect(tasks[0]).toContain("Second");
    expect(tasks[0]).toContain("implement 2/3");
    expect(tasks[0]).toContain("tests green");
    expect(text(el.querySelector("[data-testid=task-progress]"))).toBe("1/3 steps");
    // A subject of a kind poimen does not know is shown generically.
    expect(tasks[1]).toContain("A thing of a kind poimen does not know");
    expect(text(el.querySelector("[data-testid=state]"))).toBe("running, updated 2 min ago");
    expect(text(el.querySelector("[data-testid=links]"))).toContain("PR #21");
    expect(text(el.querySelector("[data-testid=links]"))).toContain("log");
  });

  it("lists what finished per outcome with reasons and links", async () => {
    const { el, deliver, show } = await render();
    deliver.apply("/r", events("milestone"));
    await show();
    const rows = [...el.querySelectorAll("[data-testid=finished] li")].map((r) => text(r));
    expect(rows[0]).toContain("First");
    expect(rows[0]).toContain("succeeded");
    expect(rows[0]).toContain("PR #21");
    expect(rows[1]).toContain("Zero");
    expect(rows[1]).toContain("failed: review rejected");
  });

  it("shows needs-you attentions apart from other notices, with their links", async () => {
    const { el, deliver, show } = await render();
    deliver.apply("/r", events("milestone"));
    await show();
    expect(text(el.querySelector("[data-testid=needs-you]"))).toContain(
      "decide whether the field names are final",
    );
    expect(text(el.querySelector("[data-testid=needs-you]"))).toContain("log");
    expect(text(el.querySelector("[data-testid=notices]"))).toContain("run-wide notice");
    expect(text(el.querySelector("[data-testid=notices]"))).not.toContain("decide");
  });

  it("colors the card by the tone of the run and shows the writer's agent", async () => {
    const { el, deliver, show } = await render();
    deliver.apply("/r", events("milestone", "failed"));
    await show();
    const cards = [...el.querySelectorAll("[data-testid=card]")];
    expect(cards.map((c) => c.getAttribute("data-tone"))).toEqual(["warning", "danger"]);
    expect(text(cards[0].querySelector("footer .hint"))).toBe("claude sonnet · /r");
    expect(text(cards[1].querySelector("[data-testid=state]"))).toContain("failed");
  });

  it("shows a run without subject, progress or agent from the core alone", async () => {
    const { el, deliver, show } = await render();
    deliver.apply("/r", events("session"));
    await show();
    const card = el.querySelector("[data-testid=card]")!;
    expect(text(card.querySelector("h2"))).toBe("r");
    expect(card.querySelector("[data-testid=progress]")).toBeNull();
    expect(card.querySelector("[data-testid=start]")).toBeNull();
    expect(card.querySelector("[data-testid=open-subject]")).toBeNull();
    expect(text(card.querySelector("[data-testid=step]"))).toBe("review");
  });

  it("marks a running run without updates for over an hour, and an abandoned one, as stale", async () => {
    const { el, deliver, show } = await render();
    deliver.apply("/r", events("milestone", "abandoned"));
    deliver.now.set(Date.parse("2026-11-08T11:58:00Z") + 61 * 60_000);
    await show();
    expect(el.querySelectorAll(".card.state-stale")).toHaveLength(2);
    expect(text(el.querySelector("[data-testid=state]"))).toContain("stale");
  });

  it("hints at events of a version it does not know, keeping the readable runs", async () => {
    const { el, deliver, show } = await render();
    deliver.apply("/r", { ...events("failed"), unknownVersion: 2 } as DeliverRead);
    await show();
    expect(text(el.querySelector("[data-testid=events-version]"))).toContain("version 2");
    expect(el.querySelectorAll("[data-testid=card]")).toHaveLength(2);
  });

  it("shows a hint for an unknown legacy schema version and for a broken file", async () => {
    const { el, deliver, show } = await render();
    deliver.apply("/a", { kind: "unsupported", schemaVersion: 2 });
    deliver.apply("/b", { kind: "invalid", error: "expected value at line 1" });
    await show();
    expect(text(el.querySelector("[data-testid=unsupported]"))).toContain("schema version 2");
    expect(text(el.querySelector("[data-testid=invalid]"))).toContain("expected value");
    expect(el.querySelectorAll("[data-testid=card]")).toHaveLength(2);
  });

  it("shows a legacy status file like any other run and drops the card when it disappears", async () => {
    const { el, deliver, show } = await render();
    deliver.apply("/src/skills", read("running"));
    await show();
    expect(text(el.querySelector("h2"))).toContain("M4");
    expect(text(el.querySelector("[data-testid=progress]"))).toBe("3/9");
    expect(text(el.querySelector("[data-testid=finished]"))).toContain("#325");
    deliver.apply("/src/skills", { kind: "missing" });
    await show();
    expect(el.querySelectorAll("[data-testid=card]")).toHaveLength(0);
  });

  it("opens subject, task and link URLs in the browser, GitHub only", async () => {
    const { el, deliver, show } = await render();
    deliver.apply("/r", events("milestone"));
    await show();
    button(el, "Open milestone").click();
    expect(opener.openUrl).toHaveBeenCalledWith("https://github.com/GravionLabs/demo/milestone/4");
    button(el, "Second").click();
    expect(opener.openUrl).toHaveBeenLastCalledWith(
      "https://github.com/GravionLabs/demo/issues/12",
    );
    opener.openUrl.mockClear();
    await deliver.openUrl("https://evil.example/x");
    await deliver.openUrl(null);
    expect(opener.openUrl).not.toHaveBeenCalled();
  });

  it("focuses the terminal of the run when there is one", async () => {
    const { fixture, el, deliver, show } = await render();
    deliver.apply("/r", events("milestone"));
    vi.spyOn(deliver, "paneOf").mockReturnValue("t7");
    const chosen: string[] = [];
    fixture.componentInstance.paneChosen.subscribe((id) => chosen.push(id));
    await show();
    button(el, "Focus terminal").click();
    expect(chosen).toEqual(["t7"]);
  });

  it("starts the loop for a finished milestone run through the deliver-loop task mode", async () => {
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

  it("offers no loop for a running run or a subject that is not a milestone", async () => {
    const { el, deliver, show } = await render();
    deliver.apply("/r", events("milestone", "abandoned"));
    deliver.now.set(Date.parse("2026-11-08T12:00:00Z"));
    await show();
    expect(el.querySelectorAll("[data-testid=start]")).toHaveLength(0);
  });
});

describe("Deliver service ordering", () => {
  it("keeps the order of the runs on an update and drops the runs that are gone", async () => {
    const { deliver } = await render();
    deliver.apply("/r", events("failed", "session"));
    const order = () =>
      deliver.cards().map((c) => (c.state.kind === "ok" ? c.state.status.tone : c.state.kind));
    expect(order()).toEqual(["danger", "neutral"]);
    deliver.apply("/r", events("session", "failed"));
    expect(order()).toEqual(["danger", "neutral"]);
    deliver.apply("/r", events("session"));
    expect(order()).toEqual(["neutral"]);
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
});

describe("remote runs (#692)", () => {
  const machine = (state: string) => ({
    name: "box",
    ssh: "box.lan",
    reposDir: "~/Source/{owner}/{repo}",
    state,
    ...(state === "connected" ? { serverVersion: "0.18.0" } : { error: "ssh: timeout" }),
  });

  async function withBox() {
    tauri.answer("list_machines", () => [machine("connected")]);
    const view = await render();
    const machines = TestBed.inject(Machines);
    await machines.start();
    return { ...view, machines };
  }

  it("merges a local and a remote run with the same run id, labelled with the machine", async () => {
    const { deliver, el, show } = await withBox();
    const stop = deliver.start();
    await vi.waitFor(() => expect(tauri.listen).toHaveBeenCalledWith("deliver", expect.anything()));
    tauri.emit("deliver", { root: "/r", read: events("session") });
    tauri.emit("deliver", { root: "/r", machine: "box", read: events("session") });
    // A replay after a reconnect updates the same card.
    tauri.emit("deliver", { root: "/r", machine: "box", read: events("session") });
    await show();
    expect(deliver.cards().map((c) => c.machine)).toEqual([undefined, "box"]);
    const cards = el.querySelectorAll("[data-testid=card]");
    expect(cards).toHaveLength(2);
    expect(cards[0]!.querySelector("[data-testid=machine]")).toBeNull();
    expect(text(cards[1]!.querySelector("[data-testid=machine]"))).toBe("box");
    expect(text(cards[1]!.querySelector("footer"))).toContain("box:/r");
    expect(cards[1]!.querySelector("[data-testid=offline]")).toBeNull();
    stop();
  });

  it("keeps the last state of an offline machine marked, and drops a removed one", async () => {
    const { deliver, el, show } = await withBox();
    deliver.apply("/r", events("milestone", "failed"), "box");
    tauri.emit("machine:state_changed", machine("offline"));
    await show();
    const card = el.querySelector("[data-testid=card]")!;
    expect(text(card.querySelector("[data-testid=offline]"))).toContain("offline");
    expect(card.classList).toContain("offline");
    expect(deliver.cards()).toHaveLength(2);
    // Remote runs are not started from here (#693).
    expect(el.querySelector("[data-testid=start]")).toBeNull();
    tauri.answer("list_machines", () => []);
    await TestBed.inject(Machines).refresh();
    await show();
    expect(deliver.cards()).toEqual([]);
  });

  it("keeps the remote cards when the local watch changes", async () => {
    tauri.answer("deliver_watch", () => ["/a"]);
    const { deliver } = await withBox();
    deliver.apply("/a", read("running"));
    deliver.apply("/remote", events("session"), "box");
    await deliver.watch(["/a"]);
    expect(deliver.cards().map((c) => c.root)).toEqual(["/a", "/remote"]);
    expect(await deliver.startLoop(MILESTONE_STATUS(), "/remote", "box")).toBe(false);
  });
});

const MILESTONE_STATUS = (): DeliverStatus =>
  (cardsOf("/r", events("milestone"))[0]!.state as { status: DeliverStatus }).status;

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

  it("reads the milestone number of a subject: the M<n> name, else the #<number> of the id", () => {
    expect(milestoneNumber({ id: "M5", title: "t" })).toBe(5);
    expect(milestoneNumber({ id: "M5.1", title: "t" })).toBe(5);
    expect(milestoneNumber({ id: "o/r#12", title: "M7 Roadmap" })).toBe(7);
    expect(milestoneNumber({ id: "o/r#12", title: "Roadmap" })).toBe(12);
    expect(milestoneNumber({ id: "x", title: "y" })).toBe(0);
  });

  it("names a link by its title, a pull request number or its rel", () => {
    expect(linkLabel({ rel: "log", url: "https://x", title: "CI log" })).toBe("CI log");
    expect(
      linkLabel({ rel: "pull-request", url: "https://github.com/o/n/pull/3", title: null }),
    ).toBe("PR #3");
    expect(linkLabel({ rel: "log", url: "https://x", title: null })).toBe("log");
    expect(stepText({ name: "implement", n: 2, total: 3, text: null })).toBe("implement 2/3");
    expect(stepText({ name: "review", n: null, total: null, text: null })).toBe("review");
  });

  it("finds the terminal of a run: the host terminal when open, else an agent pane", () => {
    const { deliver } = (() => {
      TestBed.resetTestingModule();
      return { deliver: TestBed.inject(Deliver) };
    })();
    const store = TestBed.inject(SpacesStore);
    vi.spyOn(store, "all").mockReturnValue({
      spaces: [],
      tabs: [],
      terminals: [
        pane("t7", "/elsewhere"),
        pane("t2", "/r", "claude"),
        { ...pane("box::t3", "/r", "claude"), machine: "box" },
      ],
    });
    const card = (host: Record<string, string>): DeliverCard => ({
      root: "/r",
      run: "x",
      state: {
        kind: "ok",
        status: {
          ...(cardsOf("/r", events("session"))[0]!.state as { status: DeliverStatus }).status,
          host,
        },
      },
    });
    expect(deliver.paneOf(card({ "poimen.terminal": "t7" }))).toBe("t7");
    expect(deliver.paneOf(card({ "poimen.terminal": "gone" }))).toBe("t2");
    expect(deliver.paneOf({ root: "/r", run: "", state: { kind: "invalid", error: "x" } })).toBe(
      undefined,
    );
    // A remote run finds only a terminal of its machine.
    const remote = (host: Record<string, string>) => ({ ...card(host), machine: "box" });
    expect(deliver.paneOf(remote({ "poimen.terminal": "box::t3" }))).toBe("box::t3");
    expect(deliver.paneOf(remote({ "poimen.terminal": "t7" }))).toBe("box::t3");
    expect(deliver.paneOf({ ...card({}), machine: "gpu" })).toBeUndefined();
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
