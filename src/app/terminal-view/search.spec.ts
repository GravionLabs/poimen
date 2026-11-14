import { describe, expect, it, vi } from "vitest";
import { hex6, matchLabel, searchDecorations, TerminalSearch } from "./search";

describe("hex6", () => {
  it("keeps #RRGGBB and expands #RGB", () => {
    expect(hex6("#a1B2c3", "#000000")).toBe("#a1B2c3");
    expect(hex6("#abc", "#000000")).toBe("#aabbcc");
  });

  it("falls back for anything else", () => {
    expect(hex6("", "#111111")).toBe("#111111");
    expect(hex6("red", "#111111")).toBe("#111111");
    expect(hex6("#11223344", "#111111")).toBe("#111111");
  });
});

describe("searchDecorations", () => {
  const tokens: Record<string, string> = {
    "--term-selection-bg": "#cfe0fa",
    "--term-ansi-3": "#8a5a00",
    "--term-fg": "#26272a",
  };

  it("takes the colors from the tokens", () => {
    const d = searchDecorations((n) => tokens[n] ?? "");
    expect(d.matchBackground).toBe("#cfe0fa");
    expect(d.activeMatchBackground).toBe("#8a5a00");
    expect(d.activeMatchBorder).toBe("#26272a");
  });

  it("prefers the configured selection color and survives missing tokens", () => {
    expect(searchDecorations(() => "", { selection: "#010203" }).matchBackground).toBe("#010203");
    expect(searchDecorations(() => "").matchBackground).toBe("#555555");
  });
});

describe("matchLabel", () => {
  it("counts from one, and handles no query and no match", () => {
    expect(matchLabel("a", 0, 3)).toBe("1 of 3");
    expect(matchLabel("a", -1, 0)).toBe("No results");
    expect(matchLabel("", 0, 0)).toBe("");
  });
});

describe("TerminalSearch", () => {
  it("opens the last focused view and forgets a destroyed one", () => {
    const hub = new TerminalSearch();
    const view = { openSearch: vi.fn() };
    expect(hub.open()).toBe(false);
    hub.focused(view);
    expect(hub.open()).toBe(true);
    expect(view.openSearch).toHaveBeenCalledTimes(1);
    hub.gone(view);
    expect(hub.open()).toBe(false);
  });
});
