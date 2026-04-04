use std::path::Path;
use std::process::Command;
use std::time::Duration;

use wait_timeout::ChildExt;

use crate::config::{uc_config_path, uc_min_results, uc_timeout_ms};

/// Result of a `uc retrieve` invocation.
pub struct UcResult {
    pub lines: Vec<String>,
    pub fallback_reason: Option<String>,
}

/// Options for controlling the `uc retrieve` invocation.
pub struct UcOptions {
    pub timeout_ms: u64,
    pub min_results: usize,
}

impl Default for UcOptions {
    fn default() -> Self {
        Self {
            timeout_ms: uc_timeout_ms(),
            min_results: uc_min_results(),
        }
    }
}

/// Check whether `uc` is available (binary on PATH + config file exists).
pub fn is_available() -> bool {
    which("uc") && uc_config_path().exists()
}

/// Run `uc retrieve <query> --top-k <top_k>` with the configured timeout.
/// Returns retrieved lines on success, or a fallback reason on failure/timeout.
/// Convenience wrapper around [`retrieve_with_opts`] using default config.
#[allow(dead_code)]
pub fn retrieve(query: &str, top_k: usize) -> UcResult {
    retrieve_with_opts(query, top_k, &UcOptions::default())
}

/// Run `uc retrieve` with explicit options for timeout and min-results thresholds.
pub fn retrieve_with_opts(query: &str, top_k: usize, opts: &UcOptions) -> UcResult {
    retrieve_impl(query, top_k, opts, &uc_config_path())
}

/// Inner implementation that also accepts a config path (for testing).
fn retrieve_impl(query: &str, top_k: usize, opts: &UcOptions, config_path: &Path) -> UcResult {
    let child = Command::new("uc")
        .arg("-c")
        .arg(config_path)
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

    let timeout = Duration::from_millis(opts.timeout_ms);
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
                    opts.timeout_ms
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
/// Convenience wrapper around [`meets_threshold_with`] using default config.
#[allow(dead_code)]
pub fn meets_threshold(result: &UcResult) -> bool {
    meets_threshold_with(result, uc_min_results())
}

/// Check whether uc returned enough results using an explicit minimum.
pub fn meets_threshold_with(result: &UcResult, min_results: usize) -> bool {
    result.fallback_reason.is_none() && result.lines.len() >= min_results
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
    fn meets_threshold_with_respects_explicit_minimum() {
        let two_lines = UcResult {
            lines: vec!["a".into(), "b".into()],
            fallback_reason: None,
        };
        assert!(meets_threshold_with(&two_lines, 2));
        assert!(!meets_threshold_with(&two_lines, 3));
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

    #[test]
    fn retrieve_with_opts_passes_timeout_and_returns_lines() {
        // Smoke-test: use a tiny shell script as a fake `uc` binary.
        // If real `uc` is on PATH we skip — the mock cannot shadow it without
        // manipulating PATH, which is not thread-safe.
        if which("uc") {
            return;
        }

        // Build a temporary directory with a fake `uc` script
        let tmp = std::env::temp_dir().join(format!(
            "layers-uc-smoke-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();

        // Write a fake `uc` that prints two result lines regardless of args
        let fake_uc = tmp.join("uc");
        std::fs::write(
            &fake_uc,
            "#!/bin/sh\necho 'result line one'\necho 'result line two'\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&fake_uc, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        // Write a dummy config file
        let fake_config = tmp.join("uc.toml");
        std::fs::write(&fake_config, "[uc]\n").unwrap();

        // Temporarily prepend our fake directory to PATH
        let original_path = std::env::var("PATH").unwrap_or_default();
        unsafe {
            std::env::set_var("PATH", format!("{}:{}", tmp.display(), original_path));
        }

        let opts = UcOptions {
            timeout_ms: 5000,
            min_results: 1,
        };
        let result = retrieve_impl("smoke query", 3, &opts, &fake_config);

        // Restore PATH
        unsafe {
            std::env::set_var("PATH", &original_path);
        }
        let _ = std::fs::remove_dir_all(&tmp);

        assert!(
            result.fallback_reason.is_none(),
            "expected success but got: {:?}",
            result.fallback_reason
        );
        assert_eq!(result.lines.len(), 2);
        assert_eq!(result.lines[0], "result line one");
        assert_eq!(result.lines[1], "result line two");
        assert!(meets_threshold_with(&result, opts.min_results));
    }

    #[test]
    fn default_uc_options_reads_config_values() {
        // Verify UcOptions::default() picks up the config module defaults
        let opts = UcOptions::default();
        // The default from config.rs is 500ms / 1 result (unless env overrides)
        assert!(opts.timeout_ms > 0);
        assert!(opts.min_results > 0);
    }
}
