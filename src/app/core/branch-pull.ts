import type { Checks } from "../github/pulls";

/** The pull request of a branch as `branch_pulls` returns it (`BranchPull` of `github_branches.rs`). */
export interface BranchPull {
  number: number;
  title: string;
  url: string;
  state: "open" | "merged";
  isDraft: boolean;
  checks: Checks;
  /** The head commit the checks belong to (#995). */
  headSha?: string;
  baseRef?: string;
  /** The head branch lives in a fork, where poimen cannot push. */
  isCrossRepository?: boolean;
}
