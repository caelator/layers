# Workflow Benchmark Agent Prompt

Task ID: code-refactor-context-compiler-packet-finalize
Variant: baseline
Time budget minutes: 75

## Task
Refactor packet finalization paths so packet id, created_at, provenance, and stable metadata are set through ContextCompiler rather than duplicated ad hoc call sites. Preserve existing query and preflight behavior.

## Required validation commands
- `cargo test -q context_packet_compiler -- --nocapture`
- `cargo test -q cmd::query -- --nocapture`
- `cargo check --workspace --all-targets`

## Baseline isolation
Do not run Layers commands, inspect Layers packet artifacts, or use preflight-generated context.
Work from the repository and the task prompt only so this run remains a clean no-Layers baseline.

## Scoring reminder
Full success: The requested repository change is implemented with focused regression coverage and all listed validation commands pass.
Partial success: The main behavior is implemented but validation is incomplete, coverage is weak, or minor follow-up is required.
Failure: The requested behavior remains missing, untested, or introduces regressions.
