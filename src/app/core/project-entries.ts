import { Injectable, inject, signal } from "@angular/core";
import { AgentPicker } from "./agent-picker";
import { AgentPrompts, promptMessage, promptSucceeded } from "./agent-prompt";
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

/** An entry of the project file picked in the palette (wire shape of `Entry`). */
export type ProjectEntry = { kind: "command"; name: string } | { kind: "preset"; name: string };

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
      return;
    }
    let list: ProjectEntryList;
    try {
      list = (await this.bridge.invoke<ProjectEntryList>("project_entries", { dir })) ?? NONE;
    } catch {
      list = NONE;
    }
    if (seq === this.seq) this._entries.set(list);
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
      this.spaces.notify(`${label} failed: ${String(reason)}`);
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

  private async sendToAgent(label: string, text: string, workspaceId: string): Promise<void> {
    const agents = this.store
      .snapshot()
      .terminals.filter((t) => t.space_id === workspaceId && agentKind(t));
    if (agents.length === 0) {
      this.spaces.notify(`${label}: the space has no agent`);
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
