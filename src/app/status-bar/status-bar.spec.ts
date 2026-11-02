import { describe, expect, it } from "vitest";
import { connectionLabel, sizeLabel } from "./status-bar";

describe("connectionLabel", () => {
  it("names a connected herdr", () => {
    expect(connectionLabel({ state: "connected" })).toBe("herdr connected");
  });
  it("shows the reason when reconnecting", () => {
    expect(connectionLabel({ state: "reconnecting", reason: "socket closed" })).toBe(
      "herdr reconnecting: socket closed",
    );
  });
  it("shows the reason when unreachable, and copes with an empty one", () => {
    expect(connectionLabel({ state: "unreachable", reason: "no socket" })).toBe(
      "herdr unreachable: no socket",
    );
    expect(connectionLabel({ state: "unreachable", reason: "" })).toBe("herdr unreachable");
  });
});

describe("sizeLabel", () => {
  it("formats cols by rows", () => {
    expect(sizeLabel({ cols: 120, rows: 30 })).toBe("120×30");
  });
  it("is empty until measured", () => {
    expect(sizeLabel(undefined)).toBe("");
  });
});
