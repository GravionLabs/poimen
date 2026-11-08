/**
 * The live state of deliver runs. Primary source: the run state folded from the progress events of
 * a run by the backend (`progress_events.rs`, schema `deliver-run-state.schema.json` plus `source`
 * and `tone`); fallback during the migration: `.agents/deliver-loop/status.json`, schema
 * `deliver-loop-status.schema.json` of GravionLabs/skills (`schemaVersion: 1`). Everything here is
 * pure; the Tauri side is `deliver.ts`.
 */

/** The schema version this build understands. */
export const SUPPORTED_SCHEMA = 1;

/** The version of the progress events this build reads (`v`, as `SUPPORTED_V` of the backend). */
export const SUPPORTED_EVENTS = 1;

/** A file whose `updatedAt` is older than this while `result` is `running` is stale (rule 4). */
export const STALE_MS = 60 * 60 * 1000;

export type LoopResult =
  "running" | "done" | "max reached" | "blocked" | "no progress" | "abandoned";

const RESULTS: readonly LoopResult[] = [
  "running",
  "done",
  "max reached",
  "blocked",
  "no progress",
  "abandoned",
];

/** How the emitter rated a run; the displays map it to their palette, never the text. */
export type Tone = "neutral" | "info" | "success" | "warning" | "danger";

const TONES: readonly Tone[] = ["neutral", "info", "success", "warning", "danger"];

/** Who runs it: a `loop` of the deliver loop, or an interactive `session`. */
export type RunSource = "loop" | "session";

/** A blocked or paused item, or something the user must decide. */
export interface LoopItem {
  number: number;
  url: string;
  reason: string;
}

export interface LoopCurrent {
  number: number;
  title: string;
  url: string;
  step: { n: number; total: number; text: string };
  prUrl: string | null;
}

export interface LoopDelivered {
  pbi: number;
  url: string;
  prUrl: string;
}

/** The release candidate and release pull request of a run (`release.updated`). */
export interface LoopRelease {
  reviewRound: number | null;
  prUrl: string | null;
  rcUrl: string | null;
  rcVersion: string | null;
  testIssue: { number: number; url: string } | null;
}

/** The parts of a run state the panel shows (unknown fields are ignored). */
export interface DeliverStatus {
  run: string;
  harness: string;
  model: string;
  repository: string;
  /** `null` for a run without a milestone (an interactive session). */
  milestone: { key: string; title: string; url: string } | null;
  source: RunSource | null;
  tone: Tone;
  closed: number;
  total: number;
  current: LoopCurrent | null;
  delivered: LoopDelivered[];
  blocked: LoopItem[];
  paused: LoopItem[];
  needsYou: LoopItem[];
  release: LoopRelease;
  startedAt: string;
  updatedAt: string;
  result: LoopResult;
}

/** What the `deliver` event carries for one repository (`Read` of `deliver.rs`). */
export type DeliverRead =
  | { kind: "missing" }
  | { kind: "ok"; status: unknown }
  | { kind: "unsupported"; schemaVersion?: unknown }
  | { kind: "invalid"; error: string }
  | { kind: "events"; runs: unknown[]; unknownVersion?: number | null };

/** The state of one card. */
export type CardState =
  | { kind: "ok"; status: DeliverStatus }
  | { kind: "unsupported"; version: string }
  | { kind: "events-version"; version: string }
  | { kind: "invalid"; error: string };

/** One card of the panel: a run of a repository (`run` is empty for a hint or an error). */
export interface DeliverCard {
  root: string;
  run: string;
  state: CardState;
}

const isObject = (v: unknown): v is Record<string, unknown> =>
  typeof v === "object" && v !== null && !Array.isArray(v);
const isString = (v: unknown): v is string => typeof v === "string";
const isCount = (v: unknown): v is number => typeof v === "number" && Number.isInteger(v) && v >= 0;
const isNullableString = (v: unknown): v is string | null => v === null || isString(v);

const isTone = (v: unknown): v is Tone => TONES.includes(v as Tone);

function release(value: unknown): LoopRelease | undefined {
  if (!isObject(value)) return undefined;
  const nullable = (v: unknown) => (v === undefined ? null : v);
  const [round, pr, rc, version, test] = [
    value["reviewRound"],
    value["prUrl"],
    value["rcUrl"],
    value["rcVersion"],
    value["testIssue"],
  ].map(nullable);
  if (
    !(round === null || isCount(round)) ||
    !isNullableString(pr) ||
    !isNullableString(rc) ||
    !isNullableString(version)
  )
    return undefined;
  let testIssue: LoopRelease["testIssue"] = null;
  if (test !== null) {
    if (!isObject(test) || !isCount(test["number"]) || !isString(test["url"])) return undefined;
    testIssue = { number: test["number"], url: test["url"] };
  }
  return { reviewRound: round, prUrl: pr, rcUrl: rc, rcVersion: version, testIssue };
}

/**
 * The tone of a file without one (status.json): a blocked result is danger, a running run that
 * asks for the user warning, a finished run success.
 */
function legacyTone(result: LoopResult, asks: boolean): Tone {
  if (result === "blocked") return "danger";
  if (asks && result === "running") return "warning";
  return result === "done" ? "success" : "neutral";
}

function items(value: unknown): LoopItem[] | null {
  if (!Array.isArray(value)) return null;
  const out: LoopItem[] = [];
  for (const v of value) {
    if (!isObject(v) || !isCount(v["number"]) || !isString(v["url"]) || !isString(v["reason"]))
      return null;
    out.push({ number: v["number"], url: v["url"], reason: v["reason"] });
  }
  return out;
}

function current(value: unknown): LoopCurrent | null | undefined {
  if (value === null) return null;
  if (!isObject(value)) return undefined;
  const step = value["step"];
  if (
    !isCount(value["number"]) ||
    !isString(value["title"]) ||
    !isString(value["url"]) ||
    !isObject(step) ||
    !isCount(step["n"]) ||
    !isCount(step["total"]) ||
    !isString(step["text"]) ||
    !isNullableString(value["prUrl"])
  )
    return undefined;
  return {
    number: value["number"],
    title: value["title"],
    url: value["url"],
    step: { n: step["n"], total: step["total"], text: step["text"] },
    prUrl: value["prUrl"],
  };
}

function delivered(value: unknown): LoopDelivered[] | null {
  if (!Array.isArray(value)) return null;
  const out: LoopDelivered[] = [];
  for (const v of value) {
    if (!isObject(v) || !isCount(v["pbi"]) || !isString(v["url"]) || !isString(v["prUrl"]))
      return null;
    out.push({ pbi: v["pbi"], url: v["url"], prUrl: v["prUrl"] });
  }
  return out;
}

/** Validates the fields the panel uses; unknown fields are ignored, a wrong known field fails. */
export function parseStatus(value: unknown): DeliverStatus | string {
  if (!isObject(value)) return "the status is not an object";
  const milestone = value["milestone"];
  const rel = release(value["release"]);
  const cur = current(value["current"]);
  const done = delivered(value["delivered"]);
  const blocked = items(value["blocked"]);
  const paused = items(value["paused"]);
  const needsYou = items(value["needsYou"]);
  const result = value["result"];
  if (!isString(value["run"])) return "run is missing";
  if (
    milestone !== null &&
    (!isObject(milestone) ||
      !isString(milestone["key"]) ||
      !isString(milestone["title"]) ||
      !isString(milestone["url"]))
  )
    return "milestone is invalid";
  if (value["repository"] !== null && !isString(value["repository"]))
    return "repository is missing";
  if (!isCount(value["closed"]) || !isCount(value["total"])) return "closed or total is invalid";
  if (cur === undefined) return "current is invalid";
  if (!done) return "delivered is invalid";
  if (!blocked || !paused || !needsYou) return "blocked, paused or needsYou is invalid";
  if (!rel) return "release is invalid";
  if (!isString(value["startedAt"]) || !isString(value["updatedAt"]))
    return "startedAt or updatedAt is missing";
  if (Number.isNaN(Date.parse(value["updatedAt"]))) return "updatedAt is not a date";
  if (!isString(result) || !RESULTS.includes(result as LoopResult)) return "result is unknown";
  const source = value["source"];
  return {
    run: value["run"],
    harness: isString(value["harness"]) ? value["harness"] : "",
    model: isString(value["model"]) ? value["model"] : "",
    repository: (value["repository"] as string | null) ?? "",
    milestone: isObject(milestone)
      ? {
          key: milestone["key"] as string,
          title: milestone["title"] as string,
          url: milestone["url"] as string,
        }
      : null,
    source: source === "loop" || source === "session" ? source : null,
    tone: isTone(value["tone"])
      ? value["tone"]
      : legacyTone(result as LoopResult, blocked.length + paused.length + needsYou.length > 0),
    closed: value["closed"],
    total: value["total"],
    current: cur,
    delivered: done,
    blocked,
    paused,
    needsYou,
    release: rel,
    startedAt: value["startedAt"],
    updatedAt: value["updatedAt"],
    result: result as LoopResult,
  };
}

/** The key of a card: the repository and the run. */
export const cardKey = (card: Pick<DeliverCard, "root" | "run">): string =>
  `${card.root}\n${card.run}`;

function statusCard(root: string, value: unknown): DeliverCard {
  const status = parseStatus(value);
  return typeof status === "string"
    ? { root, run: "", state: { kind: "invalid", error: status } }
    : { root, run: status.run, state: { kind: "ok", status } };
}

/** The cards of a `deliver` event: none for a repository without runs, one per run of its events. */
export function cardsOf(root: string, read: DeliverRead): DeliverCard[] {
  switch (read.kind) {
    case "missing":
      return [];
    case "invalid":
      return [{ root, run: "", state: { kind: "invalid", error: read.error } }];
    case "unsupported":
      return [
        {
          root,
          run: "",
          state: {
            kind: "unsupported",
            version:
              read.schemaVersion === undefined || read.schemaVersion === null
                ? "none"
                : String(read.schemaVersion),
          },
        },
      ];
    case "ok":
      return [statusCard(root, read.status)];
    case "events": {
      const cards = read.runs.map((run) => statusCard(root, run));
      if (read.unknownVersion !== undefined && read.unknownVersion !== null)
        cards.push({
          root,
          run: "",
          state: { kind: "events-version", version: String(read.unknownVersion) },
        });
      return cards;
    }
  }
}

/** `running` older than {@link STALE_MS} is `stale`; else the result as written. */
export type RunState = Exclude<LoopResult, "abandoned"> | "stale";

export function runState(status: DeliverStatus, now: number): RunState {
  if (status.result === "abandoned") return "stale";
  if (status.result !== "running") return status.result;
  return now - Date.parse(status.updatedAt) > STALE_MS ? "stale" : "running";
}

/** "just now", "12 min ago", "3 h ago", "2 d ago". */
export function ageText(iso: string, now: number): string {
  const ms = now - Date.parse(iso);
  if (Number.isNaN(ms)) return "";
  const min = Math.floor(Math.max(ms, 0) / 60000);
  if (min < 1) return "just now";
  if (min < 60) return `${min} min ago`;
  const hours = Math.floor(min / 60);
  if (hours < 24) return `${hours} h ago`;
  return `${Math.floor(hours / 24)} d ago`;
}

/** The line under the title: "running, updated 3 min ago", "finished 12 min ago", ... */
export function stateText(status: DeliverStatus, now: number): string {
  const age = ageText(status.updatedAt, now);
  switch (runState(status, now)) {
    case "running":
      return `running, updated ${age}`;
    case "stale":
      return `stale: no update for ${age.replace(" ago", "")}, the run may have stopped`;
    case "done":
      return `finished ${age}`;
    case "max reached":
      return `stopped at the item limit ${age}`;
    case "blocked":
      return `blocked ${age}`;
    case "no progress":
      return `stopped without progress ${age}`;
  }
}

/** Progress of the milestone in percent, 0 to 100. */
export function percent(status: Pick<DeliverStatus, "closed" | "total">): number {
  if (status.total <= 0) return 0;
  return Math.min(100, Math.round((status.closed / status.total) * 100));
}

/** Whether the tone of the run asks for a look: warning or danger (never read from text). */
export function needsAttention(status: DeliverStatus): boolean {
  return status.tone === "warning" || status.tone === "danger";
}

/** The compact status bar item. */
export interface BarSummary {
  text: string;
  title: string;
  warning: boolean;
}

/**
 * `▶ M5 7/19` per running run, `▶ n runs` for several; `null` without a running run. A fresh run
 * that ended with a warning or danger tone (blocked, no progress) still shows until it is stale,
 * so a stop is not missed; the warning mark follows the tone.
 */
export function barSummary(cards: readonly DeliverCard[], now: number): BarSummary | null {
  const active: DeliverStatus[] = [];
  for (const card of cards) {
    if (card.state.kind !== "ok") continue;
    const state = runState(card.state.status, now);
    if (
      state === "running" ||
      (state !== "stale" &&
        needsAttention(card.state.status) &&
        now - Date.parse(card.state.status.updatedAt) < STALE_MS)
    )
      active.push(card.state.status);
  }
  if (active.length === 0) return null;
  const label = (s: DeliverStatus) =>
    s.milestone ? `${s.milestone.key} ${s.closed}/${s.total}` : "session";
  const warning = active.some(needsAttention);
  const title = active
    .map(
      (s) =>
        `${s.repository} ${label(s)}${s.current ? `: #${s.current.number} ${s.current.step.text}` : ""}${
          s.needsYou.length > 0 ? " (needs you)" : ""
        }`,
    )
    .join("\n");
  const text =
    active.length === 1
      ? `${warning ? "⚠" : "▶"} ${label(active[0])}`
      : `${warning ? "⚠" : "▶"} ${active.length} runs`;
  return { text, title, warning };
}

export type DeliverEventKind = "blocked" | "needs-you" | "release" | "ended";

export interface DeliverEvent {
  kind: DeliverEventKind;
  title: string;
  body: string;
}

/**
 * What changed between two reads of the same repository that deserves a notification: a run
 * became blocked, needs a decision, opened the release pull request or ended. Nothing without a
 * previous state (the first read is not news).
 */
export function transitions(prev: DeliverStatus | undefined, next: DeliverStatus): DeliverEvent[] {
  if (!prev || prev.run !== next.run) return [];
  const title = `Deliver ${next.milestone?.key ?? "session"}`;
  const out: DeliverEvent[] = [];
  const known = new Set(prev.blocked.map((i) => i.number));
  const newlyBlocked = next.blocked.filter((i) => !known.has(i.number));
  if (newlyBlocked.length > 0) {
    out.push({
      kind: "blocked",
      title,
      body: `#${newlyBlocked[0].number} is blocked: ${newlyBlocked[0].reason}`,
    });
  } else if (next.result === "blocked" && prev.result !== "blocked") {
    out.push({ kind: "blocked", title, body: "The run is blocked" });
  }
  const asked = new Set(prev.needsYou.map((i) => i.number));
  const newlyAsked = next.needsYou.filter((i) => !asked.has(i.number));
  if (newlyAsked.length > 0) {
    out.push({
      kind: "needs-you",
      title,
      body: `#${newlyAsked[0].number} needs you: ${newlyAsked[0].reason}`,
    });
  }
  if (next.release.prUrl && next.release.prUrl !== prev.release.prUrl) {
    out.push({ kind: "release", title, body: "The release pull request is open" });
  }
  if (prev.result === "running" && next.result !== "running" && next.result !== "blocked") {
    out.push({
      kind: "ended",
      title,
      body: next.result === "done" ? "The run is done" : `The run ended: ${next.result}`,
    });
  }
  return out;
}
