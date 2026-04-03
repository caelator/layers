use anyhow::Result;
use serde_json::json;

use crate::config::{council_files, uc_config_path, workspace_root};
use crate::util::{compact, load_jsonl, run_command, which};

pub fn handle_refresh(embeddings: bool) -> Result<()> {
    let root = workspace_root();
    let mut results: Vec<serde_json::Value> = Vec::new();

    // 1. Refresh GitNexus index
    if which("npx").is_some() {
        let mut args = vec!["npx", "gitnexus", "analyze"];
        if embeddings {
            args.push("--embeddings");
        }
        eprintln!("Running: {}", args.join(" "));
        match run_command(&args, &root) {
            Ok((true, stdout, _)) => {
                results.push(json!({
                    "tool": "gitnexus",
                    "status": "ok",
                    "output": compact(stdout.trim(), 500),
                }));
            }
            Ok((false, _, stderr)) => {
                results.push(json!({
                    "tool": "gitnexus",
                    "status": "error",
                    "error": compact(stderr.trim(), 500),
                }));
            }
            Err(e) => {
                results.push(json!({
                    "tool": "gitnexus",
                    "status": "error",
                    "error": e.to_string(),
                }));
            }
        }
    } else {
        results.push(json!({
            "tool": "gitnexus",
            "status": "skipped",
            "reason": "npx not found in PATH",
        }));
    }

    // 2. Refresh MemoryPort: flush pending chunks, then check status.
    // Layers refreshes MemoryPort through `uc`; it does not assume any MCP-exposed
    // MemoryPort tool surface.
    let has_uc = which("uc").is_some() && uc_config_path().exists();
    if has_uc {
        let uc_cfg = uc_config_path();
        let uc_cfg_str = uc_cfg.to_string_lossy().to_string();

        // Flush buffered chunks (triggers embedding for any pending data)
        let flush_args = ["uc", "-c", &uc_cfg_str, "flush"];
        eprintln!("Running: {}", flush_args.join(" "));
        let flush_result = match run_command(&flush_args, &root) {
            Ok((true, stdout, _)) => {
                json!({"action": "flush", "status": "ok", "output": compact(stdout.trim(), 200)})
            }
            Ok((false, _, stderr)) => {
                json!({"action": "flush", "status": "error", "error": compact(stderr.trim(), 200)})
            }
            Err(e) => {
                json!({"action": "flush", "status": "error", "error": e.to_string()})
            }
        };

        // Check status
        let status_args = ["uc", "-c", &uc_cfg_str, "status"];
        let status_result = match run_command(&status_args, &root) {
            Ok((true, stdout, _)) => {
                json!({"action": "status", "status": "ok", "output": compact(stdout.trim(), 500)})
            }
            Ok((false, _, stderr)) => {
                json!({"action": "status", "status": "error", "error": compact(stderr.trim(), 500)})
            }
            Err(e) => {
                json!({"action": "status", "status": "error", "error": e.to_string()})
            }
        };

        let mp_ok = flush_result["status"] != "error" && status_result["status"] != "error";
        results.push(json!({
            "tool": "memoryport",
            "status": if mp_ok { "ok" } else { "error" },
            "steps": [flush_result, status_result],
        }));
    } else {
        results.push(json!({
            "tool": "memoryport",
            "status": "skipped",
            "reason": if which("uc").is_none() { "uc not found in PATH" } else { "uc.toml not found" },
        }));
    }

    // 3. Verify JSONL stores
    let spine_status: Vec<_> = council_files()
        .into_iter()
        .map(|(kind, path)| {
            let exists = path.exists();
            let count = if exists {
                load_jsonl(&path).map(|v| v.len()).unwrap_or(0)
            } else {
                0
            };
            json!({"kind": kind, "exists": exists, "records": count})
        })
        .collect();
    results.push(json!({
        "tool": "memory_spine",
        "status": "ok",
        "stores": spine_status,
    }));

    let ok = results.iter().all(|r| r["status"] != "error");
    let payload = json!({
        "ok": ok,
        "results": results,
    });
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}
