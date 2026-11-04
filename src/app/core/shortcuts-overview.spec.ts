import { describe, expect, it } from "vitest";
import { POIMEN_COMMANDS } from "./command-palette";
import { SHORTCUT_GROUPS, shortcutTitle } from "./shortcuts-overview";
import { DEFAULT_SHORTCUTS, SHORTCUT_ACTIONS } from "./shortcuts";

describe("shortcut groups", () => {
  it("put every ShortcutAction into exactly one group", () => {
    const listed = SHORTCUT_GROUPS.flatMap((g) => g.actions);
    expect([...listed].sort()).toEqual([...SHORTCUT_ACTIONS].sort());
  });

  it("start with General and the command palette", () => {
    expect(SHORTCUT_GROUPS[0].title).toBe("General");
    expect(SHORTCUT_GROUPS[0].actions[0]).toBe("commandPalette");
  });

  it("title every action like the palette, with a literal for the palette itself", () => {
    expect(shortcutTitle("splitRight")).toBe("Split Right");
    expect(shortcutTitle("commandPalette")).toBe("Command Palette");
    expect(shortcutTitle("showShortcuts")).toBe("Keyboard Shortcuts");
    for (const action of SHORTCUT_ACTIONS) {
      if (POIMEN_COMMANDS.some((c) => c.id === action)) {
        expect(shortcutTitle(action)).toBe(POIMEN_COMMANDS.find((c) => c.id === action)!.title);
      }
    }
  });

  it("binds showShortcuts to CmdOrCtrl+/ and not to a bare ?", () => {
    expect(DEFAULT_SHORTCUTS.showShortcuts).toBe("CmdOrCtrl+/");
  });
});
