import { Injector } from "@angular/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  DeliverBoard,
  checksText,
  isGithubUrl,
  type BoardRow,
  type BoardView,
} from "./deliver-board";
import { TauriBridge } from "./tauri";
import { tauri } from "./tauri.mock";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("./tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("./tauri.mock")).tauri.listen,
}));
const openUrl = vi.hoisted(() => vi.fn(async () => undefined));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const row: BoardRow = {
  number: 43,
  title: "native board",
  url: "https://github.com/GravionLabs/poimen/issues/43",
  assignee: null,
  labels: [],
  branch: null,
  pr: null,
};
const repos: BoardView = {
  status: "repos",
  repos: [
    {
      root: "/src/poimen",
      name: "poimen",
      outcome: { status: "ok", generated: null, repo: null, rows: [row] },
    },
  ],
};

describe("helpers", () => {
  it("opens only GitHub web links", () => {
    expect(isGithubUrl("https://github.com/o/r/pull/1")).toBe(true);
    expect(isGithubUrl("http://github.com/o/r")).toBe(false);
    expect(isGithubUrl("https://evil.example/https://github.com/")).toBe(false);
    expect(isGithubUrl("file:///etc/passwd")).toBe(false);
  });

  it("summarizes check counts", () => {
    const base = { passed: 3, failed: 0, pending: 1, skipped: 0 };
    expect(checksText({ state: "pending", ...base })).toBe("3 passed, 1 pending");
    expect(checksText({ state: "failing", ...base, failed: 2, pending: 0 })).toBe(
      "3 passed, 2 failed",
    );
    expect(checksText({ state: "none", passed: 0, failed: 0, pending: 0, skipped: 0 })).toBe("");
  });
});

describe("DeliverBoard", () => {
  let board: DeliverBoard;

  beforeEach(() => {
    tauri.reset();
    openUrl.mockClear();
    board = Injector.create({ providers: [TauriBridge, DeliverBoard] }).get(DeliverBoard);
  });

  it("holds the board the backend returns", async () => {
    tauri.answer("deliver_board", () => repos);
    await board.load();
    expect(board.view()).toEqual(repos);
    expect(board.error()).toBeNull();
    expect(board.updated()).not.toBeNull();
  });

  it("keeps an unsupported answer as it is", async () => {
    const view: BoardView = { status: "unsupported", reason: "no data action" };
    tauri.answer("deliver_board", () => view);
    await board.load();
    expect(board.view()).toEqual(view);
  });

  it("reports why the board could not be read and keeps the last view", async () => {
    tauri.answer("deliver_board", () => repos);
    await board.load();
    tauri.answer("deliver_board", () => {
      throw "herdr socket: refused";
    });
    await board.load();
    expect(board.error()).toBe("herdr socket: refused");
    expect(board.view()).toEqual(repos);
  });

  it("does not repeat a periodic read within the throttle, but a manual one", async () => {
    tauri.answer("deliver_board", () => repos);
    await board.load();
    await board.load(false);
    expect(tauri.calls("deliver_board")).toHaveLength(1);
    await board.load();
    expect(tauri.calls("deliver_board")).toHaveLength(2);
  });

  it("starts deliver through the deliver-starter start-link action", async () => {
    expect(await board.startDeliver("/src/poimen", row)).toBeNull();
    expect(tauri.calls("plugin_invoke")).toEqual([
      {
        pluginId: "deliver-starter",
        actionId: "start-link",
        context: { focused_pane_cwd: "/src/poimen", clicked_url: row.url },
      },
    ]);
  });

  it("returns the refusal text when deliver cannot be started", async () => {
    tauri.answer("plugin_invoke", () => {
      throw "no such plugin";
    });
    expect(await board.startDeliver("/r", row)).toBe("Start deliver #43: no such plugin");
  });

  it("opens GitHub links in the browser and ignores others", async () => {
    await board.openUrl(row.url);
    await board.openUrl("javascript:alert(1)");
    expect(openUrl).toHaveBeenCalledTimes(1);
    expect(openUrl).toHaveBeenCalledWith(row.url);
  });
});
