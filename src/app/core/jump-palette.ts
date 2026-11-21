import { Injectable, computed, inject, signal } from "@angular/core";
import { Attention } from "./attention";
import { fuzzyRank } from "./fuzzy";
import { Machines } from "./machines";
import {
  agentKind,
  needsAttention,
  paneDir,
  paneTitle,
  withMachine,
  type Space,
  type Terminal,
} from "./snapshot";
import { SpacesStore } from "./spaces-store";
import { UiState } from "./ui-state";
import type { IconName } from "../ui/icon/icon";

export const NEEDS_YOU_GROUP = "Needs you";
export const RECENT_GROUP = "Recent";
/** At most this many spaces in the Recent group. */
export const RECENT_LIMIT = 8;

/** An entry of the jump list: a pane (agent or terminal) or a space. */
export interface JumpEntry {
  key: string;
  kind: "pane" | "space";
  /** The icon of the kind, as on a tab. */
  icon: IconName;
  title: string;
  /** Directory and branch (and pull request) shown beside the title. */
  detail: string;
  /** The pane to focus; for a space its first agent pane, absent when it has none (overview). */
  pane?: Terminal;
  /** Directory of the pane or space, for branch and pull request lookups. */
  dir?: string;
  spaceId: string;
  machine?: string;
  /** Why the entry cannot be chosen (its machine is not connected); null when it can. */
  disabled: string | null;
}

/** A line of the list: a group heading or a selectable entry. */
export type JumpRow =
  | { type: "heading"; key: string; label: string }
  | { type: "item"; key: string; entry: JumpEntry; index: number; positions: readonly number[] };

const escape = (s: string): string => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/**
 * The jump palette: a switcher for agents, terminals and spaces. Without a query it lists what
 * needs the user and the recent spaces; a query is fuzzy matched over label, directory and branch
 * (`#123` matches a number in the label or branch, or the space's pull request).
 */
@Injectable({ providedIn: "root" })
export class JumpPalette {
  private readonly store = inject(SpacesStore);
  private readonly ui = inject(UiState);
  private readonly attention = inject(Attention);
  private readonly machines = inject(Machines);

  private readonly _open = signal(false);
  private readonly _query = signal("");
  private readonly _selected = signal(0);
  private readonly _active = signal<string | null>(null);

  readonly isOpen = this._open.asReadonly();
  readonly query = this._query.asReadonly();

  private info(dir: string | undefined): { branch: string | null; pr: number | null } {
    if (!dir) return { branch: null, pr: null };
    return {
      branch: this.store.repos().get(dir)?.branch ?? null,
      pr: this.store.pulls().get(dir)?.number ?? null,
    };
  }

  private detail(dir: string | undefined): string {
    const { branch, pr } = this.info(dir);
    return [dir, branch, pr !== null ? `#${pr}` : null].filter(Boolean).join("  ");
  }

  private block(id: string): string | null {
    return this.machines.blockedReason(id);
  }

  private paneEntry(pane: Terminal): JumpEntry {
    return {
      key: `pane:${pane.terminal_id}`,
      kind: "pane",
      icon: "terminal",
      title: paneTitle(this.store.all(), pane),
      detail: this.detail(paneDir(pane)),
      pane,
      dir: paneDir(pane),
      spaceId: pane.space_id,
      machine: pane.machine,
      disabled: this.block(pane.terminal_id),
    };
  }

  private spaceEntry(space: Space): JumpEntry {
    const terminals = this.store.all().terminals.filter((t) => t.space_id === space.space_id);
    const pane = terminals.find((t) => agentKind(t) !== undefined);
    const dir = space.cwd ?? (terminals[0] ? paneDir(terminals[0]) : undefined);
    return {
      key: `space:${space.space_id}`,
      kind: "space",
      icon: "space",
      title: withMachine(space.label, space.machine),
      detail: this.detail(dir),
      pane,
      dir,
      spaceId: space.space_id,
      machine: space.machine,
      disabled: this.block(space.space_id),
    };
  }

  /** Agents that are blocked, then those done and unread. */
  private readonly needsYou = computed<JumpEntry[]>(() => {
    const terminals = this.store.all().terminals;
    const unread = this.attention.unread();
    return [
      ...terminals.filter((t) => needsAttention(t.agent_status)),
      ...terminals.filter((t) => !needsAttention(t.agent_status) && unread.has(t.terminal_id)),
    ].map((t) => this.paneEntry(t));
  });

  /** The spaces of this Mac by focus time, newest first, without the active one. */
  private readonly recent = computed<JumpEntry[]>(() => {
    const spaces = this.store.snapshot().spaces;
    const times = this.ui.recentSpaces();
    return Object.entries(times)
      .filter(([id]) => id !== this._active())
      .sort((a, b) => b[1] - a[1])
      .flatMap(([id]) => spaces.filter((s) => s.space_id === id))
      .slice(0, RECENT_LIMIT)
      .map((s) => this.spaceEntry(s));
  });

  private readonly everything = computed<JumpEntry[]>(() => {
    const snap = this.store.all();
    return [
      ...snap.terminals.map((t) => this.paneEntry(t)),
      ...snap.spaces.map((s) => this.spaceEntry(s)),
    ];
  });

  /** The lines of the list. */
  readonly rows = computed<JumpRow[]>(() => {
    const rows: JumpRow[] = [];
    let index = 0;
    const push = (entry: JumpEntry, positions: readonly number[]): void => {
      rows.push({ type: "item", key: entry.key, entry, index: index++, positions });
    };
    const query = this._query().trim();
    if (query === "") {
      const needs = this.needsYou();
      const seen = new Set(needs.map((e) => e.key));
      const groups: [string, JumpEntry[]][] = [
        [NEEDS_YOU_GROUP, needs],
        [RECENT_GROUP, this.recent().filter((e) => !seen.has(e.key))],
      ];
      for (const [label, members] of groups) {
        if (members.length === 0) continue;
        rows.push({ type: "heading", key: `heading:${label}`, label });
        for (const entry of members) push(entry, []);
      }
      return rows;
    }
    const number = /^#(\d+)$/.exec(query)?.[1];
    if (number !== undefined) {
      const word = new RegExp(`(^|\\D)${escape(number)}(\\D|$)`);
      for (const entry of this.everything()) {
        const { branch, pr } = this.info(entry.dir);
        if (
          word.test(entry.title) ||
          (branch !== null && word.test(branch)) ||
          pr === Number(number)
        )
          push(entry, []);
      }
      return rows;
    }
    for (const { item, positions } of fuzzyRank(
      this.everything(),
      query,
      (e) => `${e.title} ${e.detail}`,
    ))
      push(
        item,
        positions.filter((p) => p < item.title.length),
      );
    return rows;
  });

  /** The selectable entries in display order. */
  readonly entries = computed(() =>
    this.rows().flatMap((r) => (r.type === "item" ? [r.entry] : [])),
  );

  readonly selected = computed(() =>
    Math.max(0, Math.min(this._selected(), this.entries().length - 1)),
  );

  /** Opens with an empty query; `activeSpaceId` is left out of Recent. */
  open(activeSpaceId: string | null): void {
    this._active.set(activeSpaceId);
    this._query.set("");
    this._selected.set(0);
    this._open.set(true);
  }

  close(): void {
    this._open.set(false);
  }

  toggle(activeSpaceId: string | null): void {
    if (this._open()) this.close();
    else this.open(activeSpaceId);
  }

  setQuery(query: string): void {
    this._query.set(query);
    this._selected.set(0);
  }

  hover(index: number): void {
    this._selected.set(index);
  }

  /** Moves the highlight by `delta`, wrapping around. */
  move(delta: number): void {
    const n = this.entries().length;
    if (n > 0) this._selected.set((this.selected() + delta + n) % n);
  }

  current(): JumpEntry | undefined {
    return this.entries()[this.selected()];
  }
}
