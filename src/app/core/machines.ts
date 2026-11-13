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

/** The outcome of one check of `machine_check`. `warning` is something optional (one agent). */
export type CheckStatus = "ok" | "missing" | "warning";

/** One check; `fix` is the command to run on the machine, which poimen only shows. */
export interface MachineCheckItem {
  id: string;
  status: CheckStatus;
  detail: string | null;
  fix: string | null;
}

/** The result of `machine.check` (#281). */
export interface MachineCheck {
  version: string;
  protocol: number;
  checks: MachineCheckItem[];
}

/** A machine check of the wire, or `null` when it is malformed; unknown statuses are dropped. */
export function parseMachineCheck(raw: unknown): MachineCheck | null {
  if (!raw || typeof raw !== "object") return null;
  const r = raw as Record<string, unknown>;
  if (!Array.isArray(r["checks"])) return null;
  const checks: MachineCheckItem[] = [];
  for (const c of r["checks"]) {
    if (!c || typeof c !== "object") continue;
    const o = c as Record<string, unknown>;
    const status = o["status"];
    if (typeof o["id"] !== "string") continue;
    if (status !== "ok" && status !== "missing" && status !== "warning") continue;
    checks.push({
      id: o["id"],
      status,
      detail: typeof o["detail"] === "string" ? o["detail"] : null,
      fix: typeof o["fix"] === "string" ? o["fix"] : null,
    });
  }
  return { version: str(r["version"]), protocol: num(r["protocol"]), checks };
}

/** No check is missing; warnings do not count. */
export const checkPassed = (check: MachineCheck): boolean =>
  check.checks.every((c) => c.status !== "missing");

/** The configured machines and their connection state (#279). */
@Injectable({ providedIn: "root" })
export class Machines {
  private readonly bridge = inject(TauriBridge);
  private readonly _machines = signal<readonly MachineStatus[]>([]);
  readonly machines = this._machines.asReadonly();
  private readonly _checks = signal<Readonly<Record<string, MachineCheck | { error: string }>>>({});
  /** The last check per machine name: the result, or the error when it could not run. */
  readonly checks = this._checks.asReadonly();
  /** Machines checked in this session: the first connect checks, a reconnect does not. */
  private readonly checked = new Set<string>();

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

  /**
   * Runs the read-only `machine.check` on a connected machine (from its context menu, or at its
   * first connect). poimen never runs the fixes it lists.
   */
  async check(name: string): Promise<void> {
    this.checked.add(name);
    try {
      const check = parseMachineCheck(await this.bridge.invoke<unknown>("machine_check", { name }));
      this._checks.update((c) => ({
        ...c,
        [name]: check ?? { error: "unreadable machine check" },
      }));
    } catch (e) {
      this._checks.update((c) => ({ ...c, [name]: { error: String(e) } }));
    }
  }

  private update(raw: unknown): void {
    const status = parseMachineStatus(raw);
    if (!status) return;
    const list = this._machines();
    const i = list.findIndex((m) => m.name === status.name);
    // A machine that is not listed yet (a settings change) comes with the next list.
    if (i < 0) void this.refresh();
    else this._machines.set(list.map((m, j) => (j === i ? status : m)));
    if (status.state === "connected" && !this.checked.has(status.name))
      void this.check(status.name);
  }
}
