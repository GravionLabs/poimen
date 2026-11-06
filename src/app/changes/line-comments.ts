import { Injectable, computed, inject, signal } from "@angular/core";
import { AgentPrompts, promptMessage, promptSucceeded } from "../core/agent-prompt";
import { agentKind, paneDir, type Terminal } from "../core/herdr";
import { HerdrStore } from "../core/herdr-store";
import { TauriBridge } from "../core/tauri";
import { UiState, type CommentSet, type LineComment } from "../core/ui-state";
import { buildPrompt, withFile } from "./comment-prompt";

const EMPTY: CommentSet = { pending: [], sent: [] };
let counter = 0;

/** A comment before it has an id. */
export type NewComment = Omit<LineComment, "id">;

/** The agent panes of a worktree: panes with an agent whose directory is the worktree or below. */
export function agentsOf(panes: Iterable<Terminal>, dir: string): Terminal[] {
  const base = dir.endsWith("/") ? dir.slice(0, -1) : dir;
  return [...panes].filter((pane) => {
    const at = paneDir(pane);
    return !!agentKind(pane) && !!at && (at === base || at.startsWith(`${base}/`));
  });
}

/**
 * The review comments of one worktree: pending until the agent got them, then "sent". Kept in
 * `UiState` per worktree, so closing the tab or restarting the app loses nothing. Provided per
 * Changes tab.
 */
@Injectable()
export class LineComments {
  private readonly ui = inject(UiState);
  private readonly store = inject(HerdrStore);
  private readonly bridge = inject(TauriBridge);
  private readonly prompts = inject(AgentPrompts);

  readonly dir = signal("");
  readonly error = signal<string | null>(null);
  readonly sending = signal(false);
  /** The agent chosen in the picker when the worktree has several. */
  readonly chosen = signal<string | null>(null);

  private readonly set = computed(() => this.ui.lineComments()[this.dir()] ?? EMPTY);
  readonly pending = computed(() => this.set().pending);
  readonly sent = computed(() => this.set().sent);
  readonly agents = computed(() => agentsOf(this.store.panes().values(), this.dir()));
  /** The pane that gets the prompt: the only agent, or the picked one. */
  readonly target = computed(() => {
    const agents = this.agents();
    if (agents.length === 1) return agents[0].terminal_id;
    const pick = this.chosen();
    return agents.find((a) => a.terminal_id === pick)?.terminal_id ?? null;
  });
  /** Why sending is not possible now; `null` when it is. */
  readonly blocked = computed<string | null>(() => {
    if (this.sending()) return "Sending…";
    if (!this.pending().length) return "No pending comments";
    if (!this.agents().length) return "No agent is running in this worktree";
    if (!this.target()) return "Pick the agent to send to";
    return null;
  });

  follow(dir: string): void {
    this.dir.set(dir);
  }

  forFile(path: string): LineComment[] {
    return this.pending().filter((c) => c.path === path);
  }

  add(comment: NewComment): void {
    const text = comment.text.trim();
    if (!text) return;
    const id = `c${Date.now().toString(36)}-${counter++}`;
    this.write({ ...this.set(), pending: [...this.pending(), { ...comment, text, id }] });
  }

  edit(id: string, text: string): void {
    const value = text.trim();
    if (!value) return;
    this.write({
      ...this.set(),
      pending: this.pending().map((c) => (c.id === id ? { ...c, text: value } : c)),
    });
  }

  remove(id: string): void {
    this.write({ ...this.set(), pending: this.pending().filter((c) => c.id !== id) });
  }

  clearSent(): void {
    this.write({ ...this.set(), sent: [] });
  }

  /**
   * Sends the pending comments as one prompt. They move to "sent" only once the agent took the
   * prompt; on any failure they stay pending and `error` says why.
   */
  async send(): Promise<boolean> {
    const reason = this.blocked();
    const target = this.target();
    if (reason || !target) return false;
    const dir = this.dir();
    const batch = this.pending();
    this.sending.set(true);
    this.error.set(null);
    try {
      const built = buildPrompt(batch);
      let text = built.text;
      if (built.file !== null) {
        try {
          const path = await this.bridge.invoke<string>("changes_write_comments", {
            dir,
            content: built.file,
          });
          text = withFile(path);
        } catch (err) {
          this.error.set(`Could not write the comments file: ${String(err)}`);
          return false;
        }
      }
      const outcome = await this.prompts.send(target, text);
      if (!promptSucceeded(outcome)) {
        this.error.set(promptMessage(outcome));
        return false;
      }
      const ids = new Set(batch.map((c) => c.id));
      // Re-read: comments may have been edited or added while the prompt was on its way.
      const now = this.ui.lineComments()[dir] ?? EMPTY;
      this.ui.setLineComments(dir, {
        pending: now.pending.filter((c) => !ids.has(c.id)),
        sent: [...now.sent, ...batch],
      });
      return true;
    } finally {
      this.sending.set(false);
    }
  }

  private write(set: CommentSet): void {
    this.ui.setLineComments(this.dir(), set);
  }
}
