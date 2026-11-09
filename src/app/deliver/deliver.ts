import { Injectable, computed, inject, signal } from "@angular/core";
import { agentKind, paneDir, type AgentStatus, type Terminal } from "../core/herdr";
import type { StatusChange } from "../core/attention";
import { HerdrStore } from "../core/herdr-store";
import { SettingsStore } from "../core/settings";
import { TaskStarter } from "../core/task-start";
import { TauriBridge, type Unlisten } from "../core/tauri";
import { isGithubUrl } from "../github/pulls";
import {
  barSummary,
  cardKey,
  cardsOf,
  transitions,
  type DeliverCard,
  type DeliverEvent,
  type DeliverRead,
  type DeliverStatus,
  type Subject,
} from "./deliver-status";
import { effectiveStatus, endedTerminal, runsByTerminal, withoutRunEnds } from "./pane-status";

/** The task mode `Start loop` runs; `{nr}` is the milestone number, `{title}` its title. */
export const DELIVER_MODE = "deliver-loop";

/** How often the age texts and the stale rule are re-evaluated. */
const CLOCK_MS = 30_000;

/** How long the pane's `done` after its run ended counts as part of the run's end. */
const RUN_END_MS = 10_000;

interface DeliverEventPayload {
  root: string;
  read: DeliverRead;
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
 * card per run, and raises the notifications when a run changes.
 */
@Injectable({ providedIn: "root" })
export class Deliver {
  private readonly bridge = inject(TauriBridge);
  private readonly store = inject(HerdrStore);
  private readonly prefs = inject(SettingsStore);
  private readonly tasks = inject(TaskStarter);

  /** One card per run (progress events) or status file, in the order they appeared. */
  readonly cards = signal<readonly DeliverCard[]>([]);
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

  /** Terminals whose linked run just ended, with the time; see `withoutRunEnds`. */
  private readonly endedAt = new Map<string, number>();

  /**
   * `changes` without the `done` of panes whose linked run just ended: the end of the run is
   * notified once, by `transitions`.
   */
  withoutRunEnds(changes: readonly StatusChange[], now = Date.now()): StatusChange[] {
    for (const [id, at] of this.endedAt) if (now - at > RUN_END_MS) this.endedAt.delete(id);
    return withoutRunEnds(changes, new Set(this.endedAt.keys()));
  }

  private watchedKey: string | undefined;
  private listening: Promise<Unlisten> | undefined;
  private timer: ReturnType<typeof setInterval> | undefined;

  /** Starts listening for `deliver` events and the clock; returns what stops both. */
  start(): () => void {
    this.listening ??= this.bridge.listen<DeliverEventPayload>("deliver", (e) =>
      this.apply(e.root, e.read),
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
      this.cards.update((cards) => cards.filter((c) => keep.has(c.root)));
    } catch (error) {
      // Try again with the next change of the spaces.
      this.watchedKey = undefined;
      console.warn("poimen: could not watch the deliver loops", error);
    }
  }

  /** Applies one read of a repository: its cards, then notifications for what changed. */
  apply(root: string, read: DeliverRead): void {
    const before = this.cards().filter((c) => c.root === root);
    const next = cardsOf(root, read);
    this.cards.update((cards) => {
      const rest = cards.filter((c) => c.root !== root);
      // Keep the place of a card that is only updated; new cards come last.
      const at = cards.findIndex((c) => c.root === root);
      const placed = [...next].sort(
        (x, y) => index(before, x) - index(before, y) || next.indexOf(x) - next.indexOf(y),
      );
      return at < 0 ? [...rest, ...placed] : [...rest.slice(0, at), ...placed, ...rest.slice(at)];
    });
    for (const card of next) {
      const old = before.find((c) => cardKey(c) === cardKey(card));
      if (old?.state.kind === "ok" && card.state.kind === "ok") {
        const ended = endedTerminal(old.state.status, card.state.status);
        if (ended) this.endedAt.set(ended, Date.now());
        this.notify(root, transitions(old.state.status, card.state.status));
      }
    }
  }

  private notify(root: string, events: readonly DeliverEvent[]): void {
    const { notifyOnBlocked, notifyOnDone } = this.prefs.settings();
    for (const event of events) {
      const allowed =
        event.kind === "blocked" || event.kind === "needs-you" ? notifyOnBlocked : notifyOnDone;
      if (!allowed) continue;
      void this.bridge
        .notify(event.title, event.body, { deliverRoot: root })
        .catch((error: unknown) => console.warn("poimen: notification failed", error));
    }
  }

  /** Opens a GitHub link of the run in the browser; anything else is ignored. */
  async openUrl(url: string | null | undefined): Promise<void> {
    if (url && isGithubUrl(url)) await this.bridge.openUrl(url);
  }

  /**
   * The terminal of a run: the one named by `host["poimen.terminal"]` when it is open, else an
   * agent pane of the run's harness inside the repository.
   */
  paneOf(card: DeliverCard): string | undefined {
    if (card.state.kind !== "ok") return undefined;
    const { host, harness } = card.state.status;
    const terminals = this.store.snapshot().terminals;
    const named = host["poimen.terminal"];
    if (named && terminals.some((t) => t.terminal_id === named)) return named;
    return loopPane(terminals, card.root, harness ?? "");
  }

  /**
   * Starts `deliver-loop` for the milestone: the task mode named {@link DELIVER_MODE} (no
   * worktree: a tab in the space of the repository), with the milestone number as `{nr}`.
   */
  startLoop(status: DeliverStatus, root: string): Promise<boolean> {
    const milestone = status.subject;
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
