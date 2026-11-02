import { Injectable, computed, inject, signal } from "@angular/core";
import { mergeSchemes, parseHex, type ColorScheme } from "./color-scheme";
import { TauriBridge } from "./tauri";

const isColor = (v: unknown): v is string => {
  if (typeof v !== "string") return false;
  try {
    parseHex(v);
    return true;
  } catch {
    return false;
  }
};

/** A scheme of the wire shape of `schemes.rs`, or `null` when it is malformed. */
function parseScheme(raw: unknown): ColorScheme | null {
  if (typeof raw !== "object" || raw === null) return null;
  const r = raw as Record<string, unknown>;
  const colors = [
    "background",
    "foreground",
    "cursor",
    "selectionBackground",
    "selectionForeground",
  ];
  if (typeof r["name"] !== "string" || r["name"] === "") return null;
  if (!colors.every((k) => isColor(r[k]))) return null;
  const ansi = r["ansi"];
  if (!Array.isArray(ansi) || ansi.length !== 16 || !ansi.every(isColor)) return null;
  return {
    name: r["name"],
    label: typeof r["label"] === "string" ? r["label"] : r["name"],
    background: r["background"] as string,
    foreground: r["foreground"] as string,
    cursor: r["cursor"] as string,
    selectionBackground: r["selectionBackground"] as string,
    selectionForeground: r["selectionForeground"] as string,
    ansi: ansi as string[],
  };
}

/** The user color schemes (`~/.config/poimen/themes/`), merged over the bundled ones. */
@Injectable({ providedIn: "root" })
export class UserSchemes {
  private readonly bridge = inject(TauriBridge);
  private readonly _schemes = signal<readonly ColorScheme[]>([]);
  readonly schemes = this._schemes.asReadonly();
  /** Bundled plus user schemes by name; a user scheme wins over a bundled one. */
  readonly registry = computed(() => mergeSchemes(this._schemes()));

  async start(): Promise<void> {
    await this.bridge.listen<unknown>("color-schemes", (s) => this.apply(s));
    try {
      this.apply(await this.bridge.invoke("get_color_schemes"));
    } catch {
      // keep the bundled schemes
    }
  }

  private apply(raw: unknown): void {
    const r = (typeof raw === "object" && raw !== null ? raw : {}) as Record<string, unknown>;
    const list = Array.isArray(r["schemes"]) ? r["schemes"] : [];
    const schemes: ColorScheme[] = [];
    for (const item of list) {
      const scheme = parseScheme(item);
      if (scheme) schemes.push(scheme);
      else console.warn("poimen color schemes: ignoring a malformed scheme from the backend");
    }
    if (Array.isArray(r["warnings"]))
      for (const w of r["warnings"]) if (typeof w === "string") console.warn(`poimen ${w}`);
    this._schemes.set(schemes);
  }
}
