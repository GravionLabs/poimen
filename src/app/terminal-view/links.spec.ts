// @vitest-environment jsdom
import { describe, expect, it, vi } from "vitest";
import {
  detectedLinkHandler,
  holdsLinkModifier,
  isOpenableUrl,
  linkHandler,
  trimTrailingPunctuation,
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

describe("trimTrailingPunctuation", () => {
  it.each([
    ["https://x.y/a).", "https://x.y/a"],
    ["https://x.y/a,", "https://x.y/a"],
    ["https://x.y/a.", "https://x.y/a"],
    ["https://x.y/a)", "https://x.y/a"],
    ["https://x.y/a],", "https://x.y/a"],
    ["https://example.com/a;:'\"", "https://example.com/a"],
    ["https://x.y/a", "https://x.y/a"],
    ["", ""],
  ])("trims %s", (input, expected) => expect(trimTrailingPunctuation(input)).toBe(expected));

  it("keeps a closing parenthesis with its opening partner", () => {
    expect(trimTrailingPunctuation("https://en.wikipedia.org/wiki/Foo_(bar)")).toBe(
      "https://en.wikipedia.org/wiki/Foo_(bar)",
    );
  });

  it("trims the URL part of a parenthesized link as the addon matches it", () => {
    expect(trimTrailingPunctuation("https://x.y/a)")).toBe("https://x.y/a");
  });

  it("removes only the extra closing parenthesis", () => {
    expect(trimTrailingPunctuation("https://en.wikipedia.org/wiki/Foo_(bar))")).toBe(
      "https://en.wikipedia.org/wiki/Foo_(bar)",
    );
    expect(trimTrailingPunctuation("https://en.wikipedia.org/wiki/Foo_(bar).")).toBe(
      "https://en.wikipedia.org/wiki/Foo_(bar)",
    );
  });
});

describe("detectedLinkHandler", () => {
  it("opens the trimmed URL", () => {
    const open = vi.fn(async () => undefined);
    detectedLinkHandler(open)(new Event("click") as MouseEvent, "https://x.y/a).");
    expect(open).toHaveBeenCalledWith("https://x.y/a");
  });
});
