import { describe, expect, it } from "vitest";
import { fileName, formatSize, isMarkdown, previewNotice } from "./preview-model";

describe("isMarkdown", () => {
  it.each(["a.md", "docs/PLAN.MD", "x.markdown", "y.mdx"])("%s is Markdown", (path) => {
    expect(isMarkdown(path)).toBe(true);
  });

  it.each(["a.ts", "md", "a.md.bak", "README"])("%s is not", (path) => {
    expect(isMarkdown(path)).toBe(false);
  });
});

describe("fileName", () => {
  it("is the last part of a path with either separator", () => {
    expect(fileName("/a/b/c.md")).toBe("c.md");
    expect(fileName("C:\\a\\c.md")).toBe("c.md");
    expect(fileName("c.md")).toBe("c.md");
  });
});

describe("formatSize", () => {
  it("picks the unit", () => {
    expect(formatSize(12)).toBe("12 B");
    expect(formatSize(1536)).toBe("1.5 KiB");
    expect(formatSize(3 * 1024 * 1024)).toBe("3.0 MiB");
  });
});

describe("previewNotice", () => {
  it("names a binary file and a file over the limit, and is null for text", () => {
    expect(previewNotice({ state: "binary", size: 10 })).toContain("Binary file (10 B)");
    expect(previewNotice({ state: "too_large", size: 2 * 1024 * 1024 })).toContain(
      "Too large to preview (2.0 MiB, the limit is 1 MiB)",
    );
    expect(previewNotice({ state: "text", text: "x", size: 1 })).toBeNull();
  });
});
