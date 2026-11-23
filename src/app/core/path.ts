/** The path `file` (relative to `dir`, or absolute as it is) as one path, with `/` between the parts. */
export function joinPath(dir: string, file: string): string {
  if (/^([\\/]|[A-Za-z]:[\\/])/.test(file)) return file;
  return `${dir.replace(/[\\/]+$/, "")}/${file.replace(/^[\\/]+/, "")}`;
}
