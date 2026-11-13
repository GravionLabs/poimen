import { Injectable, computed, inject, signal } from "@angular/core";
import type { GhError } from "../github/pulls";
import { Machines } from "../core/machines";
import { TauriBridge } from "../core/tauri";
import { UiState } from "../core/ui-state";
import type { TaskMode, TaskModes } from "../core/settings";

/** An open issue as `task_issues` returns it (`OpenIssue` of `github.rs`). */
export interface PickIssue {
  number: number;
  title: string;
  level: "epic" | "feature" | "pbi" | "bug" | null;
  labels: string[];
  assignees: string[];
  url: string;
}

/** What the dialog returns. `title` and `url` are null for a number typed by hand. `machine` is a machine name, absent for this Mac. */
export interface TaskChoice {
  machine?: string;
  number: number;
  title: string | null;
  url: string | null;
  mode: string;
}

export const LEVEL_FILTERS = ["epic", "feature", "pbi", "bug"] as const;

/** Markers shown next to an issue; they never hide it. */
export function markers(issue: Pick<PickIssue, "labels" | "assignees">): string[] {
  const out = ["needs-refinement", "needs-decision"]
    .filter((m) => issue.labels.includes(m))
    .map((m) => `[${m}]`);
  return [...out, ...issue.assignees.map((a) => `[@${a}]`)];
}

/** Issues matching the text (title, labels, or the number as `12` / `#12`) and the level. */
export function filterIssues(
  issues: readonly PickIssue[],
  text: string,
  level: PickIssue["level"],
): PickIssue[] {
  const q = text.trim().toLowerCase().replace(/^#/, "");
  return issues.filter(
    (i) =>
      (!level || i.level === level) &&
      (!q ||
        String(i.number).includes(q) ||
        i.title.toLowerCase().includes(q) ||
        i.labels.some((l) => l.toLowerCase().includes(q))),
  );
}

/** The issue picker: lists the open issues of a directory's repository, returns the choice. */
@Injectable({ providedIn: "root" })
export class TaskPicker {
  private readonly bridge = inject(TauriBridge);
  private readonly machineList = inject(Machines);
  private readonly ui = inject(UiState);
  private resolve: ((choice: TaskChoice | null) => void) | null = null;
  private dir = "";

  readonly isOpen = signal(false);
  readonly loading = signal(false);
  readonly issues = signal<PickIssue[]>([]);
  readonly error = signal<GhError | null>(null);
  readonly modes = signal<TaskMode[]>([]);
  readonly mode = signal(0);
  readonly query = signal("");
  readonly level = signal<PickIssue["level"]>(null);
  readonly index = signal(0);

  /** The machine to run on: a connected machine's name, `null` for this Mac. */
  readonly machine = signal<string | null>(null);
  /** The machines a task can start on: the connected ones (this Mac is always there). */
  readonly machines = computed(() =>
    this.machineList
      .machines()
      .filter((m) => m.state === "connected")
      .map((m) => m.name),
  );

  readonly visible = computed(() => filterIssues(this.issues(), this.query(), this.level()));
  readonly activeIssue = computed(() => this.visible()[this.index()] ?? null);
  readonly modeName = computed(() => this.modes()[this.mode()]?.name ?? "");
  /** A number typed by hand that no listed issue has: usable even when the list is unavailable. */
  readonly typedNumber = computed(() => {
    const m = /^#?(\d+)$/.exec(this.query().trim());
    const n = m ? Number(m[1]) : 0;
    return n > 0 && !this.issues().some((i) => i.number === n) ? n : null;
  });

  /** Opens the dialog for the repository of `dir` (with issue `preselect` highlighted); resolves with the choice, null when cancelled. */
  async open(dir: string, preselect?: number): Promise<TaskChoice | null> {
    this.cancel();
    this.dir = dir;
    const last = this.ui.taskMachine()[dir];
    this.machine.set(last && this.machines().includes(last) ? last : null);
    this.query.set("");
    this.level.set(null);
    this.index.set(0);
    this.issues.set([]);
    this.error.set(null);
    this.loading.set(true);
    this.isOpen.set(true);
    const result = new Promise<TaskChoice | null>((r) => (this.resolve = r));
    void this.load(dir, preselect);
    return result;
  }

  private async load(dir: string, preselect?: number): Promise<void> {
    const [issues, modes] = await Promise.allSettled([
      this.bridge.invoke<PickIssue[]>("task_issues", { dir }),
      this.bridge.invoke<TaskModes>("get_task_modes", { dir }),
    ]);
    if (!this.isOpen()) return;
    if (issues.status === "fulfilled") this.issues.set(issues.value ?? []);
    else this.error.set(toGhError(issues.reason));
    this.modes.set(modes.status === "fulfilled" ? (modes.value?.modes ?? []) : []);
    this.mode.set(0);
    if (preselect) this.select(preselect);
    this.loading.set(false);
  }

  /** Highlights the listed issue `nr`; one the list lacks is put in the filter as a typed number. */
  private select(nr: number): void {
    const at = this.visible().findIndex((i) => i.number === nr);
    if (at >= 0) this.index.set(at);
    else this.query.set(String(nr));
  }

  setQuery(text: string): void {
    this.query.set(text);
    this.index.set(0);
  }

  setLevel(level: PickIssue["level"]): void {
    this.level.set(this.level() === level ? null : level);
    this.index.set(0);
  }

  move(delta: number): void {
    const n = this.visible().length;
    if (n > 0) this.index.set((this.index() + delta + n) % n);
  }

  setMachine(name: string | null): void {
    this.machine.set(name && this.machines().includes(name) ? name : null);
  }

  toggleMode(): void {
    const n = this.modes().length;
    if (n > 0) this.mode.set((this.mode() + 1) % n);
  }

  /** Picks `issue`, else the highlighted issue, else the typed number. */
  confirm(issue: PickIssue | null = this.activeIssue()): void {
    const mode = this.modeName();
    const typed = this.typedNumber();
    const machine = this.machine();
    const where = machine ? { machine } : {};
    let choice: TaskChoice | null = null;
    if (issue)
      choice = { number: issue.number, title: issue.title, url: issue.url, mode, ...where };
    else if (typed) choice = { number: typed, title: null, url: null, mode, ...where };
    if (!choice) return;
    this.ui.setTaskMachine(this.dir, machine);
    this.finish(choice);
  }

  cancel(): void {
    this.finish(null);
  }

  private finish(choice: TaskChoice | null): void {
    const resolve = this.resolve;
    this.resolve = null;
    this.isOpen.set(false);
    this.loading.set(false);
    resolve?.(choice);
  }
}

function toGhError(reason: unknown): GhError {
  if (reason && typeof reason === "object" && "kind" in reason) return reason as GhError;
  return { kind: "other", message: String(reason) };
}

/** The reason line for a failed list, with the hint that fixes it. */
export function errorText(error: GhError): string {
  switch (error.kind) {
    case "gh-missing":
      return "The GitHub CLI (gh) is not installed.";
    case "not-logged-in":
      return "Not logged in to GitHub. Run `gh auth login`.";
    case "offline":
      return "GitHub is not reachable (offline?).";
    case "rate-limited":
      return "GitHub rate limit reached.";
    case "not-found":
      return "Repository not found on GitHub.";
    default:
      return error.message;
  }
}
