//! Benchmark/workflow task specifications used for packet grading.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Durable local description of a workflow task.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskSpec {
    pub task_id: String,
    pub title: String,
    pub prompt: String,
    pub category: TaskCategory,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_root: Option<PathBuf>,
    #[serde(default)]
    pub target_files: Vec<PathBuf>,
    #[serde(default)]
    pub target_symbols: Vec<String>,
    #[serde(default)]
    pub expected_relevant_files: Vec<PathBuf>,
    #[serde(default)]
    pub expected_validation_commands: Vec<String>,
    #[serde(default)]
    pub negative_control: bool,
    #[serde(default)]
    pub success_rubric: SuccessRubric,
}

/// Coarse task class for routing, grading, and benchmark grouping.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskCategory {
    Orientation,
    Bugfix,
    Feature,
    Refactor,
    Debugging,
    Planning,
    Continuity,
    DirtyRepo,
    ContextOverload,
    NegativeControl,
    Other,
}

impl TaskCategory {
    #[must_use]
    pub const fn is_code_heavy(self) -> bool {
        matches!(
            self,
            Self::Bugfix | Self::Feature | Self::Refactor | Self::Debugging | Self::DirtyRepo
        )
    }
}

/// Human/evaluator rubric for deciding task success.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SuccessRubric {
    pub full_success: String,
    #[serde(default)]
    pub partial_success: String,
    #[serde(default)]
    pub failure: String,
    #[serde(default = "default_min_verification_quality")]
    pub min_verification_quality: u8,
}

impl Default for SuccessRubric {
    fn default() -> Self {
        Self {
            full_success: "Task is complete, verified, and reviewable.".to_string(),
            partial_success: "Task is partially complete or needs minor intervention.".to_string(),
            failure: "Task is wrong, incomplete, or unusable.".to_string(),
            min_verification_quality: default_min_verification_quality(),
        }
    }
}

const fn default_min_verification_quality() -> u8 {
    3
}

impl TaskSpec {
    /// Validate semantic constraints that serde shape alone cannot enforce.
    ///
    /// # Errors
    ///
    /// Returns a list of all validation errors when the task spec is unusable.
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        require_non_empty(&mut errors, "task_id", &self.task_id);
        require_non_empty(&mut errors, "title", &self.title);

        if !self.negative_control {
            require_non_empty(&mut errors, "prompt", &self.prompt);
        }

        if self.success_rubric.min_verification_quality > 5 {
            errors.push(
                "success_rubric.min_verification_quality must be between 0 and 5".to_string(),
            );
        }
        if self.success_rubric.full_success.trim().is_empty() {
            errors.push("success_rubric.full_success must not be empty".to_string());
        }

        if self.category.is_code_heavy()
            && !self.negative_control
            && self.target_files.is_empty()
            && self.expected_relevant_files.is_empty()
            && self.target_symbols.is_empty()
        {
            errors.push(
                "code-heavy task should include target_files, target_symbols, or expected_relevant_files".to_string(),
            );
        }

        if self.negative_control && self.category != TaskCategory::NegativeControl {
            errors.push("negative_control tasks must use category negative_control".to_string());
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

fn require_non_empty(errors: &mut Vec<String>, field: &str, value: &str) {
    if value.trim().is_empty() {
        errors.push(format!("{field} must not be empty"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_minimal_task_spec_deserializes() {
        let task: TaskSpec = serde_json::from_str(
            r#"{
              "task_id":"code-orientation-1",
              "title":"Inspect benchmark CLI wiring",
              "prompt":"Inspect the workflow-benchmark command wiring.",
              "category":"orientation",
              "expected_relevant_files":["src/main.rs"],
              "expected_validation_commands":["cargo test -q workflow_benchmark"]
            }"#,
        )
        .expect("task should deserialize");

        task.validate().expect("task should validate");
        assert_eq!(task.success_rubric.min_verification_quality, 3);
    }

    #[test]
    fn invalid_rubric_bounds_fail_validation() {
        let task = TaskSpec {
            task_id: "bad".to_string(),
            title: "Bad".to_string(),
            prompt: "Do work".to_string(),
            category: TaskCategory::Planning,
            repo_root: None,
            target_files: Vec::new(),
            target_symbols: Vec::new(),
            expected_relevant_files: Vec::new(),
            expected_validation_commands: Vec::new(),
            negative_control: false,
            success_rubric: SuccessRubric {
                min_verification_quality: 6,
                ..SuccessRubric::default()
            },
        };

        let errors = task.validate().expect_err("rubric should be invalid");
        assert!(
            errors
                .iter()
                .any(|error| error.contains("min_verification_quality"))
        );
    }

    #[test]
    fn negative_control_can_omit_expected_relevant_files() {
        let task = TaskSpec {
            task_id: "negative-1".to_string(),
            title: "No repo context needed".to_string(),
            prompt: String::new(),
            category: TaskCategory::NegativeControl,
            repo_root: None,
            target_files: Vec::new(),
            target_symbols: Vec::new(),
            expected_relevant_files: Vec::new(),
            expected_validation_commands: Vec::new(),
            negative_control: true,
            success_rubric: SuccessRubric::default(),
        };

        task.validate().expect("negative control should validate");
    }

    #[test]
    fn non_negative_control_coding_task_with_empty_prompt_fails() {
        let task = TaskSpec {
            task_id: "feature-1".to_string(),
            title: "Feature".to_string(),
            prompt: " ".to_string(),
            category: TaskCategory::Feature,
            repo_root: None,
            target_files: vec![PathBuf::from("src/main.rs")],
            target_symbols: Vec::new(),
            expected_relevant_files: Vec::new(),
            expected_validation_commands: Vec::new(),
            negative_control: false,
            success_rubric: SuccessRubric::default(),
        };

        let errors = task.validate().expect_err("empty prompt should fail");
        assert!(errors.iter().any(|error| error.contains("prompt")));
    }

    #[test]
    fn benchmark_task_examples_validate() {
        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let task_dir = workspace_root.join("benchmarks/workflows/tasks");
        let entries = std::fs::read_dir(&task_dir).expect("benchmark task example dir exists");
        let mut validated = 0;

        for entry in entries {
            let path = entry.expect("read task entry").path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let content = std::fs::read_to_string(&path).expect("read task example");
            let task: TaskSpec = serde_json::from_str(&content).expect("task example deserializes");
            task.validate().expect("task example validates");
            validated += 1;
        }

        assert!(
            validated >= 2,
            "expected at least two benchmark task examples"
        );
    }
}
