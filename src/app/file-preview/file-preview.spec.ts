// @vitest-environment jsdom
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { TauriBridge } from "../core/tauri";
import { tauri } from "../core/tauri.mock";
import { FilePreview } from "./file-preview";
import type { FilePreviewData } from "./preview-model";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("../core/tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("../core/tauri.mock")).tauri.listen,
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

beforeAll(() => {
  getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
});

beforeEach(() => {
  tauri.reset();
  TestBed.resetTestingModule();
});

async function render(path: string, data: FilePreviewData | Error) {
  tauri.answer("preview_read", () => {
    if (data instanceof Error) throw data;
    return data;
  });
  const fixture = TestBed.createComponent(FilePreview);
  fixture.componentRef.setInput("path", path);
  await settle(fixture);
  return fixture;
}

async function settle(f: { detectChanges(): void; whenStable(): Promise<unknown> }) {
  for (let i = 0; i < 3; i++) {
    f.detectChanges();
    await new Promise<void>((r) => queueMicrotask(r));
    await f.whenStable();
  }
  f.detectChanges();
}

/** The text of the source view once it is there (the view and its language load lazily). */
async function source(f: { detectChanges(): void; nativeElement: HTMLElement }) {
  return vi.waitFor(() => {
    f.detectChanges();
    const content = f.nativeElement.querySelector(".cm-content");
    if (!content) throw new Error("no source view yet");
    return content;
  });
}

const root = (f: { nativeElement: HTMLElement }) => f.nativeElement;
const text = (data: string): FilePreviewData => ({ state: "text", text: data, size: data.length });
const button = (f: { nativeElement: HTMLElement }, label: string) =>
  [...root(f).querySelectorAll("button")].find((b) => b.textContent!.trim() === label);

describe("FilePreview", () => {
  it("renders Markdown through the sanitizing renderer", async () => {
    const f = await render(
      "/r/PLAN.md",
      text("# Plan\n\n<script>alert(1)</script>\n\n[x](javascript:alert(1)) **bold**"),
    );
    const html = root(f).querySelector('[data-testid="rendered"]')!;
    expect(html.querySelector("h1")?.textContent).toBe("Plan");
    expect(html.querySelector("strong")?.textContent).toBe("bold");
    expect(html.querySelector("script")).toBeNull();
    expect(html.innerHTML).not.toContain("javascript:");
    expect(tauri.calls("preview_read")).toEqual([{ path: "/r/PLAN.md" }]);
  });

  it("toggles between the rendered text and the source", async () => {
    const f = await render("/r/PLAN.md", text("# Plan"));
    expect(root(f).querySelector("app-source-view")).toBeNull();
    f.componentInstance["view"].set("source");
    await settle(f);
    expect(root(f).querySelector('[data-testid="rendered"]')).toBeNull();
    expect((await source(f)).textContent).toBe("# Plan");
  });

  it("shows other text as source without a toggle", async () => {
    const f = await render("/r/a.ts", text("const a = 1;"));
    expect(root(f).querySelector("hx-select-button")).toBeNull();
    expect((await source(f)).textContent).toBe("const a = 1;");
  });

  it("opens http links in the browser and never navigates for other links", async () => {
    const f = await render("/r/a.md", text("[web](https://example.com) [rel](./b.md)"));
    const [web, rel] = [...root(f).querySelectorAll<HTMLAnchorElement>(".markdown a")];
    const opened = vi.spyOn(TestBed.inject(TauriBridge), "openUrl").mockResolvedValue();
    const webClick = new MouseEvent("click", { bubbles: true, cancelable: true });
    web.dispatchEvent(webClick);
    const relClick = new MouseEvent("click", { bubbles: true, cancelable: true });
    rel.dispatchEvent(relClick);
    expect(webClick.defaultPrevented).toBe(true);
    expect(relClick.defaultPrevented).toBe(true);
    expect(opened.mock.calls).toEqual([["https://example.com"]]);
  });

  it("is read-only", async () => {
    const f = await render("/r/a.ts", text("x"));
    const content = await source(f);
    expect(content.getAttribute("contenteditable")).toBe("false");
  });

  it("shows a notice for a binary file and for a file over 1 MiB", async () => {
    const binary = await render("/r/a.png", { state: "binary", size: 10 });
    expect(root(binary).querySelector(".note")?.textContent).toContain("Binary file");
    expect(root(binary).querySelector(".cm-content")).toBeNull();
    TestBed.resetTestingModule();
    const large = await render("/r/big.md", { state: "too_large", size: 5 * 1024 * 1024 });
    expect(root(large).querySelector(".note")?.textContent).toContain("Too large to preview");
    expect(root(large).querySelector("hx-select-button")).toBeNull();
    expect(root(large).querySelector('[data-testid="rendered"]')).toBeNull();
  });

  it("shows the error of a file that cannot be read", async () => {
    const f = await render("/r/gone.md", new Error("The file does not exist"));
    expect(root(f).querySelector('[role="alert"]')?.textContent).toContain("does not exist");
  });

  it("follows the file: it is watched and read again on a change", async () => {
    let current = text("# One");
    tauri.answer("preview_read", () => current);
    const f = TestBed.createComponent(FilePreview);
    f.componentRef.setInput("path", "/r/PLAN.md");
    await settle(f);
    expect(tauri.calls("preview_watch")).toEqual([{ path: "/r/PLAN.md" }]);
    expect(root(f).querySelector("h1")?.textContent).toBe("One");
    current = text("# Two");
    tauri.emit("file-preview", { path: "/r/other.md" });
    await settle(f);
    expect(root(f).querySelector("h1")?.textContent).toBe("One");
    tauri.emit("file-preview", { path: "/r/PLAN.md" });
    await settle(f);
    expect(root(f).querySelector("h1")?.textContent).toBe("Two");
    f.destroy();
    expect(tauri.calls("preview_unwatch")).toContainEqual({ path: "/r/PLAN.md" });
    expect(tauri.unlisten).toHaveBeenCalledWith("file-preview");
  });

  it("has Open in editor", async () => {
    const f = await render("/r/PLAN.md", text("x"));
    button(f, "Open in editor")!.click();
    await settle(f);
    expect(tauri.calls("open_in_editor")).toEqual([
      { path: "/r/PLAN.md", line: null, column: null },
    ]);
  });
});
