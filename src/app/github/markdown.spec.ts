// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { renderMarkdown } from "./markdown";

describe("renderMarkdown", () => {
  it("renders Markdown", () => {
    const html = renderMarkdown("# Title\n\nSome **bold** and `code`\n\n- a\n- b");
    expect(html).toContain("<h1>Title</h1>");
    expect(html).toContain("<strong>bold</strong>");
    expect(html).toContain("<code>code</code>");
    expect(html).toContain("<li>a</li>");
  });

  it("strips script tags", () => {
    const html = renderMarkdown("hi <script>alert(1)</script> there");
    expect(html).not.toContain("<script");
    expect(html).not.toContain("alert(1)");
    expect(html).toContain("hi");
  });

  it("strips event handlers", () => {
    const html = renderMarkdown(
      '<img src="x" onerror="alert(1)"> <a href="https://a.b" onclick="x()">l</a>',
    );
    expect(html).not.toMatch(/onerror|onclick/i);
    expect(html).toContain('href="https://a.b"');
  });

  it("strips javascript: and data: URLs from links and images", () => {
    const html = renderMarkdown(
      '[x](javascript:alert(1)) <a href="JaVaScRiPt:alert(2)">y</a> ![i](data:text/html;base64,AAAA)',
    );
    expect(html).not.toMatch(/javascript:/i);
    expect(html).not.toMatch(/data:/i);
  });

  it("keeps https links and tolerates empty input", () => {
    expect(renderMarkdown("[ok](https://github.com/o/n)")).toContain(
      'href="https://github.com/o/n"',
    );
    expect(renderMarkdown("")).toBe("");
  });
});
