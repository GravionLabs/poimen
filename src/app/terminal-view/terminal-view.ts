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

  private async openStream(takeover: boolean): Promise<void> {
    this.banner.set(null);
    const mode = this.wanted();
    this.setState(mode, null);
    await this.connect(mode, takeover);
  }

  private async connect(mode: Mode, takeover: boolean): Promise<void> {
    this.framed = false;
    try {
      const session = await this.streams.open(
        {
          mode,
          target: this.pane().pane_id,
          cols: this.size.cols,
          rows: this.size.rows,
          takeover,
        },
        (record) => this.onRecord(session, record),
      );
      // The tab was hidden or closed while the stream opened.
      if (this.destroyed) {
        session.close();
        return;
      }
      this.session = session;
    } catch (err) {
      this.banner.set(String(err));
    }
  }

  private closeStream(): void {
    this.session?.close();
    this.session = undefined;
  }

  private async restartStream(takeover: boolean): Promise<void> {
    this.closeStream();
    if (this.term) await this.openStream(takeover);
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
      this.framed = true;
      this.term?.write(decodeBase64(record.bytes));
      return;
    }
    this.session = undefined;
    if (record.reason === "detached") return;
    if (this.mode() === "control") {
      // Control failed (another client holds the controller) or was lost to another client:
      // stay usable as an observer; going back to input is an explicit user action.
      const lost = this.framed;
      this.setState("observe", record.reason);
      if (lost) this.banner.set(record.reason);
      void this.connect("observe", false);
      return;
    }
    this.banner.set(record.reason);
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
    if (this.mode() === "control") {
      this.session.resize(cols, rows);
    } else {
      // Observers have a fixed viewport; reopen with the new size.
      void this.restartStream(false);
    }
  }
}
