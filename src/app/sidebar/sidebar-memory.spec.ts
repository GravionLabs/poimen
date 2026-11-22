import { describe, expect, it } from "vitest";
import { buildSpaces, type Snapshot } from "../core/snapshot";
import { DEFAULT_SIDEBAR_ROWS, memoryText, renderRow, rowType, spaceContext } from "./spaces";

const MB = 1024 * 1024;

const pane = (id: string, memory?: number) => ({
  terminal_id: id,
  tab_id: `tab-${id}`,
  space_id: "s1",
  agent_status: "idle" as const,
  label: id,
  ...(memory === undefined ? {} : { memory_bytes: memory }),
});

const snapshot = (terminals: object[]): Snapshot =>
  ({
    spaces: [{ space_id: "s1", label: "api", number: 1, agent_status: "idle" }],
    tabs: terminals.map((t, i) => ({
      tab_id: `tab-${(t as { terminal_id: string }).terminal_id}`,
      space_id: "s1",
      label: "x",
      number: i + 1,
      agent_status: "idle",
    })),
    terminals,
  }) as unknown as Snapshot;

const texts = (lines: ReturnType<typeof renderRow>) =>
  lines
    .flat()
    .filter((s) => s.token === "memory")
    .map((s) => s.text);

describe("memory per terminal and space (#971)", () => {
  it("formats bytes as MB or GB", () => {
    expect(memoryText(undefined)).toBeUndefined();
    expect(memoryText(10)).toBe("1 MB");
    expect(memoryText(340 * MB)).toBe("340 MB");
    expect(memoryText(1536 * MB)).toBe("1.5 GB");
  });

  it("shows the memory of a terminal and the sum on the space", () => {
    const nodes = buildSpaces(
      snapshot([pane("a", 100 * MB), pane("b", 50 * MB), pane("c")]),
      new Map(),
    );
    const [space] = nodes;
    const ctx = spaceContext(space);
    const rows = space.rows.map((r) =>
      texts(renderRow(rowType(r), DEFAULT_SIDEBAR_ROWS[rowType(r)], r, ctx)),
    );
    expect(rows).toEqual([["100 MB"], ["50 MB"], []]);
    expect(texts(renderRow("workspace", DEFAULT_SIDEBAR_ROWS.workspace, space, ctx))).toEqual([
      "150 MB",
    ]);
  });

  it("shows nothing where no terminal reports memory", () => {
    const [space] = buildSpaces(snapshot([pane("a")]), new Map());
    const lines = renderRow(
      "workspace",
      DEFAULT_SIDEBAR_ROWS.workspace,
      space,
      spaceContext(space),
    );
    expect(texts(lines)).toEqual([]);
  });
});
