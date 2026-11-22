import { describe, expect, it } from "vitest";
import { recordAccelerator, shortcutConflict } from "./shortcut-record";
import { DEFAULT_SHORTCUTS } from "./shortcuts";

function ev(over: Partial<Parameters<typeof recordAccelerator>[0]>) {
  return {
    key: "n",
    code: "KeyN",
    metaKey: false,
    ctrlKey: false,
    altKey: false,
    shiftKey: false,
    ...over,
  };
}

describe("recordAccelerator", () => {
  it("spells the primary modifier as CmdOrCtrl", () => {
    expect(recordAccelerator(ev({ ctrlKey: true, shiftKey: true }), false)).toEqual({
      accelerator: "CmdOrCtrl+Shift+N",
    });
    expect(recordAccelerator(ev({ metaKey: true }), true)).toEqual({ accelerator: "CmdOrCtrl+N" });
  });

  it("keeps the other of Cmd and Ctrl literal", () => {
    expect(recordAccelerator(ev({ ctrlKey: true }), true)).toEqual({ accelerator: "Ctrl+N" });
    expect(recordAccelerator(ev({ metaKey: true }), false)).toEqual({ accelerator: "Cmd+N" });
  });

  it("uses the key code so Alt or Shift do not change the letter", () => {
    expect(recordAccelerator(ev({ key: "ñ", altKey: true }), false)).toEqual({
      accelerator: "Alt+N",
    });
    expect(
      recordAccelerator(ev({ key: "|", code: "Backslash", ctrlKey: true, shiftKey: true }), false),
    ).toEqual({ accelerator: "CmdOrCtrl+Shift+\\" });
  });

  it("names arrows and function keys", () => {
    expect(
      recordAccelerator(ev({ key: "ArrowLeft", code: "ArrowLeft", altKey: true }), false),
    ).toEqual({ accelerator: "Alt+Left" });
    expect(recordAccelerator(ev({ key: "F5", code: "F5" }), false)).toEqual({ accelerator: "F5" });
  });

  it("waits while only modifiers are held", () => {
    expect(
      recordAccelerator(ev({ key: "Control", code: "ControlLeft", ctrlKey: true }), false),
    ).toBe(null);
  });

  it("needs Cmd, Ctrl or Alt like the settings loader", () => {
    expect(recordAccelerator(ev({}), false)).toHaveProperty("error");
    expect(recordAccelerator(ev({ shiftKey: true }), false)).toHaveProperty("error");
  });
});

describe("shortcutConflict", () => {
  it("names the other action with the same accelerator", () => {
    expect(shortcutConflict(DEFAULT_SHORTCUTS, "newTerminal", "CmdOrCtrl+W")).toBe("closeTab");
    expect(shortcutConflict(DEFAULT_SHORTCUTS, "closeTab", "CmdOrCtrl+W")).toBeNull();
    expect(shortcutConflict(DEFAULT_SHORTCUTS, "newTerminal", "")).toBeNull();
  });
});
