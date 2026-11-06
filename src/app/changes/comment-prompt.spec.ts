import { describe, expect, it } from "vitest";
import type { LineComment } from "../core/ui-state";
import {
  MAX_PROMPT_LENGTH,
  buildPrompt,
  commentLocation,
  commentsMarkdown,
  fenced,
  withFile,
} from "./comment-prompt";

function comment(over: Partial<LineComment> = {}): LineComment {
  return {
    id: "c1",
    path: "src/a.ts",
    side: "new",
    start: 3,
    end: 3,
    quote: "const x = 1;",
    text: "Name this better.",
    ...over,
  };
}

describe("comment prompt", () => {
  it("builds one intro line and a block per single-line comment", () => {
    const { text, file } = buildPrompt([comment()]);
    expect(file).toBeNull();
    expect(text).toBe(
      [
        "Please address these review comments on the changes in this worktree.",
        "",
        "src/a.ts:3",
        "```",
        "const x = 1;",
        "```",
        "Name this better.",
      ].join("\n"),
    );
  });

  it("writes a range as start-end with every quoted line, in order", () => {
    const md = commentsMarkdown([
      comment({ start: 3, end: 5, quote: "a\nb\nc", text: "Why?" }),
      comment({ id: "c2", path: "b.rs", start: 1, end: 1, quote: "fn f() {}", text: "Ok." }),
    ]);
    expect(md).toBe(
      [
        "src/a.ts:3-5",
        "```",
        "a\nb\nc",
        "```",
        "Why?",
        "",
        "b.rs:1",
        "```",
        "fn f() {}",
        "```",
        "Ok.",
      ].join("\n"),
    );
  });

  it("marks comments on removed lines", () => {
    expect(commentLocation(comment({ side: "old", start: 2, end: 4 }))).toBe(
      "src/a.ts:2-4 (removed lines)",
    );
  });

  it("writes a comment without a line as the path and no fence when there is no quote", () => {
    const md = commentsMarkdown([comment({ start: 0, end: 0, quote: "", text: "Whole file." })]);
    expect(md).toBe("src/a.ts\nWhole file.");
  });

  it("uses a fence longer than any backtick run of the quote", () => {
    expect(fenced("plain")).toBe("```\nplain\n```");
    expect(fenced("a ``` b")).toBe("````\na ``` b\n````");
    expect(fenced("`one` and ````four````")).toBe("`````\n`one` and ````four````\n`````");
    const md = commentsMarkdown([comment({ quote: "echo ```done```" })]);
    expect(md).toContain("````\necho ```done```\n````");
  });

  it("moves long comments to a file and keeps the prompt short", () => {
    const long = comment({ text: "x".repeat(MAX_PROMPT_LENGTH) });
    const { text, file } = buildPrompt([long]);
    expect(file).toBe(commentsMarkdown([long]));
    expect(text.length).toBeLessThan(MAX_PROMPT_LENGTH);
    expect(withFile("/w/.agents/state/comments-1.md")).toContain("/w/.agents/state/comments-1.md");
    expect(withFile("/w/.agents/state/comments-1.md")).toContain(text);
  });

  it("keeps a prompt of exactly the limit inline", () => {
    const base = buildPrompt([comment({ text: "" })]).text.length;
    const fit = buildPrompt([comment({ text: "y".repeat(MAX_PROMPT_LENGTH - base) })]);
    expect(fit.text.length).toBe(MAX_PROMPT_LENGTH);
    expect(fit.file).toBeNull();
    expect(
      buildPrompt([comment({ text: "y".repeat(MAX_PROMPT_LENGTH - base + 1) })]).file,
    ).not.toBeNull();
  });
});
