import type { AgentStatus } from "./snapshot";

/** A child record of a terminal (OSC 7501 with an `id`, #684): a row below the terminal. */
export interface ChildRecord {
  /** `/`-separated path such as `run/issue-12`; a parent's id is a prefix of its children's. */
  id: string;
  status: AgentStatus;
  app?: string;
  message?: string;
  progress?: number;
  kind?: string;
  tone?: string;
  title?: string;
}
