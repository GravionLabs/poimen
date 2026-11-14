import { Injectable, computed, inject, signal } from "@angular/core";
import { agentKind, paneDir, type AgentStatus, type Terminal } from "../core/snapshot";
import { SpacesStore } from "../core/spaces-store";
import { Machines } from "../core/machines";
import { TaskStarter } from "../core/task-start";
import { TauriBridge, type Unlisten } from "../core/tauri";
import { isGithubUrl } from "../github/pulls";
import {
  barSummary,
  cardKey,
  cardsOf,
  type DeliverCard,
  type DeliverRead,
  type DeliverStatus,
  type Subject,
  withMachines,
} from "./deliver-status";
import { effectiveStatus, runsByTerminal } from "./pane-status";

/** The task mode `Start loop` runs; `{nr}` is the milestone number, `{title}` its title. */
export const DELIVER_MODE = "deliver-loop";

/** How often the age texts and the stale rule are re-evaluated. */
const CLOCK_MS = 30_000;

interface DeliverEventPayload {
  root: string;
  read: DeliverRead;
  /** The machine of a remote repository (#692); absent on this Mac. */
  machine?: string;
}

/** Position of a card among `cards` by key; cards not there come after all others. */
function index(cards: readonly DeliverCard[], card: DeliverCard): number {
  const i = cards.findIndex((c) => cardKey(c) === cardKey(card));
  return i < 0 ? Number.MAX_SAFE_INTEGER : i;
}

/** The pane that runs the loop of `root`: an agent of the loop's harness inside it, else any agent. */
export function loopPane(
  terminals: readonly Terminal[],
  root: string,
  harness: string,
): string | undefined {
  const inside = terminals.filter((t) => {
    const dir = paneDir(t);
    return dir !== undefined && (dir === root || dir.startsWith(root.replace(/\/$/, "") + "/"));
  });
  const agents = inside.filter((t) => agentKind(t));
  const pick = agents.find((t) => agentKind(t) === harness) ?? agents[0];
  return pick?.terminal_id;
}

/**
 * The number of a milestone subject: from `M5` / `M5.1` in its id or title (the deliver loop's
 * naming), else the `#<number>` at the end of its id (`owner/repo#number`); 0 when there is none.
 */
export function milestoneNumber(subject: Pick<Subject, "id" | "title">): number {
  const named = /^M(\d+)/.exec(subject.id) ?? /^M(\d+)/.exec(subject.title);
  return Number(named?.[1] ?? /#(\d+)$/.exec(subject.id)?.[1] ?? 0);
}

/**
 * The deliver runs of the repositories of the open spaces: the backend watches each repository's
 * progress events (`.agents/events` of the common dir, else the legacy
 * `.agents/deliver-loop/status.json`; `deliver_watch`) and pushes `deliver` events; this keeps one
 * card per run. Runs of the machines (#692) arrive the same way with the machine name: their cards
 * are keyed and labelled by machine, stay with the last known state while it is offline and go
 * when it is removed from the settings.
 */
@Injectable({ providedIn: "root" })
export class Deliver {
  private readonly bridge = inject(TauriBridge);
  private readonly store = inject(SpacesStore);
  private readonly tasks = inject(TaskStarter);
  private readonly machines = inject(Machines);

  /** Every card received, local and remote, in the order they appeared. */
  private readonly received = signal<readonly DeliverCard[]>([]);
  /**
   * One card per run (progress events) or status file, in the order they appeared; remote cards
   * only of configured machines, marked `offline` while theirs is not connected.
   */
  readonly cards = computed(() =>
    withMachines(
      this.received(),
      new Map(this.machines.machines().map((m) => [m.name, m.state] as const)),
    ),
  );
  /** The clock of the age texts. */
  readonly now = signal(Date.now());
  /** The compact status bar item; `null` without a running loop. */
  readonly summary = computed(() => barSummary(this.cards(), this.now()));
  /** Whether any card asks for the user. */
  readonly attention = computed(() => this.summary()?.warning ?? false);

  /**
   * The status poimen shows for a pane: its hook status combined with the runs that name the pane
   * in `host["poimen.terminal"]` (see `effectiveStatus`). The store applies it to the snapshot.
   */
  readonly paneStatus = computed(() => {
    const byTerminal = runsByTerminal(this.cards());
    const now = this.now();
    return (terminalId: string, hook: AgentStatus): AgentStatus => {
      const runs = byTerminal.get(terminalId);
      return runs ? effectiveStatus(hook, runs, now) : hook;
    };
  });

  private watchedKey: string | undefined;
  private listening: Promise<Unlisten> | undefined;
  private timer: ReturnType<typeof setInterval> | undefined;

  /** Starts listening for `deliver` events and the clock; returns what stops both. */
  start(): () => void {
    this.listening ??= this.bridge.listen<DeliverEventPayload>("deliver", (e) =>
      this.apply(e.root, e.read, e.machine),
    );
    this.timer ??= setInterval(() => this.now.set(Date.now()), CLOCK_MS);
    return () => {
      void this.listening?.then((stop) => stop());
      this.listening = undefined;
      clearInterval(this.timer);
      this.timer = undefined;
    };
  }

  /** Watches the repositories of `dirs` (the open spaces); the same set again does nothing. */
  async watch(dirs: readonly string[]): Promise<void> {
    const key = [...new Set(dirs)].sort().join("\n");
    if (key === this.watchedKey) return;
    this.watchedKey = key;
    try {
      const roots = await this.bridge.invoke<string[]>("deliver_watch", {
        dirs: [...new Set(dirs)],
      });
      const keep = new Set(roots);
      // The machines' repositories are not watched here.
      this.received.update((cards) =>
        cards.filter((c) => c.machine !== undefined || keep.has(c.root)),
      );
    } catch (error) {
      // Try again with the next change of the spaces.
      this.watchedKey = undefined;
      console.warn("poimen: could not watch the deliver loops", error);
    }
  }

  /** Applies one read of a repository (on `machine`, else on this Mac): its cards. */
  apply(root: string, read: DeliverRead, machine?: string): void {
    const same = (c: DeliverCard) => c.root === root && c.machine === machine;
    const before = this.received().filter(same);
    const next = cardsOf(root, read, machine);
    this.received.update((cards) => {
      const rest = cards.filter((c) => !same(c));
      // Keep the place of a card that is only updated; new cards come last.
      const at = cards.findIndex(same);
      const placed = [...next].sort(
        (x, y) => index(before, x) - index(before, y) || next.indexOf(x) - next.indexOf(y),
      );
      return at < 0 ? [...rest, ...placed] : [...rest.slice(0, at), ...placed, ...rest.slice(at)];
    });
  }

  /** Opens a GitHub link of the run in the browser; anything else is ignored. */
  async openUrl(url: string | null | undefined): Promise<void> {
    if (url && isGithubUrl(url)) await this.bridge.openUrl(url);
  }

  /**
   * The terminal of a run: the one named by `host["poimen.terminal"]` when it is open, else an
   * agent pane of the run's harness inside the repository; a remote run only finds a terminal of
   * its machine (qualified ids).
   */
  paneOf(card: DeliverCard): string | undefined {
    if (card.state.kind !== "ok") return undefined;
    const { host, harness } = card.state.status;
    const terminals = this.store.all().terminals.filter((t) => t.machine === card.machine);
    const named = host["poimen.terminal"];
    if (named && terminals.some((t) => t.terminal_id === named)) return named;
    return loopPane(terminals, card.root, harness ?? "");
  }

  /**
   * Starts `deliver-loop` for the milestone: the task mode named {@link DELIVER_MODE} (no
   * worktree: a tab in the space of the repository), with the milestone number as `{nr}`.
   */
  startLoop(status: DeliverStatus, root: string, machine?: string): Promise<boolean> {
    const milestone = status.subject;
    // Tasks on a machine are started from its spaces (#693), not from here.
    if (machine !== undefined) return Promise.resolve(false);
    // Only a run on a milestone has a loop to start.
    if (milestone?.kind !== "github.milestone") return Promise.resolve(false);
    return this.tasks.run(root, {
      number: milestoneNumber(milestone),
      title: milestone.title,
      url: milestone.url,
      mode: DELIVER_MODE,
    });
  }
}
