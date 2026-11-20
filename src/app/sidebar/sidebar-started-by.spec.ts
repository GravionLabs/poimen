import { describe, expect, it } from "vitest";
import { buildSpaces, type Snapshot, type Terminal } from "../core/snapshot";
import { starterOf } from "../core/started-by";
import { DEFAULT_SIDEBAR_ROWS, renderRow, rowType, spaceContext } from "./spaces";
import { spaceNodes } from "./space-nodes";

const pane = (id: string, space: string, extra = {}) => ({
  terminal_id: id,
  tab_id: `tab-${id}`,
  space_id: space,
  agent_status: "idle" as const,
  agent: "claude",
  label: id,
  ...extra,
});

function snapshot(terminals: object[]): Snapshot {
  const space = (space_id: string, number: number) => ({
    space_id,
    label: `space-${space_id}`,
    number,
    agent_status: "idle" as const,
  });
  return {
    spaces: [space("s1", 1), space("s2", 2)],
    tabs: terminals.map((t, i) => ({
      tab_id: (t as Terminal).tab_id,
      space_id: (t as Terminal).space_id,
      label: (t as Terminal).label,
      number: i + 1,
      agent_status: "idle" as const,
    })),
    terminals,
  } as unknown as Snapshot;
}

const keys = (nodes: { key: string; children?: unknown[] }[]): unknown[] =>
  nodes.map((n) => (n.children?.length ? [n.key, keys(n.children as never)] : n.key));

describe("agents started by another agent (#982)", () => {
  it("nests an agent below its starter when both are in one space", () => {
    const s = snapshot([
      pane("lead", "s1"),
      pane("other", "s1"),
      pane("helper", "s1", { started_by: "lead" }),
    ]);
    const [node] = spaceNodes(buildSpaces(s, new Map()));
    expect(keys(node.children!)).toEqual([["lead", ["helper"]], "other"]);
    expect(node.children![0].expanded).toBe(true);
  });

  it("names the starter of an agent in another space and does not nest it", () => {
    const s = snapshot([pane("lead", "s1"), pane("helper", "s2", { started_by: "lead" })]);
    const nodes = buildSpaces(s, new Map());
    const row = nodes[1].rows[0];
    expect(row.startedBy).toBe("lead (space-s1)");
    expect(keys(spaceNodes(nodes)[1].children!)).toEqual(["helper"]);
    const lines = renderRow(
      rowType(row),
      DEFAULT_SIDEBAR_ROWS[rowType(row)],
      row,
      spaceContext(nodes[1]),
    );
    expect(lines.flat().map((seg) => seg.text)).toContain("↳ lead (space-s1)");
  });

  it("leaves a start cycle and a missing starter flat", () => {
    const s = snapshot([
      pane("a", "s1", { started_by: "b" }),
      pane("b", "s1", { started_by: "a" }),
      pane("c", "s1", { started_by: "gone" }),
    ]);
    const [node] = spaceNodes(buildSpaces(s, new Map()));
    expect(keys(node.children!)).toEqual(["a", "b", "c"]);
  });

  it("qualifies the starter of a remote terminal with its machine", () => {
    const remote = pane("m::helper", "m::s1", { started_by: "lead", machine: "m" });
    const lead = pane("m::lead", "m::s1", { machine: "m" });
    const other = pane("lead", "s1");
    expect(starterOf([other, lead] as never, remote as never)?.terminal_id).toBe("m::lead");
    expect(starterOf([other] as never, remote as never)).toBeUndefined();
  });
});
