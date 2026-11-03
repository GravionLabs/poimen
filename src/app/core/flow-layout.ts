import { graphlib, layout } from "@dagrejs/dagre";
import type { FlowGraph } from "./flow-graph";

export const FLOW_NODE_WIDTH = 220;
export const FLOW_NODE_HEIGHT = 84;

export interface FlowPoint {
  x: number;
  y: number;
}

/**
 * Automatic left-to-right layout (dagre): the top-left corner of every node. Parallel subagents
 * end up above each other; no node is placed by hand.
 */
export function layoutFlow(graph: FlowGraph): Map<string, FlowPoint> {
  const g = new graphlib.Graph();
  g.setGraph({ rankdir: "LR", nodesep: 24, ranksep: 56, marginx: 16, marginy: 16 });
  g.setDefaultEdgeLabel(() => ({}));
  for (const node of graph.nodes)
    g.setNode(node.id, { width: FLOW_NODE_WIDTH, height: FLOW_NODE_HEIGHT });
  for (const edge of graph.edges) g.setEdge(edge.from, edge.to);
  layout(g);
  const points = new Map<string, FlowPoint>();
  for (const node of graph.nodes) {
    const placed = g.node(node.id);
    points.set(node.id, {
      x: placed.x - FLOW_NODE_WIDTH / 2,
      y: placed.y - FLOW_NODE_HEIGHT / 2,
    });
  }
  return points;
}
