// @vitest-environment jsdom
import { Injector } from "@angular/core";
import { afterEach, describe, expect, it, vi } from "vitest";
import { TauriBridge } from "./tauri";
import { TerminalColorSync, serverColors } from "./terminal-colors";

const tokens: Record<string, string> = {
  "--term-fg": "#eeeeee",
  "--term-bg": "#101010",
  "--term-cursor": "",
  ...Object.fromEntries(
    Array.from({ length: 16 }, (_, i) => [`--term-ansi-${i}`, `#0000${i.toString(16)}0`]),
  ),
};
const token = (name: string) => tokens[name] ?? "";

describe("serverColors", () => {
  it("takes the scheme tokens and falls back to the foreground for the cursor", () => {
    const colors = serverColors(null, token)!;
    expect(colors.foreground).toBe("#eeeeee");
    expect(colors.background).toBe("#101010");
    expect(colors.cursor).toBe("#eeeeee");
    expect(colors.palette).toHaveLength(16);
    expect(colors.palette![5]).toBe("#000050");
  });

  it("lets the Ghostty colors win", () => {
    const colors = serverColors(
      {
        background: "#000001",
        foreground: null,
        cursor: "#ff0000",
        selectionBackground: null,
        selectionForeground: null,
        palette: ["#123456"],
      },
      token,
    )!;
    expect(colors.background).toBe("#000001");
    expect(colors.foreground).toBe("#eeeeee");
    expect(colors.cursor).toBe("#ff0000");
    expect(colors.palette![0]).toBe("#123456");
    expect(colors.palette![1]).toBe("#000010");
  });

  it("gives nothing for a base color that is not #rrggbb and drops a bad palette", () => {
    expect(serverColors(null, (n) => (n === "--term-bg" ? "red" : token(n)))).toBeNull();
    expect(serverColors(null, (n) => (n === "--term-ansi-3" ? "" : token(n)))!.palette).toBe(
      undefined,
    );
  });
});

describe("TerminalColorSync", () => {
  afterEach(() => document.documentElement.removeAttribute("style"));

  function setup() {
    const invoke = vi.fn(() => Promise.resolve());
    const injector = Injector.create({
      providers: [{ provide: TauriBridge, useValue: { invoke } }, TerminalColorSync],
    });
    return { sync: injector.get(TerminalColorSync), invoke };
  }

  function setTokens(bg: string) {
    const root = document.documentElement.style;
    for (const [k, v] of Object.entries({ ...tokens, "--term-bg": bg })) root.setProperty(k, v);
  }

  it("sends the colors once and again after a change", () => {
    const { sync, invoke } = setup();
    setTokens("#101010");
    sync.update(null);
    sync.update(null);
    setTokens("#202020");
    sync.update(null);
    expect(invoke).toHaveBeenCalledTimes(2);
    expect(invoke).toHaveBeenLastCalledWith("set_terminal_colors", {
      colors: expect.objectContaining({ background: "#202020" }),
    });
  });

  it("sends nothing before the tokens exist", () => {
    const { sync, invoke } = setup();
    sync.update(null);
    expect(invoke).not.toHaveBeenCalled();
  });
});
