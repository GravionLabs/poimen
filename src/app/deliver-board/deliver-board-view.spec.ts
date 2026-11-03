// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type { BoardView } from "../core/deliver-board";
import { tauri } from "../core/tauri.mock";
import { DeliverBoardView } from "./deliver-board-view";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("../core/tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("../core/tauri.mock")).tauri.listen,
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

beforeAll(() => {
  getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
});

async function render(view: BoardView) {
  tauri.answer("deliver_board", () => view);
  TestBed.overrideComponent(DeliverBoardView, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync("src/app/deliver-board/deliver-board-view.html", "utf8"),
    },
  });
  const fixture = TestBed.createComponent(DeliverBoardView);
  await fixture.whenStable();
  fixture.detectChanges();
  return fixture;
}

describe("DeliverBoardView", () => {
  beforeEach(() => {
    tauri.reset();
    TestBed.resetTestingModule();
  });
  afterEach(() => TestBed.resetTestingModule());

  it("shows the unsupported state with a way to the terminal board", async () => {
    const fixture = await render({ status: "unsupported", reason: "no data action" });
    const el: HTMLElement = fixture.nativeElement;
    expect(el.querySelector("[data-testid=unsupported]")?.textContent).toContain("no data action");
    const requested = vi.fn();
    fixture.componentInstance.terminalRequested.subscribe(requested);
    el.querySelector<HTMLButtonElement>("[data-testid=unsupported] button")!.click();
    expect(requested).toHaveBeenCalledTimes(1);
  });

  it("shows PBIs, claim and PR state per repository", async () => {
    const fixture = await render({
      status: "repos",
      repos: [
        {
          root: "/src/poimen",
          name: "poimen",
          outcome: {
            status: "ok",
            generated: null,
            repo: "GravionLabs/poimen",
            rows: [
              {
                number: 42,
                title: "adr",
                url: "https://github.com/GravionLabs/poimen/issues/42",
                assignee: "jk",
                labels: ["in-progress"],
                branch: "docs/42",
                pr: {
                  number: 150,
                  url: "https://github.com/GravionLabs/poimen/pull/150",
                  state: "OPEN",
                  checks: { state: "pending", passed: 3, failed: 0, pending: 1, skipped: 0 },
                },
              },
              {
                number: 43,
                title: "board",
                url: "https://github.com/GravionLabs/poimen/issues/43",
                assignee: null,
                labels: [],
                branch: null,
                pr: null,
              },
            ],
          },
        },
      ],
    });
    const text = (fixture.nativeElement as HTMLElement).textContent!;
    expect(text).toContain("poimen");
    expect(text).toContain("@jk");
    expect(text).toContain("unclaimed");
    expect(text).toContain("PR #150");
    expect(text).toContain("pending");
  });

  it("starts deliver for a row", async () => {
    const fixture = await render({
      status: "repos",
      repos: [
        {
          root: "/src/poimen",
          name: "poimen",
          outcome: {
            status: "ok",
            generated: null,
            repo: null,
            rows: [
              {
                number: 43,
                title: "board",
                url: "https://github.com/GravionLabs/poimen/issues/43",
                assignee: null,
                labels: [],
                branch: null,
                pr: null,
              },
            ],
          },
        },
      ],
    });
    const start = [...(fixture.nativeElement as HTMLElement).querySelectorAll("button")].find((b) =>
      b.textContent?.includes("Start deliver"),
    )!;
    start.click();
    await fixture.whenStable();
    expect(tauri.calls("plugin_invoke")).toHaveLength(1);
    fixture.detectChanges();
    expect((fixture.nativeElement as HTMLElement).textContent).toContain(
      "Started /deliver for #43",
    );
  });

  it("shows plugin errors, outdated versions and failures per repository", async () => {
    const fixture = await render({
      status: "repos",
      repos: [
        {
          root: "/a",
          name: "a",
          outcome: { status: "plugin_error", code: "no-gh-login", message: "log in" },
        },
        { root: "/b", name: "b", outcome: { status: "outdated", version: 2 } },
        { root: "/c", name: "c", outcome: { status: "failed", message: "node was not found" } },
      ],
    });
    const text = (fixture.nativeElement as HTMLElement).textContent!;
    expect(text).toContain("log in (no-gh-login)");
    expect(text).toContain("data version 2");
    expect(text).toContain("node was not found");
  });
});
