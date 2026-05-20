#!/usr/bin/env bash
set -euo pipefail
run_id="$(basename "$PWD")"
task_id="${run_id%--*}"
task_file="/Users/xxx/layers/docs/dogfood/20260518T203906Z-phase3-10task-calibration/tasks/${task_id}.json"
if [[ ! -f "$task_file" ]]; then echo "missing task file for $task_id at $task_file" >&2; exit 64; fi
target_file="$(mktemp)"
python3 - "$task_file" > "$target_file" <<'PY'
import json, sys
spec=json.load(open(sys.argv[1]))
for t in spec.get('target_files') or []:
    print(t)
for t in spec.get('target_symbols') or []:
    print(t)
PY
task_text="$(python3 - "$task_file" <<'PY'
import json, sys
spec=json.load(open(sys.argv[1]))
parts=[spec.get('title') or spec.get('task_id') or '', spec.get('prompt') or '', spec.get('description') or '', spec.get('instructions') or '']
print('\n\n'.join(str(x) for x in parts if x))
PY
)"
cmd=(/Users/xxx/layers/target/debug/layers preflight --no-audit --json --strict)
while IFS= read -r t; do
  [[ -n "$t" ]] && cmd+=(--target "$t")
done < "$target_file"
rm -f "$target_file"
cmd+=("$task_text")
exec "${cmd[@]}"
