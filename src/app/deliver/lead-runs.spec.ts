// @vitest-environment jsdom
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { AgentGrants, type ActiveGrant } from "../core/agent-grants";
import type { Terminal } from "../core/snapshot";
import { SpacesStore } from "../core/spaces-store";
import { tauri } from "../core/tauri.mock";
import { Deliver } from "./deliver";
import { LeadRuns } from "./lead-runs";
import { parseLead } from "./lead-run";
import type { DeliverRead } from "./deliver-status";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("../core/tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("../core/tauri.mock")).tauri.listen,
}));

beforeAll(() => {
  try {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  } catch {
    // already initialized by the shared setup
  }
});
beforeEach(() => tauri.reset());

const NOW = Date.parse("2026-11-24T10:10:00Z");
const subject = (id: string, title: string) => ({
  kind: "github.issue",
  id,
  title,
});
const none = { succeeded: [], failed: [], cancelled: [], skipped: [] };

/** A run as the backend sends it, with the defaults of the schema. */
const run = (over: Record<string, unknown>) => ({
  v: 1,
  status: "running",
  tasks: [],
  finished: none,
  attentions: [],
  needsYou: false,
  links: [],
  gaps: [],
  lastSeq: 5,
  updated: "2026-11-24T10:08:00.000Z",
  tone: "neutral",
  host: {},
  events: [],
  ...over,
});

/** The lead run: A#1 runs, B#2 waits for it and is blocked, C#3 is done. */
const LEAD = run({
  run: "lead-1",
  subject: subject("lead", "Ship the feature in three repositories"),
  host: { "poimen.terminal": "t1" },
  tasks: [
    {
      task: "a#1",
      subject: subject("a#1", "A"),
      step: null,
      progress: null,
      links: [],
    },
    {
      task: "b#2",
      subject: subject("b#2", "B"),
      step: null,
      progress: null,
      links: [],
    },
  ],
  finished: {
    ...none,
    succeeded: [{ task: "c#3", subject: subject("c#3", "C"), links: [] }],
  },
  attentions: [
    {
      id: "x",
      task: "b#2",
      reason: "question",
      text: "Which API?",
      needsYou: true,
    },
  ],
  lead: {
    members: [
      { task: "a#1", repository: "/r/a", terminal: "t2", deliverRun: "d-a" },
      { task: "b#2", repository: "/r/b", terminal: "t3", deliverRun: "d-b" },
      { task: "c#3", repository: "/r/c" },
    ],
    dependencies: [
      { task: "b#2", blockedBy: "a#1", note: "needs the new endpoint" },
      { task: "b#2", blockedBy: "c#3" },
    ],
    grant: { id: "g1", spaces: ["s1"], tools: ["start_agent"], maxAgents: 3 },
    resumedIn: "t9",
  },
});
const DELIVER_A = run({
  run: "d-a",
  subject: subject("a", "Milestone M1"),
  tasks: [
    {
      task: "pbi",
      subject: subject("a#7", "Add endpoint"),
      step: null,
      progress: null,
      links: [],
    },
  ],
});

const events = (...runs: unknown[]): DeliverRead => ({ kind: "events", runs });

const terminal = (id: string, extra: Partial<Terminal> = {}): Terminal =>
  ({
    terminal_id: id,
    tab_id: "tab",
    space_id: "s1",
    agent_status: "working",
    ...extra,
  }) as Terminal;

const grant = (): ActiveGrant => ({
  id: "g1",
  leadName: "Lead",
  spaces: [{ id: "s1", name: "poimen" }],
  tools: ["start_agent"],
  maxAgents: 3,
  running: 2,
  expiresAt: NOW + 3600_000,
});

function render(terminals: Terminal[], grants: ActiveGrant[] = [grant()]) {
  TestBed.resetTestingModule();
  const revoke = vi.fn();
  TestBed.configureTestingModule({
    providers: [
      { provide: AgentGrants, useValue: { active: signal(grants), revoke } },
      {
        provide: SpacesStore,
        useValue: { all: () => ({ terminals, spaces: [] }) },
      },
    ],
  });
  const deliver = TestBed.inject(Deliver);
  deliver.apply("/r/lead", events(LEAD));
  deliver.apply("/r/a", events(DELIVER_A));
  deliver.now.set(NOW);
  const fixture = TestBed.createComponent(LeadRuns);
  const chosen: string[] = [];
  fixture.componentInstance.paneChosen.subscribe((id) => chosen.push(id));
  const el = fixture.nativeElement as HTMLElement;
  const show = async () => {
    await fixture.whenStable();
    fixture.detectChanges();
  };
  return { el, show, chosen, revoke, deliver };
}

const text = (el: Element | null | undefined) => el?.textContent?.replace(/\s+/g, " ").trim() ?? "";
const member = (el: HTMLElement, task: string) =>
  el.querySelector<HTMLElement>(`[data-testid=lead-member][data-task="${task}"]`)!;

describe("lead runs", () => {
  it("parses the record of the backend and ignores a run without one", () => {
    expect(parseLead(LEAD)?.members.map((m) => m.task)).toEqual(["a#1", "b#2", "c#3"]);
    expect(parseLead(LEAD)?.grantId).toBe("g1");
    expect(parseLead(DELIVER_A)).toBeNull();
    expect(parseLead({ lead: { members: [{ task: "x" }, 3] } })?.members).toEqual([]);
  });

  it("shows nothing without a lead run", async () => {
    TestBed.resetTestingModule();
    const { el, show, deliver } = render([]);
    deliver.apply("/r/lead", { kind: "missing" });
    await show();
    expect(el.querySelectorAll("[data-testid=lead-run]")).toHaveLength(0);
  });

  it("lists per repository its deliver run, state, current PBI and blocked session", async () => {
    const { el, show } = render([terminal("t1")]);
    await show();
    expect(text(el.querySelector("[data-testid=lead-title]"))).toBe(
      "Ship the feature in three repositories",
    );
    const rows = el.querySelectorAll("[data-testid=lead-member]");
    expect(rows).toHaveLength(3);
    const a = member(el, "a#1");
    expect(text(a.querySelector("[data-testid=member-state]"))).toBe("running");
    expect(text(a.querySelector("[data-testid=member-run]"))).toBe("running, updated 2 min ago");
    expect(text(a.querySelector("[data-testid=member-current]"))).toBe("Add endpoint");
    const b = member(el, "b#2");
    expect(text(b.querySelector("[data-testid=member-state]"))).toBe("blocked");
    expect(text(b.querySelector("[data-testid=member-blocked]"))).toBe("Blocked: Which API?");
    expect(text(b.querySelector("[data-testid=member-waiting]"))).toBe("Waits for a#1");
    expect(text(member(el, "c#3").querySelector("[data-testid=member-state]"))).toBe("succeeded");
  });

  it("shows the dependencies and which of them are open", async () => {
    const { el, show } = render([]);
    await show();
    const deps = [...el.querySelectorAll("[data-testid=lead-dependency]")];
    expect(deps.map((d) => [d.getAttribute("data-open"), text(d.querySelector("hx-tag"))])).toEqual(
      [
        ["true", "open"],
        ["false", "done"],
      ],
    );
    expect(text(deps[0])).toContain("needs the new endpoint");
  });

  it("jumps to the terminal of the session (#745) and to the lead's terminal", async () => {
    const session = terminal("t20", { run: "d-a", task: "pbi" });
    const { el, show, chosen } = render([terminal("t1"), terminal("t9"), terminal("t2"), session]);
    await show();
    const a = member(el, "a#1");
    // The newest takeover wins over the terminal of the run's host.
    a.querySelector<HTMLButtonElement>("[data-testid=member-terminal]")!.click();
    a.querySelector<HTMLButtonElement>("[data-testid=member-lead]")!.click();
    el.querySelector<HTMLButtonElement>("[data-testid=lead-terminal]")!.click();
    expect(chosen).toEqual(["t20", "t9", "t9"]);
  });

  it("falls back to the member's own terminal and hides what is closed", async () => {
    const { el, show } = render([terminal("t3")]);
    await show();
    expect(member(el, "b#2").querySelector("[data-testid=member-terminal]")).not.toBeNull();
    expect(member(el, "a#1").querySelector("[data-testid=member-terminal]")).toBeNull();
    expect(el.querySelector("[data-testid=lead-terminal]")).toBeNull();
  });

  it("shows the active grant with its revoke action, as an hx-button", async () => {
    const { el, show, revoke } = render([]);
    await show();
    expect(text(el.querySelector("[data-testid=lead-grant]"))).toContain(
      "may write in poimen, 2 of 3 agents running",
    );
    const button = el.querySelector<HTMLButtonElement>("[data-testid=lead-revoke]")!;
    expect(button.hasAttribute("hx-button")).toBe(true);
    button.click();
    expect(revoke).toHaveBeenCalledWith("g1");
  });

  it("shows no grant once it ended, and states as hx-tag", async () => {
    const { el, show } = render([], []);
    await show();
    expect(el.querySelector("[data-testid=lead-grant]")).toBeNull();
    expect(el.querySelectorAll("[data-testid=member-state]")[0]!.tagName).toBe("HX-TAG");
  });
});
