import { Injectable, computed, inject, signal } from "@angular/core";
import { HerdrStore, type BackendStatus } from "./herdr-store";
import { TauriBridge } from "./tauri";

/** What the open confirmation will do: restart an outdated server, or stop it and quit. */
export type ServerAction = "restart" | "quit";

/** The notice for a running server that is not the bundled one; `null` when it is fine. */
export function serverNotice(status: BackendStatus | null): string | null {
  if (status?.state === "outdated") {
    return `poimen-server ${status.running ?? "?"} is running, ${status.bundled ?? "?"} is installed`;
  }
  if (status?.state === "incompatible") {
    const version = status.running ? ` ${status.running}` : "";
    const protocol = status.protocol === undefined ? "" : ` (protocol ${status.protocol})`;
    return `poimen-server${version}${protocol} is running and does not match this poimen`;
  }
  return null;
}

/** An error value as text. */
function errorText(error: unknown): string {
  if (typeof error === "string") return error;
  return error instanceof Error ? error.message : JSON.stringify(error);
}

/**
 * The outdated or incompatible poimen-server and the "Quit poimen and stop server" command.
 * Stopping the server ends every terminal, so nothing happens without the confirmation that names
 * them; "Later" keeps the old server for this app session (in memory only).
 */
@Injectable({ providedIn: "root" })
export class ServerRestart {
  private readonly bridge = inject(TauriBridge);
  private readonly store = inject(HerdrStore);

  private readonly _later = signal(false);
  private readonly _action = signal<ServerAction | null>(null);
  private readonly _busy = signal(false);
  private readonly _error = signal<string | null>(null);

  /** The open confirmation; `null` when none. */
  readonly action = this._action.asReadonly();
  readonly busy = this._busy.asReadonly();
  readonly error = this._error.asReadonly();

  /** The notice of the status bar; `null` when the server is fine or "Later" was chosen. */
  readonly notice = computed(() => (this._later() ? null : serverNotice(this.store.backend())));

  /** The terminals the stop would end, by name. */
  readonly terminals = computed(() =>
    this.store.snapshot().terminals.map((t) => t.label || t.terminal_id),
  );

  /** "Later": the old server stays and the notice is gone for this session. */
  later(): void {
    this._later.set(true);
  }

  askRestart(): void {
    this.open("restart");
  }

  askQuit(): void {
    this.open("quit");
  }

  cancel(): void {
    if (this._busy()) return;
    this._action.set(null);
    this._error.set(null);
  }

  /** Runs the confirmed action; an error keeps the confirmation open. */
  async confirm(): Promise<void> {
    const action = this._action();
    if (!action || this._busy()) return;
    this._busy.set(true);
    this._error.set(null);
    try {
      if (action === "quit") {
        await this.bridge.invoke("quit_and_stop_server");
        return;
      }
      const status = await this.bridge.invoke<BackendStatus | undefined>("restart_server");
      if (status?.state === "failed") {
        this._error.set(status.reason ?? "poimen-server could not be restarted");
        return;
      }
      this._action.set(null);
      // The new server answers: the status becomes `running` and the snapshot is read again.
      await this.store.ensureBackend();
    } catch (error) {
      this._error.set(errorText(error));
    } finally {
      this._busy.set(false);
    }
  }

  private open(action: ServerAction): void {
    this._error.set(null);
    this._action.set(action);
  }
}
