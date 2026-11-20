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
import { SearchAddon } from "@xterm/addon-search";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { WebglAddon } from "@xterm/addon-webgl";
import { Terminal as XTerm } from "@xterm/xterm";
import { useUnicode11 } from "./unicode";
import { TaskStarter } from "../core/task-start";
import { TauriBridge, type DragDrop, type Unlisten } from "../core/tauri";
import { SpaceActions } from "../core/space-actions";
import {
  dropAction,
  imagePasteRefusal,
  overRect,
  pastedImageType,
  quotePaths,
  savableImage,
} from "./drop";
import { agentKind, splitId, type Terminal } from "../core/snapshot";
import type { Mode, StreamRecord } from "../core/backend";
import {
  SettingsStore,
  defaultTerminalFontFamily,
  isWindows,
  windowsPtyOption,
} from "../core/settings";
import { Keymap } from "../core/keymap";
import { UiState } from "../core/ui-state";
import { TerminalSession, TerminalStreamService } from "../core/terminal-stream";
import { PromptBar } from "../prompt-bar/prompt-bar";
import { FILE_LINK_MENU, FileLinkActions } from "./file-link-actions";
import { fileLinkProvider, offersFileLinks, type FileLinkTarget } from "./file-links";
import { ContextMenu } from "../ui/context-menu/context-menu";
import { holdsLinkModifier, isRepo, linkHandler, taskLinkHandler, type IssueLink } from "./links";
import { matchLabel, searchDecorations, TerminalSearch, type Searchable } from "./search";
import { Icon } from "../ui/icon/icon";

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

export function cssVar(name: string): string {
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
 * destroys it, which closes the stream and disposes xterm; the terminal keeps running.
 */
/** Limits of the inline image addon (`pixelLimit` per image, `storageLimit` in MB). */
export const IMAGE_OPTIONS = { pixelLimit: 16_777_216, storageLimit: 32 };

@Component({
  selector: "app-terminal-view",
  imports: [HxButton, PromptBar, Icon, ContextMenu],
  templateUrl: "./terminal-view.html",
  styleUrl: "./terminal-view.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: {
    "[class.controlling]": "mode() === 'control'",
    "[class.drop-target]": "dropHover()",
    "(focusin)": "onSearchFocus()",
    "(focusout)": "onSearchBlur($event)",
  },
})
export class TerminalView implements OnDestroy, Searchable {
  readonly pane = input.required<Terminal>();
  /** The menu of a click on the path of a changed file (#1003). */
  protected readonly fileMenu = signal<{
    at: { x: number; y: number };
    target: FileLinkTarget;
  } | null>(null);
  protected readonly fileMenuGroups = FILE_LINK_MENU;
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
  private readonly search = inject(TerminalSearch);
  private readonly ui = inject(UiState);
  private readonly tauri = inject(TauriBridge);
  private readonly injector = inject(Injector);
  /** Files are being dragged over this view. */
  protected readonly dropHover = signal(false);
  private unlistenDrop?: Unlisten;
  private readonly body = viewChild.required<ElementRef<HTMLElement>>("body");

  private term?: XTerm;
  private fit?: FitAddon;
  private searchAddon?: SearchAddon;
  private searchResults?: { dispose(): void };
  /** The search bar: shown, query, options and the match position the addon reported. */
  protected readonly searchOpen = signal(false);
  protected readonly searchQuery = signal("");
  protected readonly searchCase = signal(false);
  protected readonly searchRegex = signal(false);
  protected readonly searchResult = signal({ index: -1, count: 0 });
  protected readonly searchCounter = computed(() =>
    matchLabel(this.searchQuery(), this.searchResult().index, this.searchResult().count),
  );
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  private session?: TerminalSession;
  private themeObserver?: MutationObserver;
  private resizeObserver?: ResizeObserver;
  private resizeTimer?: ReturnType<typeof setTimeout>;
  private size = { cols: 0, rows: 0 };
  /** The PTY size the server last reported (the controller's, for an observer). */
  private ptySize = { cols: 0, rows: 0 };
  private destroyed = false;
  private framed = false;
  /** The snapshot of a controller stream was just reset: refit once it is written. */
  private refitAfterSnapshot = false;
  private detachClipboard?: () => void;
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
    if (this.searchOpen() && this.searchQuery()) this.find(true, true); // new highlight colors
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
    this.unlistenDrop?.();
    this.search.gone(this);
    this.searchResults?.dispose();
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

  /** Cmd/Ctrl+click on a file path opens the editor (#1003); not on a remote pane (the file is not here). */
  private registerFileLinks(term: XTerm): void {
    if (!offersFileLinks(this.pane())) return;
    const actions = this.injector.get(FileLinkActions);
    term.registerLinkProvider(
      fileLinkProvider(
        term,
        () => this.pane().foreground_cwd || this.pane().cwd,
        (dir, paths) => actions.resolve(dir, paths),
        (event, target) => {
          if (!holdsLinkModifier(event)) return;
          const at = { x: event.clientX, y: event.clientY };
          void actions.isChanged(target).then((changed) => {
            if (changed && !this.destroyed) this.fileMenu.set({ at, target });
            else actions.open(target);
          });
        },
      ),
    );
  }

  protected pickFileLink(id: string): void {
    const target = this.fileMenu()?.target;
    this.fileMenu.set(null);
    if (!target) return;
    const actions = this.injector.get(FileLinkActions);
    if (id === "changes") actions.showInChanges(target);
    else actions.open(target);
  }

  private createTerminal(): void {
    const open = (url: string) => this.tauri.openUrl(url);
    const openLink = linkHandler(open);
    const term = new XTerm({
      // OSC 8 hyperlinks: xterm's default would confirm() and window.open(); use the opener instead.
      linkHandler: { activate: openLink },
      // The scrollback is set once a stream is open (`applyScrollback`).
      scrollback: 0,
      cursorBlink: true,
      allowProposedApi: true,
      ...this.look(),
    });
    useUnicode11(term);
    const fit = new FitAddon();
    term.loadAddon(fit);
    const searchAddon = new SearchAddon();
    term.loadAddon(searchAddon);
    this.searchAddon = searchAddon;
    this.searchResults = searchAddon.onDidChangeResults(({ resultIndex, resultCount }) =>
      this.searchResult.set({ index: resultIndex, count: resultCount }),
    );
    term.loadAddon(
      // Detected URLs are trimmed before opening (the addon's range, and so the underline, stays).
      new WebLinksAddon(taskLinkHandler(open, (link) => this.startTask(link))),
    );
    this.registerFileLinks(term);
    term.open(this.body().nativeElement);
    // Inline images (sixel and iTerm); live only, the server snapshot carries none. Limits keep a
    // flood of images from growing the webview: 16 Mpixel per image, 32 MB of stored images.
    // Loaded lazily so it stays out of the initial bundle; images only arrive after the stream.
    void import("@xterm/addon-image").then(({ ImageAddon }) => {
      if (this.term === term) term.loadAddon(new ImageAddon(IMAGE_OPTIONS));
    });
    try {
      term.loadAddon(new WebglAddon());
    } catch {
      // DOM renderer fallback
    }
    // App shortcuts belong to the app: keep xterm from forwarding them to the pane. Only Cmd
    // shortcuts match here; other keys go to the program in the pane.
    term.attachCustomKeyEventHandler((event) => {
      // Cmd shortcuts only match in a terminal; the find key is not one outside macOS, so it is
      // taken here and never reaches the PTY.
      if (event.type === "keydown" && this.keymap.match(event, false) === "find") {
        this.openSearch();
        return false;
      }
      if (this.keymap.match(event, true) !== null) return false;
      return !this.clipboardKey(event);
    });
    term.onData((data) => {
      if (this.mode() === "control") this.session?.input(data);
    });
    this.term = term;
    this.fit = fit;
    this.detachClipboard = this.attachClipboard(this.body().nativeElement);
    void this.listenForDrops();
    this.resizeObserver = new ResizeObserver(() => this.scheduleResize());
    this.resizeObserver.observe(this.body().nativeElement);
  }

  /** Files dropped on this view type their quoted paths as a paste (#905). */
  private async listenForDrops(): Promise<void> {
    try {
      const unlisten = await this.tauri.onDragDrop((event) => this.onDragDrop(event));
      if (this.destroyed) unlisten();
      else this.unlistenDrop = unlisten;
    } catch {
      // No drag-and-drop outside the app (tests, browser).
    }
  }

  private onDragDrop(event: DragDrop): void {
    if (event.type === "leave") {
      this.dropHover.set(false);
      return;
    }
    const over = overRect(
      event.position,
      this.host.nativeElement.getBoundingClientRect(),
      window.devicePixelRatio || 1,
    );
    if (event.type !== "drop") {
      this.dropHover.set(over);
      return;
    }
    this.dropHover.set(false);
    if (!over) return;
    const action = dropAction(event.paths, {
      remote: !!this.pane().machine,
      control: this.mode() === "control" && !!this.session?.open,
      windows: isWindows(),
    });
    if (!action) return;
    if (action.kind === "refuse") this.injector.get(SpaceActions).notify(action.notice);
    else this.term?.paste(action.text);
  }

  /** Shows the search bar (a second call selects the query) and moves the focus into it. */
  openSearch(): void {
    this.searchOpen.set(true);
    afterNextRender(
      () => {
        const input = this.host.nativeElement.querySelector<HTMLInputElement>(".search-input");
        input?.focus();
        input?.select();
      },
      { injector: this.injector },
    );
    if (this.searchQuery()) this.find(true);
  }

  /** Hides the bar, clears the highlights and gives the focus back to the terminal. */
  closeSearch(): void {
    this.searchOpen.set(false);
    this.searchAddon?.clearDecorations();
    this.searchResult.set({ index: -1, count: 0 });
    this.term?.focus();
  }

  protected onSearchFocus(): void {
    this.search.focused(this);
  }

  /** Focus left this view (not just moved inside it): Cmd/Ctrl+F is not ours any more. */
  protected onSearchBlur(event: FocusEvent): void {
    const next = event.relatedTarget;
    if (next instanceof Node && (event.currentTarget as Node | null)?.contains(next)) return;
    this.search.gone(this);
  }

  protected setQuery(value: string): void {
    this.searchQuery.set(value);
    this.find(true, true);
  }

  protected toggleCase(): void {
    this.searchCase.update((v) => !v);
    this.find(true);
  }

  protected toggleRegex(): void {
    this.searchRegex.update((v) => !v);
    this.find(true);
  }

  /** The next (or previous) match; `incremental` keeps the current one while it still matches. */
  protected find(forward: boolean, incremental = false): void {
    const addon = this.searchAddon;
    if (!addon) return;
    const query = this.searchQuery();
    if (!query) {
      addon.clearDecorations();
      this.searchResult.set({ index: -1, count: 0 });
      return;
    }
    const options = {
      caseSensitive: this.searchCase(),
      regex: this.searchRegex(),
      incremental,
      decorations: searchDecorations(cssVar, {
        selection: this.prefs.settings().colors?.selectionBackground ?? undefined,
      }),
    };
    if (forward) addon.findNext(query, options);
    else addon.findPrevious(query, options);
  }

  protected searchKey(event: KeyboardEvent): void {
    event.stopPropagation(); // never to the PTY or the app shortcuts
    if (event.key === "Escape") {
      event.preventDefault();
      this.closeSearch();
    } else if (event.key === "Enter") {
      event.preventDefault();
      this.find(!event.shiftKey);
    }
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
      const data = event.clipboardData;
      const image = data ? pastedImageType(Array.from(data.items ?? [], (i) => i.type)) : null;
      if (image) {
        const blob = Array.from(data!.items, (i) => i)
          .find((i) => i.type === image)
          ?.getAsFile();
        if (blob) void this.pasteImage(blob, image);
        return;
      }
      const text = data?.getData("text/plain");
      if (text && this.mode() === "control" && this.session?.open) this.term?.paste(text);
    };
    host.addEventListener("copy", onCopy, true);
    host.addEventListener("paste", onPaste, true);
    return () => {
      host.removeEventListener("copy", onCopy, true);
      host.removeEventListener("paste", onPaste, true);
    };
  }

  /** A pasted image is saved by the backend and its quoted path typed like a drop (#906). */
  private async pasteImage(blob: Blob, mime: string): Promise<void> {
    const notify = (text: string) => this.injector.get(SpaceActions).notify(text);
    const refusal = imagePasteRefusal({
      remote: !!this.pane().machine,
      control: this.mode() === "control" && !!this.session?.open,
    });
    if (refusal) return notify(refusal);
    if (!savableImage(mime))
      return notify("This image type cannot be pasted (png, jpeg, gif, webp).");
    try {
      const bytes = new Uint8Array(await blob.arrayBuffer());
      const path = await this.tauri.invokeRaw<string>("paste_image", bytes, { "x-mime": mime });
      if (this.destroyed || this.mode() !== "control" || !this.session?.open) return;
      this.term?.paste(quotePaths([path], isWindows()));
    } catch (e) {
      notify(`The pasted image was not saved: ${e}`);
    }
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
    this.detachClipboard?.();
    this.detachClipboard = undefined;
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
        // A snapshot follows as bytes. Reset first, so a reattach does not stack the snapshot
        // (and its scrollback) on the old screen; the grid is the server's until the
        // controller has refit.
        this.term?.reset();
        this.term?.resize(record.cols, record.rows);
        this.refitAfterSnapshot = this.attempt === "control";
        return;
      case "bytes":
        this.framed = true;
        this.writeBytes(record.data);
        return;
      case "resized":
        this.ptySize = { cols: record.cols, rows: record.rows };
        if (this.attempt !== "control") {
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

  /** Lines of scrollback: xterm.js keeps its own. */
  private applyScrollback(): void {
    if (!this.term) return;
    this.term.options.scrollback = this.prefs.settings().scrollback;
    // The terminal is a ConPTY on Windows (the poimen server runs on this machine).
    this.term.options.windowsPty = windowsPtyOption();
  }

  private scheduleResize(): void {
    clearTimeout(this.resizeTimer);
    this.resizeTimer = setTimeout(() => this.applySize(), RESIZE_DEBOUNCE_MS);
  }

  /** Refits, and when cols/rows changed tells the status bar and the server. */
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
    } else {
      this.resizeObserved(session, cols, rows);
    }
  }

  /**
   * An observer sets its terminal's PTY size while no controller holds it, so an
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
