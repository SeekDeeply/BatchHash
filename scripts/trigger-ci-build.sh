#!/usr/bin/env bash
# BatchHash "build" entry point.
#
# Local builds are forbidden. This script only dispatches a GitHub Actions run,
# which is the single build/test environment for this repository.
#
# Usage:
#   ./scripts/trigger-ci-build.sh              # dispatch CI verification
#   ./scripts/trigger-ci-build.sh --release    # dispatch Release workflow (needs tag inputs)
#
# Requires: gh CLI authenticated, or GITHUB_TOKEN + GITHUB_REPOSITORY in the env.
set -euo pipefail

REPO="${GITHUB_REPOSITORY:-SeekDeeply/BatchHash}"
WORKFLOW="ci.yml"
REF="${GITHUB_REF_NAME:-main}"

if [ "${1:-}" = "--release" ]; then
  WORKFLOW="release.yml"
fi

if command -v gh >/dev/null 2>&1; then
  gh workflow run "$WORKFLOW" --repo "$REPO" --ref "$REF"
  echo "Dispatched $WORKFLOW on $REPO@$REF"
  gh run list --repo "$REPO" --workflow "$WORKFLOW" --limit 3
  exit 0
fi

if [ -z "${GITHUB_TOKEN:-}" ]; then
  echo "error: no gh CLI and no GITHUB_TOKEN; cannot dispatch CI" >&2
  exit 1
fi

curl -sSf -X POST \
  -H "Authorization: Bearer ${GITHUB_TOKEN}" \
  -H "Accept: application/vnd.github+json" \
  "https://api.github.com/repos/${REPO}/actions/workflows/${WORKFLOW}/dispatches" \
  -d "{\"ref\":\"${REF}\"}"
echo "Dispatched $WORKFLOW on $REPO@$REF"
