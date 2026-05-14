#!/usr/bin/env bash
set -euo pipefail

repo="${1:-/Users/xxx/layers}"
cd "$repo"

root="docs/dogfood/20260513T2355Z-phase15-fixed-validpacket-minibatch"
if [ ! -d "$root" ]; then
  echo "No Phase 15 fixed artifact root found at $root" >&2
  exit 2
fi

cargo build -q
./target/debug/layers workflow-benchmark finalize-run "$root"

python3 - <<'PY'
from pathlib import Path
root = Path('docs/dogfood/20260513T2355Z-phase15-fixed-validpacket-minibatch')
checks = {
    'workflow_records': sum(1 for _ in (root/'compare'/'workflow-runs.jsonl').open()) if (root/'compare'/'workflow-runs.jsonl').exists() else 0,
    'transcripts': len(list((root/'transcripts').glob('*'))) if (root/'transcripts').exists() else 0,
    'validation_logs': len(list((root/'validation').glob('*'))) if (root/'validation').exists() else 0,
    'diff_stats': len(list((root/'diffs').glob('*.stat'))) if (root/'diffs').exists() else 0,
    'diff_patches': len(list((root/'diffs').glob('*.patch'))) if (root/'diffs').exists() else 0,
    'packet_json': len(list((root/'packets').glob('*.json'))) if (root/'packets').exists() else 0,
}
print(checks)
required = {
    'workflow_records': 10,
    'transcripts': 10,
    'validation_logs': 10,
    'diff_stats': 10,
    'diff_patches': 10,
    'packet_json': 3,
}
missing = {k: (checks[k], v) for k, v in required.items() if checks[k] < v}
if missing:
    raise SystemExit(f'artifact gate failed: {missing}')
print('TRAINING_DATA_ARTIFACT_GATE_OK')
PY
