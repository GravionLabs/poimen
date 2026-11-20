import { describe, expect, it, vi } from "vitest";
import type { Terminal } from "@xterm/xterm";
import { fileLinkProvider, findPaths, offersFileLinks, type ResolvedFile } from "./file-links";

const paths = (text: string) => findPaths(text).map((m) => m.path);

describe("findPaths", () => {
  it("finds path, path:line and path:line:column", () => {
    expect(findPaths("src/app/a.ts")).toEqual([{ path: "src/app/a.ts", start: 0, end: 12 }]);
    const [m] = findPaths("error in src/a.ts:12");
    expect(m).toMatchObject({ path: "src/a.ts", line: 12, column: undefined, start: 9, end: 20 });
    expect(findPaths("src/a.ts:12:5")[0]).toMatchObject({ line: 12, column: 5, end: 13 });
  });

  it("takes relative, dot, home and absolute paths and bare file names with an extension", () => {
    expect(paths("./a.ts ../b/c.ts ~/d/e.ts /abs/f.ts README.md .gitignore")).toEqual([
      "./a.ts",
      "../b/c.ts",
      "~/d/e.ts",
      "/abs/f.ts",
      "README.md",
      ".gitignore",
    ]);
  });

  it("leaves out words, numbers and directory-only text", () => {
    expect(paths("hello world 3.14 done . .. ./ src/")).toEqual([]);
  });

  it("drops trailing punctuation and wrapping brackets", () => {
    expect(paths("see src/a.ts.")).toEqual(["src/a.ts"]);
    expect(paths("(src/a.ts), then src/b.ts; and src/c.ts:")).toEqual([
      "src/a.ts",
      "src/b.ts",
      "src/c.ts",
    ]);
    expect(paths("[src/a.ts:3]")).toEqual(["src/a.ts"]);
    expect(findPaths("at src/a.ts:3.")[0]).toMatchObject({ line: 3, end: 13 });
  });

  it("handles quoted paths with spaces and gives the link what is inside the quotes", () => {
    const [m] = findPaths('open "my docs/a b.md:7" now');
    expect(m).toMatchObject({ path: "my docs/a b.md", line: 7, start: 6, end: 22 });
    expect(paths("'dir name/x.ts'")).toEqual(["dir name/x.ts"]);
    expect(paths("`src/a.ts`")).toEqual(["src/a.ts"]);
  });

  it("handles Windows paths", () => {
    expect(paths(String.raw`C:\Users\me\a.ts`)).toEqual([String.raw`C:\Users\me\a.ts`]);
    expect(findPaths(String.raw`C:\Users\me\a.ts:10:2`)[0]).toMatchObject({ line: 10, column: 2 });
    expect(paths(String.raw`.\src\a.ts, D:/x/y.rs`)).toEqual([String.raw`.\src\a.ts`, "D:/x/y.rs"]);
    expect(paths(String.raw`"C:\Program Files\app\a.ts"`)).toEqual([
      String.raw`C:\Program Files\app\a.ts`,
    ]);
  });

  it("does not take URLs or e-mail addresses", () => {
    expect(paths("https://github.com/a/b/issues/1 and http://x.dev/a.html")).toEqual([]);
    expect(paths("mail me@example.com")).toEqual([]);
    expect(paths('"https://example.com/a.png"')).toEqual([]);
  });

  it("finds several paths in one line, in order", () => {
    expect(paths("M src/a.ts and A src/b.ts:4")).toEqual(["src/a.ts", "src/b.ts"]);
  });

  it("ignores a zero or huge position", () => {
    expect(findPaths("a/b.ts:0")[0]).toMatchObject({ line: undefined });
  });
});

describe("fileLinkProvider", () => {
  const term = (text: string) =>
    ({
      buffer: { active: { getLine: () => ({ translateToString: () => text }) } },
    }) as unknown as Terminal;

  it("links only the paths that are files, at their cell range", async () => {
    const resolve = vi.fn(async (_dir: string, paths: string[]): Promise<(ResolvedFile | null)[]> =>
      paths.map((p) => (p === "src/a.ts" ? { path: `/r/${p}` } : null)),
    );
    const activate = vi.fn();
    const provider = fileLinkProvider(
      term("at src/a.ts:12:3 and src/gone.ts"),
      () => "/r",
      resolve,
      activate,
    );
    const links = await new Promise<Parameters<Parameters<typeof provider.provideLinks>[1]>[0]>(
      (r) => provider.provideLinks(4, r),
    );
    expect(resolve).toHaveBeenCalledWith("/r", ["src/a.ts", "src/gone.ts"]);
    expect(links).toHaveLength(1);
    expect(links?.[0].range).toEqual({ start: { x: 4, y: 4 }, end: { x: 16, y: 4 } });
    const event = {} as MouseEvent;
    links?.[0].activate(event, "");
    expect(activate).toHaveBeenCalledWith(event, {
      file: { path: "/r/src/a.ts" },
      line: 12,
      column: 3,
    });
  });

  it("offers nothing without a directory", async () => {
    const provider = fileLinkProvider(term("src/a.ts"), () => undefined, vi.fn(), vi.fn());
    const links = await new Promise((r) => provider.provideLinks(1, r));
    expect(links).toBeUndefined();
  });
});

describe("offersFileLinks", () => {
  it("is false on a pane of a remote machine", () => {
    expect(offersFileLinks({})).toBe(true);
    expect(offersFileLinks({ machine: "box" })).toBe(false);
  });
});
