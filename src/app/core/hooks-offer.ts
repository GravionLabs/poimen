import { Injectable, computed, effect, inject, signal, untracked } from "@angular/core";
import { HerdrStore } from "./herdr-store";
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

const NAMES: Record<string, string> = { claude: "Claude Code", copilot: "GitHub Copilot CLI" };

/** Display name of an agent id. */
export const agentName = (agent: string): string => NAMES[agent] ?? agent;

/**
 * Offers to install the agent hooks (`poimen-server integration install`) once the poimen backend
 * runs and Claude Code or Copilot CLI lack them. "Not now" is remembered in the UI state; nothing
 * is installed without the click.
 */
@Injectable({ providedIn: "root" })
export class HooksOffer {
  private readonly bridge = inject(TauriBridge);
  private readonly store = inject(HerdrStore);
  private readonly ui = inject(UiState);

  private readonly _missing = signal<string[]>([]);
  private readonly _busy = signal(false);
  private readonly _error = signal<string | null>(null);
  private checked = false;

  /** Agents offered; empty when there is nothing to offer. */
  readonly missing = this._missing.asReadonly();
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

  /** Looks for missing hooks once per run, unless the offer was declined. */
  async check(): Promise<void> {
    if (this.checked || this.ui.hooksOfferDismissed()) return;
    this.checked = true;
    try {
      const statuses = await this.bridge.invoke<HookStatus[]>("hooks_status");
      this._missing.set(missingHooks(statuses ?? []));
    } catch {
      // no bundled server found: nothing to offer
    }
  }

  /** Installs the hooks of every offered agent; keeps the offer open with the error on failure. */
  async install(): Promise<void> {
    this._busy.set(true);
    this._error.set(null);
    try {
      for (const agent of this._missing()) {
        await this.bridge.invoke("hooks_install", { agent });
      }
      this._missing.set([]);
    } catch (err) {
      this._error.set(String(err));
    } finally {
      this._busy.set(false);
    }
  }

  /** "Not now": closes the offer and does not ask again. */
  dismiss(): void {
    this.ui.dismissHooksOffer();
    this._missing.set([]);
    this._error.set(null);
  }
}
