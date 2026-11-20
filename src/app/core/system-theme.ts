/** Calls `set` with the OS dark appearance now and on every change; returns the stop function. */
export function followSystemTheme(set: (dark: boolean) => void): () => void {
  if (typeof matchMedia !== "function") return () => undefined;
  const query = matchMedia("(prefers-color-scheme: dark)");
  const update = () => set(query.matches);
  update();
  query.addEventListener("change", update);
  return () => query.removeEventListener("change", update);
}
