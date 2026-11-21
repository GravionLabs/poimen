// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { EditorTabs } from "../core/editor-tabs";
import { TauriBridge } from "../core/tauri";
import { UiState } from "../core/ui-state";
import { expectFocusInside, focusOpener, press } from "../ui/dialog-shell/dialog-contract";
import { GithubPulls } from "../github/pulls";
import { Changes } from "./changes";
import { CreatePullRequest, type PullState } from "./pull-request";
import { PullRequestDialog } from "./pull-request-dialog";

const READY: PullState = {
  branch: "feat/x",
  base: "release/next",
  pushed: true,
  existing: null,
};

function setup(state: PullState | Error = READY, harness: string | null = "claude") {
  const calls: Record<string, unknown>[] = [];
  const handlers: Record<string, (args: Record<string, unknown>) => unknown> = {
    changes_pr_state: () => {
      if (state instanceof Error) throw state;
      return state;
    },
    changes_draft_pull_request: () => ({ title: "feat(x): add y", body: "Why." }),
    changes_create_pull_request: () => ({ number: 9, url: "https://github.com/o/n/pull/9" }),
  };
  const bridge = {
    invoke: vi.fn(async (cmd: string, args: Record<string, unknown> = {}) => {
      calls.push({ cmd, ...args });
      return handlers[cmd]?.(args);
    }),
  };
  const pulls = {
    load: vi.fn(async () => undefined),
    rows: signal([{ url: "https://github.com/o/n/pull/9", number: 9 }]),
    openDetail: vi.fn(async () => undefined),
  };
  const tabs = { openGithub: vi.fn() };
  const ui = { selectGithubPage: vi.fn() };
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({
    providers: [
      CreatePullRequest,
      { provide: TauriBridge, useValue: bridge },
      {
        provide: Changes,
        useValue: { dir: signal("/w/p"), messageHarness: signal(harness) },
      },
      { provide: EditorTabs, useValue: tabs },
      { provide: UiState, useValue: ui },
      { provide: GithubPulls, useValue: pulls },
    ],
  });
  TestBed.overrideComponent(PullRequestDialog, {
    set: {
      templateUrl: undefined,
      template: readFileSync("src/app/changes/pull-request-dialog.html", "utf8"),
    },
  });
  const pr = TestBed.inject(CreatePullRequest);
  return { pr, bridge, calls, pulls, tabs, ui, handlers };
}

describe("CreatePullRequest (#996)", () => {
  beforeAll(() => {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting(), {
      teardown: { destroyAfterEach: true },
    });
  });

  it("offers the dialog only for a pushed branch without an open pull request", async () => {
    const cases: [PullState, boolean][] = [
      [READY, true],
      [{ ...READY, pushed: false }, false],
      [{ ...READY, existing: { number: 1, url: "u" } }, false],
      [{ ...READY, branch: null }, false],
      [{ ...READY, branch: "release/next" }, false],
    ];
    for (const [state, expected] of cases) {
      const { pr } = setup(state);
      await pr.load();
      expect(pr.canCreate(), JSON.stringify(state)).toBe(expected);
    }
  });

  it("hides the button when the state cannot be read", async () => {
    const { pr } = setup(new Error("no gh"));
    await pr.load();
    expect(pr.canCreate()).toBe(false);
  });

  it("drafts title and body with the harness for the chosen base", async () => {
    const { pr, calls } = setup();
    await pr.load();
    pr.open();
    pr.patch({ base: "main" });
    await pr.draftWithAgent();
    expect(calls.find((c) => c["cmd"] === "changes_draft_pull_request")).toMatchObject({
      dir: "/w/p",
      harness: "claude",
      base: "main",
    });
    expect(pr.dialog()).toMatchObject({ title: "feat(x): add y", body: "Why.", drafting: false });
  });

  it("keeps the fields when the draft fails", async () => {
    const { pr, handlers } = setup();
    handlers["changes_draft_pull_request"] = () => {
      throw "claude failed: no";
    };
    await pr.load();
    pr.open();
    pr.patch({ title: "mine", body: "my body" });
    await pr.draftWithAgent();
    expect(pr.dialog()).toMatchObject({
      title: "mine",
      body: "my body",
      error: "claude failed: no",
      drafting: false,
    });
  });

  it("creates the pull request, then opens its detail", async () => {
    const { pr, calls, pulls, tabs, ui } = setup();
    await pr.load();
    pr.open();
    pr.patch({ title: " feat: y ", body: "text", draft: true });
    await pr.create();
    expect(calls.find((c) => c["cmd"] === "changes_create_pull_request")).toMatchObject({
      dir: "/w/p",
      base: "release/next",
      title: "feat: y",
      body: "text",
      draft: true,
    });
    expect(pr.dialog()).toBeNull();
    expect(ui.selectGithubPage).toHaveBeenCalledWith("pulls");
    expect(tabs.openGithub).toHaveBeenCalled();
    expect(pulls.openDetail).toHaveBeenCalledWith(expect.objectContaining({ number: 9 }));
  });

  it("shows a gh error in the dialog and loses nothing", async () => {
    const { pr, handlers, tabs } = setup();
    handlers["changes_create_pull_request"] = () => {
      throw "a pull request for branch feat/x already exists";
    };
    await pr.load();
    pr.open();
    pr.patch({ title: "t", body: "b", draft: true });
    await pr.create();
    expect(pr.dialog()).toMatchObject({
      title: "t",
      body: "b",
      draft: true,
      creating: false,
      error: "a pull request for branch feat/x already exists",
    });
    expect(tabs.openGithub).not.toHaveBeenCalled();
  });

  it("does not create without a title", async () => {
    const { pr, calls } = setup();
    await pr.load();
    pr.open();
    await pr.create();
    expect(calls.some((c) => c["cmd"] === "changes_create_pull_request")).toBe(false);
  });

  it("cancelling a draft stops the call", async () => {
    const { pr, calls, handlers } = setup();
    let release: (v: unknown) => void = () => undefined;
    handlers["changes_draft_pull_request"] = () => new Promise((r) => (release = r));
    await pr.load();
    pr.open();
    const running = pr.draftWithAgent();
    await pr.cancelDraft();
    release({ title: "late", body: "" });
    await running;
    expect(calls.some((c) => c["cmd"] === "changes_generate_cancel")).toBe(true);
    expect(pr.dialog()).toMatchObject({ title: "", drafting: false });
  });
});

describe("PullRequestDialog (#996)", () => {
  it("moves the focus in, has the Draft checkbox and Escape returns the focus", async () => {
    const opener = focusOpener();
    const { pr } = setup();
    await pr.load();
    pr.open();
    const fixture = TestBed.createComponent(PullRequestDialog);
    fixture.detectChanges();
    const root = fixture.nativeElement as HTMLElement;
    expectFocusInside(root);
    expect(root.querySelector<HTMLInputElement>("input.draft")?.type).toBe("checkbox");
    expect(root.textContent).toContain("Draft with agent");
    press("Escape");
    fixture.detectChanges();
    expect(pr.dialog()).toBeNull();
    expect(document.activeElement).toBe(opener);
  });

  it("hides Draft with agent without a harness", async () => {
    const { pr } = setup(READY, null);
    await pr.load();
    pr.open();
    const fixture = TestBed.createComponent(PullRequestDialog);
    fixture.detectChanges();
    expect((fixture.nativeElement as HTMLElement).textContent).not.toContain("Draft with agent");
  });
});
