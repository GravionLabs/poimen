// Backend-neutral wire types of the terminal backends besides the snapshot (events, terminal
// streams, version and protocol), and the agent kinds a backend can start.

import type { AgentStatus } from "./snapshot";

/**
 * Payload of the `backend` Tauri event: a backend event or a poimen connection event. An agent
 * status change is `terminal.agent_status_changed` keyed by `terminal_id` .
 */
export interface BackendEvent {
  kind: string;
  data: {
    terminal_id?: string;
    agent_status?: AgentStatus;
    agent_message?: string;
    agent_message_at?: number;
    agent_progress?: number;
    agent_kind?: string;
    agent_tone?: string;
    agent_title?: string;
    reason?: string;
    awake?: boolean;
  } | null;
}

/** How a terminal view attaches: as one of many observers or as the single controller. */
export type Mode = "observe" | "control";

/**
 * One record of a terminal stream (sent through the channel of `open_terminal`). The snapshot and
 * the output follow a `reset` as raw bytes (`bytes`).
 */
export type StreamRecord =
  | { type: "reset"; cols: number; rows: number }
  | { type: "resized"; cols: number; rows: number }
  | { type: "controller"; attached: boolean }
  | {
      type: "closed";
      reason: string;
      exit_code?: number;
      /**
       * Set when the reason is more than text; `controller_held`: another client is the
       * controller; `exited`: the process ended (`exit_code` says how) and its last screen stays;
       * `lost`: the connection to the server ended (a restart or a crash), the terminal may come
       * back with the next server.
       */
      code?: "controller_held" | "exited" | "lost";
    }
  /** Raw PTY bytes (an `ArrayBuffer` over the channel). */
  | { type: "bytes"; data: Uint8Array };

/** What `open_terminal` answers: the stream id. */
export interface OpenedStream {
  id: number;
}

/** The server version and protocol reported by `ping`; `supported` is the protocol poimen is written for. */
export interface BackendInfo {
  version: string;
  /** Absent when the server reports a version but no protocol: nothing to check. */
  protocol?: number | null;
  supported: number;
}

/** `backend poimen <version>`, or `backend poimen ?` when unreachable or too old to report it. */
export function backendVersionLabel(
  info: BackendInfo | null | undefined,
  backend = "poimen",
): string {
  return info?.version ? `backend ${backend} ${info.version}` : `backend ${backend} ?`;
}

/** The release channel of the build (`app_channel`): `rc` is the "Poimen RC" app installed beside the stable one. */
export type AppChannel = "stable" | "rc";

/** The window title: `poimen`, `poimen RC` on rc, with ` (<n> waiting)` while agents wait. */
export function windowTitle(channel: AppChannel, waiting: number): string {
  const name = channel === "rc" ? "poimen RC" : "poimen";
  return waiting > 0 ? `${name} (${waiting} waiting)` : name;
}

/** `frontend poimen <version>`, or `frontend poimen ?` until the build version is known. */
export function appVersionLabel(version: string | null | undefined): string {
  return version ? `frontend poimen ${version}` : "frontend poimen ?";
}

/** The tooltip warning for a server protocol other than the supported one; empty when it matches or is unknown. */
export function protocolWarning(info: BackendInfo | null | undefined, backend = "poimen"): string {
  if (!info || info.protocol == null || info.protocol === info.supported) return "";
  return `${backend} protocol ${info.protocol}, poimen supports ${info.supported}`;
}

/**
 * The agent kinds that report their state through the poimen hooks
 * (`poimen-server integration install <kind>`, the server's `integration.rs`): the one place
 * on this side. A further kind is one entry here plus its mapper on the server. `start_agent`
 * waits for the hooks' `SessionStart`, so any other kind would time out although it runs.
 * `transcripts`: whether Output and Flow can follow its session files.
 */
export const HOOK_AGENTS = [
  { kind: "claude", name: "Claude Code", transcripts: true },
  { kind: "copilot", name: "GitHub Copilot CLI", transcripts: true },
  { kind: "gemini", name: "Gemini CLI", transcripts: false },
] as const;

export const HOOK_AGENT_KINDS: readonly string[] = HOOK_AGENTS.map((a) => a.kind);

/** Display name of an agent kind (the kind itself when it has no hooks). */
export function agentName(kind: string): string {
  return HOOK_AGENTS.find((a) => a.kind === kind)?.name ?? kind;
}

/** Whether poimen reads the transcripts of `kind` (Output panel, Flow tab). */
export function hasTranscripts(kind: string): boolean {
  return HOOK_AGENTS.some((a) => a.kind === kind && a.transcripts);
}

/** What poimen gives for an agent kind: `status`, `transcript`; a kind without hooks gives no status. */
export function agentSupport(kind: string): string {
  const a = HOOK_AGENTS.find((h) => h.kind === kind);
  if (!a) return "no status";
  return a.transcripts ? "status, transcript" : "status";
}

/**
 * The kinds the "Start agent" dialog offers: the hook kinds plus the other harnesses of the
 * registry (`harnessIds`), which are started without waiting for a status and show none.
 */
export function startableKinds(harnessIds: readonly string[] = []): readonly string[] {
  return [...HOOK_AGENT_KINDS, ...harnessIds.filter((id) => !HOOK_AGENT_KINDS.includes(id))];
}

/** Label suffix of an offered kind: a kind without hooks reports no status. */
export function statusNote(kind: string): string {
  return HOOK_AGENT_KINDS.includes(kind) ? "" : " (no status)";
}
