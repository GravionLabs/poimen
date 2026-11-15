import { Injectable, inject, signal } from "@angular/core";
import { TauriBridge } from "./tauri";

/**
 * Export and import of the user settings (#581), over `settings_export`, `settings_import_plan` and
 * `settings_import_apply` of `src-tauri/src/settings_transfer.rs`. The backend validates, diffs and
 * writes; `trustedProjects` never leaves or enters (per machine) and UI state is not in the file.
 */

/** Keeps the keys the file does not set, or makes the file the settings. */
export type ImportMode = "merge" | "replace";
/** The whole settings file, or only its space templates (merged by name). */
export type ImportScope = "all" | "template";

/** A key the import adds, changes or removes, with its values. */
export interface ImportChange {
  path: string;
  before?: unknown;
  after?: unknown;
}

/** An entry of the file that is not imported. */
export interface ImportRejected {
  path: string;
  reason: string;
}

export interface ImportPlan {
  added: ImportChange[];
  changed: ImportChange[];
  removed: ImportChange[];
  invalid: ImportRejected[];
  skipped: ImportRejected[];
}

/** A pending import: the chosen file and what importing it would do. */
export interface PendingImport {
  path: string;
  scope: ImportScope;
  mode: ImportMode;
  plan: ImportPlan;
}

function record(raw: unknown): Record<string, unknown> {
  return typeof raw === "object" && raw !== null ? (raw as Record<string, unknown>) : {};
}

function changes(raw: unknown): ImportChange[] {
  if (!Array.isArray(raw)) return [];
  return raw.flatMap((c) => {
    const r = record(c);
    if (typeof r["path"] !== "string") return [];
    const out: ImportChange = { path: r["path"] };
    if ("before" in r) out.before = r["before"];
    if ("after" in r) out.after = r["after"];
    return [out];
  });
}

function rejected(raw: unknown): ImportRejected[] {
  if (!Array.isArray(raw)) return [];
  return raw.flatMap((c) => {
    const r = record(c);
    return typeof r["path"] === "string" && typeof r["reason"] === "string"
      ? [{ path: r["path"], reason: r["reason"] }]
      : [];
  });
}

/** The plan as the backend sends it; missing or malformed lists are empty. */
export function parseImportPlan(raw: unknown): ImportPlan {
  const r = record(raw);
  return {
    added: changes(r["added"]),
    changed: changes(r["changed"]),
    removed: changes(r["removed"]),
    invalid: rejected(r["invalid"]),
    skipped: rejected(r["skipped"]),
  };
}

/** Whether applying the plan writes anything. */
export function hasChanges(plan: ImportPlan): boolean {
  return plan.added.length + plan.changed.length + plan.removed.length > 0;
}

/** One line for the plan: `2 added, 1 changed, 0 removed`, plus the invalid entries if any. */
export function planSummary(plan: ImportPlan): string {
  const parts = [
    `${plan.added.length} added`,
    `${plan.changed.length} changed`,
    `${plan.removed.length} removed`,
  ];
  if (plan.invalid.length) parts.push(`${plan.invalid.length} invalid`);
  return parts.join(", ");
}

/** A value of the diff as short JSON (cut at `max` characters). */
export function shortValue(value: unknown, max = 80): string {
  const text = value === undefined ? "" : JSON.stringify(value);
  return text.length > max ? `${text.slice(0, max - 1)}…` : text;
}

/** The file name the export dialog suggests: `poimen-settings.json` or `poimen-template-<name>.json`. */
export function exportFileName(template: string | null): string {
  if (template === null) return "poimen-settings.json";
  const safe = template.replace(/[^A-Za-z0-9._-]+/g, "-").replace(/^-+|-+$/g, "");
  return `poimen-template-${safe || "template"}.json`;
}

@Injectable({ providedIn: "root" })
export class SettingsTransfer {
  private readonly bridge = inject(TauriBridge);
  private readonly _pending = signal<PendingImport | null>(null);
  private readonly _message = signal<string | null>(null);
  private readonly _error = signal<string | null>(null);
  private readonly _busy = signal(false);
  /** The import waiting for the user's confirmation. */
  readonly pending = this._pending.asReadonly();
  /** The outcome of the last export or import. */
  readonly message = this._message.asReadonly();
  readonly error = this._error.asReadonly();
  readonly busy = this._busy.asReadonly();

  /** Exports the settings (or the space template `template`) to a file chosen in the save dialog. */
  async export(template: string | null = null): Promise<void> {
    const path = await this.bridge.pickSaveFile(exportFileName(template));
    if (path === null) return;
    await this.run(async () => {
      await this.bridge.invoke("settings_export", { path, template });
      this._message.set(
        template === null ? `Settings exported to ${path}` : `Template exported to ${path}`,
      );
    });
  }

  /** Picks a file and shows what importing it would do; nothing is written yet. */
  async choose(scope: ImportScope): Promise<void> {
    const path = await this.bridge.pickJsonFile();
    if (path === null) return;
    await this.plan(path, scope, "merge");
  }

  /** Plans the pending import again with `mode`. */
  async setMode(mode: ImportMode): Promise<void> {
    const pending = this._pending();
    if (pending && pending.mode !== mode) await this.plan(pending.path, pending.scope, mode);
  }

  /** Writes the pending import; the settings watcher then brings the new settings. */
  async apply(): Promise<void> {
    const pending = this._pending();
    if (!pending) return;
    await this.run(async () => {
      const raw = await this.bridge.invoke<unknown>("settings_import_apply", {
        path: pending.path,
        mode: pending.mode,
        scope: pending.scope,
      });
      this._pending.set(null);
      this._message.set(`Imported: ${planSummary(parseImportPlan(raw))}`);
    });
  }

  cancel(): void {
    this._pending.set(null);
  }

  private async plan(path: string, scope: ImportScope, mode: ImportMode): Promise<void> {
    await this.run(async () => {
      const raw = await this.bridge.invoke<unknown>("settings_import_plan", { path, mode, scope });
      this._pending.set({ path, scope, mode, plan: parseImportPlan(raw) });
    });
  }

  private async run(step: () => Promise<void>): Promise<void> {
    this._busy.set(true);
    this._error.set(null);
    this._message.set(null);
    try {
      await step();
    } catch (e) {
      this._error.set(String(e));
    } finally {
      this._busy.set(false);
    }
  }
}
