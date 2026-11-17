import { Injectable, computed, inject, signal } from "@angular/core";
import { Machines, serverInstallOffer, type MachineStatus } from "./machines";
import { RemoteSpaces } from "./remote-spaces";
import { TauriBridge } from "./tauri";

/** One step of the plan, with the exact command that runs on the machine (`server_install.rs`). */
export interface InstallStep {
  id: string;
  label: string;
  command: string;
}

/** What the confirmation shows: the answer of `machine_install_plan`. */
export interface InstallPlan {
  machine: string;
  ssh: string;
  version: string;
  channel: "stable" | "rc";
  target: string;
  archive: string;
  origin: string;
  sha256: string;
  size: number;
  dir: string;
  server: string;
  host: string;
  existing: string | null;
  steps: InstallStep[];
}

export type StepStatus = "pending" | "running" | "ok" | "failed";

/** The progress of one step: `machine:install_step`. */
export interface StepProgress extends InstallStep {
  status: StepStatus;
  output: string;
}

export type InstallPhase = "planning" | "confirm" | "running" | "failed" | "done";

/** The open install or update dialog. */
export interface InstallState {
  machine: string;
  /** `update` when a server is already there (incompatible or on the PATH), else `install`. */
  kind: "install" | "update";
  /** The version of the server found on the machine (incompatible), when known. */
  found: string | null;
  phase: InstallPhase;
  plan: InstallPlan | null;
  steps: StepProgress[];
  error: string | null;
}

function parsePlan(raw: unknown): InstallPlan | null {
  if (!raw || typeof raw !== "object") return null;
  const r = raw as Record<string, unknown>;
  const s = (k: string): string => (typeof r[k] === "string" ? (r[k] as string) : "");
  const steps = Array.isArray(r["steps"])
    ? r["steps"].flatMap((x): InstallStep[] => {
        const o = x as Record<string, unknown> | null;
        return o && typeof o["id"] === "string" && typeof o["command"] === "string"
          ? [
              {
                id: o["id"],
                label: typeof o["label"] === "string" ? o["label"] : o["id"],
                command: o["command"],
              },
            ]
          : [];
      })
    : [];
  if (!steps.length) return null;
  return {
    machine: s("machine"),
    ssh: s("ssh"),
    version: s("version"),
    channel: r["channel"] === "rc" ? "rc" : "stable",
    target: s("target"),
    archive: s("archive"),
    origin: s("origin"),
    sha256: s("sha256"),
    size: typeof r["size"] === "number" ? r["size"] : 0,
    dir: s("dir"),
    server: s("server"),
    host: s("host"),
    existing: typeof r["existing"] === "string" ? r["existing"] : null,
    steps,
  };
}

/** The exact commands of a plan as one copyable block (what the dialog shows and Copy copies). */
export function planScript(plan: InstallPlan): string {
  return plan.steps
    .map((s) => (s.id === "copy" ? `${plan.archive} | ${s.command}` : s.command))
    .join("\n");
}

/** `1.2 MiB` for a byte count. */
export function sizeText(bytes: number): string {
  return bytes >= 1024 * 1024
    ? `${(bytes / 1024 / 1024).toFixed(1)} MiB`
    : `${Math.max(1, Math.round(bytes / 1024))} KiB`;
}

/**
 * Install or update `poimen-server` on a machine (ADR 0013, #865). Opening the dialog plans only
 * (read-only probe, download and checksum here); nothing runs on the machine before `confirm`.
 * Every install asks again; nothing is remembered.
 */
@Injectable({ providedIn: "root" })
export class ServerInstall {
  private readonly bridge = inject(TauriBridge);
  private readonly machines = inject(Machines);
  private readonly remote = inject(RemoteSpaces);
  private readonly _state = signal<InstallState | null>(null);
  readonly state = this._state.asReadonly();
  private listening: Promise<unknown> | null = null;

  /** Terminals of the machine from its last snapshot: those that keep running and those that do not survive a server restart. */
  readonly terminals = computed(() => {
    const name = this._state()?.machine;
    const all = this.remote.snapshot().terminals.filter((t) => t.machine === name);
    return {
      running: all.length,
      lost: all.filter((t) => t.survives_restart === false).map((t) => t.label || t.terminal_id),
    };
  });

  /** Opens the dialog for `m` (planning at once); `null` offer means nothing to do. */
  async open(m: MachineStatus): Promise<void> {
    const offer = serverInstallOffer(m);
    if (!offer || this._state()) return;
    this.listening ??= this.bridge.listen<unknown>("machine:install_step", (raw) => this.step(raw));
    await this.listening;
    this._state.set({
      machine: m.name,
      kind: offer,
      found: m.state === "incompatible" ? m.serverVersion : null,
      phase: "planning",
      plan: null,
      steps: [],
      error: null,
    });
    try {
      const plan = parsePlan(
        await this.bridge.invoke<unknown>("machine_install_plan", { name: m.name }),
      );
      if (!this._state()) return;
      if (!plan) throw new Error("unreadable install plan");
      this.patch({
        phase: "confirm",
        plan,
        kind: plan.existing || m.state === "incompatible" ? "update" : "install",
        steps: plan.steps.map((s) => ({ ...s, status: "pending", output: "" })),
      });
    } catch (e) {
      this.patch({ phase: "failed", error: String(e) });
    }
  }

  /** The confirmation: the only place that starts anything on the machine. */
  async confirm(): Promise<void> {
    const s = this._state();
    if (!s || s.phase !== "confirm") return;
    this.patch({ phase: "running", error: null });
    try {
      await this.bridge.invoke("machine_install_run", { name: s.machine });
      this.patch({ phase: "done" });
      // The machine connects again; the first connect runs the check.
      this.machines.forgetCheck(s.machine);
    } catch (e) {
      this.patch({ phase: "failed", error: String(e) });
    }
  }

  /** Closes the dialog; before the confirmation nothing was written on the machine. */
  async cancel(): Promise<void> {
    const s = this._state();
    if (!s || s.phase === "running") return;
    this._state.set(null);
    if (s.phase === "confirm" || s.phase === "planning") {
      try {
        await this.bridge.invoke("machine_install_cancel", { name: s.machine });
      } catch {
        // nothing to drop
      }
    }
  }

  private step(raw: unknown): void {
    const r = raw as { machine?: string; step?: string; status?: StepStatus; output?: string };
    const s = this._state();
    if (!s || r?.machine !== s.machine || !r.step || !r.status) return;
    this.patch({
      steps: s.steps.map((x) =>
        x.id === r.step ? { ...x, status: r.status!, output: r.output ?? "" } : x,
      ),
    });
  }

  private patch(p: Partial<InstallState>): void {
    this._state.update((s) => (s ? { ...s, ...p } : s));
  }
}
