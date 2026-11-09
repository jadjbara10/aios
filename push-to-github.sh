#!/usr/bin/env bash
# AIOS — one-command public-repo publisher.
#
# Creates the PUBLIC GitHub repository, sets topics, and pushes main.
# Run from anywhere; the script cds to its own directory (the repo root).
#
# Usage:
#   export GH_TOKEN=ghp_<fresh token with repo/public_repo scope>
#   ./push-to-github.sh [repo-name]     # default name: aios
#
# Security notes:
#   - The token is read from the GH_TOKEN environment variable ONLY.
#     It is never written to any file inside the repository.
#   - The push uses a one-off URL; the stored `origin` remote is the
#     clean https URL without credentials.
#   - Revoke the token right after the push succeeds.

set -euo pipefail
cd "$(dirname "$0")"

REPO_NAME="${1:-aios}"
: "${GH_TOKEN:?Set GH_TOKEN to a fresh personal access token (repo scope), e.g. export GH_TOKEN=ghp_...}"
TOKEN="$GH_TOKEN"
API="https://api.github.com"
auth=(-H "Authorization: Bearer $TOKEN" -H "Accept: application/vnd.github+json")

echo "==> Verifying token..."
USER_JSON=$(curl -sS -m 20 "${auth[@]}" "$API/user")
LOGIN=$(printf '%s' "$USER_JSON" | grep -o '"login": *"[^"]*"' | head -1 | cut -d'"' -f4)
if [ -z "$LOGIN" ]; then
  echo "Token rejected by GitHub:"
  printf '%s\n' "$USER_JSON"
  exit 1
fi
echo "    authenticated as: $LOGIN"

echo "==> Creating PUBLIC repository $LOGIN/$REPO_NAME..."
CREATE=$(curl -sS -m 30 -X POST "${auth[@]}" "$API/user/repos" -d "{
  \"name\": \"$REPO_NAME\",
  \"description\": \"AIOS v0.1 Alpha - AI-Native Operating System. Rust no_std hybrid microkernel, UEFI boot, local AI engine (GGUF), multi-agent federation. Author: Jad Jbara.\",
  \"private\": false,
  \"has_issues\": true,
  \"has_wiki\": false,
  \"auto_init\": false
}")
FULL_NAME=$(printf '%s' "$CREATE" | grep -o '"full_name": *"[^"]*"' | head -1 | cut -d'"' -f4)
if [ -z "$FULL_NAME" ]; then
  if printf '%s' "$CREATE" | grep -q 'already exists'; then
    FULL_NAME="$LOGIN/$REPO_NAME"
    echo "    repository already exists, pushing into it"
  else
    echo "Repository creation failed:"
    printf '%s\n' "$CREATE"
    exit 1
  fi
else
  echo "    created: $FULL_NAME"
fi

echo "==> Setting open-source topics..."
curl -sS -m 20 -X PUT "${auth[@]}" "$API/repos/$FULL_NAME/topics" \
  -d '{"names":["operating-system","rust","no-std","kernel","uefi","ai","microkernel","agents","open-source"]}' >/dev/null 2>&1 || true

echo "==> Pushing main (81 files, 3 commits)..."
git push "https://x-access-token:$TOKEN@github.com/$FULL_NAME.git" main:main 2>&1 | sed "s/x-access-token:[^@]*/x-access-token:***REDACTED***/g"

echo "==> Setting clean origin remote (no credentials stored)..."
git remote remove origin 2>/dev/null || true
git remote add origin "https://github.com/$FULL_NAME.git"

echo ""
echo "=== SUCCESS ==="
echo "Public repository: https://github.com/$FULL_NAME"
echo "License: MIT OR Apache-2.0 (open source)"
echo ""
echo "IMPORTANT: revoke the token now -> https://github.com/settings/tokens"
