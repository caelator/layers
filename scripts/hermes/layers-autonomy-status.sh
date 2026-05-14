#!/usr/bin/env bash
set -euo pipefail

repo="${1:-/Users/xxx/layers}"
cd "$repo"

now="$(date -u +%Y%m%dT%H%M%SZ)"
report_dir="$repo/.hermes/reports"
mkdir -p "$report_dir"
report="$report_dir/autonomy-status-$now.md"
latest_link="$report_dir/autonomy-status-latest.md"

{
  echo "# Layers autonomy status — $now"
  echo
  echo "## Repository"
  echo '```'
  printf 'cwd: %s\n' "$repo"
  printf 'branch: '
  git branch --show-current || true
  printf 'head: '
  git rev-parse --short HEAD || true
  echo '```'
  echo
  echo "## Git status"
  echo '```'
  git status --short || true
  echo '```'
  echo
  echo "## Uncommitted diff stat"
  echo '```'
  git diff --stat || true
  echo '```'
  echo
  echo "## Recent dogfood artifacts"
  echo '```'
  if [ -d docs/dogfood ]; then
    find docs/dogfood -maxdepth 1 -mindepth 1 -type d -print | sort | tail -20
  else
    echo "docs/dogfood missing"
  fi
  echo '```'
  echo
  echo "## Phase 15 fixed mini-batch artifact check"
  root="docs/dogfood/20260513T2355Z-phase15-fixed-validpacket-minibatch"
  if [ -d "$root" ]; then
    echo '```'
    printf 'root: %s\n' "$root"
    printf 'workflow records: '; test -f "$root/compare/workflow-runs.jsonl" && wc -l < "$root/compare/workflow-runs.jsonl" || echo 0
    printf 'transcripts: '; find "$root/transcripts" -type f 2>/dev/null | wc -l | tr -d ' '
    printf '\nvalidation logs: '; find "$root/validation" -type f 2>/dev/null | wc -l | tr -d ' '
    printf '\ndiff stats: '; find "$root/diffs" -name '*.stat' -type f 2>/dev/null | wc -l | tr -d ' '
    printf '\ndiff patches: '; find "$root/diffs" -name '*.patch' -type f 2>/dev/null | wc -l | tr -d ' '
    printf '\npacket json: '; find "$root/packets" -name '*.json' -type f 2>/dev/null | wc -l | tr -d ' '
    printf '\n'
    echo '```'
  else
    echo "Phase 15 fixed artifact root not present."
  fi
  echo
  echo "## Suggested next autonomous actions"
  echo "- If working tree contains only intended Phase 15 code/docs artifacts, run full verification and commit [verified] slice."
  echo "- Keep memoryport/telemetry/events.jsonl out of verified commits unless intentionally changed."
  echo "- Scale next benchmark only after the finalizer gate remains clean."
} > "$report"

ln -sf "$report" "$latest_link"
printf '%s\n' "$report"
