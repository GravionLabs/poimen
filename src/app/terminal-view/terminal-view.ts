import {
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  afterNextRender,
  effect,
  inject,
  input,
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
import { TerminalSession, TerminalStreamService } from "../core/terminal-stream";

const RESIZE_DEBOUNCE_MS = 150;

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
  readonly mode = input<Mode>("observe");
  /** Why the stream closed; `null` while it runs. */
  readonly banner = signal<string | null>(null);

  private readonly streams = inject(TerminalStreamService);
  private readonly body = viewChild.required<ElementRef<HTMLElement>>("body");

  private term?: Terminal;
  private fit?: FitAddon;
  private session?: TerminalSession;
  private resizeObserver?: ResizeObserver;
  private resizeTimer?: ReturnType<typeof setTimeout>;
  private size = { cols: 0, rows: 0 };
  private destroyed = false;

  constructor() {
    afterNextRender(() => void this.start(false));
    // A mode switch reopens the stream; a switch to input never takes over a controller by itself.
    let seen = this.mode();
    effect(() => {
      const mode = this.mode();
      if (mode === seen) return;
      seen = mode;
      untracked(() => {
        if (this.term) void this.restartStream(false);
      });
    });
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
      fontFamily: cssVar("--font-mono") || "Menlo, monospace",
      fontSize: 13,
      cursorBlink: true,
      allowProposedApi: true,
      theme: {
        background: cssVar("--term-bg"),
        foreground: cssVar("--term-fg"),
        cursor: cssVar("--term-fg"),
      },
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
    await this.openStream(takeover);
    this.term?.focus();
  }

  private async openStream(takeover: boolean): Promise<void> {
    this.banner.set(null);
    try {
      const session = await this.streams.open(
        {
          mode: this.mode(),
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
      this.term?.write(decodeBase64(record.bytes));
      return;
    }
    this.session = undefined;
    if (record.reason !== "detached") this.banner.set(record.reason);
  }

  private scheduleResize(): void {
    clearTimeout(this.resizeTimer);
    this.resizeTimer = setTimeout(() => {
      if (!this.term || !this.fit) return;
      this.fit.fit();
      const { cols, rows } = this.term;
      if (cols === this.size.cols && rows === this.size.rows) return;
      this.size = { cols, rows };
      if (!this.session?.open) return;
      if (this.mode() === "control") {
        this.session.resize(cols, rows);
      } else {
        // Observers have a fixed viewport; reopen with the new size.
        void this.restartStream(false);
      }
    }, RESIZE_DEBOUNCE_MS);
  }
}
