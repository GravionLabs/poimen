/**
 * Space templates (`spaceTemplates[]` of the user config, `src-tauri/src/templates.rs`): the wire
 * shape the backend sends, the JSON written back, and the validation of the settings form, which
 * mirrors `settings::parse_space_templates` (the backend validates again before it writes).
 */
import {
  isValidLayout,
  layoutErrors,
  parseSpaceLayout,
  toConfig,
  type SpaceLayout,
} from "./space-layout";

export type RemoteMatch =
  | { provider: "github"; owner: string }
  | { provider: "azure-devops"; organization: string; project: string };

export interface SpaceTemplate {
  name: string;
  /** `match.dir`: absolute, or starting with `~`. */
  dir: string | null;
  remote: RemoteMatch | null;
  /** A harness id. */
  agent: string;
  model: string | null;
  layout: SpaceLayout | null;
}

/** The template a directory resolves to (`resolve_space_template`). */
export interface TemplateHint {
  name: string;
  agent: string;
}

function parseRemote(raw: unknown): RemoteMatch | null {
  if (typeof raw !== "object" || raw === null) return null;
  const r = raw as Record<string, unknown>;
  const text = (v: unknown) => (typeof v === "string" ? v : "");
  if (r["provider"] === "github") return { provider: "github", owner: text(r["owner"]) };
  if (r["provider"] === "azure-devops")
    return {
      provider: "azure-devops",
      organization: text(r["organization"]),
      project: text(r["project"]),
    };
  return null;
}

/** The templates of a settings payload; malformed entries are skipped. */
export function parseSpaceTemplates(raw: unknown): SpaceTemplate[] {
  if (!Array.isArray(raw)) return [];
  return raw.flatMap((item): SpaceTemplate[] => {
    if (typeof item !== "object" || item === null) return [];
    const r = item as Record<string, unknown>;
    if (typeof r["name"] !== "string" || typeof r["agent"] !== "string") return [];
    return [
      {
        name: r["name"],
        dir: typeof r["dir"] === "string" ? r["dir"] : null,
        remote: parseRemote(r["remote"]),
        agent: r["agent"],
        model: typeof r["model"] === "string" ? r["model"] : null,
        layout: parseSpaceLayout(r["layout"]),
      },
    ];
  });
}

/** An empty draft for "Add template". */
export function emptyTemplate(agent: string): SpaceTemplate {
  return { name: "", dir: null, remote: null, agent, model: null, layout: null };
}

/** The JSON of one entry in `spaceTemplates`: trimmed values, no empty optional field. */
export function templateConfig(t: SpaceTemplate): Record<string, unknown> {
  const match: Record<string, unknown> = {};
  const dir = t.dir?.trim();
  if (dir) match["dir"] = dir;
  if (t.remote?.provider === "github") {
    match["remote"] = { provider: "github", owner: t.remote.owner.trim() };
  } else if (t.remote) {
    match["remote"] = {
      provider: "azure-devops",
      organization: t.remote.organization.trim(),
      project: t.remote.project.trim(),
    };
  }
  const out: Record<string, unknown> = { name: t.name.trim(), match, agent: t.agent.trim() };
  const model = t.model?.trim();
  if (model) out["model"] = model;
  if (t.layout) out["layout"] = toConfig(t.layout.panes);
  return out;
}

/** Whether `dir` is absolute (POSIX, Windows, UNC) or starts with `~` (`templates::is_valid_dir`). */
export function isValidDir(dir: string): boolean {
  return (
    dir === "~" ||
    dir.startsWith("~/") ||
    dir.startsWith("~\\") ||
    dir.startsWith("/") ||
    dir.startsWith("\\") ||
    /^[A-Za-z]:[\\/]/.test(dir)
  );
}

export interface TemplateErrors {
  name?: string;
  match?: string;
  dir?: string;
  remote?: string;
  agent?: string;
  layout?: string;
}

/**
 * Problems of the draft, per field. `others` are the names of the other templates (names are
 * unique), `harnessIds` the configured harnesses.
 */
export function templateErrors(
  draft: SpaceTemplate,
  others: readonly string[],
  harnessIds: readonly string[],
): TemplateErrors {
  const errors: TemplateErrors = {};
  const name = draft.name.trim();
  if (!name) errors.name = "A name is required";
  else if (others.includes(name)) errors.name = `Another template is called "${name}"`;
  const dir = draft.dir?.trim() ?? "";
  if (dir && !isValidDir(dir)) errors.dir = "Use an absolute directory or one starting with ~";
  if (draft.remote) {
    const fields =
      draft.remote.provider === "github"
        ? [draft.remote.owner]
        : [draft.remote.organization, draft.remote.project];
    if (fields.some((f) => !f.trim())) {
      errors.remote =
        draft.remote.provider === "github"
          ? "The owner is required"
          : "The organization and the project are required";
    }
  }
  if (!dir && !draft.remote) errors.match = "Set a directory, a remote, or both";
  const agent = draft.agent.trim();
  if (!agent) errors.agent = "Choose an agent";
  else if (!harnessIds.includes(agent)) errors.agent = `Unknown harness "${agent}"`;
  if (draft.layout && !isValidLayout(draft.layout.panes)) {
    errors.layout = layoutErrors(draft.layout.panes).find((e) => e !== null) ?? "Add a pane";
  }
  return errors;
}

export function hasErrors(errors: TemplateErrors): boolean {
  return Object.keys(errors).length > 0;
}

/** The match of a template in words: `~/src`, `github acme`, or both joined by " + ". */
export function matchLabel(t: SpaceTemplate): string {
  const parts: string[] = [];
  if (t.dir) parts.push(t.dir);
  if (t.remote?.provider === "github") parts.push(`github.com/${t.remote.owner}`);
  else if (t.remote) parts.push(`azure ${t.remote.organization}/${t.remote.project}`);
  return parts.join(" + ");
}
