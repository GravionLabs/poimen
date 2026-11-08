// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { EditorTabs } from "../core/editor-tabs";
import type { Terminal } from "../core/herdr";
import { HerdrStore } from "../core/herdr-store";
import { Deliver } from "../deliver/deliver";
import { TaskLog } from "../core/task-log";
import { UiState } from "../core/ui-state";
import { Subagents } from "../core/transcripts";
import { BottomPanel } from "./bottom-panel";
import { FlowView } from "./flow-view";

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
  ) {
    const ui = { panelTab: signal(tab), panelHeight: signal(200), togglePanel: vi.fn() };
    const subagents = {
      supported: signal(supported),
      agents: signal([]),
      items: signal([]),
      paneId: signal(null),
      selected: signal(null),
      selectedId: signal(null),
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
        { provide: Deliver, useValue: { cards: signal([]), openUrl: vi.fn() } },
        { provide: TaskLog, useValue: { lines: signal([]) } },
        { provide: HerdrStore, useValue: { panes: signal(panes) } },
      ],
    });
    const fixture = TestBed.createComponent(BottomPanel);
    fixture.detectChanges();
    return {
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
});
