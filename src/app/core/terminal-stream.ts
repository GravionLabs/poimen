import { DestroyRef, Injectable, inject } from "@angular/core";
import type { Mode, StreamRecord } from "./herdr";
import { TauriBridge, type Unlisten } from "./tauri";

export interface OpenTerminal {
  mode: Mode;
  /** The herdr pane id. */
  target: string;
  cols: number;
  rows: number;
  /** Replaces the current controller (control mode only). */
  takeover: boolean;
}

/** A `terminal.mouse` command; `modifiers` combines Shift = 1, Ctrl = 2, Alt = 4. */
export interface MouseCommand {
  action: "down" | "up" | "drag" | "move";
  button: "left" | "middle" | "right";
  column: number;
  row: number;
  modifiers: number;
}

/** Payload of the `terminal` Tauri event. */
interface TerminalEvent {
  id: number;
  record: StreamRecord;
}

/**
 * One live `herdr terminal session observe|control` stream. Closing it ends the CLI child process
 * only; the herdr pane keeps running.
 */
export class TerminalSession {
  private closed = false;

  constructor(
    readonly id: number,
    private readonly bridge: TauriBridge,
    private readonly release: (id: number) => void,
  ) {}

  /** `false` once the stream ended (closed record or {@link close}). */
  get open(): boolean {
    return !this.closed;
  }

  input(text: string): void {
    if (this.closed) return;
    void this.bridge.invoke("terminal_input", { id: this.id, text });
  }

  resize(cols: number, rows: number): void {
    if (this.closed) return;
    void this.bridge.invoke("terminal_resize", { id: this.id, cols, rows });
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

  /** The backend ended the stream: forget the route, but there is nothing left to close. */
  ended(): void {
    this.closed = true;
    this.release(this.id);
  }
}

/**
 * Owns the `terminal` event subscription. The backend emits the records of every stream on the one
 * event; this service routes them by session id to the view that opened the stream and drops
 * records of ids it does not know (late frames of a stream that was closed).
 */
@Injectable({ providedIn: "root" })
export class TerminalStreamService {
  private readonly bridge = inject(TauriBridge);
  private readonly routes = new Map<number, (record: StreamRecord) => void>();
  /** Streams being opened: records of ids not routed yet are held for them. */
  private opening = 0;
  private readonly early = new Map<number, StreamRecord[]>();
  private readonly ready: Promise<Unlisten>;

  constructor() {
    this.ready = this.bridge.listen<TerminalEvent>("terminal", ({ id, record }) =>
      this.deliver(id, record),
    );
    inject(DestroyRef).onDestroy(() => {
      this.routes.clear();
      void this.ready.then((stop) => stop());
    });
  }

  private deliver(id: number, record: StreamRecord): void {
    const route = this.routes.get(id);
    if (route) route(record);
    else if (this.opening > 0) this.early.set(id, [...(this.early.get(id) ?? []), record]);
  }

  /** Number of streams that currently receive records. */
  get size(): number {
    return this.routes.size;
  }

  /**
   * Opens a stream; `onRecord` gets its frames and, last, its closed record. Rejects with the
   * backend's error (for example, another controller holds the terminal).
   */
  async open(
    options: OpenTerminal,
    onRecord: (record: StreamRecord) => void,
  ): Promise<TerminalSession> {
    await this.ready;
    this.opening++;
    let id: number;
    try {
      // The backend emits as soon as the child runs, possibly before this call resolves.
      id = await this.bridge.invoke<number>("open_terminal", { ...options });
    } catch (err) {
      this.opening--;
      if (this.opening === 0) this.early.clear();
      throw err;
    }
    this.opening--;
    const session = new TerminalSession(id, this.bridge, (gone) => this.routes.delete(gone));
    this.routes.set(id, (record) => {
      if (record.type === "closed") session.ended();
      onRecord(record);
    });
    const held = this.early.get(id) ?? [];
    this.early.delete(id);
    if (this.opening === 0) this.early.clear();
    for (const record of held) this.routes.get(id)?.(record);
    return session;
  }
}
