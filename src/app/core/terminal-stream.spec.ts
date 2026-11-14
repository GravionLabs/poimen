import { Injector } from "@angular/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { StreamRecord } from "./backend";
import { TauriBridge } from "./tauri";
import { tauri } from "./tauri.mock";
import { TerminalStreamService, recordOf, type OpenTerminal } from "./terminal-stream";

vi.mock("@tauri-apps/api/core", async () => {
  const { tauri } = await import("./tauri.mock");
  return { invoke: tauri.invoke, Channel: tauri.Channel };
});

const OBSERVE: OpenTerminal = {
  mode: "observe",
  target: "w1:p1",
  cols: 80,
  rows: 24,
  takeover: false,
};

const frame = (bytes: string): StreamRecord => ({ type: "output", bytes });

type Push = (message: unknown) => void;

describe("TerminalStreamService", () => {
  let injector: ReturnType<typeof Injector.create>;
  let service: TerminalStreamService;
  let nextId: number;
  /** The channel of each opened stream, oldest first. */
  let channels: Push[];

  beforeEach(() => {
    tauri.reset();
    nextId = 7;
    channels = [];
    tauri.answer("open_terminal", (args) => {
      const channel = args?.["onRecord"] as { onmessage: Push };
      channels.push((message) => channel.onmessage(message));
      return { id: nextId++, emulated: false };
    });
    injector = Injector.create({ providers: [TauriBridge, TerminalStreamService] });
    service = injector.get(TerminalStreamService);
  });

  afterEach(() => injector.destroy());

  it("opens a stream with the given mode, terminal and size, and a channel for the records", async () => {
    const session = await service.open(
      { mode: "control", target: "w1:p2", cols: 120, rows: 40, takeover: true },
      () => {},
    );

    expect(session.id).toBe(7);
    const [args] = tauri.calls("open_terminal");
    expect(args).toMatchObject({
      mode: "control",
      target: "w1:p2",
      cols: 120,
      rows: 40,
      takeover: true,
    });
    expect(args?.["onRecord"]).toBeInstanceOf(tauri.Channel);
  });

  it("gives every stream a channel of its own, so records never reach another view", async () => {
    const first: StreamRecord[] = [];
    const second: StreamRecord[] = [];
    await service.open(OBSERVE, (r) => first.push(r));
    await service.open(OBSERVE, (r) => second.push(r));

    channels[1](frame("Yg=="));
    channels[0](frame("YQ=="));

    expect(first).toEqual([frame("YQ==")]);
    expect(second).toEqual([frame("Yg==")]);
  });

  it("turns raw bytes into a bytes record", async () => {
    const got: StreamRecord[] = [];
    await service.open(OBSERVE, (r) => got.push(r));

    channels[0](new Uint8Array([104, 105]).buffer);

    expect(got).toEqual([{ type: "bytes", data: new Uint8Array([104, 105]) }]);
  });

  it("accepts a typed array view of the bytes and keeps only its own range", () => {
    const view = new Uint8Array([0, 104, 105, 0]).subarray(1, 3);
    expect(recordOf(view)).toEqual({ type: "bytes", data: new Uint8Array([104, 105]) });
  });

  it("passes JSON records through unchanged", () => {
    const record: StreamRecord = { type: "resized", cols: 90, rows: 30 };
    expect(recordOf(record)).toBe(record);
  });

  it("closes the backend stream on close, once", async () => {
    const session = await service.open(OBSERVE, () => {});

    session.close();
    session.close();

    expect(session.open).toBe(false);
    expect(tauri.calls("close_terminal")).toEqual([{ id: 7 }]);
  });

  it("marks the session ended when the closed record arrives, without closing it again", async () => {
    const received: StreamRecord[] = [];
    const session = await service.open(OBSERVE, (r) => received.push(r));

    channels[0]({ type: "closed", reason: "controller taken" });
    session.close();

    expect(received).toEqual([{ type: "closed", reason: "controller taken" }]);
    expect(session.open).toBe(false);
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

  it("sends a mouse event for the session id", async () => {
    const session = await service.open(OBSERVE, () => {});
    const event = { action: "down", button: "left", column: 12, row: 5, modifiers: 0 } as const;

    session.mouse(event);

    expect(tauri.calls("terminal_mouse")).toEqual([{ id: 7, ...event }]);
  });

  it("sends nothing on a closed session", async () => {
    const session = await service.open(OBSERVE, () => {});
    session.close();
    session.mouse({ action: "up", button: "left", column: 0, row: 0, modifiers: 0 });

    session.input("x");
    session.resize(100, 30);
    session.scroll(false, 1);

    expect(tauri.calls("terminal_input")).toEqual([]);
    expect(tauri.calls("terminal_resize")).toEqual([]);
    expect(tauri.calls("terminal_scroll")).toEqual([]);
    expect(tauri.calls("terminal_mouse")).toEqual([]);
  });

  it("tells whether the opened stream is emulated", async () => {
    tauri.answer("open_terminal", () => ({ id: 9, emulated: true }));
    const session = await service.open(OBSERVE, () => {});
    expect(session.id).toBe(9);
    expect(session.emulated).toBe(true);
  });

  it("delivers records that arrive before open resolves, with no buffering of its own", async () => {
    tauri.answer("open_terminal", (args) => {
      const channel = args?.["onRecord"] as { onmessage: Push };
      channel.onmessage({ type: "reset", cols: 80, rows: 24, bytes: "" });
      channel.onmessage(new Uint8Array([1]).buffer);
      channel.onmessage({ type: "closed", reason: "pane gone" });
      return { id: 7, emulated: true };
    });
    const got: StreamRecord[] = [];

    const session = await service.open(OBSERVE, (r) => got.push(r));

    expect(got.map((r) => r.type)).toEqual(["reset", "bytes", "closed"]);
    expect(session.open).toBe(false);
  });

  it("rejects with the backend error", async () => {
    tauri.answer("open_terminal", () => {
      throw new Error("terminal has a controller");
    });

    await expect(service.open(OBSERVE, () => {})).rejects.toThrow("terminal has a controller");
  });

  it("listens to no global event", async () => {
    await service.open(OBSERVE, () => {});
    expect(tauri.listen).not.toHaveBeenCalled();
  });
});
