import { Injectable, signal } from "@angular/core";

/** The repository (`owner/name`) the GitHub tab is narrowed to; `null` is all repositories. */
@Injectable({ providedIn: "root" })
export class GithubFilter {
  readonly repo = signal<string | null>(null);

  /** Whether `repo` passes the filter. */
  allows(repo: string): boolean {
    const r = this.repo();
    return r === null || r === repo;
  }
}
