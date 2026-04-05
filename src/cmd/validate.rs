use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;

use crate::config::{canonical_curated_memory_path, council_files, uc_config_path, workspace_root};
use crate::router::{self, Confidence, Route};
use crate::util::{load_jsonl, which};

pub fn handle_validate(routing_benchmarks: Option<String>, ci: bool) -> Result<()> {
    // Check JSONL stores exist
    let spine_files: Vec<_> = council_files()
        .into_iter()
        .map(|(kind, path)| {
            let exists = path.exists();
            let count = if exists {
                load_jsonl(&path).map(|v| v.len()).unwrap_or(0)
            } else {
                0
            };
            json!({"kind": kind, "path": path, "exists": exists, "records": count})
        })
        .collect();

    let curated_path = canonical_curated_memory_path();
    let curated_count = if curated_path.exists() {
        load_jsonl(&curated_path).map(|v| v.len()).unwrap_or(0)
    } else {
        0
    };

    // Check council commands
    let council_configured = [
        "LAYERS_COUNCIL_GEMINI_CMD",
        "LAYERS_COUNCIL_CLAUDE_CMD",
        "LAYERS_COUNCIL_CODEX_CMD",
    ]
    .iter()
    .all(|key| {
        std::env::var(key)
            .ok()
            .is_some_and(|v| !v.trim().is_empty())
    });

    // Check external tools
    let has_uc = which("uc").is_some() && uc_config_path().exists();
    let has_gitnexus = which("gitnexus").is_some();

    let ok = has_uc || has_gitnexus; // at least one retrieval source

    // Run routing benchmarks if requested
    let benchmark_result = if let Some(ref bench_file) = routing_benchmarks {
        Some(run_routing_benchmarks(bench_file)?)
    } else {
        None
    };

    let mut payload = json!({
        "ok": ok,
        "memory_spine": spine_files,
        "curated_memory": {
            "path": curated_path,
            "exists": curated_path.exists(),
            "records": curated_count,
        },
        "council": {
            "commands_configured": council_configured,
            "order": "Gemini -> Claude -> Codex",
        },
        "tools": {
            "uc": has_uc,
            "gitnexus": has_gitnexus,
        },
        "integration_notes": {
            "memoryport": "Layers expects direct MemoryPort access via uc + canonical files; codex-memoryport-bridge is a model proxy, not a raw MCP tool server.",
            "gitnexus": "Layers expects GitNexus via local CLI and optionally MCP-backed runtimes/skills."
        },
        "workspace": workspace_root(),
    });

    let benchmarks_pass = benchmark_result
        .as_ref()
        .is_some_and(|b| b["pass_rate"].as_f64().unwrap_or(0.0) >= 1.0);
    if let Some(bench) = &benchmark_result {
        payload["routing_benchmarks"] = bench.clone();
        if !benchmarks_pass {
            payload["ok"] = json!(false);
        }
    }

    println!("{}", serde_json::to_string_pretty(&payload)?);
    if ci && payload["ok"] == json!(false) && !benchmarks_pass {
        anyhow::bail!("validation failed");
    }
    Ok(())
}

/// Run routing benchmarks from an answer-key JSONL file.
///
/// Each line: `{"query": "...", "expected_route": "neither|memory_only|graph_only|both"}`
/// Optional: `"expected_confidence": "high|low"`, `"note": "..."`
pub fn run_routing_benchmarks(file: &str) -> Result<Value> {
    let path = Path::new(file);
    if !path.exists() {
        anyhow::bail!("benchmark file not found: {}", file);
    }
    let lines = fs::read_to_string(path)?;
    let mut total = 0u32;
    let mut passed = 0u32;
    let mut failures: Vec<Value> = Vec::new();

    for (line_num, line) in lines.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
            continue;
        }
        let case: Value = serde_json::from_str(line)
            .with_context(|| format!("parse error on line {}", line_num + 1))?;

        let query = case["query"].as_str().context("missing 'query' field")?;
        let expected_route = case["expected_route"]
            .as_str()
            .context("missing 'expected_route' field")?;

        let result = router::classify(query);

        // Apply refusal bias (same as handle_query)
        let effective_route = if result.confidence == Confidence::Low {
            Route::Neither
        } else {
            result.route
        };

        let route_match = effective_route.label() == expected_route;

        let confidence_match = case
            .get("expected_confidence")
            .and_then(|v| v.as_str())
            .map(|ec| {
                let actual = result.confidence.to_string();
                actual == ec
            })
            .unwrap_or(true); // no expectation = pass

        total += 1;
        if route_match && confidence_match {
            passed += 1;
        } else {
            let mut failure = json!({
                "line": line_num + 1,
                "query": query,
                "expected_route": expected_route,
                "actual_route": effective_route.label(),
                "actual_confidence": result.confidence.to_string(),
                "scores": result.scores,
            });
            if let Some(ec) = case.get("expected_confidence") {
                failure["expected_confidence"] = ec.clone();
            }
            if let Some(note) = case.get("note") {
                failure["note"] = note.clone();
            }
            failures.push(failure);
        }
    }

    let pass_rate = if total > 0 {
        passed as f64 / total as f64
    } else {
        1.0
    };

    Ok(json!({
        "file": file,
        "total": total,
        "passed": passed,
        "failed": total - passed,
        "pass_rate": pass_rate,
        "failures": failures,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestWorkspace;
    use std::fs;

    #[test]
    fn validate_runs_without_benchmarks() {
        let _ws = TestWorkspace::new("validate-no-bench");
        let result = handle_validate(None, false);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_runs_routing_benchmarks() {
        let ws = TestWorkspace::new("validate-bench");
        let root = ws.root();
        let bench_file = root.join("benchmarks.jsonl");
        fs::write(
            &bench_file,
            concat!(
                r#"{"query": "rename this variable to snake_case", "expected_route": "neither", "expected_confidence": "high"}"#,
                "\n",
                r#"{"query": "hello", "expected_route": "neither", "expected_confidence": "low"}"#,
                "\n",
            ),
        )
        .unwrap();

        let result = handle_validate(Some(bench_file.to_string_lossy().to_string()), false);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_benchmarks_reports_failures() {
        let ws = TestWorkspace::new("validate-bench-fail");
        let root = ws.root();
        let bench_file = root.join("bad-bench.jsonl");
        // Force a mismatch: expect "both" for a trivial query
        fs::write(
            &bench_file,
            r#"{"query": "rename x to y", "expected_route": "both"}"#,
        )
        .unwrap();

        // validate should still succeed (it reports, doesn't bail)
        let result = handle_validate(Some(bench_file.to_string_lossy().to_string()), false);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_ci_mode_fails_when_benchmarks_fail() {
        let ws = TestWorkspace::new("validate-bench-ci-fail");
        let root = ws.root();
        let bench_file = root.join("bad-bench.jsonl");
        fs::write(
            &bench_file,
            r#"{"query": "rename x to y", "expected_route": "both"}"#,
        )
        .unwrap();

        let result = handle_validate(Some(bench_file.to_string_lossy().to_string()), true);
        assert!(result.is_err());
    }

    #[test]
    fn validate_ci_mode_succeeds_when_benchmarks_pass() {
        let ws = TestWorkspace::new("validate-bench-ci-pass");
        let root = ws.root();
        let bench_file = root.join("benchmarks.jsonl");
        fs::write(
            &bench_file,
            concat!(
                r#"{"query": "rename this variable to snake_case", "expected_route": "neither", "expected_confidence": "high"}"#,
                "\n",
                r#"{"query": "why did we already decide this about Layers? What was the rationale?", "expected_route": "memory_only"}"#,
                "\n",
            ),
        )
        .unwrap();

        let result = handle_validate(Some(bench_file.to_string_lossy().to_string()), true);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_benchmarks_rejects_missing_file() {
        let _ws = TestWorkspace::new("validate-bench-missing");
        let result = handle_validate(Some("/nonexistent/file.jsonl".to_string()), false);
        assert!(result.is_err());
    }
}
