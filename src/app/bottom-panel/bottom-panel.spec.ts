// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { EditorTabs } from "../core/editor-tabs";
import type { Terminal } from "../core/snapshot";
import { SpacesStore } from "../core/spaces-store";
import { Deliver } from "../deliver/deliver";
import { TaskLog } from "../core/task-log";
import { UiState } from "../core/ui-state";
import { Subagents } from "../core/transcripts";
import { parseStatus, type DeliverCard } from "../deliver/deliver-status";
import { BottomPanel } from "./bottom-panel";
import { FlowView } from "./flow-view";
import { choose, optionLabels, shown } from "../../testing/select";

const named = "Claude Code and GitHub Copilot CLI";

const pane = (over: Partial<Terminal>): Terminal => ({
  terminal_id: "w1:p1",
  tab_id: "w1:t1",
  space_id: "w1",
  agent_status: "unknown",
  ...over,
});

describe("BottomPanel empty states", () => {
  beforeAll(() => {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting(), {
      teardown: { destroyAfterEach: true },
    });
  });

  /** Renders the panel on `tab` with the active tab on `panes` (one pane) and `supported`. */
  function render(
    tab: "output" | "flow" | "events",
    active: Terminal | undefined,
    supported: boolean,
    cards = signal<DeliverCard[]>([]),
    agents: { agent_id: string; description: string }[] = [],
  ) {
    const eventsAll = signal<Record<string, string>>({});
    const ui = {
      panelTab: signal(tab),
      panelHeight: signal(200),
      togglePanel: vi.fn(),
      eventColumns: signal({ time: 80, task: 80, result: 80 }),
      eventsAll,
      setEventsAll: (root: string, key: string | null) =>
        eventsAll.update((all) => {
          const next = { ...all };
          if (key) next[root] = key;
          else delete next[root];
          return next;
        }),
    };
    const subagents = {
      supported: signal(supported),
      agents: signal(agents),
      items: signal([]),
      paneId: signal(null),
      selected: signal(null),
      selectedId: signal(agents[0]?.agent_id ?? null),
      pick: vi.fn(),
    };
    const panes = new Map(active ? [[active.terminal_id, active]] : []);
    TestBed.resetTestingModule();
    // The panel's template is read from its file, as the build would resolve it.
    // The flow view is only rendered once the Flow tab has a followed agent; a stub keeps it out.
    TestBed.overrideComponent(FlowView, {
      set: { template: "", templateUrl: undefined, styleUrl: undefined },
    });
    TestBed.overrideComponent(BottomPanel, {
      set: {
        templateUrl: undefined,
        styleUrl: undefined,
        template: readFileSync("src/app/bottom-panel/bottom-panel.html", "utf8"),
      },
    });
    TestBed.configureTestingModule({
      providers: [
        { provide: UiState, useValue: ui },
        { provide: Subagents, useValue: subagents },
        {
          provide: EditorTabs,
          useValue: { activeId: signal(active ? active.terminal_id : null), openFlow: vi.fn() },
        },
        { provide: Deliver, useValue: { cards, openUrl: vi.fn() } },
        { provide: TaskLog, useValue: { lines: signal([]) } },
        { provide: SpacesStore, useValue: { panes: signal(panes) } },
      ],
    });
    const fixture = TestBed.createComponent(BottomPanel);
    fixture.detectChanges();
    return {
      fixture,
      subagents,
      text: () =>
        (fixture.nativeElement as HTMLElement).querySelector(".empty")?.textContent?.trim(),
      show: (next: "output" | "flow" | "events") => {
        ui.panelTab.set(next);
        fixture.detectChanges();
      },
    };
  }

  it("names Claude Code and GitHub Copilot CLI on the Output tab", () => {
    const panel = render("output", pane({ agent: "codex" }), false);
    expect(panel.text()).toBe(
      `Subagent output is available for ${named} agents. This pane does not run one.`,
    );
  });

  it("picks a subagent from the select of the Output tab", async () => {
    const panel = render("output", pane({ agent: "claude" }), true, signal<DeliverCard[]>([]), [
      { agent_id: "a1", description: "first" },
      { agent_id: "a2", description: "second" },
    ]);
    const select = (panel.fixture.nativeElement as HTMLElement).querySelector("hx-select")!;
    const settle = () => panel.fixture.detectChanges();
    const labels = await optionLabels(select, settle);
    expect(labels).toEqual([expect.stringContaining("first"), expect.stringContaining("second")]);
    expect(shown(select)).toBe(labels[0]);
    await choose(select, labels[1], settle);
    expect(panel.subagents.pick.mock.calls).toEqual([["a2"]]);
  });

  it("names both harnesses on the Flow tab", () => {
    const panel = render("flow", pane({}), false);
    panel.show("flow");
    expect(panel.text()).toBe(
      `The flow is available for ${named} agents. This pane does not run one.`,
    );
  });

  it("waits for the session of a known agent, on both tabs", () => {
    const panel = render("output", pane({ agent: "claude" }), false);
    expect(panel.text()).toBe("Waiting for the agent to report its session.");
    panel.show("flow");
    expect(panel.text()).toBe("Waiting for the agent to report its session.");
  });

  it("waits for the transcript once the session is reported", () => {
    const active = pane({ agent: "copilot", agent_session: { value: "s1" } });
    const panel = render("flow", active, false);
    expect(panel.text()).toBe("Waiting for the agent's transcript to appear.");
  });

  it("shows the same empty state on the Events tab for Claude and Copilot", () => {
    const text = "No run in this space yet.";
    for (const agent of ["claude", "copilot"]) {
      const panel = render("events", pane({ agent }), false);
      expect(panel.text()).toContain(text);
    }
  });

  it("asks for a pane when the active tab has none", () => {
    const panel = render("output", undefined, false);
    expect(panel.text()).toBe(
      `Subagent output is available for ${named} agents. Select a pane that runs one.`,
    );
  });

  it("drops a picked run when a run of the repository starts after it", () => {
    const run = (id: string, ts: string): DeliverCard => {
      const status = parseStatus({
        v: 1,
        run: id,
        status: "running",
        tasks: [],
        finished: {},
        attentions: [],
        needsYou: false,
        links: [],
        updated: new Date().toISOString(),
        tone: "neutral",
        host: { "poimen.terminal": "w1:p1" },
        events: [{ type: "run.started", tone: "neutral", ts }],
      });
      if (typeof status === "string") throw new Error(status);
      return { root: "/repo", run: id, state: { kind: "ok", status } };
    };
    const cards = signal([
      run("a", "2026-11-07T10:00:00.000Z"),
      run("b", "2026-11-07T11:00:00.000Z"),
    ]);
    const panel = render("events", pane({ cwd: "/repo" }), false, cards);
    const root = panel.fixture.nativeElement as HTMLElement;
    const select = () => root.querySelector<HTMLSelectElement>("select[aria-label=Run]")!;
    const shown = () => select().selectedOptions[0]?.textContent ?? "";
    panel.fixture.detectChanges();
    // Newest first, and the newest run is shown.
    expect(select().options.length).toBe(2);
    select().selectedIndex = 1;
    select().dispatchEvent(new Event("change"));
    panel.fixture.detectChanges();
    const picked = shown();
    // The older run stays shown after the pick.
    expect(select().selectedIndex).toBe(1);
    expect(picked).not.toBe(select().options[0]!.textContent);
    cards.update((c) => [...c, run("c", "2026-11-07T12:00:00.000Z")]);
    panel.fixture.detectChanges();
    panel.fixture.detectChanges();
    expect(select().options.length).toBe(3);
    expect(select().selectedIndex).toBe(0);
    expect(shown()).not.toBe(picked);
  });

  it("shows skipped tasks as one row that opens to the list and stays open", () => {
    const ev = (type: string, task: string, extra: Record<string, unknown> = {}) => ({
      type,
      tone: "neutral",
      ts: "2026-11-07T10:00:00.000Z",
      task,
      ...extra,
    });
    const status = parseStatus({
      v: 1,
      run: "a",
      status: "running",
      tasks: [],
      finished: {},
      attentions: [],
      needsYou: false,
      links: [],
      updated: new Date().toISOString(),
      tone: "neutral",
      host: { "poimen.terminal": "w1:p1" },
      events: [
        ev("task.started", "t1", { subject: "PBI one", text: "waits" }),
        ev("task.finished", "t1", {
          outcome: "skipped",
          reason: "blocked",
          text: "Blocked by #12",
        }),
        ev("task.started", "t2", { subject: "PBI two" }),
        ev("task.finished", "t2", { outcome: "skipped", reason: "blocked" }),
      ],
    });
    if (typeof status === "string") throw new Error(status);
    const cards = signal<DeliverCard[]>([
      { root: "/repo", run: "a", state: { kind: "ok", status } },
    ]);
    const panel = render("events", pane({ cwd: "/repo" }), false, cards);
    const root = panel.fixture.nativeElement as HTMLElement;
    panel.fixture.detectChanges();
    expect(root.querySelectorAll("[data-testid=skipped-fold]").length).toBe(1);
    expect(root.querySelector("[data-testid=skipped-fold]")!.textContent).toContain(
      "2 skipped (Blocked by #12, blocked)",
    );
    expect(root.querySelectorAll("[data-testid=skipped-task]").length).toBe(0);
    root.querySelector<HTMLButtonElement>(".fold-toggle")!.click();
    panel.fixture.detectChanges();
    const tasks = root.querySelectorAll("[data-testid=skipped-task]");
    expect(tasks.length).toBe(2);
    expect(tasks[0]!.textContent).toContain("PBI one: waits");
    // `text ?? reason` (#622): the sentence when present, else the code.
    expect(tasks[0]!.textContent).toContain("Blocked by #12");
    expect(tasks[1]!.textContent).toContain("blocked");
    // A new event keeps the fold open.
    cards.update((c) => [...c]);
    panel.fixture.detectChanges();
    expect(root.querySelectorAll("[data-testid=skipped-task]").length).toBe(2);
  });

  it("lists the events of all runs of a subject with a separator per run", () => {
    const run = (id: string, ts: string): DeliverCard => {
      const status = parseStatus({
        v: 1,
        run: id,
        status: "running",
        tasks: [],
        finished: {},
        attentions: [],
        needsYou: false,
        links: [],
        updated: new Date().toISOString(),
        tone: "neutral",
        host: {},
        subject: { kind: "github.milestone", id: "7", title: "M007" },
        events: [
          { type: "run.started", tone: "neutral", ts },
          {
            type: "step.finished",
            tone: "info",
            task: "t1",
            step: "implement",
            ts,
            link: "https://x/pr/1",
          },
        ],
      });
      if (typeof status === "string") throw new Error(status);
      return { root: "/repo", run: id, state: { kind: "ok", status } };
    };
    const cards = signal([
      run("a", "2026-11-07T10:00:00.000Z"),
      run("b", "2026-11-07T11:00:00.000Z"),
    ]);
    const panel = render("events", pane({ cwd: "/repo" }), false, cards);
    const root = panel.fixture.nativeElement as HTMLElement;
    panel.fixture.detectChanges();
    // The default stays a single run.
    expect(root.querySelectorAll("[data-testid=run-separator]").length).toBe(0);
    const select = root.querySelector<HTMLSelectElement>("select[aria-label=Run]")!;
    expect(select.querySelectorAll("optgroup").length).toBe(1);
    expect(select.options[0]!.textContent).toContain("All runs (2)");
    select.selectedIndex = 0;
    select.dispatchEvent(new Event("change"));
    panel.fixture.detectChanges();
    expect(root.querySelectorAll("[data-testid=run-separator]").length).toBe(2);
    expect(root.querySelectorAll("a.pr").length).toBe(2);
    // Back to one run.
    select.selectedIndex = 1;
    select.dispatchEvent(new Event("change"));
    panel.fixture.detectChanges();
    expect(root.querySelectorAll("[data-testid=run-separator]").length).toBe(0);
  });
});
