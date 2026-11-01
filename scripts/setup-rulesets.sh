#!/usr/bin/env bash
# Creates the branch rulesets `main` and `release` of GravionLabs/poimen, like the other GravionLabs repositories.
# Run it once after the workflows have reported their checks on a pull request, so the required contexts exist:
#   ci                     gate job of .github/workflows/ci.yml (frontend and Rust checks)
#   pr-checks / PR checks  aggregate job of .github/workflows/pr-checks.yml (readiness, title, ADR, spec)
# Repository admins (RepositoryRole 5) bypass both rulesets; the release workflow pushes the changelog commit to main
# with GH_PAT of an admin. Needs gh with admin rights on the repository. An existing ruleset of the same name is
# updated instead of duplicated.
set -euo pipefail

repo="${1:-GravionLabs/poimen}"

checks='[{"context":"ci"},{"context":"pr-checks / PR checks"}]'
bypass='[{"actor_id":5,"actor_type":"RepositoryRole","bypass_mode":"always"}]'

main_ruleset=$(cat <<EOF
{
  "name": "main",
  "target": "branch",
  "enforcement": "active",
  "conditions": {"ref_name": {"include": ["~DEFAULT_BRANCH"], "exclude": []}},
  "bypass_actors": $bypass,
  "rules": [
    {"type": "deletion"},
    {"type": "non_fast_forward"},
    {"type": "pull_request", "parameters": {
      "required_approving_review_count": 0,
      "dismiss_stale_reviews_on_push": false,
      "require_code_owner_review": false,
      "require_last_push_approval": false,
      "required_review_thread_resolution": false,
      "allowed_merge_methods": ["merge"]
    }},
    {"type": "required_status_checks", "parameters": {
      "strict_required_status_checks_policy": false,
      "do_not_enforce_on_create": false,
      "required_status_checks": $checks
    }}
  ]
}
EOF
)

release_ruleset=$(cat <<EOF
{
  "name": "release",
  "target": "branch",
  "enforcement": "active",
  "conditions": {"ref_name": {"include": ["refs/heads/release/**"], "exclude": []}},
  "bypass_actors": $bypass,
  "rules": [
    {"type": "non_fast_forward"},
    {"type": "required_status_checks", "parameters": {
      "strict_required_status_checks_policy": false,
      "do_not_enforce_on_create": true,
      "required_status_checks": $checks
    }},
    {"type": "pull_request", "parameters": {
      "required_approving_review_count": 0,
      "dismiss_stale_reviews_on_push": false,
      "require_code_owner_review": false,
      "require_last_push_approval": false,
      "require_extra_approval_for_unattributed_changes": true,
      "required_review_thread_resolution": false,
      "required_reviewers": [],
      "allowed_merge_methods": ["squash"]
    }}
  ]
}
EOF
)

apply() {
  local name="$1" body="$2" id
  id=$(gh api "repos/$repo/rulesets" --jq ".[] | select(.name == \"$name\") | .id")
  if [ -n "$id" ]; then
    echo "Updating ruleset $name ($id)"
    gh api --method PUT "repos/$repo/rulesets/$id" --input - <<<"$body" >/dev/null
  else
    echo "Creating ruleset $name"
    gh api --method POST "repos/$repo/rulesets" --input - <<<"$body" >/dev/null
  fi
}

apply main "$main_ruleset"
apply release "$release_ruleset"
