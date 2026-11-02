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
import { WebLinksAddon } from "@xterm/addon-web-links";
import { WebglAddon } from "@xterm/addon-webgl";
import { Terminal } from "@xterm/xterm";
import { TauriBridge } from "../core/tauri";
import { decodeBase64, type Mode, type Pane, type StreamRecord } from "../core/herdr";
import { SettingsStore } from "../core/settings";
import { Keymap } from "../core/keymap";
import { UiState } from "../core/ui-state";
import { TerminalSession, TerminalStreamService, type MouseCommand } from "../core/terminal-stream";
import { detectedLinkHandler, holdsLinkModifier, linkHandler } from "./links";
import { buttonName, cellAt, modifierBits } from "./mouse";

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

const ANSI_NAMES = [
  "black",
  "red",
  "green",
  "yellow",
  "blue",
  "magenta",
  "cyan",
  "white",
  "brightBlack",
  "brightRed",
  "brightGreen",
  "brightYellow",
  "brightBlue",
  "brightMagenta",
  "brightCyan",
  "brightWhite",
] as const;

/** The 16 ANSI colors: Ghostty's `palette` entries over `--term-ansi-0..15`; a missing token leaves xterm's default. */
function ansiColors(
  palette: readonly (string | null)[] = [],
): Partial<Record<(typeof ANSI_NAMES)[number], string>> {
  const colors: Partial<Record<(typeof ANSI_NAMES)[number], string>> = {};
  ANSI_NAMES.forEach((name, i) => {
    const value = palette[i] || cssVar(`--term-ansi-${i}`);
    if (value) colors[name] = value;
  });
  return colors;
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
  private readonly keymap = inject(Keymap);
  private readonly ui = inject(UiState);
  private readonly tauri = inject(TauriBridge);
  private readonly body = viewChild.required<ElementRef<HTMLElement>>("body");

  private term?: Terminal;
  private fit?: FitAddon;
  private session?: TerminalSession;
  private themeObserver?: MutationObserver;
  private resizeObserver?: ResizeObserver;
  private resizeTimer?: ReturnType<typeof setTimeout>;
  private size = { cols: 0, rows: 0 };
  private destroyed = false;
  private framed = false;
  private detachMouse?: () => void;
  private detachClipboard?: () => void;
  /** Whether the pointer is over a link (xterm reports it for every renderer). */
  private linkHovered = false;
  private endDrag?: () => void;
  /** Counts connects; a result of an older one is dropped. */
  private generation = 0;
  /** The mode of the stream being opened or running (what the CLI child was spawned with). */
  private attempt: Mode = "observe";

  constructor() {
    afterNextRender(() => void this.start(false));
    // The theme tokens change with the class HxTheme puts on <html>, in its own effect and possibly
    // in a view transition, so re-read the colors when the class really changed, not when the setting did.
    if (typeof MutationObserver === "function") {
      this.themeObserver = new MutationObserver(() => this.applyLook());
      this.themeObserver.observe(document.documentElement, {
        attributes: true,
        attributeFilter: ["class", "data-theme", "style"],
      });
    }
    // Settings edits (and theme switches) apply to the open terminal without a restart.
    effect(() => {
      this.prefs.settings();
      this.ui.zoom();
      untracked(() => this.applyLook());
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

  private applyLook(): void {
    if (!this.term) return;
    Object.assign(this.term.options, this.look());
    // A new font changes the cell size, so cols/rows change without a body resize.
    this.applySize();
  }

  /** Font and colors from the settings, over the theme tokens. */
  private look() {
    const { fontFamily, fontSize: configured, colors } = this.prefs.settings();
    const fontSize = Math.min(72, Math.max(6, configured + this.ui.zoom()));
    const foreground = colors?.foreground ?? cssVar("--term-fg");
    return {
      fontFamily: fontFamily ?? (cssVar("--font-mono") || "Menlo, monospace"),
      fontSize,
      theme: {
        background: colors?.background ?? cssVar("--term-bg"),
        foreground,
        cursor: colors?.cursor ?? (cssVar("--term-cursor") || foreground),
        selectionBackground:
          colors?.selectionBackground ?? (cssVar("--term-selection-bg") || undefined),
        selectionForeground:
          colors?.selectionForeground ?? (cssVar("--term-selection-fg") || undefined),
        ...ansiColors(colors?.palette),
      },
    };
  }

  ngOnDestroy(): void {
    this.destroyed = true;
    this.themeObserver?.disconnect();
    this.stop();
  }

  reconnect(takeover: boolean): void {
    void this.restartStream(takeover);
  }

  private createTerminal(): void {
    const open = (url: string) => this.tauri.openUrl(url);
    const openLink = linkHandler(open);
    const hoverCallbacks = {
      hover: () => (this.linkHovered = true),
      leave: () => (this.linkHovered = false),
    };
    const term = new Terminal({
      // OSC 8 hyperlinks: xterm's default would confirm() and window.open(); use the opener instead.
      linkHandler: {
        activate: openLink,
        ...hoverCallbacks,
      },
      scrollback: 0, // herdr owns scrollback; wheel events are forwarded instead
      cursorBlink: true,
      allowProposedApi: true,
      ...this.look(),
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    // xterm marks a hovered link itself (underline, pointer); the view only needs to know about it.
    this.linkHovered = false;
    term.loadAddon(
      // Detected URLs are trimmed before opening (the addon's range, and so the underline, stays).
      new WebLinksAddon(detectedLinkHandler(open), hoverCallbacks),
    );
    term.open(this.body().nativeElement);
    try {
      term.loadAddon(new WebglAddon());
    } catch {
      // DOM renderer fallback
    }
    // App shortcuts belong to the app: keep xterm from forwarding them to the pane. Only Cmd
    // shortcuts match here; other keys go to the program in the pane.
    term.attachCustomKeyEventHandler((event) => {
      if (this.keymap.match(event, true) !== null) return false;
      return !this.clipboardKey(event);
    });
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
    this.detachMouse = this.attachMouse(this.body().nativeElement);
    this.detachClipboard = this.attachClipboard(this.body().nativeElement);
    this.resizeObserver = new ResizeObserver(() => this.scheduleResize());
    this.resizeObserver.observe(this.body().nativeElement);
  }

  /**
   * Clipboard keys that are not the native Edit menu's. On macOS Cmd+C and Cmd+V are the menu's
   * (they arrive as `copy` and `paste` events, see {@link attachClipboard}). Elsewhere Ctrl+C and
   * Ctrl+V belong to the pane application, so the terminal convention Ctrl+Shift+C copies the
   * selection and Ctrl+Shift+V pastes (the browser's own paste event follows). Returns whether the
   * key was taken, so xterm does not send it to the pane.
   */
  private clipboardKey(event: KeyboardEvent): boolean {
    if (this.keymap.mac || !event.ctrlKey || !event.shiftKey || event.altKey || event.metaKey) {
      return false;
    }
    const key = event.key.toLowerCase();
    if (key === "c") {
      const text = this.term?.getSelection();
      if (text) void navigator.clipboard?.writeText(text).catch(() => undefined);
      return true;
    }
    return key === "v"; // the paste event does the work
  }

  /**
   * Copy and paste through the DOM events of the Edit menu / browser. Copy puts the terminal
   * selection (Shift+drag in input mode, plain drag when read-only) on the clipboard in either
   * mode. Paste goes through xterm's `paste`, which wraps the text in bracketed paste markers when
   * the pane application asked for them; a read-only tab pastes nothing.
   */
  private attachClipboard(host: HTMLElement): () => void {
    const onCopy = (event: ClipboardEvent) => {
      const text = this.term?.getSelection();
      if (!text || !event.clipboardData) return;
      event.clipboardData.setData("text/plain", text);
      event.preventDefault();
    };
    const onPaste = (event: ClipboardEvent) => {
      event.preventDefault();
      event.stopPropagation(); // xterm's own paste listener would send it a second time
      const text = event.clipboardData?.getData("text/plain");
      if (text && this.mode() === "control" && this.session?.open) this.term?.paste(text);
    };
    host.addEventListener("copy", onCopy, true);
    host.addEventListener("paste", onPaste, true);
    return () => {
      host.removeEventListener("copy", onCopy, true);
      host.removeEventListener("paste", onPaste, true);
    };
  }

  /** A press with the link modifier over a link opens it instead of going to the pane application. */
  private opensLink(event: { metaKey: boolean; ctrlKey: boolean }): boolean {
    return this.linkHovered && holdsLinkModifier(event);
  }

  /**
   * herdr's stream is a rendered screen: it carries no DECSET 1000/1002/1003/1006, so xterm never
   * enables mouse tracking and would never report anything. The mouse is therefore forwarded as
   * `terminal.mouse` cells; herdr encodes them for the pane application's mouse mode and drops
   * them when the application did not ask for the mouse. herdr does not expose that mode to
   * clients, so it cannot be checked here; a held Shift (as in other terminals) bypasses the
   * forwarding and leaves xterm's text selection and the native context menu; so does Cmd on macOS
   * or Ctrl elsewhere while the pointer is over a link, which is how a link is opened in input mode
   * (the web-links addon sees the click only then; Ctrl+click is the context menu on macOS, hence
   * Cmd there). Ctrl elsewhere stays the pane application's modifier outside a link. Read-only tabs send
   * nothing. Returns the function that removes every listener.
   */
  private attachMouse(host: HTMLElement): () => void {
    let pressed: MouseCommand["button"] | null = null;
    let last = "";
    let at = { column: 0, row: 0 };
    const send = (event: MouseEvent, action: MouseCommand["action"], button: number) => {
      const session = this.session;
      const term = this.term;
      // Gate on the mode the stream was opened with, not on the (earlier) requested mode.
      if (!term || !session?.open || this.attempt !== "control") return;
      const screen = host.querySelector(".xterm-screen") ?? host;
      at = cellAt(
        screen.getBoundingClientRect(),
        term.cols,
        term.rows,
        event.clientX,
        event.clientY,
      );
      const key = `${action}:${at.column}:${at.row}`;
      if ((action === "move" || action === "drag") && key === last) return;
      last = key;
      session.mouse({
        action,
        button: buttonName(button),
        ...at,
        modifiers: modifierBits(event),
      });
    };
    const onMove = (event: MouseEvent) => {
      if (pressed) send(event, "drag", event.button);
      else if (!event.shiftKey && !this.opensLink(event)) send(event, "move", 0);
    };
    const onHostMove = (event: MouseEvent) => {
      if (!pressed) onMove(event);
    };
    const release = () => {
      window.removeEventListener("mouseup", onUp, true);
      window.removeEventListener("mousemove", onMove, true);
      pressed = null;
      last = "";
    };
    function onUp(event: MouseEvent) {
      if (pressed) send(event, "up", event.button);
      release();
    }
    const onDown = (event: MouseEvent) => {
      if (this.attempt !== "control" || event.shiftKey || this.opensLink(event)) return; // Shift: xterm selects text, Cmd (macOS) or Ctrl over a link: open it
      event.stopPropagation(); // no text selection while the pane application owns the mouse
      event.preventDefault();
      this.term?.focus();
      pressed = buttonName(event.button);
      last = "";
      send(event, "down", event.button);
      window.addEventListener("mouseup", onUp, true);
      window.addEventListener("mousemove", onMove, true);
    };
    const onContext = (event: MouseEvent) => {
      if (this.attempt === "control" && !event.shiftKey) event.preventDefault();
    };
    host.addEventListener("mousedown", onDown, true);
    host.addEventListener("mousemove", onHostMove, true);
    host.addEventListener("contextmenu", onContext);
    // A drag still held when the stream ends (tab closed, mode switch) is released first.
    this.endDrag = () => {
      if (pressed && this.session?.open && this.attempt === "control") {
        this.session.mouse({ action: "up", button: pressed, ...at, modifiers: 0 });
      }
      release();
    };
    return () => {
      release();
      host.removeEventListener("mousedown", onDown, true);
      host.removeEventListener("mousemove", onHostMove, true);
      host.removeEventListener("contextmenu", onContext);
    };
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
    this.endDrag?.();
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
    this.detachMouse?.();
    this.detachMouse = undefined;
    this.detachClipboard?.();
    this.detachClipboard = undefined;
    this.linkHovered = false;
    this.endDrag = undefined;
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
