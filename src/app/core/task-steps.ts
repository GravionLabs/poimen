/** The steps of creating a space with a worktree (#1041), in order. */
export type SpaceStepId = "worktree" | "setup" | "agent";

export type SpaceStepState = "pending" | "current" | "done" | "skipped";

export interface SpaceStep {
  id: SpaceStepId;
  label: string;
  state: SpaceStepState;
}

export const SPACE_STEP_LABELS: Record<SpaceStepId, string> = {
  worktree: "Create worktree",
  setup: "Run setup",
  agent: "Start agent",
};

/** All steps pending except `current`, which is marked, and the ones before it, which are done. */
export function stepsAt(current: SpaceStepId | null): SpaceStep[] {
  const order: SpaceStepId[] = ["worktree", "setup", "agent"];
  const at = current === null ? order.length : order.indexOf(current);
  return order.map((id, i) => ({
    id,
    label: SPACE_STEP_LABELS[id],
    state: i < at ? "done" : i === at ? "current" : "pending",
  }));
}

/** Marks one step as skipped (a project without a setup command). */
export function skip(steps: SpaceStep[], id: SpaceStepId): SpaceStep[] {
  return steps.map((s) => (s.id === id ? { ...s, state: "skipped" } : s));
}
