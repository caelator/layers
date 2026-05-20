# Phase4 corpus audit

Task specs: 31
Category counts: {'bugfix': 13, 'other': 1, 'feature': 9, 'refactor': 2, 'negative_control': 6}
Negative controls: 6
Code-heavy non-negative tasks: 25
Stale-context traps: 29

## Gate check

- 30 paired tasks: PASS (30/30 selected)
- 20 code-heavy paired tasks: PASS (24/20 selected)
- 5 negative-control paired tasks: PASS (6/5 selected)

## Selected 30-task preregistration candidate

- code-bugfix-context-routing (bugfix; code-heavy)
- code-bugfix-council-circuit-exit-gate (bugfix; code-heavy, stale-trap)
- code-bugfix-mcp-client-error-redaction (bugfix; code-heavy, stale-trap)
- code-bugfix-preflight-strict-low-relevance (bugfix; code-heavy, stale-trap)
- code-bugfix-proveit-artifact-paths (bugfix; code-heavy, stale-trap)
- code-bugfix-provider-budget-overflow (bugfix; code-heavy, stale-trap)
- code-bugfix-query-target-traversal (bugfix; code-heavy, stale-trap)
- code-bugfix-router-historical-code-intent (bugfix; code-heavy, stale-trap)
- code-bugfix-runtime-queue-starvation (bugfix; code-heavy, stale-trap)
- code-bugfix-telemetry-malformed-jsonl-skip (bugfix; code-heavy, stale-trap)
- code-bugfix-workflow-benchmark-duplicate-pairing (bugfix; code-heavy, stale-trap)
- code-docs-architecture-context-spine (other; code-heavy, stale-trap)
- code-feature-config-secret-redaction-nested (feature; code-heavy, stale-trap)
- code-feature-daemon-heartbeat-stale-detection (feature; code-heavy, stale-trap)
- code-feature-mcp-preflight-stable-registry (feature; code-heavy, stale-trap)
- code-feature-packet-validate-warnings-json (feature; code-heavy, stale-trap)
- code-feature-quality-abstain-low-specificity (feature; code-heavy, stale-trap)
- code-feature-remember-reject-empty-records (feature; code-heavy, stale-trap)
- code-feature-session-monitor-threshold-env (feature; code-heavy, stale-trap)
- code-feature-tools-registry-duplicate-names (feature; code-heavy, stale-trap)
- code-feature-workflow-benchmark-human-surfaces (feature; code-heavy, stale-trap)
- code-refactor-context-compiler-packet-finalize (refactor; code-heavy, stale-trap)
- code-refactor-memory-index-fallback-tags (refactor; code-heavy, stale-trap)
- code-stale-trap-prefer-current-task-spec (bugfix; code-heavy, stale-trap)
- negative-control-basic-unit-conversion (negative_control; negative-control, stale-trap)
- negative-control-count-letters (negative_control; negative-control, stale-trap)
- negative-control-iso-date-format (negative_control; negative-control, stale-trap)
- negative-control-simple-json-validity (negative_control; negative-control, stale-trap)
- negative-control-sort-three-words (negative_control; negative-control, stale-trap)
- negative-control-trivial-arithmetic (negative_control; negative-control)
