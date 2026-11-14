import { Injectable, computed, effect, inject, signal, untracked } from "@angular/core";
import { agentName } from "./backend";
import { SpacesStore } from "./spaces-store";
import { TauriBridge } from "./tauri";
import { UiState } from "./ui-state";

/** Wire shape of `hooks_status` (`hooks.rs`). */
export interface HookStatus {
  agent: string;
  state: "installed" | "partial" | "missing" | "unknown";
}

/** The agents whose hooks are missing (or partial); `unknown` is never offered. */
export function missingHooks(statuses: readonly HookStatus[]): string[] {
  return statuses.filter((s) => s.state === "missing" || s.state === "partial").map((s) => s.agent);
}

export { agentName };

/**
 * Offers to install the agent hooks (`poimen-server integration install`) once the poimen backend
 * runs and an agent with hooks lacks them. "Not now" is remembered in the UI state; nothing
 * is installed without the click.
 */
@Injectable({ providedIn: "root" })
export class HooksOffer {
  private readonly bridge = inject(TauriBridge);
  private readonly store = inject(SpacesStore);
  private readonly ui = inject(UiState);

  private readonly _missing = signal<string[]>([]);
  private readonly _statuses = signal<HookStatus[]>([]);
  private readonly _busy = signal(false);
  private readonly _error = signal<string | null>(null);
  private checked = false;

  /** Agents offered; empty when there is nothing to offer. */
  readonly missing = this._missing.asReadonly();
  /** The last hook state per agent, for the Server settings page. */
  readonly statuses = this._statuses.asReadonly();
  readonly visible = computed(() => this._missing().length > 0);
  readonly busy = this._busy.asReadonly();
  readonly error = this._error.asReadonly();

  constructor() {
    effect(() => {
      const status = this.store.backend();
      const ready = status?.state === "running" || status?.state === "started";
      if (ready && this.store.backendKind() === "poimen") untracked(() => void this.check());
    });
  }

  /** Looks for missing hooks once per run. */
  async check(): Promise<void> {
    if (this.checked) return;
    this.checked = true;
    await this.refresh();
  }

  /**
   * Reads the hook states. Without a "Not now" every missing agent is offered; after it only an
   * agent whose hooks were seen installed and are now gone (moved or deleted executable).
   */
  async refresh(): Promise<void> {
    try {
      const statuses = (await this.bridge.invoke<HookStatus[]>("hooks_status")) ?? [];
      this._statuses.set(statuses);
      const gone = missingHooks(statuses);
      const seen = this.ui.hooksSeen();
      this._missing.set(
        this.ui.hooksOfferDismissed() ? gone.filter((a) => seen.includes(a)) : gone,
      );
      // After the offer is computed: a vanished agent stays "seen" until the user declines it.
      this.ui.setHooksSeen(
        statuses.filter((s) => s.state === "installed").map((s) => s.agent),
        statuses.filter((s) => s.state === "unknown").map((s) => s.agent),
      );
    } catch {
      // no bundled server found: nothing to offer
    }
  }

  /** Installs the hooks of every offered agent; keeps the offer open with the error on failure. */
  async install(): Promise<void> {
    await this.installAgents([...this._missing()]);
  }

  /** Installs the hooks of one agent (the Server settings page). */
  async installAgent(agent: string): Promise<void> {
    await this.installAgents([agent]);
  }

  private async installAgents(agents: string[]): Promise<void> {
    this._busy.set(true);
    this._error.set(null);
    try {
      for (const agent of agents) {
        await this.bridge.invoke("hooks_install", { agent });
      }
      this._missing.update((m) => m.filter((a) => !agents.includes(a)));
      await this.refresh();
    } catch (err) {
      this._error.set(String(err));
    } finally {
      this._busy.set(false);
    }
  }

  /** "Not now": closes the offer; it returns only when installed hooks go missing. */
  dismiss(): void {
    this.ui.dismissHooksOffer();
    this.ui.setHooksSeen([], this._missing());
    this._missing.set([]);
    this._error.set(null);
  }
}
