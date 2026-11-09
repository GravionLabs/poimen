import { Terminal } from "@xterm/headless";
import { describe, expect, it } from "vitest";
import { useUnicode11 } from "./unicode";

/**
 * What the server sends for an 8x4 grid holding `✅⚠🚀ab` and `0123456789`, the cursor on row 1,
 * column 6. Pinned by `snapshot_of_wide_characters_and_a_wrapped_row` in
 * `src-tauri/crates/poimen-server/src/emulator.rs`: wide characters without their spacer cells,
 * the second row longer than `cols` (autowrap), a final CUP. Keep both in sync.
 */
const SNAPSHOT = "\x1bc✅⚠🚀ab\r\n0123456789\r\n\x1b[1;6H";

/** Where the server grid has each character: [row, column, char, width]. */
const SERVER_CELLS: [number, number, string, number][] = [
  [0, 0, "✅", 2],
  [0, 2, "⚠", 1],
  [0, 3, "🚀", 2],
  [0, 5, "a", 1],
  [0, 6, "b", 1],
  [1, 0, "0", 1],
  [1, 7, "7", 1],
  [2, 0, "8", 1],
  [2, 1, "9", 1],
];

async function replay(unicode11: boolean): Promise<Terminal> {
  const term = new Terminal({ cols: 8, rows: 4, allowProposedApi: true });
  if (unicode11) useUnicode11(term);
  await new Promise<void>((done) => term.write(SNAPSHOT, done));
  return term;
}

function cells(term: Terminal): [number, number, string, number][] {
  const buf = term.buffer.active;
  return SERVER_CELLS.map(([row, col]) => {
    const cell = buf.getLine(row)?.getCell(col);
    return [row, col, cell?.getChars() ?? "", cell?.getWidth() ?? 0];
  });
}

describe("useUnicode11", () => {
  it("puts every cell of a server snapshot where the server grid has it", async () => {
    const term = await replay(true);
    expect(cells(term)).toEqual(SERVER_CELLS);
    expect(term.buffer.active.getLine(2)?.isWrapped).toBe(true);
    term.dispose();
  });

  it("puts the cursor where the server grid has it, on the `a`", async () => {
    const term = await replay(true);
    const buf = term.buffer.active;
    expect([buf.cursorY, buf.cursorX]).toEqual([0, 5]);
    expect(buf.getLine(buf.cursorY)?.getCell(buf.cursorX)?.getChars()).toBe("a");
    term.dispose();
  });

  it("selects Unicode 11 widths", () => {
    const term = new Terminal({ allowProposedApi: true });
    useUnicode11(term);
    expect(term.unicode.activeVersion).toBe("11");
    term.dispose();
  });

  it("without it, the default Unicode 6 widths shift the row, which was the bug", async () => {
    const term = await replay(false);
    expect(cells(term)).not.toEqual(SERVER_CELLS);
    term.dispose();
  });
});
