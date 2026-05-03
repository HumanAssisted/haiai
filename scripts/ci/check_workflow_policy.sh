#!/usr/bin/env bash
#
# Workflow policy linter.
#
# Catches structural errors in .github/workflows/*.yml that would otherwise
# only surface as a runtime CI failure (or worse, a private-content leak):
#
#   1. Public-repo workflows must not check out private repos. The default
#      GITHUB_TOKEN can't read private repos in the same org, and gating on
#      a PAT secret leaks private-repo file names / artifacts into public
#      Actions logs. The cross-repo-jacsdb-e2e job (removed 2026-05-03) is
#      the regression this rule prevents from coming back.
#
# Add new rules below as they earn their place. Keep each rule:
#   - Cheap (grep / awk only — no YAML parser dep)
#   - Precise (false positives drive engineers to disable the lint)
#   - Self-explaining (the error message must tell the reader what to fix)

set -euo pipefail

ROOT="${1:-.}"
cd "$ROOT"

WORKFLOWS_DIR=".github/workflows"

if [[ ! -d "$WORKFLOWS_DIR" ]]; then
  echo "check_workflow_policy: no $WORKFLOWS_DIR/ directory found at $ROOT — nothing to lint."
  exit 0
fi

status=0

# -------------------------------------------------------------------------
# Rule 1: no private-repo checkouts.
#
# We allowlist the public repos this org's public CI legitimately checks out.
# Anything else triggers the rule.
# -------------------------------------------------------------------------
PRIVATE_REPO_ALLOWLIST_REGEX='^(HumanAssisted/(JACS|haiai))$'

# Find every `repository: <owner/repo>` line in any workflow file. The
# `actions/checkout@v4` action accepts the repo name in this exact field.
matches="$(grep -RnE 'repository:[[:space:]]*[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+' \
    "$WORKFLOWS_DIR" || true)"

if [[ -n "$matches" ]]; then
  while IFS= read -r line; do
    [[ -z "$line" ]] && continue
    repo="$(echo "$line" | sed -E 's/.*repository:[[:space:]]*([A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+).*/\1/')"
    if [[ ! "$repo" =~ $PRIVATE_REPO_ALLOWLIST_REGEX ]]; then
      echo "ERROR: workflow checks out non-allowlisted repo '$repo':"
      echo "  $line"
      echo
      echo "  Public-repo workflows must only check out other public repos."
      echo "  If '$repo' is private, move the workflow to that private repo's"
      echo "  CI instead. If '$repo' is a public repo this org legitimately"
      echo "  uses, add it to PRIVATE_REPO_ALLOWLIST_REGEX in this script."
      status=1
    fi
  done <<<"$matches"
fi

if [[ "$status" -eq 0 ]]; then
  echo "check_workflow_policy: OK"
fi

exit "$status"
