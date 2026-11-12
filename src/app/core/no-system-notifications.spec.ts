import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

/** Every source file below `dir`. */
function files(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    return statSync(path).isDirectory() ? files(path) : [path];
  });
}

/**
 * poimen sends no OS notifications (#676): on macOS a notification with an action waits on a
 * thread and polls the Notification Center from the main run loop until it is dismissed, and
 * enough of them froze the window after a few hours.
 */
describe("no system notifications (#676)", () => {
  it("nothing imports the notification plugin", () => {
    const users = files("src")
      .filter((f) => f.endsWith(".ts") && !f.endsWith("no-system-notifications.spec.ts"))
      .filter((f) => readFileSync(f, "utf8").includes("@tauri-apps/plugin-notification"));
    expect(users).toEqual([]);
  });

  it("the app neither depends on the plugin nor grants its permission", () => {
    const pkg = JSON.parse(readFileSync("package.json", "utf8"));
    expect(Object.keys({ ...pkg.dependencies, ...pkg.devDependencies })).not.toContain(
      "@tauri-apps/plugin-notification",
    );
    const capability = JSON.parse(readFileSync("src-tauri/capabilities/default.json", "utf8"));
    expect(
      capability.permissions.filter((p: string) => String(p).startsWith("notification:")),
    ).toEqual([]);
  });
});
