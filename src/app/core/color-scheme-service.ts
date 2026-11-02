import { Injectable, inject, signal } from "@angular/core";
import { HxTheme } from "@gravionlabs/helix-ui";
import { appearance, deriveTokens, resolveScheme, type ColorScheme } from "./color-scheme";
import { UserSchemes } from "./user-schemes";

/**
 * Puts a scheme's derived tokens on `<html>` and sets helix' light/dark class from its background.
 * Without a scheme (no `colorSchema`, `poimen`, or an unknown name) the tokens of `styles.css` are in
 * charge and `theme` picks light or dark.
 */
@Injectable({ providedIn: "root" })
export class ColorSchemeService {
  private readonly theme = inject(HxTheme);
  private readonly user = inject(UserSchemes);
  private applied: string[] = [];
  private lastName: string | null | undefined;
  /** Bumped when a preview ends, so the saved scheme (and light or dark) is applied again. */
  readonly restored = signal(0);

  /**
   * Applies the scheme named by `colorSchema`. Returns whether a scheme now decides light or dark
   * (then `theme` is ignored); `false` means the caller follows `theme`. An unknown name warns once.
   */
  sync(name: string | null): boolean {
    const changed = name !== this.lastName;
    this.lastName = name;
    const scheme = resolveScheme(name, this.user.registry(), changed ? undefined : () => undefined);
    if (!scheme) {
      this.clear();
      return false;
    }
    this.apply(scheme);
    return true;
  }

  /** Shows a scheme without saving it; `revert` brings the saved one back. Unknown names are ignored. */
  preview(name: string): void {
    const scheme = resolveScheme(name, this.user.registry(), () => undefined);
    if (scheme) this.apply(scheme);
  }

  /** Ends a preview: the saved scheme is applied again. */
  revert(): void {
    this.restored.update((n) => n + 1);
  }

  private apply(scheme: ColorScheme): void {
    const root = document.documentElement;
    const tokens = deriveTokens(scheme);
    for (const name of this.applied) if (!(name in tokens)) root.style.removeProperty(name);
    for (const [name, value] of Object.entries(tokens)) root.style.setProperty(name, value);
    this.applied = Object.keys(tokens);
    this.theme.setDark(appearance(scheme) === "dark");
  }

  private clear(): void {
    const root = document.documentElement;
    for (const name of this.applied) root.style.removeProperty(name);
    this.applied = [];
  }
}
