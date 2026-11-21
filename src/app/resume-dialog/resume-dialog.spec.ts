// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { SessionResume } from "../core/session-resume";
import { SpaceActions } from "../core/space-actions";
import { SpacesStore } from "../core/spaces-store";
import { TauriBridge } from "../core/tauri";
import { expectFocusInside, focusOpener, press } from "../ui/dialog-shell/dialog-contract";
import { ResumeDialog } from "./resume-dialog";

async function setup(sessions: unknown[]) {
  const pane = { terminal_id: "t1", space_id: "sp", cwd: "/w/p" };
  const bridge = {
    invoke: vi.fn(async (cmd: string): Promise<unknown> => {
      if (cmd === "get_harnesses") {
        return [
          {
            id: "claude",
            command: ["claude"],
            resumeArgs: ["--resume", "{session}"],
            available: true,
          },
        ];
      }
      return cmd === "list_agent_sessions" ? sessions : undefined;
    }),
  };
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({
    providers: [
      { provide: TauriBridge, useValue: bridge },
      {
        provide: SpacesStore,
        useValue: {
          pane: () => pane,
          snapshot: () => ({ spaces: [], tabs: [], terminals: [pane] }),
          refresh: vi.fn(async () => undefined),
        },
      },
      { provide: SpaceActions, useValue: { notify: vi.fn(), fail: vi.fn() } },
    ],
  });
  TestBed.overrideComponent(ResumeDialog, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync("src/app/resume-dialog/resume-dialog.html", "utf8"),
    },
  });
  const resume = TestBed.inject(SessionResume);
  const fixture = TestBed.createComponent(ResumeDialog);
  await resume.open({ kind: "pane", paneId: "t1" });
  fixture.detectChanges();
  await fixture.whenStable();
  fixture.detectChanges();
  return { resume, fixture, bridge, root: fixture.nativeElement as HTMLElement };
}

const one = {
  agent: "claude",
  session: "s1",
  started: "2026-11-01T10:00:00Z",
  lastActivity: "2026-11-01T11:00:00Z",
  firstPrompt: "fix the bug",
};

describe("ResumeDialog (#986)", () => {
  beforeAll(() => {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting(), {
      teardown: { destroyAfterEach: true },
    });
  });

  it("lists the first prompt and the agent, moves the focus in and Escape returns it", async () => {
    const opener = focusOpener();
    const { root, resume, fixture } = await setup([one]);
    expectFocusInside(root);
    expect(root.textContent).toContain("fix the bug");
    expect(root.textContent).toContain("Claude Code");
    press("Escape");
    fixture.detectChanges();
    expect(resume.dialog()).toBeNull();
    expect(document.activeElement).toBe(opener);
  });

  it("starts the picked session", async () => {
    const { root, bridge } = await setup([one]);
    root.querySelector<HTMLButtonElement>("button.session")!.click();
    await vi.waitFor(() =>
      expect(bridge.invoke).toHaveBeenCalledWith("agent_start", {
        terminalId: "t1",
        kind: "claude",
        name: null,
        args: ["--resume", "s1"],
      }),
    );
  });

  it("says when there is no earlier session", async () => {
    const { root } = await setup([]);
    expect(root.textContent).toContain("No earlier session");
    expect(root.querySelector("button.session")).toBeNull();
  });
});
