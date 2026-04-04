use std::process::Command;
use std::time::Duration;

use wait_timeout::ChildExt;

use crate::config::{uc_config_path, uc_min_results, uc_timeout_ms};

/// Result of a `uc retrieve` invocation.
pub struct UcResult {
    pub lines: Vec<String>,
    pub fallback_reason: Option<String>,
}

/// Check whether `uc` is available (binary on PATH + config file exists).
pub fn is_available() -> bool {
    which("uc") && uc_config_path().exists()
}

/// Run `uc retrieve <query> --top-k <top_k>` with the configured timeout.
/// Returns retrieved lines on success, or a fallback reason on failure/timeout.
pub fn retrieve(query: &str, top_k: usize) -> UcResult {
    let config_path = uc_config_path();

    let child = Command::new("uc")
        .arg("-c")
        .arg(&config_path)
        .arg("retrieve")
        .arg(query)
        .arg("--top-k")
        .arg(top_k.to_string())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn();

    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            return UcResult {
                lines: vec![],
                fallback_reason: Some(format!("uc spawn failed: {e}")),
            };
        }
    };

    let timeout = Duration::from_millis(uc_timeout_ms());
    match child.wait_timeout(timeout) {
        Ok(Some(status)) if status.success() => {
            let stdout = child
                .stdout
                .take()
                .and_then(|out| std::io::read_to_string(out).ok())
                .unwrap_or_default();
            let lines: Vec<String> = stdout
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect();
            UcResult {
                lines,
                fallback_reason: None,
            }
        }
        Ok(Some(status)) => {
            let stderr = child
                .stderr
                .take()
                .and_then(|out| std::io::read_to_string(out).ok())
                .unwrap_or_default();
            UcResult {
                lines: vec![],
                fallback_reason: Some(format!(
                    "uc exited with {}: {}",
                    status,
                    stderr.trim()
                )),
            }
        }
        Ok(None) => {
            // Timeout — kill the process
            let _ = child.kill();
            let _ = child.wait();
            UcResult {
                lines: vec![],
                fallback_reason: Some(format!(
                    "uc timed out after {}ms",
                    uc_timeout_ms()
                )),
            }
        }
        Err(e) => UcResult {
            lines: vec![],
            fallback_reason: Some(format!("uc wait failed: {e}")),
        },
    }
}

/// Check whether uc returned enough results to be considered successful.
pub fn meets_threshold(result: &UcResult) -> bool {
    result.fallback_reason.is_none() && result.lines.len() >= uc_min_results()
}

fn which(bin: &str) -> bool {
    Command::new("which")
        .arg(bin)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meets_threshold_requires_no_fallback_and_enough_lines() {
        let ok = UcResult {
            lines: vec!["result1".into()],
            fallback_reason: None,
        };
        assert!(meets_threshold(&ok));

        let empty = UcResult {
            lines: vec![],
            fallback_reason: None,
        };
        assert!(!meets_threshold(&empty));

        let failed = UcResult {
            lines: vec!["result1".into()],
            fallback_reason: Some("timeout".into()),
        };
        assert!(!meets_threshold(&failed));
    }

    #[test]
    fn retrieve_returns_fallback_when_uc_not_on_path() {
        // uc is unlikely to be on PATH in CI/test — this exercises the spawn-fail path
        if which("uc") {
            return; // skip if uc happens to be installed
        }
        let result = retrieve("test query", 3);
        assert!(result.fallback_reason.is_some());
        assert!(result.lines.is_empty());
    }
}
