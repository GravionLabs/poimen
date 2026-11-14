import { Injectable, computed, effect, inject, signal, untracked } from "@angular/core";
import {
  applyStatus,
  mergeSnapshots,
  qualifySnapshot,
  toSnapshot,
  type AgentStatus,
  type Snapshot,
  type WireSnapshot,
} from "./snapshot";
import { Machines, type MachineStatus } from "./machines";
import { TauriBridge } from "./tauri";

/** Delay that collapses a burst of events of a machine into one re-read. */
export const REMOTE_REFRESH_DEBOUNCE_MS = 80;

/** The snapshot of the machines: the last one read of each, qualified, for the views. */
export function remoteSnapshot(
  machines: readonly MachineStatus[],
  read: ReadonlyMap<string, Snapshot>,
): Snapshot {
  return mergeSnapshots(
    { spaces: [], tabs: [], terminals: [] },
    machines.flatMap((m) => {
      const snapshot = read.get(m.name);
      // Not connected: the last known spaces stay, without any status that could ring.
      return snapshot ? [qualifySnapshot(m.name, snapshot, m.state !== "connected")] : [];
    }),
  );
}

/** Payload of the `machine:event` Tauri event: an event of a machine's server. */
export interface MachineEvent {
  machine: string;
  kind: string;
  data: { pane_id?: string; agent_status?: AgentStatus } | null;
}

/**
 * Spaces, tabs and terminals of the configured machines (#280). Each machine's server owns its
 * state: the snapshot is read through `machine_snapshot` when the machine connects and again,
 * debounced, after its events. A machine that drops keeps its last snapshot.
 */
@Injectable({ providedIn: "root" })
export class RemoteSpaces {
  private readonly bridge = inject(TauriBridge);
  private readonly machines = inject(Machines);
  private readonly read = signal<ReadonlyMap<string, Snapshot>>(new Map());
  private readonly timers = new Map<string, ReturnType<typeof setTimeout>>();
  private connected = new Set<string>();

  /** Spaces, tabs and terminals of every machine, ids qualified by the machine name. */
  readonly snapshot = computed(() => remoteSnapshot(this.machines.machines(), this.read()));

  constructor() {
    // A machine that connects (or connects again) is read at once.
    effect(() => {
      const now = new Set(
        this.machines
          .machines()
          .filter((m) => m.state === "connected")
          .map((m) => m.name),
      );
      untracked(() => {
        for (const name of now) if (!this.connected.has(name)) void this.refresh(name);
        this.connected = now;
        // A removed machine leaves.
        const names = new Set(this.machines.machines().map((m) => m.name));
        const kept = [...this.read()].filter(([name]) => names.has(name));
        if (kept.length !== this.read().size) this.read.set(new Map(kept));
      });
    });
  }

  async start(): Promise<void> {
    await this.bridge.listen<MachineEvent>("machine:event", (event) => this.handle(event));
  }

  /** Reads the snapshot of `name`; a failure keeps the last one. */
  async refresh(name: string): Promise<void> {
    try {
      const wire = await this.bridge.invoke<WireSnapshot>("machine_snapshot", { name });
      this.read.update((m) => new Map(m).set(name, toSnapshot(wire)));
    } catch {
      // not connected or the read failed: the last known spaces stay
    }
  }

  private handle(event: MachineEvent): void {
    const { machine, kind, data } = event;
    if (kind === "pane.agent_status_changed" && data?.pane_id && data.agent_status) {
      const status = data.agent_status;
      const pane = data.pane_id;
      this.read.update((m) => {
        const snapshot = m.get(machine);
        return snapshot ? new Map(m).set(machine, applyStatus(snapshot, pane, status)) : m;
      });
    }
    clearTimeout(this.timers.get(machine));
    this.timers.set(
      machine,
      setTimeout(() => {
        this.timers.delete(machine);
        void this.refresh(machine);
      }, REMOTE_REFRESH_DEBOUNCE_MS),
    );
  }
}
