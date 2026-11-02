import { Injector } from "@angular/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { fixedSchemeName, resolveScheme } from "./color-scheme";
import { TauriBridge } from "./tauri";
import { tauri } from "./tauri.mock";
import { UserSchemes } from "./user-schemes";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("./tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("./tauri.mock")).tauri.listen,
}));

const wire = (name: string, background = "#101010") => ({
  name,
  label: name,
  background,
  foreground: "#eeeeee",
  cursor: "#eeeeee",
  selectionBackground: "#333333",
  selectionForeground: "#ffffff",
  ansi: Array.from(
    { length: 16 },
    (_, i) => `#${(i * 15).toString(16).padStart(2, "0").repeat(3)}`,
  ),
});

const create = () => Injector.create({ providers: [TauriBridge, UserSchemes] }).get(UserSchemes);

beforeEach(() => tauri.reset());

describe("UserSchemes", () => {
  it("adds the schemes of get_color_schemes to the registry", async () => {
    tauri.answer("get_color_schemes", () => ({ schemes: [wire("my-scheme")], warnings: [] }));
    const u = create();
    await u.start();
    expect(resolveScheme("my-scheme", u.registry())?.background).toBe("#101010");
    expect(u.registry().has("dracula")).toBe(true);
  });

  it("applies color-schemes events without a restart", async () => {
    tauri.answer("get_color_schemes", () => ({ schemes: [], warnings: [] }));
    const u = create();
    await u.start();
    tauri.emit("color-schemes", { schemes: [wire("late")], warnings: [] });
    expect(u.registry().has("late")).toBe(true);
    tauri.emit("color-schemes", { schemes: [], warnings: [] });
    expect(u.registry().has("late")).toBe(false);
  });

  it("lets a user scheme replace a bundled one of the same name", async () => {
    tauri.answer("get_color_schemes", () => ({ schemes: [wire("dracula", "#000001")] }));
    const u = create();
    await u.start();
    expect(u.registry().get("dracula")?.background).toBe("#000001");
  });

  it("makes a user scheme fix light or dark", async () => {
    tauri.answer("get_color_schemes", () => ({ schemes: [wire("mine")] }));
    const u = create();
    await u.start();
    expect(fixedSchemeName("mine")).toBeNull();
    expect(fixedSchemeName("mine", u.registry())).toBe("mine");
  });

  it("logs backend warnings and drops malformed schemes", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    tauri.answer("get_color_schemes", () => ({
      schemes: [wire("ok"), { ...wire("bad"), background: "red" }, { name: "short", ansi: [] }, 3],
      warnings: ["color scheme x.toml: boom; skipped"],
    }));
    const u = create();
    await u.start();
    expect(u.schemes().map((s) => s.name)).toEqual(["ok"]);
    expect(warn).toHaveBeenCalledWith("poimen color scheme x.toml: boom; skipped");
    expect(warn).toHaveBeenCalledTimes(4);
  });

  it("keeps the bundled schemes when the command fails or answers garbage", async () => {
    tauri.answer("get_color_schemes", () => {
      throw new Error("no");
    });
    const u = create();
    await u.start();
    expect(u.schemes()).toEqual([]);
    tauri.emit("color-schemes", "nonsense");
    expect(u.schemes()).toEqual([]);
  });
});
