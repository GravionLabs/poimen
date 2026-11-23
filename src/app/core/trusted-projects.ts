/** The `trustedProjects` map; entries that are not strings are skipped. */
export function parseTrusted(raw: unknown): Record<string, string> {
  if (typeof raw !== "object" || raw === null || Array.isArray(raw)) return {};
  return Object.fromEntries(
    Object.entries(raw).filter((e): e is [string, string] => typeof e[1] === "string"),
  );
}
