import { Injectable, inject, signal } from "@angular/core";
import { splitId } from "./snapshot";
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

/**
 * Why an action on `id` cannot run (#693): `id` is qualified by a machine (`<machine>::<id>`) that
 * is not connected. `null` for a local id and for a connected machine. The action is disabled with
 * this reason and never sent anywhere else.
 */
export function machineBlock(machines: readonly MachineStatus[], id: string): string | null {
  const name = splitId(id).machine;
  if (name === undefined) return null;
  const machine = machines.find((m) => m.name === name);
  if (!machine) return `${name} is not a configured machine`;
  switch (machine.state) {
    case "connected":
      return null;
    case "connecting":
      return `${name} is connecting`;
    case "offline":
      return `${name} is offline${machine.error ? `: ${machine.error}` : ""}`;
    case "incompatible":
      return `${name} runs an incompatible server`;
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

/** A machine state as a tag: text, tone and a glyph (never color alone). */
export interface MachineTag {
  label: string;
  severity: "success" | "warn" | "danger" | "secondary";
  icon: string;
}

/** The state tag of a machine: connecting, connected with the version, offline, incompatible. */
export function machineTag(m: MachineStatus): MachineTag {
  switch (m.state) {
    case "connecting":
      return { label: "connecting…", severity: "secondary", icon: "…" };
    case "connected":
      return {
        label: m.serverVersion ? `connected, server ${m.serverVersion}` : "connected",
        severity: "success",
        icon: "✓",
      };
    case "offline":
      return {
        label: m.error ? `offline: ${m.error}` : "offline",
        severity: "danger",
        icon: "✕",
      };
    case "incompatible":
      return {
        label: `incompatible: server ${m.serverVersion} (protocol ${m.serverProtocol}), app ${m.appVersion} (protocol ${m.appProtocol})`,
        severity: "warn",
        icon: "!",
      };
  }
}

/**
 * What to do about a machine that is not connected, from its state: unreachable, unknown host
 * key, server missing or server incompatible. `null` for a machine that is connected or
 * connecting. The text is advice; poimen runs nothing.
 */
export function machineGuidance(m: MachineStatus): string | null {
  if (m.state === "incompatible") {
    const older = m.serverProtocol < m.appProtocol;
    return older
      ? `The server on ${m.name} is older than this app. Update poimen-server on the machine to the same version, then press Reconnect.`
      : `The server on ${m.name} is newer than this app. Update poimen, then press Reconnect.`;
  }
  if (m.state !== "offline") return null;
  const e = m.error.toLowerCase();
  if (e.includes("host key verification failed") || e.includes("host key"))
    return `The host key of ${m.ssh} is not known or has changed. Run "ssh ${m.ssh}" once in a terminal, check the fingerprint and accept it, then press Reconnect.`;
  if (
    e.includes("command not found") ||
    e.includes("not found") ||
    e.includes("no such file") ||
    e.includes("poimen-server")
  )
    return `ssh works, but poimen-server is not installed on ${m.name} or not on its PATH. Install it on the machine, then press Reconnect.`;
  if (e.includes("permission denied"))
    return `ssh refused the login to ${m.ssh}. poimen needs a key or the agent, no password prompt; check "ssh ${m.ssh}" in a terminal, then press Reconnect.`;
  return `${m.name} cannot be reached. Check that "ssh ${m.ssh}" works without a prompt (network, VPN, host name), then press Reconnect.`;
}

/** The glyph of a check status. */
export const checkIcon = (status: CheckStatus): string =>
  status === "ok" ? "✓" : status === "warning" ? "!" : "✕";

/** A validation error of `machine_save`, for the field it names (`null`: the whole write). */
export interface MachineFieldError {
  field: "name" | "ssh" | "reposDir" | null;
  message: string;
}

/** The default `reposDir` of a machine without one (`machines.rs`, `DEFAULT_REPOS_DIR`). */
export const DEFAULT_REPOS_DIR = "~/Source/{owner}/{repo}";

/** The field errors of a failed `machine_save`; anything else is one error for the whole form. */
export function parseFieldErrors(raw: unknown): MachineFieldError[] {
  if (Array.isArray(raw)) {
    const errors = raw.flatMap((e): MachineFieldError[] => {
      if (!e || typeof e !== "object") return [];
      const o = e as Record<string, unknown>;
      if (typeof o["message"] !== "string") return [];
      const f = o["field"];
      return [
        {
          field: f === "name" || f === "ssh" || f === "reposDir" ? f : null,
          message: o["message"],
        },
      ];
    });
    if (errors.length) return errors;
  }
  return [{ field: null, message: String(raw) }];
}

/** The machines of `machines[]` that the settings could not load: warnings of the config. */
export const machineWarnings = (warnings: readonly string[]): string[] =>
  warnings.filter((w) => w.startsWith("machines"));

/** The configured machines and their connection state (#279). */
@Injectable({ providedIn: "root" })
export class Machines {
  private readonly bridge = inject(TauriBridge);
  private readonly _machines = signal<readonly MachineStatus[]>([]);
  readonly machines = this._machines.asReadonly();
  private readonly _checks = signal<Readonly<Record<string, MachineCheck | { error: string }>>>({});
  /** The last check per machine name: the result, or the error when it could not run. */
  readonly checks = this._checks.asReadonly();
  /** The form of the Machines page: `original` is the machine being edited, `null` for a new one. */
  private readonly _form = signal<{ original: string | null } | null>(null);
  readonly form = this._form.asReadonly();
  private readonly _running = signal<ReadonlySet<string>>(new Set());
  /** Names of the machines whose check is running now. */
  readonly running = this._running.asReadonly();
  /** Machines checked in this session: the first connect checks, a reconnect does not. */
  private readonly checked = new Set<string>();

  /** Why an action on `id` is blocked (see {@link machineBlock}); `null` when it can run. */
  blockedReason(id: string): string | null {
    return machineBlock(this._machines(), id);
  }

  async start(): Promise<void> {
    await this.bridge.listen<unknown>("machine:state_changed", (raw) => this.update(raw));
    await this.refresh();
  }

  /** Reads `list_machines` again (the machines follow the settings). */
  async refresh(): Promise<void> {
    try {
      const raw = await this.bridge.invoke<unknown>("list_machines");
      const list = Array.isArray(raw) ? raw : [];
      const machines = list.map(parseMachineStatus).filter((m) => m !== null);
      this._machines.set(machines);
      // A machine already connected when the list is read is checked now (first connect).
      for (const m of machines)
        if (m.state === "connected" && !this.checked.has(m.name)) void this.check(m.name);
    } catch {
      // keep the last list
    }
  }

  /** Opens the add form (or the edit form of `original`); the page shows it. */
  openForm(original: string | null = null): void {
    this._form.set({ original });
  }

  closeForm(): void {
    this._form.set(null);
  }

  /**
   * Adds a machine or edits `original` in `config.json`; the settings watcher connects it. Resolves
   * with the errors per field, or an empty list when it was saved.
   */
  async save(
    original: string | null,
    values: { name: string; ssh: string; reposDir: string },
  ): Promise<MachineFieldError[]> {
    try {
      await this.bridge.invoke("machine_save", {
        original,
        name: values.name,
        ssh: values.ssh,
        reposDir: values.reposDir,
      });
    } catch (e) {
      return parseFieldErrors(e);
    }
    // A new (or changed) machine is checked once when it connects, even when its name was
    // checked before.
    this.checked.delete(values.name);
    if (original) this.checked.delete(original);
    await this.refresh();
    return [];
  }

  /** Removes a machine from `config.json`; no terminal on the machine is ended. */
  async remove(name: string): Promise<string | null> {
    try {
      await this.bridge.invoke("machine_remove", { name });
    } catch (e) {
      return String(e);
    }
    await this.refresh();
    return null;
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
    if (this._running().has(name)) return;
    this._running.update((r) => new Set(r).add(name));
    try {
      const check = parseMachineCheck(await this.bridge.invoke<unknown>("machine_check", { name }));
      this._checks.update((c) => ({
        ...c,
        [name]: check ?? { error: "unreadable machine check" },
      }));
    } catch (e) {
      this._checks.update((c) => ({ ...c, [name]: { error: String(e) } }));
    } finally {
      this._running.update((r) => {
        const next = new Set(r);
        next.delete(name);
        return next;
      });
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
