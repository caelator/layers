use std::path::{Path, PathBuf};

pub fn workspace_root() -> PathBuf {
    if let Ok(root) = std::env::var("LAYERS_WORKSPACE_ROOT") {
        return PathBuf::from(root);
    }
    if let Ok(cwd) = std::env::current_dir() {
        if let Some(git_root) = find_git_root(&cwd) {
            return git_root;
        }
        return cwd;
    }
    PathBuf::from(".")
}

fn find_git_root(start: &Path) -> Option<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        if current.join(".git").exists() {
            return Some(current);
        }
        if !current.pop() {
            break;
        }
    }
    None
}

pub fn memoryport_dir() -> PathBuf {
    workspace_root().join("memoryport")
}

pub fn canonical_curated_memory_path() -> PathBuf {
    memoryport_dir().join("curated-memory.jsonl")
}

pub fn uc_config_path() -> PathBuf {
    dirs_home().join(".memoryport").join("uc.toml")
}

pub fn council_files() -> Vec<(&'static str, PathBuf)> {
    let base = memoryport_dir();
    vec![
        ("plan", base.join("council-plans.jsonl")),
        ("trace", base.join("council-traces.jsonl")),
        ("learning", base.join("council-learnings.jsonl")),
    ]
}

/// Timeout in milliseconds before falling back from `uc` to local JSONL.
/// Override with LAYERS_UC_TIMEOUT_MS.
pub fn uc_timeout_ms() -> u64 {
    std::env::var("LAYERS_UC_TIMEOUT_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(500)
}

/// Minimum results from `uc` to consider the retrieval successful.
/// If fewer are returned, local JSONL gets boosted.
/// Override with LAYERS_UC_MIN_RESULTS.
pub fn uc_min_results() -> usize {
    std::env::var("LAYERS_UC_MIN_RESULTS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1)
}

/// Current schema version for ContextPayload.
pub const CONTEXT_PAYLOAD_SCHEMA_VERSION: u32 = 1;

fn dirs_home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string()))
}
