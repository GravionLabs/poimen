import {
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  afterNextRender,
  computed,
  inject,
  input,
  output,
  signal,
  viewChild,
  type OnDestroy,
} from "@angular/core";
import { HxButton } from "@gravionlabs/helix-ui";
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import { Terminal } from "@xterm/xterm";
import { decodeBase64, paneLabel, type Mode, type Pane, type StreamRecord } from "../core/herdr";
import { TerminalSession, TerminalStreamService } from "../core/terminal-stream";

const RESIZE_DEBOUNCE_MS = 150;

function cssVar(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

/**
 * One open pane: a collapsible card. Expanded = live herdr stream in xterm.js. Collapsed = stream
 * closed and terminal disposed; the herdr pane keeps running.
 */
@Component({
  selector: "app-terminal-view",
  imports: [HxButton],
  templateUrl: "./terminal-view.html",
  styleUrl: "./terminal-view.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: {
    class: "view",
    "[class.collapsed]": "collapsed()",
    "[class.controlling]": "mode() === 'control'",
  },
})
export class TerminalView implements OnDestroy {
  readonly pane = input.required<Pane>();
  /** The user closed the view (the herdr pane keeps running). */
  readonly closed = output<void>();

  readonly collapsed = signal(false);
  readonly mode = signal<Mode>("observe");
  /** Why the stream closed; `null` while it runs. */
  readonly banner = signal<string | null>(null);
  protected readonly label = computed(() => paneLabel(this.pane()));

  private readonly streams = inject(TerminalStreamService);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
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
  }

  ngOnDestroy(): void {
    this.destroyed = true;
    this.stop();
  }

  toggle(): void {
    if (this.collapsed()) this.expand();
    else this.collapse();
  }

  expand(): void {
    if (!this.collapsed()) return;
    this.collapsed.set(false);
    // Wait for layout so fit() sees the real size.
    requestAnimationFrame(() => void this.start(false));
  }

  collapse(): void {
    if (this.collapsed()) return;
    this.collapsed.set(true);
    this.stop();
  }

  close(): void {
    this.stop();
    this.closed.emit();
  }

  scrollIntoView(): void {
    this.host.nativeElement.scrollIntoView({ behavior: "smooth", block: "nearest" });
  }

  toggleMode(): void {
    this.setMode(this.mode() === "observe" ? "control" : "observe");
  }

  setMode(mode: Mode, takeover = false): void {
    this.mode.set(mode);
    if (!this.collapsed()) void this.restartStream(takeover);
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
    if (this.collapsed() || this.destroyed) return;
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
      // The view was collapsed or closed while the stream opened.
      if (this.collapsed() || this.destroyed) {
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
      if (!this.term || !this.fit || this.collapsed()) return;
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
