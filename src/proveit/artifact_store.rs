use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::SecondsFormat;

use super::types::{FeatureVerdict, ProofRecord};

pub struct ArtifactStore {
    workspace_root: PathBuf,
}

impl ArtifactStore {
    pub fn new(workspace_root: &Path) -> Self {
        Self {
            workspace_root: workspace_root.to_path_buf(),
        }
    }

    pub fn write_proof(&self, record: &ProofRecord) -> Result<PathBuf> {
        let directory = self
            .workspace_root
            .join(".proveit")
            .join("artifacts")
            .join(&record.feature_id)
            .join(&record.proof_id);
        fs::create_dir_all(&directory)
            .with_context(|| format!("failed to create {}", directory.display()))?;

        let timestamp = record
            .timestamp
            .to_rfc3339_opts(SecondsFormat::Secs, true)
            .replace(':', "-");
        let short_sha: String = record.commit_sha.chars().take(12).collect();
        let path = directory.join(format!("{timestamp}_{short_sha}.json"));
        let payload = serde_json::to_string_pretty(record)?;
        fs::write(&path, payload).with_context(|| format!("failed to write {}", path.display()))?;
        Ok(path)
    }

    pub fn latest_proof(&self, feature_id: &str, proof_id: &str) -> Result<Option<ProofRecord>> {
        let directory = self
            .workspace_root
            .join(".proveit")
            .join("artifacts")
            .join(feature_id)
            .join(proof_id);
        if !directory.exists() {
            return Ok(None);
        }

        let mut candidates = Vec::new();
        for entry in fs::read_dir(&directory)
            .with_context(|| format!("failed to read {}", directory.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
                candidates.push(path);
            }
        }
        candidates.sort();
        candidates.reverse();

        if let Some(path) = candidates.into_iter().next() {
            let raw = fs::read_to_string(&path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            let record = serde_json::from_str::<ProofRecord>(&raw)
                .with_context(|| format!("failed to parse {}", path.display()))?;
            return Ok(Some(record));
        }

        Ok(None)
    }

    pub fn write_report(&self, feature_id: &str, verdict: &FeatureVerdict) -> Result<PathBuf> {
        let directory = self.workspace_root.join(".proveit").join("verdicts");
        fs::create_dir_all(&directory)
            .with_context(|| format!("failed to create {}", directory.display()))?;
        let path = directory.join(format!("{feature_id}.json"));
        let payload = serde_json::to_string_pretty(verdict)?;
        fs::write(&path, payload).with_context(|| format!("failed to write {}", path.display()))?;
        Ok(path)
    }
}
