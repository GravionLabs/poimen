import { Injectable, signal } from "@angular/core";

/** The directory last picked in the explorer; the palette entry "Open terminal here" uses it. */
@Injectable({ providedIn: "root" })
export class ExplorerSelection {
  private readonly _dir = signal<string | null>(null);
  readonly dir = this._dir.asReadonly();

  select(dir: string | null): void {
    this._dir.set(dir);
  }
}
