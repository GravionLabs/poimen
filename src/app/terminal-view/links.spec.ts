// @vitest-environment jsdom
import { describe, expect, it, vi } from "vitest";
import {
  LinkUnderline,
  holdsLinkModifier,
  isOpenableUrl,
  linkHandler,
  underlineRects,
} from "./links";

describe("isOpenableUrl", () => {
  it.each(["https://github.com/GravionLabs/poimen/pull/69", "http://localhost:4200/x?a=1#b"])(
    "accepts %s",
    (uri) => expect(isOpenableUrl(uri)).toBe(true),
  );

  it.each([
    "file:///etc/passwd",
    "javascript:alert(1)",
    "vscode://open?x=1",
    "mailto:a@b.c",
    "not a url",
    "",
  ])("rejects %s", (uri) => expect(isOpenableUrl(uri)).toBe(false));
});

describe("linkHandler", () => {
  const event = new Event("click") as MouseEvent;

  it("opens an https link through the opener", () => {
    const open = vi.fn(async () => undefined);
    linkHandler(open)(event, "https://example.com/a");
    expect(open).toHaveBeenCalledWith("https://example.com/a");
  });

  it("never opens other schemes", () => {
    const open = vi.fn(async () => undefined);
    linkHandler(open)(event, "javascript:alert(1)");
    linkHandler(open)(event, "file:///etc/hosts");
    expect(open).not.toHaveBeenCalled();
  });

  it("survives an opener failure", async () => {
    const open = vi.fn(async () => Promise.reject(new Error("denied")));
    expect(() => linkHandler(open)(event, "https://example.com")).not.toThrow();
    await Promise.resolve();
  });
});

describe("holdsLinkModifier", () => {
  it("is Cmd on macOS", () => {
    expect(holdsLinkModifier({ metaKey: true, ctrlKey: false }, true)).toBe(true);
    expect(holdsLinkModifier({ metaKey: false, ctrlKey: true }, true)).toBe(false);
  });

  it("is Ctrl elsewhere", () => {
    expect(holdsLinkModifier({ metaKey: false, ctrlKey: true }, false)).toBe(true);
    expect(holdsLinkModifier({ metaKey: true, ctrlKey: false }, false)).toBe(false);
  });
});

describe("underlineRects", () => {
  const cell = { width: 10, height: 20 };

  it("covers the cells of a link on one row, on the bottom of the cell", () => {
    const range = { start: { x: 3, y: 1 }, end: { x: 5, y: 1 } };
    expect(underlineRects(range, 80, cell)).toEqual([{ left: 20, top: 19, width: 30 }]);
  });

  it("splits a wrapped link into one segment per row", () => {
    const range = { start: { x: 78, y: 2 }, end: { x: 3, y: 4 } };
    expect(underlineRects(range, 80, cell)).toEqual([
      { left: 770, top: 39, width: 30 },
      { left: 0, top: 59, width: 800 },
      { left: 0, top: 79, width: 30 },
    ]);
  });

  it("follows the scroll position", () => {
    const range = { start: { x: 1, y: 11 }, end: { x: 2, y: 11 } };
    expect(underlineRects(range, 80, cell, 10)).toEqual([{ left: 0, top: 19, width: 20 }]);
  });
});

describe("LinkUnderline", () => {
  it("replaces the previous underline on show and removes it on hide", () => {
    const screen = document.createElement("div");
    const underline = new LinkUnderline();
    underline.show(screen, [{ left: 0, top: 0, width: 5 }], "red");
    underline.show(screen, [{ left: 1, top: 2, width: 3 }], "red");
    expect(screen.querySelectorAll(".link-underline")).toHaveLength(1);
    underline.hide();
    expect(screen.querySelectorAll(".link-underline")).toHaveLength(0);
  });
});
