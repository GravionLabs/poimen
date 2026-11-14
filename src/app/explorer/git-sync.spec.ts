// @vitest-environment jsdom
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { SpacesStore } from "../core/spaces-store";
import { SpaceActions } from "../core/space-actions";
import { TauriBridge } from "../core/tauri";
import { GitSync, outcomeText, progressText, summaryText, type SyncSummary } from "./git-sync";

describe("texts", () => {
  it("describes single outcomes", () => {
    expect(outcomeText("Fetch", "/s/app", { state: "updated" })).toBe("Fetch app: updated.");
    expect(outcomeText("Pull", "/s/app", { state: "current" })).toBe(
      "Pull app: already up to date.",
    );
    expect(outcomeText("Pull", "/s/app", { state: "failed", reason: "diverged" })).toBe(
      "Pull app failed: diverged",
    );
  });

  it("summarizes with the failed repositories and their first reason line", () => {
    const summary: SyncSummary = {
      results: [
        { path: "/s/a", state: "updated" },
        { path: "/s/b", state: "failed", reason: "no route\nmore" },
      ],
      updated: 1,
      current: 0,
      failed: 1,
    };
    expect(summaryText(summary)).toBe("Fetch all: 1 updated, 0 up to date, 1 failed. b: no route");
  });

  it("says so when nothing was found", () => {
    expect(summaryText({ results: [], updated: 0, current: 0, failed: 0 })).toBe(
      "Fetch all: no repositories found.",
    );
    expect(progressText(2, 5)).toBe("Fetch all: 2 of 5 repositories…");
  });
});

function setup(invoke: (cmd: string, args: unknown) => unknown) {
  const notify = vi.fn();
  const refreshRepos = vi.fn();
  const unlisten = vi.fn();
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({
    providers: [
      {
        provide: TauriBridge,
        useValue: {
          invoke: vi.fn(async (c: string, a: unknown) => invoke(c, a)),
          listen: vi.fn(async () => unlisten),
        },
      },
      { provide: SpaceActions, useValue: { notify } },
      { provide: SpacesStore, useValue: { refreshRepos } },
    ],
  });
  return { sync: TestBed.inject(GitSync), notify, refreshRepos, unlisten };
}

describe("GitSync", () => {
  beforeAll(() => {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  });

  it("reports a pull refusal as the reason and refreshes the badges", async () => {
    const { sync, notify, refreshRepos } = setup(() => {
      throw "The working tree has uncommitted changes";
    });
    await sync.pull("/s/app");
    expect(notify).toHaveBeenLastCalledWith(
      "Pull app failed: The working tree has uncommitted changes",
    );
    expect(refreshRepos).toHaveBeenCalledOnce();
  });

  it("fetches one repository", async () => {
    const { sync, notify } = setup(() => ({ state: "updated" }));
    await sync.fetch("/s/app");
    expect(notify).toHaveBeenLastCalledWith("Fetch app: updated.");
  });

  it("fetch all shows the summary, stops listening and refreshes", async () => {
    const { sync, notify, refreshRepos, unlisten } = setup(() => ({
      results: [{ path: "/s/a", state: "current" }],
      updated: 0,
      current: 1,
      failed: 0,
    }));
    await sync.fetchAll("/s");
    expect(notify).toHaveBeenLastCalledWith("Fetch all: 0 updated, 1 up to date, 0 failed.");
    expect(unlisten).toHaveBeenCalledOnce();
    expect(refreshRepos).toHaveBeenCalledOnce();
  });
});
