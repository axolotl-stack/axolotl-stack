#!/usr/bin/env bash
# Mirror project skills from .agents/skills (canonical, read by Codex) to .claude/skills
# (read by Claude Code). Skills named openspec-* are generated per tool by `openspec update`
# and are left alone.
#
#   tools/agents/sync-skills.sh          copy
#   tools/agents/sync-skills.sh --check  exit 1 if the mirror is out of date (CI)
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
src="$root/.agents/skills"
dst="$root/.claude/skills"
check=0
[[ "${1:-}" == "--check" ]] && check=1

status=0
mkdir -p "$dst"

for dir in "$src"/*/; do
  name="$(basename "$dir")"
  [[ "$name" == openspec-* ]] && continue
  if (( check )); then
    if ! diff -r "$src/$name" "$dst/$name" >/dev/null 2>&1; then
      echo "out of date: .claude/skills/$name (run tools/agents/sync-skills.sh)" >&2
      status=1
    fi
  else
    rm -rf "${dst:?}/$name"
    cp -r "$src/$name" "$dst/$name"
    echo "synced $name"
  fi
done

# Project skills deleted from .agents/skills must not linger in .claude/skills.
for dir in "$dst"/*/; do
  name="$(basename "$dir")"
  [[ "$name" == openspec-* ]] && continue
  if [[ ! -d "$src/$name" ]]; then
    if (( check )); then
      echo "stale: .claude/skills/$name has no .agents/skills source" >&2
      status=1
    else
      rm -rf "${dst:?}/$name"
      echo "removed $name"
    fi
  fi
done

exit "$status"
