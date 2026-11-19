import { TauriBridge } from "./tauri";
import type { AppChannel } from "./backend";

/** Version and channel of the running build, read from the backend (`app_version`, `app_channel`). */
export async function readAppInfo(
  bridge: Pick<TauriBridge, "invoke">,
): Promise<{ version: string; channel: AppChannel }> {
  const [version, channel] = await Promise.all([
    bridge.invoke<string | undefined>("app_version").catch(() => undefined),
    bridge.invoke<string | undefined>("app_channel").catch(() => undefined),
  ]);
  return { version: version ?? "", channel: channel === "rc" ? "rc" : "stable" };
}
