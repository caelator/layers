# TurboCALM Retrieval Proof Implementation Plan

> For Hermes: Use test-driven-development for code changes and keep candle-turbocalm optional until benchmark evidence supports it.

Goal: Prove or reject whether candle-turbocalm-backed semantic retrieval can reduce Layers' missed-critical-context rate without breaking abstention or bloating packets.

Architecture: Add an offline retrieval-eval corpus/export path first. The first implementation does not call candle-turbocalm and does not add a hard dependency. It turns preregistered workflow task specs plus repo files/memory into a deterministic JSON corpus that candle-turbocalm or any OpenAI-compatible embedding service can evaluate. Later phases add optional sidecar retrieval behind explicit flags.

Tech Stack: Rust CLI, existing workflow-benchmark task specs, JSON output, optional candle-turbocalm /v1/embeddings sidecar in later phases.

---

## Success Gates

1. Offline eval corpus can be generated from `benchmarks/workflows/tasks` and the local repo.
2. Non-negative-control tasks map prompt -> expected relevant files.
3. Negative controls are preserved as abstention cases and do not force context injection.
4. The first implementation has no candle-turbocalm build/runtime dependency.
5. `cargo check --workspace --all-targets` passes.
6. Follow-on semantic retrieval is kept behind an explicit experimental flag.

## Task 1: Add retrieval eval corpus export

Objective: Add a workflow-benchmark subcommand that emits a deterministic retrieval-eval JSON document for benchmark tasks.

Files:
- Modify: `src/cmd/workflow_benchmark.rs`
- Modify: `src/main.rs` parser tests

TDD steps:
1. Add a failing unit test that builds a temporary task spec and asserts the corpus contains:
   - one pair with query equal to the task prompt
   - relevant_ids equal to expected_relevant_files
   - documents for expected files that exist under repo root
   - negative_controls count tracked separately
2. Run the exact test and verify it fails because the function/command does not exist.
3. Add minimal structs and helper functions:
   - `RetrievalEvalConfig`
   - `RetrievalEvalCorpus`
   - `RetrievalEvalPair`
   - `RetrievalEvalDocument`
   - `build_retrieval_eval_corpus`
4. Add `workflow-benchmark retrieval-eval-corpus <tasks> --repo-root <repo> --json`.
5. Run targeted tests, then full cargo check.

## Task 2: Add lexical baseline evaluator

Objective: Compare current keyword retrieval against the eval corpus before trying embeddings.

Files:
- Modify: `src/cmd/workflow_benchmark.rs`

TDD steps:
1. Write failing test where a query should rank a relevant document above an irrelevant one by token overlap.
2. Implement a deterministic lexical scorer.
3. Emit recall@5, recall@10, MRR, and negative-control injection rate.

## Task 3: Add optional OpenAI-compatible embedding client

Objective: Add sidecar-only semantic scoring without linking candle-turbocalm.

Files:
- New: `src/semantic_retrieval.rs` or local workflow-benchmark module section
- Modify: CLI only behind `--embedding-base-url`

TDD steps:
1. Test request/response parsing with a local mock function, not network.
2. Implement minimal HTTP client only when flag is provided.
3. Fail closed: if sidecar unavailable, report degraded evaluation rather than blocking stable core.

## Task 4: Run candle-turbocalm comparison

Objective: Compare lexical vs candle-turbocalm semantic vs hybrid retrieval on the preregistered task corpus.

Commands:
1. Start candle-turbocalm embedding server separately:
   `cd /Users/xxx/candle-turbocalm && cargo run -p turbocalm-train -- serve --checkpoint ./artifacts/checkpoints/latest.safetensors --port 11435 --pooled --device cpu`
2. Generate eval corpus:
   `cd /Users/xxx/layers && cargo run -q -- workflow-benchmark retrieval-eval-corpus benchmarks/workflows/tasks --repo-root . --json > target/retrieval-eval-corpus.json`
3. Run lexical and semantic comparisons once implemented.

Continue gate:
- Hybrid semantic retrieval improves file recall@10 by at least 25% relative over current lexical fallback.
- Negative-control unnecessary injection rate remains <= 0.05.
- Stale-trap regressions remain zero.

## Task 5: Experimental preflight integration

Objective: Only after Task 4 passes, add `layers preflight --semantic-sidecar http://localhost:11435/v1`.

Rules:
- Explicit targets always win.
- Semantic candidates are citations, not prompt dumps.
- Include scores and model id in retrieval metadata.
- Strict mode must abstain/request-target when confidence is low.

## Non-goals

- Do not merge candle-turbocalm into the Layers workspace.
- Do not make stable Layers require Candle, Metal, or trained checkpoints.
- Do not add online training to Layers.
- Do not claim Layers is better until the preregistered gates pass.
