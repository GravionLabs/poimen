import { Injectable, computed, effect, inject, signal, untracked } from "@angular/core";
import { agentName } from "./backend";
import { parseHarnesses } from "./settings";
import { SpacesStore } from "./spaces-store";
import { TauriBridge } from "./tauri";
import { UiState } from "./ui-state";

/** Wire shape of `hooks_status` (`hooks.rs`). */
export interface HookStatus {
  agent: string;
  state: "installed" | "partial" | "missing" | "unknown";
  /** The `poimen mcp` entry in the agent's MCP config (#614); absent: none to offer. */
  mcp?: "installed" | "missing" | "unknown" | null;
}

/** The agents whose hooks are missing (or partial); `unknown` is never offered. */
export function missingHooks(statuses: readonly HookStatus[]): string[] {
  return statuses.filter((s) => s.state === "missing" || s.state === "partial").map((s) => s.agent);
}

/** The agents whose hooks are fine but whose `poimen mcp` entry is missing. */
export function missingMcp(statuses: readonly HookStatus[]): string[] {
  const hooks = missingHooks(statuses);
  return statuses
    .filter((s) => s.mcp === "missing" && !hooks.includes(s.agent))
    .map((s) => s.agent);
}

export { agentName };

/** A state as shown in the Server settings table: text, tone and a glyph (never color alone). */
export interface StateTag {
  label: string;
  severity: "success" | "warn" | "secondary";
  icon: string;
}

const TAGS: Record<string, StateTag> = {
  installed: { label: "installed", severity: "success", icon: "✓" },
  partial: { label: "partial", severity: "warn", icon: "!" },
  missing: { label: "missing", severity: "warn", icon: "!" },
  unknown: { label: "unknown", severity: "secondary", icon: "?" },
};

/** The tag of a hook or MCP state; `null` when the agent has none (no MCP config). */
export function stateTag(state: string | null | undefined): StateTag | null {
  return state ? (TAGS[state] ?? TAGS["unknown"]) : null;
}

/**
 * What the one action of an agent's row does, from what is missing: `null` when there is
 * nothing to do or it cannot be told (`unknown`). `hooks_install` installs hooks and MCP entry.
 */
export function installAction(status: HookStatus): string | null {
  if (status.state === "partial") return "Repair";
  if (status.state === "missing") {
    return status.mcp === "missing" ? "Install hooks and MCP" : "Install hooks";
  }
  if (status.state === "installed" && status.mcp === "missing") return "Add MCP server";
  return null;
}

/**
 * Offers to install the agent hooks and the `poimen mcp` server (`poimen integration
 * install`, one consent for both) once the poimen backend runs and an agent lacks them. "Not now" is remembered in the UI state; nothing
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
  private readonly _cli = signal<ReadonlySet<string>>(new Set());
  private readonly _installing = signal<string | null>(null);
  private readonly _errors = signal<Readonly<Record<string, string>>>({});
  private checked = false;

  /** Agents offered; empty when there is nothing to offer. */
  readonly missing = this._missing.asReadonly();
  /** The last hook state per agent, for the Server settings page. */
  readonly statuses = this._statuses.asReadonly();
  readonly visible = computed(() => this._missing().length > 0);
  readonly busy = this._busy.asReadonly();
  readonly error = this._error.asReadonly();
  /** Agents whose CLI `get_harnesses` reports available (read with the states). */
  readonly cli = this._cli.asReadonly();
  /** The agent being installed right now (the Server page); `null` when none. */
  readonly installing = this._installing.asReadonly();
  /** The last install error per agent (the Server page). */
  readonly errors = this._errors.asReadonly();

  constructor() {
    effect(() => {
      const status = this.store.backend();
      const ready = status?.state === "running" || status?.state === "started";
      if (ready) untracked(() => void this.check());
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
      // Only a CLI that is installed is offered (#769): its harness is reported available.
      const installed = await this.availableAgents();
      this._cli.set(installed);
      const offerable = statuses.filter((s) => installed.has(s.agent));
      const gone = missingHooks(offerable);
      const seen = this.ui.hooksSeen();
      // An agent that only lacks its MCP entry is offered until "Not now", never again after.
      this._missing.set(
        this.ui.hooksOfferDismissed()
          ? gone.filter((a) => seen.includes(a))
          : [...gone, ...missingMcp(offerable)],
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

  /** Agent kinds whose harness `get_harnesses` reports available; none when it cannot be read. */
  private async availableAgents(): Promise<Set<string>> {
    try {
      const raw = await this.bridge.invoke<unknown>("get_harnesses");
      return new Set(
        parseHarnesses(raw)
          .filter((h) => h.available)
          .map((h) => h.id),
      );
    } catch {
      return new Set();
    }
  }

  /** Installs the hooks of every offered agent; keeps the offer open with the error on failure. */
  async install(): Promise<void> {
    await this.installAgents([...this._missing()]);
  }

  /** Installs the hooks of one agent (the Server settings page). */
  async installAgent(agent: string): Promise<void> {
    if (this._busy()) return;
    this._installing.set(agent);
    this._errors.update((e) => Object.fromEntries(Object.entries(e).filter(([k]) => k !== agent)));
    try {
      await this.installAgents([agent], false);
    } finally {
      this._installing.set(null);
    }
  }

  private async installAgents(agents: string[], offer = true): Promise<void> {
    this._busy.set(true);
    this._error.set(null);
    try {
      for (const agent of agents) {
        await this.bridge.invoke("hooks_install", { agent });
      }
      this._missing.update((m) => m.filter((a) => !agents.includes(a)));
      await this.refresh();
    } catch (err) {
      if (offer) this._error.set(String(err));
      else this._errors.update((e) => ({ ...e, [agents[0]]: String(err) }));
    } finally {
      this._busy.set(false);
    }
  }

  /** "Not now": closes the offer; it returns only when installed hooks go missing. */
  dismiss(): void {
    this.ui.dismissHooksOffer();
    this.ui.setHooksSeen(
      [],
      missingHooks(this._statuses()).filter((a) => this._missing().includes(a)),
    );
    this._missing.set([]);
    this._error.set(null);
  }
}
