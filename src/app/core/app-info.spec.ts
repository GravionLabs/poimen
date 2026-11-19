import { describe, expect, it } from "vitest";
import { readAppInfo } from "./app-info";

const bridge = (answers: Record<string, unknown>) => ({
  invoke: async (command: string) => {
    if (!(command in answers)) throw new Error("no backend");
    return answers[command] as never;
  },
});

describe("readAppInfo", () => {
  it("reads version and channel from the backend", async () => {
    expect(await readAppInfo(bridge({ app_version: "1.2.3", app_channel: "rc" }))).toEqual({
      version: "1.2.3",
      channel: "rc",
    });
  });

  it("falls back to a stable build without a backend", async () => {
    expect(await readAppInfo(bridge({}))).toEqual({ version: "", channel: "stable" });
  });
});
