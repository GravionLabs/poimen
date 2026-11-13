import { Injectable, inject, signal } from "@angular/core";
import { parseMachine, type Machine } from "./settings";
import { TauriBridge } from "./tauri";

/** The connection state of a machine (wire shape of `machines.rs`, `State`). */
export type MachineState =
  | { state: "connecting" }
  | { state: "connected"; serverVersion: string }
  | { state: "offline"; error: string }
  | {
      state: "incompatible";
      serverVersion: string;
      serverProtocol: number;
      appVersion: string;
      appProtocol: number;
    };

/** A machine with its state: an entry of `list_machines` and of `machine:state_changed`. */
export type MachineStatus = Machine & MachineState;

const str = (v: unknown): string => (typeof v === "string" ? v : "");
const num = (v: unknown): number => (typeof v === "number" ? v : 0);

/** A machine status of the wire, or `null` when it is malformed. */
export function parseMachineStatus(raw: unknown): MachineStatus | null {
  const machine = parseMachine(raw);
  if (!machine) return null;
  const r = raw as Record<string, unknown>;
  switch (r["state"]) {
    case "connecting":
      return { ...machine, state: "connecting" };
    case "connected":
      return { ...machine, state: "connected", serverVersion: str(r["serverVersion"]) };
    case "offline":
      return { ...machine, state: "offline", error: str(r["error"]) };
    case "incompatible":
      return {
        ...machine,
        state: "incompatible",
        serverVersion: str(r["serverVersion"]),
        serverProtocol: num(r["serverProtocol"]),
        appVersion: str(r["appVersion"]),
        appProtocol: num(r["appProtocol"]),
      };
    default:
      return null;
  }
}

/** The configured machines and their connection state (#279). */
@Injectable({ providedIn: "root" })
export class Machines {
  private readonly bridge = inject(TauriBridge);
  private readonly _machines = signal<readonly MachineStatus[]>([]);
  readonly machines = this._machines.asReadonly();

  async start(): Promise<void> {
    await this.bridge.listen<unknown>("machine:state_changed", (raw) => this.update(raw));
    await this.refresh();
  }

  /** Reads `list_machines` again (the machines follow the settings). */
  async refresh(): Promise<void> {
    try {
      const raw = await this.bridge.invoke<unknown>("list_machines");
      const list = Array.isArray(raw) ? raw : [];
      this._machines.set(list.map(parseMachineStatus).filter((m) => m !== null));
    } catch {
      // keep the last list
    }
  }

  /** Asks an offline or incompatible machine to connect at once. */
  async reconnect(name: string): Promise<void> {
    await this.bridge.invoke("reconnect_machine", { name });
  }

  private update(raw: unknown): void {
    const status = parseMachineStatus(raw);
    if (!status) return;
    const list = this._machines();
    const i = list.findIndex((m) => m.name === status.name);
    // A machine that is not listed yet (a settings change) comes with the next list.
    if (i < 0) void this.refresh();
    else this._machines.set(list.map((m, j) => (j === i ? status : m)));
  }
}
