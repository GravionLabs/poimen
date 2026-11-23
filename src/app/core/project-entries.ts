import { Injectable, inject, signal } from "@angular/core";
import { AgentPicker } from "./agent-picker";
import { AgentPrompts, promptMessage, promptSucceeded } from "./agent-prompt";
import { parseHarnesses } from "./settings";
import { agentKind } from "./snapshot";
import { SpacesStore } from "./spaces-store";
import { SpaceActions, type TrustDecision, type TrustRequest } from "./space-actions";
import { TauriBridge } from "./tauri";

/** Wire shape of `Entries` (`project.rs`): names only; `locked` while the file is not trusted. */
export interface ProjectEntryList {
  commands: { name: string; target: "terminal" | "agent" }[];
  presets: string[];
  locked: boolean;
}

/** Wire shape of `DetectedEntry` (`poimen_core::detect`): a command found in the repository. */
export interface DetectedEntry {
  source: string;
  name: string;
  /** The command line of the script, for the user. */
  description: string;
}

/** The source of the skills of the repository (`poimen_core::detect::skills`). */
export const SKILLS_SOURCE = "skills";

/** Whether a detected entry is a skill: it is sent to an agent as `/<name>`, not run. */
export function isSkill(entry: DetectedEntry): boolean {
  return entry.source === SKILLS_SOURCE;
}

/** An entry of the project file picked in the palette (wire shape of `Entry`). */
export type ProjectEntry = { kind: "command"; name: string } | { kind: "preset"; name: string };

/** What the palette runs: an entry of the project file or a detected script. */
export type PaletteEntry = ProjectEntry | DetectedEntry;

/** Wire shape of `EntryOutcome` (`project.rs`). */
export type EntryOutcome =
  | { kind: "done" }
  | { kind: "agent"; command: string }
  | { kind: "ask"; request: TrustRequest }
  | { kind: "declined" }
  | { kind: "missing" };

const NONE: ProjectEntryList = { commands: [], presets: [], locked: false };

/**
 * The `commands[]` and `presets[]` of the project file of the focused space, for the palette, and
 * the runner of an entry. The backend decides trust (`project_run` goes through `trust::gate`);
 * nothing runs from here without it.
 */
@Injectable({ providedIn: "root" })
export class ProjectEntries {
  private readonly bridge = inject(TauriBridge);
  private readonly store = inject(SpacesStore);
  private readonly spaces = inject(SpaceActions);
  private readonly prompts = inject(AgentPrompts);
  private readonly picker = inject(AgentPicker);

  private readonly _entries = signal<ProjectEntryList>(NONE);
  readonly entries = this._entries.asReadonly();
  private readonly _detected = signal<DetectedEntry[]>([]);
  /** The commands detected in the repository (`package.json` scripts), for the Scripts tab. */
  readonly detected = this._detected.asReadonly();
  private dir: string | undefined;
  private seq = 0;

  /** The focused space's directory changed: reads its project file. */
  setDir(dir: string | undefined): void {
    if (dir === this.dir) return;
    this.dir = dir;
    void this.reload();
  }

  /** Reads the project file again (the palette opens, the file may have changed). */
  async reload(): Promise<void> {
    const dir = this.dir;
    const seq = ++this.seq;
    if (!dir) {
      this._entries.set(NONE);
      this._detected.set([]);
      return;
    }
    const [list, detected] = await Promise.all([
      this.bridge.invoke<ProjectEntryList>("project_entries", { dir }).then(
        (l) => l ?? NONE,
        () => NONE,
      ),
      this.bridge.invoke<DetectedEntry[]>("detected_entries", { dir }).then(
        (d) => d ?? [],
        () => [],
      ),
    ]);
    if (seq !== this.seq) return;
    this._entries.set(list);
    this._detected.set(detected);
  }

  /**
   * Runs the detected `entry` in space `workspaceId` at `dir`: a tab named after it with the
   * command typed. An explicit pick only; the backend builds the argv, no hash trust.
   */
  async runDetected(entry: DetectedEntry, workspaceId: string, dir: string): Promise<void> {
    const label = `Script: ${entry.name}`;
    try {
      const ran = await this.bridge.invoke<boolean>("detected_run", {
        spaceId: workspaceId,
        dir,
        source: entry.source,
        name: entry.name,
      });
      if (ran) {
        await this.store.refresh();
        return;
      }
      this.spaces.notify(`${label}: the script no longer exists`);
      await this.reload();
    } catch (err) {
      const reason = err && typeof err === "object" && "message" in err ? err.message : err;
      this.spaces.fail(`${label} failed: ${String(reason)}`);
    }
  }

  /** Runs a picked palette entry in space `workspaceId` at `dir`. */
  start(entry: PaletteEntry, workspaceId: string, dir: string): Promise<void> {
    if (!("source" in entry)) return this.run(entry, workspaceId, dir);
    return isSkill(entry)
      ? this.runSkill(entry, workspaceId)
      : this.runDetected(entry, workspaceId, dir);
  }

  /**
   * Sends `/<skill>` to an agent of the space whose harness takes slash skills (`slashSkills` of
   * the registry); several: the agent picker. An explicit pick only.
   */
  async runSkill(entry: DetectedEntry, workspaceId: string): Promise<void> {
    const harnesses = await this.bridge
      .invoke<unknown>("get_harnesses")
      .then(parseHarnesses, () => []);
    const kinds = new Set(harnesses.filter((h) => h.slashSkills).map((h) => h.id));
    await this.sendToAgent(
      `Skill: ${entry.name}`,
      `/${entry.name}`,
      workspaceId,
      (kind) => kinds.has(kind),
      "the space has no agent that runs skills",
    );
  }

  /** Runs `entry` in space `workspaceId` at `dir`; an untrusted file asks first. */
  async run(
    entry: ProjectEntry,
    workspaceId: string,
    dir: string,
    decision: TrustDecision | null = null,
  ): Promise<void> {
    const label = `${entry.kind === "preset" ? "Preset" : "Project"}: ${entry.name}`;
    let outcome: EntryOutcome;
    try {
      outcome = await this.bridge.invoke<EntryOutcome>("project_run", {
        spaceId: workspaceId,
        dir,
        entry,
        decision,
      });
    } catch (err) {
      const reason = err && typeof err === "object" && "message" in err ? err.message : err;
      this.spaces.fail(`${label} failed: ${String(reason)}`);
      return;
    }
    switch (outcome.kind) {
      case "ask":
        this.spaces.askTrust(outcome.request, async (answer) => {
          await this.run(entry, workspaceId, dir, answer);
          await this.reload();
        });
        return;
      case "agent":
        await this.sendToAgent(label, outcome.command, workspaceId);
        return;
      case "done":
        await this.store.refresh();
        return;
      case "missing":
        this.spaces.notify(`${label}: the project file no longer lists it`);
        await this.reload();
        return;
      case "declined":
        return;
    }
  }

  private async sendToAgent(
    label: string,
    text: string,
    workspaceId: string,
    accepts: (kind: string) => boolean = () => true,
    none = "the space has no agent",
  ): Promise<void> {
    const agents = this.store.snapshot().terminals.filter((t) => {
      const kind = agentKind(t);
      return t.space_id === workspaceId && kind !== undefined && accepts(kind);
    });
    if (agents.length === 0) {
      this.spaces.notify(`${label}: ${none}`);
      return;
    }
    const target =
      agents.length === 1
        ? agents[0].terminal_id
        : await this.picker.pick(
            agents.map((a) => ({
              paneId: a.terminal_id,
              label: `${agentKind(a)} (${a.label || a.terminal_id})`,
            })),
          );
    if (!target) return;
    const outcome = await this.prompts.send(target, text);
    if (!promptSucceeded(outcome)) this.spaces.notify(`${label}: ${promptMessage(outcome)}`);
  }
}
