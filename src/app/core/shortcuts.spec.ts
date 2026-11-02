import { describe, expect, it } from "vitest";
import {
  DEFAULT_SHORTCUTS,
  formatShortcut,
  matchShortcut,
  parseChord,
  parseShortcuts,
} from "./shortcuts";

const key = (k: string, extra: Partial<KeyboardEvent> = {}) => ({
  key: k,
  code: "",
  metaKey: false,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  ...extra,
});
const mac = { mac: true, inTerminal: false };
const other = { mac: false, inTerminal: false };

describe("parseShortcuts", () => {
  it("keeps the defaults for anything but strings", () => {
    expect(parseShortcuts(null)).toEqual(DEFAULT_SHORTCUTS);
    expect(parseShortcuts({ closeTab: 1, refresh: "Ctrl+F5" }).closeTab).toBe("CmdOrCtrl+W");
    expect(parseShortcuts({ refresh: "Ctrl+F5" }).refresh).toBe("Ctrl+F5");
    expect(parseShortcuts({ refresh: "" }).refresh).toBe("");
  });
});

describe("parseChord", () => {
  it("resolves CmdOrCtrl per platform", () => {
    expect(parseChord("CmdOrCtrl+Shift+W", true)).toMatchObject({ meta: true, ctrl: false });
    expect(parseChord("CmdOrCtrl+Shift+W", false)).toMatchObject({ meta: false, ctrl: true });
  });

  it("is null for an unbound or unreadable shortcut", () => {
    expect(parseChord("", false)).toBeNull();
    expect(parseChord("Hyper+W", false)).toBeNull();
  });
});

describe("matchShortcut", () => {
  it("matches the defaults with Ctrl outside macOS", () => {
    expect(matchShortcut(key("b", { ctrlKey: true }), DEFAULT_SHORTCUTS, other)).toBe(
      "toggleSidebar",
    );
    expect(
      matchShortcut(
        key("|", { ctrlKey: true, shiftKey: true, code: "Backslash" }),
        DEFAULT_SHORTCUTS,
        other,
      ),
    ).toBe("splitDown");
    expect(
      matchShortcut(key("ArrowRight", { ctrlKey: true, altKey: true }), DEFAULT_SHORTCUTS, other),
    ).toBe("nextTab");
  });

  it("leaves Cmd shortcuts to the native menu on macOS", () => {
    expect(matchShortcut(key("b", { metaKey: true }), DEFAULT_SHORTCUTS, mac)).toBeNull();
  });

  it("uses an override and no longer the default of that action", () => {
    const shortcuts = { ...DEFAULT_SHORTCUTS, toggleSidebar: "Ctrl+Alt+B" };
    expect(matchShortcut(key("b", { ctrlKey: true, altKey: true }), shortcuts, other)).toBe(
      "toggleSidebar",
    );
    expect(matchShortcut(key("b", { ctrlKey: true }), shortcuts, other)).toBeNull();
  });

  it("ignores an unbound action, extra modifiers and plain keys", () => {
    const shortcuts = { ...DEFAULT_SHORTCUTS, refresh: "" };
    expect(matchShortcut(key("r", { ctrlKey: true }), shortcuts, other)).toBeNull();
    expect(
      matchShortcut(key("b", { ctrlKey: true, altKey: true }), DEFAULT_SHORTCUTS, other),
    ).toBeNull();
    expect(matchShortcut(key("b"), DEFAULT_SHORTCUTS, other)).toBeNull();
  });

  it("passes non-Cmd shortcuts through while a terminal has focus", () => {
    const inTerminal = { mac: false, inTerminal: true };
    expect(matchShortcut(key("b", { ctrlKey: true }), DEFAULT_SHORTCUTS, inTerminal)).toBeNull();
    const withSuper = { ...DEFAULT_SHORTCUTS, toggleSidebar: "Cmd+B" };
    expect(matchShortcut(key("b", { metaKey: true }), withSuper, inTerminal)).toBe("toggleSidebar");
  });
});

describe("formatShortcut", () => {
  it("formats for macOS and other platforms", () => {
    expect(formatShortcut("CmdOrCtrl+Shift+\\", true)).toBe("⇧⌘\\");
    expect(formatShortcut("CmdOrCtrl+Alt+Right", false)).toBe("Ctrl+Alt+Right");
    expect(formatShortcut("", false)).toBe("");
  });
});
