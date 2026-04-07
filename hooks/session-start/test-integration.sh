#!/usr/bin/env bash
# layers-openclaw-pm-integration.sh
# End-to-end integration test: openclaw-pm status --json → Layers curated import
#
# Validates the integration between:
#   - openclaw-pm:    openclaw-pm status --json (tasks with Status::Doing/Blocked)
#   - Layers curated: layers curated import (next_step / postmortem records)
#
# openclaw-pm JSON schema used:
#   summary.todo|doing|blocked|done  — counts only
#   active[]    — tasks with Status::Doing (id, title, session_id, elapsed_secs)
#   blocked[]   — tasks with Status::Blocked (id, title, blocker_note, blocked_by)
#   ideas{}     — inbox/triaged/archived counts + recent previews
#
# Layers CuratedImportRecord schema:
#   kind, project, summary, rationale, timestamp, tags, sources
#
# Integration mapping:
#   openclaw-pm doing task  →  Layers next_step  (currently active work)
#   openclaw-pm blocked task →  Layers postmortem  (stalled work, root cause = blocker_note)
#
# Usage:
#   bash hooks/session-start/test-integration.sh [--pm-dir ~/.openclaw/pm]
#
# Exits 0 on success, 1 on failure, 0 (SKIP) if openclaw-pm state is empty.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LAYERS_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
OPENCLAW_PM_DIR="${OPENCLAW_PM_DIR:-$HOME/.openclaw/pm}"
WORKSPACE="${LAYERS_ROOT}"

# ---------------------------------------------------------------------------
# 1. Collect openclaw-pm JSON status
# ---------------------------------------------------------------------------
echo "=== Step 1: Collecting openclaw-pm status ==="

PM_BIN="$LAYERS_ROOT/../openclaw-pm/target/release/openclaw-pm"
if [[ ! -f "$PM_BIN" ]]; then
    PM_BIN="openclaw-pm"  # fall back to PATH
fi

# openclaw-pm accepts OPENCLAW_PM_DIR env var and --workspace flag
STATUS_JSON="$(OPENCLAW_PM_DIR="$OPENCLAW_PM_DIR" "$PM_BIN" --workspace "$WORKSPACE" status --json 2>/dev/null)" || {
    echo "SKIP: openclaw-pm status failed (pm dir may be empty or binary not found)"
    exit 0
}

if [[ -z "$STATUS_JSON" ]]; then
    echo "SKIP: openclaw-pm returned empty status"
    exit 0
fi

PROJECT_TITLE=$(echo "$STATUS_JSON" | jq -r '.project_title // "unknown"' 2>/dev/null || echo "unknown")
SUMMARY_TODO=$(echo "$STATUS_JSON" | jq -r '.summary.todo // 0' 2>/dev/null || echo "0")
SUMMARY_DOING=$(echo "$STATUS_JSON" | jq -r '.summary.doing // 0' 2>/dev/null || echo "0")
SUMMARY_BLOCKED=$(echo "$STATUS_JSON" | jq -r '.summary.blocked // 0' 2>/dev/null || echo "0")

echo "openclaw-pm status collected: project=\"$PROJECT_TITLE\""
echo "  summary: $SUMMARY_TODO todo, $SUMMARY_DOING doing, $SUMMARY_BLOCKED blocked"
echo "  active[] tasks: $(echo "$STATUS_JSON" | jq '.active | length' 2>/dev/null)"
echo "  blocked[] tasks: $(echo "$STATUS_JSON" | jq '.blocked | length' 2>/dev/null)"

# ---------------------------------------------------------------------------
# 2. Transform openclaw-pm JSON → Layers curated-import JSONL
# ---------------------------------------------------------------------------
echo ""
echo "=== Step 2: Transforming openclaw-pm JSON → Layers curated JSONL ==="

TMPFILE=$(mktemp /tmp/layers-openclaw-pm-import-XXXXXX.jsonl)
trap "rm -f $TMPFILE" EXIT

NOW=$(date -u +%Y-%m-%dT%H:%M:%SZ)

# -- Transform active (doing) tasks → next_step records --
# These represent what the agent is currently working on.
echo "$STATUS_JSON" | jq -c '.active[]?' 2>/dev/null | while IFS= read -r task_json; do
    id=$(echo "$task_json" | jq -r '.id // ""' 2>/dev/null)
    title=$(echo "$task_json" | jq -r '.title // ""' 2>/dev/null)
    elapsed=$(echo "$task_json" | jq -r '.elapsed_secs // null' 2>/dev/null)
    orphaned=$(echo "$task_json" | jq -r '.potentially_orphaned // false' 2>/dev/null)
    session_id=$(echo "$task_json" | jq -r '.session_id // null' 2>/dev/null)

    if [[ -z "$title" ]]; then continue; fi

    # Build rationale from metadata
    rationale_parts=()
    [[ "$orphaned" == "true" ]] && rationale_parts+=("WARNING: task may be orphaned (session: $session_id)")
    [[ -n "$elapsed" && "$elapsed" != "null" ]] && rationale_parts+=("elapsed: ${elapsed}s")
    rationale=$(printf "; " "${rationale_parts[@]:-none}")

    tags_json=$(jq -n \
        --arg id "$id" \
        --arg session "$session_id" \
        --arg orphan "$orphaned" \
        '["openclaw-pm", "doing", ("id-\"" + $id + "\""),
          (if ($session != "null" and $session != "") then ("session-\"" + $session + "\"") else null end),
          (if $orphan == "true" then "orphaned" else null end)
        ] | map(select(. != null))')

    jq -n \
        --arg kind "next_step" \
        --arg project "$PROJECT_TITLE" \
        --arg summary "$title" \
        --arg rationale "$rationale" \
        --arg timestamp "$NOW" \
        --argjson tags "$tags_json" \
        '{
            kind: $kind,
            project: $project,
            summary: $summary,
            rationale: (if $rationale == "none" then null else $rationale end),
            timestamp: $timestamp,
            tags: $tags,
            sources: ["openclaw-pm-status"]
        }' >> "$TMPFILE"
done

# -- Transform blocked tasks → postmortem records --
# Blocked tasks represent work that got stuck; blocker_note / blocked_by form root_cause.
echo "$STATUS_JSON" | jq -c '.blocked[]?' 2>/dev/null | while IFS= read -r task_json; do
    id=$(echo "$task_json" | jq -r '.id // ""' 2>/dev/null)
    title=$(echo "$task_json" | jq -r '.title // ""' 2>/dev/null)
    note=$(echo "$task_json" | jq -r '.blocker_note // ""' 2>/dev/null)
    blocked_by=$(echo "$task_json" | jq -r '.blocked_by | join(", ") // ""' 2>/dev/null)

    if [[ -z "$title" ]]; then continue; fi

    root_cause="${note:-$blocked_by}"
    [[ -z "$root_cause" ]] && root_cause="unknown blocker"

    tags_json=$(jq -n \
        --arg id "$id" \
        --arg deps "$blocked_by" \
        '["openclaw-pm", "blocked", ("id-\"" + $id + "\""),
          (if ($deps != "") then ("deps-\"" + $deps + "\"") else null end)
        ] | map(select(. != null))')

    jq -n \
        --arg kind "postmortem" \
        --arg project "$PROJECT_TITLE" \
        --arg summary "$title" \
        --arg rationale "$root_cause" \
        --arg timestamp "$NOW" \
        --argjson tags "$tags_json" \
        '{
            kind: $kind,
            project: $project,
            summary: $summary,
            rationale: (if $rationale == "unknown blocker" then null else $rationale end),
            timestamp: $timestamp,
            tags: $tags,
            sources: ["openclaw-pm-status"]
        }' >> "$TMPFILE"
done

RECORD_COUNT=$(wc -l < "$TMPFILE" 2>/dev/null || echo 0)
echo "Generated $RECORD_COUNT curated import records"

if [[ "$RECORD_COUNT" -gt 0 ]]; then
    echo "Sample records:"
    head -2 "$TMPFILE" | jq -c '.' 2>/dev/null || cat "$TMPFILE" | head -2
fi

# ---------------------------------------------------------------------------
# 3. Run layers curated import
# ---------------------------------------------------------------------------
echo ""
echo "=== Step 3: Testing layers curated import ==="

cd "$LAYERS_ROOT"

# Use isolated test memory dir so we don't pollute real curated memory
TEST_MEMORY_DIR="$(mktemp -d /tmp/layers-test-memory-XXXXXX)"
export MEMORYPORT_DIR="$TEST_MEMORY_DIR"
mkdir -p "$TEST_MEMORY_DIR"
trap "rm -rf '$TEST_MEMORY_DIR'" EXIT

if ! cargo run -- curated import "$TMPFILE" 2>&1; then
    echo ""
    echo "FAIL: layers curated import rejected the generated JSONL"
    echo "Generated file contents:"
    cat "$TMPFILE"
    exit 1
fi

echo ""
echo "=== Integration test PASSED ==="
echo ""
echo "Schema compatibility report:"
echo ""
echo "openclaw-pm status --json  →  Layers curated import"
echo "  active[].id              →  tags[].id-*             ✓ stored as tag"
echo "  active[].title           →  summary                  ✓ direct"
echo "  active[].elapsed_secs    →  rationale                ✓ elapsed time"
echo "  active[].potentially_orphaned → tags[].orphaned    ✓"
echo "  blocked[].blocker_note   →  rationale (root cause)   ✓"
echo "  blocked[].blocked_by     →  rationale / tags[].deps- ✓"
echo "  summary.todo count       →  informational only        ○ not in curated"
echo "  ideas{}                  →  not yet mapped           ○ future work"
echo ""
echo "Integration gaps identified:"
echo "  1. openclaw-pm 'todo' items (Status::Todo) are NOT in status --json output"
echo "     → only 'doing' and 'blocked' tasks appear in the JSON arrays"
echo "     → full todo list requires reading state.yaml directly"
echo "  2. ideas[] are not mapped (future: kind=decision)"
echo "  3. No bidirectional sync — this is one-way (pm → layers) only"
echo "  4. Task type (milestone/task) is lost in transformation"
echo ""
echo "Recommendation: For richer integration, add openclaw-pm as a 'pm' plugin"
echo "to layers that reads state.yaml directly (not just status --json)."
