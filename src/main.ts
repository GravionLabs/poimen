import "@xterm/xterm/css/xterm.css";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  applyStatus,
  buildTree,
  paneLabel,
  waitingCount,
  type AgentStatus,
  type Pane,
  type Snapshot,
} from "./model";
import { streams, TerminalView, type StreamRecord } from "./terminal-view";

interface HerdrEvent {
  kind: string;
  data: { pane_id?: string; agent_status?: AgentStatus; reason?: string } | null;
}

const sidebar = document.querySelector<HTMLElement>("#tree")!;
const views = document.querySelector<HTMLElement>("#views")!;
const empty = document.querySelector<HTMLElement>("#empty")!;
const connection = document.querySelector<HTMLElement>("#connection")!;

let snapshot: Snapshot = { workspaces: [], tabs: [], panes: [] };
const open = new Map<string, TerminalView>();

function renderSidebar() {
  sidebar.innerHTML = "";
  for (const { workspace, tabs } of buildTree(snapshot)) {
    const group = document.createElement("details");
    group.className = "workspace";
    group.open = true;
    const summary = document.createElement("summary");
    summary.innerHTML = `<span class="dot status-${workspace.agent_status}"></span><span></span>`;
    summary.lastElementChild!.textContent = workspace.label;
    group.append(summary);

    for (const { tab, panes } of tabs) {
      for (const pane of panes) {
        const row = document.createElement("button");
        row.className = "pane-row";
        row.classList.toggle("open", open.has(pane.pane_id));
        row.title = `${pane.pane_id}${pane.cwd ? ` · ${pane.cwd}` : ""}`;
        row.innerHTML = `<span class="dot status-${pane.agent_status}"></span><span class="label"></span><span class="tab"></span>`;
        row.querySelector(".label")!.textContent = paneLabel(pane);
        row.querySelector(".tab")!.textContent = panes.length > 1 ? `${tab.label} · ${pane.pane_id}` : tab.label;
        row.addEventListener("click", () => openPane(pane));
        group.append(row);
      }
    }
    sidebar.append(group);
  }
  const waiting = waitingCount(snapshot.panes);
  document.title = waiting > 0 ? `poimen (${waiting} waiting)` : "poimen";
  empty.hidden = open.size > 0;
}

function syncViews() {
  for (const [paneId, view] of open) {
    const pane = snapshot.panes.find((p) => p.pane_id === paneId);
    if (pane) view.update(pane);
  }
}

function openPane(pane: Pane) {
  const existing = open.get(pane.pane_id);
  if (existing) {
    if (existing.collapsed) existing.expand();
    existing.element.scrollIntoView({ behavior: "smooth", block: "nearest" });
    return;
  }
  const view = new TerminalView(pane, (removed) => {
    open.delete(removed.pane.pane_id);
    renderSidebar();
  });
  open.set(pane.pane_id, view);
  views.append(view.element);
  view.mount();
  renderSidebar();
}

let refreshTimer: number | undefined;
let refreshing = false;
let refreshAgain = false;

/** Events are invalidation signals; snapshots are authoritative (see herdr socket-api docs). */
function scheduleRefresh() {
  clearTimeout(refreshTimer);
  refreshTimer = window.setTimeout(refresh, 80);
}

async function refresh() {
  if (refreshing) {
    refreshAgain = true;
    return;
  }
  refreshing = true;
  try {
    snapshot = await invoke<Snapshot>("snapshot");
    renderSidebar();
    syncViews();
  } catch (err) {
    setConnection(false, String(err));
  } finally {
    refreshing = false;
    if (refreshAgain) {
      refreshAgain = false;
      void refresh();
    }
  }
}

function setConnection(ok: boolean, reason = "") {
  connection.hidden = ok;
  connection.textContent = ok ? "" : `herdr not reachable: ${reason}`;
}

void listen<HerdrEvent>("herdr", ({ payload }) => {
  switch (payload.kind) {
    case "poimen.connected":
      setConnection(true);
      break;
    case "poimen.disconnected":
      setConnection(false, payload.data?.reason ?? "");
      return;
    case "pane.agent_status_changed":
      if (payload.data?.pane_id && payload.data.agent_status) {
        snapshot = applyStatus(snapshot, payload.data.pane_id, payload.data.agent_status);
        renderSidebar();
        syncViews();
      }
      break;
  }
  scheduleRefresh();
});

void listen<{ id: number; record: StreamRecord }>("terminal", ({ payload }) => {
  streams.get(payload.id)?.(payload.record);
});

void listen<string>("menu", ({ payload }) => {
  switch (payload) {
    case "expand_all":
      open.forEach((view) => view.collapsed && view.expand());
      break;
    case "collapse_all":
      open.forEach((view) => !view.collapsed && view.collapse());
      break;
    case "close_all":
      [...open.values()].forEach((view) => view.remove());
      break;
    case "refresh":
      void refresh();
      break;
  }
});

void refresh();
