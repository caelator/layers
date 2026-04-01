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

pub fn find_git_root(start: &Path) -> Option<PathBuf> {
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

pub fn audit_path() -> PathBuf {
    memoryport_dir().join("layers-audit.jsonl")
}

pub fn curated_memory_path() -> PathBuf {
    memoryport_dir().join("curated-memory.jsonl")
}

pub fn canonical_curated_memory_path() -> PathBuf {
    curated_memory_path()
}

pub fn legacy_project_records_path() -> PathBuf {
    memoryport_dir().join("project-records.jsonl")
}

pub fn project_records_path() -> PathBuf {
    canonical_curated_memory_path()
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

pub fn dirs_home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/Users/bri".to_string()))
}
