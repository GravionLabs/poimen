import { Injector } from "@angular/core";
import { describe, expect, it, vi } from "vitest";
import { TauriBridge } from "./tauri";
import { errorTitle, Updater, type UpdateInfo } from "./updater";

const INFO: UpdateInfo = { version: "0.9.0", currentVersion: "0.8.0", notes: "Notes", date: null };

function setup(answers: Record<string, () => unknown>) {
  const invoke = vi.fn(async (command: string) => answers[command]?.());
  const injector = Injector.create({
    providers: [{ provide: TauriBridge, useValue: { invoke } }, Updater],
  });
  return { updater: injector.get(Updater), invoke };
}

describe("Updater", () => {
  it("makes no call before the user asks", () => {
    const { updater, invoke } = setup({});
    expect(updater.state().kind).toBe("idle");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("offers a newer version with its notes", async () => {
    const { updater } = setup({ update_check: () => INFO });
    await updater.check();
    expect(updater.state()).toEqual({ kind: "available", info: INFO });
  });

  it("says when it is up to date", async () => {
    const { updater } = setup({ update_check: () => null });
    await updater.check();
    expect(updater.state().kind).toBe("current");
  });

  it("names a failed check", async () => {
    const { updater } = setup({
      update_check: () => {
        throw "offline";
      },
    });
    await updater.check();
    expect(updater.state()).toEqual({
      kind: "error",
      step: "check",
      message: "offline",
      info: null,
    });
    expect(errorTitle("check")).toBe("Update check failed");
  });

  it("keeps the update available after a failed install and retries it", async () => {
    let fail = true;
    const { updater, invoke } = setup({
      update_check: () => INFO,
      update_install: () => {
        if (fail) throw new Error("signature mismatch");
      },
    });
    await updater.check();
    await updater.install();
    expect(updater.state()).toEqual({
      kind: "error",
      step: "install",
      message: "signature mismatch",
      info: INFO,
    });
    expect(errorTitle("install")).toBe("Update failed");
    fail = false;
    await updater.install();
    expect(updater.state().kind).toBe("installing");
    expect(invoke.mock.calls.filter(([c]) => c === "update_install")).toHaveLength(2);
  });

  it("does not install without a found update", async () => {
    const { updater, invoke } = setup({});
    await updater.install();
    expect(invoke).not.toHaveBeenCalled();
  });
});
