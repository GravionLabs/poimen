import { Injectable, computed, inject, signal } from "@angular/core";
import { agentKind, paneDir, rowName, type AgentStatus, type Terminal } from "../core/snapshot";
import { starterOf } from "../core/started-by";
import { SpacesStore } from "../core/spaces-store";
import { Machines } from "../core/machines";
import { TauriBridge, type Unlisten } from "../core/tauri";
import { NotificationCenter, type RunAlert } from "../core/notification-center";
import { EditorTabs } from "../core/editor-tabs";
import { UiState } from "../core/ui-state";
import { TokenTotalsService, windowTotals, type TokenTotals } from "../core/token-totals";
import { isGithubUrl } from "../github/pulls";
import {
  barSummary,
  cardKey,
  cardsOf,
  groupRuns,
  isAlert,
  outcomeText,
  runState,
  type DeliverCard,
  type DeliverRead,
  startedAt,
  withMachines,
} from "./deliver-status";
import { effectiveStatus, runsByTerminal } from "./pane-status";
import { activeLoop, type ActiveLoop, type Launched } from "./deliver-start";

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
  private readonly machines = inject(Machines);
  private readonly ui = inject(UiState);
  private readonly editor = inject(EditorTabs);
  private readonly center = inject(NotificationCenter);
  private readonly tokenTotals = inject(TokenTotalsService);
  /** Attentions already seen (notified or part of the first read), by machine, root, run and id. */
  private readonly seenAlerts = new Set<string>();
  /** Repositories whose first read is in; it only sets the baseline. */
  private readonly baselined = new Set<string>();

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
  /** The runs grouped by subject (#584), the model of the Runs view. */
  readonly groups = computed(() => groupRuns(this.cards()));
  /** The group the view should put its cursor on, set by {@link reveal}. */
  readonly revealed = signal<string | null>(null);
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

  /**
   * The tokens of a run (#896): the usage samples of the terminal in `host["poimen.terminal"]`
   * between the run's start and its last event (`now` while it runs). No link, no total: the
   * `paneOf` fallback does not count. A remote run reads its machine's samples (qualified id).
   */
  readonly tokensOf = computed(() => {
    const samples = this.tokenTotals.samples();
    const now = this.now();
    return (card: DeliverCard): TokenTotals | null => {
      if (card.state.kind !== "ok") return null;
      const { status } = card.state;
      // `cardsOf` qualified the id of a remote run already.
      const id = status.host["poimen.terminal"];
      const mine = id ? samples.get(id) : undefined;
      if (!mine) return null;
      const from = Date.parse(startedAt(status) ?? status.updated);
      const live = status.status === "running" && runState(status, now) === "running";
      const to = live ? now : Date.parse(status.updated);
      if (!Number.isFinite(from) || !Number.isFinite(to)) return null;
      return windowTotals(mine, from, to);
    };
  });

  /**
   * The agent that started the terminal of a run through `start_agent` (#982): its name, with
   * its space when the run's own space is another. Empty without a link or when it is gone.
   */
  starterOf(card: DeliverCard): string | undefined {
    if (card.state.kind !== "ok") return undefined;
    const id = card.state.status.host["poimen.terminal"];
    const { terminals, spaces } = this.store.all();
    const pane = id ? terminals.find((t) => t.terminal_id === id) : undefined;
    const starter = pane ? starterOf(terminals, pane) : undefined;
    if (!starter) return undefined;
    const space = spaces.find((s) => s.space_id === starter.space_id)?.label;
    return space ? `${rowName(starter)} (${space})` : rowName(starter);
  }

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
    this.notify(root, machine, next);
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

  /**
   * One notification-center entry per attention of a run that needs the user (#806); the first
   * read of a repository only records what is already there.
   */
  private notify(root: string, machine: string | undefined, cards: readonly DeliverCard[]): void {
    const where = `${machine ?? ""}\n${root}`;
    const first = !this.baselined.has(where);
    this.baselined.add(where);
    const alerts: RunAlert[] = [];
    for (const card of cards) {
      if (card.state.kind !== "ok") continue;
      const status = card.state.status;
      for (const attention of status.attentions) {
        if (!isAlert(attention, status)) continue;
        const key = `${where}\n${card.run}\n${attention.id}`;
        if (this.seenAlerts.has(key)) continue;
        this.seenAlerts.add(key);
        if (first) continue;
        const label = status.subject?.title || root.split(/[\\/]/).filter(Boolean).pop() || root;
        const said = outcomeText(attention) ?? "needs you";
        alerts.push({
          root,
          ...(machine === undefined ? {} : { machine }),
          label,
          message: `${label}: ${said}`,
        });
      }
    }
    this.center.recordRuns(alerts, Date.now());
  }

  /**
   * Opens the Runs view with the group of the newest run of `root` (on `machine`) expanded and
   * under the cursor, as a notification with `extra.deliverRoot` asks for; `false` when the
   * repository has no run (the view still opens).
   */
  reveal(root: string, machine?: string): boolean {
    const group = this.groups()
      .filter((g) => g.root === root && g.machine === machine)
      .sort((a, b) => b.updated - a.updated)[0];
    this.editor.openDeliver();
    if (!group) return false;
    this.ui.setRunGroupExpanded(group.key, true);
    this.revealed.set(group.key);
    return true;
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

  /** Starts by this action (#746), by repository root: the terminal and when. */
  private readonly launched = new Map<string, Launched>();

  /** Records the terminal of a loop this action just started in the repository at `root`. */
  markLaunched(root: string, terminalId: string): void {
    this.launched.set(root, { terminalId, at: Date.now() });
  }

  /**
   * The loop that makes a second start for `root` (this Mac) pointless: a running run, or a start
   * by this action whose run has not appeared yet. `undefined` when it may start.
   */
  activeLoop(root: string): ActiveLoop | undefined {
    return activeLoop(
      this.received(),
      this.launched.get(root),
      this.store.all().terminals,
      root,
      this.now(),
    );
  }
}
