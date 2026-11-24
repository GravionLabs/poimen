// The answers the app needs to start on the fake variant (#1156): the commands it calls at startup, each
// with the emptiest value it accepts, and a snapshot with one space and one shell terminal. A test adds
// or replaces answers through `openFake(page, { ...startup(), snapshot })` or at runtime (`window.__demo`).
export const SPACE = {
  space_id: "s1",
  label: "alpha",
  number: 1,
  agent_status: "unknown",
  cwd: "/work/alpha",
};

/** One shell terminal in one tab of the space `alpha`. */
export const SNAPSHOT = {
  spaces: [SPACE],
  tabs: [
    {
      tab_id: "tab1",
      space_id: "s1",
      label: "shell",
      number: 1,
      agent_status: "unknown",
      root: { kind: "terminal", terminal_id: "t1" },
      active_terminal: "t1",
    },
  ],
  terminals: [
    {
      terminal_id: "t1",
      tab_id: "tab1",
      space_id: "s1",
      agent_status: "unknown",
      focused: true,
      survives_restart: true,
      cwd: "/work/alpha",
      program: "zsh",
      label: "shell",
    },
  ],
};

/** What the app calls at startup, answered with nothing to show. */
export function startup(snapshot: unknown = SNAPSHOT): Record<string, unknown> {
  return {
    app_version: "0.0.0",
    app_channel: "stable",
    ensure_backend: { state: "running", reason: null },
    snapshot,
    get_settings: {},
    get_color_schemes: { schemes: [], warnings: [] },
    list_machines: [],
    usage_claude: [],
    seed_space_defaults: null,
    get_harnesses: [],
    deliver_watch: [],
    set_terminal_colors: null,
    subagents_unwatch: null,
    backend_info: { version: "0.0.0", protocol: 1, supported: 1 },
    repo_info: { branch: null, worktree: false },
    hooks_status: [],
    agent_grants: { grants: [] },
    branch_pulls: {},
    project_entries: { commands: [], presets: [], locked: false },
    detected_entries: [],
    repo_root: "/repo",
  };
}
