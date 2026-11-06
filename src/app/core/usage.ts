import { Injectable, inject, signal } from "@angular/core";
import { TauriBridge } from "./tauri";

/** One window of tamias' `today --json` (wire shape of `usage.rs`, tamias' own field names). */
export interface UsageWindow {
  window: "seven_day" | "five_hour" | "monthly" | string;
  unit: "percent" | "requests" | string;
  used: number;
  limit: number;
  resets_at: string;
  today_usage?: number;
  budget?: number;
  ratio?: number;
  month_usage?: number;
}

export interface UsageProvider {
  provider: "claude" | "copilot" | string;
  status: "ok" | "stale" | "unavailable" | string;
  stale_since?: string;
  windows: UsageWindow[];
}

export interface UsageReport {
  schema_version: number;
  generated_at: string;
  providers: UsageProvider[];
}

export type UsageColor = "green" | "yellow" | "red" | "grey";

/** One figure of the status bar item, e.g. `5h 12%`. */
export interface UsageFigure {
  key: "5h" | "7d" | "1d" | "1m" | "CP";
  label: string;
  /** Rounded percent. */
  percent: number;
  color: UsageColor;
}

/** Green below 50 %, yellow below 80 %, red from 80 % (like the Claude Code status line). */
export function usageColor(percent: number, stale = false): UsageColor {
  if (stale) return "grey";
  if (percent < 50) return "green";
  if (percent < 80) return "yellow";
  return "red";
}

/** Days of the month of `now` (local). */
function daysInMonth(now: Date): number {
  return new Date(now.getFullYear(), now.getMonth() + 1, 0).getDate();
}

function find(report: UsageReport, provider: string): UsageProvider | undefined {
  return report.providers.find((p) => p.provider === provider && p.status !== "unavailable");
}

function window(p: UsageProvider | undefined, name: string): UsageWindow | undefined {
  return p?.windows.find((w) => w.window === name);
}

/**
 * The figures of the status bar item: `5h`, `7d`, `1d` (today against a seventh of the weekly
 * limit), `1m` (the month against its share of weekly limits) and `CP` (Copilot's monthly
 * requests). A figure whose source field is absent, or whose limit is not positive, is left out.
 */
export function figures(report: UsageReport | null, now = new Date()): UsageFigure[] {
  if (!report) return [];
  const out: UsageFigure[] = [];
  const add = (
    key: UsageFigure["key"],
    value: number | undefined,
    divisor: number,
    stale: boolean,
  ) => {
    if (value === undefined || !(divisor > 0)) return;
    const percent = (value / divisor) * 100;
    out.push({
      key,
      label: `${key} ${Math.round(percent)}%`,
      percent: Math.round(percent),
      color: usageColor(percent, stale),
    });
  };
  const claude = find(report, "claude");
  const stale = claude?.status === "stale";
  const five = window(claude, "five_hour");
  const seven = window(claude, "seven_day");
  add("5h", five?.used, five?.limit ?? 0, stale);
  add("7d", seven?.used, seven?.limit ?? 0, stale);
  add("1d", seven?.today_usage, (seven?.limit ?? 0) / 7, stale);
  add("1m", seven?.month_usage, ((seven?.limit ?? 0) * daysInMonth(now)) / 7, stale);
  const copilot = find(report, "copilot");
  const monthly = window(copilot, "monthly");
  add("CP", monthly?.used, monthly?.limit ?? 0, copilot?.status === "stale");
  return out;
}

const pad = (n: number) => String(n).padStart(2, "0");

/** Local `MM-DD HH:MM` of an RFC 3339 instant; the input as is when it is no date. */
export function localTime(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** Tooltip: reset times, today's budget and the stale state. */
export function usageTitle(report: UsageReport | null): string {
  if (!report) return "";
  const lines: string[] = [];
  const claude = find(report, "claude");
  const five = window(claude, "five_hour");
  const seven = window(claude, "seven_day");
  if (five) lines.push(`5h resets ${localTime(five.resets_at)}`);
  if (seven) lines.push(`7d resets ${localTime(seven.resets_at)}`);
  if (seven?.budget !== undefined) {
    const ratio = seven.ratio !== undefined ? ` (${Math.round(seven.ratio * 100)}%)` : "";
    const unit = seven.unit === "percent" ? "%" : ` ${seven.unit}`;
    const today = seven.today_usage !== undefined ? `${round(seven.today_usage)} of ` : "";
    lines.push(`Today's budget: ${today}${round(seven.budget)}${unit}${ratio}`);
  }
  for (const p of report.providers) {
    if (p.status === "stale") {
      const name = p.provider === "claude" ? "Claude" : "Copilot";
      lines.push(`${name}: stale${p.stale_since ? ` since ${localTime(p.stale_since)}` : ""}`);
    }
  }
  return lines.join("\n");
}

function round(n: number): string {
  return String(Math.round(n * 10) / 10);
}

/** Claude usage from tamias: the first read, then the `usage` events (every 60 s on change). */
@Injectable({ providedIn: "root" })
export class Usage {
  private readonly bridge = inject(TauriBridge);
  private readonly _report = signal<UsageReport | null>(null);
  readonly report = this._report.asReadonly();

  constructor() {
    void this.start();
  }

  private async start(): Promise<void> {
    try {
      await this.bridge.listen<UsageReport | null>("usage", (r) => this._report.set(r ?? null));
      this._report.set((await this.bridge.invoke<UsageReport | null>("get_usage")) ?? null);
    } catch {
      // usage is optional: without tamias (or outside Tauri) the item stays hidden
    }
  }
}
