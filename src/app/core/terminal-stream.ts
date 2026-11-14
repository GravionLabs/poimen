import { Injectable, inject } from "@angular/core";
import type { Mode, OpenedStream, StreamRecord } from "./herdr";
import { TauriBridge } from "./tauri";

export interface OpenTerminal {
  mode: Mode;
  /** The backend's terminal id (herdr: the pane id). */
  target: string;
  cols: number;
  rows: number;
  /** Replaces the current controller (control mode only). */
  takeover: boolean;
  /** The machine whose backend runs the terminal (#280); absent for this Mac. */
  machine?: string;
}

/** A mouse command for a rendered stream; `modifiers` combines Shift = 1, Ctrl = 2, Alt = 4. */
export interface MouseCommand {
  action: "down" | "up" | "drag" | "move";
  button: "left" | "middle" | "right";
  column: number;
  row: number;
  modifiers: number;
}

/**
 * One live terminal stream. Closing it ends the stream only (for herdr, the CLI child process); the
 * terminal keeps running.
 */
export class TerminalSession {
  private closed = false;

  constructor(
    readonly id: number,
    /**
     * Raw PTY bytes: xterm.js tracks the terminal modes, so the view sends no mouse and wheel
     * commands. Otherwise the stream is rendered (finished frames) and the view forwards them.
     */
    readonly emulated: boolean,
    private readonly bridge: TauriBridge,
  ) {}

  /** `false` once the stream ended (closed record or {@link close}). */
  get open(): boolean {
    return !this.closed;
  }

  input(text: string): void {
    if (this.closed) return;
    void this.bridge.invoke("terminal_input", { id: this.id, text });
  }

  /** Resolves when the server set the size; rejects when it refused it (`not_controller`). */
  resize(cols: number, rows: number): Promise<void> {
    if (this.closed) return Promise.resolve();
    return this.bridge.invoke<void>("terminal_resize", { id: this.id, cols, rows });
  }

  scroll(up: boolean, lines: number): void {
    if (this.closed) return;
    void this.bridge.invoke("terminal_scroll", { id: this.id, up, lines });
  }

  /** One mouse event at a zero-based cell; herdr drops it unless the pane app tracks the mouse. */
  mouse(event: MouseCommand): void {
    if (this.closed) return;
    void this.bridge.invoke("terminal_mouse", { id: this.id, ...event });
  }

  /** Stops the stream. Safe to call twice or after the backend closed it. */
  close(): void {
    if (this.closed) return;
    this.ended();
    void this.bridge.invoke("close_terminal", { id: this.id });
  }

  /** The backend ended the stream: there is nothing left to close. */
  ended(): void {
    this.closed = true;
  }
}

/** What arrives on the channel of a stream: a JSON record, or raw PTY bytes of an emulated stream. */
type ChannelMessage = StreamRecord | ArrayBuffer | ArrayBufferView;

/** The record for a message of the channel. */
export function recordOf(message: ChannelMessage): StreamRecord {
  if (message instanceof ArrayBuffer) return { type: "bytes", data: new Uint8Array(message) };
  if (ArrayBuffer.isView(message)) {
    const { buffer, byteOffset, byteLength } = message;
    return { type: "bytes", data: new Uint8Array(buffer, byteOffset, byteLength) };
  }
  return message;
}

/**
 * Opens terminal streams. Each stream has a channel of its own that the backend pushes its records
 * through (raw bytes as an `ArrayBuffer` on an emulated stream), so there is no global event to
 * route and nothing to hold back for a stream whose id is not known yet: the view that opened the
 * stream gets its records from the first one on.
 */
@Injectable({ providedIn: "root" })
export class TerminalStreamService {
  private readonly bridge = inject(TauriBridge);

  /**
   * Opens a stream; `onRecord` gets its records and, last, its closed record (possibly before this
   * call returns). Rejects with the backend's error (for example, the terminal does not exist).
   */
  async open(
    options: OpenTerminal,
    onRecord: (record: StreamRecord) => void,
  ): Promise<TerminalSession> {
    // The session exists only once `open_terminal` answered; a closed record may come before.
    const state: { session?: TerminalSession; closed: boolean } = { closed: false };
    const channel = this.bridge.channel<ChannelMessage>((message) => {
      const record = recordOf(message);
      if (record.type === "closed") {
        state.closed = true;
        state.session?.ended();
      }
      onRecord(record);
    });
    const opened = await this.bridge.invoke<OpenedStream>("open_terminal", {
      ...options,
      onRecord: channel,
    });
    const session = new TerminalSession(opened.id, opened.emulated, this.bridge);
    state.session = session;
    if (state.closed) session.ended();
    return session;
  }
}
