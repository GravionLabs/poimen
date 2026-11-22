// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from "vitest";
import { UnansweredCommandError } from "./backend";
import { createFakeBackend } from "./fake";

beforeEach(() => {
  delete window.__demoFailures;
  delete window.__demoCalls;
  vi.spyOn(console, "error").mockImplementation(() => {});
});

describe("createFakeBackend", () => {
  it("answers only what the test set up and fails any other command by name", async () => {
    const backend = createFakeBackend({ snapshot: { tabs: [] } }, { echo: (args) => args });
    await expect(backend.invoke("snapshot")).resolves.toEqual({ tabs: [] });
    await expect(backend.invoke("echo", { a: 1 })).resolves.toEqual({ a: 1 });
    await expect(backend.invoke("close_tab", { id: "t" })).rejects.toBeInstanceOf(
      UnansweredCommandError,
    );
    expect(window.__demoFailures).toEqual(["close_tab"]);
  });

  it("logs the calls and takes a replaced answer and a pushed event", async () => {
    const backend = createFakeBackend({ snapshot: 1 });
    const seen: unknown[] = [];
    backend.listen("snapshot-changed", (p) => seen.push(p));
    backend.set("snapshot", 2);
    backend.emit("snapshot-changed", "x");
    await expect(backend.invoke("snapshot")).resolves.toBe(2);
    expect(seen).toEqual(["x"]);
    expect(backend.calls.map((c) => c.command)).toEqual(["snapshot"]);
  });
});
