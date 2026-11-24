import { Injectable, computed, inject, signal } from "@angular/core";
import type { BackendEvent } from "./backend";
import { TauriBridge } from "./tauri";
import type { AgentWriteAction } from "./agent-confirm";

/** A space of a grant, as the server resolved it. */
export interface GrantSpace {
  id: string;
  name: string;
  path?: string;
}

/** A lead agent asks for a scoped write grant (`agent.grant_requested`, ADR 0018). */
export interface GrantRequest {
  id: string;
  leadName: string;
  plan: string;
  spaces: GrantSpace[];
  tools: AgentWriteAction[];
  maxAgents: number;
  /** Seconds the grant lasts once confirmed. */
  durationS: number;
}

/** An active grant (`agent.grants_changed`, `agent_grants`). */
export interface ActiveGrant {
  id: string;
  leadName: string;
  spaces: GrantSpace[];
  tools: AgentWriteAction[];
  maxAgents: number;
  /** Agents the lead started that are still running. */
  running: number;
  /** Milliseconds since the epoch. */
  expiresAt: number;
}

interface WireGrant {
  id?: string;
  lead_name?: string;
  spaces?: GrantSpace[];
  tools?: AgentWriteAction[];
  max_agents?: number;
  running?: number;
  expires_at?: number;
}

interface GrantData extends WireGrant {
  plan?: string;
  duration_s?: number;
  grants?: WireGrant[];
}

/** `agent.grant_requested` as a request; `null` when the event lacks what the dialog shows. */
export function toGrantRequest(data: GrantData | null): GrantRequest | null {
  if (!data?.id || !data.lead_name || !data.spaces?.length || !data.tools?.length) return null;
  return {
    id: data.id,
    leadName: data.lead_name,
    plan: data.plan ?? "",
    spaces: data.spaces,
    tools: data.tools,
    maxAgents: data.max_agents ?? 1,
    durationS: data.duration_s ?? 0,
  };
}

/** One grant of `agent.grants_changed`; `null` when it lacks an id. */
export function toActiveGrant(data: WireGrant): ActiveGrant | null {
  if (!data.id) return null;
  return {
    id: data.id,
    leadName: data.lead_name ?? "agent",
    spaces: data.spaces ?? [],
    tools: data.tools ?? [],
    maxAgents: data.max_agents ?? 0,
    running: data.running ?? 0,
    expiresAt: data.expires_at ?? 0,
  };
}

/** "8 hours", "90 minutes": how long a grant lasts. */
export function durationText(seconds: number): string {
  if (seconds >= 3600) {
    const hours = Math.round((seconds / 3600) * 10) / 10;
    return `${hours} ${hours === 1 ? "hour" : "hours"}`;
  }
  const minutes = Math.max(1, Math.round(seconds / 60));
  return `${minutes} ${minutes === 1 ? "minute" : "minutes"}`;
}

/**
 * The scoped write grants of lead agents (ADR 0018, #1211): the requests that wait for the user
 * and the active grants, which can be revoked. The server holds them (in memory, they end with
 * it); this only shows what it publishes. A request that times out is settled by the server
 * (`agent.grant_resolved`).
 */
@Injectable({ providedIn: "root" })
export class AgentGrants {
  private readonly bridge = inject(TauriBridge);

  private readonly queue = signal<readonly GrantRequest[]>([]);
  /** The request on screen: the oldest open one. */
  readonly current = computed(() => this.queue()[0] ?? null);
  readonly waiting = computed(() => this.queue().length);
  readonly active = signal<readonly ActiveGrant[]>([]);
  readonly error = signal<string | null>(null);

  /** Reads the active grants once; later changes come as events. */
  start(): void {
    void this.bridge
      .invoke<{ grants?: WireGrant[] }>("agent_grants")
      .then((r) => this.setActive(r?.grants ?? []))
      .catch(() => undefined);
  }

  /** Applies one backend event; the ones that are not about grants are ignored. */
  handle(event: BackendEvent): void {
    const data = event.data as GrantData | null;
    switch (event.kind) {
      case "agent.grant_requested": {
        const request = toGrantRequest(data);
        if (request && !this.queue().some((r) => r.id === request.id)) {
          this.queue.update((q) => [...q, request]);
        }
        return;
      }
      case "agent.grant_resolved":
        this.queue.update((q) => q.filter((r) => r.id !== data?.id));
        return;
      case "agent.grants_changed":
        this.setActive(data?.grants ?? []);
        return;
    }
  }

  private setActive(grants: readonly WireGrant[]): void {
    this.active.set(grants.map(toActiveGrant).filter((g): g is ActiveGrant => g !== null));
  }

  /**
   * The user's answer for the request on screen: no, or yes for the `spaces` that stay checked
   * and the `maxAgents` that stays (the server refuses anything wider than asked).
   */
  async answer(approve: boolean, spaces?: string[], maxAgents?: number): Promise<void> {
    const request = this.current();
    if (!request) return;
    this.error.set(null);
    try {
      await this.bridge.invoke("agent_grant_answer", {
        id: request.id,
        approve,
        spaces: approve ? spaces : undefined,
        maxAgents: approve ? maxAgents : undefined,
      });
    } catch (e) {
      // Settled already (timed out) or refused as wider than asked.
      this.error.set(typeof e === "string" ? e : String(e));
      if (approve) return;
    }
    this.queue.update((q) => q.filter((r) => r.id !== request.id));
  }

  /** Revokes a grant; the agents it started keep running (ADR 0018). */
  async revoke(id: string): Promise<void> {
    try {
      await this.bridge.invoke("agent_grant_revoke", { id });
    } catch (e) {
      this.error.set(typeof e === "string" ? e : String(e));
    }
  }
}
