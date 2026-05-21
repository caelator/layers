use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Result;
use clap::Args;
use serde::Serialize;

use crate::config::workspace_root;

/// Arguments for `layers impact`.
#[derive(Debug, Args)]
pub struct ImpactArgs {
    /// File path, symbol, or task target to analyze.
    pub target: String,
    /// Output structured JSON.
    #[arg(long)]
    pub json: bool,
    /// Include tests in `GitNexus` impact results.
    #[arg(long)]
    pub include_tests: bool,
    /// Relationship depth for `GitNexus` impact.
    #[arg(long, default_value_t = 2)]
    pub depth: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ImpactReport {
    pub target: String,
    pub workspace: PathBuf,
    pub source: ImpactSource,
    pub status: ImpactStatus,
    pub summary: String,
    pub affected_files: Vec<String>,
    pub validation_commands: Vec<String>,
    pub warnings: Vec<String>,
    pub raw_gitnexus: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImpactSource {
    #[serde(rename = "gitnexus")]
    GitNexus,
    Local,
    Degraded,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImpactStatus {
    Ok,
    Degraded,
}

pub fn handle_impact(args: &ImpactArgs) -> Result<()> {
    let report = build_impact_report(
        &workspace_root(),
        &args.target,
        args.include_tests,
        args.depth,
    );
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_human_report(&report);
    }
    Ok(())
}

#[must_use]
pub fn build_impact_report(
    workspace: &Path,
    target: &str,
    include_tests: bool,
    depth: usize,
) -> ImpactReport {
    match gitnexus_impact(workspace, target, include_tests, depth) {
        Ok(raw) if !raw.trim().is_empty() => {
            let affected_files = extract_file_mentions(&raw);
            let validation_commands = validation_commands_for(target, include_tests);
            ImpactReport {
                target: target.to_string(),
                workspace: workspace.to_path_buf(),
                source: ImpactSource::GitNexus,
                status: ImpactStatus::Ok,
                summary: first_non_empty_line(&raw)
                    .unwrap_or_else(|| format!("GitNexus impact analysis for {target}")),
                affected_files,
                validation_commands,
                warnings: Vec::new(),
                raw_gitnexus: Some(raw),
            }
        }
        Ok(_) => degraded_report(
            workspace,
            target,
            include_tests,
            "GitNexus returned no impact output.",
        ),
        Err(message) => degraded_report(workspace, target, include_tests, &message),
    }
}

fn gitnexus_impact(
    workspace: &Path,
    target: &str,
    include_tests: bool,
    depth: usize,
) -> std::result::Result<String, String> {
    let mut command = Command::new("gitnexus");
    command
        .arg("impact")
        .arg(target)
        .arg("--direction")
        .arg("upstream")
        .arg("--depth")
        .arg(depth.to_string())
        .current_dir(workspace);
    if include_tests {
        command.arg("--include-tests");
    }
    let output = command
        .output()
        .map_err(|err| format!("GitNexus unavailable: {err}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if stderr.is_empty() { stdout } else { stderr };
        return Err(if detail.is_empty() {
            format!("GitNexus impact exited with status {}", output.status)
        } else {
            format!("GitNexus impact failed: {detail}")
        });
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if gitnexus_output_is_error(&stdout) {
        return Err(format!(
            "GitNexus impact returned no usable target: {stdout}"
        ));
    }
    Ok(stdout)
}

fn gitnexus_output_is_error(stdout: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(stdout) else {
        return false;
    };
    value.get("error").is_some()
        || value
            .get("risk")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|risk| risk.eq_ignore_ascii_case("unknown"))
}

fn degraded_report(
    workspace: &Path,
    target: &str,
    include_tests: bool,
    warning: &str,
) -> ImpactReport {
    let target_path = workspace.join(target);
    let affected_files = target_path
        .exists()
        .then(|| target.to_string())
        .into_iter()
        .collect();
    ImpactReport {
        target: target.to_string(),
        workspace: workspace.to_path_buf(),
        source: if target_path.exists() {
            ImpactSource::Local
        } else {
            ImpactSource::Degraded
        },
        status: ImpactStatus::Degraded,
        summary: format!("Degraded local impact summary for {target}"),
        affected_files,
        validation_commands: validation_commands_for(target, include_tests),
        warnings: vec![warning.to_string()],
        raw_gitnexus: None,
    }
}

fn validation_commands_for(target: &str, include_tests: bool) -> Vec<String> {
    let mut commands = vec!["git diff --check".to_string()];
    let target_path = Path::new(target);
    if target_path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
        || target.contains("src/")
        || target.contains("crates/")
    {
        commands.push("cargo test --workspace --all-targets".to_string());
        commands.push("cargo clippy --workspace --all-targets -- -D warnings".to_string());
    }
    if include_tests {
        commands.push("cargo test --workspace --all-targets".to_string());
    }
    commands.sort();
    commands.dedup();
    commands
}

fn first_non_empty_line(raw: &str) -> Option<String> {
    raw.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

fn extract_file_mentions(raw: &str) -> Vec<String> {
    let mut files = raw
        .split(|ch: char| ch.is_whitespace() || ch == '`' || ch == '|' || ch == ',' || ch == ':')
        .filter(|token| token.contains('/'))
        .filter(|token| {
            let Some(extension) = Path::new(token).extension().and_then(|ext| ext.to_str()) else {
                return false;
            };
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "rs" | "md" | "toml" | "json" | "yaml" | "yml"
            )
        })
        .map(|token| token.trim_matches(|ch: char| ch == '.' || ch == ')' || ch == '('))
        .map(str::to_string)
        .collect::<Vec<_>>();
    files.sort();
    files.dedup();
    files.truncate(20);
    files
}

fn print_human_report(report: &ImpactReport) {
    println!("Impact target: {}", report.target);
    println!("Status: {:?}", report.status);
    println!("Source: {:?}", report.source);
    println!("Summary: {}", report.summary);
    if !report.warnings.is_empty() {
        println!("Warnings:");
        for warning in &report.warnings {
            println!("- {warning}");
        }
    }
    if !report.affected_files.is_empty() {
        println!("Affected files:");
        for file in &report.affected_files {
            println!("- {file}");
        }
    }
    println!("Validation commands:");
    for command in &report.validation_commands {
        println!("- {command}");
    }
    if let Some(raw) = &report.raw_gitnexus {
        println!("\nGitNexus output:\n{raw}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn degraded_impact_for_existing_file_includes_validation() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).expect("src dir");
        std::fs::write(src.join("lib.rs"), "pub fn demo() {}\n").expect("fixture");

        let report = degraded_report(tmp.path(), "src/lib.rs", false, "gitnexus unavailable");

        assert_eq!(report.status, ImpactStatus::Degraded);
        assert_eq!(report.source, ImpactSource::Local);
        assert!(report.affected_files.contains(&"src/lib.rs".to_string()));
        assert!(
            report
                .validation_commands
                .iter()
                .any(|cmd| cmd.contains("cargo test"))
        );
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("gitnexus unavailable"))
        );
    }

    #[test]
    fn extracts_file_mentions_from_gitnexus_text() {
        let files = extract_file_mentions(
            "| caller | src/cmd/preflight.rs |\n`crates/layers-core/src/context_packet.rs` more",
        );
        assert!(files.contains(&"src/cmd/preflight.rs".to_string()));
        assert!(files.contains(&"crates/layers-core/src/context_packet.rs".to_string()));
    }

    #[test]
    fn gitnexus_json_error_is_degraded_signal() {
        let raw = r#"{"error":"Target 'src/cmd/preflight.rs' not found","risk":"UNKNOWN"}"#;
        assert!(gitnexus_output_is_error(raw));
    }

    #[test]
    fn gitnexus_source_serializes_without_underscore() {
        assert_eq!(
            serde_json::to_value(&ImpactSource::GitNexus).expect("serialize"),
            serde_json::json!("gitnexus")
        );
    }
}
