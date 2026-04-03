use anyhow::{Context, Result};
use serde_json::json;
use std::fs;

use crate::config::memoryport_dir;
use crate::util::{append_jsonl, iso_now, parse_targets};

pub fn handle_remember(
    kind: &str,
    task: Option<String>,
    task_type: Option<String>,
    summary: Option<String>,
    file: Option<String>,
    artifacts_dir: Option<String>,
    targets: Option<String>,
) -> Result<()> {
    let record = match kind {
        "plan" => {
            let task = task.context("--task required for kind=plan")?;
            let file = file.context("--file required for kind=plan")?;
            let plan_markdown = fs::read_to_string(&file)
                .with_context(|| format!("failed to read plan file: {}", file))?;
            json!({
                "timestamp": iso_now(),
                "task_type": task_type.unwrap_or_else(|| "architecture".to_string()),
                "task": task,
                "plan_markdown": plan_markdown,
                "summary": summary,
                "artifacts_dir": artifacts_dir,
                "targets": parse_targets(targets.as_deref()),
            })
        }
        "learning" => json!({
            "timestamp": iso_now(),
            "kind": "manual-learning",
            "summary": summary.context("--summary required for kind=learning")?,
            "task_type": task_type,
        }),
        "trace" => {
            if task.is_none() && summary.is_none() {
                anyhow::bail!("--task or --summary is required for kind=trace");
            }
            json!({
                "timestamp": iso_now(),
                "task": task,
                "summary": summary,
                "task_type": task_type,
            })
        }
        _ => anyhow::bail!(
            "unsupported kind: {}. Valid kinds: plan, learning, trace",
            kind
        ),
    };
    let path = match kind {
        "plan" => memoryport_dir().join("council-plans.jsonl"),
        "learning" => memoryport_dir().join("council-learnings.jsonl"),
        "trace" => memoryport_dir().join("council-traces.jsonl"),
        _ => unreachable!(),
    };
    append_jsonl(&path, &record)?;
    println!(
        "{}",
        serde_json::to_string(&json!({"ok": true, "kind": kind}))?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestWorkspace;
    use crate::util::load_jsonl;
    use std::fs;

    #[test]
    fn remember_plan_writes_to_council_plans() {
        let ws = TestWorkspace::new("remember-plan");
        let root = ws.root();
        let plan_file = root.join("test-plan.md");
        fs::write(&plan_file, "# Test Plan\nDo the thing.").unwrap();

        handle_remember(
            "plan",
            Some("test-task".to_string()),
            Some("architecture".to_string()),
            None,
            Some(plan_file.to_string_lossy().to_string()),
            None,
            None,
        )
        .unwrap();

        let records = load_jsonl(&root.join("memoryport").join("council-plans.jsonl")).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["task"], "test-task");
        assert!(
            records[0]["plan_markdown"]
                .as_str()
                .unwrap()
                .contains("Do the thing")
        );
    }

    #[test]
    fn remember_learning_writes_to_council_learnings() {
        let ws = TestWorkspace::new("remember-learning");
        let root = ws.root();

        handle_remember(
            "learning",
            None,
            None,
            Some("Always check convergence before promoting.".to_string()),
            None,
            None,
            None,
        )
        .unwrap();

        let records = load_jsonl(&root.join("memoryport").join("council-learnings.jsonl")).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0]["summary"],
            "Always check convergence before promoting."
        );
    }

    #[test]
    fn remember_trace_writes_to_council_traces() {
        let ws = TestWorkspace::new("remember-trace");
        let root = ws.root();

        handle_remember(
            "trace",
            Some("council-run-123".to_string()),
            None,
            Some("Council converged after 2 rounds.".to_string()),
            None,
            None,
            None,
        )
        .unwrap();

        let records = load_jsonl(&root.join("memoryport").join("council-traces.jsonl")).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["task"], "council-run-123");
    }

    #[test]
    fn remember_rejects_unsupported_kind() {
        let _ws = TestWorkspace::new("remember-bad-kind");
        let result = handle_remember("bogus", None, None, None, None, None, None);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unsupported kind"));
    }

    #[test]
    fn remember_plan_requires_task_and_file() {
        let _ws = TestWorkspace::new("remember-plan-missing");
        let no_task = handle_remember("plan", None, None, None, None, None, None);
        assert!(no_task.is_err());
        let no_file = handle_remember(
            "plan",
            Some("task".to_string()),
            None,
            None,
            None,
            None,
            None,
        );
        assert!(no_file.is_err());
    }
}
