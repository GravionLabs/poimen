import {
  ChangeDetectionStrategy,
  Component,
  computed,
  effect,
  inject,
  signal,
  untracked,
} from "@angular/core";
import { parseHarnesses, SettingsStore, type HarnessInfo } from "../core/settings";
import {
  addPane,
  parseSpaceLayout,
  removePane,
  updatePane,
  type SpaceLayout,
} from "../core/space-layout";
import {
  emptyTemplate,
  hasErrors,
  isDefaultTemplate,
  matchLabel,
  templateErrors,
  type RemoteMatch,
  type SpaceTemplate,
} from "../core/space-templates";
import { TauriBridge } from "../core/tauri";

/** Where the layout of the draft started: none, a source, or edited by hand. */
type LayoutSource = "none" | "default" | "user" | "custom" | `preset:${string}`;

interface Editing {
  /** Index in `spaceTemplates`; `null` for a new template. */
  index: number | null;
  draft: SpaceTemplate;
  /** The default template: the form has no directory and no remote. */
  asDefault: boolean;
}

/** `dir` with a leading `~` made absolute with `home`; other paths stay as they are. */
function expandHome(dir: string, home: string | null): string | null {
  if (dir === "~" || dir.startsWith("~/")) return home ? home + dir.slice(1) : null;
  return dir;
}

function value(event: Event): string {
  return (event.target as HTMLInputElement | HTMLSelectElement).value;
}

/**
 * Settings page "Spaces": lists the space templates of the user config and edits them one at a
 * time in a form. Every change writes only `spaceTemplates`.
 */
@Component({
  selector: "app-space-templates-editor",
  templateUrl: "./space-templates-editor.html",
  styleUrl: "./settings-view.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class SpaceTemplatesEditor {
  private readonly store = inject(SettingsStore);
  private readonly bridge = inject(TauriBridge);

  protected readonly settings = this.store.settings;
  protected readonly writeError = this.store.writeError;
  protected readonly templates = computed(() => this.settings().spaceTemplates);
  /** What the backend said about `spaceTemplates` while loading the config. */
  protected readonly warnings = computed(() =>
    this.settings().warnings.filter((w) => w.startsWith("spaceTemplates")),
  );
  protected readonly harnesses = signal<HarnessInfo[]>([]);
  protected readonly editing = signal<Editing | null>(null);
  protected readonly source = signal<LayoutSource>("none");
  private readonly builtIn = signal<SpaceLayout | null>(null);
  private readonly presets = signal<{ name: string; layout: SpaceLayout }[]>([]);

  /** The default template (no directory and no remote), when there is one. */
  protected readonly defaultTemplate = computed(
    () => this.templates().find(isDefaultTemplate) ?? null,
  );
  /** The templates other than the default, with their index in `spaceTemplates`. */
  protected readonly listed = computed(() =>
    this.templates().flatMap((t, i) => (isDefaultTemplate(t) ? [] : [{ t, i }])),
  );

  protected readonly errors = computed(() => {
    const e = this.editing();
    if (!e) return {};
    const others = this.templates()
      .filter((_, i) => i !== e.index)
      .map((t) => t.name);
    const defaultTaken = this.templates().some((t, i) => i !== e.index && isDefaultTemplate(t));
    return templateErrors(
      e.draft,
      others,
      this.harnesses().map((h) => h.id),
      defaultTaken,
    );
  });
  protected readonly valid = computed(() => !hasErrors(this.errors()));
  protected readonly presetNames = computed(() => this.presets().map((p) => p.name));
  protected readonly harness = computed(() =>
    this.harnesses().find((h) => h.id === this.editing()?.draft.agent),
  );
  protected readonly matchLabel = matchLabel;

  constructor() {
    void this.store.defaultSpacesLayout().then((l) => this.builtIn.set(l));
    // The harnesses come from config.json, so they are read again when the settings change.
    effect(() => {
      this.settings();
      void this.bridge
        .invoke<unknown>("get_harnesses")
        .then((raw) => this.harnesses.set(parseHarnesses(raw)))
        .catch(() => this.harnesses.set([]));
    });
  }

  protected add(): void {
    const first = this.harnesses().find((h) => h.available) ?? this.harnesses()[0];
    this.open(null, emptyTemplate(first?.id ?? ""), false);
  }

  protected edit(index: number): void {
    const draft = structuredClone(this.templates()[index]);
    this.open(index, draft, isDefaultTemplate(draft));
  }

  /** Edits the default template in the form without a directory and a remote. */
  protected editDefault(): void {
    const index = this.templates().findIndex(isDefaultTemplate);
    if (index >= 0) this.edit(index);
  }

  /** Adds the default template: no match, named `default`. */
  protected addDefault(): void {
    const first = this.harnesses().find((h) => h.available) ?? this.harnesses()[0];
    this.open(null, { ...emptyTemplate(first?.id ?? ""), name: "default" }, true);
  }

  protected removeDefault(): void {
    const index = this.templates().findIndex(isDefaultTemplate);
    if (index >= 0) void this.remove(index);
  }

  private open(index: number | null, draft: SpaceTemplate, asDefault = false): void {
    this.source.set(draft.layout ? "custom" : "none");
    this.presets.set([]);
    this.editing.set({ index, draft, asDefault });
    void this.loadPresets();
  }

  protected cancel(): void {
    this.editing.set(null);
  }

  protected async save(): Promise<void> {
    const e = this.editing();
    if (!e || !this.valid()) return;
    const list = [...this.templates()];
    if (e.index === null) list.push(e.draft);
    else list[e.index] = e.draft;
    if (await this.store.setSpaceTemplates(list)) this.editing.set(null);
  }

  protected async remove(index: number): Promise<void> {
    const e = this.editing();
    await this.store.setSpaceTemplates(this.templates().filter((_, i) => i !== index));
    // Editing a template that is gone (or whose index moved) would write the wrong entry.
    if (e && e.index !== null) this.editing.set(null);
  }

  private patch(change: Partial<SpaceTemplate>): void {
    this.editing.update((e) => (e ? { ...e, draft: { ...e.draft, ...change } } : e));
  }

  protected setName(event: Event): void {
    this.patch({ name: value(event) });
  }

  protected setDir(event: Event): void {
    this.patch({ dir: value(event) });
    void this.loadPresets();
  }

  protected async browse(): Promise<void> {
    const dir = await this.bridge.pickDirectory();
    if (dir) {
      this.patch({ dir });
      void this.loadPresets();
    }
  }

  protected setProvider(event: Event): void {
    const provider = value(event);
    this.patch({
      remote:
        provider === "github"
          ? { provider, owner: "" }
          : provider === "azure-devops"
            ? { provider, organization: "", project: "" }
            : null,
    });
  }

  protected setRemoteField(field: "owner" | "organization" | "project", event: Event): void {
    const remote = this.editing()?.draft.remote;
    if (remote) this.patch({ remote: { ...remote, [field]: value(event) } as RemoteMatch });
  }

  protected setAgent(event: Event): void {
    this.patch({ agent: value(event), model: null });
  }

  protected setModel(event: Event): void {
    this.patch({ model: value(event) || null });
  }

  /** Models of the chosen harness, plus the draft's own when the harness does not list it. */
  protected models(): string[] {
    const model = this.editing()?.draft.model;
    const listed = this.harness()?.models ?? [];
    return model && !listed.includes(model) ? [...listed, model] : listed;
  }

  protected agentKnown(): boolean {
    return this.harness() !== undefined;
  }

  protected setSource(event: Event): void {
    const source = value(event) as LayoutSource;
    const layout =
      source === "none"
        ? null
        : source === "default"
          ? this.builtIn()
          : source === "user"
            ? this.settings().spacesLayout
            : source.startsWith("preset:")
              ? (this.presets().find((p) => p.name === source.slice(7))?.layout ?? null)
              : null;
    if (source === "custom") return;
    this.source.set(layout ? source : "none");
    this.patch({ layout: layout ? structuredClone(layout) : null });
  }

  protected setPaneName(index: number, event: Event): void {
    this.changePanes((p) => updatePane(p, index, { name: value(event) }));
  }

  protected setPaneCommand(index: number, event: Event): void {
    const command = value(event);
    this.changePanes((p) => updatePane(p, index, { command: command === "" ? null : command }));
  }

  protected addPane(): void {
    this.changePanes(addPane);
  }

  protected removePane(index: number): void {
    this.changePanes((p) => removePane(p, index));
  }

  private changePanes(change: (panes: SpaceLayout["panes"]) => SpaceLayout["panes"]): void {
    const layout = this.editing()?.draft.layout;
    if (!layout) return;
    this.source.set("custom");
    this.patch({ layout: { panes: change(layout.panes) } });
  }

  /** The project presets of the repository of the draft's directory, when it is known. */
  private async loadPresets(): Promise<void> {
    const dir = this.editing()?.draft.dir?.trim();
    const expanded = dir ? expandHome(dir, untracked(this.settings).home) : null;
    if (!expanded) {
      this.presets.set([]);
      return;
    }
    try {
      const raw = await this.bridge.invoke<{ config?: { presets?: unknown } }>("project_config", {
        dir: expanded,
      });
      if (this.editing()?.draft.dir?.trim() !== dir) return;
      const list = Array.isArray(raw?.config?.presets) ? raw.config.presets : [];
      this.presets.set(
        list.flatMap((p: unknown) => {
          const r = p as Record<string, unknown>;
          const layout = parseSpaceLayout(r?.["layout"]);
          return typeof r?.["name"] === "string" && layout ? [{ name: r["name"], layout }] : [];
        }),
      );
    } catch {
      this.presets.set([]);
    }
  }
}
