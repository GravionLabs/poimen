import { Injector } from "@angular/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { StreamRecord } from "./herdr";
import { TauriBridge } from "./tauri";
import { tauri } from "./tauri.mock";
import { TerminalStreamService, type OpenTerminal } from "./terminal-stream";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("./tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("./tauri.mock")).tauri.listen,
}));

const OBSERVE: OpenTerminal = {
  mode: "observe",
  target: "w1:p1",
  cols: 80,
  rows: 24,
  takeover: false,
};

const frame = (bytes: string): StreamRecord => ({
  type: "frame",
  seq: 1,
  full: true,
  width: 80,
  height: 24,
  bytes,
});

describe("TerminalStreamService", () => {
  let injector: ReturnType<typeof Injector.create>;
  let service: TerminalStreamService;
  let nextId: number;

  const create = () => {
    injector = Injector.create({ providers: [TauriBridge, TerminalStreamService] });
    service = injector.get(TerminalStreamService);
  };

  beforeEach(() => {
    tauri.reset();
    nextId = 7;
    tauri.answer("open_terminal", () => nextId++);
    create();
  });

  afterEach(() => injector.destroy());

  it("opens a stream with the given mode, pane and size", async () => {
    const session = await service.open(
      { mode: "control", target: "w1:p2", cols: 120, rows: 40, takeover: true },
      () => {},
    );

    expect(session.id).toBe(7);
    expect(tauri.calls("open_terminal")).toEqual([
      { mode: "control", target: "w1:p2", cols: 120, rows: 40, takeover: true },
    ]);
  });

  it("routes records to the view that opened the stream", async () => {
    const first: StreamRecord[] = [];
    const second: StreamRecord[] = [];
    await service.open(OBSERVE, (r) => first.push(r));
    await service.open(OBSERVE, (r) => second.push(r));

    tauri.emit("terminal", { id: 8, record: frame("Yg==") });
    tauri.emit("terminal", { id: 7, record: frame("YQ==") });

    expect(first).toEqual([frame("YQ==")]);
    expect(second).toEqual([frame("Yg==")]);
  });

  it("ignores records of unknown session ids", async () => {
    const received: StreamRecord[] = [];
    await service.open(OBSERVE, (r) => received.push(r));

    expect(() => tauri.emit("terminal", { id: 99, record: frame("YQ==") })).not.toThrow();
    expect(received).toEqual([]);
  });

  it("stops routing and closes the backend stream on close", async () => {
    const received: StreamRecord[] = [];
    const session = await service.open(OBSERVE, (r) => received.push(r));

    session.close();
    tauri.emit("terminal", { id: 7, record: frame("YQ==") });

    expect(received).toEqual([]);
    expect(service.size).toBe(0);
    expect(session.open).toBe(false);
    expect(tauri.calls("close_terminal")).toEqual([{ id: 7 }]);
  });

  it("closes the backend stream only once", async () => {
    const session = await service.open(OBSERVE, () => {});

    session.close();
    session.close();

    expect(tauri.calls("close_terminal")).toEqual([{ id: 7 }]);
  });

  it("delivers the closed record, then forgets the stream without closing it again", async () => {
    const received: StreamRecord[] = [];
    const session = await service.open(OBSERVE, (r) => received.push(r));

    tauri.emit("terminal", { id: 7, record: { type: "closed", reason: "controller taken" } });
    tauri.emit("terminal", { id: 7, record: frame("YQ==") });
    session.close();

    expect(received).toEqual([{ type: "closed", reason: "controller taken" }]);
    expect(session.open).toBe(false);
    expect(service.size).toBe(0);
    expect(tauri.calls("close_terminal")).toEqual([]);
  });

  it("sends input, resize and scroll for the session id", async () => {
    const session = await service.open(OBSERVE, () => {});

    session.input("ls\r");
    session.resize(100, 30);
    session.scroll(true, 3);

    expect(tauri.calls("terminal_input")).toEqual([{ id: 7, text: "ls\r" }]);
    expect(tauri.calls("terminal_resize")).toEqual([{ id: 7, cols: 100, rows: 30 }]);
    expect(tauri.calls("terminal_scroll")).toEqual([{ id: 7, up: true, lines: 3 }]);
  });

  it("sends nothing on a closed session", async () => {
    const session = await service.open(OBSERVE, () => {});
    session.close();

    session.input("x");
    session.resize(100, 30);
    session.scroll(false, 1);

    expect(tauri.calls("terminal_input")).toEqual([]);
    expect(tauri.calls("terminal_resize")).toEqual([]);
    expect(tauri.calls("terminal_scroll")).toEqual([]);
  });

  it("delivers a record that arrives before open resolves", async () => {
    tauri.answer("open_terminal", () => {
      tauri.emit("terminal", { id: 7, record: { type: "closed", reason: "pane gone" } });
      return 7;
    });
    const got: StreamRecord[] = [];

    const session = await service.open(OBSERVE, (r) => got.push(r));

    expect(got).toEqual([{ type: "closed", reason: "pane gone" }]);
    expect(session.open).toBe(false);
  });

  it("drops held records when an open rejects, so a later open does not get them", async () => {
    tauri.answer("open_terminal", () => {
      tauri.emit("terminal", { id: 7, record: frame("c3RhbGU=") });
      throw new Error("terminal has a controller");
    });
    await expect(service.open(OBSERVE, () => {})).rejects.toThrow("controller");
    tauri.answer("open_terminal", () => 7);
    const got: StreamRecord[] = [];

    await service.open(OBSERVE, (r) => got.push(r));

    expect(got).toEqual([]);
  });

  it("rejects with the backend error and routes nothing", async () => {
    tauri.answer("open_terminal", () => {
      throw new Error("terminal has a controller");
    });

    await expect(service.open(OBSERVE, () => {})).rejects.toThrow("terminal has a controller");
    expect(service.size).toBe(0);
  });

  it("stops listening when its injector is destroyed", async () => {
    await service.open(OBSERVE, () => {});

    injector.destroy();
    await Promise.resolve();

    expect(tauri.unlisten).toHaveBeenCalledWith("terminal");
    expect(service.size).toBe(0);
    create(); // afterEach destroys the injector again
  });
});
