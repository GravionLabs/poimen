import {
  ChangeDetectionStrategy,
  Component,
  computed,
  effect,
  ElementRef,
  HostListener,
  inject,
  signal,
  untracked,
} from "@angular/core";
import {
  appearance,
  pickSchemeName,
  type Appearance,
  type ColorScheme,
} from "../core/color-scheme";
import { ColorSchemeService } from "../core/color-scheme-service";
import {
  defaultTerminalFontFamily,
  defaultTerminalFontSize,
  parseHarnesses,
  parseAgentActions,
  type AgentAction,
  SettingsStore,
  type ColorSchemeSetting,
  type HarnessInfo,
  type ThemeSetting,
} from "../core/settings";
import { HooksOffer, agentName } from "../core/hooks-offer";
import { TauriBridge } from "../core/tauri";
import { UiState } from "../core/ui-state";
import { UserSchemes } from "../core/user-schemes";
import {
  allPages,
  DEFAULT_SETTINGS_PAGE,
  filterGroups,
  findPage,
  matches,
  SETTINGS_GROUPS,
} from "./settings-categories";
import {
  COMMON_MONO_FONTS,
  COMMON_UI_FONTS,
  MAX_TERMINAL_FONT_SIZE,
  MAX_UI_FONT_SIZE,
  MIN_TERMINAL_FONT_SIZE,
  MIN_UI_FONT_SIZE,
} from "../core/ui-font";
import { SpaceLayoutEditor } from "./space-layout-editor";
import { SpaceTemplatesEditor } from "./space-templates-editor";

export type SchemeMode = "single" | "pair";

/** The scheme used for a side of a pair that has none yet. */
const DEFAULT_SIDE: Record<Appearance, string> = { light: "poimen-light", dark: "poimen-dark" };

export const THEME_CHOICES: readonly { value: ThemeSetting; label: string }[] = [
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
  { value: "system", label: "System" },
];

/** The schemes by appearance, each group sorted by label. */
export function groupSchemes(
  registry: ReadonlyMap<string, ColorScheme>,
): Record<Appearance, ColorScheme[]> {
  const groups: Record<Appearance, ColorScheme[]> = { dark: [], light: [] };
  for (const scheme of registry.values()) groups[appearance(scheme)].push(scheme);
  for (const list of Object.values(groups)) list.sort((a, b) => a.label.localeCompare(b.label));
  return groups;
}

/** The scheme name that is selected in the list of `side`, or `null`. */
export function selectedIn(
  schema: ColorSchemeSetting | null,
  side: Appearance,
  registry: ReadonlyMap<string, ColorScheme>,
): string | null {
  if (schema === null) return null;
  if (typeof schema === "object") return schema[side];
  const scheme = registry.get(schema);
  return scheme && appearance(scheme) === side ? schema : null;
}

/** A pair that keeps a single scheme on its own side and the poimen colors on the other. */
export function pairOf(
  schema: ColorSchemeSetting | null,
  side: Appearance,
): { light: string; dark: string } {
  if (typeof schema === "object" && schema !== null) return schema;
  const pair = { ...DEFAULT_SIDE };
  if (typeof schema === "string") pair[side] = schema;
  return pair;
}

/** The `colorSchema` after choosing `name` in the list of `side`. */
export function chosen(
  schema: ColorSchemeSetting | null,
  mode: SchemeMode,
  side: Appearance,
  name: string,
): ColorSchemeSetting {
  return mode === "single" ? name : { ...pairOf(schema, side), [side]: name };
}

/** `list` with the item at `index` moved by `delta` places (clamped); a copy. */
export function moved<T>(list: readonly T[], index: number, delta: number): T[] {
  const next = [...list];
  const target = Math.min(Math.max(index + delta, 0), next.length - 1);
  const [item] = next.splice(index, 1);
  next.splice(target, 0, item);
  return next;
}

const WINDOWS_ABSOLUTE = /^[A-Za-z]:[\\/]/;

/**
 * `path` as the backend stores it: below `home` as `~` or `~/…`, without a trailing slash. With a
 * Windows home (`C:\Users\me`) both separators match and the comparison ignores case.
 */
function tilded(path: string, home: string | null): string {
  const windows = !!home && WINDOWS_ABSOLUTE.test(home);
  const norm = (s: string) => {
    const n = windows ? s.replaceAll("\\", "/") : s;
    return n.length > 1 ? n.replace(/\/+$/, "") : n;
  };
  const p = norm(path);
  const h = home ? norm(home) : home;
  if (!h) return p;
  const same = (a: string, b: string) => (windows ? a.toLowerCase() === b.toLowerCase() : a === b);
  if (same(p, h)) return "~";
  const head = p.slice(0, h.length + 1);
  return same(head, `${h}/`) ? `~${p.slice(h.length)}` : p;
}

/** `roots` with `dir` appended, unless it is already listed (`~/…` and absolute spellings match). */
export function withRoot(
  roots: readonly string[],
  dir: string,
  home: string | null = null,
): string[] {
  const seen = roots.map((r) => tilded(r, home));
  return seen.includes(tilded(dir, home)) ? [...roots] : [...roots, dir];
}

/** The settings view (an editor tab): appearance and the color scheme picker with live preview. */
@Component({
  selector: "app-settings-view",
  imports: [SpaceLayoutEditor, SpaceTemplatesEditor],
  templateUrl: "./settings-view.html",
  styleUrl: "./settings-view.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class SettingsView {
  private readonly store = inject(SettingsStore);
  private readonly schemes = inject(ColorSchemeService);
  private readonly user = inject(UserSchemes);
  private readonly bridge = inject(TauriBridge);
  private readonly ui = inject(UiState);
  protected readonly hooks = inject(HooksOffer);
  protected readonly agentName = agentName;

  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  /** The search text; filters the category tree and highlights the labels on the page. */
  protected readonly query = signal("");
  protected readonly visibleGroups = computed(() => filterGroups(this.query()));
  /** The selected page; the first one when the stored id no longer exists. */
  protected readonly current = computed(
    () =>
      findPage(this.ui.settingsPage()) ??
      findPage(DEFAULT_SETTINGS_PAGE) ??
      findPage(allPages()[0].id)!,
  );
  /** The tree node with the tab stop (`g:<group>` or `p:<page>`); the selected page by default. */
  private readonly focusedNode = signal<string | null>(null);
  protected readonly tabStopNode = computed(() => {
    const focused = this.focusedNode();
    const nodes = this.nodes();
    return focused !== null && nodes.includes(focused) ? focused : `p:${this.current().page.id}`;
  });

  /** The visible tree nodes in order; a search expands every group that has a match. */
  private readonly nodes = computed(() =>
    this.visibleGroups().flatMap((g) => [
      `g:${g.id}`,
      ...(this.expanded(g.id) ? g.pages.map((p) => `p:${p.id}`) : []),
    ]),
  );

  protected expanded(groupId: string): boolean {
    return this.query().trim() !== "" || !this.ui.settingsCollapsed().has(groupId);
  }

  protected selectPage(id: string): void {
    this.ui.selectSettingsPage(id);
  }

  protected toggleGroup(id: string): void {
    this.ui.setSettingsGroupCollapsed(id, this.expanded(id) && this.query().trim() === "");
  }

  protected setQuery(event: Event): void {
    this.query.set((event.target as HTMLInputElement).value);
  }

  /** Whether `label` matches the search, so the page highlights it. */
  protected hit(label: string): boolean {
    return matches(label, this.query());
  }

  /** Cmd/Ctrl+F in the tab moves to the search field. */
  @HostListener("keydown", ["$event"])
  protected onViewKey(event: KeyboardEvent): void {
    const search = this.host.nativeElement.querySelector<HTMLInputElement>("input.search");
    if (
      search &&
      (event.metaKey || event.ctrlKey) &&
      !event.altKey &&
      event.key.toLowerCase() === "f"
    ) {
      event.preventDefault();
      search.focus();
      search.select();
    }
  }

  /** Escape in the search field clears it. */
  protected searchKey(event: KeyboardEvent): void {
    if (event.key === "Escape" && this.query() !== "") {
      event.stopPropagation();
      this.query.set("");
      (event.target as HTMLInputElement).value = "";
    }
  }

  /** Arrow keys, Home/End, Left/Right and Enter/Space on the tree. */
  protected treeKey(event: KeyboardEvent, node: string): void {
    const nodes = this.nodes();
    const at = nodes.indexOf(node);
    const [kind, id] = [node.slice(0, 1), node.slice(2)];
    const focus = (target: string | undefined) => {
      if (!target) return;
      event.preventDefault();
      this.focusedNode.set(target);
      (event.currentTarget as HTMLElement)
        .closest('[role="tree"]')
        ?.querySelector<HTMLElement>(`[data-node="${target}"]`)
        ?.focus();
    };
    switch (event.key) {
      case "ArrowDown":
        return focus(nodes[Math.min(at + 1, nodes.length - 1)]);
      case "ArrowUp":
        return focus(nodes[Math.max(at - 1, 0)]);
      case "Home":
        return focus(nodes[0]);
      case "End":
        return focus(nodes[nodes.length - 1]);
      case "ArrowRight":
        if (kind !== "g") return;
        if (!this.expanded(id)) {
          event.preventDefault();
          this.toggleGroup(id);
        } else {
          focus(nodes[at + 1]?.startsWith("p:") ? nodes[at + 1] : undefined);
        }
        return;
      case "ArrowLeft":
        if (kind === "g") {
          if (this.expanded(id) && this.query().trim() === "") {
            event.preventDefault();
            this.toggleGroup(id);
          }
        } else {
          const group = SETTINGS_GROUPS.find((g) => g.pages.some((p) => p.id === id));
          if (group) focus(`g:${group.id}`);
        }
        return;
      case "Enter":
      case " ":
        event.preventDefault();
        if (kind === "p") this.selectPage(id);
        else this.toggleGroup(id);
        return;
    }
  }

  protected readonly harnesses = signal<HarnessInfo[]>([]);
  protected readonly agentActions = signal<AgentAction[]>([]);

  constructor() {
    effect(() => {
      if (this.current().page.id === "herdr") untracked(() => void this.hooks.refresh());
    });
    // The harnesses come from config.json, so they are read again when the settings change.
    effect(() => {
      this.settings();
      void this.bridge
        .invoke<unknown>("get_harnesses")
        .then((raw) => this.harnesses.set(parseHarnesses(raw)))
        .catch(() => this.harnesses.set([]));
      void this.bridge
        .invoke<unknown>("get_agent_actions", { root: null })
        .then((raw) => this.agentActions.set(parseAgentActions(raw)))
        .catch(() => this.agentActions.set([]));
    });
  }

  protected readonly themeChoices = THEME_CHOICES;
  protected readonly settings = this.store.settings;
  protected readonly groups = computed(() => groupSchemes(this.user.registry()));
  protected readonly sides: readonly Appearance[] = ["dark", "light"];
  protected readonly mode = computed<SchemeMode>(() =>
    typeof this.settings().colorSchema === "object" ? "pair" : "single",
  );
  /** A saved single scheme fixes light or dark, so the appearance has no effect. */
  protected readonly appearanceFixed = computed(
    () => this.mode() === "single" && this.settings().colorSchema !== null,
  );

  protected selected(side: Appearance): string | null {
    return selectedIn(this.settings().colorSchema, side, this.user.registry());
  }

  /** The option that takes the tab stop of a list: the selected one, else the first. */
  protected tabStop(side: Appearance, name: string, index: number): boolean {
    const selected = this.selected(side);
    return selected !== null && this.groups()[side].some((s) => s.name === selected)
      ? selected === name
      : index === 0;
  }

  protected setTheme(theme: ThemeSetting): void {
    void this.store.setTheme(theme);
  }

  protected readonly writeError = this.store.writeError;

  /** Adds a folder chosen in the native picker to `explorer.roots`. */
  protected async addRoot(): Promise<void> {
    const dir = await this.bridge.pickDirectory();
    if (dir)
      await this.store.setExplorerRoots(
        withRoot(this.settings().explorerRoots, dir, this.settings().home),
      );
  }

  protected removeRoot(index: number): void {
    const roots = this.settings().explorerRoots;
    if (roots.length > 1) void this.store.setExplorerRoots(roots.filter((_, i) => i !== index));
  }

  protected moveRoot(index: number, delta: number): void {
    void this.store.setExplorerRoots(moved(this.settings().explorerRoots, index, delta));
  }

  /** The repositories whose project file is trusted, sorted. */
  protected readonly trustedRoots = computed(() =>
    Object.keys(this.settings().trustedProjects).sort((a, b) => a.localeCompare(b)),
  );

  protected revokeProject(root: string): void {
    void this.store.revokeProject(root);
  }

  protected setShowHidden(event: Event): void {
    void this.store.setExplorerShowHidden((event.target as HTMLInputElement).checked);
  }

  protected readonly monoFonts = COMMON_MONO_FONTS;
  protected readonly uiFonts = COMMON_UI_FONTS;
  protected readonly uiSizeRange = [MIN_UI_FONT_SIZE, MAX_UI_FONT_SIZE] as const;
  /** The effective default terminal font, shown as placeholder and reset target. */
  protected readonly defaultTerminalFont = defaultTerminalFontFamily(
    getComputedStyle(document.documentElement).getPropertyValue("--font-mono").trim(),
  );
  protected readonly defaultTerminalSize = defaultTerminalFontSize();
  protected readonly terminalSizeRange = [MIN_TERMINAL_FONT_SIZE, MAX_TERMINAL_FONT_SIZE] as const;

  protected setFont(which: "terminal" | "ui", event: Event): void {
    const value = (event.target as HTMLInputElement).value;
    void (which === "terminal"
      ? this.store.setTerminalFontFamily(value)
      : this.store.setUiFontFamily(value));
  }

  protected setFontSize(which: "terminal" | "ui", event: Event): void {
    const input = event.target as HTMLInputElement;
    const empty = input.value.trim() === "";
    const size = empty ? Number.NaN : Number(input.value);
    // An empty terminal size removes the key: the platform default applies again.
    const write =
      which === "terminal"
        ? empty
          ? this.store.clearTerminalFontSize()
          : this.store.setTerminalFontSize(size)
        : this.store.setUiFontSize(size);
    void write.then((ok) => {
      if (!ok)
        input.value = String(this.settings()[which === "terminal" ? "fontSize" : "uiFontSize"]);
    });
  }

  protected setNotification(kind: "blocked" | "done", event: Event): void {
    void this.store.setNotification(kind, (event.target as HTMLInputElement).checked);
  }

  protected setAutoStart(event: Event): void {
    void this.store.setHerdrAutoStart((event.target as HTMLInputElement).checked);
  }

  protected setMode(mode: SchemeMode): void {
    const schema = this.settings().colorSchema;
    if (mode === "pair" && typeof schema !== "object") {
      const scheme = typeof schema === "string" ? this.user.registry().get(schema) : undefined;
      void this.store.setColorSchema(pairOf(schema, scheme ? appearance(scheme) : "dark"));
    } else if (mode === "single" && typeof schema === "object" && schema !== null) {
      const picked = pickSchemeName(schema, this.settings().theme, this.systemDark());
      void this.store.setColorSchema(picked ?? schema.dark);
    }
  }

  protected preview(name: string): void {
    this.schemes.preview(name);
  }

  protected save(side: Appearance, name: string): void {
    void this.store.setColorSchema(chosen(this.settings().colorSchema, this.mode(), side, name));
  }

  /** The mouse leaves the list: keep previewing the focused option, else restore the saved scheme. */
  protected leave(event: MouseEvent): void {
    const list = event.currentTarget as HTMLElement;
    const focused = list.querySelector<HTMLElement>('[role="option"]:focus');
    if (focused?.dataset["name"]) this.schemes.preview(focused.dataset["name"]);
    else this.schemes.revert();
  }

  /** Focus leaves the list: restore the saved scheme. */
  protected blur(event: FocusEvent): void {
    const list = event.currentTarget as HTMLElement;
    if (!(event.relatedTarget instanceof Node && list.contains(event.relatedTarget))) {
      this.schemes.revert();
    }
  }

  protected key(event: KeyboardEvent, side: Appearance, name: string, index: number): void {
    if (event.key === "Enter" || event.key === " ") {
      this.save(side, name);
      event.preventDefault();
      return;
    }
    const last = this.groups()[side].length - 1;
    const target =
      event.key === "ArrowDown"
        ? Math.min(index + 1, last)
        : event.key === "ArrowUp"
          ? Math.max(index - 1, 0)
          : event.key === "Home"
            ? 0
            : event.key === "End"
              ? last
              : null;
    if (target === null) return;
    event.preventDefault();
    const options = (
      event.currentTarget as HTMLElement
    ).parentElement?.querySelectorAll<HTMLElement>('[role="option"]');
    options?.[target]?.focus();
  }

  private systemDark(): boolean {
    return typeof matchMedia === "function" && matchMedia("(prefers-color-scheme: dark)").matches;
  }
}
