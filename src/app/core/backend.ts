// Backend-neutral wire types of the terminal backends besides the snapshot (events, terminal
// streams, version and protocol), and the agent kinds a backend can start.

import type { BackendKind } from "./settings";
import type { AgentStatus } from "./snapshot";

/** Payload of the `herdr` Tauri event: a herdr event or a poimen connection event. */
export interface BackendEvent {
  kind: string;
  data: {
    pane_id?: string;
    agent_status?: AgentStatus;
    agent_message?: string;
    agent_message_at?: number;
    reason?: string;
  } | null;
}

/** How a terminal view attaches: as one of many observers or as the single controller. */
export type Mode = "observe" | "control";

/**
 * One record of a terminal stream (sent through the channel of `open_terminal`), the same
 * for every backend. `bytes` is base64.
 */
export type StreamRecord =
  | { type: "reset"; cols: number; rows: number; bytes: string }
  | { type: "output"; bytes: string }
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
  /** Raw PTY bytes of an emulated stream (an `ArrayBuffer` over the channel). */
  | { type: "bytes"; data: Uint8Array };

/** What `open_terminal` answers: the stream id and whether the stream is emulated. */
export interface OpenedStream {
  id: number;
  /** Raw PTY bytes (xterm.js tracks the modes) rather than finished frames. */
  emulated: boolean;
}

export function decodeBase64(text: string): Uint8Array {
  const binary = atob(text);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

/** The herdr version and protocol reported by `ping`; `supported` is the protocol poimen is written for. */
export interface BackendInfo {
  version: string;
  /** Absent when herdr reports a version but no protocol: nothing to check. */
  protocol?: number | null;
  supported: number;
}

/** `backend herdr <version>`, or `backend herdr ?` when unreachable or too old to report it; `backend` names another server. */
export function backendVersionLabel(
  info: BackendInfo | null | undefined,
  backend = "herdr",
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

/** The tooltip warning for a herdr protocol other than the supported one; empty when it matches or is unknown. */
export function protocolWarning(info: BackendInfo | null | undefined, backend = "herdr"): string {
  if (!info || info.protocol == null || info.protocol === info.supported) return "";
  return `${backend} protocol ${info.protocol}, poimen supports ${info.supported}`;
}

/** Agent kinds `agent.start` supports (`herdr agent start --help`). */
export const AGENT_KINDS = [
  "claude",
  "codex",
  "gemini",
  "opencode",
  "copilot",
  "cursor",
  "pi",
  "devin",
  "agy",
  "cline",
  "omp",
  "mastracode",
  "kimi",
  "kiro",
  "droid",
  "amp",
  "grok",
  "hermes",
  "kilo",
  "qodercli",
  "qwen",
  "letta",
  "maki",
  "muse",
] as const;

/** Agent kinds that report their state through the poimen hooks
 * (`poimen-server integration install claude|copilot`); `start_agent` waits for their
 * `SessionStart`, so any other kind would time out although it runs. */
export const HOOK_AGENT_KINDS = ["claude", "copilot"] as const;

/** The kinds the "Start agent" dialog offers on `backend`: herdr detects all of `AGENT_KINDS`,
 * the poimen backend only learns of the ones with hooks. */
export function agentKinds(backend: BackendKind): readonly string[] {
  return backend === "poimen" ? HOOK_AGENT_KINDS : AGENT_KINDS;
}
