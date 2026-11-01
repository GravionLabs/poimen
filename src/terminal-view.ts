import { invoke } from "@tauri-apps/api/core";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import { decodeBase64, paneLabel, type Pane } from "./model";

export type Mode = "observe" | "control";

export type StreamRecord =
  | { type: "frame"; seq: number; full: boolean; width: number; height: number; bytes: string }
  | { type: "closed"; reason: string };

/** Routes stream records (emitted globally by the backend) to the owning view. */
export const streams = new Map<number, (record: StreamRecord) => void>();

function cssVar(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

/**
 * One open pane: a collapsible card. Expanded = live herdr stream in xterm.js.
 * Collapsed = stream closed; the herdr pane keeps running.
 */
export class TerminalView {
  readonly element: HTMLElement;
  private readonly header: HTMLElement;
  private readonly body: HTMLElement;
  private readonly banner: HTMLElement;
  private readonly modeButton: HTMLButtonElement;
  private term?: Terminal;
  private fit?: FitAddon;
  private sessionId?: number;
  private resizeObserver?: ResizeObserver;
  private resizeTimer?: number;
  private size = { cols: 0, rows: 0 };
  collapsed = false;
  mode: Mode = "observe";

  constructor(
    public pane: Pane,
    private readonly onRemove: (view: TerminalView) => void,
  ) {
    this.element = document.createElement("section");
    this.element.className = "view";
    this.element.innerHTML = `
      <header class="view-header">
        <button class="icon chevron" title="Collapse / expand">▾</button>
        <span class="dot"></span>
        <span class="view-title"></span>
        <span class="view-id"></span>
        <span class="spacer"></span>
        <button class="mode" title="Switch between read-only and input"></button>
        <button class="icon close" title="Close view (pane keeps running)">✕</button>
      </header>
      <div class="view-banner" hidden></div>
      <div class="view-body"></div>`;
    this.header = this.element.querySelector(".view-header")!;
    this.body = this.element.querySelector(".view-body")!;
    this.banner = this.element.querySelector(".view-banner")!;
    this.modeButton = this.element.querySelector(".mode")!;

    this.header.querySelector(".chevron")!.addEventListener("click", () => this.toggle());
    this.header.querySelector(".view-title")!.addEventListener("dblclick", () => this.toggle());
    this.header.querySelector(".close")!.addEventListener("click", () => this.remove());
    this.modeButton.addEventListener("click", () =>
      this.setMode(this.mode === "observe" ? "control" : "observe"),
    );
    this.update(pane);
  }

  update(pane: Pane) {
    this.pane = pane;
    this.header.querySelector(".view-title")!.textContent = paneLabel(pane);
    this.header.querySelector(".view-id")!.textContent = pane.pane_id;
    this.header.querySelector(".dot")!.className = `dot status-${pane.agent_status}`;
  }

  mount() {
    if (!this.collapsed) this.expand();
    this.renderMode();
  }

  toggle() {
    if (this.collapsed) {
      this.expand();
    } else {
      this.collapse();
    }
  }

  expand() {
    this.collapsed = false;
    this.element.classList.remove("collapsed");
    // Wait for layout so fit() sees the real size.
    requestAnimationFrame(() => this.start(false));
  }

  collapse() {
    this.collapsed = true;
    this.element.classList.add("collapsed");
    this.stop();
  }

  remove() {
    this.stop();
    this.element.remove();
    this.onRemove(this);
  }

  setMode(mode: Mode, takeover = false) {
    this.mode = mode;
    this.renderMode();
    if (!this.collapsed) void this.restartStream(takeover);
  }

  private renderMode() {
    const control = this.mode === "control";
    this.modeButton.textContent = control ? "Input" : "Read-only";
    this.modeButton.classList.toggle("active", control);
    this.element.classList.toggle("controlling", control);
  }

  private createTerminal() {
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
    term.open(this.body);
    try {
      term.loadAddon(new WebglAddon());
    } catch {
      // DOM renderer fallback
    }
    term.onData((data) => {
      if (this.mode === "control" && this.sessionId !== undefined) {
        void invoke("terminal_input", { id: this.sessionId, text: data });
      }
    });
    term.attachCustomWheelEventHandler((event) => {
      if (this.mode === "control" && this.sessionId !== undefined && event.deltaY !== 0) {
        const lines = Math.max(1, Math.min(10, Math.round(Math.abs(event.deltaY) / 30)));
        void invoke("terminal_scroll", { id: this.sessionId, up: event.deltaY < 0, lines });
      }
      return false;
    });
    this.term = term;
    this.fit = fit;
    this.resizeObserver = new ResizeObserver(() => this.scheduleResize());
    this.resizeObserver.observe(this.body);
  }

  private async start(takeover: boolean) {
    if (this.collapsed) return;
    if (!this.term) this.createTerminal();
    this.fit!.fit();
    this.size = { cols: this.term!.cols, rows: this.term!.rows };
    await this.openStream(takeover);
    this.term!.focus();
  }

  private async openStream(takeover: boolean) {
    this.hideBanner();
    try {
      const id = await invoke<number>("open_terminal", {
        mode: this.mode,
        target: this.pane.pane_id,
        cols: this.size.cols,
        rows: this.size.rows,
        takeover,
      });
      this.sessionId = id;
      streams.set(id, (record) => this.onRecord(id, record));
    } catch (err) {
      this.showBanner(String(err));
    }
  }

  private closeStream() {
    if (this.sessionId === undefined) return;
    const id = this.sessionId;
    streams.delete(id);
    this.sessionId = undefined;
    void invoke("close_terminal", { id });
  }

  private async restartStream(takeover: boolean) {
    this.closeStream();
    if (this.term) await this.openStream(takeover);
    else await this.start(takeover);
  }

  private stop() {
    this.closeStream();
    clearTimeout(this.resizeTimer);
    this.resizeObserver?.disconnect();
    this.term?.dispose();
    this.term = undefined;
    this.fit = undefined;
  }

  private onRecord(id: number, record: StreamRecord) {
    if (id !== this.sessionId) return;
    if (record.type === "frame") {
      this.term?.write(decodeBase64(record.bytes));
      return;
    }
    streams.delete(id);
    this.sessionId = undefined;
    if (record.reason !== "detached") this.showBanner(record.reason);
  }

  private scheduleResize() {
    clearTimeout(this.resizeTimer);
    this.resizeTimer = window.setTimeout(() => {
      if (!this.term || !this.fit || this.collapsed) return;
      this.fit.fit();
      const { cols, rows } = this.term;
      if (cols === this.size.cols && rows === this.size.rows) return;
      this.size = { cols, rows };
      if (this.sessionId === undefined) return;
      if (this.mode === "control") {
        void invoke("terminal_resize", { id: this.sessionId, cols, rows });
      } else {
        // Observers have a fixed viewport; reopen with the new size.
        void this.restartStream(false);
      }
    }, 150);
  }

  private showBanner(reason: string) {
    this.banner.hidden = false;
    this.banner.innerHTML = "";
    const text = document.createElement("span");
    text.textContent = `Stream closed: ${reason}`;
    const reconnect = document.createElement("button");
    reconnect.textContent = "Reconnect";
    reconnect.addEventListener("click", () => void this.restartStream(false));
    this.banner.append(text, reconnect);
    if (this.mode === "control") {
      const takeover = document.createElement("button");
      takeover.textContent = "Take over";
      takeover.title = "Replace the current controller of this terminal";
      takeover.addEventListener("click", () => void this.restartStream(true));
      this.banner.append(takeover);
    }
  }

  private hideBanner() {
    this.banner.hidden = true;
  }
}
