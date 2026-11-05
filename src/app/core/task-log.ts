import { Injectable, signal } from "@angular/core";

/** One line of the Log tab of the bottom panel. */
export interface TaskLogLine {
  time: number;
  text: string;
  /** A follow-up line (the command to run by hand), shown indented. */
  detail: boolean;
}

/** The most lines kept; older ones are dropped. */
export const TASK_LOG_LIMIT = 500;

/** The log of task actions (clean up merged tasks): in memory, shown in the bottom panel. */
@Injectable({ providedIn: "root" })
export class TaskLog {
  private readonly _lines = signal<readonly TaskLogLine[]>([]);
  readonly lines = this._lines.asReadonly();

  /** Appends lines; `detail` lines belong to the line before them. */
  add(lines: readonly { text: string; detail?: boolean }[]): void {
    const time = Date.now();
    const added = lines.map((l) => ({ time, text: l.text, detail: l.detail ?? false }));
    this._lines.update((old) => [...old, ...added].slice(-TASK_LOG_LIMIT));
  }

  clear(): void {
    this._lines.set([]);
  }
}
