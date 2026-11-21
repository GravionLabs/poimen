import { Injectable, inject, signal } from "@angular/core";
import { HOOK_AGENT_KINDS } from "./backend";
import { Machines } from "./machines";
import { parseHarnesses, type HarnessInfo } from "./settings";
import { paneDir, spaceRoot, splitId, type Terminal } from "./snapshot";
import { SpaceActions } from "./space-actions";
import { SpacesStore } from "./spaces-store";
import { TauriBridge } from "./tauri";

/** An earlier agent session of a directory (`list_agent_sessions`, wire shape of `agent_sessions.rs`). */
export interface AgentSessionInfo {
  agent: string;
  session: string;
  started: string | null;
  lastActivity: string | null;
  firstPrompt: string;
}

/** Where a picked session starts: a new tab of a space, or the shell pane at its prompt. */
export type ResumeTarget = { kind: "space"; spaceId: string } | { kind: "pane"; paneId: string };

/** The open "Resume session..." dialog. */
export interface ResumeDialog {
  target: ResumeTarget;
  dir: string;
  /** The sessions are being read. */
  loading: boolean;
  sessions: AgentSessionInfo[];
  error: string | null;
}

/** The reason remote machines are not offered (#986). */
export const RESUME_REMOTE_REASON = "Resuming a session is not available on remote machines yet.";

/** Why resuming is not offered for the space or pane `id` (remote); `null` for a local one. */
export function resumeUnavailable(id: string): string | null {
  return splitId(id).machine !== undefined ? RESUME_REMOTE_REASON : null;
}

/** The sessions of a `list_agent_sessions` answer; malformed entries are skipped. */
export function parseSessions(raw: unknown): AgentSessionInfo[] {
  if (!Array.isArray(raw)) return [];
  const text = (v: unknown): string | null => (typeof v === "string" && v ? v : null);
  return raw.flatMap((item): AgentSessionInfo[] => {
    const r = typeof item === "object" && item !== null ? (item as Record<string, unknown>) : {};
    const agent = text(r["agent"]);
    const session = text(r["session"]);
    if (!agent || !session) return [];
    return [
      {
        agent,
        session,
        started: text(r["started"]),
        lastActivity: text(r["lastActivity"]),
        firstPrompt: text(r["firstPrompt"]) ?? "",
      },
    ];
  });
}

/**
 * The program and arguments that resume `session` in `harness`: the harness's resume template with
 * `{session}` replaced per element. A hook kind runs under its id, as the "New pane" dialog does.
 * `null` for a harness without `resumeArgs` or a program that was not found.
 */
export function resumeCommand(
  harness: HarnessInfo | undefined,
  session: string,
): { kind: string; args: string[] } | null {
  if (!harness || !harness.available || harness.resumeArgs.length === 0) return null;
  const resume = harness.resumeArgs.map((a) => a.replaceAll("{session}", session));
  if (HOOK_AGENT_KINDS.includes(harness.id) || harness.command.length === 0) {
    return { kind: harness.id, args: resume };
  }
  const [program = harness.id, ...base] = harness.command;
  return { kind: program, args: [...base, ...resume] };
}

/**
 * "Resume session..." (#986): lists the earlier sessions of the directory of a space or a shell
 * pane and starts the harness with its `resumeArgs` for the picked one, in a new tab of the space
 * or in the pane at its prompt. Remote machines are not supported yet.
 */
@Injectable({ providedIn: "root" })
export class SessionResume {
  private readonly bridge = inject(TauriBridge);
  private readonly store = inject(SpacesStore);
  private readonly toast = inject(SpaceActions);
  private readonly machines = inject(Machines, { optional: true });

  private readonly _dialog = signal<ResumeDialog | null>(null);
  readonly dialog = this._dialog.asReadonly();
  private harnesses: HarnessInfo[] = [];
  /** Guards a late answer of a dialog that was closed or reopened. */
  private generation = 0;

  /** Why resuming is not offered for the space or pane `id`; `null` when it is. */
  unavailable(id: string): string | null {
    return resumeUnavailable(id) ?? this.machines?.blockedReason(id) ?? null;
  }

  /** The directory sessions are listed for: the space's root pane, or the pane itself. */
  dirOf(target: ResumeTarget): string | undefined {
    const pane =
      target.kind === "pane"
        ? this.store.pane(target.paneId)
        : spaceRoot(this.store.snapshot(), target.spaceId);
    return pane && paneDir(pane);
  }

  /** The palette entry: the sessions of the active pane's directory. */
  openActive(paneId: string | null): void {
    if (paneId) void this.open({ kind: "pane", paneId });
    else this.toast.notify("Resume session: there is no active pane");
  }

  /** Opens the dialog and reads the sessions. */
  async open(target: ResumeTarget): Promise<void> {
    const id = target.kind === "pane" ? target.paneId : target.spaceId;
    const reason = this.unavailable(id);
    if (reason) {
      this.toast.notify(reason);
      return;
    }
    const dir = this.dirOf(target);
    if (!dir) {
      this.toast.notify("Resume session: the directory is unknown.");
      return;
    }
    const generation = ++this.generation;
    this._dialog.set({ target, dir, loading: true, sessions: [], error: null });
    try {
      const [harnesses, sessions] = await Promise.all([
        this.bridge.invoke<unknown>("get_harnesses").then(parseHarnesses),
        this.bridge.invoke<unknown>("list_agent_sessions", { dir }).then(parseSessions),
      ]);
      if (generation !== this.generation) return;
      this.harnesses = harnesses;
      // A harness without `resumeArgs` (or not installed) is not offered.
      const offered = sessions.filter(
        (s) =>
          resumeCommand(
            harnesses.find((h) => h.id === s.agent),
            s.session,
          ) !== null,
      );
      this.patch({ loading: false, sessions: offered });
    } catch (err) {
      if (generation === this.generation) this.patch({ loading: false, error: String(err) });
    }
  }

  cancel(): void {
    this.generation++;
    this._dialog.set(null);
  }

  /** Starts the harness for `session`; resolves to the new pane when a new tab was created. */
  async pick(session: AgentSessionInfo): Promise<Terminal | undefined> {
    const dialog = this._dialog();
    if (!dialog) return undefined;
    const command = resumeCommand(
      this.harnesses.find((h) => h.id === session.agent),
      session.session,
    );
    if (!command) {
      this.patch({ error: `${session.agent} cannot resume a session.` });
      return undefined;
    }
    this._dialog.set(null);
    this.generation++;
    const { target } = dialog;
    try {
      let terminalId: string | null;
      if (target.kind === "pane") terminalId = target.paneId;
      else {
        terminalId = await this.bridge.invoke<string | null>("terminal_create", {
          spaceId: target.spaceId,
          cwd: dialog.dir,
        });
      }
      if (!terminalId) return undefined;
      await this.bridge.invoke("agent_start", {
        terminalId,
        kind: command.kind,
        name: null,
        args: command.args,
      });
      await this.store.refresh();
      return target.kind === "space" ? this.store.pane(terminalId) : undefined;
    } catch (err) {
      this.toast.fail(`Cannot resume the session: ${String(err)}`);
      await this.store.refresh();
      return undefined;
    }
  }

  private patch(change: Partial<ResumeDialog>): void {
    this._dialog.update((d) => (d ? { ...d, ...change } : d));
  }
}
