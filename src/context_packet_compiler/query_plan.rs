//! Broad-query planning for safe context packet compilation.
//!
//! The query surface should not scatter safety checks through CLI plumbing. This
//! module converts a broad natural-language query into an explicit context plan:
//! intent, target candidates, and the policy a packet compiler should enforce.

use std::cmp::Reverse;
use std::path::{Path, PathBuf};

use crate::router;

const MAX_DISCOVERED_TARGETS: usize = 3;
const MAX_DISCOVERY_BYTES: u64 = 256 * 1024;
const MAX_DISCOVERY_FILES: usize = 2_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryIntent {
    CodeHeavy,
    Historical,
    Orientation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryInjectionPolicy {
    UseGroundedTargets,
    AllowMemoryOnly,
    NeedsTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredTarget {
    pub path: PathBuf,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BroadQueryPlan {
    pub intent: QueryIntent,
    pub explicit_targets: Vec<PathBuf>,
    pub discovered_targets: Vec<DiscoveredTarget>,
    pub injection_policy: QueryInjectionPolicy,
    pub suggested_command: String,
}

impl BroadQueryPlan {
    #[must_use]
    pub fn new(task: &str, scores: &router::Scores, workspace: &Path) -> Self {
        let explicit_targets = extract_path_like_targets(task, workspace);
        let historical = looks_historical(task, scores);
        let code_heavy = looks_code_heavy(task, scores) || !explicit_targets.is_empty();
        let intent = if historical && explicit_targets.is_empty() {
            QueryIntent::Historical
        } else if code_heavy {
            QueryIntent::CodeHeavy
        } else {
            QueryIntent::Orientation
        };
        let discovered_targets = if explicit_targets.is_empty() && intent == QueryIntent::CodeHeavy
        {
            discover_targets(task, workspace)
        } else {
            Vec::new()
        };
        let injection_policy = match intent {
            QueryIntent::CodeHeavy
                if explicit_targets.is_empty() && discovered_targets.is_empty() =>
            {
                QueryInjectionPolicy::NeedsTarget
            }
            QueryIntent::CodeHeavy => QueryInjectionPolicy::UseGroundedTargets,
            QueryIntent::Historical | QueryIntent::Orientation => {
                QueryInjectionPolicy::AllowMemoryOnly
            }
        };
        let suggested_command = suggested_command(task, &explicit_targets, &discovered_targets);
        Self {
            intent,
            explicit_targets,
            discovered_targets,
            injection_policy,
            suggested_command,
        }
    }

    #[must_use]
    pub fn all_targets(&self) -> Vec<PathBuf> {
        self.explicit_targets
            .iter()
            .cloned()
            .chain(
                self.discovered_targets
                    .iter()
                    .map(|target| target.path.clone()),
            )
            .collect()
    }
}

#[must_use]
pub fn extract_path_like_targets(task: &str, workspace: &Path) -> Vec<PathBuf> {
    let mut targets = Vec::new();
    for token in task.split_whitespace().filter_map(clean_token) {
        if looks_like_target(token) {
            let raw_target = PathBuf::from(token);
            if let Some(target) = grounded_workspace_target(workspace, &raw_target) {
                if !targets.iter().any(|existing| existing == &target) {
                    targets.push(target);
                }
            }
        }
    }
    targets
}

fn grounded_workspace_target(workspace: &Path, target: &Path) -> Option<PathBuf> {
    let workspace = workspace.canonicalize().ok()?;
    let candidate = if target.is_absolute() {
        target.to_path_buf()
    } else {
        workspace.join(target)
    };
    let canonical = candidate.canonicalize().ok()?;
    if !canonical.starts_with(&workspace) {
        return None;
    }
    canonical
        .strip_prefix(&workspace)
        .ok()
        .map(Path::to_path_buf)
}

#[must_use]
pub fn looks_code_heavy(task: &str, scores: &router::Scores) -> bool {
    scores.structural > 0
        || scores.local > 0
        || task.split_whitespace().filter_map(clean_token).any(|word| {
            looks_like_target(word)
                || matches!(
                    word.to_ascii_lowercase().as_str(),
                    "fix"
                        | "bug"
                        | "bugfix"
                        | "implement"
                        | "refactor"
                        | "debug"
                        | "test"
                        | "tests"
                        | "parser"
                        | "function"
                        | "module"
                        | "crate"
                        | "compile"
                        | "clippy"
                )
        })
}

fn looks_historical(task: &str, scores: &router::Scores) -> bool {
    scores.historical > 0
        || task.split_whitespace().filter_map(clean_token).any(|word| {
            matches!(
                word.to_ascii_lowercase().as_str(),
                "prior"
                    | "previous"
                    | "decided"
                    | "decision"
                    | "rationale"
                    | "memory"
                    | "recall"
                    | "history"
            )
        })
}

fn clean_token(token: &str) -> Option<&str> {
    let cleaned = token.trim_matches(|ch: char| {
        matches!(
            ch,
            ',' | '.' | ':' | ';' | ')' | '(' | '`' | '"' | '\'' | '[' | ']' | '{' | '}'
        )
    });
    (!cleaned.is_empty()).then_some(cleaned)
}

fn looks_like_target(value: &str) -> bool {
    value.contains('/') || has_supported_extension(value)
}

fn has_supported_extension(value: &str) -> bool {
    Path::new(value).extension().is_some_and(|ext| {
        ext.to_str()
            .map(str::to_ascii_lowercase)
            .is_some_and(|ext| {
                matches!(
                    ext.as_str(),
                    "rs" | "toml"
                        | "md"
                        | "json"
                        | "yaml"
                        | "yml"
                        | "lock"
                        | "ts"
                        | "tsx"
                        | "js"
                        | "jsx"
                        | "py"
                        | "go"
                        | "java"
                        | "kt"
                        | "swift"
                        | "c"
                        | "cc"
                        | "cpp"
                        | "h"
                        | "hpp"
                        | "rb"
                        | "sh"
                        | "sql"
                        | "proto"
                )
            })
    })
}

fn has_discoverable_extension(value: &str) -> bool {
    Path::new(value).extension().is_some_and(|ext| {
        ext.to_str()
            .map(str::to_ascii_lowercase)
            .is_some_and(|ext| {
                matches!(
                    ext.as_str(),
                    "rs" | "toml"
                        | "json"
                        | "yaml"
                        | "yml"
                        | "ts"
                        | "tsx"
                        | "js"
                        | "jsx"
                        | "py"
                        | "go"
                        | "java"
                        | "kt"
                        | "swift"
                        | "c"
                        | "cc"
                        | "cpp"
                        | "h"
                        | "hpp"
                        | "rb"
                        | "sh"
                        | "sql"
                        | "proto"
                )
            })
    })
}

fn discover_targets(task: &str, workspace: &Path) -> Vec<DiscoveredTarget> {
    let keywords = keywords(task);
    if keywords.is_empty() {
        return Vec::new();
    }
    let mut candidates = Vec::new();
    collect_candidates(workspace, workspace, &keywords, &mut candidates, &mut 0);
    candidates.sort_by_key(|candidate| (Reverse(candidate.score), candidate.path.clone()));
    candidates
        .into_iter()
        .take(MAX_DISCOVERED_TARGETS)
        .map(|candidate| DiscoveredTarget {
            path: candidate.path,
            reason: format!(
                "query terms matched: {}",
                candidate.matched_keywords.join(", ")
            ),
        })
        .collect()
}

#[derive(Debug)]
struct CandidateTarget {
    path: PathBuf,
    score: usize,
    matched_keywords: Vec<String>,
}

fn collect_candidates(
    workspace: &Path,
    dir: &Path,
    keywords: &[String],
    candidates: &mut Vec<CandidateTarget>,
    visited_files: &mut usize,
) {
    if *visited_files >= MAX_DISCOVERY_FILES {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if matches!(
                name.as_str(),
                ".git" | "target" | "memoryport" | "node_modules"
            ) {
                continue;
            }
            collect_candidates(workspace, &path, keywords, candidates, visited_files);
        } else if path.is_file() && has_discoverable_extension(&name) {
            *visited_files += 1;
            if *visited_files > MAX_DISCOVERY_FILES {
                return;
            }
            if let Some(candidate) = score_candidate(workspace, &path, keywords) {
                candidates.push(candidate);
            }
        }
    }
}

fn score_candidate(workspace: &Path, path: &Path, keywords: &[String]) -> Option<CandidateTarget> {
    let metadata = std::fs::metadata(path).ok()?;
    if metadata.len() > MAX_DISCOVERY_BYTES {
        return None;
    }
    let rel_path = path.strip_prefix(workspace).ok()?.to_path_buf();
    let rel_text = rel_path.to_string_lossy().to_ascii_lowercase();
    let content = std::fs::read_to_string(path)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let matched_keywords = keywords
        .iter()
        .filter(|keyword| rel_text.contains(keyword.as_str()) || content.contains(keyword.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let score = matched_keywords.len()
        + keywords
            .iter()
            .filter(|keyword| rel_text.contains(keyword.as_str()))
            .count();
    (score > 0).then_some(CandidateTarget {
        path: rel_path,
        score,
        matched_keywords,
    })
}

fn keywords(task: &str) -> Vec<String> {
    const STOP_WORDS: &[&str] = &[
        "the",
        "and",
        "for",
        "with",
        "before",
        "after",
        "should",
        "what",
        "know",
        "this",
        "that",
        "fix",
        "debug",
        "implement",
        "refactor",
        "test",
        "tests",
        "bug",
        "bugfix",
    ];
    let mut words = Vec::new();
    for word in task
        .split(|ch: char| !ch.is_alphanumeric() && ch != '_' && ch != '-')
        .map(str::to_ascii_lowercase)
        .filter(|word| word.len() >= 4 && !STOP_WORDS.contains(&word.as_str()))
    {
        if !words.iter().any(|existing| existing == &word) {
            words.push(word);
        }
    }
    words
}

fn suggested_command(
    task: &str,
    explicit_targets: &[PathBuf],
    discovered_targets: &[DiscoveredTarget],
) -> String {
    let targets = explicit_targets
        .iter()
        .cloned()
        .chain(discovered_targets.iter().map(|target| target.path.clone()))
        .map(|path| format!("--target {}", path.display()))
        .collect::<Vec<_>>()
        .join(" ");
    if targets.is_empty() {
        format!("layers preflight --strict --target <file-or-dir> {task:?}")
    } else {
        format!("layers preflight --strict {targets} {task:?}")
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use crate::router;

    use super::*;

    #[test]
    fn code_heavy_query_without_targets_needs_target() {
        let workspace = TempDir::new().unwrap();
        let scores = router::Scores {
            historical: 0,
            structural: 1,
            local: 1,
            action: 1,
        };

        let plan = BroadQueryPlan::new("fix the CLI parser regression", &scores, workspace.path());

        assert_eq!(plan.intent, QueryIntent::CodeHeavy);
        assert!(plan.explicit_targets.is_empty());
        assert!(plan.discovered_targets.is_empty());
        assert_eq!(plan.injection_policy, QueryInjectionPolicy::NeedsTarget);
        assert!(plan.suggested_command.contains("layers preflight"));
    }

    #[test]
    fn historical_query_allows_memory_only_context() {
        let workspace = TempDir::new().unwrap();
        let scores = router::Scores {
            historical: 2,
            structural: 0,
            local: 0,
            action: 0,
        };

        let plan = BroadQueryPlan::new(
            "recall the prior decided rationale from memory",
            &scores,
            workspace.path(),
        );

        assert_eq!(plan.intent, QueryIntent::Historical);
        assert_eq!(plan.injection_policy, QueryInjectionPolicy::AllowMemoryOnly);
    }

    #[test]
    fn path_extraction_handles_existing_config_and_punctuation() {
        let workspace = TempDir::new().unwrap();
        fs::create_dir_all(workspace.path().join("config")).unwrap();
        fs::create_dir_all(workspace.path().join("crates/layers-core/src")).unwrap();
        fs::write(workspace.path().join("config/app.yaml"), "app: true").unwrap();
        fs::write(
            workspace.path().join("crates/layers-core/src/lib.rs"),
            "pub fn layers_core() {}",
        )
        .unwrap();
        fs::write(workspace.path().join("package.json"), "{}").unwrap();
        let scores = router::Scores {
            historical: 0,
            structural: 1,
            local: 1,
            action: 1,
        };

        let plan = BroadQueryPlan::new(
            "debug `config/app.yaml`, crates/layers-core/src/lib.rs and package.json.",
            &scores,
            workspace.path(),
        );
        let targets = plan
            .all_targets()
            .into_iter()
            .map(|target| target.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            targets,
            vec![
                "config/app.yaml".to_string(),
                "crates/layers-core/src/lib.rs".to_string(),
                "package.json".to_string(),
            ]
        );
        assert_eq!(
            plan.injection_policy,
            QueryInjectionPolicy::UseGroundedTargets
        );
    }

    #[test]
    fn explicit_absolute_target_is_rejected() {
        let workspace = TempDir::new().unwrap();
        let scores = router::Scores {
            historical: 0,
            structural: 1,
            local: 1,
            action: 1,
        };

        let plan = BroadQueryPlan::new("debug /etc/passwd", &scores, workspace.path());

        assert!(plan.explicit_targets.is_empty());
        assert_eq!(plan.injection_policy, QueryInjectionPolicy::NeedsTarget);
    }

    #[test]
    fn explicit_parent_traversal_target_is_rejected() {
        let workspace = TempDir::new().unwrap();
        let outside = workspace.path().parent().unwrap().join("outside-secret.rs");
        fs::write(&outside, "secret").unwrap();
        let scores = router::Scores {
            historical: 0,
            structural: 1,
            local: 1,
            action: 1,
        };

        let plan = BroadQueryPlan::new("debug ../outside-secret.rs", &scores, workspace.path());

        assert!(plan.explicit_targets.is_empty());
        assert_eq!(plan.injection_policy, QueryInjectionPolicy::NeedsTarget);
    }

    #[test]
    fn nonexistent_explicit_target_is_not_grounded() {
        let workspace = TempDir::new().unwrap();
        let scores = router::Scores {
            historical: 0,
            structural: 1,
            local: 1,
            action: 1,
        };

        let plan = BroadQueryPlan::new("fix src/does_not_exist.rs", &scores, workspace.path());

        assert!(plan.explicit_targets.is_empty());
        assert_eq!(plan.injection_policy, QueryInjectionPolicy::NeedsTarget);
    }

    #[test]
    fn historical_rationale_about_code_allows_memory_only() {
        let workspace = TempDir::new().unwrap();
        let scores = router::Scores {
            historical: 2,
            structural: 0,
            local: 0,
            action: 0,
        };

        let plan = BroadQueryPlan::new(
            "recall why we decided to refactor the parser",
            &scores,
            workspace.path(),
        );

        assert_eq!(plan.intent, QueryIntent::Historical);
        assert_eq!(plan.injection_policy, QueryInjectionPolicy::AllowMemoryOnly);
    }

    #[test]
    fn discovers_likely_repo_targets_from_query_terms() {
        let workspace = TempDir::new().unwrap();
        fs::create_dir_all(workspace.path().join("src/cmd")).unwrap();
        fs::write(
            workspace.path().join("src/cmd/query.rs"),
            "fn query_parser_regression() {}",
        )
        .unwrap();
        fs::write(workspace.path().join("README.md"), "query docs").unwrap();
        let scores = router::Scores {
            historical: 0,
            structural: 1,
            local: 1,
            action: 1,
        };

        let plan = BroadQueryPlan::new("fix query parser regression", &scores, workspace.path());

        assert_eq!(plan.intent, QueryIntent::CodeHeavy);
        assert_eq!(
            plan.injection_policy,
            QueryInjectionPolicy::UseGroundedTargets
        );
        assert_eq!(plan.discovered_targets.len(), 1);
        assert_eq!(
            plan.discovered_targets[0].path,
            std::path::PathBuf::from("src/cmd/query.rs")
        );
        assert!(plan.discovered_targets[0].reason.contains("query"));
    }
}
