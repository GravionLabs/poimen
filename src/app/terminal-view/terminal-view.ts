import {
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  afterNextRender,
  effect,
  inject,
  input,
  output,
  signal,
  untracked,
  viewChild,
  type OnDestroy,
} from "@angular/core";
import { HxButton } from "@gravionlabs/helix-ui";
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import { Terminal } from "@xterm/xterm";
import { decodeBase64, type Mode, type Pane, type StreamRecord } from "../core/herdr";
import { SettingsStore } from "../core/settings";
import { TerminalSession, TerminalStreamService } from "../core/terminal-stream";

const RESIZE_DEBOUNCE_MS = 150;

export interface ModeState {
  mode: Mode;
  blocked: string | null;
}

/**
 * Whether a close reason says another client holds the controller. herdr's wording for a held
 * terminal is "... already has an attached client; retry with --takeover" (found in the herdr
 * binary; the control path itself is not exercised). Anything else (pane gone, crash, spawn
 * failure) is a plain error, not a held controller.
 */
export function isControllerHeld(reason: string): boolean {
  return /already has an attached client|retry with --takeover|controlled by another client|controller taken over/i.test(
    reason,
  );
}

function cssVar(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

/**
 * The live terminal of the active tab. It exists only while its tab is visible: hiding the tab
 * destroys it, which closes the stream and disposes xterm; the herdr pane keeps running.
 */
@Component({
  selector: "app-terminal-view",
  imports: [HxButton],
  templateUrl: "./terminal-view.html",
  styleUrl: "./terminal-view.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { "[class.controlling]": "mode() === 'control'" },
})
export class TerminalView implements OnDestroy {
  readonly pane = input.required<Pane>();
  /** The mode the tab asks for; `control` is tried without a takeover and may fall back. */
  readonly wanted = input<Mode>("observe");
  /** The mode the stream really has. */
  readonly mode = signal<Mode>("observe");
  /** Why the tab is read-only although input was wanted (another client holds the controller). */
  readonly blocked = signal<string | null>(null);
  /** The real mode and the reason it is read-only, emitted on every change. */
  readonly modeChange = output<ModeState>();
  /** The terminal size in cells, emitted on every change. */
  readonly sized = output<{ cols: number; rows: number }>();
  /** Why the stream closed; `null` while it runs. */
  readonly banner = signal<string | null>(null);

  private readonly streams = inject(TerminalStreamService);
  private readonly prefs = inject(SettingsStore);
  private readonly body = viewChild.required<ElementRef<HTMLElement>>("body");

  private term?: Terminal;
  private fit?: FitAddon;
  private session?: TerminalSession;
  private resizeObserver?: ResizeObserver;
  private resizeTimer?: ReturnType<typeof setTimeout>;
  private size = { cols: 0, rows: 0 };
  private destroyed = false;
  private framed = false;
  /** Counts connects; a result of an older one is dropped. */
  private generation = 0;
  /** The mode of the stream being opened or running (what the CLI child was spawned with). */
  private attempt: Mode = "observe";

  constructor() {
    afterNextRender(() => void this.start(false));
    // Settings edits (and theme switches) apply to the open terminal without a restart.
    effect(() => {
      this.prefs.settings();
      untracked(() => {
        if (!this.term) return;
        Object.assign(this.term.options, this.look());
        // A new font changes the cell size, so cols/rows change without a body resize.
        this.applySize();
      });
    });
    // A change of the wanted mode reopens the stream; it never takes over a controller by itself.
    let seen = this.wanted();
    effect(() => {
      const wanted = this.wanted();
      if (wanted === seen) return;
      seen = wanted;
      untracked(() => {
        if (this.term) void this.restartStream(false);
      });
    });
  }

  /** Font and colors from the settings, over the theme tokens. */
  private look() {
    const { fontFamily, fontSize, colors } = this.prefs.settings();
    const foreground = colors?.foreground ?? cssVar("--term-fg");
    return {
      fontFamily: fontFamily ?? (cssVar("--font-mono") || "Menlo, monospace"),
      fontSize,
      theme: {
        background: colors?.background ?? cssVar("--term-bg"),
        foreground,
        cursor: foreground,
      },
    };
  }

  ngOnDestroy(): void {
    this.destroyed = true;
    this.stop();
  }

  reconnect(takeover: boolean): void {
    void this.restartStream(takeover);
  }

  private createTerminal(): void {
    const term = new Terminal({
      scrollback: 0, // herdr owns scrollback; wheel events are forwarded instead
      cursorBlink: true,
      allowProposedApi: true,
      ...this.look(),
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(this.body().nativeElement);
    try {
      term.loadAddon(new WebglAddon());
    } catch {
      // DOM renderer fallback
    }
    term.onData((data) => {
      if (this.mode() === "control") this.session?.input(data);
    });
    term.attachCustomWheelEventHandler((event) => {
      if (this.mode() === "control" && this.session?.open && event.deltaY !== 0) {
        const lines = Math.max(1, Math.min(10, Math.round(Math.abs(event.deltaY) / 30)));
        this.session.scroll(event.deltaY < 0, lines);
      }
      return false;
    });
    this.term = term;
    this.fit = fit;
    this.resizeObserver = new ResizeObserver(() => this.scheduleResize());
    this.resizeObserver.observe(this.body().nativeElement);
  }

  private async start(takeover: boolean): Promise<void> {
    if (this.destroyed) return;
    if (!this.term) this.createTerminal();
    this.fit!.fit();
    this.size = { cols: this.term!.cols, rows: this.term!.rows };
    this.sized.emit(this.size);
    await this.openStream(takeover);
    this.term?.focus();
  }

  private setState(mode: Mode, blocked: string | null): void {
    this.mode.set(mode);
    this.blocked.set(blocked);
    this.modeChange.emit({ mode, blocked });
  }

  /** Opens with `mode` (default: the wanted one). Input is reported only after the first frame. */
  private async openStream(
    takeover: boolean,
    mode: Mode = this.wanted(),
    keepBlocked = false,
  ): Promise<void> {
    this.banner.set(null);
    this.setState("observe", keepBlocked ? this.blocked() : null);
    await this.connect(mode, takeover);
  }

  private async connect(mode: Mode, takeover: boolean): Promise<void> {
    this.framed = false;
    this.attempt = mode;
    const generation = ++this.generation;
    let session: TerminalSession | undefined;
    // Records that arrive before open() returned are held until the session is assigned.
    const early: StreamRecord[] = [];
    try {
      session = await this.streams.open(
        {
          mode,
          target: this.pane().pane_id,
          cols: this.size.cols,
          rows: this.size.rows,
          takeover,
        },
        (record) => {
          if (session && session === this.session) this.onRecord(session, record);
          else early.push(record);
        },
      );
      // The tab was hidden or closed, or a newer restart superseded this open.
      if (this.destroyed || generation !== this.generation) {
        session.close();
        return;
      }
      this.session = session;
      for (const record of early.splice(0)) this.onRecord(session, record);
    } catch (err) {
      if (generation === this.generation) this.failed(mode, String(err));
    }
  }

  /** A stream ended or could not open: fall back to observing when control was attempted. */
  private failed(mode: Mode, reason: string, lost = false): void {
    if (mode !== "control" || this.destroyed) {
      this.banner.set(reason);
      return;
    }
    // Held by another client: stay usable as an observer; going back to input is an explicit
    // user action. Any other failure is shown as an error.
    const held = isControllerHeld(reason);
    this.setState("observe", held ? reason : null);
    void this.connect("observe", false);
    if (lost || !held) this.banner.set(reason);
  }

  private closeStream(): void {
    this.session?.close();
    this.session = undefined;
  }

  private async restartStream(takeover: boolean, mode?: Mode, keepBlocked = false): Promise<void> {
    this.closeStream();
    if (this.term) await this.openStream(takeover, mode, keepBlocked);
    else await this.start(takeover);
  }

  private stop(): void {
    this.closeStream();
    clearTimeout(this.resizeTimer);
    this.resizeObserver?.disconnect();
    this.resizeObserver = undefined;
    this.term?.dispose();
    this.term = undefined;
    this.fit = undefined;
  }

  private onRecord(session: TerminalSession, record: StreamRecord): void {
    if (session !== this.session) return;
    if (record.type === "frame") {
      if (!this.framed && this.attempt === "control") this.setState("control", null);
      this.framed = true;
      this.term?.write(decodeBase64(record.bytes));
      return;
    }
    this.session = undefined;
    if (record.reason === "detached") return;
    this.failed(this.attempt, record.reason, this.framed);
  }

  private scheduleResize(): void {
    clearTimeout(this.resizeTimer);
    this.resizeTimer = setTimeout(() => this.applySize(), RESIZE_DEBOUNCE_MS);
  }

  /** Refits, and when cols/rows changed tells the status bar and herdr (resize or reopen). */
  private applySize(): void {
    if (!this.term || !this.fit) return;
    this.fit.fit();
    const { cols, rows } = this.term;
    if (cols === this.size.cols && rows === this.size.rows) return;
    this.size = { cols, rows };
    this.sized.emit(this.size);
    if (!this.session?.open) return;
    if (this.attempt === "control") {
      this.session.resize(cols, rows);
    } else {
      // Observers have a fixed viewport; reopen as an observer, never retrying control.
      void this.restartStream(false, "observe", true);
    }
  }
}
