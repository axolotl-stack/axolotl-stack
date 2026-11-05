#!/usr/bin/env bash
# One-time setup for the agent tooling: installs the pinned OpenSpec CLI with Bun and turns its
# usage telemetry off in OpenSpec's per-user config, so it stays off for every tool on this
# machine (Claude Code, Codex, a plain terminal). Safe to re-run.
#
#   tools/agents/setup.sh
set -euo pipefail

OPENSPEC_VERSION="1.16.0"   # keep in sync with .github/workflows/test.yml and docs/README.md

if ! command -v bun >/dev/null 2>&1; then
  echo "bun is required: https://bun.sh" >&2
  exit 1
fi

export OPENSPEC_TELEMETRY=0
installed="$(openspec --version 2>/dev/null || true)"
if [[ "$installed" != "$OPENSPEC_VERSION" ]]; then
  bun add -g "@fission-ai/openspec@${OPENSPEC_VERSION}"
fi

openspec config set telemetry.enabled false
echo "openspec $(openspec --version) installed; telemetry disabled in the global OpenSpec config"
