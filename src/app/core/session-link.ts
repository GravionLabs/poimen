/** The agent-events run and task a terminal's session was started for (`poimen session run --run --task`, #744, read by #745). */
export interface SessionLink {
  run?: string;
  task?: string;
}
