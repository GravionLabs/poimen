import { ChangeDetectionStrategy, Component, computed, inject } from "@angular/core";
import {
  appearance,
  pickSchemeName,
  type Appearance,
  type ColorScheme,
} from "../core/color-scheme";
import { ColorSchemeService } from "../core/color-scheme-service";
import { SettingsStore, type ColorSchemeSetting, type ThemeSetting } from "../core/settings";
import { TauriBridge } from "../core/tauri";
import { UserSchemes } from "../core/user-schemes";
import { SpaceLayoutEditor } from "./space-layout-editor";

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

/** `path` as the backend stores it: below `home` as `~` or `~/…`, without a trailing slash. */
function tilded(path: string, home: string | null): string {
  const p = path.length > 1 ? path.replace(/\/+$/, "") : path;
  const h = home && home.length > 1 ? home.replace(/\/+$/, "") : home;
  if (!h) return p;
  if (p === h) return "~";
  return p.startsWith(`${h}/`) ? `~${p.slice(h.length)}` : p;
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
  imports: [SpaceLayoutEditor],
  templateUrl: "./settings-view.html",
  styleUrl: "./settings-view.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class SettingsView {
  private readonly store = inject(SettingsStore);
  private readonly schemes = inject(ColorSchemeService);
  private readonly user = inject(UserSchemes);
  private readonly bridge = inject(TauriBridge);

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

  protected setShowHidden(event: Event): void {
    void this.store.setExplorerShowHidden((event.target as HTMLInputElement).checked);
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
