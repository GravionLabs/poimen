// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from "vitest";
import { DemoBackend, DemoChannel, UnansweredCommandError } from "./backend";

beforeEach(() => {
  delete window.__demoFailures;
  vi.spyOn(console, "error").mockImplementation(() => {});
});

describe("DemoBackend", () => {
  it("answers a command from a function with its arguments", async () => {
    const backend = new DemoBackend().answer({ add: (args) => Number(args?.["a"]) + 1 });
    await expect(backend.invoke("add", { a: 1 })).resolves.toBe(2);
  });

  it("gives every call of a fixed answer its own copy", async () => {
    const backend = new DemoBackend().fixed({ list: { items: [1] } });
    const first = await backend.invoke<{ items: number[] }>("list");
    first.items.push(2);
    await expect(backend.invoke("list")).resolves.toEqual({ items: [1] });
  });

  it("fails a command nobody answered, with its name, and tells the runner", async () => {
    const backend = new DemoBackend().fixed({ known: null });
    await expect(backend.invoke("new_command")).rejects.toBeInstanceOf(UnansweredCommandError);
    await expect(backend.invoke("new_command")).rejects.toThrow('"new_command"');
    expect([...backend.unanswered]).toEqual(["new_command"]);
    expect(window.__demoFailures).toEqual(["new_command", "new_command"]);
  });

  it("does not report a command that has an answer", async () => {
    const backend = new DemoBackend().fixed({ known: undefined });
    await backend.invoke("known");
    expect(window.__demoFailures).toBeUndefined();
  });

  it("emits an event to its listeners until they stop", () => {
    const backend = new DemoBackend();
    const seen: unknown[] = [];
    const stop = backend.listen("tokens", (p) => seen.push(p));
    backend.emit("tokens", 1);
    stop();
    backend.emit("tokens", 2);
    expect(seen).toEqual([1]);
  });

  it("lets an answer push messages through a channel the view gave it", async () => {
    const backend = new DemoBackend().answer({
      open: (args) => {
        (args!["channel"] as DemoChannel<string>).send("hello");
        return { id: 1 };
      },
    });
    const channel = new DemoChannel<string>();
    const got: string[] = [];
    channel.onmessage = (m) => got.push(m);
    await backend.invoke("open", { channel });
    expect(got).toEqual(["hello"]);
    expect(new DemoChannel().id).not.toBe(channel.id);
  });
});
