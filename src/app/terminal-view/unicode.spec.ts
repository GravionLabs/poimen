import { Terminal } from "@xterm/xterm";
import { describe, expect, it } from "vitest";
import { useUnicode11 } from "./unicode";

/** Writes `text` to a real xterm.js and returns the cursor column. */
async function cursorAfter(text: string, unicode11: boolean): Promise<number> {
  const term = new Terminal({ cols: 20, rows: 3, allowProposedApi: true });
  if (unicode11) useUnicode11(term);
  await new Promise<void>((done) => term.write(text, done));
  const x = term.buffer.active.cursorX;
  term.dispose();
  return x;
}

describe("useUnicode11", () => {
  it("measures emoji as two cells, as Claude Code does", async () => {
    expect(await cursorAfter("a\u{1F600}b", true)).toBe(4);
  });

  it("selects Unicode 11 widths", () => {
    const term = new Terminal({ allowProposedApi: true });
    useUnicode11(term);
    expect(term.unicode.activeVersion).toBe("11");
    term.dispose();
  });

  it("differs from the xterm.js default, which was the bug", async () => {
    expect(await cursorAfter("a\u{1F600}b", false)).toBe(3);
  });
});
