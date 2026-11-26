// The answers of the demo backend (#1109): the commands the screenshot views call, from `data.ts`.
// A command that is not answered here fails the run with its name (`kit/backend.ts`).
import { DEFAULT_SETTINGS } from "../../app/core/settings";
import { DemoBackend, type DemoChannel } from "../kit/backend";
import {
  CHANGED_FILE,
  CHANGES,
  CLOSED_ISSUES,
  DETECTED,
  HARNESSES,
  MAIN_CALLS,
  MILESTONES,
  OPEN_ISSUES,
  TOKENS,
  HOME,
  MACHINES,
  MACHINE_CHECK,
  MACHINE_SNAPSHOT,
  RUN,
  REPOS,
  SCREENS,
  SNAPSHOT,
} from "./data";

const enc = new TextEncoder();

/** A string argument of a command; empty when it is missing. */
const text = (args: Record<string, unknown> | undefined, key: string): string => {
  const value = args?.[key];
  return typeof value === "string" ? value : "";
};
/** The blinking cursor would make two runs differ. */
const HIDE_CURSOR = "\x1b[?25l";

export function createPoimenDemo(): DemoBackend {
  const backend = new DemoBackend();
  let nextStream = 1;
  backend.fixed({
    app_version: "0.31.0",
    app_channel: "stable",
    ensure_backend: { state: "running", reason: null },
    backend_info: { version: "0.31.0", protocol: 1, supported: 1 },
    snapshot: SNAPSHOT,
    get_settings: {
      ...DEFAULT_SETTINGS,
      home: HOME,
      theme: "system",
      machines: MACHINES.map(({ name, ssh, reposDir }) => ({ name, ssh, reposDir })),
    },
    usage_claude: [
      { label: "5h", usedPercent: 38, resetsAt: "2026-11-21T13:00:00Z" },
      { label: "7d", usedPercent: 21, resetsAt: "2026-11-25T09:00:00Z" },
    ],
    seed_space_defaults: null,
    server_socket_path: `${HOME}/Library/Application Support/poimen/server.sock`,
    server_log_path: `${HOME}/Library/Logs/poimen/server.log`,
    get_harnesses: HARNESSES,
    list_agent_sessions: [],
    get_agent_actions: { actions: [] },
    agent_grants: { grants: [] },
    set_terminal_colors: null,
    get_color_schemes: { schemes: [], warnings: [] },
    list_machines: MACHINES,
    machine_snapshot: MACHINE_SNAPSHOT,
    ssh_config_hosts: [{ host: "build-box", configured: true }],
    subagents_unwatch: null,
    tokens_unwatch: null,
    branch_pulls: {},
    hooks_status: [
      { agent: "claude", state: "installed", mcp: "installed" },
      { agent: "copilot", state: "installed", mcp: "installed" },
    ],
    machine_check: MACHINE_CHECK,
    terminal_input: null,
    tab_focus: null,
    project_entries: { commands: [], presets: [], locked: false },
    terminal_resize: null,
    close_terminal: null,
    schedules_list: { paused: false, schedules: [] },
    schedules_alerts: { alerts: [] },
    changes_file: CHANGED_FILE,
    changes_findings: { state: "none" },
    worktree_branches: [],
    changes_pr_state: { branch: null, base: null, pushed: false, existing: null },
  });
  const POIMEN = `${HOME}/code/poimen`;
  backend.answer({
    changes_list: (args) => ({ ...CHANGES, scope: text(args, "scope") }),
    detected_entries: (args) => (text(args, "dir") === POIMEN ? DETECTED : []),
    repo_root: (args) => text(args, "dir"),
    github_pulls: (args) =>
      ((args?.["dirs"] as string[] | undefined) ?? [])
        .filter((d) => d === POIMEN)
        .map((root) => ({ root, repo: "acme/poimen", status: "ok", pulls: [] })),
    subagents_watch: (args, b) => {
      const id = text(args, "terminalId");
      const session = SNAPSHOT.terminals.find((t) => t.terminal_id === id)?.agent_session?.value;
      if (!session) return false;
      // The transcript reader answers with the main agent's calls once the watch started.
      setTimeout(() => {
        const update = { kind: "main", calls: MAIN_CALLS, tools: 14, usage: [] };
        b.emit("subagents", { key: args?.["key"], session, update });
        b.emit("subagents", { key: args?.["key"], session, update: { kind: "list", agents: [] } });
      }, 30);
      return true;
    },
    tokens_watch: (args, b) => {
      const id = text(args, "terminalId");
      const totals = TOKENS[id];
      if (totals) setTimeout(() => b.emit("tokens", { terminal_id: id, totals, samples: [] }), 30);
      return totals !== undefined;
    },
    github_milestones: (args) =>
      ((args?.["dirs"] as string[] | undefined) ?? [])
        .filter((d) => d === POIMEN)
        .map((root) => ({ root, repo: "acme/poimen", status: "ok", milestones: MILESTONES })),
    github_issues: (args) =>
      ((args?.["dirs"] as string[] | undefined) ?? [])
        .filter((d) => d === POIMEN)
        .map((root) => ({
          root,
          repo: "acme/poimen",
          status: "ok",
          issues: args?.["closed"] ? CLOSED_ISSUES : OPEN_ISSUES,
          truncated: false,
        })),
    github_repo: (args) => (args?.["dir"] === POIMEN ? { owner: "acme", name: "poimen" } : null),
    deliver_watch: (args, b) => {
      const dirs = (args?.["dirs"] as string[] | undefined) ?? [];
      const root = `${HOME}/code/poimen`;
      if (dirs.includes(root)) {
        setTimeout(() => b.emit("deliver", { root, read: { kind: "events", runs: [RUN] } }), 0);
      }
      return dirs.filter((d) => d === root);
    },
    repo_info: (args) => {
      const repo = REPOS[text(args, "dir")];
      return repo
        ? {
            branch: repo.branch,
            worktree: repo.worktree ?? false,
            dirty: repo.dirty,
            ahead: repo.ahead,
            behind: repo.behind,
            on_base: repo.branch === "main",
            common_dir: `${text(args, "dir")}/.git`,
          }
        : { branch: null, worktree: false };
    },
    open_terminal: (args) => {
      const channel = args?.["onRecord"] as DemoChannel;
      const id = text(args, "target");
      const control = args?.["mode"] === "control";
      // The view knows the stream only once this answer arrived: push after it, as the server does.
      setTimeout(() => {
        channel.send({ type: "reset", cols: 100, rows: 30 });
        if (control) channel.send({ type: "controller", attached: true });
        channel.send({
          type: "bytes",
          data: enc.encode((SCREENS[id] ?? []).join("\r\n") + HIDE_CURSOR),
        });
        // What the screenshot views wait for: a canvas has no text to look for.
        const shown = new Set(
          (document.body.dataset["demoTerminals"] ?? "").split(" ").filter(Boolean),
        );
        document.body.dataset["demoTerminals"] = [...shown.add(id)].join(" ");
      }, 50);
      return { id: nextStream++ };
    },
  });
  return backend;
}
