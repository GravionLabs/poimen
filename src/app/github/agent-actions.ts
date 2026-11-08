import { Injectable, computed, inject, signal } from "@angular/core";
import { HerdrStore } from "../core/herdr-store";
import {
  parseAgentActions,
  parseHarnesses,
  type AgentAction,
  type HarnessInfo,
} from "../core/settings";
import { SpaceActions, type SpaceOutcome } from "../core/space-actions";
import { TauriBridge } from "../core/tauri";
import { UiState } from "../core/ui-state";
import { GithubIssues, issueLevel, type Issue } from "./issues";
import { GithubPulls, type PullRow } from "./pulls";

/** Why an action that is shown cannot run yet. */
export const NEEDS_WORKTREE = "needs worktree support";

/** An issue or pull request an action runs on, with what `when` looks at. */
export interface ActionTarget {
  kind: "issue" | "pr";
  /** `owner/name`. */
  repo: string;
  /** Directory of the repository root. */
  root: string;
  number: number;
  title: string;
  url: string;
  branch: string;
  base: string;
  level: string | null;
  labels: string[];
  hasChildren: boolean;
  /** `failed` when the checks of the pull request failed. */
  checks: string | null;
  state: string;
  review: string | null;
}

export function issueTarget(repo: string, root: string, issue: Issue): ActionTarget {
  return {
    kind: "issue",
    repo,
    root,
    number: issue.number,
    title: issue.title,
    url: issue.url,
    branch: "",
    base: "",
    level: issueLevel(issue),
    labels: issue.labels,
    hasChildren: issue.subTotal > 0,
    checks: null,
    state: issue.state.toLowerCase(),
    review: null,
  };
}

export function pullTarget(row: PullRow): ActionTarget {
  return {
    kind: "pr",
    repo: row.repo,
    root: row.root,
    number: row.number,
    title: row.title,
    url: row.url,
    branch: row.headRef,
    base: row.baseRef,
    level: null,
    labels: [],
    hasChildren: false,
    checks: row.checks.state === "failing" ? "failed" : null,
    state: "open",
    review: row.reviewDecision,
  };
}

function anyOf(wanted: readonly string[], have: readonly string[]): boolean {
  if (wanted.length === 0) return true;
  const lower = have.map((h) => h.toLowerCase());
  return wanted.some((w) => lower.includes(w.toLowerCase()));
}

/** Whether `action` applies to `target`: the kind and every condition of `when`. */
export function matches(action: AgentAction, target: ActionTarget): boolean {
  const w = action.when;
  return (
    action.on === target.kind &&
    anyOf(w.level, target.level ? [target.level] : []) &&
    anyOf(w.label, target.labels) &&
    !(w.noChildren && target.hasChildren) &&
    (w.checks === null || w.checks === target.checks) &&
    anyOf(w.state, [target.state]) &&
    (w.review === null || w.review === target.review)
  );
}

/** An action offered for the targets; a disabled one carries the reason. */
export interface OfferedAction {
  action: AgentAction;
  enabled: boolean;
  reason: string | null;
}

/** What the Shift+click dialog shows and answers. */
export interface ActionDialog {
  action: AgentAction;
  targets: ActionTarget[];
  harness: string;
  model: string;
}

/** The popup of a row: opened by the `⋯` button or the context menu. */
export interface ActionMenu {
  targets: ActionTarget[];
  x: number;
  y: number;
}

/** One entry of the command palette: "GitHub: Refine #501". */
export interface ActionEntry {
  action: AgentAction;
  title: string;
}

interface Launch {
  kind: string;
  args: string[];
}

/** How a run ended, per target. */
export interface RunSummary {
  started: number[];
  failed: { numbers: number[]; reason: string }[];
}

function text(err: unknown): string {
  return err && typeof err === "object" && "message" in err
    ? String((err as { message: unknown }).message)
    : String(err);
}

function numbers(targets: readonly ActionTarget[]): string {
  return targets.map((t) => `#${t.number}`).join(", ");
}

/** The notification after a run: what started and what failed. */
export function summaryText(label: string, summary: RunSummary): string {
  const parts: string[] = [];
  if (summary.started.length)
    parts.push(`started ${summary.started.map((n) => `#${n}`).join(", ")}`);
  for (const f of summary.failed)
    parts.push(`failed ${f.numbers.map((n) => `#${n}`).join(", ")} (${f.reason})`);
  return `${label}: ${parts.join("; ")}`;
}

/**
 * The agent actions of `github.agentActions` on issues and pull requests (#346): which ones apply
 * to a row, and starting them at the repository root: a new terminal tab `<label> #<nr>` in the
 * space of the repository, where the harness of the action (or the dialog's choice) starts through
 * `agent_start`. `batch: each` starts one agent per item in parallel, `once` one for all; a failed
 * start does not stop the others. `worktree` and `ask` are shown but disabled until #347.
 */
@Injectable({ providedIn: "root" })
export class AgentActions {
  private readonly bridge = inject(TauriBridge);
  private readonly spaces = inject(SpaceActions);
  private readonly store = inject(HerdrStore);
  private readonly pulls = inject(GithubPulls);
  private readonly issues = inject(GithubIssues);
  private readonly ui = inject(UiState);

  private readonly byRoot = signal<ReadonlyMap<string, readonly AgentAction[]>>(new Map());
  private readonly _dialog = signal<ActionDialog | null>(null);
  private readonly _menu = signal<ActionMenu | null>(null);
  private readonly _harnesses = signal<HarnessInfo[]>([]);
  private readonly requested = new Set<string>();

  readonly dialog = this._dialog.asReadonly();
  readonly menu = this._menu.asReadonly();
  /** The harnesses the dialog can offer (found on the `PATH`). */
  readonly harnesses = computed(() => this._harnesses().filter((h) => h.available));

  /** The targets the palette and the keyboard act on: the open or selected pull requests or issue. */
  readonly current = computed<ActionTarget[]>(() => {
    if (this.ui.githubPage() === "pulls") {
      const selected = this.pulls.selectedPulls();
      if (selected.length > 0) return selected.map(pullTarget);
      const row = this.pulls.detailRow();
      return row ? [pullTarget(row)] : [];
    }
    if (this.ui.githubPage() === "issues") {
      const sel = this.issues.detailIssue();
      const root = (this.issues.result() ?? []).find((r) => r.repo === sel?.repo)?.root;
      return sel && root ? [issueTarget(sel.repo, root, sel.issue)] : [];
    }
    return [];
  });

  /** Palette entries for `current`: only the actions that can run now. */
  readonly entries = computed<ActionEntry[]>(() => {
    const targets = this.current();
    return this.offered(targets)
      .filter((o) => o.enabled)
      .map((o) => ({ action: o.action, title: `GitHub: ${o.action.label} ${numbers(targets)}` }));
  });

  /** The repositories the lists have read, once each; the GitHub view calls this when they change. */
  ensureLoaded(): void {
    const roots = [
      ...(this.pulls.result()?.repos ?? []).map((r) => r.root),
      ...(this.issues.result() ?? []).map((r) => r.root),
    ];
    for (const root of roots) void this.load(root);
  }

  /** Reads the actions of the repository at `root` (defaults, user and trusted project file). */
  async load(root: string, force = false): Promise<void> {
    if (!force && this.requested.has(root)) return;
    this.requested.add(root);
    try {
      const raw = await this.bridge.invoke<unknown>("get_agent_actions", { root });
      this.byRoot.update((m) => new Map(m).set(root, parseAgentActions(raw)));
    } catch {
      this.requested.delete(root);
    }
  }

  /** Reads everything again (a changed config file or project file). */
  async reload(): Promise<void> {
    const roots = [...this.requested];
    this.requested.clear();
    await Promise.all(roots.map((r) => this.load(r, true)));
  }

  /**
   * The actions that apply to every one of `targets` (one kind, one repository). A `worktree` or
   * `ask` action is listed but disabled.
   */
  offered(targets: readonly ActionTarget[]): OfferedAction[] {
    const first = targets[0];
    if (!first || targets.some((t) => t.kind !== first.kind || t.root !== first.root)) return [];
    return (this.byRoot().get(first.root) ?? [])
      .filter((a) => targets.every((t) => matches(a, t)))
      .map((action) =>
        action.run === "repoRoot"
          ? { action, enabled: true, reason: null }
          : { action, enabled: false, reason: NEEDS_WORKTREE },
      );
  }

  /** A click on an action: Shift opens the harness and model dialog, else it starts at once. */
  async request(
    action: AgentAction,
    targets: readonly ActionTarget[],
    shift: boolean,
  ): Promise<RunSummary | null> {
    this._menu.set(null);
    if (action.run !== "repoRoot" || targets.length === 0) return null;
    if (!shift) return this.run(action, targets);
    await this.loadHarnesses();
    const harnesses = this._harnesses();
    const harness = action.harness ?? harnesses[0]?.id ?? "";
    this._dialog.set({ action, targets: [...targets], harness, model: action.model ?? "" });
    return null;
  }

  /** Runs the action chosen in the palette on `current`. */
  async runCurrent(id: string): Promise<RunSummary | null> {
    const action = this.entries().find((e) => e.action.id === id)?.action;
    return action ? this.run(action, this.current()) : null;
  }

  setDialog(patch: Partial<Pick<ActionDialog, "harness" | "model">>): void {
    this._dialog.update((d) => (d ? { ...d, ...patch } : d));
  }

  cancelDialog(): void {
    this._dialog.set(null);
  }

  /** Starts with the harness and model of the dialog. */
  async confirmDialog(): Promise<RunSummary | null> {
    const d = this._dialog();
    if (!d) return null;
    this._dialog.set(null);
    return this.run(d.action, d.targets, { harness: d.harness, model: d.model });
  }

  openMenu(targets: readonly ActionTarget[], x: number, y: number): void {
    this._menu.set({ targets: [...targets], x, y });
  }

  closeMenu(): void {
    this._menu.set(null);
  }

  private async loadHarnesses(): Promise<void> {
    if (this._harnesses().length > 0) return;
    try {
      this._harnesses.set(parseHarnesses(await this.bridge.invoke<unknown>("get_harnesses")));
    } catch {
      // The dialog then has no harness to choose from.
    }
  }

  /**
   * Starts the action on `targets` in new tabs of the space of the repository root (focused or
   * created). Resolves to what started and failed; `null` when nothing could be attempted (the
   * notification then says why) or the trust question is still open (the answer continues).
   */
  async run(
    action: AgentAction,
    targets: readonly ActionTarget[],
    choice?: { harness: string; model: string },
  ): Promise<RunSummary | null> {
    const first = targets[0];
    if (!first) return null;
    if (action.run !== "repoRoot") {
      this.spaces.notify(`${action.label}: ${NEEDS_WORKTREE}`);
      return null;
    }
    let launches: Launch[];
    try {
      launches = await this.bridge.invoke<Launch[]>("agent_action_plan", {
        root: first.root,
        id: action.id,
        repo: first.repo,
        items: targets.map((t) => ({
          nr: t.number,
          title: t.title,
          url: t.url,
          branch: t.branch,
          base: t.base,
        })),
        harness: choice?.harness || null,
        model: choice?.model || null,
      });
    } catch (err) {
      this.spaces.notify(`${action.label}: ${text(err)}`);
      return null;
    }
    // `each` has one launch per target, `once` one for all of them.
    const groups =
      launches.length === targets.length && action.batch !== "once"
        ? launches.map((launch, i) => ({ launch, targets: [targets[i]] }))
        : launches.map((launch) => ({ launch, targets: [...targets] }));
    let summary: RunSummary | null = null;
    const after = async (outcome: SpaceOutcome): Promise<void> => {
      summary = await this.startAll(action, first.root, outcome.workspace_id, groups);
      await this.store.refresh();
      this.spaces.notify(summaryText(action.label, summary));
    };
    const outcome = await this.spaces.create(first.root, "", null, null, after);
    if (outcome) return summary;
    // Held for the trust question: the answer creates the space, then `after` starts the agents.
    if (this.spaces.trust()) return null;
    this.spaces.notify(`${action.label}: ${this.spaces.error() ?? "the space was not created"}`);
    return null;
  }

  private async startAll(
    action: AgentAction,
    root: string,
    workspaceId: string,
    groups: { launch: Launch; targets: ActionTarget[] }[],
  ): Promise<RunSummary> {
    const results = await Promise.allSettled(
      groups.map(async ({ launch, targets }) => {
        const paneId = await this.bridge.invoke<string | null>("terminal_create", {
          workspaceId,
          cwd: root,
        });
        if (!paneId) throw new Error("the terminal tab was not created");
        await this.bridge.invoke("agent_start", {
          paneId,
          kind: launch.kind,
          name: `${action.label} ${numbers(targets).replaceAll(",", "")}`,
          args: launch.args,
        });
      }),
    );
    const summary: RunSummary = { started: [], failed: [] };
    results.forEach((r, i) => {
      const nrs = groups[i].targets.map((t) => t.number);
      if (r.status === "fulfilled") summary.started.push(...nrs);
      else summary.failed.push({ numbers: nrs, reason: text(r.reason) });
    });
    return summary;
  }
}
