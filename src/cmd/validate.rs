use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;

use crate::config::{canonical_curated_memory_path, council_files, uc_config_path, workspace_root};
use crate::router::{self, Confidence, Route};
use crate::util::{load_jsonl, which};

/// Build the validation payload without printing or running benchmarks.
fn build_validate_payload() -> Value {
    // Check council commands — configured if env var set OR binary found on PATH
    fn council_cmd_available(stage: &str) -> bool {
        let env_key = match stage {
            "gemini" => "LAYERS_COUNCIL_GEMINI_CMD",
            "claude" => "LAYERS_COUNCIL_CLAUDE_CMD",
            "codex" => "LAYERS_COUNCIL_CODEX_CMD",
            _ => return false,
        };
        if std::env::var(env_key)
            .ok()
            .is_some_and(|v| !v.trim().is_empty())
        {
            return true;
        }
        let candidates: &[&str] = match stage {
            "gemini" => &["gemini", "gemini-cli"],
            "claude" => &["claude", "claude-code"],
            "codex" => &["codex", "opencode"],
            _ => return false,
        };
        candidates.iter().any(|&c| which(c).is_some())
    }
    let council_configured = council_cmd_available("gemini")
        && council_cmd_available("claude")
        && council_cmd_available("codex");

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

    // Check external tools
    let has_uc = which("uc").is_some() && uc_config_path().exists();
    let has_gitnexus = which("gitnexus").is_some();

    let ok = has_uc || has_gitnexus; // at least one retrieval source

    json!({
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
    })
}

pub fn handle_validate(routing_benchmarks: Option<String>, ci: bool) -> Result<()> {
    let mut payload = build_validate_payload();

    // Run routing benchmarks if requested
    let benchmark_result = if let Some(ref bench_file) = routing_benchmarks {
        Some(run_routing_benchmarks(bench_file)?)
    } else {
        None
    };

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
        anyhow::bail!("benchmark file not found: {file}");
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
            .and_then(serde_json::Value::as_str)
            .is_none_or(|ec| ec == result.confidence.to_string());

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

    /// Helper: run `build_validate_payload` with PATH set to `path_val` and
    /// council env vars cleared.
    #[allow(unsafe_code)]
    fn payload_with_path(ws_name: &str, path_val: &str) -> (TestWorkspace, Value) {
        let ws = TestWorkspace::new(ws_name);
        let orig_path = std::env::var("PATH").unwrap_or_default();
        // Clear council env vars so detection relies solely on PATH
        let council_vars = [
            "LAYERS_COUNCIL_GEMINI_CMD",
            "LAYERS_COUNCIL_CLAUDE_CMD",
            "LAYERS_COUNCIL_CODEX_CMD",
        ];
        let orig_council: Vec<_> = council_vars
            .iter()
            .map(|&k| (k, std::env::var_os(k)))
            .collect();
        unsafe {
            std::env::set_var("PATH", path_val);
            for &k in &council_vars {
                std::env::remove_var(k);
            }
        }
        let payload = build_validate_payload();
        unsafe {
            std::env::set_var("PATH", &orig_path);
            for (k, v) in &orig_council {
                match v {
                    Some(val) => std::env::set_var(k, val),
                    None => std::env::remove_var(k),
                }
            }
        }
        (ws, payload)
    }

    #[test]
    fn validate_reports_degraded_when_no_retrieval_tools() {
        // Empty PATH: neither uc nor gitnexus will be found
        let (_ws, payload) = payload_with_path("validate-no-retrieval", "");
        assert_eq!(
            payload["ok"],
            json!(false),
            "ok must be false when no retrieval tools available"
        );
        assert_eq!(payload["tools"]["uc"], json!(false));
        assert_eq!(payload["tools"]["gitnexus"], json!(false));
    }

    #[test]
    fn validate_reports_partial_when_only_one_tool() {
        let ws_name = "validate-partial-tool";
        let ws = TestWorkspace::new(ws_name);
        let bin_dir = ws.root().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();

        // Create a fake gitnexus binary on a custom PATH
        let fake_gitnexus = bin_dir.join("gitnexus");
        fs::write(&fake_gitnexus, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&fake_gitnexus, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let orig_path = std::env::var("PATH").unwrap_or_default();
        let council_vars = [
            "LAYERS_COUNCIL_GEMINI_CMD",
            "LAYERS_COUNCIL_CLAUDE_CMD",
            "LAYERS_COUNCIL_CODEX_CMD",
        ];
        let orig_council: Vec<_> = council_vars
            .iter()
            .map(|&k| (k, std::env::var_os(k)))
            .collect();
        #[allow(unsafe_code)]
        unsafe {
            std::env::set_var("PATH", bin_dir.to_str().unwrap());
            for &k in &council_vars {
                std::env::remove_var(k);
            }
        }
        let payload = build_validate_payload();
        #[allow(unsafe_code)]
        unsafe {
            std::env::set_var("PATH", &orig_path);
            for (k, v) in &orig_council {
                match v {
                    Some(val) => std::env::set_var(k, val),
                    None => std::env::remove_var(k),
                }
            }
        }

        // gitnexus present, uc absent → ok is true but uc is false
        assert_eq!(
            payload["ok"],
            json!(true),
            "ok should be true with at least one tool"
        );
        assert_eq!(payload["tools"]["gitnexus"], json!(true));
        assert_eq!(
            payload["tools"]["uc"],
            json!(false),
            "uc should be reported as missing"
        );
    }

    #[test]
    fn validate_reports_missing_council_commands() {
        // Empty PATH means no council CLIs will be found
        let (_ws, payload) = payload_with_path("validate-no-council", "");
        assert_eq!(
            payload["council"]["commands_configured"],
            json!(false),
            "council.commands_configured must be false when no CLIs on PATH"
        );
    }

    #[test]
    fn validate_handles_missing_curated_memory_gracefully() {
        // TestWorkspace creates memoryport/ but not curated-memory.jsonl
        let (_ws, payload) = payload_with_path("validate-no-curated", "");
        assert_eq!(payload["curated_memory"]["exists"], json!(false));
        assert_eq!(payload["curated_memory"]["records"], json!(0));
    }
}
