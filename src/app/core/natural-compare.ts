/**
 * Compares two names like `poimen_core::order::natural_cmp` (the TUI): case ignored, runs of ASCII
 * digits as numbers (`#9` before `#12`), accents significant (code points after lowercasing).
 * `fixtures/name-order.json` of poimen-core is read by both specs.
 */
export function naturalCompare(a: string, b: string): number {
  let i = 0;
  let j = 0;
  const digit = (c: string | undefined) => c !== undefined && c >= "0" && c <= "9";
  while (i < a.length && j < b.length) {
    if (digit(a[i]) && digit(b[j])) {
      let ie = i;
      let je = j;
      while (digit(a[ie])) ie++;
      while (digit(b[je])) je++;
      const x = a.slice(i, ie).replace(/^0+/, "");
      const y = b.slice(j, je).replace(/^0+/, "");
      if (x.length !== y.length) return x.length < y.length ? -1 : 1;
      if (x !== y) return x < y ? -1 : 1;
      i = ie;
      j = je;
      continue;
    }
    const x = String.fromCodePoint(a.codePointAt(i)!);
    const y = String.fromCodePoint(b.codePointAt(j)!);
    i += x.length;
    j += y.length;
    const lx = Array.from(x.toLowerCase(), (c) => c.codePointAt(0)!);
    const ly = Array.from(y.toLowerCase(), (c) => c.codePointAt(0)!);
    for (let k = 0; k < Math.min(lx.length, ly.length); k++) {
      if (lx[k] !== ly[k]) return lx[k] < ly[k] ? -1 : 1;
    }
    if (lx.length !== ly.length) return lx.length < ly.length ? -1 : 1;
  }
  const ra = a.length - i;
  const rb = b.length - j;
  return ra === rb ? 0 : ra === 0 ? -1 : rb === 0 ? 1 : 0;
}
