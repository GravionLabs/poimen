import { describe, expect, it } from "vitest";
import { statusText, type Terminal } from "./snapshot";

describe("an ended terminal with a session to resume (#1232)", () => {
  it("shows as ended, can resume, not as hibernated", () => {
    const pane = { terminal_id: "e", hibernated: true, ended: true, agent_status: "unknown" };
    expect(statusText(pane as Terminal)).toBe("ended, can resume");
    expect(statusText({ ...pane, ended: false } as Terminal)).toBe("hibernated");
  });
});
