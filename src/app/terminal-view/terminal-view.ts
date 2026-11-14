import {
  ChangeDetectionStrategy,
  Component,
  computed,
  ElementRef,
  afterNextRender,
  effect,
  inject,
  Injector,
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
import { Terminal as XTerm } from "@xterm/xterm";
import { useUnicode11 } from "./unicode";
import { TaskStarter } from "../core/task-start";
import { TauriBridge } from "../core/tauri";
import {
  agentKind,
  decodeBase64,
  splitId,
  type Mode,
  type Terminal,
  type StreamRecord,
} from "../core/herdr";
import { SettingsStore, defaultTerminalFontFamily, windowsPtyOption } from "../core/settings";
import { Keymap } from "../core/keymap";
import { UiState } from "../core/ui-state";
import { TerminalSession, TerminalStreamService, type MouseCommand } from "../core/terminal-stream";
import { PromptBar } from "../prompt-bar/prompt-bar";
import { holdsLinkModifier, isRepo, linkHandler, taskLinkHandler, type IssueLink } from "./links";
import { buttonName, cellAt, modifierBits } from "./mouse";

const RESIZE_DEBOUNCE_MS = 150;
/**
 * Waits before each attempt to attach again after the connection to the server ended (a restart
 * or a crash; the terminal lives on in its PTY host and the next server adopts it, #604): about
 * 30 s in all, then the reason stays as the banner.
 */
export const REATTACH_DELAYS_MS = [250, 500, 1000, 2000, 3000, 5000, 8000, 10000];
/** The banner while the view attaches again. */
export const REATTACHING = "poimen-server is not reachable; attaching again…";

export interface ModeState {
  mode: Mode;
  blocked: string | null;
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
  imports: [HxButton, PromptBar],
  templateUrl: "./terminal-view.html",
  styleUrl: "./terminal-view.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { "[class.controlling]": "mode() === 'control'" },
})
export class TerminalView implements OnDestroy {
  readonly pane = input.required<Terminal>();
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
  /** Whether the pane hosts an agent, so the prompt bar is shown (also in read-only mode). */
  protected readonly isAgent = computed(() => !!agentKind(this.pane()));

  private readonly streams = inject(TerminalStreamService);
  private readonly prefs = inject(SettingsStore);
  private readonly keymap = inject(Keymap);
  private readonly ui = inject(UiState);
  private readonly tauri = inject(TauriBridge);
  private readonly injector = inject(Injector);
  private readonly body = viewChild.required<ElementRef<HTMLElement>>("body");

  private term?: XTerm;
  private fit?: FitAddon;
  private session?: TerminalSession;
  /** Whether the stream opened last is emulated; stays after the stream closed (scrollback keeps). */
  private streamEmulated = false;
  private themeObserver?: MutationObserver;
  private resizeObserver?: ResizeObserver;
  private resizeTimer?: ReturnType<typeof setTimeout>;
  private size = { cols: 0, rows: 0 };
  /** The PTY size the server last reported (the controller's, for an observer). */
  private ptySize = { cols: 0, rows: 0 };
  private destroyed = false;
  private framed = false;
  /** Grid of the last `terminal.frame`; herdr's pane may differ from xterm's during a resize. */
  private frameGrid: { cols: number; rows: number } | undefined;
  /** The snapshot of an emulated controller stream was just reset: refit once it is written. */
  private refitAfterSnapshot = false;
  private detachMouse?: () => void;
  private detachClipboard?: () => void;
  /** Whether the pointer is over a link (xterm reports it for every renderer). */
  private linkHovered = false;
  private endDrag?: () => void;
  /** The next attempt to attach again after a lost connection, and how many came before it. */
  private reattachTimer?: ReturnType<typeof setTimeout>;
  private reattaches = 0;
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
    this.applyScrollback();
    // A new font changes the cell size, so cols/rows change without a body resize.
    this.applySize();
  }

  /** Font and colors from the settings, over the theme tokens. */
  private look() {
    const { fontFamily, fontSize: configured, colors } = this.prefs.settings();
    const fontSize = Math.min(72, Math.max(6, configured + this.ui.zoom()));
    const foreground = colors?.foreground ?? cssVar("--term-fg");
    return {
      fontFamily: fontFamily ?? defaultTerminalFontFamily(cssVar("--font-mono")),
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

  /** Modifier click on an issue link: the task dialog when it is an issue of this pane's repository. */
  private async startTask(link: IssueLink): Promise<boolean> {
    const pane = this.pane();
    const dir = pane.foreground_cwd || pane.cwd;
    if (!dir) return false;
    const repo = await this.tauri.invoke<{ owner: string; name: string } | null>("github_repo", {
      dir,
    });
    if (!isRepo(link, repo)) return false;
    void this.injector.get(TaskStarter).start(dir, link.number);
    return true;
  }

  private createTerminal(): void {
    const open = (url: string) => this.tauri.openUrl(url);
    const openLink = linkHandler(open);
    const hoverCallbacks = {
      hover: () => (this.linkHovered = true),
      leave: () => (this.linkHovered = false),
    };
    const term = new XTerm({
      // OSC 8 hyperlinks: xterm's default would confirm() and window.open(); use the opener instead.
      linkHandler: {
        activate: openLink,
        ...hoverCallbacks,
      },
      // herdr owns scrollback (wheel events are forwarded instead); an emulated stream gets
      // xterm.js's own once it is open (`applyScrollback`).
      scrollback: 0,
      cursorBlink: true,
      allowProposedApi: true,
      ...this.look(),
    });
    useUnicode11(term);
    const fit = new FitAddon();
    term.loadAddon(fit);
    // xterm marks a hovered link itself (underline, pointer); the view only needs to know about it.
    this.linkHovered = false;
    term.loadAddon(
      // Detected URLs are trimmed before opening (the addon's range, and so the underline, stays).
      new WebLinksAddon(
        taskLinkHandler(open, (link) => this.startTask(link)),
        hoverCallbacks,
      ),
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
      // Emulated streams carry PTY bytes: xterm.js itself scrolls and reports the wheel, so its
      // default handling must run (returning false would skip it).
      if (this.session?.emulated) return true;
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

  /**
   * Whether mouse and wheel go to the pane as commands: only in a stream opened for control and
   * only for a rendered stream (finished frames, whose application modes xterm.js cannot see). An
   * emulated stream carries the PTY bytes, so xterm.js tracks the modes and reports the mouse
   * itself.
   */
  private forwardsMouse(): boolean {
    return this.attempt === "control" && this.session?.emulated !== true;
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
      if (!term || !session?.open || !this.forwardsMouse()) return;
      const screen = host.querySelector(".xterm-screen") ?? host;
      at = cellAt(
        screen.getBoundingClientRect(),
        term.cols,
        term.rows,
        event.clientX,
        event.clientY,
        this.frameGrid,
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
      if (!this.forwardsMouse() || event.shiftKey || this.opensLink(event)) return; // Shift: xterm selects text, Cmd (macOS) or Ctrl over a link: open it
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
      if (this.forwardsMouse() && !event.shiftKey) event.preventDefault();
    };
    host.addEventListener("mousedown", onDown, true);
    host.addEventListener("mousemove", onHostMove, true);
    host.addEventListener("contextmenu", onContext);
    // A drag still held when the stream ends (tab closed, mode switch) is released first.
    this.endDrag = () => {
      if (pressed && this.session?.open && this.forwardsMouse()) {
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

  /** `reattach`: an attempt after a lost connection, which tries again when it fails. */
  private async connect(mode: Mode, takeover: boolean, reattach = false): Promise<void> {
    this.framed = false;
    this.frameGrid = undefined;
    this.refitAfterSnapshot = false;
    this.attempt = mode;
    const generation = ++this.generation;
    let session: TerminalSession | undefined;
    // Records that arrive before open() returned are held until the session is assigned.
    const early: StreamRecord[] = [];
    try {
      session = await this.streams.open(
        {
          mode,
          target: splitId(this.pane().terminal_id).id,
          ...(this.pane().machine ? { machine: this.pane().machine } : {}),
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
      this.streamEmulated = session.emulated;
      this.applyScrollback();
      for (const record of early.splice(0)) this.onRecord(session, record);
    } catch (err) {
      if (generation !== this.generation) return;
      if (reattach) this.reattach(String(err));
      else this.failed(mode, String(err));
    }
  }

  /**
   * The connection to the server ended without a record: attach again in the same mode (never
   * with a takeover) after the next of {@link REATTACH_DELAYS_MS}, read-only meanwhile; once they
   * are used up the reason stays as the banner.
   */
  private reattach(reason: string): void {
    if (this.destroyed) return;
    const delay = REATTACH_DELAYS_MS[this.reattaches];
    if (delay === undefined) {
      this.reattaches = 0;
      this.setState("observe", null);
      this.banner.set(reason);
      return;
    }
    this.reattaches++;
    const mode = this.attempt;
    this.setState("observe", null);
    this.banner.set(REATTACHING);
    clearTimeout(this.reattachTimer);
    this.reattachTimer = setTimeout(() => void this.connect(mode, false, true), delay);
  }

  /** A stream ended or could not open: fall back to observing when control was attempted. */
  private failed(mode: Mode, reason: string, lost = false, held = false): void {
    if (mode !== "control" || this.destroyed) {
      this.banner.set(reason);
      return;
    }
    // Held by another client: stay usable as an observer; going back to input is an explicit
    // user action. Any other failure is shown as an error.
    this.setState("observe", held ? reason : null);
    void this.connect("observe", false);
    if (lost || !held) this.banner.set(reason);
  }

  private closeStream(): void {
    clearTimeout(this.reattachTimer);
    this.reattaches = 0;
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
    switch (record.type) {
      case "reset":
        this.framed = true;
        this.ptySize = { cols: record.cols, rows: record.rows };
        if (this.reattaches > 0) {
          // Attached again after a lost connection.
          this.reattaches = 0;
          this.banner.set(null);
        }
        if (session.emulated) {
          // A snapshot follows as bytes. Reset first, so a reattach does not stack the snapshot
          // (and its scrollback) on the old screen; the grid is the server's until the
          // controller has refit.
          this.term?.reset();
          this.term?.resize(record.cols, record.rows);
          this.refitAfterSnapshot = this.attempt === "control";
        } else {
          this.frameGrid = { cols: record.cols, rows: record.rows };
        }
        this.term?.write(decodeBase64(record.bytes));
        return;
      case "output":
        this.framed = true;
        this.term?.write(decodeBase64(record.bytes));
        return;
      case "bytes":
        this.framed = true;
        this.writeBytes(record.data);
        return;
      case "resized":
        this.ptySize = { cols: record.cols, rows: record.rows };
        if (!session.emulated) {
          this.frameGrid = { cols: record.cols, rows: record.rows };
        } else if (this.attempt !== "control") {
          // An observer renders at the terminal's size, the controller's while one holds it.
          this.term?.resize(record.cols, record.rows);
        }
        return;
      case "controller":
        // Input is reported only once the backend says this client holds the controller.
        if (record.attached && this.attempt === "control") this.setState("control", null);
        return;
      case "closed":
        this.session = undefined;
        if (record.reason === "detached") return;
        if (record.code === "lost") {
          this.reattach(record.reason);
          return;
        }
        if (record.code === "exited") {
          // The process ended: keep the last screen and say how; nothing to fall back to.
          this.setState("observe", null);
          this.banner.set(record.reason);
          return;
        }
        this.failed(this.attempt, record.reason, this.framed, record.code === "controller_held");
        return;
    }
  }

  /** Raw PTY bytes; the first ones after a snapshot are followed by the controller's refit. */
  private writeBytes(data: Uint8Array): void {
    if (!this.refitAfterSnapshot) {
      this.term?.write(data);
      return;
    }
    this.refitAfterSnapshot = false;
    this.term?.write(data, () => {
      // The snapshot is on the screen: take our own size back and tell the server.
      if (this.attempt !== "control" || !this.session?.open) return;
      this.size = { cols: 0, rows: 0 };
      this.applySize();
    });
  }

  /** Lines of scrollback: xterm.js keeps its own on an emulated stream; herdr owns it otherwise. */
  private applyScrollback(): void {
    if (!this.term) return;
    this.term.options.scrollback = this.streamEmulated ? this.prefs.settings().scrollback : 0;
    // An emulated stream is a ConPTY on Windows (the poimen server runs on this machine).
    this.term.options.windowsPty = this.streamEmulated ? windowsPtyOption() : undefined;
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
    const session = this.session;
    if (!session?.open) return;
    if (this.attempt === "control") {
      void session.resize(cols, rows);
    } else if (session.emulated) {
      this.resizeObserved(session, cols, rows);
    } else {
      // Observers have a fixed viewport; reopen as an observer, never retrying control.
      void this.restartStream(false, "observe", true);
    }
  }

  /**
   * An emulated observer sets its terminal's PTY size while no controller holds it, so an
   * unfocused leaf of a split tab follows its container. A refusal means a controller holds the
   * terminal: the leaf renders at the size the server last reported, the controller's.
   */
  private resizeObserved(session: TerminalSession, cols: number, rows: number): void {
    session.resize(cols, rows).catch(() => {
      if (this.session !== session || !session.open || this.ptySize.cols === 0) return;
      this.term?.resize(this.ptySize.cols, this.ptySize.rows);
      this.size = { ...this.ptySize };
      this.sized.emit(this.size);
    });
  }
}
