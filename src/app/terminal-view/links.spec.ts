// @vitest-environment jsdom
import { describe, expect, it, vi } from "vitest";
import {
  detectedLinkHandler,
  holdsLinkModifier,
  isOpenableUrl,
  isRepo,
  linkHandler,
  parseIssueLink,
  taskLinkHandler,
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

describe("parseIssueLink", () => {
  it("reads owner, repo and number", () => {
    expect(parseIssueLink("https://github.com/GravionLabs/poimen/issues/193")).toEqual({
      owner: "GravionLabs",
      repo: "poimen",
      number: 193,
    });
    expect(parseIssueLink("https://github.com/a.b/c_d-e/issues/7#issuecomment-1")?.number).toBe(7);
  });

  it.each([
    "https://github.com/GravionLabs/poimen/pull/193",
    "https://github.com/../poimen/issues/1",
    "https://github.com/o/../issues/1",
    "https://github.com/./r/issues/1",
    "https://github.com/o r/p/issues/1",
    "https://github.com/o/p/issues/0",
    "https://github.com/o/p/issues/x",
    "http://github.com/o/p/issues/1",
    "https://evil.com/o/p/issues/1",
  ])("rejects %s", (uri) => expect(parseIssueLink(uri)).toBeNull());
});

describe("isRepo", () => {
  const link = { owner: "GravionLabs", repo: "poimen", number: 1 };
  it("matches the origin regardless of case", () => {
    expect(isRepo(link, { owner: "gravionlabs", name: "Poimen" })).toBe(true);
  });
  it("rejects a foreign repository and a missing origin", () => {
    expect(isRepo(link, { owner: "other", name: "poimen" })).toBe(false);
    expect(isRepo(link, null)).toBe(false);
  });
});

describe("taskLinkHandler", () => {
  const issue = "https://github.com/GravionLabs/poimen/issues/193";
  const click = new Event("click") as MouseEvent;
  const flush = () => new Promise((r) => setTimeout(r));

  it("starts the task on a modifier click and does not open the browser", async () => {
    const open = vi.fn(async () => undefined);
    const start = vi.fn(async () => true);
    taskLinkHandler(open, start, () => true)(click, `${issue}.`);
    await flush();
    expect(start).toHaveBeenCalledWith({ owner: "GravionLabs", repo: "poimen", number: 193 });
    expect(open).not.toHaveBeenCalled();
  });

  it("opens the link when the task is not started (foreign repository)", async () => {
    const open = vi.fn(async () => undefined);
    taskLinkHandler(
      open,
      async () => false,
      () => true,
    )(click, issue);
    await flush();
    expect(open).toHaveBeenCalledWith(issue);
  });

  it("opens the link when starting fails", async () => {
    const open = vi.fn(async () => undefined);
    taskLinkHandler(
      open,
      async () => Promise.reject(new Error("x")),
      () => true,
    )(click, issue);
    await flush();
    expect(open).toHaveBeenCalledWith(issue);
  });

  it("keeps a plain click as it was", async () => {
    const open = vi.fn(async () => undefined);
    const start = vi.fn(async () => true);
    taskLinkHandler(open, start, () => false)(click, issue);
    await flush();
    expect(start).not.toHaveBeenCalled();
    expect(open).toHaveBeenCalledWith(issue);
  });

  it("never involves the task for other links", async () => {
    const open = vi.fn(async () => undefined);
    const start = vi.fn(async () => true);
    taskLinkHandler(open, start, () => true)(click, "https://example.com/a");
    await flush();
    expect(start).not.toHaveBeenCalled();
    expect(open).toHaveBeenCalledWith("https://example.com/a");
  });
});
